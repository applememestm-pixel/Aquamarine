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
    /// Telegram `file_unique_id`: a refresh keeps files it already has.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
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
    /// Unix seconds of the last download / check against Telegram.
    #[serde(default)]
    pub updated: u64,
    /// Bumped whenever the set of files or their order changes.
    #[serde(default)]
    pub rev: u64,
    /// Uploaded on this server (Aquamarine Server → Stickers), not from
    /// Telegram: never checked against Telegram.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub local: bool,
    /// 18+ (Telegram's "sensitive content"): set when served, from the
    /// admin's choice or a guess by the title (`Rules::adult_of`). Apps with
    /// the 18+ filter on hide the pack and blur its stickers in chats.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub adult: bool,
    /// What the image detector found in it (`scan`), for revision `rev`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scan: Option<Scan>,
}

/// The detector's look at a pack (aqua-nsfw): how many stickers it could
/// read (static ones — .tgs / .webm are skipped) and how many are explicit.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Scan {
    pub rev: u64,
    pub checked: u32,
    pub explicit: u32,
}

impl Scan {
    /// 18+ by the pictures: two explicit stickers, or one in a pack this small.
    pub fn adult(&self) -> bool {
        self.explicit >= 2 || (self.explicit >= 1 && self.checked <= 4)
    }
}

/// At most this many stickers of a pack are looked at (evenly spread):
/// enough to tell, and a 120-sticker pack costs ~2 s of one core, not 8.
const SCAN_SAMPLE: usize = 32;

/// One scan at a time on the whole relay (it's one core's work): imports
/// and the start-up pass queue up instead of loading a small home server.
fn scan_gate() -> &'static tokio::sync::Semaphore {
    static G: std::sync::OnceLock<tokio::sync::Semaphore> = std::sync::OnceLock::new();
    G.get_or_init(|| tokio::sync::Semaphore::new(1))
}

/// Look at a pack's stickers with the detector, unless this revision was
/// already looked at. Stores the result in its manifest.
pub async fn scan(name: &str) {
    let Some(m) = load_manifest(name) else { return };
    if m.scan.is_some_and(|s| s.rev == m.rev) {
        return;
    }
    let Ok(_permit) = scan_gate().acquire().await else { return };
    let dir = pack_dir(&m.name);
    let files: Vec<String> = m.files.iter().filter(|f| f.mime.starts_with("image/")).map(|f| f.file.clone()).collect();
    let step = files.len().div_ceil(SCAN_SAMPLE).max(1);
    let pick: Vec<std::path::PathBuf> = files.iter().step_by(step).map(|f| dir.join(f)).collect();
    let started = std::time::Instant::now();
    let (checked, explicit) = tokio::task::spawn_blocking(move || {
        let (mut c, mut e) = (0u32, 0u32);
        for p in pick {
            if let Some(s) = aqua_nsfw::classify_file(&p) {
                c += 1;
                e += u32::from(s.explicit());
            }
        }
        (c, e)
    }).await.unwrap_or((0, 0));
    let lock = pack_lock(&m.name.to_ascii_lowercase());
    let _g = lock.lock().await;
    // Changed while we looked: the next pass looks again.
    let Some(mut now) = load_manifest(&m.name).filter(|x| x.rev == m.rev) else { return };
    now.scan = Some(Scan { rev: m.rev, checked, explicit });
    let _ = std::fs::write(dir.join("manifest.json"), serde_json::to_vec(&now).unwrap_or_default());
    tracing::info!(pack = %m.name, checked, explicit, ms = started.elapsed().as_millis() as u64, "sticker pack scanned");
}

/// In the background: [`scan`] for one pack.
pub fn spawn_scan(name: &str) {
    let name = name.to_string();
    tokio::spawn(async move { scan(&name).await });
}

/// At start: every stored pack not looked at yet (or changed since), one
/// after another.
pub fn spawn_scan_all() {
    tokio::spawn(async {
        // Let the relay come up first.
        tokio::time::sleep(std::time::Duration::from_secs(20)).await;
        for m in list_manifests() {
            if !m.scan.is_some_and(|s| s.rev == m.rev) {
                scan(&m.name).await;
            }
        }
    });
}

// ---------- What the admin decided about packs (aqua-pack-rules.json) -------
//
// Kept apart from `aqua-packs/` (which "Clear the server" empties): a pack
// banned here can't be imported again by anyone; stickers removed from a
// pack don't come back with Telegram's next refresh; 18+ marks. Apps get
// the bans and marks with `/v1/packs` and apply them to stickers already
// in chats and libraries — what the admin removes is gone for everyone.

const RULES_FILE: &str = "aqua-pack-rules.json";

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Rules {
    /// Banned packs (short names, lowercase).
    #[serde(default)]
    pub banned: Vec<String>,
    /// Pack (lowercase) → removed stickers: file names and Telegram
    /// `file_unique_id`s.
    #[serde(default)]
    pub removed: std::collections::BTreeMap<String, Vec<String>>,
    /// Pack (lowercase) → 18+ as the admin set it (absent = guessed).
    #[serde(default)]
    pub adult: std::collections::BTreeMap<String, bool>,
}

impl Rules {
    pub fn load() -> Self {
        std::fs::read(RULES_FILE).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }
    fn save(&self) -> std::io::Result<()> {
        let tmp = format!("{RULES_FILE}.part");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self).unwrap_or_default())?;
        std::fs::rename(tmp, RULES_FILE)
    }
    pub fn is_banned(&self, name: &str) -> bool {
        self.banned.iter().any(|b| b.eq_ignore_ascii_case(name))
    }
    fn removed_of(&self, name: &str) -> &[String] {
        self.removed.get(&name.to_ascii_lowercase()).map(Vec::as_slice).unwrap_or(&[])
    }
    /// The admin's word; else the detector's (for this revision) or the title.
    pub fn adult_of(&self, m: &Manifest) -> bool {
        self.adult.get(&m.name.to_ascii_lowercase()).copied().unwrap_or_else(|| {
            m.scan.is_some_and(|s| s.rev == m.rev && s.adult()) || looks_adult(&m.name, &m.title)
        })
    }
}

/// A guess by the name and title (Telegram's Bot API has no 18+ flag);
/// the admin overrides it either way.
pub fn looks_adult(name: &str, title: &str) -> bool {
    let t = format!("{name} {title}").to_lowercase();
    ["18+", "r18", "r-18", "nsfw", "hentai", "yiff", "porn", "xxx", "lewd", "nude", "ecchi", "секс", "порн", "хентай", "эротик"]
        .iter().any(|k| t.contains(k))
}

/// A manifest as people get it: removed stickers out, the 18+ mark on.
fn served(mut m: Manifest, r: &Rules) -> Manifest {
    let gone = r.removed_of(&m.name);
    if !gone.is_empty() {
        m.files.retain(|f| !gone.contains(&f.file) && (f.id.is_empty() || !gone.contains(&f.id)));
    }
    m.adult = r.adult_of(&m);
    m
}

/// A stored pack older than this is checked against Telegram again.
const STALE_SECS: u64 = 6 * 3600;
/// Explicit refreshes of one pack at most this often.
const REFRESH_MIN_SECS: u64 = 60;

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
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
/// One import per pack at a time: a second request for the same pack
/// waits for the first one and then gets its manifest.
fn pack_lock(name: &str) -> std::sync::Arc<tokio::sync::Mutex<()>> {
    static LOCKS: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, std::sync::Arc<tokio::sync::Mutex<()>>>>> =
        std::sync::OnceLock::new();
    let map = LOCKS.get_or_init(Default::default);
    map.lock().unwrap().entry(name.to_string()).or_default().clone()
}

/// Files downloaded in parallel (Telegram is fine with a handful).
const PARALLEL: usize = 16;

/// Download one sticker of a set into `tmp`; None if it can't be fetched.
async fn fetch_sticker(
    http: reqwest::Client,
    base: String,
    token: String,
    tmp: std::path::PathBuf,
    i: usize,
    st: serde_json::Value,
) -> Option<PackFile> {
    let fid = st["file_id"].as_str()?;
    let meta: serde_json::Value = http
        .get(format!("{base}/bot{token}/getFile?file_id={}", urlenc(fid)))
        .send().await.ok()?
        .json().await.ok()?;
    let fp = meta["result"]["file_path"].as_str()?;
    let ext = match fp.rsplit('.').next().unwrap_or("") {
        e @ ("webp" | "png" | "tgs" | "webm" | "gif") => e.to_string(),
        _ if st["is_animated"].as_bool() == Some(true) => "tgs".into(),
        _ if st["is_video"].as_bool() == Some(true) => "webm".into(),
        _ => "webp".into(),
    };
    let bytes = http.get(format!("{base}/file/bot{token}/{fp}")).send().await.ok()?.bytes().await.ok()?;
    if bytes.is_empty() || bytes.len() > MAX_FILE {
        return None;
    }
    let emoji = st["emoji"].as_str().unwrap_or("").trim().to_string();
    let uid = st["file_unique_id"].as_str().unwrap_or("").to_string();
    // <emoji>_<nnn>.<ext> (the client sorts by the number); a refresh
    // names new files after their position too, but never reuses a name.
    let mut file = format!("{}_{:03}.{ext}", if emoji.is_empty() { "x" } else { &emoji }, i + 1);
    if tmp.join(&file).exists() {
        file = format!("{}_{:03}-{}.{ext}", if emoji.is_empty() { "x" } else { &emoji }, i + 1, &uid.chars().filter(|c| c.is_ascii_alphanumeric()).take(10).collect::<String>());
    }
    std::fs::write(tmp.join(&file), &bytes).ok()?;
    Some(PackFile { mime: mime_of(&file).into(), file, emoji, id: uid })
}

const BANNED: &str = "this pack is blocked on this server";

/// Import a pack (or return the stored one; a stale one is re-checked in
/// the background).
pub async fn import(pack_ref: &str) -> Result<Manifest, String> {
    let name = short_name(pack_ref);
    if name.is_empty() {
        return Err("empty pack link".into());
    }
    if Rules::load().is_banned(&name) {
        return Err(BANNED.into());
    }
    if let Some(m) = load_manifest(&name) {
        refresh_if_stale(&m);
        return Ok(m);
    }
    sync(&name, false).await
}

/// Check a stored pack against Telegram in the background when it's old.
pub fn refresh_if_stale(m: &Manifest) {
    if m.local || now_secs().saturating_sub(m.updated) < STALE_SECS || bot_token().is_none() {
        return;
    }
    let name = m.name.clone();
    tokio::spawn(async move {
        if let Err(e) = sync(&name, true).await {
            tracing::warn!(pack = %name, error = %e, "pack refresh failed");
        }
    });
}

/// Download a pack from Telegram, or bring a stored one up to date: files
/// it already has (same `file_unique_id`) are kept, only new stickers are
/// downloaded, removed ones disappear, Telegram's order is applied.
pub async fn sync(name: &str, refresh: bool) -> Result<Manifest, String> {
    use futures_util::StreamExt;
    let lock = pack_lock(&name.to_ascii_lowercase());
    let _guard = lock.lock().await;
    let old = load_manifest(name);
    if let Some(m) = old.as_ref().filter(|m| m.local) {
        return Ok(m.clone());
    }
    if let Some(m) = &old {
        // Someone else just did it while we waited for the lock.
        if !refresh || now_secs().saturating_sub(m.updated) < REFRESH_MIN_SECS {
            return Ok(m.clone());
        }
    }
    let token = bot_token().ok_or("this server has no Telegram bot token — ask its admin to add one in the admin panel")?;
    // Direct, or through the proxy / API address set in the panel (tgnet.rs).
    let http = crate::tgnet::client(40);
    let base = crate::tgnet::base();
    let set: serde_json::Value = http
        .get(format!("{base}/bot{token}/getStickerSet?name={}", urlenc(name)))
        .send().await.map_err(|e| if e.is_connect() || e.is_timeout() {
            // Never the URL: it carries the bot token, and this goes to clients.
            "this server can't reach Telegram (blocked in its network?) — its admin sets a proxy in Aquamarine Server → Stickers → Telegram network".to_string()
        } else { format!("Telegram: {}", e.without_url()) })?
        .json().await.map_err(|e| format!("Telegram: {}", e.without_url()))?;
    if !set["ok"].as_bool().unwrap_or(false) {
        return Err(format!("Telegram: {}", set["description"].as_str().unwrap_or("error")));
    }
    let r = &set["result"];
    let real = r["name"].as_str().map(short_name).unwrap_or_else(|| name.to_string());
    let rules = Rules::load();
    if rules.is_banned(&real) {
        return Err(BANNED.into());
    }
    let gone = rules.removed_of(&real).to_vec();
    let title = r["title"].as_str().unwrap_or(&real).to_string();
    let kind = r["sticker_type"].as_str().unwrap_or("regular").to_string();
    let dir = pack_dir(&real);
    let tmp = Path::new(PACKS_DIR).join(format!(".{real}.part"));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    // Stickers the admin removed stay out.
    let stickers: Vec<serde_json::Value> = r["stickers"].as_array().cloned().unwrap_or_default().into_iter()
        .filter(|st| !st["file_unique_id"].as_str().is_some_and(|u| gone.iter().any(|g| g == u)))
        .take(MAX_STICKERS).collect();
    let started = std::time::Instant::now();
    // Keep what we have: link (or copy) the old file into the new folder.
    let have: std::collections::HashMap<String, PackFile> = old.as_ref()
        .map(|m| m.files.iter().filter(|f| !f.id.is_empty()).map(|f| (f.id.clone(), f.clone())).collect())
        .unwrap_or_default();
    let mut slots: Vec<Option<PackFile>> = vec![None; stickers.len()];
    let mut todo = Vec::new();
    for (i, st) in stickers.into_iter().enumerate() {
        let uid = st["file_unique_id"].as_str().unwrap_or("");
        if let Some(f) = have.get(uid) {
            let src = dir.join(&f.file);
            if std::fs::hard_link(&src, tmp.join(&f.file)).is_ok() || std::fs::copy(&src, tmp.join(&f.file)).is_ok() {
                let mut f = f.clone();
                f.emoji = st["emoji"].as_str().unwrap_or(&f.emoji).trim().to_string();
                slots[i] = Some(f);
                continue;
            }
        }
        todo.push((i, st));
    }
    let fresh = todo.len();
    tracing::info!(pack = %real, kept = slots.iter().flatten().count(), downloading = fresh, "syncing sticker pack");
    let got: Vec<(usize, PackFile)> = futures_util::stream::iter(todo)
        .map(|(i, st)| {
            let (http, base, token, tmp) = (http.clone(), base.clone(), token.clone(), tmp.clone());
            async move { fetch_sticker(http, base, token, tmp, i, st).await.map(|f| (i, f)) }
        })
        .buffer_unordered(PARALLEL)
        .filter_map(|x| async move { x })
        .collect()
        .await;
    for (i, f) in got {
        slots[i] = Some(f);
    }
    let files: Vec<PackFile> = slots.into_iter().flatten().collect();
    if files.is_empty() {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(format!("nothing could be downloaded from \"{title}\""));
    }
    let same = old.as_ref().is_some_and(|o| {
        o.title == title && o.files.len() == files.len() && o.files.iter().zip(&files).all(|(a, b)| a.file == b.file && a.emoji == b.emoji)
    });
    let rev = old.as_ref().map(|o| o.rev + u64::from(!same)).unwrap_or(1);
    // Same stickers: the detector's last look still holds.
    let scan = old.as_ref().and_then(|o| o.scan).filter(|s| s.rev == rev);
    let m = Manifest { name: real.clone(), title, kind, files, updated: now_secs(), rev, local: false, adult: false, scan };
    std::fs::write(tmp.join("manifest.json"), serde_json::to_vec(&m).unwrap_or_default()).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::rename(&tmp, &dir).map_err(|e| e.to_string())?;
    tracing::info!(pack = %m.name, files = m.files.len(), downloaded = fresh, rev = m.rev, ms = started.elapsed().as_millis() as u64, "sticker pack synced");
    if m.scan.is_none() {
        spawn_scan(&m.name);
    }
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
    /// Banned here (lowercase): apps drop them from libraries and hide
    /// their stickers in chats (with 1.20.1-9).
    pub banned: Vec<String>,
    /// Pack (lowercase) → stickers removed from it (file names / ids).
    pub removed: std::collections::BTreeMap<String, Vec<String>>,
}
#[derive(Serialize)]
pub struct PackSummary {
    pub name: String,
    pub title: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub count: usize,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub adult: bool,
}

pub async fn list(State(_): State<AppState>) -> Json<PacksIndex> {
    let r = Rules::load();
    Json(PacksIndex {
        telegram: bot_token().is_some(),
        packs: list_manifests()
            .into_iter()
            .filter(|m| !r.is_banned(&m.name))
            .map(|m| served(m, &r))
            .map(|m| PackSummary { count: m.files.len(), adult: m.adult, name: m.name, title: m.title, kind: m.kind })
            .collect(),
        banned: r.banned.clone(),
        removed: r.removed.clone(),
    })
}

pub async fn manifest(UrlPath(name): UrlPath<String>) -> Result<Json<Manifest>, StatusCode> {
    let r = Rules::load();
    let m = load_manifest(&short_name(&name)).filter(|m| !r.is_banned(&m.name)).ok_or(StatusCode::NOT_FOUND)?;
    refresh_if_stale(&m);
    Ok(Json(served(m, &r)))
}

/// Check a pack against Telegram now (at most once a minute per pack).
pub async fn refresh_http(_caller: Caller, UrlPath(name): UrlPath<String>) -> Result<Json<Manifest>, (StatusCode, String)> {
    let name = short_name(&name);
    if load_manifest(&name).is_none() {
        return Err((StatusCode::NOT_FOUND, "no such pack".into()));
    }
    let m = sync(&name, true).await.map_err(|e| (StatusCode::BAD_GATEWAY, e))?;
    Ok(Json(served(m, &Rules::load())))
}

pub async fn file(UrlPath((name, file)): UrlPath<(String, String)>) -> impl IntoResponse {
    let name = short_name(&name);
    let Some(m) = load_manifest(&name).map(|m| served(m, &Rules::load())) else { return Err(StatusCode::NOT_FOUND) };
    // Only files listed in the manifest — no path tricks (nor removed ones).
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
    let m = import(&b.link).await.map_err(|e| (if e == BANNED { StatusCode::FORBIDDEN } else { StatusCode::BAD_GATEWAY }, e))?;
    Ok(Json(served(m, &Rules::load())))
}

#[derive(Serialize)]
pub struct TokenView {
    pub source: &'static str,
    pub has_token: bool,
    pub packs: usize,
    /// The packs stored here, for the panel's list.
    pub list: Vec<PackBrief>,
    /// Banned packs (they can't be imported again).
    pub banned: Vec<String>,
}

/// One stored pack in the admin panel: name, title, kind, how many files,
/// and a few files to show (`/v1/packs/<name>/<file>`).
#[derive(Serialize)]
pub struct PackBrief {
    pub name: String,
    pub title: String,
    pub kind: String,
    pub count: usize,
    pub preview: Vec<(String, String)>,
    pub updated: u64,
    /// Uploaded here, not from Telegram.
    pub local: bool,
    /// 18+ (set or guessed), and whether the admin set it.
    pub adult: bool,
    pub adult_set: Option<bool>,
    /// The detector's look: (checked, explicit), if it has looked.
    pub scan: Option<(u32, u32)>,
}

fn briefs() -> Vec<PackBrief> {
    let r = Rules::load();
    list_manifests()
        .into_iter()
        .map(|m| served(m, &r))
        .map(|m| PackBrief {
            adult_set: r.adult.get(&m.name.to_ascii_lowercase()).copied(),
            scan: m.scan.filter(|s| s.rev == m.rev).map(|s| (s.checked, s.explicit)),
            adult: m.adult,
            count: m.files.len(),
            preview: m.files.iter().take(4).map(|f| (f.file.clone(), f.mime.clone())).collect(),
            name: m.name,
            title: m.title,
            kind: m.kind,
            updated: m.updated,
            local: m.local,
        })
        .collect()
}
#[derive(Deserialize)]
pub struct TokenUpdate {
    #[serde(default)]
    pub token: String,
}

pub async fn admin_get(State(state): State<AppState>, headers: axum::http::HeaderMap) -> Result<Json<TokenView>, StatusCode> {
    admin_authed(&state, &headers)?;
    Ok(Json(token_view()))
}

fn token_view() -> TokenView {
    let list = briefs();
    TokenView { source: token_source(), has_token: bot_token().is_some(), packs: list.len(), list, banned: Rules::load().banned }
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
    Ok(Json(token_view()))
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
    #[test]
    fn adult_guess_and_override() {
        assert!(looks_adult("x", "Random Stickers R18 @sharky_png"));
        assert!(looks_adult("MLP", "MLP HENTAI 🦄"));
        assert!(!looks_adult("eltaller47", "eltaller47"));
        let mut r = Rules::default();
        let mut pm = Manifest { name: "P".into(), title: "nsfw pack".into(), kind: "regular".into(), updated: 0, rev: 3, local: false, adult: false, scan: None, files: vec![] };
        assert!(r.adult_of(&pm));
        pm.title = "cats".into();
        assert!(!r.adult_of(&pm));
        pm.scan = Some(Scan { rev: 3, checked: 30, explicit: 4 });
        assert!(r.adult_of(&pm));
        pm.scan = Some(Scan { rev: 2, checked: 30, explicit: 4 }); // stale
        assert!(!r.adult_of(&pm));
        pm.scan = Some(Scan { rev: 3, checked: 30, explicit: 4 });
        r.adult.insert("p".into(), false);
        assert!(!r.adult_of(&pm));
        r.removed.insert("p".into(), vec!["2.webp".into(), "uid9".into()]);
        let m = Manifest { name: "P".into(), title: "t".into(), kind: "regular".into(), updated: 0, rev: 1, local: false, adult: false, scan: None,
            files: ["1.webp", "2.webp", "3.webp"].iter().enumerate().map(|(i, f)| PackFile { file: f.to_string(), emoji: String::new(), mime: String::new(), id: if i == 2 { "uid9".into() } else { String::new() } }).collect() };
        assert_eq!(served(m, &r).files.len(), 1);
    }
}

// ---------- Aquamarine Server → Stickers: add / upload / remove ------------

#[derive(Deserialize)]
pub struct AdminImport {
    pub link: String,
}

/// The admin adds a Telegram pack by its link.
pub async fn admin_import(State(state): State<AppState>, headers: axum::http::HeaderMap, Json(b): Json<AdminImport>) -> Result<Json<Manifest>, (StatusCode, String)> {
    admin_authed(&state, &headers).map_err(|c| (c, "admin only".into()))?;
    import(&b.link).await.map(Json).map_err(|e| (StatusCode::BAD_GATEWAY, e))
}

#[derive(Deserialize)]
pub struct UploadFile {
    pub name: String,
    /// base64
    pub data: String,
    #[serde(default)]
    pub emoji: String,
}

#[derive(Deserialize)]
pub struct AdminUpload {
    pub title: String,
    /// "regular" (stickers) | "custom_emoji"
    #[serde(default)]
    pub kind: String,
    /// Add to this uploaded pack instead of making a new one.
    #[serde(default)]
    pub to: Option<String>,
    pub files: Vec<UploadFile>,
}

/// Admin uploads: whole sizes per request is capped by the route's body limit.
pub const UPLOAD_LIMIT: usize = 64 * 1024 * 1024;

/// Pictures (png/webp/gif), Telegram animations (.tgs) and videos (.webm)
/// from the admin's computer become a pack every app on this server sees.
pub async fn admin_upload(State(state): State<AppState>, headers: axum::http::HeaderMap, Json(b): Json<AdminUpload>) -> Result<Json<Manifest>, (StatusCode, String)> {
    use base64::Engine;
    admin_authed(&state, &headers).map_err(|c| (c, "admin only".into()))?;
    let bad = |m: &str| (StatusCode::BAD_REQUEST, m.to_string());
    if b.files.is_empty() {
        return Err(bad("no files"));
    }
    let mut m = match b.to.as_deref().map(short_name).filter(|n| !n.is_empty()) {
        Some(n) => load_manifest(&n).filter(|m| m.local).ok_or_else(|| bad("no such uploaded pack"))?,
        None => {
            let title = b.title.trim();
            if title.is_empty() {
                return Err(bad("the pack needs a name"));
            }
            let slug: String = title.chars().filter(|c| c.is_ascii_alphanumeric()).take(24).collect::<String>().to_ascii_lowercase();
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
            let tail = format!("{:06x}", (nanos ^ (nanos >> 24)) as u32 & 0xff_ffff);
            Manifest {
                name: format!("my_{}{tail}", if slug.is_empty() { "pack_".into() } else { slug + "_" }),
                title: title.chars().take(64).collect(),
                kind: if b.kind == "custom_emoji" { "custom_emoji".into() } else { "regular".into() },
                files: Vec::new(),
                updated: now_secs(),
                rev: 0,
                local: true,
                adult: false,
                scan: None,
            }
        }
    };
    let dir = pack_dir(&m.name);
    std::fs::create_dir_all(&dir).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    for f in b.files {
        if m.files.len() >= MAX_STICKERS {
            break;
        }
        let ext = f.name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
        let ext = match ext.as_str() {
            "png" | "webp" | "gif" | "tgs" | "webm" => ext,
            "jpg" | "jpeg" => return Err(bad("JPEG has no transparency — use PNG or WebP")),
            _ => return Err(bad("only PNG, WebP, GIF, TGS and WebM")),
        };
        let bytes = base64::engine::general_purpose::STANDARD.decode(f.data.as_bytes()).map_err(|_| bad("bad file data"))?;
        if bytes.is_empty() || bytes.len() > MAX_FILE {
            return Err(bad("each file up to 2 MB"));
        }
        let file = format!("{}_{}.{ext}", m.files.len() + 1, now_secs() % 100_000);
        std::fs::write(dir.join(&file), &bytes).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        let emoji = f.emoji.chars().take(8).collect::<String>();
        m.files.push(PackFile { mime: mime_of(&file).into(), file, emoji: if emoji.is_empty() { "🙂".into() } else { emoji }, id: String::new() });
    }
    m.rev += 1;
    m.updated = now_secs();
    let json = serde_json::to_vec_pretty(&m).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    std::fs::write(dir.join("manifest.json"), json).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    spawn_scan(&m.name);
    Ok(Json(m))
}

/// One pack, whole, for the panel.
pub async fn admin_manifest(State(state): State<AppState>, headers: axum::http::HeaderMap, UrlPath(name): UrlPath<String>) -> Result<Json<Manifest>, StatusCode> {
    admin_authed(&state, &headers)?;
    load_manifest(&short_name(&name)).map(|m| Json(served(m, &Rules::load()))).ok_or(StatusCode::NOT_FOUND)
}

#[derive(Deserialize)]
pub struct RulesUpdate {
    /// "on" | "off" | "auto" (the guess by the title).
    #[serde(default)]
    pub adult: Option<String>,
    /// true: remove the pack and refuse it from now on; false: allow it again.
    #[serde(default)]
    pub ban: Option<bool>,
}

/// 18+ mark and ban of a pack (also of one not on this server: by name / link).
pub async fn admin_rules(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    UrlPath(name): UrlPath<String>,
    Json(u): Json<RulesUpdate>,
) -> Result<Json<TokenView>, (StatusCode, String)> {
    admin_authed(&state, &headers).map_err(|c| (c, "admin only".into()))?;
    let name = short_name(&name);
    if name.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "no pack name".into()));
    }
    let lc = name.to_ascii_lowercase();
    let _guard = pack_lock(&lc);
    let _guard = _guard.lock().await;
    let mut r = Rules::load();
    match u.adult.as_deref() {
        Some("on") => { r.adult.insert(lc.clone(), true); }
        Some("off") => { r.adult.insert(lc.clone(), false); }
        Some("auto") => { r.adult.remove(&lc); }
        _ => {}
    }
    match u.ban {
        Some(true) => {
            if !r.is_banned(&lc) { r.banned.push(lc.clone()); }
            if load_manifest(&name).is_some() { let _ = std::fs::remove_dir_all(pack_dir(&name)); }
        }
        Some(false) => r.banned.retain(|b| !b.eq_ignore_ascii_case(&lc)),
        None => {}
    }
    r.save().map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(token_view()))
}

/// Take one sticker out of a pack for everyone (a Telegram refresh won't
/// bring it back; apps drop it from libraries and hide it in chats).
pub async fn admin_remove_file(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    UrlPath((name, file)): UrlPath<(String, String)>,
) -> Result<Json<Manifest>, (StatusCode, String)> {
    admin_authed(&state, &headers).map_err(|c| (c, "admin only".into()))?;
    let name = short_name(&name);
    let lock = pack_lock(&name.to_ascii_lowercase());
    let _guard = lock.lock().await;
    let mut m = load_manifest(&name).ok_or((StatusCode::NOT_FOUND, "no such pack".into()))?;
    let i = m.files.iter().position(|f| f.file == file).ok_or((StatusCode::NOT_FOUND, "no such sticker".into()))?;
    let f = m.files.remove(i);
    let mut r = Rules::load();
    let gone = r.removed.entry(name.to_ascii_lowercase()).or_default();
    for k in [f.file.clone(), f.id.clone()] {
        if !k.is_empty() && !gone.contains(&k) { gone.push(k); }
    }
    r.save().map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let dir = pack_dir(&m.name);
    let _ = std::fs::remove_file(dir.join(&f.file));
    m.rev += 1;
    std::fs::write(dir.join("manifest.json"), serde_json::to_vec(&m).unwrap_or_default())
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    spawn_scan(&m.name);
    Ok(Json(served(m, &r)))
}

/// Remove a pack from this server (apps that already have it keep their copy).
pub async fn admin_delete(State(state): State<AppState>, headers: axum::http::HeaderMap, UrlPath(name): UrlPath<String>) -> Result<StatusCode, StatusCode> {
    admin_authed(&state, &headers)?;
    let name = short_name(&name);
    if name.is_empty() || load_manifest(&name).is_none() {
        return Err(StatusCode::NOT_FOUND);
    }
    std::fs::remove_dir_all(pack_dir(&name)).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}

/// "Clear the server" with packs ticked: every sticker and emoji pack here.
pub fn clear_all() -> usize {
    let Ok(rd) = std::fs::read_dir(PACKS_DIR) else { return 0 };
    let mut n = 0;
    for e in rd.flatten() {
        let p = e.path();
        let ok = if p.is_dir() { std::fs::remove_dir_all(&p).is_ok() } else { std::fs::remove_file(&p).is_ok() };
        if ok {
            n += 1;
        }
    }
    n
}
