//! Sticker / custom-emoji packs hosted by the relay.
//!
//! The server operator puts a Telegram bot token in the admin panel once
//! (or `AQUA_TG_BOT_TOKEN`); after that any user just pastes a
//! `t.me/addstickers/…` / `t.me/addemoji/…` link and the *server* downloads
//! the pack, stores it once under `aqua-packs/<name>/` and serves it to
//! everyone. Custom-emoji reactions from such packs therefore render for
//! people who never imported the pack themselves (fetched lazily, cached).
//!
//! On start the server imports `AQUA_DEFAULT_PACKS` (default: the
//! `eltaller47` reaction pack) in the background, if a token is set.

use std::path::{Path, PathBuf};

use axum::{
    body::Bytes,
    extract::{Path as UrlPath, State},
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};

use crate::{admin_authed, AppState, Caller};

const PACKS_DIR: &str = "aqua-packs";
const TOKEN_FILE: &str = "aqua-telegram.json";
const MAX_STICKERS: usize = 200;
const MAX_FILE: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackFile {
    pub file: String,
    pub emoji: String,
    pub mime: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    /// Telegram short name — the id used in URLs and reactions.
    pub name: String,
    pub title: String,
    /// "regular" | "custom_emoji" | "mask"
    #[serde(rename = "type")]
    pub kind: String,
    pub files: Vec<PackFile>,
}

#[derive(Default, Serialize, Deserialize)]
struct TokenFile {
    #[serde(default)]
    token: String,
}

/// Bot token: env var wins, else the one saved from the admin panel.
pub fn bot_token() -> Option<String> {
    if let Ok(t) = std::env::var("AQUA_TG_BOT_TOKEN") {
        if !t.trim().is_empty() {
            return Some(t.trim().to_string());
        }
    }
    let f: TokenFile = std::fs::read(TOKEN_FILE).ok().and_then(|b| serde_json::from_slice(&b).ok())?;
    (!f.token.trim().is_empty()).then(|| f.token.trim().to_string())
}

fn token_source() -> &'static str {
    if std::env::var("AQUA_TG_BOT_TOKEN").map(|t| !t.trim().is_empty()).unwrap_or(false) {
        "env"
    } else if bot_token().is_some() {
        "file"
    } else {
        "none"
    }
}

/// `t.me/addstickers/Name`, `https://t.me/addemoji/Name`, `@Name`, `Name` → `Name`.
pub fn short_name(r: &str) -> String {
    let r = r.trim().trim_end_matches('/');
    let last = r.rsplit('/').next().unwrap_or(r);
    let last = last.split(['?', '#']).next().unwrap_or(last).trim_start_matches('@');
    last.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').take(64).collect()
}

fn pack_dir(name: &str) -> PathBuf {
    Path::new(PACKS_DIR).join(name)
}

fn mime_of(file: &str) -> &'static str {
    match file.rsplit('.').next().unwrap_or("") {
        "webp" => "image/webp",
        "png" => "image/png",
        "tgs" => "application/x-tgsticker",
        "webm" => "video/webm",
        "gif" => "image/gif",
        _ => "application/octet-stream",
    }
}

pub fn load_manifest(name: &str) -> Option<Manifest> {
    let b = std::fs::read(pack_dir(name).join("manifest.json")).ok()?;
    serde_json::from_slice(&b).ok()
}

pub fn list_manifests() -> Vec<Manifest> {
    let Ok(rd) = std::fs::read_dir(PACKS_DIR) else { return Vec::new() };
    let mut v: Vec<Manifest> = rd.flatten().filter_map(|e| load_manifest(e.file_name().to_str()?)).collect();
    v.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    v
}

fn urlenc(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

/// Download a pack from Telegram (or return the stored one).
pub async fn import(pack_ref: &str) -> Result<Manifest, String> {
    let name = short_name(pack_ref);
    if name.is_empty() {
        return Err("empty pack link".into());
    }
    if let Some(m) = load_manifest(&name) {
        return Ok(m);
    }
    let token = bot_token().ok_or("this server has no Telegram bot token — ask its admin to add one in the admin panel")?;
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(40))
        .build()
        .map_err(|e| e.to_string())?;
    // AQUA_TG_API: another Bot API endpoint (a local bot-api server, tests).
    let base = std::env::var("AQUA_TG_API").unwrap_or_else(|_| "https://api.telegram.org".into());
    let base = base.trim_end_matches('/').to_string();
    let api = |m: &str, q: String| format!("{base}/bot{token}/{m}?{q}");
    let set: serde_json::Value = http
        .get(api("getStickerSet", format!("name={}", urlenc(&name))))
        .send().await.map_err(|e| format!("Telegram: {e}"))?
        .json().await.map_err(|e| format!("Telegram: {e}"))?;
    if !set["ok"].as_bool().unwrap_or(false) {
        return Err(format!("Telegram: {}", set["description"].as_str().unwrap_or("error")));
    }
    let r = &set["result"];
    let real = r["name"].as_str().map(short_name).unwrap_or_else(|| name.clone());
    let title = r["title"].as_str().unwrap_or(&real).to_string();
    let kind = r["sticker_type"].as_str().unwrap_or("regular").to_string();
    let dir = pack_dir(&real);
    let tmp = Path::new(PACKS_DIR).join(format!(".{real}.part"));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let mut files = Vec::new();
    for (i, st) in r["stickers"].as_array().cloned().unwrap_or_default().iter().take(MAX_STICKERS).enumerate() {
        let Some(fid) = st["file_id"].as_str() else { continue };
        let meta: serde_json::Value = match http.get(api("getFile", format!("file_id={}", urlenc(fid)))).send().await {
            Ok(x) => x.json().await.unwrap_or_default(),
            Err(_) => continue,
        };
        let Some(fp) = meta["result"]["file_path"].as_str() else { continue };
        let ext = match fp.rsplit('.').next().unwrap_or("") {
            e @ ("webp" | "png" | "tgs" | "webm" | "gif") => e.to_string(),
            _ if st["is_animated"].as_bool() == Some(true) => "tgs".into(),
            _ if st["is_video"].as_bool() == Some(true) => "webm".into(),
            _ => "webp".into(),
        };
        let bytes = match http.get(format!("{base}/file/bot{token}/{fp}")).send().await {
            Ok(x) => x.bytes().await.map(|b| b.to_vec()).unwrap_or_default(),
            Err(_) => continue,
        };
        if bytes.is_empty() || bytes.len() > MAX_FILE {
            continue;
        }
        let emoji = st["emoji"].as_str().unwrap_or("").trim().to_string();
        // Same naming as the client importer: <emoji>_<nnn>.<ext>
        let file = format!("{}_{:03}.{ext}", if emoji.is_empty() { "x" } else { &emoji }, i + 1);
        if std::fs::write(tmp.join(&file), &bytes).is_ok() {
            files.push(PackFile { mime: mime_of(&file).into(), file, emoji });
        }
    }
    if files.is_empty() {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(format!("nothing could be downloaded from \"{title}\""));
    }
    let m = Manifest { name: real.clone(), title, kind, files };
    std::fs::write(tmp.join("manifest.json"), serde_json::to_vec(&m).unwrap_or_default()).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::rename(&tmp, &dir).map_err(|e| e.to_string())?;
    tracing::info!(pack = %m.name, files = m.files.len(), "sticker pack imported");
    Ok(m)
}

/// Background import of the default packs at start-up.
pub fn spawn_default_imports() {
    let list = std::env::var("AQUA_DEFAULT_PACKS").unwrap_or_else(|_| "eltaller47".into());
    if bot_token().is_none() {
        return;
    }
    tokio::spawn(async move {
        for p in list.split([',', ' ']).filter(|s| !s.trim().is_empty()) {
            if let Err(e) = import(p).await {
                tracing::warn!(pack = p, error = %e, "default pack import failed");
            }
        }
    });
}

// ---------------------------------------------------------------- HTTP

#[derive(Serialize)]
pub struct PacksIndex {
    /// Users can import by link (the server has a bot token).
    pub telegram: bool,
    pub packs: Vec<PackSummary>,
}
#[derive(Serialize)]
pub struct PackSummary {
    pub name: String,
    pub title: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub count: usize,
}

pub async fn list(State(_): State<AppState>) -> Json<PacksIndex> {
    Json(PacksIndex {
        telegram: bot_token().is_some(),
        packs: list_manifests()
            .into_iter()
            .map(|m| PackSummary { count: m.files.len(), name: m.name, title: m.title, kind: m.kind })
            .collect(),
    })
}

pub async fn manifest(UrlPath(name): UrlPath<String>) -> Result<Json<Manifest>, StatusCode> {
    load_manifest(&short_name(&name)).map(Json).ok_or(StatusCode::NOT_FOUND)
}

pub async fn file(UrlPath((name, file)): UrlPath<(String, String)>) -> impl IntoResponse {
    let name = short_name(&name);
    let Some(m) = load_manifest(&name) else { return Err(StatusCode::NOT_FOUND) };
    // Only files listed in the manifest — no path tricks.
    let Some(f) = m.files.iter().find(|f| f.file == file) else { return Err(StatusCode::NOT_FOUND) };
    let bytes = std::fs::read(pack_dir(&name).join(&f.file)).map_err(|_| StatusCode::NOT_FOUND)?;
    Ok((
        [(header::CONTENT_TYPE, f.mime.clone()), (header::CACHE_CONTROL, "public, max-age=31536000, immutable".into())],
        Bytes::from(bytes),
    ))
}

#[derive(Deserialize)]
pub struct ImportBody {
    pub link: String,
}

/// Any signed-in user may import (the token never leaves the server).
pub async fn import_http(_caller: Caller, Json(b): Json<ImportBody>) -> Result<Json<Manifest>, (StatusCode, String)> {
    import(&b.link).await.map(Json).map_err(|e| (StatusCode::BAD_GATEWAY, e))
}

#[derive(Serialize)]
pub struct TokenView {
    pub source: &'static str,
    pub has_token: bool,
    pub packs: usize,
}
#[derive(Deserialize)]
pub struct TokenUpdate {
    #[serde(default)]
    pub token: String,
}

pub async fn admin_get(State(state): State<AppState>, headers: axum::http::HeaderMap) -> Result<Json<TokenView>, StatusCode> {
    admin_authed(&state, &headers)?;
    Ok(Json(TokenView { source: token_source(), has_token: bot_token().is_some(), packs: list_manifests().len() }))
}

pub async fn admin_set(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(u): Json<TokenUpdate>,
) -> Result<Json<TokenView>, (StatusCode, String)> {
    admin_authed(&state, &headers).map_err(|c| (c, String::new()))?;
    if token_source() == "env" {
        return Err((StatusCode::CONFLICT, "the token comes from AQUA_TG_BOT_TOKEN; change it there".into()));
    }
    let t = u.token.trim().to_string();
    if !t.is_empty() && !t.contains(':') {
        return Err((StatusCode::BAD_REQUEST, "that doesn't look like a bot token (123456:ABC…)".into()));
    }
    std::fs::write(TOKEN_FILE, serde_json::to_vec_pretty(&TokenFile { token: t }).unwrap_or_default())
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    spawn_default_imports();
    Ok(Json(TokenView { source: token_source(), has_token: bot_token().is_some(), packs: list_manifests().len() }))
}

/// First start without a Telegram token: ask for one in the terminal, so
/// sticker / emoji import works for everyone without any user ever
/// seeing a token. Enter skips; AQUA_TG_PROMPT=0 turns the question off.
/// Runs before the server starts (plain blocking stdin).
pub fn prompt_for_token_if_missing() {
    use std::io::{BufRead, IsTerminal, Write};
    if bot_token().is_some()
        || std::env::var("AQUA_TG_PROMPT").map(|v| v == "0" || v == "off").unwrap_or(false)
        || !std::io::stdin().is_terminal()
    {
        return;
    }
    println!();
    println!("  Stickers & emoji from Telegram");
    println!("  ------------------------------");
    println!("  With a Telegram bot token this server downloads sticker and emoji");
    println!("  packs for everyone — users just paste a t.me/addstickers/… link.");
    println!("  Get one from @BotFather in Telegram (/newbot), it looks like 123456:ABC…");
    println!("  It is saved to {TOKEN_FILE} in the folder the server runs from; change it");
    println!("  later in the admin panel. Press Enter to skip.");
    loop {
        print!("  Bot token: ");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        if std::io::stdin().lock().read_line(&mut line).is_err() {
            return;
        }
        let t = line.trim();
        if t.is_empty() {
            println!("  Skipped — stickers can't be imported until a token is set.\n");
            return;
        }
        if !t.contains(':') {
            println!("  That doesn't look like a bot token (123456:ABC…). Try again or press Enter.");
            continue;
        }
        match std::fs::write(TOKEN_FILE, serde_json::to_vec_pretty(&TokenFile { token: t.to_string() }).unwrap_or_default()) {
            Ok(()) => println!("  Saved. Default packs will download in the background.\n"),
            Err(e) => println!("  Could not save {TOKEN_FILE}: {e}\n"),
        }
        return;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names() {
        assert_eq!(short_name("https://t.me/addemoji/eltaller47"), "eltaller47");
        assert_eq!(short_name("t.me/addstickers/Cats_by_x/"), "Cats_by_x");
        assert_eq!(short_name("@Pack?x=1"), "Pack");
        assert_eq!(short_name("../../etc"), "etc");
        assert_eq!(short_name("a b"), "ab");
    }
}
