//! Federation: several relays act as one network.
//!
//! Peers share a secret (`AQUA_FED_SECRET` / admin panel) and list each
//! other (`AQUA_FED_PEERS`). Every relay-to-relay request is signed:
//!
//! ```text
//! x-aqua-fed: <unix ms>.<hex HMAC-SHA256(secret, METHOD \n PATH \n ms \n SHA256(body))>
//! ```
//!
//! What they do together:
//! * accounts and encrypted backups replicate (pull every 30 s, newest
//!   row wins), so any relay can sign a user in;
//! * an envelope for someone connected elsewhere is forwarded there; if
//!   nobody has them online it waits where it was sent, and the relay they
//!   connect to next collects it from its peers;
//! * prekey bundles, encrypted files and profile media are fetched from a
//!   peer when the local relay doesn't have them;
//! * `GET /v1/fed/peers` lists the public addresses, so clients can switch
//!   to a live relay when theirs goes down.

use std::sync::Arc;
use std::time::Duration;

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{AppState, RelayMessage};

const FILE: &str = "aqua-federation.json";
const SKEW_MS: i64 = 5 * 60 * 1000;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FedConfig {
    #[serde(default)]
    pub secret: String,
    #[serde(default)]
    pub peers: Vec<String>,
    /// How clients reach THIS relay (shown to them as a fallback).
    #[serde(default)]
    pub public_url: String,
}

impl FedConfig {
    pub fn load() -> Self {
        let mut c: FedConfig = std::fs::read(FILE).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        if let Ok(s) = std::env::var("AQUA_FED_SECRET") {
            if !s.trim().is_empty() { c.secret = s.trim().to_string(); }
        }
        if let Ok(p) = std::env::var("AQUA_FED_PEERS") {
            let list: Vec<String> = p.split([',', ' ']).map(|x| x.trim().trim_end_matches('/').to_string()).filter(|x| !x.is_empty()).collect();
            if !list.is_empty() { c.peers = list; }
        }
        if let Ok(u) = std::env::var("AQUA_PUBLIC_URL") {
            if !u.trim().is_empty() { c.public_url = u.trim().trim_end_matches('/').to_string(); }
        }
        c.peers.retain(|p| !p.is_empty() && p.trim_end_matches('/') != c.public_url);
        c
    }
    pub fn save(&self) -> std::io::Result<()> {
        std::fs::write(FILE, serde_json::to_vec_pretty(self).unwrap_or_default())
    }
    pub fn active(&self) -> bool {
        self.secret.len() >= 16 && !self.peers.is_empty()
    }
}

pub struct Fed {
    pub cfg: FedConfig,
    http: reqwest::Client,
}

pub type FedSlot = Arc<std::sync::RwLock<Option<Arc<Fed>>>>;

pub fn slot_from_config() -> FedSlot {
    Arc::new(std::sync::RwLock::new(Fed::new(FedConfig::load())))
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn mac(secret: &str, method: &str, path: &str, ts: i64, body: &[u8]) -> String {
    let msg = format!("{method}\n{path}\n{ts}\n{}", hex::encode(Sha256::digest(body)));
    hex::encode(aqua_core::crypto::hmac_sha256(secret.as_bytes(), msg.as_bytes()))
}

impl Fed {
    pub fn new(cfg: FedConfig) -> Option<Arc<Self>> {
        if !cfg.active() {
            return None;
        }
        let http = reqwest::Client::builder().timeout(Duration::from_secs(15)).build().ok()?;
        Some(Arc::new(Self { cfg, http }))
    }

    fn header(&self, method: &str, path: &str, body: &[u8]) -> String {
        let ts = now_ms();
        format!("{ts}.{}", mac(&self.cfg.secret, method, path, ts, body))
    }

    pub fn verify(&self, headers: &HeaderMap, method: &str, path: &str, body: &[u8]) -> bool {
        let Some(h) = headers.get("x-aqua-fed").and_then(|v| v.to_str().ok()) else { return false };
        let Some((ts, sig)) = h.split_once('.') else { return false };
        let Ok(ts) = ts.parse::<i64>() else { return false };
        if (now_ms() - ts).abs() > SKEW_MS {
            return false;
        }
        let want = mac(&self.cfg.secret, method, path, ts, body);
        subtle::ConstantTimeEq::ct_eq(want.as_bytes(), sig.as_bytes()).into()
    }

    async fn get(&self, peer: &str, path: &str) -> Option<reqwest::Response> {
        let r = self.http.get(format!("{peer}{path}")).header("x-aqua-fed", self.header("GET", path, b"")).send().await.ok()?;
        r.status().is_success().then_some(r)
    }

    async fn post(&self, peer: &str, path: &str, body: Vec<u8>) -> Option<reqwest::Response> {
        let r = self.http.post(format!("{peer}{path}"))
            .header("x-aqua-fed", self.header("POST", path, &body))
            .header("content-type", "application/json")
            .timeout(Duration::from_secs(4))
            .body(body).send().await.ok()?;
        r.status().is_success().then_some(r)
    }

    /// Try to hand an envelope to whichever peer has the recipient online.
    pub async fn forward(&self, msg: &RelayMessage) -> bool {
        let Ok(body) = serde_json::to_vec(msg) else { return false };
        let tries = self.cfg.peers.iter().map(|p| {
            let body = body.clone();
            async move {
                match self.post(p, "/v1/fed/deliver", body).await {
                    Some(r) => r.json::<serde_json::Value>().await.ok().and_then(|v| v["delivered"].as_bool()).unwrap_or(false),
                    None => false,
                }
            }
        });
        futures_util::future::join_all(tries).await.into_iter().any(|d| d)
    }

    /// Envelopes that peers kept for `user` (they drop them once sent).
    pub async fn collect_inbox(&self, user: &str) -> Vec<RelayMessage> {
        let path = format!("/v1/fed/inbox/{user}");
        let mut out = Vec::new();
        for p in &self.cfg.peers {
            if let Some(r) = self.get(p, &path).await {
                if let Ok(list) = r.json::<Vec<RelayMessage>>().await {
                    out.extend(list);
                }
            }
        }
        out
    }

    pub async fn fetch_bytes(&self, path: &str) -> Option<(String, Vec<u8>)> {
        for p in &self.cfg.peers {
            if let Some(r) = self.get(p, path).await {
                let mime = r.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("application/octet-stream").to_string();
                if let Ok(b) = r.bytes().await {
                    return Some((mime, b.to_vec()));
                }
            }
        }
        None
    }

    pub async fn fetch_bundle(&self, user: &str) -> Option<serde_json::Value> {
        let path = format!("/v1/fed/bundle/{user}");
        for p in &self.cfg.peers {
            if let Some(r) = self.get(p, &path).await {
                if let Ok(v) = r.json().await {
                    return Some(v);
                }
            }
        }
        None
    }
}

pub fn current(state: &AppState) -> Option<Arc<Fed>> {
    state.fed.read().ok().and_then(|g| g.clone())
}

fn check(state: &AppState, headers: &HeaderMap, method: &str, path: &str, body: &[u8]) -> Result<Arc<Fed>, StatusCode> {
    let fed = current(state).ok_or(StatusCode::NOT_FOUND)?;
    if fed.verify(headers, method, path, body) { Ok(fed) } else { Err(StatusCode::UNAUTHORIZED) }
}

// ------------------------------------------------------------------ handlers

/// A peer asks: is this user connected here? Deliver if so (never queue).
pub async fn h_deliver(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Result<Json<serde_json::Value>, StatusCode> {
    check(&state, &headers, "POST", "/v1/fed/deliver", &body)?;
    let msg: RelayMessage = serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;
    let delivered = crate::deliver_local(&state, &msg).await;
    Ok(Json(serde_json::json!({ "delivered": delivered })))
}

pub async fn h_inbox(State(state): State<AppState>, headers: HeaderMap, Path(user): Path<String>) -> Result<Json<Vec<RelayMessage>>, StatusCode> {
    check(&state, &headers, "GET", &format!("/v1/fed/inbox/{user}"), b"")?;
    let mut out = Vec::new();
    for (id, bytes) in state.directory.inbox_take(&user) {
        if let Ok(m) = serde_json::from_slice::<RelayMessage>(&bytes) {
            out.push(m);
        }
        state.directory.inbox_ack(id);
    }
    Ok(Json(out))
}

pub async fn h_bundle(State(state): State<AppState>, headers: HeaderMap, Path(user): Path<String>) -> Result<impl IntoResponse, StatusCode> {
    check(&state, &headers, "GET", &format!("/v1/fed/bundle/{user}"), b"")?;
    crate::local_bundle(&state, &user).await.map(Json).ok_or(StatusCode::NOT_FOUND)
}

pub async fn h_blob(State(state): State<AppState>, headers: HeaderMap, Path(cid): Path<String>) -> Result<impl IntoResponse, StatusCode> {
    check(&state, &headers, "GET", &format!("/v1/fed/blob/{cid}"), b"")?;
    let store = state.blobs.clone();
    tokio::task::spawn_blocking(move || store.get(&cid)).await.ok().flatten().ok_or(StatusCode::NOT_FOUND)
}

#[derive(Deserialize)]
pub struct Since { #[serde(default)] since: i64 }

pub async fn h_users(State(state): State<AppState>, headers: HeaderMap, Query(q): Query<Since>) -> Result<impl IntoResponse, StatusCode> {
    check(&state, &headers, "GET", &format!("/v1/fed/users?since={}", q.since), b"")?;
    Ok(Json(state.directory.fed_users_since(q.since)))
}

pub async fn h_backups(State(state): State<AppState>, headers: HeaderMap, Query(q): Query<Since>) -> Result<impl IntoResponse, StatusCode> {
    check(&state, &headers, "GET", &format!("/v1/fed/backups?since={}", q.since), b"")?;
    Ok(Json(state.directory.fed_backups_since(q.since)))
}

pub async fn h_backup(State(state): State<AppState>, headers: HeaderMap, Path(user): Path<String>) -> Result<impl IntoResponse, StatusCode> {
    check(&state, &headers, "GET", &format!("/v1/fed/backup/{user}"), b"")?;
    state.directory.get_backup(&user).ok_or(StatusCode::NOT_FOUND)
}

pub async fn h_media(State(state): State<AppState>, headers: HeaderMap, Path((user, kind)): Path<(String, String)>) -> Result<impl IntoResponse, StatusCode> {
    check(&state, &headers, "GET", &format!("/v1/fed/media/{user}/{kind}"), b"")?;
    let k = crate::directory::MediaKind::parse(&kind).ok_or(StatusCode::NOT_FOUND)?;
    let (mime, data) = state.directory.get_media(&user, k).ok_or(StatusCode::NOT_FOUND)?;
    Ok(([(axum::http::header::CONTENT_TYPE, mime)], data))
}

/// Public: where else clients can connect.
pub async fn h_peers(State(state): State<AppState>) -> Json<serde_json::Value> {
    let cfg = current(&state).map(|f| f.cfg.clone());
    Json(serde_json::json!({
        "federated": cfg.is_some(),
        "self": cfg.as_ref().map(|c| c.public_url.clone()).unwrap_or_default(),
        "servers": cfg.map(|c| c.peers).unwrap_or_default(),
    }))
}

// ------------------------------------------------------------------ sync loop

/// Pull accounts and backups from every peer every 30 seconds.
pub fn spawn_sync(state: AppState) {
    tokio::spawn(async move {
        let mut cursors = Cursors::default();
        loop {
            sync_once(&state, &mut cursors).await;
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    });
}

/// How far we've read from each peer.
#[derive(Default)]
pub struct Cursors {
    users: std::collections::HashMap<String, i64>,
    backups: std::collections::HashMap<String, i64>,
}

pub async fn sync_once(state: &AppState, cursors: &mut Cursors) {
    if let Some(fed) = current(state) {
        for peer in fed.cfg.peers.clone() {
            sync_peer(state, &fed, &peer, &mut cursors.users, &mut cursors.backups).await;
        }
    }
}

async fn sync_peer(
    state: &AppState,
    fed: &Fed,
    peer: &str,
    since_users: &mut std::collections::HashMap<String, i64>,
    since_backups: &mut std::collections::HashMap<String, i64>,
) {
    // Accounts (paged: 500 per call).
    loop {
        let since = *since_users.get(peer).unwrap_or(&0);
        let Some(r) = fed.get(peer, &format!("/v1/fed/users?since={since}")).await else { return };
        let Ok(rows) = r.json::<Vec<crate::directory::FedUser>>().await else { return };
        if rows.is_empty() {
            break;
        }
        let mut applied = 0;
        for u in &rows {
            if state.directory.fed_apply_user(u) {
                applied += 1;
            }
        }
        let last = rows.iter().map(|u| u.updated_at).max().unwrap_or(since);
        since_users.insert(peer.to_string(), last);
        if applied > 0 {
            tracing::info!(peer, applied, "federation: accounts updated");
        }
        if rows.len() < 500 {
            break;
        }
    }
    // Encrypted backups.
    let since = *since_backups.get(peer).unwrap_or(&0);
    if let Some(r) = fed.get(peer, &format!("/v1/fed/backups?since={since}")).await {
        if let Ok(list) = r.json::<Vec<(String, i64)>>().await {
            for (user, at) in list {
                if state.directory.backup_meta(&user).is_some_and(|(_, ours)| ours >= at) {
                    continue;
                }
                if let Some(b) = fed.get(peer, &format!("/v1/fed/backup/{user}")).await {
                    if let Ok(bytes) = b.bytes().await {
                        state.directory.fed_apply_backup(&user, &bytes, at);
                    }
                }
                since_backups.insert(peer.to_string(), at.max(*since_backups.get(peer).unwrap_or(&0)));
            }
        }
    }
}
