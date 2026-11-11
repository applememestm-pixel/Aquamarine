//! How this relay reaches Telegram (stickers from Telegram, "Sign in with
//! Telegram"): directly, through a proxy, or through another Bot API address.
//!
//! In some countries `api.telegram.org` is blocked and the relay got
//! "can't reach Telegram". The admin panel (Stickers → Telegram network) or
//! the environment sets it:
//!
//! - `AQUA_TG_PROXY` — an HTTP(S) proxy, e.g. `http://user:pass@host:3128`;
//! - `AQUA_TG_API` — another Bot API address: a local `telegram-bot-api`
//!   server, or a forwarding Worker (tools/report-worker: `…/tg`).

use axum::{extract::State, http::{HeaderMap, StatusCode}, Json};
use serde::{Deserialize, Serialize};

use crate::{admin_authed, AppState};

const FILE: &str = "aqua-telegram-net.json";
const DIRECT: &str = "https://api.telegram.org";

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Net {
    #[serde(default)]
    pub api: String,
    #[serde(default)]
    pub proxy: String,
}

fn env(k: &str) -> Option<String> {
    std::env::var(k).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

pub fn load() -> Net {
    let mut n: Net = std::fs::read(FILE).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    if let Some(a) = env("AQUA_TG_API") { n.api = a; }
    if let Some(p) = env("AQUA_TG_PROXY") { n.proxy = p; }
    n
}

/// The Bot API address, without a trailing slash.
pub fn base() -> String {
    let a = load().api;
    let a = a.trim().trim_end_matches('/');
    if a.is_empty() { DIRECT.into() } else { a.to_string() }
}

/// An HTTP client for Telegram, with the proxy if one is set.
pub fn client(timeout_secs: u64) -> reqwest::Client {
    let mut b = reqwest::Client::builder().timeout(std::time::Duration::from_secs(timeout_secs));
    let p = load().proxy;
    if !p.trim().is_empty() {
        if let Ok(px) = reqwest::Proxy::all(p.trim()) {
            b = b.proxy(px);
        }
    }
    b.build().unwrap_or_default()
}

/// Can we reach the Bot API this way? (Any HTTP answer counts: without a
/// token Telegram says 404, which still proves the way works.)
pub async fn test() -> Result<u128, String> {
    let t = std::time::Instant::now();
    match client(12).get(format!("{}/bot0/getMe", base())).send().await {
        Ok(_) => Ok(t.elapsed().as_millis()),
        Err(e) if e.is_connect() || e.is_timeout() => Err("no connection — Telegram is blocked in this network, set a proxy or another API address".into()),
        Err(e) => Err(e.without_url().to_string()),
    }
}

#[derive(Serialize)]
pub struct View {
    pub api: String,
    pub proxy: String,
    pub from_env: bool,
}

fn view() -> View {
    let n = load();
    // The proxy may carry a password: show only where it goes.
    let proxy = match reqwest::Url::parse(n.proxy.trim()) {
        Ok(u) if !n.proxy.trim().is_empty() => format!("{}://{}{}", u.scheme(), u.host_str().unwrap_or(""), u.port().map(|p| format!(":{p}")).unwrap_or_default()),
        _ => n.proxy.clone(),
    };
    View { api: n.api, proxy, from_env: env("AQUA_TG_API").is_some() || env("AQUA_TG_PROXY").is_some() }
}

pub async fn admin_get(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<serde_json::Value>, StatusCode> {
    admin_authed(&state, &headers)?;
    Ok(Json(serde_json::json!({ "net": view() })))
}

#[derive(Deserialize)]
pub struct Set {
    #[serde(default)]
    pub api: String,
    /// None keeps the saved one (it may hold a password the panel doesn't show).
    #[serde(default)]
    pub proxy: Option<String>,
}

pub async fn admin_set(State(state): State<AppState>, headers: HeaderMap, Json(s): Json<Set>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    admin_authed(&state, &headers).map_err(|c| (c, String::new()))?;
    let api = s.api.trim().trim_end_matches('/').to_string();
    if !api.is_empty() && !(api.starts_with("https://") || api.starts_with("http://")) {
        return Err((StatusCode::BAD_REQUEST, "the API address starts with https://".into()));
    }
    let mut n: Net = std::fs::read(FILE).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    n.api = api;
    if let Some(p) = s.proxy {
        let p = p.trim().to_string();
        if !p.is_empty() && reqwest::Proxy::all(&p).is_err() {
            return Err((StatusCode::BAD_REQUEST, "the proxy looks like http://host:port".into()));
        }
        n.proxy = p;
    }
    std::fs::write(FILE, serde_json::to_vec_pretty(&n).unwrap_or_default()).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let test = match test().await { Ok(ms) => serde_json::json!({ "ok": true, "ms": ms }), Err(e) => serde_json::json!({ "ok": false, "error": e }) };
    Ok(Json(serde_json::json!({ "net": view(), "test": test })))
}

pub async fn admin_test(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<serde_json::Value>, StatusCode> {
    admin_authed(&state, &headers)?;
    Ok(Json(match test().await { Ok(ms) => serde_json::json!({ "ok": true, "ms": ms }), Err(e) => serde_json::json!({ "ok": false, "error": e }) }))
}
