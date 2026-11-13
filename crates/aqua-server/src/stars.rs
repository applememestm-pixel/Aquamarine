//! Stars: marks only the project owner hands out — and every app trusts.
//!
//! A star is a statement signed with the owner's Ed25519 key: "the account
//! with ID <uid> has star <kind> until <exp>". No server and no app can make
//! one up: every app checks the signature against the public key built into
//! it (aqua-client-ui `src/stars.rs`, `STAR_PUBLIC`), and the ID comes from
//! the account's identity key, so a statement can't be moved to another
//! account either. Servers only pass statements on.
//!
//! The private key lives only on the relay that hands stars out —
//! `aqua-star.key` in its data folder, made once with `aqua-server star-key`
//! (which prints the public key to build into the apps). That relay runs the
//! @stars bot, for the owner only (the e-mail check of verify.rs):
//!   /star @name [official|1-8]   /unstar @name   /stars
//! and, with `AQUA_BOOSTY_TOKEN` + `AQUA_BOOSTY_BLOG`, follows the Boosty
//! subscribers: an account whose e-mail here is a subscriber's gets the star
//! of that subscription level (levels by price: the cheapest is 1). Stars set
//! by hand are never touched by that.
//!
//! Kinds: 0 — the official star; 1–8 — the Boosty levels.
//! A statement lasts 8 days and is signed afresh on every request while the
//! star stands, so a removed star fades from other people's screens within
//! that time (from its owner's own app at once).
//! `GET /v1/stars/:uid` — the statement, from the relay with the key, or from
//! any other relay fetched there (`AQUA_STAR_HOME`).

use std::collections::BTreeMap;
use std::sync::Mutex;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::AppState;

const KEY_FILE: &str = "aqua-star.key";
const STORE: &str = "aqua-stars.json";
/// How long a signed statement holds.
const LIFE_SECS: u64 = 8 * 24 * 3600;
/// The relay with the key, for relays without it (the owner's).
const DEFAULT_HOME: &str = "http://193.233.245.196:7777";

#[derive(Clone, Serialize, Deserialize)]
struct Star {
    kind: u8,
    /// "manual" (the owner's command) or "boosty".
    source: String,
    at: u64,
}

/// What every app checks (see the module note).
#[derive(Clone, Serialize, Deserialize)]
pub struct Statement {
    pub uid: String,
    pub kind: u8,
    pub exp: u64,
    /// Ed25519 over [`signed_bytes`], hex.
    pub sig: String,
}

pub fn signed_bytes(uid: &str, kind: u8, exp: u64) -> Vec<u8> {
    format!("aqua-star-v1|{uid}|{kind}|{exp}").into_bytes()
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

static STARS: Mutex<Option<BTreeMap<String, Star>>> = Mutex::new(None);

fn with_stars<R>(f: impl FnOnce(&mut BTreeMap<String, Star>) -> R) -> R {
    let mut g = STARS.lock().unwrap_or_else(|e| e.into_inner());
    let map = g.get_or_insert_with(|| std::fs::read(STORE).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default());
    f(map)
}

fn save(map: &BTreeMap<String, Star>) {
    // Whole or not at all: a cut file read back as "nobody has a star".
    let tmp = format!("{STORE}.tmp");
    if std::fs::write(&tmp, serde_json::to_vec_pretty(map).unwrap_or_default()).is_ok() {
        let _ = std::fs::rename(&tmp, STORE);
    }
}

/// The signing key, if this is the relay that hands stars out.
fn key() -> Option<aqua_core::crypto::SignSecret> {
    let b = std::fs::read(KEY_FILE).ok()?;
    let hex_str = String::from_utf8(b).ok()?;
    let raw: [u8; 32] = hex::decode(hex_str.trim()).ok()?.try_into().ok()?;
    Some(aqua_core::crypto::SignSecret::from_bytes(raw))
}

pub fn has_key() -> bool {
    key().is_some()
}

/// Make the key once (never over an existing one); its public half.
pub fn make_key() -> Result<String, String> {
    if let Some(k) = key() {
        return Ok(hex::encode(k.public().as_bytes()));
    }
    let k = aqua_core::crypto::SignSecret::generate();
    std::fs::write(KEY_FILE, hex::encode(k.to_bytes())).map_err(|e| format!("Couldn't write {KEY_FILE}: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(KEY_FILE, std::fs::Permissions::from_mode(0o600));
    }
    Ok(hex::encode(k.public().as_bytes()))
}

/// `GET /v1/admin/stars` — is this the star relay, its public key, how many
/// stars; `POST` makes the key (once) and starts the @stars bot.
pub async fn admin_get(State(state): State<AppState>, headers: axum::http::HeaderMap) -> Result<Json<serde_json::Value>, StatusCode> {
    crate::admin_authed(&state, &headers)?;
    let n = with_stars(|m| m.len());
    Ok(Json(serde_json::json!({ "has_key": has_key(), "public": key().map(|k| hex::encode(k.public().as_bytes())), "stars": n, "bot": bot_name() })))
}

pub async fn admin_make(State(state): State<AppState>, headers: axum::http::HeaderMap) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    crate::admin_authed(&state, &headers).map_err(|c| (c, String::new()))?;
    let had = has_key();
    let public = make_key().map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    if !had {
        spawn(state.clone());
    }
    Ok(Json(serde_json::json!({ "has_key": true, "public": public, "stars": with_stars(|m| m.len()), "bot": bot_name() })))
}

/// `aqua-server star-key`: make the key once (never over an existing one)
/// and print the public half.
pub fn keygen_cli() -> String {
    if let Some(k) = key() {
        return format!("The star key exists already ({KEY_FILE}). Public key:\n{}", hex::encode(k.public().as_bytes()));
    }
    let k = match make_key().ok().and_then(|_| key()) {
        Some(k) => k,
        None => return format!("Couldn't write {KEY_FILE}"),
    };
    format!(
        "Star key made: {KEY_FILE} (keep it secret, back it up; never share it).\nPublic key — send this one to be built into the apps:\n{}",
        hex::encode(k.public().as_bytes())
    )
}

fn sign(uid: &str, kind: u8) -> Option<Statement> {
    let k = key()?;
    let exp = now() + LIFE_SECS;
    let sig = k.sign(&signed_bytes(uid, kind, exp));
    Some(Statement { uid: uid.to_string(), kind, exp, sig: hex::encode(sig) })
}

/// Proxied statements, for a while (relays without the key).
static CACHE: Mutex<Option<BTreeMap<String, (u64, Option<Statement>)>>> = Mutex::new(None);

/// `GET /v1/stars/:uid`.
pub async fn get_star(State(_state): State<AppState>, Path(uid): Path<String>) -> Result<Json<Statement>, StatusCode> {
    let uid = uid.trim().to_string();
    if uid.is_empty() || uid.len() > 32 || !uid.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(StatusCode::BAD_REQUEST);
    }
    if has_key() {
        let kind = with_stars(|m| m.get(&uid).map(|s| s.kind));
        return kind.and_then(|k| sign(&uid, k)).map(Json).ok_or(StatusCode::NOT_FOUND);
    }
    // Not the star relay: ask it (and remember the answer for 10 minutes).
    if let Some((at, st)) = CACHE.lock().ok().and_then(|g| g.as_ref().and_then(|m| m.get(&uid).cloned())) {
        if now() < at + 600 {
            return st.map(Json).ok_or(StatusCode::NOT_FOUND);
        }
    }
    let home = std::env::var("AQUA_STAR_HOME").ok().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| DEFAULT_HOME.into());
    let url = format!("{}/v1/stars/{uid}", home.trim_end_matches('/'));
    // Ok(None): the star relay says "no star". Err: it couldn't be asked —
    // that's not "no star" (the app would take the star off the card).
    let res: Result<Option<Statement>, ()> = async {
        let c = reqwest::Client::builder().timeout(std::time::Duration::from_secs(8)).build().map_err(|_| ())?;
        let r = c.get(&url).send().await.map_err(|_| ())?;
        if r.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !r.status().is_success() {
            return Err(());
        }
        r.json::<Statement>().await.map(Some).map_err(|_| ())
    }
    .await;
    let Ok(st) = res else { return Err(StatusCode::BAD_GATEWAY) };
    if let Ok(mut g) = CACHE.lock() {
        let m = g.get_or_insert_with(BTreeMap::new);
        if m.len() > 5000 {
            m.clear();
        }
        m.insert(uid, (now(), st.clone()));
    }
    st.map(Json).ok_or(StatusCode::NOT_FOUND)
}

// ---------- the @stars bot (only on the relay with the key) ----------------

pub fn bot_name() -> String {
    std::env::var("AQUA_STARS_BOT").ok().map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()).unwrap_or_else(|| "stars".into())
}

pub(crate) fn ensure_bot(state: &AppState) {
    let name = bot_name();
    if state.directory.is_bot(&name) {
        return;
    }
    match state.directory.register_bot(&name, &crate::music::relay_bot_owner(), "Stars") {
        Ok(_) => tracing::info!(bot = %name, "stars bot created"),
        Err(e) => tracing::warn!(bot = %name, "can't create the stars bot: {e}"),
    }
}

/// Started with the relay: does anything only where the key is.
pub fn spawn(state: AppState) {
    if !has_key() {
        return;
    }
    tracing::info!("star key found: this relay hands out stars");
    ensure_bot(&state);
    let st = state.clone();
    tokio::spawn(async move {
        loop {
            let q = st.bot_queue(&bot_name());
            let batch: Vec<crate::BotUpdate> = {
                let notify = q.lock().await.notify.clone();
                let wait = notify.notified();
                let mut g = q.lock().await;
                if g.updates.is_empty() {
                    drop(g);
                    let _ = tokio::time::timeout(std::time::Duration::from_secs(30), wait).await;
                    q.lock().await.updates.drain(..).collect()
                } else {
                    g.updates.drain(..).collect()
                }
            };
            for u in batch {
                if !u.from_bot {
                    answer(&st, &u).await;
                }
            }
        }
    });
    boosty::spawn(state);
}

async fn say(state: &AppState, to: &str, text: &str) {
    crate::bot_deliver_text(state, &bot_name(), to, text).await;
}

fn kind_name(k: u8) -> String {
    if k == 0 { "official ⭐".into() } else { format!("level {k} ★") }
}

async fn answer(state: &AppState, u: &crate::BotUpdate) {
    let text = u.text.trim();
    if text.is_empty() {
        return;
    }
    if !crate::verify::is_owner(state, &u.from) {
        say(state, &u.from, "⭐ Stars are given by the Aquamarine team. Boosty supporters get theirs on their own.").await;
        return;
    }
    let (cmd, arg) = text.split_once(char::is_whitespace).unwrap_or((text, ""));
    let cmd = cmd.split('@').next().unwrap_or(cmd);
    let mut words = arg.split_whitespace();
    let who = words.next().map(|w| w.trim_start_matches('@').to_lowercase()).filter(|w| !w.is_empty());
    match cmd {
        "/star" | "/unstar" => {
            let Some(name) = who else {
                say(state, &u.from, &format!("Who? {cmd} @name{}", if cmd == "/star" { " [official|1-8]" } else { "" })).await;
                return;
            };
            let Some(uid) = state.directory.uid_of(&name).filter(|x| !x.is_empty()) else {
                say(state, &u.from, &format!("There's no @{name} on this server.")).await;
                return;
            };
            if cmd == "/unstar" {
                let had = with_stars(|m| {
                    let had = m.remove(&uid).is_some();
                    save(m);
                    had
                });
                say(state, &u.from, &if had { format!("@{name} has no star now (other people stop seeing it within 8 days).") } else { format!("@{name} had no star.") }).await;
                return;
            }
            let kind = match words.next().map(|w| w.to_lowercase()) {
                None => 0,
                Some(w) if w == "official" || w == "0" => 0,
                Some(w) => match w.parse::<u8>() {
                    Ok(n) if (1..=8).contains(&n) => n,
                    _ => {
                        say(state, &u.from, "The kind: official, or 1–8 (the Boosty levels).").await;
                        return;
                    }
                },
            };
            with_stars(|m| {
                m.insert(uid, Star { kind, source: "manual".into(), at: now() });
                save(m);
            });
            say(state, &u.from, &format!("@{name} now has the {} star.", kind_name(kind))).await;
        }
        "/stars" => {
            let list = with_stars(|m| m.iter().map(|(uid, s)| (uid.clone(), s.clone())).collect::<Vec<_>>());
            if list.is_empty() {
                say(state, &u.from, "Nobody has a star yet.").await;
                return;
            }
            let lines: Vec<String> = list
                .iter()
                .map(|(uid, s)| {
                    let n = state.directory.username_by_uid(uid).map(|n| format!("@{n}")).unwrap_or_else(|| format!("ID {uid}"));
                    format!("{n} — {}{}", kind_name(s.kind), if s.source == "boosty" { " (Boosty)" } else { "" })
                })
                .collect();
            say(state, &u.from, &format!("Stars ({}):\n{}", lines.len(), lines.join("\n"))).await;
        }
        _ => {
            say(state, &u.from, "⭐ Stars:\n/star @name — the official star\n/star @name 1-8 — a Boosty level\n/unstar @name — take it away\n/stars — who has one").await;
        }
    }
}

/// Boosty subscribers → stars (by the account's e-mail on this relay).
mod boosty {
    use super::*;

    pub fn spawn(state: AppState) {
        let (Ok(token), Ok(blog)) = (std::env::var("AQUA_BOOSTY_TOKEN"), std::env::var("AQUA_BOOSTY_BLOG")) else { return };
        if token.trim().is_empty() || blog.trim().is_empty() {
            return;
        }
        tokio::spawn(async move {
            loop {
                match sync(&state, token.trim(), blog.trim()).await {
                    Ok(n) => tracing::info!(subscribers = n, "Boosty stars synced"),
                    Err(e) => tracing::warn!("Boosty sync: {e}"),
                }
                tokio::time::sleep(std::time::Duration::from_secs(30 * 60)).await;
            }
        });
    }

    async fn sync(state: &AppState, token: &str, blog: &str) -> Result<usize, String> {
        let c = reqwest::Client::builder().timeout(std::time::Duration::from_secs(30)).build().map_err(|e| e.to_string())?;
        let get = |url: String| {
            let c = c.clone();
            async move {
                let r = c.get(&url).bearer_auth(token).send().await.map_err(|e| e.without_url().to_string())?;
                if !r.status().is_success() {
                    return Err(format!("HTTP {}", r.status()));
                }
                r.json::<serde_json::Value>().await.map_err(|e| e.without_url().to_string())
            }
        };
        // The blog's levels, cheapest first → 1..8.
        let levels = get(format!("https://api.boosty.to/v1/blog/{blog}/subscription_level/?show_free_level=false")).await?;
        let mut lv: Vec<(i64, u64)> = levels["data"]
            .as_array()
            .map(|a| a.iter().filter_map(|l| Some((l["id"].as_i64()?, l["price"].as_u64().unwrap_or(0)))).collect())
            .unwrap_or_default();
        lv.sort_by_key(|x| x.1);
        let tier_of = |id: i64| lv.iter().position(|x| x.0 == id).map(|i| (i as u8 + 1).min(8));
        // Every active subscriber: e-mail → level.
        let mut subs: BTreeMap<String, u8> = BTreeMap::new();
        let mut offset = 0;
        loop {
            let page = get(format!("https://api.boosty.to/v1/blog/{blog}/subscribers?limit=100&offset={offset}&sort_by=on_time&order=gt")).await?;
            let data = page["data"].as_array().cloned().unwrap_or_default();
            if data.is_empty() {
                break;
            }
            for s in &data {
                let active = s["subscribed"].as_bool().unwrap_or(true);
                let email = s["email"].as_str().unwrap_or("").trim().to_lowercase();
                let tier = s["level"]["id"].as_i64().and_then(tier_of);
                if active && !email.is_empty() {
                    if let Some(t) = tier {
                        let e = subs.entry(email).or_insert(t);
                        *e = (*e).max(t);
                    }
                }
            }
            offset += data.len();
            if data.len() < 100 || offset > 50_000 {
                break;
            }
        }
        // Accounts here with those e-mails.
        let mut want: BTreeMap<String, u8> = BTreeMap::new();
        for (email, tier) in &subs {
            for name in state.directory.usernames_by_email(email) {
                if let Some(uid) = state.directory.uid_of(&name).filter(|x| !x.is_empty()) {
                    want.insert(uid, *tier);
                }
            }
        }
        let n = subs.len();
        with_stars(|m| {
            // Boosty stars that no longer stand go; hand-set ones stay.
            m.retain(|uid, s| s.source != "boosty" || want.contains_key(uid));
            for (uid, tier) in want {
                match m.get(&uid) {
                    Some(s) if s.source == "manual" => {}
                    _ => {
                        m.insert(uid, Star { kind: tier, source: "boosty".into(), at: now() });
                    }
                }
            }
            save(m);
        });
        Ok(n)
    }
}
