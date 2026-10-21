//! Optional static website next to the API.
//!
//! With `AQUA_SITE_DIR` pointing at a folder (the Docker image sets it to
//! `/srv/site`: the project site plus its Markdown documents), every path
//! the API doesn't use is served from there: `/` → `index.html`,
//! `/docs.html`, `/docs/api.md`, … Without it the relay answers 404 as before.

use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

use axum::{
    body::Body,
    http::{header, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
};

fn root() -> Option<&'static PathBuf> {
    static ROOT: OnceLock<Option<PathBuf>> = OnceLock::new();
    ROOT.get_or_init(|| {
        let dir = std::env::var("AQUA_SITE_DIR").ok().filter(|d| !d.trim().is_empty())?;
        let dir = PathBuf::from(dir).canonicalize().ok()?;
        if dir.join("index.html").is_file() {
            tracing::info!(path = %dir.display(), "serving the website");
            Some(dir)
        } else {
            tracing::warn!(path = %dir.display(), "AQUA_SITE_DIR has no index.html — website off");
            None
        }
    })
    .as_ref()
}

/// Whether a website is configured (used for the startup banner).
pub fn enabled() -> bool {
    root().is_some()
}

fn mime(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase().as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "md" => "text/markdown; charset=utf-8",
        "txt" => "text/plain; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        _ => "application/octet-stream",
    }
}

/// Maps a URL path to a file inside the site folder; `None` for anything
/// that tries to leave it or hide behind dot-files.
fn resolve(root: &Path, url_path: &str) -> Option<PathBuf> {
    let decoded = percent_decode(url_path)?;
    let mut rel = PathBuf::new();
    for comp in Path::new(decoded.trim_start_matches('/')).components() {
        match comp {
            Component::Normal(c) if !c.to_string_lossy().starts_with('.') => rel.push(c),
            Component::CurDir => {}
            _ => return None,
        }
    }
    let mut path = root.join(rel);
    if path.is_dir() {
        path.push("index.html");
    }
    let path = path.canonicalize().ok()?;
    (path.starts_with(root) && path.is_file()).then_some(path)
}

fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn not_found() -> Response {
    (StatusCode::NOT_FOUND, "Not Found").into_response()
}

/// Serves `url_path` from the site folder (GET and HEAD only).
pub async fn serve(method: &Method, url_path: &str) -> Response {
    let Some(root) = root() else { return not_found() };
    if method != Method::GET && method != Method::HEAD {
        return not_found();
    }
    let Some(path) = resolve(root, url_path) else {
        // Unknown page → the front page's 404 would be nicer, but a plain 404
        // keeps API clients' errors honest.
        return not_found();
    };
    let Ok(bytes) = tokio::fs::read(&path).await else { return not_found() };
    let ct = mime(&path);
    // Pages and documents change with every update; assets can be cached briefly.
    let cache = if ct.starts_with("text/html") || ct.starts_with("text/markdown") { "no-cache" } else { "public, max-age=3600" };
    let body = if method == Method::HEAD { Body::empty() } else { Body::from(bytes) };
    Response::builder()
        .header(header::CONTENT_TYPE, ct)
        .header(header::CACHE_CONTROL, cache)
        .header("x-content-type-options", "nosniff")
        .body(body)
        .unwrap_or_else(|_| not_found())
}

/// Router fallback: everything the API doesn't claim.
pub async fn fallback(method: Method, uri: Uri) -> Response {
    serve(&method, uri.path()).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_stays_inside() {
        let dir = std::env::temp_dir().join(format!("aqua-site-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        std::fs::write(dir.join("index.html"), "<h1>hi</h1>").unwrap();
        std::fs::write(dir.join("docs/api.md"), "# API").unwrap();
        std::fs::write(dir.join(".secret"), "x").unwrap();
        let root = dir.canonicalize().unwrap();
        assert_eq!(resolve(&root, "/").unwrap(), root.join("index.html"));
        assert_eq!(resolve(&root, "/docs/api.md").unwrap(), root.join("docs/api.md"));
        assert_eq!(resolve(&root, "/docs/%61pi.md").unwrap(), root.join("docs/api.md"));
        assert!(resolve(&root, "/../etc/passwd").is_none());
        assert!(resolve(&root, "/docs/%2e%2e/%2e%2e/etc/passwd").is_none());
        assert!(resolve(&root, "/.secret").is_none());
        assert!(resolve(&root, "/missing.html").is_none());
        std::fs::remove_dir_all(&dir).ok();
    }
}
