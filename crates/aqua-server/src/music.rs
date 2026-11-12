//! Navidrome (any Subsonic server) behind the relay.
//!
//! The operator puts a Navidrome address and an account into Aquamarine
//! Server → Music (or `AQUA_NAVIDROME_URL` / `_USER` / `_PASSWORD`). Signed-in
//! users then browse and play it through `/v1/music/*`:
//!
//! - `rest/<method>` — read-only Subsonic calls (search, albums, playlists…),
//!   the relay adds its own credentials, which therefore never reach anyone;
//! - `stream/<id>`, `download/<id>`, `cover/<id>` — passed straight through,
//!   `Range` included: nothing is stored or cached on the relay.
//!
//! With the bot switched on the relay also runs `@music`: write it a song or
//! an artist and it answers with track cards (`[track:v1]{…}`) that the app
//! plays with its own player.

use axum::{
    body::Body,
    extract::{Path as UrlPath, Query, RawQuery, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{admin_authed, AppState, Caller};

const CONFIG_FILE: &str = "aqua-navidrome.json";
const API_VERSION: &str = "1.16.1";
const CLIENT: &str = "aquamarine";
/// The relay's own bot: `@music` unless `AQUA_MUSIC_BOT` names another.
pub fn bot_name() -> String {
    std::env::var("AQUA_MUSIC_BOT").ok().map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()).unwrap_or_else(|| "music".into())
}

/// Subsonic calls a user may make through the relay: reading only.
const READ_METHODS: &[&str] = &[
    "ping", "getLicense", "search3", "getAlbumList2", "getAlbum", "getArtists", "getArtist", "getArtistInfo2",
    "getAlbumInfo2", "getPlaylists", "getPlaylist", "getSong", "getRandomSongs", "getStarred2", "getGenres",
    "getSongsByGenre", "getTopSongs", "getSimilarSongs2", "getLyrics", "getLyricsBySongId", "getMusicDirectory",
    "getIndexes", "getMusicFolders",
];

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub password: String,
    /// Run `@music`.
    #[serde(default = "yes")]
    pub bot: bool,
}
fn yes() -> bool { true }

fn env(k: &str) -> Option<String> {
    std::env::var(k).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

fn from_env() -> bool { env("AQUA_NAVIDROME_URL").is_some() }

/// Env vars win, else the file saved from the admin panel.
pub fn config() -> Config {
    let mut c: Config = std::fs::read(CONFIG_FILE).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Config { bot: true, ..Default::default() });
    if let Some(u) = env("AQUA_NAVIDROME_URL") {
        c.url = u;
        c.user = env("AQUA_NAVIDROME_USER").unwrap_or_default();
        c.password = env("AQUA_NAVIDROME_PASSWORD").unwrap_or_default();
        if let Some(b) = env("AQUA_MUSIC_BOT_ENABLED") { c.bot = b != "0" && b != "false"; }
    }
    c.url = c.url.trim().trim_end_matches('/').to_string();
    c
}

pub fn enabled() -> bool {
    let c = config();
    !c.url.is_empty() && !c.user.is_empty()
}

/// A short tag of the Navidrome address: a track card from another relay
/// with the same Navidrome plays here too, others are shown without sound.
pub fn tag_of(url: &str) -> String {
    use sha2::{Digest, Sha256};
    let u = url.trim().trim_end_matches('/').to_lowercase();
    let u = u.strip_prefix("https://").or_else(|| u.strip_prefix("http://")).unwrap_or(&u).to_string();
    hex::encode(&Sha256::digest(u.as_bytes())[..6])
}

// ------------------------------------------------------------------ Subsonic

/// `u`, `t`, `s`, `v`, `c` for one request (token auth: md5(password + salt)).
fn auth_params(c: &Config) -> Vec<(String, String)> {
    let mut salt = [0u8; 8];
    rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut salt);
    let salt = hex::encode(salt);
    let token = hex::encode(md5(format!("{}{salt}", c.password).as_bytes()));
    vec![
        ("u".into(), c.user.clone()),
        ("t".into(), token),
        ("s".into(), salt),
        ("v".into(), API_VERSION.into()),
        ("c".into(), CLIENT.into()),
    ]
}

fn client() -> reqwest::Client {
    static C: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    C.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(8))
            .user_agent(concat!("aqua-server/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_default()
    })
    .clone()
}

/// The user's query minus anything that names an account.
fn clean_query(raw: Option<&str>) -> Vec<(String, String)> {
    let raw = raw.unwrap_or("");
    url_pairs(raw)
        .into_iter()
        .filter(|(k, _)| !matches!(k.as_str(), "u" | "p" | "t" | "s" | "v" | "c" | "f" | "apiKey" | "callback"))
        .take(32)
        .collect()
}

fn url_pairs(raw: &str) -> Vec<(String, String)> {
    raw.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (pct_decode(k), pct_decode(v))
        })
        .collect()
}

fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => { out.push(b' '); i += 1; }
            b'%' if i + 2 < b.len() => {
                match u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16) {
                    Ok(v) => { out.push(v); i += 3; }
                    Err(_) => { out.push(b'%'); i += 1; }
                }
            }
            c => { out.push(c); i += 1; }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// One JSON call to Navidrome; `subsonic-response` unwrapped, errors as text.
pub async fn call(c: &Config, method: &str, params: &[(String, String)]) -> Result<Value, String> {
    if c.url.is_empty() {
        return Err("Navidrome isn't set up on this server".into());
    }
    let mut q = auth_params(c);
    q.push(("f".into(), "json".into()));
    q.extend(params.iter().cloned());
    let r = client()
        .get(format!("{}/rest/{method}", c.url))
        .query(&q)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .map_err(net_err)?;
    let status = r.status();
    let v: Value = r.json().await.map_err(|e| format!("Navidrome answered {status}: {}", e.without_url()))?;
    let resp = v.get("subsonic-response").cloned().unwrap_or(Value::Null);
    if resp.get("status").and_then(|s| s.as_str()) != Some("ok") {
        let msg = resp.pointer("/error/message").and_then(|m| m.as_str()).unwrap_or("unexpected answer");
        return Err(format!("Navidrome: {msg}"));
    }
    Ok(resp)
}

/// Never the URL: it carries the credentials.
fn net_err(e: reqwest::Error) -> String {
    if e.is_connect() || e.is_timeout() {
        "can't reach Navidrome from this server".into()
    } else {
        format!("Navidrome: {}", e.without_url())
    }
}

// --------------------------------------------------------------------- admin

#[derive(Serialize)]
pub struct AdminView {
    pub url: String,
    pub user: String,
    pub has_password: bool,
    pub bot: bool,
    pub bot_name: String,
    pub from_env: bool,
}

fn view() -> AdminView {
    let c = config();
    AdminView { url: c.url, user: c.user, has_password: !c.password.is_empty(), bot: c.bot, bot_name: bot_name(), from_env: from_env() }
}

pub async fn admin_get(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<AdminView>, StatusCode> {
    admin_authed(&state, &headers)?;
    Ok(Json(view()))
}

#[derive(Deserialize)]
pub struct AdminSet {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub user: String,
    /// Empty keeps the saved one.
    #[serde(default)]
    pub password: String,
    #[serde(default = "yes")]
    pub bot: bool,
}

pub async fn admin_set(State(state): State<AppState>, headers: HeaderMap, Json(b): Json<AdminSet>) -> Result<Json<Value>, (StatusCode, String)> {
    admin_authed(&state, &headers).map_err(|c| (c, String::new()))?;
    if from_env() {
        return Err((StatusCode::CONFLICT, "Navidrome comes from AQUA_NAVIDROME_URL; change it there".into()));
    }
    let url = b.url.trim().trim_end_matches('/').to_string();
    if !url.is_empty() && !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err((StatusCode::BAD_REQUEST, "the address starts with http:// or https://".into()));
    }
    let old = config();
    let c = Config {
        password: if b.password.is_empty() && url == old.url && b.user.trim() == old.user { old.password } else { b.password },
        url,
        user: b.user.trim().to_string(),
        bot: b.bot,
    };
    std::fs::write(CONFIG_FILE, serde_json::to_vec_pretty(&c).unwrap_or_default())
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let test = if c.url.is_empty() { Value::Null } else { test_json(&c).await };
    if c.bot && !c.url.is_empty() {
        ensure_bot(&state);
    }
    Ok(Json(json!({ "view": view(), "test": test })))
}

pub async fn admin_test(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<Value>, StatusCode> {
    admin_authed(&state, &headers)?;
    Ok(Json(test_json(&config()).await))
}

async fn test_json(c: &Config) -> Value {
    match call(c, "ping", &[]).await {
        Ok(r) => {
            let songs = call(c, "getAlbumList2", &[("type".into(), "newest".into()), ("size".into(), "1".into())]).await.is_ok();
            json!({ "ok": true, "server": r.get("type").and_then(|t| t.as_str()).unwrap_or("subsonic"),
                    "version": r.get("serverVersion").and_then(|t| t.as_str()).unwrap_or(""), "albums": songs })
        }
        Err(e) => json!({ "ok": false, "error": e }),
    }
}

// ---------------------------------------------------------------------- users

/// What the app needs to know: is there music here, and which Navidrome.
pub async fn info(_caller: Caller) -> Json<Value> {
    let c = config();
    let on = !c.url.is_empty() && !c.user.is_empty();
    Json(json!({
        "enabled": on,
        "tag": if on { tag_of(&c.url) } else { String::new() },
        "bot": if on && c.bot { Some(bot_name()) } else { None },
    }))
}

pub async fn rest(_caller: Caller, UrlPath(method): UrlPath<String>, RawQuery(q): RawQuery) -> Result<Json<Value>, (StatusCode, String)> {
    let method = method.trim_end_matches(".view");
    if !READ_METHODS.contains(&method) {
        return Err((StatusCode::FORBIDDEN, format!("{method}: not available through the server")));
    }
    if !enabled() {
        return Err((StatusCode::NOT_FOUND, "Navidrome isn't set up on this server".into()));
    }
    call(&config(), method, &clean_query(q.as_deref())).await.map(Json).map_err(|e| (StatusCode::BAD_GATEWAY, e))
}

#[derive(Deserialize)]
pub struct CoverQ {
    #[serde(default)]
    pub size: Option<u32>,
}

pub async fn stream(_caller: Caller, UrlPath(id): UrlPath<String>, RawQuery(q): RawQuery, headers: HeaderMap) -> Response {
    let mut p: Vec<(String, String)> = clean_query(q.as_deref())
        .into_iter()
        .filter(|(k, _)| matches!(k.as_str(), "maxBitRate" | "format" | "timeOffset" | "estimateContentLength"))
        .collect();
    p.push(("id".into(), id));
    pass(&headers, "stream", p, None).await
}

pub async fn download(_caller: Caller, UrlPath(id): UrlPath<String>, headers: HeaderMap) -> Response {
    pass(&headers, "download", vec![("id".into(), id)], None).await
}

pub async fn cover(_caller: Caller, UrlPath(id): UrlPath<String>, Query(q): Query<CoverQ>, headers: HeaderMap) -> Response {
    let mut p = vec![("id".into(), id)];
    if let Some(s) = q.size {
        p.push(("size".into(), s.clamp(32, 1200).to_string()));
    }
    pass(&headers, "getCoverArt", p, Some("private, max-age=86400")).await
}

/// Bytes from Navidrome straight to the caller, chunk by chunk: the relay
/// keeps nothing. `Range` goes through, so seeking works.
async fn pass(headers: &HeaderMap, method: &str, params: Vec<(String, String)>, cache: Option<&str>) -> Response {
    if !enabled() {
        return (StatusCode::NOT_FOUND, "Navidrome isn't set up on this server").into_response();
    }
    let c = config();
    let mut q = auth_params(&c);
    q.extend(params);
    let mut req = client().get(format!("{}/rest/{method}", c.url)).query(&q);
    if let Some(r) = headers.get(header::RANGE).and_then(|v| v.to_str().ok()) {
        req = req.header(reqwest::header::RANGE, r);
    }
    let r = match req.send().await {
        Ok(r) => r,
        Err(e) => return (StatusCode::BAD_GATEWAY, net_err(e)).into_response(),
    };
    let status = StatusCode::from_u16(r.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let ct = r.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    // A Subsonic error comes back as JSON/XML with 200: say it plainly.
    if ct.contains("json") || ct.contains("xml") {
        let body = r.text().await.unwrap_or_default();
        let msg = body.split("message=\"").nth(1).and_then(|s| s.split('"').next())
            .or_else(|| body.split("\"message\":\"").nth(1).and_then(|s| s.split('"').next()))
            .unwrap_or("not found").to_string();
        return (StatusCode::NOT_FOUND, format!("Navidrome: {msg}")).into_response();
    }
    let mut out = Response::builder().status(status);
    for h in [reqwest::header::CONTENT_TYPE, reqwest::header::CONTENT_LENGTH, reqwest::header::CONTENT_RANGE,
              reqwest::header::ACCEPT_RANGES, reqwest::header::CONTENT_DISPOSITION, reqwest::header::LAST_MODIFIED, reqwest::header::ETAG] {
        if let Some(v) = r.headers().get(&h) {
            out = out.header(h.as_str(), v.as_bytes());
        }
    }
    if let Some(cc) = cache {
        out = out.header(header::CACHE_CONTROL, cc);
    }
    let body = futures_util::stream::unfold(r, |mut r| async move {
        match r.chunk().await {
            Ok(Some(b)) => Some((Ok::<_, std::io::Error>(b), r)),
            Ok(None) => None,
            Err(e) => Some((Err(std::io::Error::other(e.without_url().to_string())), r)),
        }
    });
    out.body(Body::from_stream(body)).unwrap_or_else(|_| StatusCode::BAD_GATEWAY.into_response())
}

// ------------------------------------------------------------------- @music

/// The relay's own bots (@music, @verify) belong to a fixed owner key nobody holds.
pub fn relay_bot_owner() -> [u8; 32] {
    bot_owner()
}

/// The bot belongs to the relay itself: a fixed owner key nobody holds.
fn bot_owner() -> [u8; 32] {
    use sha2::{Digest, Sha256};
    Sha256::digest(b"aquamarine relay: built-in bots").into()
}

/// Create (or keep) the bot account and its menu button "open the player".
pub(crate) fn ensure_bot(state: &AppState) {
    let name = bot_name();
    if !state.directory.is_bot(&name) {
        if let Err(e) = state.directory.register_bot(&name, &bot_owner(), "Music") {
            tracing::warn!(bot = %name, "can't create the music bot: {e}");
            return;
        }
        tracing::info!(bot = %name, "music bot created");
    }
    let st = state.clone();
    tokio::spawn(async move {
        let mut p = serde_json::Map::new();
        p.insert("menu_button".into(), json!({ "type": "web_app", "text": "Player", "web_app": { "url": "aqua:music" } }));
        let _ = crate::tgapi::dispatch(&st, &bot_name(), "setChatMenuButton", p).await;
    });
}

/// Started with the relay: answers `@music` while Navidrome and the bot are on.
pub fn spawn_bot(state: AppState) {
    tokio::spawn(async move {
        let mut made = false;
        loop {
            let c = config();
            if c.url.is_empty() || !c.bot {
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                continue;
            }
            if !made {
                ensure_bot(&state);
                made = true;
            }
            let q = state.bot_queue(&bot_name());
            let batch: Vec<crate::BotUpdate> = {
                let notify = q.lock().await.notify.clone();
                let wait = notify.notified();
                let mut g = q.lock().await;
                if g.updates.is_empty() {
                    drop(g);
                    let _ = tokio::time::timeout(std::time::Duration::from_secs(10), wait).await;
                    q.lock().await.updates.drain(..).collect()
                } else {
                    g.updates.drain(..).collect()
                }
            };
            for u in batch {
                if u.from_bot {
                    continue;
                }
                let st = state.clone();
                tokio::spawn(async move { answer(&st, &u).await });
            }
        }
    });
}

async fn say(state: &AppState, to: &str, text: &str) {
    crate::bot_deliver_text(state, &bot_name(), to, text).await;
}

async fn answer(state: &AppState, u: &crate::BotUpdate) {
    let text = u.text.trim();
    let c = config();
    if text.is_empty() {
        return;
    }
    if text.starts_with("/start") || text.starts_with("/help") {
        say(state, &u.from, "🎵 Write a song, an artist or an album — I'll find it on this server's Navidrome.\n\n/new — new albums\n/random — something random\n\nTap ▶ on a card to listen, ⬇ to download. The «Player» button opens the music player.").await;
        return;
    }
    let songs: Vec<Value> = if text.starts_with("/random") {
        match call(&c, "getRandomSongs", &[("size".into(), "6".into())]).await {
            Ok(r) => r.pointer("/randomSongs/song").and_then(|s| s.as_array()).cloned().unwrap_or_default(),
            Err(e) => { say(state, &u.from, &format!("⚠️ {e}")).await; return; }
        }
    } else if text.starts_with("/new") {
        match call(&c, "getAlbumList2", &[("type".into(), "newest".into()), ("size".into(), "6".into())]).await {
            Ok(r) => {
                let albums = r.pointer("/albumList2/album").and_then(|s| s.as_array()).cloned().unwrap_or_default();
                if albums.is_empty() {
                    say(state, &u.from, "No albums yet.").await;
                    return;
                }
                let lines: Vec<String> = albums.iter().map(|a| format!("💿 {} — {}", s(a, "name"), s(a, "artist"))).collect();
                say(state, &u.from, &format!("{}\n\nWrite an album's name to get its songs.", lines.join("\n"))).await;
                return;
            }
            Err(e) => { say(state, &u.from, &format!("⚠️ {e}")).await; return; }
        }
    } else {
        let q: String = text.chars().take(100).collect();
        match call(&c, "search3", &[("query".into(), q), ("songCount".into(), "8".into()), ("albumCount".into(), "0".into()), ("artistCount".into(), "0".into())]).await {
            Ok(r) => r.pointer("/searchResult3/song").and_then(|s| s.as_array()).cloned().unwrap_or_default(),
            Err(e) => { say(state, &u.from, &format!("⚠️ {e}")).await; return; }
        }
    };
    if songs.is_empty() {
        say(state, &u.from, "Nothing found 🤷").await;
        return;
    }
    let tag = tag_of(&c.url);
    for song in songs.iter().take(8) {
        say(state, &u.from, &format!("[track:v1]{}", card(song, &tag))).await;
    }
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("")
}

/// A Subsonic song → the app's track card.
pub fn card(song: &Value, tag: &str) -> Value {
    json!({
        "id": s(song, "id"),
        "title": s(song, "title"),
        "artist": s(song, "artist"),
        "album": s(song, "album"),
        "dur": song.get("duration").and_then(|d| d.as_u64()).unwrap_or(0),
        "cover": s(song, "coverArt"),
        "src": "server",
        "nd": tag,
    })
}

// ----------------------------------------------------------------------- md5

/// RFC 1321. Subsonic's token auth needs it and nothing else here does.
pub fn md5(input: &[u8]) -> [u8; 16] {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20,
        4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    let k: Vec<u32> = (0..64).map(|i| ((i as f64 + 1.0).sin().abs() * 4294967296.0) as u32).collect();
    let (mut a0, mut b0, mut c0, mut d0) = (0x67452301u32, 0xefcdab89u32, 0x98badcfeu32, 0x10325476u32);
    let mut msg = input.to_vec();
    let bits = (input.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bits.to_le_bytes());
    for chunk in msg.chunks(64) {
        let m: Vec<u32> = chunk.chunks(4).map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]])).collect();
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64 {
            let (f, g) = match i {
                0..=15 => ((b & c) | (!b & d), i),
                16..=31 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let f = f.wrapping_add(a).wrapping_add(k[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f.rotate_left(S[i]));
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }
    let mut out = [0u8; 16];
    out[..4].copy_from_slice(&a0.to_le_bytes());
    out[4..8].copy_from_slice(&b0.to_le_bytes());
    out[8..12].copy_from_slice(&c0.to_le_bytes());
    out[12..].copy_from_slice(&d0.to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn md5_vectors() {
        assert_eq!(hex::encode(md5(b"")), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(hex::encode(md5(b"abc")), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(hex::encode(md5(b"The quick brown fox jumps over the lazy dog")), "9e107d9d372bb6826bd81d3542a419d6");
        // Subsonic's own example: password "sesame", salt "c19b2d".
        assert_eq!(hex::encode(md5(b"sesamec19b2d")), "26719a1196d2a940705a59634eb18eab");
    }

    #[test]
    fn query_drops_credentials() {
        let q = clean_query(Some("query=a%20b&u=admin&t=x&songCount=5&p=enc:00"));
        assert_eq!(q, vec![("query".to_string(), "a b".to_string()), ("songCount".to_string(), "5".to_string())]);
    }

    #[test]
    fn tag_ignores_scheme_and_slash() {
        assert_eq!(tag_of("https://Music.example/"), tag_of("http://music.example"));
    }
}
