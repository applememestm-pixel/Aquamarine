//! A tiny loopback HTTP server for playing local video / audio.
//!
//! Why: Tauri's asset protocol answers a request without a `Range` header by
//! reading the *whole* file into memory and handing it to the webview, and
//! on Linux WebKitGTK feeds custom-scheme responses through the UI thread.
//! A multi-hundred-MB video then freezes the window. Over plain HTTP the
//! webview's media stack streams with real range requests in its own
//! network process.
//!
//! Safety: bound to 127.0.0.1 only; every file gets an unguessable random
//! token (128 bit) and only registered files are served — nothing else on
//! disk is reachable, by other local programs or by web pages.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use rand::RngCore;
use tiny_http::{Header, Method, Response, Server, StatusCode};

struct State {
    port: u16,
    by_token: Mutex<HashMap<String, PathBuf>>,
    by_path: Mutex<HashMap<PathBuf, String>>,
}

static STATE: OnceLock<Result<State, String>> = OnceLock::new();

fn state() -> Result<&'static State, String> {
    STATE
        .get_or_init(|| {
            let server = Server::http("127.0.0.1:0").map_err(|e| format!("media server: {e}"))?;
            let port = server.server_addr().to_ip().map(|a| a.port()).ok_or("media server: no port")?;
            // A few workers so a seek doesn't wait for another stream.
            let server = std::sync::Arc::new(server);
            for _ in 0..4 {
                let srv = server.clone();
                std::thread::Builder::new()
                    .name("aqua-media-http".into())
                    .spawn(move || {
                        for req in srv.incoming_requests() {
                            serve(req);
                        }
                    })
                    .map_err(|e| e.to_string())?;
            }
            Ok(State { port, by_token: Mutex::new(HashMap::new()), by_path: Mutex::new(HashMap::new()) })
        })
        .as_ref()
        .map_err(|e| e.clone())
}

pub fn mime_of(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "mkv" => "video/x-matroska",
        "avi" => "video/x-msvideo",
        "ogv" => "video/ogg",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "aac" => "audio/aac",
        "wav" => "audio/wav",
        "ogg" | "oga" => "audio/ogg",
        "opus" => "audio/ogg",
        "flac" => "audio/flac",
        "weba" => "audio/webm",
        _ => return None,
    })
}

/// URL that streams `path` (a video / audio file) over loopback HTTP.
pub fn url_for(path: &Path) -> Result<String, String> {
    if mime_of(path).is_none() {
        return Err("not a video or audio file".into());
    }
    let path = path.canonicalize().map_err(|e| format!("{}: {e}", path.display()))?;
    if !path.is_file() {
        return Err("not a file".into());
    }
    let st = state()?;
    let token = {
        let mut by_path = st.by_path.lock().map_err(|e| e.to_string())?;
        if let Some(t) = by_path.get(&path) {
            t.clone()
        } else {
            let mut b = [0u8; 16];
            rand::rngs::OsRng.fill_bytes(&mut b);
            let t = hex::encode(b);
            by_path.insert(path.clone(), t.clone());
            st.by_token.lock().map_err(|e| e.to_string())?.insert(t.clone(), path.clone());
            t
        }
    };
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("bin");
    Ok(format!("http://127.0.0.1:{}/m/{token}.{ext}", st.port))
}

fn header(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).expect("valid header")
}

fn serve(req: tiny_http::Request) {
    let respond_err = |req: tiny_http::Request, code: u16| {
        let _ = req.respond(Response::empty(StatusCode(code)).with_header(header("Access-Control-Allow-Origin", "*")));
    };
    if !matches!(req.method(), Method::Get | Method::Head) {
        return respond_err(req, 405);
    }
    // /m/<32 hex>.<ext>
    let token = req.url().strip_prefix("/m/").and_then(|r| r.split('.').next()).unwrap_or("").to_string();
    if token.len() != 32 || !token.chars().all(|c| c.is_ascii_hexdigit()) {
        return respond_err(req, 404);
    }
    let path = match STATE.get().and_then(|s| s.as_ref().ok()).and_then(|s| s.by_token.lock().ok()?.get(&token).cloned()) {
        Some(p) => p,
        None => return respond_err(req, 404),
    };
    let mut file = match File::open(&path) {
        Ok(f) => f,
        Err(_) => return respond_err(req, 404),
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let mime = mime_of(&path).unwrap_or("application/octet-stream");

    // Range: bytes=start-end | bytes=start- | bytes=-suffix
    let range = req
        .headers()
        .iter()
        .find(|h| h.field.equiv("Range"))
        .and_then(|h| parse_range(h.value.as_str(), len));
    let head = *req.method() == Method::Head;
    let common = |r: Response<Box<dyn Read + Send>>| {
        r.with_header(header("Content-Type", mime))
            .with_header(header("Accept-Ranges", "bytes"))
            .with_header(header("Access-Control-Allow-Origin", "*"))
            .with_header(header("Cache-Control", "no-store"))
    };
    match range {
        Some((start, end)) => {
            if file.seek(SeekFrom::Start(start)).is_err() {
                return respond_err(req, 416);
            }
            let n = end - start + 1;
            let body: Box<dyn Read + Send> = if head { Box::new(std::io::empty()) } else { Box::new(file.take(n)) };
            let resp = common(Response::new(StatusCode(206), vec![], body, Some(n as usize), None))
                .with_header(header("Content-Range", &format!("bytes {start}-{end}/{len}")));
            let _ = req.respond(resp);
        }
        None if req.headers().iter().any(|h| h.field.equiv("Range")) => {
            let _ = req.respond(
                Response::empty(StatusCode(416))
                    .with_header(header("Content-Range", &format!("bytes */{len}")))
                    .with_header(header("Access-Control-Allow-Origin", "*")),
            );
        }
        None => {
            let body: Box<dyn Read + Send> = if head { Box::new(std::io::empty()) } else { Box::new(file) };
            let _ = req.respond(common(Response::new(StatusCode(200), vec![], body, Some(len as usize), None)));
        }
    }
}

/// `bytes=a-b` → inclusive (start, end), clamped to the file.
pub fn parse_range(v: &str, len: u64) -> Option<(u64, u64)> {
    if len == 0 {
        return None;
    }
    let spec = v.trim().strip_prefix("bytes=")?.split(',').next()?.trim();
    let (a, b) = spec.split_once('-')?;
    let (start, end) = match (a.trim(), b.trim()) {
        ("", suf) => {
            let n: u64 = suf.parse().ok()?;
            if n == 0 { return None; }
            (len.saturating_sub(n), len - 1)
        }
        (s, "") => (s.parse().ok()?, len - 1),
        (s, e) => (s.parse().ok()?, e.parse::<u64>().ok()?.min(len - 1)),
    };
    (start <= end && start < len).then_some((start, end))
}

/// Streaming URL for a local video / audio file (picked by the user, a chat
/// attachment, or something in the app's caches).
#[tauri::command]
pub fn media_stream_url(path: String) -> Result<String, String> {
    url_for(&PathBuf::from(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn ranges() {
        assert_eq!(parse_range("bytes=0-99", 1000), Some((0, 99)));
        assert_eq!(parse_range("bytes=500-", 1000), Some((500, 999)));
        assert_eq!(parse_range("bytes=-100", 1000), Some((900, 999)));
        assert_eq!(parse_range("bytes=900-5000", 1000), Some((900, 999)));
        assert_eq!(parse_range("bytes=1000-", 1000), None);
        assert_eq!(parse_range("bytes=5-2", 1000), None);
        assert_eq!(parse_range("items=0-1", 1000), None);
    }

    #[test]
    fn serves_registered_files_with_ranges_only() {
        let dir = std::env::temp_dir().join(format!("aqua-ms-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("clip.mp4");
        std::fs::File::create(&f).unwrap().write_all(&(0u8..=255).cycle().take(5000).collect::<Vec<u8>>()).unwrap();
        let url = url_for(&f).unwrap();
        assert!(url.starts_with("http://127.0.0.1:"));
        assert_eq!(url, url_for(&f).unwrap(), "same file → same URL");

        let get = |u: &str, range: Option<&str>| -> (u16, Vec<u8>, String) {
            let rest = u.strip_prefix("http://").unwrap();
            let (host, path) = rest.split_once('/').unwrap();
            let mut s = std::net::TcpStream::connect(host).unwrap();
            let r = range.map(|r| format!("Range: {r}\r\n")).unwrap_or_default();
            write!(s, "GET /{path} HTTP/1.1\r\nHost: {host}\r\n{r}Connection: close\r\n\r\n").unwrap();
            let mut buf = Vec::new();
            s.read_to_end(&mut buf).unwrap();
            let split = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
            let head = String::from_utf8_lossy(&buf[..split]).to_string();
            let code: u16 = head.split_whitespace().nth(1).unwrap().parse().unwrap();
            (code, buf[split + 4..].to_vec(), head)
        };
        let (c, body, head) = get(&url, Some("bytes=10-19"));
        assert_eq!(c, 206);
        assert_eq!(body, (10u8..20).collect::<Vec<u8>>());
        assert!(head.contains("bytes 10-19/5000"), "{head}");
        let (c, body, _) = get(&url, None);
        assert_eq!(c, 200);
        assert_eq!(body.len(), 5000);
        // Unknown token / other paths are not served.
        let port = url.split(':').nth(2).unwrap().split('/').next().unwrap();
        let (c, _, _) = get(&format!("http://127.0.0.1:{port}/m/00000000000000000000000000000000.mp4"), None);
        assert_eq!(c, 404);
        let (c, _, _) = get(&format!("http://127.0.0.1:{port}/etc/passwd"), None);
        assert_eq!(c, 404);
        assert!(url_for(&dir.join("notes.txt")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
