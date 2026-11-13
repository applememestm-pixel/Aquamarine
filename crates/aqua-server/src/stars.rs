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
//!   /<name> level <0-6>   /star @name [1-6]   /unstar @name   /stars
//! and, with `AQUA_BOOSTY_TOKEN` + `AQUA_BOOSTY_BLOG`, follows the Boosty
//! subscribers: an account whose e-mail here is a subscriber's gets the star
//! of that subscription level (levels by price: the cheapest is 1, at most
//! 5). Stars set by hand are never touched by that.
//!
//! The CeZeOs star (the owner's mark, a sun in its level's colour): levels
//! 1–5 — donations (Boosty); 6 — above them, only the owner gives it by hand.
//! Older builds had 0 ("official", now level 6) and 1–8: stored ones are
//! moved over on load (0 → 6, 7–8 → 5).
//! A statement lasts 8 days and is signed afresh on every request while the
//! star stands. Apps ask `GET /v1/stars/:uid` whenever they refresh a card
//! (aqua-client-ui `stars::star_live`), so a removed star goes within minutes;
//! the statement on the card counts only while the star relay can't be asked.
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
/// The donation levels (Boosty) end here…
pub const DONATION_MAX: u8 = 5;
/// …and this one stands above them: only the owner gives it.
pub const OWNER_LEVEL: u8 = 6;

/// The relay with the key, for relays without it (the owner's).
const DEFAULT_HOME: &str = "http://193.233.245.196:7777";

#[derive(Clone, Serialize, Deserialize)]
struct Star {
    kind: u8,
    /// "manual" (the owner's command) or "boosty".
    source: String,
    at: u64,
    /// Given for a while (`/<name> level 3 30d`): gone after this (unix s).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    until: Option<u64>,
}

impl Star {
    fn live(&self, now: u64) -> bool {
        self.until.map_or(true, |u| now < u)
    }
}

/// "30d", "12h", "2w", "3m" (months of 30 days), also д/ч/н/м → seconds.
fn parse_for(w: &str) -> Option<u64> {
    let w = w.trim().to_lowercase();
    let split = w.find(|c: char| !c.is_ascii_digit())?;
    let (n, unit) = w.split_at(split);
    let n: u64 = n.parse().ok().filter(|&n| n > 0 && n <= 10_000)?;
    let secs = match unit.trim() {
        "h" | "ч" | "час" | "часа" | "часов" => 3600,
        "d" | "д" | "день" | "дня" | "дней" => 86_400,
        "w" | "н" | "нед" => 7 * 86_400,
        "m" | "м" | "мес" | "month" | "months" => 30 * 86_400,
        "y" | "г" | "год" | "года" | "лет" => 365 * 86_400,
        _ => return None,
    };
    Some(n * secs)
}

fn left(until: u64, now: u64) -> String {
    let s = until.saturating_sub(now);
    if s >= 2 * 86_400 { format!("{} d", s / 86_400) } else { format!("{} h", (s / 3600).max(1)) }
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
    let map = g.get_or_insert_with(|| {
        let mut m: BTreeMap<String, Star> = std::fs::read(STORE).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        // The old kinds: "official" (0) is level 6 now, 7–8 are 5.
        let mut moved = false;
        for st in m.values_mut() {
            let k = migrate(st.kind);
            if k != st.kind {
                st.kind = k;
                moved = true;
            }
        }
        if moved {
            save(&m);
        }
        m
    });
    f(map)
}

pub fn migrate(kind: u8) -> u8 {
    match kind {
        0 => OWNER_LEVEL,
        k if k > OWNER_LEVEL => DONATION_MAX,
        k => k,
    }
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

fn sign(uid: &str, kind: u8, until: Option<u64>) -> Option<Statement> {
    let k = key()?;
    // A star given for a while ends there for everyone, not 8 days later.
    let exp = (now() + LIFE_SECS).min(until.unwrap_or(u64::MAX));
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
        let t = now();
        let star = with_stars(|m| m.get(&uid).filter(|s| s.live(t)).map(|s| (s.kind, s.until)));
        return star.and_then(|(k, until)| sign(&uid, k, until)).map(Json).ok_or(StatusCode::NOT_FOUND);
    }
    // Not the star relay: ask it (and remember the answer for 5 minutes).
    if let Some((at, st)) = CACHE.lock().ok().and_then(|g| g.as_ref().and_then(|m| m.get(&uid).cloned())) {
        if now() < at + 300 {
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
    crate::tgapi::set_builtin_app(state, &name, "Stars", APP_URL);
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

/// A message with buttons (the app draws them from the `[kb:v1]` line).
async fn say_kb(state: &AppState, to: &str, text: &str, row: serde_json::Value) {
    let kb = serde_json::json!({ "kind": "inline", "m": 0, "rows": [row] });
    say(state, to, &format!("{}{kb}\n{text}", crate::tgapi::KB_PREFIX)).await;
}

/// …with the button that opens the mini app.
async fn say_app(state: &AppState, to: &str, text: &str) {
    say_kb(state, to, text, serde_json::json!([{ "t": "✦ Open", "wa": APP_URL }])).await;
}

fn kind_name(k: u8) -> String {
    if k == OWNER_LEVEL { format!("CeZeOs level {k} (the owner's)") } else { format!("CeZeOs level {k}") }
}

/// The level word of a command: 1–6 (0 — take the star away).
fn parse_level(w: Option<&str>) -> Result<u8, &'static str> {
    match w.map(str::trim) {
        None | Some("") => Ok(OWNER_LEVEL),
        Some(w) => match w.parse::<u8>() {
            Ok(n) if n <= OWNER_LEVEL => Ok(n),
            _ => Err("The level: 1–5 (donations) or 6 (yours); 0 takes the star away."),
        },
    }
}

async fn answer(state: &AppState, u: &crate::BotUpdate) {
    let text = u.text.trim();
    if text.is_empty() {
        return;
    }
    if !crate::verify::is_owner(state, &u.from) {
        say_app(state, &u.from, "✦ The CeZeOs star: levels 1–5 come with a Boosty subscription — on their own, by your e-mail here, or link your Boosty in the app below (no e-mail needed). Level 6 only from the owner.\nThe app also shows how many days are left until the next payment.").await;
        return;
    }
    // The owner pressed "Link" / "Don't" under a request (app::request).
    let cb = u.callback.clone().or_else(|| {
        let v: serde_json::Value = serde_json::from_str(text.strip_prefix(crate::tgapi::CB_PREFIX)?).ok()?;
        v.get("d")?.as_str().map(str::to_string)
    });
    if let Some(d) = cb {
        if let Some(rest) = d.strip_prefix("bl:") {
            let (verb, uid) = rest.split_once(':').unwrap_or((rest, ""));
            return app::decide(state, &u.from, uid, verb == "ok").await;
        }
        return;
    }
    let (cmd, arg) = text.split_once(char::is_whitespace).unwrap_or((text, ""));
    let cmd = cmd.split('@').next().unwrap_or(cmd);
    // `/<name> level <n>` (the owner's way): the same as `/star @name n`.
    let lower = arg.trim().to_lowercase();
    if !["/star", "/unstar", "/stars", "/start", "/help"].contains(&cmd) && cmd.len() > 1 && lower.starts_with("level") {
        let name = cmd[1..].trim_start_matches('@').to_lowercase();
        let mut rest = lower["level".len()..].split_whitespace();
        let lvl = rest.next().unwrap_or("x");
        return set_level(state, &u.from, &name, parse_level(Some(lvl)), rest.next()).await;
    }
    let mut words = arg.split_whitespace();
    let who = words.next().map(|w| w.trim_start_matches('@').to_lowercase()).filter(|w| !w.is_empty());
    match cmd {
        "/star" | "/unstar" => {
            let Some(name) = who else {
                say(state, &u.from, &format!("Who? {cmd} @name{}", if cmd == "/star" { " [1-6]" } else { "" })).await;
                return;
            };
            let lvl = if cmd == "/unstar" { Ok(0) } else { parse_level(words.next()) };
            set_level(state, &u.from, &name, lvl, words.next()).await;
        }
        "/links" => {
            let list = app::list_links(state);
            say(state, &u.from, &if list.is_empty() { "Nobody linked a Boosty account yet.".to_string() } else { format!("Boosty links:\n{list}") }).await;
        }
        "/unlink" => {
            let Some(name) = who else { return say(state, &u.from, "Who? /unlink @name").await };
            let Some(uid) = state.directory.uid_of(&name).filter(|x| !x.is_empty()) else {
                return say(state, &u.from, &format!("There's no @{name} on this server.")).await;
            };
            let had = app::unlink(&uid);
            say(state, &u.from, &if had { format!("@{name}'s Boosty link is gone.") } else { format!("@{name} had no Boosty link.") }).await;
        }
        "/stars" => {
            let t = now();
            let list = with_stars(|m| m.iter().filter(|(_, s)| s.live(t)).map(|(uid, s)| (uid.clone(), s.clone())).collect::<Vec<_>>());
            if list.is_empty() {
                say(state, &u.from, "Nobody has a star yet.").await;
                return;
            }
            let lines: Vec<String> = list
                .iter()
                .map(|(uid, s)| {
                    let n = state.directory.username_by_uid(uid).map(|n| format!("@{n}")).unwrap_or_else(|| format!("ID {uid}"));
                    let until = s.until.map(|u| format!(" · {} left", left(u, t))).unwrap_or_default();
                    format!("{n} — {}{}{until}", kind_name(s.kind), if s.source == "boosty" { " (Boosty)" } else { "" })
                })
                .collect();
            say(state, &u.from, &format!("Stars ({}):\n{}", lines.len(), lines.join("\n"))).await;
        }
        _ => {
            say(state, &u.from, "✦ The CeZeOs star:\n/<name> level 6 — yours, above the donation levels\n/<name> level 1-5 — a donation level by hand (Boosty gives them on its own)\n/<name> level 4 30d — for a while: h, d, w, m (30 days), y; it goes by itself\n/<name> level 0 — take it away\n(also /star @name [1-6] [30d], /unstar @name)\n/stars — who has one\n/links — Boosty accounts linked without an e-mail, /unlink @name").await;
        }
    }
}

/// Give `name` the star of level `lvl` (0: take it away), by hand.
async fn set_level(state: &AppState, owner: &str, name: &str, lvl: Result<u8, &'static str>, time: Option<&str>) {
    let lvl = match lvl {
        Ok(l) => l,
        Err(e) => return say(state, owner, e).await,
    };
    let dur = match time {
        None => None,
        Some(w) => match parse_for(w) {
            Some(d) => Some(d),
            None => return say(state, owner, "For how long: 12h, 30d, 2w, 3m (months), 1y.").await,
        },
    };
    if name.is_empty() {
        return say(state, owner, "Who? /<name> level <0-6>").await;
    }
    let Some(uid) = state.directory.uid_of(name).filter(|x| !x.is_empty()) else {
        return say(state, owner, &format!("There's no @{name} on this server.")).await;
    };
    if lvl == 0 {
        let had = with_stars(|m| {
            let had = m.remove(&uid).is_some();
            save(m);
            had
        });
        return say(state, owner, &if had { format!("@{name} has no star now (other people stop seeing it within about 15 minutes).") } else { format!("@{name} had no star.") }).await;
    }
    let until = dur.map(|d| now() + d);
    with_stars(|m| {
        m.insert(uid, Star { kind: lvl, source: "manual".into(), at: now(), until });
        save(m);
    });
    let tail = until.map(|u| format!(" for {} (until {})", left(u, now()), fmt_date(u))).unwrap_or_default();
    say(state, owner, &format!("@{name} now has the {} star{tail}.", kind_name(lvl))).await;
}

/// "2026-12-13" (UTC) for unix seconds.
fn fmt_date(t: u64) -> String {
    // Civil date from days (Howard Hinnant's algorithm).
    let z = (t / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
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
        // The blog's levels, cheapest first → 1..5 (more expensive ones: 5).
        let levels = get(format!("https://api.boosty.to/v1/blog/{blog}/subscription_level/?show_free_level=false")).await?;
        let mut lv: Vec<(i64, u64)> = levels["data"]
            .as_array()
            .map(|a| a.iter().filter_map(|l| Some((l["id"].as_i64()?, l["price"].as_u64().unwrap_or(0)))).collect())
            .unwrap_or_default();
        lv.sort_by_key(|x| x.1);
        let tier_of = |id: i64| lv.iter().position(|x| x.0 == id).map(|i| (i as u8 + 1).min(DONATION_MAX));
        // Every active subscriber: e-mail → level; and all of them for the
        // mini app and the links without an e-mail (app.rs-like part below).
        let mut subs: BTreeMap<String, u8> = BTreeMap::new();
        let mut all: Vec<app::Sub> = Vec::new();
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
                if let (true, Some(t), Some(id)) = (active, tier, s["id"].as_i64()) {
                    all.push(app::Sub::from_boosty(s, id, t, email.clone()));
                }
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
        // Linked by hand (no e-mail): the owner approved the Boosty account.
        for (uid, id) in app::approved() {
            if let Some(sub) = all.iter().find(|x| x.id == id) {
                let e = want.entry(uid).or_insert(sub.tier);
                *e = (*e).max(sub.tier);
            }
        }
        app::set_subs(all);
        let n = subs.len();
        with_stars(|m| {
            // Boosty stars that no longer stand go; hand-set ones stay until
            // their time is up.
            let t = now();
            m.retain(|uid, s| if s.source == "boosty" { want.contains_key(uid) } else { s.live(t) });
            for (uid, tier) in want {
                match m.get(&uid) {
                    // Set by hand (level 6 among them): Boosty never lowers it.
                    Some(s) if s.source == "manual" && s.live(now()) => {}
                    _ => {
                        m.insert(uid, Star { kind: tier, source: "boosty".into(), at: now(), until: None });
                    }
                }
            }
            save(m);
        });
        Ok(n)
    }
}

// ---------- the mini app: my star, my Boosty, days to the next payment ------

/// Where the app opens it: the client puts its own relay in front.
pub const APP_URL: &str = "aqua:relay/v1/stars-app";

pub mod app {
    use super::*;
    use axum::response::{Html, IntoResponse};

    const LINKS: &str = "aqua-boosty-links.json";
    const PAGE: &str = include_str!("stars_app.html");

    /// A Boosty account an account here says is theirs (no e-mail needed).
    #[derive(Clone, Serialize, Deserialize)]
    struct Link {
        boosty_id: i64,
        name: String,
        at: u64,
        /// The owner said yes; until then it gives nothing.
        #[serde(default)]
        ok: bool,
    }

    /// An active subscriber, as the last sync saw them.
    #[derive(Clone)]
    pub struct Sub {
        pub id: i64,
        pub tier: u8,
        name: String,
        email: String,
        level: String,
        price: u64,
        on_time: u64,
        next_pay: Option<u64>,
    }

    fn secs(v: &serde_json::Value) -> Option<u64> {
        let n = v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))?;
        // Some fields come in milliseconds.
        Some(if n > 100_000_000_000 { n / 1000 } else { n })
    }

    impl Sub {
        pub fn from_boosty(s: &serde_json::Value, id: i64, tier: u8, email: String) -> Sub {
            let next = ["nextPayTime", "next_pay_time", "nextPayDate", "payTime"].iter().find_map(|k| secs(&s[*k]));
            Sub {
                id,
                tier,
                name: s["name"].as_str().unwrap_or("").trim().to_string(),
                email,
                level: s["level"]["name"].as_str().unwrap_or("").to_string(),
                price: s["price"].as_u64().or_else(|| s["level"]["price"].as_u64()).unwrap_or(0),
                on_time: secs(&s["onTime"]).unwrap_or(0),
                next_pay: next,
            }
        }

        #[cfg(test)]
        pub fn next_for_test(&self, now: u64) -> Option<(u64, bool)> {
            self.next(now)
        }

        /// The next payment: Boosty's own date, or a month after the last
        /// one counted from the start (subscriptions renew monthly).
        fn next(&self, now: u64) -> Option<(u64, bool)> {
            if let Some(t) = self.next_pay.filter(|&t| t > now) {
                return Some((t, true));
            }
            if self.on_time == 0 || self.on_time > now {
                return None;
            }
            const MONTH: u64 = 30 * 86_400;
            let k = (now - self.on_time) / MONTH + 1;
            Some((self.on_time + k * MONTH, false))
        }
    }

    static SUBS: Mutex<Option<Vec<Sub>>> = Mutex::new(None);
    static LINKS_M: Mutex<Option<BTreeMap<String, Link>>> = Mutex::new(None);

    pub fn set_subs(all: Vec<Sub>) {
        *SUBS.lock().unwrap_or_else(|e| e.into_inner()) = Some(all);
    }
    fn subs() -> Option<Vec<Sub>> {
        SUBS.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn with_links<R>(f: impl FnOnce(&mut BTreeMap<String, Link>) -> R) -> R {
        let mut g = LINKS_M.lock().unwrap_or_else(|e| e.into_inner());
        let m = g.get_or_insert_with(|| std::fs::read(LINKS).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default());
        f(m)
    }
    fn save_links(m: &BTreeMap<String, Link>) {
        let tmp = format!("{LINKS}.tmp");
        if std::fs::write(&tmp, serde_json::to_vec_pretty(m).unwrap_or_default()).is_ok() {
            let _ = std::fs::rename(&tmp, LINKS);
        }
    }

    /// uid → Boosty id, the approved links.
    pub fn approved() -> Vec<(String, i64)> {
        with_links(|m| m.iter().filter(|(_, l)| l.ok).map(|(u, l)| (u.clone(), l.boosty_id)).collect())
    }

    pub fn list_links(state: &AppState) -> String {
        with_links(|m| {
            m.iter()
                .map(|(uid, l)| {
                    let n = state.directory.username_by_uid(uid).map(|n| format!("@{n}")).unwrap_or_else(|| format!("ID {uid}"));
                    format!("{n} — Boosty «{}» ({}){}", l.name, l.boosty_id, if l.ok { "" } else { " · waiting for you" })
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
    }

    /// Take the link away (and the star it gave; an e-mail one comes back
    /// with the next sync).
    pub fn unlink(uid: &str) -> bool {
        let had = with_links(|m| {
            let had = m.remove(uid).is_some();
            save_links(m);
            had
        });
        if had {
            with_stars(|m| {
                if m.get(uid).is_some_and(|s| s.source == "boosty") {
                    m.remove(uid);
                    save(m);
                }
            });
        }
        had
    }

    fn boosty_on() -> bool {
        ["AQUA_BOOSTY_TOKEN", "AQUA_BOOSTY_BLOG"].iter().all(|k| std::env::var(k).is_ok_and(|v| !v.trim().is_empty()))
    }

    /// The owner said yes or no to a link.
    pub async fn decide(state: &AppState, owner: &str, uid: &str, yes: bool) {
        let name = state.directory.username_by_uid(uid).unwrap_or_default();
        let link = with_links(|m| {
            let l = m.get(uid).cloned()?;
            if yes {
                m.insert(uid.to_string(), Link { ok: true, ..l.clone() });
            } else {
                m.remove(uid);
            }
            save_links(m);
            Some(l)
        });
        let Some(link) = link else { return say(state, owner, "That request is gone already.").await };
        if !yes {
            say(state, owner, &format!("Not linked: @{name} and Boosty «{}».", link.name)).await;
            if !name.is_empty() {
                say_app(state, &name, &format!("✦ The owner didn't confirm Boosty «{}» as yours.", link.name)).await;
            }
            return;
        }
        // The star at once, from what the last sync saw.
        let tier = subs().and_then(|s| s.iter().find(|x| x.id == link.boosty_id).map(|x| x.tier));
        if let Some(t) = tier {
            with_stars(|m| {
                let keep = m.get(uid).is_some_and(|s| s.source == "manual" && s.live(now()));
                let higher = m.get(uid).is_some_and(|s| s.source == "boosty" && s.kind >= t);
                if !keep && !higher {
                    m.insert(uid.to_string(), Star { kind: t, source: "boosty".into(), at: now(), until: None });
                    save(m);
                }
            });
        }
        say(state, owner, &format!("Linked: @{name} ↔ Boosty «{}».", link.name)).await;
        if !name.is_empty() {
            let t = tier.map(|t| format!(" Your star: CeZeOs level {t}.")).unwrap_or_default();
            say_app(state, &name, &format!("✦ Boosty «{}» is linked to you.{t}", link.name)).await;
        }
    }

    #[derive(Deserialize)]
    pub struct Body {
        #[serde(default)]
        init: String,
        #[serde(default)]
        q: String,
    }

    type Res = Result<Json<serde_json::Value>, (StatusCode, String)>;

    fn who(state: &AppState, b: &Body) -> Result<(String, String), (StatusCode, String)> {
        if !has_key() {
            return Err((StatusCode::NOT_FOUND, "no stars here".into()));
        }
        let me = crate::tgapi::check_init(state, &bot_name(), &b.init).ok_or((StatusCode::UNAUTHORIZED, "reopen the app".into()))?;
        let uid = state.directory.uid_of(&me).filter(|x| !x.is_empty()).ok_or((StatusCode::NOT_FOUND, "no such account".into()))?;
        Ok((me, uid))
    }

    /// `GET /v1/stars-app` — the page.
    pub async fn page() -> impl IntoResponse {
        ([(axum::http::header::CACHE_CONTROL, "no-store")], Html(PAGE))
    }

    /// `POST /v1/stars-app/me` — my star, my subscription, my link.
    pub async fn me(State(state): State<AppState>, Json(b): Json<Body>) -> Res {
        let (me, uid) = who(&state, &b)?;
        let t = now();
        let star = with_stars(|m| m.get(&uid).filter(|s| s.live(t)).cloned());
        let link = with_links(|m| m.get(&uid).cloned());
        let email = state.directory.email_of(&me);
        let all = subs();
        let by_mail = all.as_ref().and_then(|a| email.as_ref().and_then(|e| a.iter().filter(|x| &x.email == e).max_by_key(|x| x.tier).cloned()));
        let by_link = all.as_ref().and_then(|a| link.as_ref().filter(|l| l.ok).and_then(|l| a.iter().find(|x| x.id == l.boosty_id).cloned()));
        let (sub, via) = match (by_link, by_mail) {
            (Some(l), Some(m)) if m.tier > l.tier => (Some(m), "email"),
            (Some(l), _) => (Some(l), "link"),
            (None, Some(m)) => (Some(m), "email"),
            (None, None) => (None, ""),
        };
        let sub = sub.map(|s| {
            let next = s.next(t);
            serde_json::json!({
                "name": s.name, "level": s.level, "tier": s.tier, "price": s.price, "since": s.on_time,
                "next": next.map(|n| n.0), "exact": next.is_some_and(|n| n.1), "via": via,
            })
        });
        Ok(Json(serde_json::json!({
            "me": me,
            "owner": crate::verify::is_owner(&state, &me),
            "star": star.map(|s| serde_json::json!({ "level": s.kind, "source": s.source, "until": s.until })),
            "boosty": boosty_on(),
            "synced": all.is_some(),
            "blog": std::env::var("AQUA_BOOSTY_BLOG").ok().map(|b| format!("https://boosty.to/{}", b.trim())),
            "sub": sub,
            "link": link.map(|l| serde_json::json!({ "name": l.name, "id": l.boosty_id, "ok": l.ok })),
            "now": t,
        })))
    }

    /// `POST /v1/stars-app/link {q}` — "this Boosty account is mine": the
    /// owner gets a request with two buttons.
    pub async fn link(State(state): State<AppState>, Json(b): Json<Body>) -> Res {
        let (me, uid) = who(&state, &b)?;
        if !boosty_on() {
            return Err((StatusCode::SERVICE_UNAVAILABLE, "boosty-off".into()));
        }
        let all = subs().ok_or((StatusCode::SERVICE_UNAVAILABLE, "not-synced".into()))?;
        let q = b.q.trim().trim_start_matches('@').to_string();
        if q.is_empty() || q.chars().count() > 80 {
            return Err((StatusCode::BAD_REQUEST, "empty".into()));
        }
        let by_id: Vec<&Sub> = q.parse::<i64>().ok().map(|id| all.iter().filter(|x| x.id == id).collect()).unwrap_or_default();
        let found: Vec<&Sub> = if by_id.is_empty() { all.iter().filter(|x| x.name.eq_ignore_ascii_case(&q) || x.name.to_lowercase() == q.to_lowercase()).collect() } else { by_id };
        let sub = match found.as_slice() {
            [] => return Err((StatusCode::NOT_FOUND, "not-found".into())),
            [one] => (*one).clone(),
            _ => return Err((StatusCode::CONFLICT, "many".into())),
        };
        // One Boosty account, one person here.
        let taken = with_links(|m| m.iter().any(|(u, l)| u != &uid && l.ok && l.boosty_id == sub.id));
        if taken {
            return Err((StatusCode::CONFLICT, "taken".into()));
        }
        with_links(|m| {
            m.insert(uid.clone(), Link { boosty_id: sub.id, name: sub.name.clone(), at: now(), ok: false });
            save_links(m);
        });
        let text = format!(
            "✦ @{me} says Boosty «{}» is theirs: level {} «{}», {} ₽, since {}. Link it?",
            sub.name,
            sub.tier,
            sub.level,
            sub.price,
            if sub.on_time > 0 { fmt_date(sub.on_time) } else { "?".into() }
        );
        let row = serde_json::json!([{ "t": "✓ Link", "cb": format!("bl:ok:{uid}") }, { "t": "✕ Don't", "cb": format!("bl:no:{uid}") }]);
        for owner in crate::verify::owner_names(&state) {
            say_kb(&state, &owner, &text, row.clone()).await;
        }
        Ok(Json(serde_json::json!({ "ok": true, "name": sub.name })))
    }

    /// `POST /v1/stars-app/unlink`.
    pub async fn unlink_me(State(state): State<AppState>, Json(b): Json<Body>) -> Res {
        let (_, uid) = who(&state, &b)?;
        unlink(&uid);
        Ok(Json(serde_json::json!({ "ok": true })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn levels() {
        assert_eq!(migrate(0), 6);
        assert_eq!(migrate(3), 3);
        assert_eq!(migrate(8), 5);
        assert_eq!(parse_level(None), Ok(6));
        assert_eq!(parse_level(Some("2")), Ok(2));
        assert_eq!(parse_level(Some("0")), Ok(0));
        assert!(parse_level(Some("7")).is_err());
        assert!(parse_level(Some("x")).is_err());
        assert_eq!(parse_for("30d"), Some(30 * 86_400));
        assert_eq!(parse_for("12ч"), Some(12 * 3600));
        assert_eq!(parse_for("2w"), Some(14 * 86_400));
        assert_eq!(parse_for("3m"), Some(90 * 86_400));
        assert_eq!(parse_for("0d"), None);
        assert_eq!(parse_for("5x"), None);
        assert_eq!(fmt_date(0), "1970-01-01");
        assert_eq!(fmt_date(1_794_528_000), "2026-11-13");
        let b = serde_json::json!({ "name": "Ivan", "onTime": 1_000_000, "price": 300, "level": { "name": "Gold" } });
        let sub = app::Sub::from_boosty(&b, 7, 3, String::new());
        assert_eq!(sub.next_for_test(1_000_000 + 86_400), Some((1_000_000 + 30 * 86_400, false)));
        assert_eq!(sub.next_for_test(1_000_000 + 31 * 86_400), Some((1_000_000 + 60 * 86_400, false)));
        let b = serde_json::json!({ "onTime": 1_000_000, "nextPayTime": 5_000_000_000_000u64 });
        assert_eq!(app::Sub::from_boosty(&b, 7, 3, String::new()).next_for_test(2_000_000), Some((5_000_000_000, true)));
        let s = Star { kind: 3, source: "manual".into(), at: 0, until: Some(100) };
        assert!(s.live(99) && !s.live(100));
    }
}
