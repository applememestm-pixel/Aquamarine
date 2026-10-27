//! Things from the open web: link previews (OpenGraph / YouTube / direct
//! media), a small on-disk cache for remote images and videos, Telegraph
//! pages for the in-app reader, and opening a link in the system browser.
//!
//! Every request goes through `aqua_client_core::transport::http_client()`,
//! so the SOCKS5 proxy from Settings applies here too. Remote media is saved
//! under `<data_dir>/webcache/` and shown through the asset protocol — the
//! webview never talks to third-party hosts itself (strict CSP, and the
//! sites only see this client, not a browser fingerprint).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex as StdMutex;
use std::time::Duration;

use reqwest::header::{ACCEPT, ACCEPT_LANGUAGE, CONTENT_TYPE, USER_AGENT};
use reqwest::Url;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::State;

use crate::state::AppState;

/// Sites hand OpenGraph tags to link-preview bots; a plain browser UA often
/// gets a cookie wall or a JS shell instead.
const PREVIEW_UA: &str = "Mozilla/5.0 (compatible; AquamarineLinkPreview/0.1; like TwitterBot)";
const HTML_LIMIT: usize = 768 * 1024;
const IMAGE_LIMIT: u64 = 12 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Default)]
pub struct LinkPreview {
    pub url: String,
    /// "page" | "image" | "video" | "youtube" | "telegraph"
    pub kind: String,
    pub site: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    /// Local (cached) preview image.
    pub image_path: Option<String>,
    /// Direct video file to download & play inline.
    pub video_url: Option<String>,
    pub video_mime: Option<String>,
    pub video_size: Option<u64>,
    pub youtube_id: Option<String>,
    pub telegraph_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CachedMedia {
    pub path: String,
    pub mime: String,
    pub size: u64,
}

static PREVIEWS: StdMutex<Option<HashMap<String, LinkPreview>>> = StdMutex::new(None);

fn data_dir(state: &State<'_, StdMutex<AppState>>) -> Result<PathBuf, String> {
    Ok(state.lock().map_err(|e| e.to_string())?.data_dir.clone())
}

fn client() -> Result<reqwest::Client, String> {
    aqua_client_core::transport::http_client().map_err(|e| e.to_string())
}

fn parse_web_url(raw: &str) -> Result<Url, String> {
    let u = Url::parse(raw.trim()).map_err(|_| "not a valid link".to_string())?;
    match u.scheme() {
        "http" | "https" => Ok(u),
        _ => Err("only http(s) links are supported".into()),
    }
}

// ---------------------------------------------------------------- detection

/// YouTube video id from the usual URL shapes.
pub fn youtube_id(u: &Url) -> Option<String> {
    let host = u.host_str()?.trim_start_matches("www.").trim_start_matches("m.");
    let ok = |id: &str| id.len() == 11 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    let id = match host {
        "youtu.be" => u.path_segments()?.next()?.to_string(),
        "youtube.com" | "music.youtube.com" | "youtube-nocookie.com" => {
            let mut seg = u.path_segments()?;
            match seg.next()? {
                "watch" => u.query_pairs().find(|(k, _)| k == "v")?.1.to_string(),
                "shorts" | "embed" | "live" | "v" => seg.next()?.to_string(),
                _ => return None,
            }
        }
        _ => return None,
    };
    ok(&id).then_some(id)
}

/// `telegra.ph/Some-Title-10-12` → `Some-Title-10-12`.
pub fn telegraph_path(u: &Url) -> Option<String> {
    let host = u.host_str()?.trim_start_matches("www.");
    if !matches!(host, "telegra.ph" | "graph.org" | "te.legra.ph") {
        return None;
    }
    let p = u.path_segments()?.next()?.to_string();
    (!p.is_empty() && p != "file").then_some(p)
}

fn ext_for(mime: &str, url: &Url) -> String {
    let from_mime = match mime.split(';').next().unwrap_or("").trim() {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "image/avif" => "avif",
        "image/svg+xml" => "svg",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "video/quicktime" => "mov",
        "video/x-matroska" => "mkv",
        _ => "",
    };
    if !from_mime.is_empty() {
        return from_mime.into();
    }
    let last = url.path().rsplit('/').next().unwrap_or("");
    match last.rsplit_once('.') {
        Some((_, e)) if (1..=5).contains(&e.len()) && e.chars().all(|c| c.is_ascii_alphanumeric()) => e.to_ascii_lowercase(),
        _ => "bin".into(),
    }
}

fn mime_from_ext(url: &Url) -> Option<&'static str> {
    let p = url.path().to_ascii_lowercase();
    Some(match p.rsplit('.').next()? {
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "mkv" => "video/x-matroska",
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => return None,
    })
}

// ---------------------------------------------------------------- HTML meta

fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let end = rest.find(';').filter(|&e| e <= 10);
        let Some(end) = end else { out.push('&'); rest = &rest[1..]; continue };
        let ent = &rest[1..end];
        let ch = match ent {
            "amp" => Some('&'), "lt" => Some('<'), "gt" => Some('>'), "quot" => Some('"'),
            "apos" => Some('\''), "nbsp" => Some(' '), "mdash" => Some('—'), "ndash" => Some('–'),
            "laquo" => Some('«'), "raquo" => Some('»'), "hellip" => Some('…'),
            _ if ent.starts_with("#x") || ent.starts_with("#X") => u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32),
            _ if ent.starts_with('#') => ent[1..].parse::<u32>().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch {
            Some(c) => { out.push(c); rest = &rest[end + 1..]; }
            None => { out.push('&'); rest = &rest[1..]; }
        }
    }
    out.push_str(rest);
    out
}

/// Attributes of one `<meta ...>` tag body.
fn attrs(tag: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    let b = tag.as_bytes();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b'/') { i += 1; }
        let ks = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'=' && b[i] != b'>' { i += 1; }
        let key = tag[ks..i].to_ascii_lowercase();
        while i < b.len() && b[i].is_ascii_whitespace() { i += 1; }
        if i < b.len() && b[i] == b'=' {
            i += 1;
            while i < b.len() && b[i].is_ascii_whitespace() { i += 1; }
            let val = if i < b.len() && (b[i] == b'"' || b[i] == b'\'') {
                let q = b[i];
                i += 1;
                let vs = i;
                while i < b.len() && b[i] != q { i += 1; }
                let v = &tag[vs..i.min(tag.len())];
                i += 1;
                v
            } else {
                let vs = i;
                while i < b.len() && !b[i].is_ascii_whitespace() { i += 1; }
                &tag[vs..i]
            };
            if !key.is_empty() { m.insert(key, decode_entities(val)); }
        } else if key.is_empty() {
            i += 1;
        }
    }
    m
}

/// OpenGraph / Twitter / plain meta tags + `<title>`.
pub fn parse_meta(html: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let lower = html.to_ascii_lowercase();
    let mut pos = 0;
    while let Some(i) = lower[pos..].find("<meta") {
        let start = pos + i + 5;
        let Some(end) = lower[start..].find('>') else { break };
        let a = attrs(&html[start..start + end]);
        let key = a.get("property").or_else(|| a.get("name")).or_else(|| a.get("itemprop"));
        if let (Some(k), Some(v)) = (key, a.get("content")) {
            let k = k.to_ascii_lowercase();
            if !v.trim().is_empty() { out.entry(k).or_insert_with(|| v.trim().to_string()); }
        }
        pos = start + end;
    }
    if let Some(i) = lower.find("<title") {
        if let Some(gt) = lower[i..].find('>') {
            let s = i + gt + 1;
            if let Some(e) = lower[s..].find("</title") {
                let t = decode_entities(html[s..s + e].trim());
                if !t.is_empty() { out.insert("html:title".into(), t); }
            }
        }
    }
    out
}

fn clip(s: Option<&String>, n: usize) -> Option<String> {
    s.map(|v| {
        let v = v.split_whitespace().collect::<Vec<_>>().join(" ");
        if v.chars().count() > n { format!("{}…", v.chars().take(n).collect::<String>().trim_end()) } else { v }
    })
}

// ---------------------------------------------------------------- download

/// Download `url` into the web cache (once; later calls reuse the file).
async fn cache_url(dir: &Path, url: &Url, limit: u64) -> Result<CachedMedia, String> {
    let cache = dir.join("webcache");
    std::fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
    let key = hex::encode(&Sha256::digest(url.as_str().as_bytes())[..12]);
    if let Ok(rd) = std::fs::read_dir(&cache) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with(&key) && !name.ends_with(".part") {
                let mime = mime_from_ext(&Url::parse(&format!("file:///{name}")).unwrap()).unwrap_or("application/octet-stream");
                let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                return Ok(CachedMedia { path: e.path().to_string_lossy().into(), mime: mime.into(), size });
            }
        }
    }
    let resp = client()?
        .get(url.clone())
        .header(USER_AGENT, PREVIEW_UA)
        .timeout(Duration::from_secs(600))
        .send().await.map_err(|e| e.to_string())?
        .error_for_status().map_err(|e| e.to_string())?;
    if let Some(len) = resp.content_length() {
        if len > limit {
            return Err(format!("too large ({} MB)", len / 1024 / 1024));
        }
    }
    let mime = resp.headers().get(CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    let ext = ext_for(&mime, resp.url());
    let final_path = cache.join(format!("{key}.{ext}"));
    let part = cache.join(format!("{key}.{ext}.part"));
    let mut f = tokio::fs::File::create(&part).await.map_err(|e| e.to_string())?;
    let mut resp = resp;
    let mut got: u64 = 0;
    use tokio::io::AsyncWriteExt;
    while let Some(chunk) = resp.chunk().await.map_err(|e| e.to_string())? {
        got += chunk.len() as u64;
        if got > limit {
            drop(f);
            let _ = tokio::fs::remove_file(&part).await;
            return Err(format!("too large (over {} MB)", limit / 1024 / 1024));
        }
        f.write_all(&chunk).await.map_err(|e| e.to_string())?;
    }
    f.flush().await.map_err(|e| e.to_string())?;
    drop(f);
    tokio::fs::rename(&part, &final_path).await.map_err(|e| e.to_string())?;
    let mime = if mime.is_empty() { mime_from_ext(url).unwrap_or("application/octet-stream").to_string() } else { mime.split(';').next().unwrap_or("").trim().to_string() };
    Ok(CachedMedia { path: final_path.to_string_lossy().into(), mime, size: got })
}

async fn cache_image(dir: &Path, base: &Url, src: Option<&String>) -> Option<String> {
    let src = src?;
    let u = base.join(src).ok()?;
    if !matches!(u.scheme(), "http" | "https") { return None; }
    match cache_url(dir, &u, IMAGE_LIMIT).await {
        Ok(m) if m.mime.starts_with("image/") || m.mime == "application/octet-stream" => Some(m.path),
        Ok(_) => None,
        Err(e) => { tracing::debug!("preview image {u}: {e}"); None }
    }
}

// ---------------------------------------------------------------- previews

async fn build_preview(dir: &Path, url: Url) -> Result<LinkPreview, String> {
    let mut p = LinkPreview { url: url.to_string(), kind: "page".into(), ..Default::default() };
    p.site = url.host_str().map(|h| h.trim_start_matches("www.").to_string());

    if let Some(path) = telegraph_path(&url) {
        p.kind = "telegraph".into();
        p.telegraph_path = Some(path.clone());
        p.site = Some("Telegraph".into());
        if let Ok(page) = telegraph_fetch(&path, false).await {
            p.title = page.get("title").and_then(|v| v.as_str()).map(String::from);
            p.description = clip(page.get("description").and_then(|v| v.as_str()).map(String::from).as_ref(), 220);
            if let Some(img) = page.get("image_url").and_then(|v| v.as_str()) {
                let base = Url::parse("https://telegra.ph/").unwrap();
                p.image_path = cache_image(dir, &base, Some(&img.to_string())).await;
            }
        }
        return Ok(p);
    }

    if let Some(id) = youtube_id(&url) {
        p.kind = "youtube".into();
        p.site = Some("YouTube".into());
        p.youtube_id = Some(id.clone());
        let oembed = format!("https://www.youtube.com/oembed?format=json&url=https://www.youtube.com/watch?v={id}");
        if let Ok(r) = client()?.get(&oembed).timeout(Duration::from_secs(10)).send().await {
            if let Ok(v) = r.json::<serde_json::Value>().await {
                p.title = v.get("title").and_then(|x| x.as_str()).map(String::from);
                p.description = v.get("author_name").and_then(|x| x.as_str()).map(String::from);
            }
        }
        let base = Url::parse("https://i.ytimg.com/").unwrap();
        p.image_path = match cache_image(dir, &base, Some(&format!("/vi/{id}/maxresdefault.jpg"))).await {
            Some(x) => Some(x),
            None => cache_image(dir, &base, Some(&format!("/vi/{id}/hqdefault.jpg"))).await,
        };
        return Ok(p);
    }

    let resp = client()?
        .get(url.clone())
        .header(USER_AGENT, PREVIEW_UA)
        .header(ACCEPT, "text/html,application/xhtml+xml,*/*;q=0.8")
        .header(ACCEPT_LANGUAGE, "ru,en;q=0.8")
        .timeout(Duration::from_secs(12))
        .send().await.map_err(|e| e.to_string())?;
    let final_url = resp.url().clone();
    let mime = resp.headers().get(CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_ascii_lowercase();
    let size = resp.content_length();

    if mime.starts_with("video/") || (mime.is_empty() || mime.starts_with("application/octet-stream")) && mime_from_ext(&final_url).is_some_and(|m| m.starts_with("video/")) {
        p.kind = "video".into();
        p.video_url = Some(final_url.to_string());
        p.video_mime = Some(if mime.starts_with("video/") { mime.split(';').next().unwrap_or("").into() } else { mime_from_ext(&final_url).unwrap_or("video/mp4").into() });
        p.video_size = size;
        p.title = final_url.path().rsplit('/').next().map(|s| decode_entities(&s.replace("%20", " ")));
        return Ok(p);
    }
    if mime.starts_with("image/") {
        drop(resp);
        p.kind = "image".into();
        p.image_path = cache_image(dir, &final_url, Some(&final_url.to_string())).await;
        return Ok(p);
    }
    if !mime.contains("html") {
        return Ok(p);
    }

    // Read the head of the page only.
    let mut resp = resp;
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|e| e.to_string())? {
        buf.extend_from_slice(&chunk);
        if buf.len() > HTML_LIMIT || buf.windows(7).any(|w| w.eq_ignore_ascii_case(b"</head>")) { break; }
    }
    let html = String::from_utf8_lossy(&buf);
    let meta = parse_meta(&html);
    let get = |k: &str| meta.get(k);
    p.title = clip(get("og:title").or(get("twitter:title")).or(get("html:title")), 160);
    p.description = clip(get("og:description").or(get("twitter:description")).or(get("description")), 300);
    if let Some(s) = get("og:site_name") { p.site = Some(s.clone()); }
    let img = get("og:image:secure_url").or(get("og:image")).or(get("og:image:url")).or(get("twitter:image")).or(get("twitter:image:src"));
    p.image_path = cache_image(dir, &final_url, img).await;

    // A page that points at a plain video file (og:video) gets the inline player.
    let vtype = get("og:video:type").map(|s| s.to_ascii_lowercase()).unwrap_or_default();
    if let Some(v) = get("og:video:secure_url").or(get("og:video:url")).or(get("og:video")) {
        if let Ok(vu) = final_url.join(v) {
            let direct = vtype.starts_with("video/") || mime_from_ext(&vu).is_some_and(|m| m.starts_with("video/"));
            if direct {
                p.kind = "video".into();
                p.video_mime = Some(if vtype.starts_with("video/") { vtype } else { mime_from_ext(&vu).unwrap_or("video/mp4").into() });
                p.video_url = Some(vu.to_string());
            }
        }
    }
    Ok(p)
}

/// Preview card data for a link (cached for the session).
#[tauri::command]
pub async fn link_preview(state: State<'_, StdMutex<AppState>>, url: String) -> Result<LinkPreview, String> {
    let u = parse_web_url(&url)?;
    if let Some(p) = PREVIEWS.lock().ok().and_then(|g| g.as_ref().and_then(|m| m.get(u.as_str()).cloned())) {
        return Ok(p);
    }
    let dir = data_dir(&state)?;
    let p = build_preview(&dir, u.clone()).await?;
    if let Ok(mut g) = PREVIEWS.lock() {
        g.get_or_insert_with(HashMap::new).insert(u.to_string(), p.clone());
    }
    Ok(p)
}

/// Download a remote image / video into the web cache and return its path.
#[tauri::command]
pub async fn cache_remote_media(state: State<'_, StdMutex<AppState>>, url: String, max_mb: Option<u64>) -> Result<CachedMedia, String> {
    let u = parse_web_url(&url)?;
    let dir = data_dir(&state)?;
    let limit = max_mb.unwrap_or(100).clamp(1, 1024) * 1024 * 1024;
    cache_url(&dir, &u, limit).await
}

async fn telegraph_fetch(path: &str, content: bool) -> Result<serde_json::Value, String> {
    if path.is_empty() || path.contains('/') || path.contains('?') || path.contains('#') {
        return Err("bad Telegraph path".into());
    }
    let url = format!("https://api.telegra.ph/getPage/{path}?return_content={content}");
    let v: serde_json::Value = client()?
        .get(url)
        .timeout(Duration::from_secs(15))
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;
    if v.get("ok").and_then(|x| x.as_bool()) != Some(true) {
        return Err(v.get("error").and_then(|x| x.as_str()).unwrap_or("Telegraph error").to_string());
    }
    Ok(v.get("result").cloned().unwrap_or_default())
}

/// A Telegraph page (title, author, content nodes) for the in-app reader.
#[tauri::command]
pub async fn telegraph_page(path: String) -> Result<serde_json::Value, String> {
    telegraph_fetch(path.trim(), true).await
}

/// Open an http(s) / mailto link in the system browser / mail app.
#[tauri::command]
pub fn open_external(url: String) -> Result<(), String> {
    let u = Url::parse(url.trim()).map_err(|_| "not a valid link".to_string())?;
    if !matches!(u.scheme(), "http" | "https" | "mailto") {
        return Err("only web and mail links can be opened".into());
    }
    let s = u.to_string();
    #[cfg(target_os = "linux")]
    let r = std::process::Command::new("xdg-open").arg(&s).spawn();
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(&s).spawn();
    #[cfg(target_os = "windows")]
    let r = std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", &s]).spawn();
    #[cfg(target_os = "android")]
    return crate::relay_host::android::open_url(&s);
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows", target_os = "android")))]
    let r: std::io::Result<std::process::Child> = Err(std::io::Error::other("opening links is not supported here"));
    #[cfg(not(target_os = "android"))]
    r.map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn youtube_ids() {
        let id = |s: &str| youtube_id(&Url::parse(s).unwrap());
        assert_eq!(id("https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=1").as_deref(), Some("dQw4w9WgXcQ"));
        assert_eq!(id("https://youtu.be/dQw4w9WgXcQ?si=x").as_deref(), Some("dQw4w9WgXcQ"));
        assert_eq!(id("https://youtube.com/shorts/dQw4w9WgXcQ").as_deref(), Some("dQw4w9WgXcQ"));
        assert_eq!(id("https://m.youtube.com/watch?v=dQw4w9WgXcQ").as_deref(), Some("dQw4w9WgXcQ"));
        assert_eq!(id("https://youtube.com/@channel"), None);
        assert_eq!(id("https://example.com/watch?v=dQw4w9WgXcQ"), None);
    }

    #[test]
    fn telegraph_paths() {
        let p = |s: &str| telegraph_path(&Url::parse(s).unwrap());
        assert_eq!(p("https://telegra.ph/Hello-World-10-12").as_deref(), Some("Hello-World-10-12"));
        assert_eq!(p("https://graph.org/Test-01-01-2").as_deref(), Some("Test-01-01-2"));
        assert_eq!(p("https://telegra.ph/file/abc.jpg"), None);
        assert_eq!(p("https://example.com/Hello"), None);
    }

    #[test]
    fn meta_tags() {
        let html = r#"<html><head><title>Fallback &amp; title</title>
            <meta property="og:title" content="Hello &quot;world&quot;">
            <meta name='description' content='Plain description'>
            <META PROPERTY=og:image CONTENT=/img/a.png>
            <meta property="og:video:type" content="video/mp4" />
            </head><body>"#;
        let m = parse_meta(html);
        assert_eq!(m.get("og:title").unwrap(), "Hello \"world\"");
        assert_eq!(m.get("description").unwrap(), "Plain description");
        assert_eq!(m.get("og:image").unwrap(), "/img/a.png");
        assert_eq!(m.get("og:video:type").unwrap(), "video/mp4");
        assert_eq!(m.get("html:title").unwrap(), "Fallback & title");
    }

    /// Hits the network: `cargo test -p aqua-client-ui web::tests::live -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn live() {
        let dir = std::env::temp_dir().join("aqua-web-live");
        for u in [
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://telegra.ph/api",
            "https://github.com/applememestm-pixel/aquamarine",
            "https://download.samplelib.com/mp4/sample-5s.mp4",
        ] {
            let p = build_preview(&dir, Url::parse(u).unwrap()).await;
            println!("{u}\n  => {p:?}\n");
        }
        let page = telegraph_fetch("api", true).await.unwrap();
        println!("telegraph nodes: {}", page["content"].as_array().map(|a| a.len()).unwrap_or(0));
    }

    /// `AQUA_HTML=page.html cargo test -p aqua-client-ui web::tests::meta_file -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn meta_file() {
        for f in std::env::var("AQUA_HTML").unwrap().split(',') {
            let html = String::from_utf8_lossy(&std::fs::read(f).unwrap()).to_string();
            let m = parse_meta(&html);
            let mut keys: Vec<_> = m.iter().filter(|(k, _)| k.starts_with("og:") || k.starts_with("twitter:") || *k == "description" || *k == "html:title").collect();
            keys.sort();
            println!("== {f}");
            for (k, v) in keys { println!("  {k} = {}", v.chars().take(90).collect::<String>()); }
        }
    }

    #[test]
    fn entities() {
        assert_eq!(decode_entities("a &amp; b &#x41;&#66; &laquo;x&raquo; & c"), "a & b AB «x» & c");
    }
}
