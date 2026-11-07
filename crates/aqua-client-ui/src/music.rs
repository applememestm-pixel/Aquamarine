//! Navidrome (any Subsonic server) in the app.
//!
//! Two sources, both optional:
//! - `server` — the Navidrome the relay offers (`/v1/music/*`, aqua-server
//!   music.rs): the relay adds its own login, the app signs in as itself;
//! - `own` — the user's own Navidrome: address, login and password kept in
//!   the encrypted vault (`music_own`), never sent to the relay.
//!
//! JSON calls go through Rust (the webview's CSP talks to no one), audio and
//! covers through the loopback media server (`media_server::remote_url`):
//! streamed chunk by chunk with `Range`, nothing written to disk.

use std::io::Read;
use std::sync::{Arc, Mutex as StdMutex};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::State;

use crate::media_server::{remote_url, Remote};
use crate::state::AppState;

const VAULT_KEY: &str = "music_own";
const API_VERSION: &str = "1.16.1";
const CLIENT: &str = "aquamarine";

/// What the own source may do (the relay's allows reading only).
const OWN_METHODS: &[&str] = &[
    "ping", "getLicense", "search3", "getAlbumList2", "getAlbum", "getArtists", "getArtist", "getArtistInfo2",
    "getAlbumInfo2", "getPlaylists", "getPlaylist", "getSong", "getRandomSongs", "getStarred2", "getGenres",
    "getSongsByGenre", "getTopSongs", "getSimilarSongs2", "getLyrics", "getLyricsBySongId", "getMusicDirectory",
    "getIndexes", "getMusicFolders", "scrobble", "star", "unstar",
];

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct Own {
    pub url: String,
    pub user: String,
    #[serde(default)]
    pub password: String,
}

fn own(state: &StdMutex<AppState>) -> Option<Own> {
    let s = state.lock().ok()?;
    let b = s.vault.get_record(VAULT_KEY).ok().flatten()?;
    serde_json::from_slice::<Own>(&b).ok().filter(|o| !o.url.is_empty())
}

fn transport(state: &StdMutex<AppState>) -> Result<Arc<aqua_client_core::transport::Transport>, String> {
    state.lock().map_err(|e| e.to_string())?.transport.clone().ok_or_else(|| "not connected".to_string())
}

/// Same tag as the relay's (aqua-server music.rs `tag_of`): which Navidrome a
/// track card belongs to.
pub fn tag_of(url: &str) -> String {
    use sha2::{Digest, Sha256};
    let u = url.trim().trim_end_matches('/').to_lowercase();
    let u = u.strip_prefix("https://").or_else(|| u.strip_prefix("http://")).unwrap_or(&u).to_string();
    hex::encode(&Sha256::digest(u.as_bytes())[..6])
}

fn auth(o: &Own) -> Vec<(String, String)> {
    let mut salt = [0u8; 8];
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut salt);
    let salt = hex::encode(salt);
    vec![
        ("u".into(), o.user.clone()),
        ("t".into(), hex::encode(md5(format!("{}{salt}", o.password).as_bytes()))),
        ("s".into(), salt),
        ("v".into(), API_VERSION.into()),
        ("c".into(), CLIENT.into()),
    ]
}

fn plain_client() -> reqwest::Client {
    static C: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    C.get_or_init(|| {
        aqua_client_core::transport::http_client().unwrap_or_else(|_| reqwest::Client::new())
    })
    .clone()
}

fn net_err(e: reqwest::Error) -> String {
    if e.is_connect() || e.is_timeout() { "can't reach Navidrome".into() } else { e.without_url().to_string() }
}

fn unwrap_subsonic(v: Value) -> Result<Value, String> {
    let r = v.get("subsonic-response").cloned().unwrap_or(v);
    if r.get("status").and_then(|s| s.as_str()) == Some("failed") {
        return Err(format!("Navidrome: {}", r.pointer("/error/message").and_then(|m| m.as_str()).unwrap_or("error")));
    }
    Ok(r)
}

async fn own_call(o: &Own, method: &str, params: &[(String, String)]) -> Result<Value, String> {
    let mut q = auth(o);
    q.push(("f".into(), "json".into()));
    q.extend(params.iter().cloned());
    let r = plain_client()
        .get(format!("{}/rest/{method}", o.url.trim_end_matches('/')))
        .query(&q)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .map_err(net_err)?;
    let status = r.status();
    let v: Value = r.json().await.map_err(|e| format!("Navidrome answered {status}: {}", e.without_url()))?;
    unwrap_subsonic(v)
}

// ------------------------------------------------------------------ commands

/// `{ server: {enabled, tag, bot} | null, own: {url, user, tag} | null }`.
#[tauri::command]
pub async fn music_status(state: State<'_, StdMutex<AppState>>) -> Result<Value, String> {
    let server = match transport(&state) {
        Ok(t) => {
            let url = format!("{}/v1/music", t.server_url().trim_end_matches('/'));
            match t.bearer().await {
                Ok(b) => match t.http().get(&url).bearer_auth(b).timeout(std::time::Duration::from_secs(10)).send().await {
                    Ok(r) if r.status().is_success() => r.json::<Value>().await.ok().filter(|v| v["enabled"].as_bool() == Some(true)),
                    _ => None,
                },
                Err(_) => None,
            }
        }
        Err(_) => None,
    };
    let own = own(&state).map(|o| json!({ "url": o.url, "user": o.user, "tag": tag_of(&o.url) }));
    Ok(json!({ "server": server, "own": own }))
}

/// Check and keep the user's own Navidrome. Empty password keeps the saved one.
#[tauri::command]
pub async fn music_set_own(state: State<'_, StdMutex<AppState>>, url: String, user: String, password: String) -> Result<Value, String> {
    let url = url.trim().trim_end_matches('/').to_string();
    let url = if url.starts_with("http://") || url.starts_with("https://") { url } else { format!("https://{url}") };
    let mut o = Own { url, user: user.trim().to_string(), password };
    if o.password.is_empty() {
        if let Some(old) = own(&state).filter(|old| old.url == o.url && old.user == o.user) {
            o.password = old.password;
        }
    }
    let r = own_call(&o, "ping", &[]).await?;
    {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.vault.put_record(VAULT_KEY, &serde_json::to_vec(&o).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    }
    Ok(json!({ "ok": true, "server": r.get("type"), "version": r.get("serverVersion"), "tag": tag_of(&o.url) }))
}

#[tauri::command]
pub fn music_clear_own(state: State<'_, StdMutex<AppState>>) -> Result<(), String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    s.vault.delete_record(VAULT_KEY).map_err(|e| e.to_string())
}

/// One Subsonic call; the answer without its `subsonic-response` wrapper.
#[tauri::command]
pub async fn music_api(
    state: State<'_, StdMutex<AppState>>,
    src: String,
    method: String,
    params: Option<std::collections::BTreeMap<String, String>>,
) -> Result<Value, String> {
    let params: Vec<(String, String)> = params.unwrap_or_default().into_iter().take(32).collect();
    if src == "own" {
        if !OWN_METHODS.contains(&method.as_str()) {
            return Err(format!("{method}: not allowed"));
        }
        let o = own(&state).ok_or("your Navidrome isn't set up")?;
        return own_call(&o, &method, &params).await;
    }
    if !method.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err("bad method".into());
    }
    let t = transport(&state)?;
    let url = format!("{}/v1/music/rest/{method}", t.server_url().trim_end_matches('/'));
    let r = t.http().get(&url).bearer_auth(t.bearer().await.map_err(|e| e.to_string())?).query(&params)
        .timeout(std::time::Duration::from_secs(25)).send().await.map_err(net_err)?;
    if !r.status().is_success() {
        let code = r.status().as_u16();
        let body = r.text().await.unwrap_or_default();
        return Err(if body.is_empty() { format!("HTTP {code}") } else { body });
    }
    unwrap_subsonic(r.json().await.map_err(|e| e.without_url().to_string())?)
}

/// How a request for bytes is made, at the moment the player asks.
#[derive(Clone)]
enum Fetch {
    Own(Own, &'static str, Vec<(String, String)>),
    Server(Arc<aqua_client_core::transport::Transport>, String, Vec<(String, String)>),
}

impl Fetch {
    fn new(state: &StdMutex<AppState>, src: &str, kind: &str, id: &str, size: Option<u32>) -> Result<Self, String> {
        let mut p: Vec<(String, String)> = vec![("id".into(), id.to_string())];
        if let Some(s) = size {
            p.push(("size".into(), s.clamp(32, 1200).to_string()));
        }
        let method: &'static str = match kind {
            "stream" => "stream",
            "download" => "download",
            "cover" => "getCoverArt",
            _ => return Err("bad kind".into()),
        };
        if src == "own" {
            Ok(Fetch::Own(own(state).ok_or("your Navidrome isn't set up")?, method, p))
        } else {
            let t = transport(state)?;
            let path = match kind { "cover" => "cover", "download" => "download", _ => "stream" };
            let size_q: Vec<(String, String)> = p.into_iter().filter(|(k, _)| k == "size").collect();
            Ok(Fetch::Server(t.clone(), format!("{}/v1/music/{path}/{}", t.server_url().trim_end_matches('/'), urlenc(id)), size_q))
        }
    }

    async fn send(&self, range: Option<String>) -> Result<reqwest::Response, String> {
        let req = match self {
            Fetch::Own(o, method, p) => {
                let mut q = auth(o);
                q.extend(p.iter().cloned());
                plain_client().get(format!("{}/rest/{method}", o.url.trim_end_matches('/'))).query(&q)
            }
            Fetch::Server(t, url, q) => t.http().get(url).query(q).bearer_auth(t.bearer().await.map_err(|e| e.to_string())?),
        };
        let req = match range { Some(r) => req.header(reqwest::header::RANGE, r), None => req };
        let r = req.send().await.map_err(net_err)?;
        let ct = r.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
        if !r.status().is_success() || ct.contains("json") || ct.contains("xml") {
            let code = r.status().as_u16();
            let body = r.text().await.unwrap_or_default();
            return Err(format!("HTTP {code}: {}", body.chars().take(200).collect::<String>()));
        }
        Ok(r)
    }
}

fn urlenc(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// Reads a reqwest body from a plain thread (the loopback server's).
struct BodyReader {
    resp: reqwest::Response,
    buf: Vec<u8>,
    pos: usize,
}

impl Read for BodyReader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        while self.pos >= self.buf.len() {
            match tauri::async_runtime::block_on(self.resp.chunk()) {
                Ok(Some(b)) => { self.buf = b.to_vec(); self.pos = 0; }
                Ok(None) => return Ok(0),
                Err(e) => return Err(std::io::Error::other(e.without_url().to_string())),
            }
        }
        let n = out.len().min(self.buf.len() - self.pos);
        out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

/// A loopback URL the `<audio>` / `<img>` can use: `kind` is "stream",
/// "download" or "cover".
#[tauri::command]
pub async fn music_url(
    state: State<'_, StdMutex<AppState>>,
    src: String,
    kind: String,
    id: String,
    size: Option<u32>,
) -> Result<String, String> {
    let f = Fetch::new(&state, &src, &kind, &id, size)?;
    let key = format!("{src}|{kind}|{id}|{}", size.unwrap_or(0));
    let ext = if kind == "cover" { "jpg" } else { "audio" };
    let fetch = Arc::new(f);
    remote_url(&key, ext, Arc::new(move |range: Option<String>| -> Result<Remote, String> {
        let f = fetch.clone();
        let resp = tauri::async_runtime::block_on(async move { f.send(range).await })?;
        let status = resp.status().as_u16();
        let mut headers = Vec::new();
        for h in ["content-type", "content-range", "accept-ranges", "last-modified", "etag"] {
            if let Some(v) = resp.headers().get(h).and_then(|v| v.to_str().ok()) {
                headers.push((h.to_string(), v.to_string()));
            }
        }
        if !headers.iter().any(|(k, _)| k == "accept-ranges") && kind_is_stream(&headers) {
            headers.push(("accept-ranges".into(), "bytes".into()));
        }
        headers.push(("cache-control".into(), "no-store".into()));
        let len = resp.content_length().map(|l| l as usize);
        Ok(Remote { status, headers, len, body: Box::new(BodyReader { resp, buf: Vec::new(), pos: 0 }) })
    }))
}

fn kind_is_stream(h: &[(String, String)]) -> bool {
    h.iter().any(|(k, v)| k == "content-type" && v.starts_with("audio/"))
}

/// Download a track into Downloads/Aquamarine; returns the file's path.
#[tauri::command]
pub async fn music_save(state: State<'_, StdMutex<AppState>>, src: String, id: String, name: String) -> Result<String, String> {
    let dir = {
        let s = state.lock().map_err(|e| e.to_string())?;
        crate::commands::downloads_dir(&s.data_dir)
    };
    let f = Fetch::new(&state, &src, "download", &id, None)?;
    let mut r = f.send(None).await?;
    let ext = r.headers().get(reqwest::header::CONTENT_DISPOSITION).and_then(|v| v.to_str().ok())
        .and_then(|d| d.rsplit('.').next()).map(|e| e.trim_matches(|c: char| !c.is_ascii_alphanumeric()).to_string())
        .filter(|e| !e.is_empty() && e.len() <= 5)
        .or_else(|| r.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).map(|ct| match ct {
            c if c.contains("mpeg") => "mp3".to_string(),
            c if c.contains("flac") => "flac".to_string(),
            c if c.contains("ogg") => "ogg".to_string(),
            c if c.contains("mp4") || c.contains("m4a") || c.contains("aac") => "m4a".to_string(),
            c if c.contains("wav") => "wav".to_string(),
            _ => "mp3".to_string(),
        }))
        .unwrap_or_else(|| "mp3".into());
    let safe: String = name.chars().map(|c| if "\\/:*?\"<>|".contains(c) || c.is_control() { '_' } else { c }).take(120).collect();
    let safe = if safe.trim().is_empty() { "track".to_string() } else { safe.trim().to_string() };
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut path = dir.join(format!("{safe}.{ext}"));
    let mut n = 1;
    while path.exists() {
        n += 1;
        path = dir.join(format!("{safe} ({n}).{ext}"));
    }
    let tmp = path.with_extension("part");
    {
        use std::io::Write;
        let mut file = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
        while let Some(chunk) = r.chunk().await.map_err(|e| e.without_url().to_string())? {
            file.write_all(&chunk).map_err(|e| e.to_string())?;
        }
    }
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().into_owned())
}

// ----------------------------------------------------------------------- md5

/// RFC 1321 — Subsonic's token auth (same as aqua-server music.rs).
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
        assert_eq!(hex::encode(md5(b"sesamec19b2d")), "26719a1196d2a940705a59634eb18eab");
    }

    #[test]
    fn tag_matches_server() {
        assert_eq!(tag_of("https://Music.example/"), tag_of("http://music.example"));
    }
}

/// `aqua.fetch` of a plugin (ui/src/lib/plugins.ts): the app checked that the
/// host is one the user allowed for that plugin. Text answers up to 2 MB.
#[tauri::command]
pub async fn plugin_fetch(
    url: String,
    method: String,
    headers: Option<std::collections::BTreeMap<String, String>>,
    body: Option<String>,
) -> Result<Value, String> {
    let u = reqwest::Url::parse(&url).map_err(|e| e.to_string())?;
    if !matches!(u.scheme(), "http" | "https") {
        return Err("http(s) only".into());
    }
    let m = reqwest::Method::from_bytes(method.to_uppercase().as_bytes()).map_err(|e| e.to_string())?;
    let mut req = plain_client().request(m, u).timeout(std::time::Duration::from_secs(20));
    for (k, v) in headers.unwrap_or_default().into_iter().take(32) {
        let lk = k.to_ascii_lowercase();
        if matches!(lk.as_str(), "host" | "cookie" | "content-length") { continue; }
        req = req.header(k, v);
    }
    if let Some(b) = body {
        req = req.body(b);
    }
    let mut r = req.send().await.map_err(|e| e.without_url().to_string())?;
    let status = r.status().as_u16();
    let mut hs = serde_json::Map::new();
    for (k, v) in r.headers() {
        if let Ok(v) = v.to_str() { hs.insert(k.as_str().to_string(), json!(v)); }
    }
    let mut buf: Vec<u8> = Vec::new();
    while let Some(c) = r.chunk().await.map_err(|e| e.without_url().to_string())? {
        if buf.len() + c.len() > 2 * 1024 * 1024 {
            return Err("the answer is larger than 2 MB".into());
        }
        buf.extend_from_slice(&c);
    }
    Ok(json!({ "status": status, "headers": hs, "body": String::from_utf8_lossy(&buf) }))
}
