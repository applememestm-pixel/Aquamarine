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
    c.url = clean_url(&c.url);
    c
}

/// The address as typed, fixed up: "http://https://host" (a pasted address
/// after a typed scheme) → "https://host", "host:4533" → "http://host:4533".
pub fn clean_url(s: &str) -> String {
    let mut u = s.trim().trim_end_matches('/');
    loop {
        let low = u.to_ascii_lowercase();
        let rest = if low.starts_with("http://") { &u[7..] } else if low.starts_with("https://") { &u[8..] } else { break };
        let rl = rest.to_ascii_lowercase();
        if rl.starts_with("http://") || rl.starts_with("https://") || rl.starts_with("http//") || rl.starts_with("https//") {
            u = rest;
        } else {
            break;
        }
    }
    let low = u.to_ascii_lowercase();
    if u.is_empty() || low.starts_with("http://") || low.starts_with("https://") {
        return u.to_string();
    }
    // "https//host" (a lost colon) or a bare "host:port".
    if let Some(r) = low.strip_prefix("https//") { return format!("https://{}", &u[u.len() - r.len()..]); }
    if let Some(r) = low.strip_prefix("http//") { return format!("http://{}", &u[u.len() - r.len()..]); }
    format!("http://{u}")
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

// ------------------------------------------------------- several Navidromes
//
// Every Navidrome of the network is one library here: search, albums, the
// @music bot ask all of them at once (in parallel) and merge the answers.
// What came from another relay carries its tag in its ids (`<tag>~<id>`),
// so a track, a cover or an album opened later goes to that relay; every
// song also says where it's from (`nd`, `srv` — shown on its card).

/// One Navidrome of the network: this relay's (`peer: None`) or a peer's.
#[derive(Clone, Debug)]
pub struct Source {
    pub tag: String,
    pub name: String,
    pub peer: Option<String>,
}

/// An address without its scheme: how a relay is named next to a track.
fn host_of(url: &str) -> String {
    let u = url.trim().trim_end_matches('/');
    u.strip_prefix("https://").or_else(|| u.strip_prefix("http://")).unwrap_or(u).to_string()
}

/// This relay's Navidrome as the network sees it (`/v1/fed/music/info`).
pub fn local_info() -> Option<Value> {
    if !enabled() { return None; }
    let me = crate::public::url();
    Some(json!({ "tag": tag_of(&config().url), "name": if me.is_empty() { "this server".to_string() } else { host_of(&me) } }))
}

static SOURCES: std::sync::Mutex<Option<(std::time::Instant, Vec<Source>)>> = std::sync::Mutex::new(None);

/// Every Navidrome reachable from here, this relay's first; asked of the
/// peers in parallel and remembered for a minute.
pub async fn sources(state: &AppState) -> Vec<Source> {
    if let Some((at, v)) = SOURCES.lock().ok().and_then(|g| g.clone()) {
        if at.elapsed() < std::time::Duration::from_secs(60) { return v; }
    }
    let mut out = Vec::new();
    if let Some(i) = local_info() {
        out.push(Source { tag: i["tag"].as_str().unwrap_or("").to_string(), name: i["name"].as_str().unwrap_or("").to_string(), peer: None });
    }
    if let Some(f) = crate::fed::current(state) {
        let asks = f.peer_list().into_iter().map(|p| {
            let f = f.clone();
            async move {
                let t = std::time::Duration::from_secs(6);
                if let Ok(Some(v)) = tokio::time::timeout(t, f.music_json_at(&p, "/v1/fed/music/info")).await {
                    let tag = v["tag"].as_str().unwrap_or("").to_string();
                    if !tag.is_empty() {
                        let name = v["name"].as_str().filter(|n| !n.is_empty()).map(str::to_string).unwrap_or_else(|| host_of(&p));
                        return Some(Source { tag, name, peer: Some(p) });
                    }
                }
                // An older relay: no info, but its Navidrome answers.
                match tokio::time::timeout(t, f.music_json_at(&p, "/v1/fed/music/rest/ping")).await {
                    Ok(Some(_)) => Some(Source { tag: format!("p{}", &tag_of(&p)[..8]), name: host_of(&p), peer: Some(p) }),
                    _ => None,
                }
            }
        });
        for s in futures_util::future::join_all(asks).await.into_iter().flatten() {
            // The same Navidrome behind two relays: once.
            if !out.iter().any(|o: &Source| o.tag == s.tag) { out.push(s); }
        }
    }
    if let Ok(mut g) = SOURCES.lock() { *g = Some((std::time::Instant::now(), out.clone())); }
    out
}

/// `<tag>~<id>` of a peer's source → (that source, the id there).
fn routed<'a>(srcs: &'a [Source], id: &'a str) -> Option<(&'a Source, &'a str)> {
    let (tag, rest) = id.split_once('~')?;
    srcs.iter().find(|s| s.peer.is_some() && s.tag == tag).map(|s| (s, rest))
}

/// A card made on another relay can carry THIS relay's tag (`<mine>~<id>`):
/// the id as this Navidrome knows it.
fn unlocal(id: &str) -> String {
    if enabled() {
        let mine = tag_of(&config().url);
        if let Some(rest) = id.strip_prefix(&format!("{mine}~")) { return rest.to_string(); }
    }
    id.to_string()
}

const ID_KEYS: &[&str] = &["id", "albumId", "artistId", "coverArt", "parent"];

/// Mark an answer as `src`'s: a peer's ids get its tag, every song / album /
/// artist says where it's from (`nd`, and `srv` when there are several).
fn stamp(v: &mut Value, src: &Source, label: bool) {
    match v {
        Value::Object(o) => {
            if src.peer.is_some() {
                for k in ID_KEYS {
                    if let Some(Value::String(x)) = o.get_mut(*k) {
                        if !x.is_empty() { *x = format!("{}~{x}", src.tag); }
                    }
                }
            }
            if o.contains_key("id") && (o.contains_key("title") || o.contains_key("name")) {
                o.insert("nd".into(), json!(src.tag));
                if label { o.insert("srv".into(), json!(src.name)); }
            }
            for x in o.values_mut() { stamp(x, src, label); }
        }
        Value::Array(a) => a.iter_mut().for_each(|x| stamp(x, src, label)),
        _ => {}
    }
}

/// Lists that are the same kind of thing on every Navidrome: asked of all.
const MERGED: &[&str] = &["search3", "search2", "getAlbumList2", "getAlbumList", "getRandomSongs", "getStarred2", "getStarred", "getPlaylists", "getArtists", "getIndexes", "getGenres"];

/// Put the lists of `more` after those of `base` (`{"searchResult3": {"song": [...]}}`);
/// the same song (title, artist, length) twice — once.
fn merge_into(base: &mut Value, more: Value) {
    let Value::Object(more) = more else { return };
    for (k, v) in more {
        let Value::Object(inner) = v else { continue };
        let slot = base.as_object_mut().map(|b| b.entry(k).or_insert_with(|| json!({})));
        let Some(Value::Object(slot)) = slot else { continue };
        for (k2, arr) in inner {
            let Value::Array(items) = arr else { continue };
            let e = slot.entry(k2.clone()).or_insert_with(|| json!([]));
            let Value::Array(have) = e else { continue };
            for it in items {
                let key = |x: &Value| (s(x, "title").to_lowercase(), s(x, "artist").to_lowercase(), x.get("duration").and_then(|d| d.as_u64()).unwrap_or(0));
                if k2 == "song" && !s(&it, "title").is_empty() && have.iter().any(|h| key(h) == key(&it)) { continue; }
                have.push(it);
            }
        }
    }
}

/// One Subsonic call across the network's Navidromes: routed by a tagged
/// id, merged for the lists, else this relay's (or the first peer's).
pub async fn call_all(state: &AppState, method: &str, params: Vec<(String, String)>) -> Result<Value, String> {
    let params: Vec<(String, String)> = params.into_iter().map(|(k, v)| if matches!(k.as_str(), "id" | "albumId" | "artistId") { let v = unlocal(&v); (k, v) } else { (k, v) }).collect();
    let srcs = sources(state).await;
    if srcs.is_empty() { return Err("Navidrome isn't set up on this server".into()); }
    let label = srcs.len() > 1;
    let ask = |src: Source, params: Vec<(String, String)>| {
        let state = state.clone();
        let method = method.to_string();
        async move {
            let mut v = match &src.peer {
                None => call(&config(), &method, &params).await?,
                Some(p) => {
                    let f = crate::fed::current(&state).ok_or("no network")?;
                    let qs = params.iter().map(|(k, v)| format!("{}={}", urlenc(k), urlenc(v))).collect::<Vec<_>>().join("&");
                    let path = if qs.is_empty() { format!("/v1/fed/music/rest/{method}") } else { format!("/v1/fed/music/rest/{method}?{qs}") };
                    tokio::time::timeout(std::time::Duration::from_secs(20), f.music_json_at(p, &path)).await.ok().flatten().ok_or_else(|| format!("{} doesn't answer", src.name))?
                }
            };
            fix_mojibake(&mut v);
            stamp(&mut v, &src, label);
            Ok::<Value, String>(v)
        }
    };
    // An id of another relay's: that relay alone, the id as it knows it.
    if let Some((src, _)) = params.iter().filter(|(k, _)| matches!(k.as_str(), "id" | "albumId" | "artistId")).find_map(|(_, v)| routed(&srcs, v)) {
        let src = src.clone();
        let local: Vec<(String, String)> = params.iter().map(|(k, v)| {
            let v = if matches!(k.as_str(), "id" | "albumId" | "artistId") { routed(&srcs, v).filter(|(s2, _)| s2.tag == src.tag).map(|(_, r)| r.to_string()).unwrap_or_else(|| v.clone()) } else { v.clone() };
            (k.clone(), v)
        }).collect();
        return ask(src, local).await;
    }
    if MERGED.contains(&method) && srcs.len() > 1 {
        let all = futures_util::future::join_all(srcs.iter().cloned().map(|src| ask(src, params.clone()))).await;
        let mut base: Option<Value> = None;
        let mut err = None;
        for r in all {
            match r {
                Ok(v) => match base.as_mut() { None => base = Some(v), Some(b) => merge_into(b, v) },
                Err(e) => err = Some(e),
            }
        }
        return base.ok_or_else(|| err.unwrap_or_default());
    }
    let first = srcs[0].clone();
    ask(first, params).await
}

// ------------------------------------------------------------ smart search
//
// Navidrome's search3 finds the words as typed. People type a band in the
// wrong keyboard layout ("ytdhs"), in translit ("nervy"), with extra words
// ("нервы песня про"), or a word of a title that sits in a file name
// ("01. Нервы - Критика"). So: several spellings at once, then the songs
// ranked by how much of the query they hold (artist and title first).

const EN: &str = "qwertyuiop[]asdfghjkl;'zxcvbnm,.`";
const RU: &str = "йцукенгшщзхъфывапролджэячсмитьбюё";

/// The same keys in the other layout (ghbdtn ↔ привет).
fn swap_layout(q: &str) -> String {
    let en: Vec<char> = EN.chars().collect();
    let ru: Vec<char> = RU.chars().collect();
    q.chars().map(|c| {
        let l = c.to_lowercase().next().unwrap_or(c);
        if let Some(i) = en.iter().position(|&x| x == l) { ru[i] } else if let Some(i) = ru.iter().position(|&x| x == l) { en[i] } else { c }
    }).collect()
}

const TR: &[(&str, &str)] = &[("щ", "shch"), ("ё", "yo"), ("ж", "zh"), ("ц", "ts"), ("ч", "ch"), ("ш", "sh"), ("ю", "yu"), ("я", "ya"), ("х", "kh"),
    ("а", "a"), ("б", "b"), ("в", "v"), ("г", "g"), ("д", "d"), ("е", "e"), ("з", "z"), ("и", "i"), ("й", "y"), ("к", "k"), ("л", "l"), ("м", "m"),
    ("н", "n"), ("о", "o"), ("п", "p"), ("р", "r"), ("с", "s"), ("т", "t"), ("у", "u"), ("ф", "f"), ("ъ", ""), ("ы", "y"), ("ь", ""), ("э", "e")];

/// Cyrillic → Latin (нервы → nervy).
fn to_latin(q: &str) -> String {
    let mut out = String::new();
    for c in q.to_lowercase().chars() {
        let cs = c.to_string();
        match TR.iter().find(|(r, _)| *r == cs) { Some((_, l)) => out.push_str(l), None => out.push(c) }
    }
    out
}

/// Latin → Cyrillic, longest letters first (shch, zh, ya…).
fn to_cyrillic(q: &str) -> String {
    // "y" reads as ы, "e" as е (not й / э): the usual way back.
    let mut pairs: Vec<(&str, &str)> = TR.iter().filter(|(r, l)| !l.is_empty() && *r != "й" && *r != "э").map(|(r, l)| (*l, *r)).collect();
    pairs.sort_by_key(|(l, _)| std::cmp::Reverse(l.len()));
    let q = q.to_lowercase();
    let mut out = String::new();
    let mut i = 0;
    'outer: while i < q.len() {
        for (l, r) in &pairs {
            if q[i..].starts_with(l) { out.push_str(r); i += l.len(); continue 'outer; }
        }
        let c = q[i..].chars().next().unwrap_or(' ');
        out.push(c);
        i += c.len_utf8();
    }
    out
}

fn words(s: &str) -> Vec<String> {
    s.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| w.chars().count() >= 2).map(str::to_string).collect()
}

/// How well a song answers the query (higher is better; 0 — not at all).
fn score(song: &Value, q: &str) -> u32 {
    let ql = q.trim().to_lowercase();
    let (title, artist, album) = (s(song, "title").to_lowercase(), s(song, "artist").to_lowercase(), s(song, "album").to_lowercase());
    let hay = format!("{title} {artist} {album}");
    let mut sc = 0;
    if artist == ql || title == ql { sc += 100; }
    if title.starts_with(&ql) || artist.starts_with(&ql) { sc += 40; }
    if hay.contains(&ql) { sc += 30; }
    let qw = words(&ql);
    let hw = words(&hay);
    let hits = qw.iter().filter(|w| hw.iter().any(|h| h.starts_with(w.as_str()))).count() as u32;
    sc += hits * 20;
    if !qw.is_empty() && hits as usize == qw.len() { sc += 25; }
    sc
}

/// Songs for what a person typed: every spelling of it on every Navidrome,
/// ranked, the same song once.
pub async fn smart_songs(state: &AppState, q: &str, limit: usize) -> Result<Vec<Value>, String> {
    let q = q.trim();
    let mut tries: Vec<String> = vec![q.to_string()];
    let mut add = |v: String| { let v = v.trim().to_string(); if v.chars().count() >= 2 && !tries.contains(&v) { tries.push(v); } };
    add(swap_layout(q));
    if q.chars().any(|c| ('а'..='я').contains(&c.to_lowercase().next().unwrap_or(c)) || c == 'ё') { add(to_latin(q)); } else { add(to_cyrillic(q)); }
    let ws = words(q);
    if ws.len() > 1 { for w in ws.iter().filter(|w| w.chars().count() >= 3).take(4) { add(w.clone()); } }
    tries.truncate(7);
    let asks = tries.iter().map(|t| call_all(state, "search3", vec![("query".into(), t.clone()), ("songCount".into(), "30".into()), ("albumCount".into(), "0".into()), ("artistCount".into(), "0".into())]));
    let mut seen = std::collections::HashSet::new();
    let mut songs: Vec<(u32, Value)> = Vec::new();
    let mut err = None;
    for (i, r) in futures_util::future::join_all(asks).await.into_iter().enumerate() {
        match r {
            Ok(v) => for song in v.pointer("/searchResult3/song").and_then(|x| x.as_array()).cloned().unwrap_or_default() {
                let key = format!("{}|{}|{}", s(&song, "title").to_lowercase(), s(&song, "artist").to_lowercase(), song.get("duration").and_then(|d| d.as_u64()).unwrap_or(0));
                if !seen.insert(key) { continue; }
                // Scored against the spelling that found it; the typed one first.
                let sc = score(&song, &tries[i]).max(score(&song, q)) + if i == 0 { 5 } else { 0 };
                songs.push((sc, song));
            },
            Err(e) => err = Some(e),
        }
    }
    if songs.is_empty() { if let Some(e) = err { if !e.contains("isn't set up") { return Err(e); } } }
    songs.sort_by(|a, b| b.0.cmp(&a.0));
    Ok(songs.into_iter().take(limit).map(|(_, v)| v).collect())
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

/// Percent-decoding for the network's query strings (fed.rs).
pub fn pct(s: &str) -> String { pct_decode(s) }

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
    let mut resp = v.get("subsonic-response").cloned().unwrap_or(Value::Null);
    fix_mojibake(&mut resp);
    if resp.get("status").and_then(|s| s.as_str()) != Some("ok") {
        let msg = resp.pointer("/error/message").and_then(|m| m.as_str()).unwrap_or("unexpected answer");
        return Err(format!("Navidrome: {msg}"));
    }
    Ok(resp)
}

/// Old Russian MP3s carry Windows-1251 tags that Navidrome reads as Latin-1:
/// «Сектор Газа» comes as «ÑÅÊÒÎÐ ÃÀÇÀ». Every string of the answer that
/// looks like that is read back as 1251.
pub fn fix_mojibake(v: &mut Value) {
    match v {
        Value::String(s) => if let Some(f) = from_1251(s) { *s = f },
        Value::Array(a) => a.iter_mut().for_each(fix_mojibake),
        Value::Object(o) => o.values_mut().for_each(fix_mojibake),
        _ => {}
    }
}

/// Windows-1251 punctuation read as Latin-1: «», dashes, quotes, №, …
/// (they used to make the whole tag "not cp1251").
fn cp1251_punct(c: u32) -> Option<char> {
    Some(match c {
        0x85 => '…', 0x91 => '‘', 0x92 => '’', 0x93 => '“', 0x94 => '”', 0x96 => '–', 0x97 => '—',
        0xA0 => '\u{a0}', 0xAB => '«', 0xBB => '»', 0xB9 => '№', 0xB0 => '°',
        _ => return None,
    })
}

fn from_1251(s: &str) -> Option<String> {
    let mut high = 0;
    let mut letters = 0;
    for ch in s.chars() {
        let c = ch as u32;
        if c > 0xFF { return None; }
        if c >= 0x80 && !(c == 0xA8 || c == 0xB8 || c >= 0xC0 || cp1251_punct(c).is_some()) { return None; }
        if c >= 0xC0 || c == 0xA8 || c == 0xB8 { high += 1; }
        if ch.is_alphabetic() { letters += 1; }
    }
    // «Motörhead», «Beyoncé»: a letter or two — real Latin-1, left alone.
    if high < 3 || high * 2 < letters { return None; }
    Some(s.chars().map(|ch| match ch as u32 {
        0xA8 => 'Ё',
        0xB8 => 'ё',
        c if cp1251_punct(c).is_some() => cp1251_punct(c).unwrap_or(ch),
        c @ 0xC0..=0xFF => char::from_u32(0x410 + c - 0xC0).unwrap_or(ch),
        _ => ch,
    }).collect())
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
    let url = clean_url(&b.url);
    if url.contains(char::is_whitespace) {
        return Err((StatusCode::BAD_REQUEST, "the address has spaces in it".into()));
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

/// `POST /v1/admin/music/test` — what's typed in the form (not saved yet),
/// or the saved settings when the form is empty. Before, only the saved ones:
/// "Check" before "Save" said "Navidrome isn't set up on this server".
pub async fn admin_test(State(state): State<AppState>, headers: HeaderMap, body: Option<Json<AdminSet>>) -> Result<Json<Value>, StatusCode> {
    admin_authed(&state, &headers)?;
    let old = config();
    let c = match body.map(|Json(b)| b).filter(|b| !b.url.trim().is_empty() && !from_env()) {
        Some(b) => {
            let url = b.url.trim().trim_end_matches('/').to_string();
            if !(url.starts_with("http://") || url.starts_with("https://")) {
                return Ok(Json(json!({ "ok": false, "error": "the address starts with http:// or https://" })));
            }
            let user = b.user.trim().to_string();
            // An empty password: the saved one, for the same address and login.
            let password = if b.password.is_empty() && url == old.url && user == old.user { old.password.clone() } else { b.password };
            Config { url, user, password, bot: b.bot }
        }
        None => old,
    };
    Ok(Json(test_json(&c).await))
}

async fn test_json(c: &Config) -> Value {
    match call(c, "ping", &[]).await {
        Ok(r) => {
            let songs = call(c, "getAlbumList2", &[("type".into(), "newest".into()), ("size".into(), "1".into())]).await.is_ok();
            json!({ "ok": true, "server": r.get("type").and_then(|t| t.as_str()).unwrap_or("subsonic"),
                    "version": r.get("serverVersion").and_then(|t| t.as_str()).unwrap_or(""), "albums": songs })
        }
        // Navidrome itself serves plain http (port 4533); https by an IP
        // address has no certificate to match — it looks unreachable.
        Err(e) if e.starts_with("can't reach") && c.url.starts_with("https://") => {
            json!({ "ok": false, "error": format!("{e} — Navidrome itself serves http://, not https:// (unless it's behind a proxy with a certificate)") })
        }
        Err(e) => json!({ "ok": false, "error": e }),
    }
}

// ---------------------------------------------------------------------- users

/// What the app needs to know: is there music here, and which Navidrome.
pub async fn info(State(state): State<AppState>, _caller: Caller) -> Json<Value> {
    let c = config();
    // Every Navidrome of the network (this relay's first): their cards play here.
    let srcs = sources(&state).await;
    let on = !srcs.is_empty();
    Json(json!({
        "enabled": on,
        // Older apps know one tag: this relay's, else the first peer's.
        "tag": srcs.first().map(|s| s.tag.clone()).unwrap_or_default(),
        "sources": srcs.iter().map(|s| json!({ "tag": s.tag, "name": s.name })).collect::<Vec<_>>(),
        "bot": if on && c.bot { Some(bot_name()) } else { None },
    }))
}

pub async fn rest(State(state): State<AppState>, _caller: Caller, UrlPath(method): UrlPath<String>, RawQuery(q): RawQuery) -> Result<Json<Value>, (StatusCode, String)> {
    let method = method.trim_end_matches(".view");
    if !READ_METHODS.contains(&method) {
        return Err((StatusCode::FORBIDDEN, format!("{method}: not available through the server")));
    }
    // This relay's Navidrome and the network's, as one.
    call_all(&state, method, clean_query(q.as_deref())).await.map(Json).map_err(|e| {
        let code = if e.contains("isn't set up") { StatusCode::NOT_FOUND } else { StatusCode::BAD_GATEWAY };
        (code, e)
    })
}

fn urlenc(s: &str) -> String {
    s.bytes().map(|b| match b {
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
        _ => format!("%{b:02X}"),
    }).collect()
}

/// This relay's Navidrome for a relay of the network (`fed.rs`): read-only.
pub async fn local_rest(method: &str, q: Option<&str>) -> Result<Value, String> {
    let method = method.trim_end_matches(".view");
    if !READ_METHODS.contains(&method) || !enabled() { return Err("no".into()); }
    call(&config(), method, &clean_query(q)).await
}

/// A cover or a track of this relay's Navidrome, whole, for another relay.
pub async fn local_file(what: &str, id: &str) -> Option<(String, Vec<u8>)> {
    if !enabled() || id.is_empty() || id.len() > 128 { return None; }
    let method = match what { "cover" => "getCoverArt", "stream" => "stream", "download" => "download", _ => return None };
    let c = config();
    let mut q = auth_params(&c);
    q.push(("id".into(), id.to_string()));
    let r = client().get(format!("{}/rest/{method}", c.url)).query(&q).timeout(std::time::Duration::from_secs(60)).send().await.ok()?;
    let ct = r.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("application/octet-stream").to_string();
    if ct.contains("json") || ct.contains("xml") { return None; }
    let b = r.bytes().await.ok()?;
    if b.len() > 60 * 1024 * 1024 { return None; }
    Some((ct, b.to_vec()))
}

/// A file of another relay's Navidrome (`<tag>~<id>`), or — no Navidrome
/// here — of the relay of the network that has one.
async fn via_network(state: &AppState, what: &str, id: &str) -> Option<Response> {
    let f = crate::fed::current(state)?;
    let srcs = sources(state).await;
    if let Some((src, rest)) = routed(&srcs, id) {
        let (mime, data) = f.fetch_bytes_at(src.peer.as_deref()?, &format!("/v1/fed/music/file/{what}/{}", urlenc(rest))).await?;
        return Some(([(header::CONTENT_TYPE, mime)], data).into_response());
    }
    // A local id that just has a "~" in it: this Navidrome's.
    if enabled() { return None; }
    let (mime, data) = f.fetch_bytes(&format!("/v1/fed/music/file/{what}/{}", urlenc(id))).await?;
    Some(([(header::CONTENT_TYPE, mime)], data).into_response())
}

#[derive(Deserialize)]
pub struct CoverQ {
    #[serde(default)]
    pub size: Option<u32>,
}

pub async fn stream(State(state): State<AppState>, _caller: Caller, UrlPath(id): UrlPath<String>, RawQuery(q): RawQuery, headers: HeaderMap) -> Response {
    let id = unlocal(&id);
    if !enabled() || id.contains('~') { if let Some(r) = via_network(&state, "stream", &id).await { return r; } }
    let mut p: Vec<(String, String)> = clean_query(q.as_deref())
        .into_iter()
        .filter(|(k, _)| matches!(k.as_str(), "maxBitRate" | "format" | "timeOffset" | "estimateContentLength"))
        .collect();
    p.push(("id".into(), id));
    pass(&headers, "stream", p, None).await
}

pub async fn download(State(state): State<AppState>, _caller: Caller, UrlPath(id): UrlPath<String>, headers: HeaderMap) -> Response {
    let id = unlocal(&id);
    if !enabled() || id.contains('~') { if let Some(r) = via_network(&state, "download", &id).await { return r; } }
    pass(&headers, "download", vec![("id".into(), id)], None).await
}

pub async fn cover(State(state): State<AppState>, _caller: Caller, UrlPath(id): UrlPath<String>, Query(q): Query<CoverQ>, headers: HeaderMap) -> Response {
    let id = unlocal(&id);
    if !enabled() || id.contains('~') { if let Some(r) = via_network(&state, "cover", &id).await { return r; } }
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
            // On while this relay — or another of the network — has music.
            if !c.bot || sources(&state).await.is_empty() {
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
    if text.is_empty() {
        return;
    }
    if text.starts_with("/start") || text.starts_with("/help") {
        say(state, &u.from, "🎵 Write a song, an artist or an album — I'll find it on every Navidrome of this server's network.\n\n/new — new albums\n/random — something random\n\nTap ▶ on a card to listen, ⬇ to download. The «Player» button opens the music player.").await;
        return;
    }
    let songs: Vec<Value> = if text.starts_with("/random") {
        match call_all(state, "getRandomSongs", vec![("size".into(), "6".into())]).await {
            Ok(r) => r.pointer("/randomSongs/song").and_then(|s| s.as_array()).cloned().unwrap_or_default(),
            Err(e) => { say(state, &u.from, &format!("⚠️ {e}")).await; return; }
        }
    } else if text.starts_with("/new") {
        match call_all(state, "getAlbumList2", vec![("type".into(), "newest".into()), ("size".into(), "6".into())]).await {
            Ok(r) => {
                let albums = r.pointer("/albumList2/album").and_then(|s| s.as_array()).cloned().unwrap_or_default();
                if albums.is_empty() {
                    say(state, &u.from, "No albums yet.").await;
                    return;
                }
                let lines: Vec<String> = albums.iter().map(|a| {
                    let from = s(a, "srv");
                    format!("💿 {} — {}{}", s(a, "name"), s(a, "artist"), if from.is_empty() { String::new() } else { format!(" · {from}") })
                }).collect();
                say(state, &u.from, &format!("{}\n\nWrite an album's name to get its songs.", lines.join("\n"))).await;
                return;
            }
            Err(e) => { say(state, &u.from, &format!("⚠️ {e}")).await; return; }
        }
    } else {
        let q: String = text.chars().take(100).collect();
        match smart_songs(state, &q, 10).await {
            Ok(v) => v,
            Err(e) => { say(state, &u.from, &format!("⚠️ {e}")).await; return; }
        }
    };
    if songs.is_empty() {
        say(state, &u.from, "Nothing found 🤷").await;
        return;
    }
    // Each card says which Navidrome it plays from (`nd`) and, with several, whose (`srv`).
    let local = if enabled() { tag_of(&config().url) } else { String::new() };
    for song in songs.iter().take(10) {
        let tag = song.get("nd").and_then(|t| t.as_str()).map(str::to_string).unwrap_or_else(|| local.clone());
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
        "srv": s(song, "srv"),
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
    fn smart_search_spellings() {
        assert_eq!(swap_layout("ytdhs"), "нервы");
        assert_eq!(swap_layout("нервы"), "ytdhs");
        assert_eq!(to_latin("Нервы"), "nervy");
        assert_eq!(to_cyrillic("nervy"), "нервы");
        assert_eq!(to_cyrillic("shchuka"), "щука");
        let song = json!({ "title": "01. Нервы - Критика", "artist": "[Unknown Artist]", "album": "" });
        assert!(score(&song, "нервы критика") > score(&song, "кино"));
        assert_eq!(score(&song, "кино"), 0);
    }

    #[test]
    fn cp1251_tags_are_read_back() {
        assert_eq!(from_1251("ÑÅÊÒÎÐ ÃÀÇÀ").as_deref(), Some("СЕКТОР ГАЗА"));
        assert_eq!(from_1251("Ñåêòîð Ãàçà - Ëèðèê").as_deref(), Some("Сектор Газа - Лирик"));
        // cp1251 «», dashes and № read as Latin-1 come back too.
        assert_eq!(from_1251("\u{ab}Ñåêòîð Ãàçà\u{bb} \u{96} Ëèðèê \u{b9}1").as_deref(), Some("«Сектор Газа» – Лирик №1"));
        assert_eq!(from_1251("\u{ab}Le Café\u{bb}"), None);
        assert_eq!(from_1251("Motörhead"), None);
        assert_eq!(from_1251("Beyoncé"), None);
        assert_eq!(from_1251("Rob Zombie"), None);
        assert_eq!(from_1251("Кино"), None);
    }

    #[test]
    fn addresses_are_fixed_up() {
        assert_eq!(clean_url("http://https://1.2.3.4:4533/"), "https://1.2.3.4:4533");
        assert_eq!(clean_url("https://http://nd.local"), "http://nd.local");
        assert_eq!(clean_url("http://https//nd.local"), "https://nd.local");
        assert_eq!(clean_url(" 127.0.0.1:4533 "), "http://127.0.0.1:4533");
        assert_eq!(clean_url("HTTP://Host:4533"), "HTTP://Host:4533");
        assert_eq!(clean_url(""), "");
    }

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
