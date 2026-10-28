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
//! * accounts, public profiles, profile media and encrypted backups
//!   replicate between all relays: a relay that changes something pokes
//!   its peers (`POST /v1/fed/poke`, within ~2 s) and they pull "everything
//!   since T" right away (plus a pull every 30 s as a safety net; newest
//!   row wins), so any relay can sign a user in and show their profile;
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
    /// "solo" — this relay works alone; "network" — with the others; ""
    /// — not chosen yet (the admin panel asks on first open; a config with
    /// a secret and peers from before counts as "network").
    #[serde(default)]
    pub mode: String,
    /// Kept in memory only (tests): peers learnt later aren't written to disk.
    #[serde(skip)]
    pub ephemeral: bool,
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
    /// In a network: a secret is set (peers may still be on their way —
    /// they say hello and get added by themselves).
    pub fn active(&self) -> bool {
        self.mode != "solo" && self.secret.len() >= 16 && (self.mode == "network" || !self.peers.is_empty())
    }
    /// "solo" / "network" / "" — what the panel shows.
    pub fn effective_mode(&self) -> &str {
        if self.mode.is_empty() && self.secret.len() >= 16 && !self.peers.is_empty() { "network" } else { &self.mode }
    }
}

/// An invitation into this network: another relay's admin pastes it and
/// both relays add each other. `aquafed:` + base64url(JSON {url, secret}).
pub fn invite(cfg: &FedConfig) -> Option<String> {
    use base64::Engine;
    let url = if cfg.public_url.is_empty() { crate::public::url() } else { cfg.public_url.clone() };
    if cfg.secret.len() < 16 || url.is_empty() {
        return None;
    }
    let json = serde_json::json!({ "url": url, "secret": cfg.secret }).to_string();
    Some(format!("aquafed:{}", base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json)))
}

/// (relay address, secret) from an invitation.
pub fn parse_invite(s: &str) -> Option<(String, String)> {
    use base64::Engine;
    let b = s.trim().strip_prefix("aquafed:")?;
    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(b.trim_end_matches('=')).ok()?;
    let v: serde_json::Value = serde_json::from_slice(&raw).ok()?;
    let url = v["url"].as_str()?.trim().trim_end_matches('/').to_string();
    let secret = v["secret"].as_str()?.trim().to_string();
    (url.starts_with("http") && secret.len() >= 16).then_some((url, secret))
}

/// Add relays to the peer list (saved, and the live config replaced).
/// Returns the ones that were new.
pub fn add_peers(state: &AppState, urls: &[String]) -> Vec<String> {
    let mut c = current(state).map(|f| f.cfg.clone()).unwrap_or_else(FedConfig::load);
    let me = if c.public_url.is_empty() { crate::public::url() } else { c.public_url.clone() };
    let mut added = Vec::new();
    for u in urls {
        let u = u.trim().trim_end_matches('/').to_string();
        if !u.starts_with("http") || u == me || c.peers.contains(&u) || c.peers.len() >= 64 {
            continue;
        }
        c.peers.push(u.clone());
        added.push(u);
    }
    if !added.is_empty() && (c.ephemeral || c.save().is_ok()) {
        let next = Fed::new(c);
        if let Ok(mut g) = state.fed.write() { *g = next; }
        tracing::info!(?added, "federation: new peers");
    }
    added
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
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .user_agent(concat!("aqua-server/", env!("CARGO_PKG_VERSION")))
            .build()
            .ok()?;
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

    /// Is `peer` a relay we still talk to (1.2.0 or newer)? Asked once in a
    /// while (`/v1/stats` says its version; older relays don't say).
    async fn modern(&self, peer: &str) -> bool {
        static SEEN: std::sync::Mutex<Option<std::collections::HashMap<String, (std::time::Instant, bool)>>> = std::sync::Mutex::new(None);
        if let Some((at, ok)) = SEEN.lock().ok().and_then(|m| m.as_ref().and_then(|m| m.get(peer).copied())) {
            if at.elapsed() < Duration::from_secs(if ok { 600 } else { 120 }) {
                return ok;
            }
        }
        let ok = match self.http.get(format!("{peer}/v1/stats")).timeout(Duration::from_secs(5)).send().await {
            Ok(r) => r.json::<serde_json::Value>().await.ok()
                .and_then(|v| v["version"].as_str().map(crate::version_ok))
                .unwrap_or(false),
            // Unreachable now: don't decide for 10 minutes on that.
            Err(_) => return false,
        };
        if !ok {
            tracing::warn!(%peer, "federation: this relay is older than {} — not talking to it", crate::MIN_VERSION);
        }
        if let Ok(mut m) = SEEN.lock() {
            m.get_or_insert_with(std::collections::HashMap::new).insert(peer.to_string(), (std::time::Instant::now(), ok));
        }
        ok
    }

    async fn get(&self, peer: &str, path: &str) -> Option<reqwest::Response> {
        if !self.modern(peer).await {
            return None;
        }
        let r = self.http.get(format!("{peer}{path}")).header("x-aqua-fed", self.header("GET", path, b"")).send().await.ok()?;
        r.status().is_success().then_some(r)
    }

    async fn post(&self, peer: &str, path: &str, body: Vec<u8>) -> Option<reqwest::Response> {
        if !self.modern(peer).await {
            return None;
        }
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

    /// Say hello to every peer: they add us if they didn't know us, and we
    /// learn whom they know (same secret — the reply is signed by it).
    /// Returns relays that were new to us.
    pub async fn hello_all(&self) -> Vec<String> {
        let me = if self.cfg.public_url.is_empty() { crate::public::url() } else { self.cfg.public_url.clone() };
        let body = serde_json::to_vec(&serde_json::json!({ "url": me, "peers": self.cfg.peers })).unwrap_or_default();
        let tries = self.cfg.peers.iter().map(|p| {
            let body = body.clone();
            async move {
                let r = self.post(p, "/v1/fed/hello", body).await?;
                let v: serde_json::Value = r.json().await.ok()?;
                let mut out: Vec<String> = v["peers"].as_array()?.iter().filter_map(|x| x.as_str().map(str::to_string)).collect();
                if let Some(s) = v["self"].as_str() { out.push(s.to_string()); }
                Some(out)
            }
        });
        let mut known: Vec<String> = futures_util::future::join_all(tries).await.into_iter().flatten().flatten().collect();
        known.sort();
        known.dedup();
        known.retain(|u| u.trim_end_matches('/') != me && !self.cfg.peers.iter().any(|p| p == u.trim_end_matches('/')));
        known
    }

    /// Is `url` a relay of this network (does it accept our signature)?
    pub async fn same_network(&self, url: &str) -> bool {
        self.get(url.trim_end_matches('/'), "/v1/fed/ping").await.is_some()
    }

    /// Have a peer with mail set up send a sign-up code (this relay has no
    /// mail server): the SMTP password never leaves the relay that has it.
    pub async fn send_mail(&self, to: &str, code: &str, link: Option<&str>) -> bool {
        let body = serde_json::to_vec(&serde_json::json!({ "to": to, "code": code, "link": link })).unwrap_or_default();
        for p in &self.cfg.peers {
            let r = self.http.post(format!("{p}/v1/fed/mail"))
                .header("x-aqua-fed", self.header("POST", "/v1/fed/mail", &body))
                .header("content-type", "application/json")
                .timeout(Duration::from_secs(50))
                .body(body.clone()).send().await;
            if r.is_ok_and(|r| r.status().is_success()) {
                tracing::info!(peer = %p, "sign-up code e-mailed by a peer");
                return true;
            }
        }
        false
    }

    /// Tell every peer to pull from us now.
    pub async fn poke_all(&self) {
        let body = serde_json::to_vec(&serde_json::json!({ "from": self.cfg.public_url })).unwrap_or_default();
        let tries = self.cfg.peers.iter().map(|p| {
            let body = body.clone();
            async move { self.post(p, "/v1/fed/poke", body).await.is_some() }
        });
        let ok = futures_util::future::join_all(tries).await.into_iter().filter(|x| *x).count();
        tracing::debug!(ok, peers = self.cfg.peers.len(), "federation: poked peers");
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
    if !fed.verify(headers, method, path, body) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    // Relays older than 1.2.0 don't know two-step codes: an account copied
    // there could be signed into without one. They're not served.
    if !crate::caller_version(headers, None).as_deref().is_some_and(crate::version_ok) {
        return Err(StatusCode::UPGRADE_REQUIRED);
    }
    Ok(fed)
}

// ------------------------------------------------------------------ handlers

/// A peer asks: is this user connected here? Deliver if so (never queue).
pub async fn h_deliver(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Result<Json<serde_json::Value>, StatusCode> {
    check(&state, &headers, "POST", "/v1/fed/deliver", &body)?;
    let msg: RelayMessage = serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;
    crate::journal::visitor(crate::base_user(&msg.from), "user");
    let delivered = crate::deliver_local(&state, &msg).await;
    Ok(Json(serde_json::json!({ "delivered": delivered })))
}

/// A peer changed something: pull from the peers now instead of in ≤ 30 s.
pub async fn h_poke(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Result<StatusCode, StatusCode> {
    check(&state, &headers, "POST", "/v1/fed/poke", &body)?;
    state.fed_kick.notify_one();
    Ok(StatusCode::NO_CONTENT)
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

#[derive(Deserialize)]
pub struct Hello { url: String, #[serde(default)] peers: Vec<String> }

/// A relay of the network says hello: it's added to our peers if new, and
/// gets the list of relays we know (that's how they find each other).
/// History records changed after `since` (ms): `[[user, key, updated_at]]`.
pub async fn h_hist_list(State(state): State<AppState>, headers: HeaderMap, Query(q): Query<Since>) -> Result<impl IntoResponse, StatusCode> {
    check(&state, &headers, "GET", &format!("/v1/fed/hist?since={}", q.since), b"")?;
    Ok(Json(state.directory.fed_hist_since(q.since)))
}

pub async fn h_hist_get(State(state): State<AppState>, headers: HeaderMap, Path((user, k)): Path<(String, String)>) -> Result<impl IntoResponse, StatusCode> {
    check(&state, &headers, "GET", &format!("/v1/fed/hist/{user}/{k}"), b"")?;
    state.directory.hist_get(&user, &k).ok_or(StatusCode::NOT_FOUND)
}

pub async fn h_hello(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Result<Json<serde_json::Value>, StatusCode> {
    let fed = check(&state, &headers, "POST", "/v1/fed/hello", &body)?;
    let h: Hello = serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;
    crate::journal::visitor(h.url.trim_end_matches('/'), "relay");
    let mut urls = vec![h.url];
    urls.extend(h.peers);
    let st = state.clone();
    // Peers it knows join too — but only those that answer our signature.
    tokio::spawn(async move {
        let mut ok = Vec::new();
        for u in urls {
            if let Some(f) = current(&st) {
                if f.cfg.peers.iter().any(|p| p == u.trim_end_matches('/')) { continue; }
                if f.same_network(&u).await { ok.push(u); }
            }
        }
        if !add_peers(&st, &ok).is_empty() { st.fed_kick.notify_one(); }
    });
    let me = if fed.cfg.public_url.is_empty() { crate::public::url() } else { fed.cfg.public_url.clone() };
    Ok(Json(serde_json::json!({ "self": me, "peers": fed.cfg.peers })))
}

/// "Do you accept our signature?" — 204 for relays of the same network.
pub async fn h_ping(State(state): State<AppState>, headers: HeaderMap) -> Result<StatusCode, StatusCode> {
    check(&state, &headers, "GET", "/v1/fed/ping", b"")?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct MailReq { to: String, code: String, #[serde(default)] link: Option<String> }

/// A relay of the network without mail asks us to send a sign-up code.
/// Only to relays that share the secret; a few per address per 10 minutes.
pub async fn h_mail(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Result<StatusCode, StatusCode> {
    check(&state, &headers, "POST", "/v1/fed/mail", &body)?;
    let m: MailReq = serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;
    if m.code.len() > 12 || !m.to.contains('@') || m.link.as_ref().is_some_and(|l| !l.starts_with("http") || l.len() > 300) {
        return Err(StatusCode::BAD_REQUEST);
    }
    {
        static RECENT: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, Vec<i64>>>> = std::sync::OnceLock::new();
        let mut g = RECENT.get_or_init(Default::default).lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let now = now_ms();
        let list = g.entry(m.to.to_lowercase()).or_default();
        list.retain(|t| now - t < 10 * 60 * 1000);
        if list.len() >= 5 {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
        list.push(now);
    }
    match state.directory.send_code_mail(&m.to, &m.code, m.link.as_deref()).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            tracing::warn!(error = %e, "relaying a sign-up e-mail for a peer failed");
            Err(StatusCode::SERVICE_UNAVAILABLE)
        }
    }
}

/// Public: where else clients can connect.
pub async fn h_peers(State(state): State<AppState>) -> Json<serde_json::Value> {
    let cfg = current(&state).map(|f| f.cfg.clone());
    Json(serde_json::json!({
        "federated": cfg.is_some(),
        "self": cfg.as_ref().map(|c| c.public_url.clone()).filter(|u| !u.is_empty()).unwrap_or_else(crate::public::url),
        "servers": cfg.map(|c| c.peers).unwrap_or_default(),
    }))
}

// ------------------------------------------------------------------ sync loop

/// Pull accounts, profiles and backups from every peer — right away when a
/// peer pokes us, and every 30 seconds anyway. A second task watches the
/// local database and pokes the peers whenever something here changed.
pub fn spawn_sync(state: AppState) {
    let st = state.clone();
    tokio::spawn(async move {
        let mut cursors = Cursors::default();
        loop {
            sync_once(&st, &mut cursors).await;
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(30)) => {}
                _ = st.fed_kick.notified() => {
                    // Let a burst of changes on the peer settle.
                    tokio::time::sleep(Duration::from_millis(300)).await;
                }
            }
        }
    });
    spawn_announcer(state);
}

/// Poke the peers whenever an account, profile or backup changed here.
pub fn spawn_announcer(state: AppState) {
    // Every few minutes: hello to the peers, so relays of the network find
    // each other (and a relay that was down learns who joined meanwhile).
    let st = state.clone();
    tokio::spawn(async move {
        loop {
            if let Some(fed) = current(&st) {
                let mut new = Vec::new();
                for u in fed.hello_all().await {
                    if fed.same_network(&u).await { new.push(u); }
                }
                if !add_peers(&st, &new).is_empty() { st.fed_kick.notify_one(); }
            }
            tokio::time::sleep(Duration::from_secs(180)).await;
        }
    });
    tokio::spawn(async move {
        let mut last: Option<(i64, i64)> = None;
        loop {
            let dir = state.directory.clone();
            let mark = tokio::task::spawn_blocking(move || dir.change_mark()).await.unwrap_or_default();
            if last != Some(mark) {
                // First round too: peers learn what changed while we were down.
                if let Some(fed) = current(&state) {
                    fed.poke_all().await;
                }
                last = Some(mark);
            }
            tokio::time::sleep(Duration::from_millis(1500)).await;
        }
    });
}

/// How far we've read from each peer.
#[derive(Default)]
pub struct Cursors {
    users: std::collections::HashMap<String, i64>,
    backups: std::collections::HashMap<String, i64>,
    hist: std::collections::HashMap<String, i64>,
}

pub async fn sync_once(state: &AppState, cursors: &mut Cursors) {
    if let Some(fed) = current(state) {
        for peer in fed.cfg.peers.clone() {
            sync_peer(state, &fed, &peer, &mut cursors.users, &mut cursors.backups).await;
            sync_hist(state, &fed, &peer, &mut cursors.hist).await;
        }
    }
}

/// Encrypted chat history of accounts (client history_sync.rs): a device may
/// talk to any relay of the network and still find it.
async fn sync_hist(state: &AppState, fed: &Fed, peer: &str, since: &mut std::collections::HashMap<String, i64>) {
    loop {
        let from = *since.get(peer).unwrap_or(&0);
        let Some(r) = fed.get(peer, &format!("/v1/fed/hist?since={from}")).await else { return };
        let Ok(rows) = r.json::<Vec<(String, String, i64)>>().await else { return };
        if rows.is_empty() { return; }
        let mut last = from;
        for (user, k, at) in &rows {
            last = last.max(*at);
            if !crate::directory::Directory::valid_name(user) || k.len() > 64 { continue; }
            if let Some(b) = fed.get(peer, &format!("/v1/fed/hist/{user}/{k}")).await {
                if let Ok(bytes) = b.bytes().await {
                    state.directory.fed_apply_hist(user, k, &bytes, *at);
                }
            }
        }
        since.insert(peer.to_string(), last);
        if rows.len() < 1000 { return; }
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
            let before = state.directory.media_versions(&u.username).unwrap_or((0, 0, 0));
            let res = state.directory.fed_apply_user(u);
            if let Some((old, new)) = &res.displaced {
                crate::move_live(state, old, new);
            }
            if res.applied {
                applied += 1;
                // Profile media travels with the account, so it is here
                // even if the other relay goes away.
                for (kind, old, new) in [("avatar", before.0, u.avatar_ver), ("banner", before.1, u.banner_ver), ("music", before.2, u.music_ver)] {
                    if new == old || new == 0 {
                        continue;
                    }
                    let Some(k) = crate::directory::MediaKind::parse(kind) else { continue };
                    if let Some(r) = fed.get(peer, &format!("/v1/fed/media/{}/{kind}", u.username)).await {
                        let mime = r.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("application/octet-stream").to_string();
                        if let Ok(b) = r.bytes().await {
                            state.directory.fed_store_media(&res.name, k, &mime, &b);
                        }
                    }
                }
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
