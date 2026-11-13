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
    /// Solo relays that share the name registry with us (they said hello
    /// at `/v1/fed/names/hello`): only their (uid, @name) list is pulled.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub solo_peers: Vec<String>,
    /// A lasting id of this relay: how a relay without a reachable address
    /// is known to its peers (`tunnel:<node>`, see fedtunnel.rs).
    #[serde(default)]
    pub node: String,
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
        if c.node.is_empty() {
            let mut b = [0u8; 8];
            rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut b);
            c.node = hex::encode(b);
            if std::path::Path::new(FILE).exists() {
                let _ = c.save();
            }
        }
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
    /// A solo relay that shares only the name registry with a network
    /// (joined with an invitation, but stays solo).
    pub fn registry(&self) -> bool {
        self.mode == "solo" && self.secret.len() >= 16 && !self.peers.is_empty()
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
    let me = my_id(&c);
    let mut added = Vec::new();
    for u in urls {
        let u = u.trim().trim_end_matches('/').to_string();
        if !(u.starts_with("http") || u.starts_with("tunnel:")) || u == me || c.peers.contains(&u) || c.peers.len() >= 64 {
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

/// How this relay is known to its peers: its address, or `tunnel:<node>`
/// when it has none that answers (behind a router; fedtunnel.rs).
pub fn my_id(cfg: &FedConfig) -> String {
    if !cfg.public_url.is_empty() {
        return cfg.public_url.clone();
    }
    let u = crate::public::url();
    if !u.is_empty() { u } else { format!("tunnel:{}", cfg.node) }
}

/// A peer polling us over a reverse link (fedtunnel.rs): keep it in the
/// list under `id`, dropping `addr` — the address of it that never answered.
pub fn adopt_peer(state: &AppState, id: &str, addr: &str) {
    let Some(f) = current(state) else { return };
    let has = f.cfg.peers.iter().any(|p| p == id);
    let stale = !addr.is_empty() && addr != id && f.cfg.peers.iter().any(|p| p == addr);
    if has && !stale {
        return;
    }
    let mut c = f.cfg.clone();
    if stale {
        c.peers.retain(|p| p != addr);
    }
    if !has && c.peers.len() < 64 && id != my_id(&c) {
        c.peers.push(id.to_string());
    }
    if c.ephemeral || c.save().is_ok() {
        let next = Fed::new(c);
        if let Ok(mut g) = state.fed.write() { *g = next; }
        tracing::info!(peer = %id, "federation: a peer behind a router linked back to us");
        state.fed_kick.notify_one();
    }
}

/// Direct requests to these peers failed lately: use their reverse link.
fn down() -> &'static std::sync::Mutex<std::collections::HashMap<String, std::time::Instant>> {
    static D: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, std::time::Instant>>> = std::sync::OnceLock::new();
    D.get_or_init(Default::default)
}
fn mark_down(peer: &str) {
    if let Ok(mut d) = down().lock() { d.insert(peer.to_string(), std::time::Instant::now()); }
}
/// The direct way to `peer` failed lately (the panel shows its reverse link).
pub fn tunnel_preferred(peer: &str) -> bool {
    is_down(peer)
}
fn is_down(peer: &str) -> bool {
    down().lock().ok().and_then(|d| d.get(peer).map(|t| t.elapsed() < Duration::from_secs(300))).unwrap_or(false)
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
        if !cfg.active() && !cfg.registry() {
            return None;
        }
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .user_agent(concat!("aqua-server/", env!("CARGO_PKG_VERSION")))
            .build()
            .ok()?;
        Some(Arc::new(Self { cfg, http }))
    }

    /// The `x-aqua-fed` header for a request (fedtunnel.rs signs its polls).
    pub fn sign(&self, method: &str, path: &str, body: &[u8]) -> String {
        self.header(method, path, body)
    }

    /// Over the peer's reverse link, when that's the way to it: it has no
    /// address, or its address stopped answering and it polls us.
    async fn via_tunnel(&self, peer: &str, method: &str, path: &str, body: &[u8], wait: Duration) -> Option<Option<reqwest::Response>> {
        let tunnel_only = peer.starts_with("tunnel:");
        if !tunnel_only && !(is_down(peer) && crate::fedtunnel::active(peer)) {
            return None;
        }
        let sig = self.header(method, path, body);
        let r = crate::fedtunnel::request(peer, method, path, body.to_vec(), sig, wait).await;
        Some(r.filter(|r| r.status().is_success()))
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
        if peer.starts_with("tunnel:") {
            return false;
        }
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
            Err(_) => { mark_down(peer); return false }
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
        if let Some(r) = self.via_tunnel(peer, "GET", path, b"", Duration::from_secs(30)).await {
            return r;
        }
        if !self.modern(peer).await {
            return self.tunnel_fallback(peer, "GET", path, b"").await;
        }
        match self.http.get(format!("{peer}{path}")).header("x-aqua-fed", self.header("GET", path, b"")).send().await {
            Ok(r) => r.status().is_success().then_some(r),
            Err(_) => { mark_down(peer); self.tunnel_fallback(peer, "GET", path, b"").await }
        }
    }

    async fn post(&self, peer: &str, path: &str, body: Vec<u8>) -> Option<reqwest::Response> {
        if let Some(r) = self.via_tunnel(peer, "POST", path, &body, Duration::from_secs(8)).await {
            return r;
        }
        if !self.modern(peer).await {
            return self.tunnel_fallback(peer, "POST", path, &body).await;
        }
        let sent = self.http.post(format!("{peer}{path}"))
            .header("x-aqua-fed", self.header("POST", path, &body))
            .header("content-type", "application/json")
            .timeout(Duration::from_secs(4))
            .body(body.clone()).send().await;
        match sent {
            Ok(r) => r.status().is_success().then_some(r),
            Err(_) => { mark_down(peer); self.tunnel_fallback(peer, "POST", path, &body).await }
        }
    }

    /// The direct way failed: the reverse link, if the peer keeps one.
    async fn tunnel_fallback(&self, peer: &str, method: &str, path: &str, body: &[u8]) -> Option<reqwest::Response> {
        if !crate::fedtunnel::active(peer) {
            return None;
        }
        mark_down(peer);
        let sig = self.header(method, path, body);
        crate::fedtunnel::request(peer, method, path, body.to_vec(), sig, Duration::from_secs(30)).await.filter(|r| r.status().is_success())
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
        let me = my_id(&self.cfg);
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

    /// Tell every peer: something is queued here — pull it for whoever is
    /// connected to you (only that, not a full sync).
    pub async fn poke_inbox_all(&self) {
        let body = serde_json::to_vec(&serde_json::json!({ "from": self.cfg.public_url, "inbox": true })).unwrap_or_default();
        let tries = self.cfg.peers.iter().map(|p| {
            let body = body.clone();
            async move { self.post(p, "/v1/fed/poke", body).await.is_some() }
        });
        futures_util::future::join_all(tries).await;
    }

    /// Envelopes that peers kept for `user` (they drop them once sent).
    pub async fn collect_inbox(&self, user: &str) -> Vec<RelayMessage> {
        let path = format!("/v1/fed/inbox/{user}");
        // All peers at once (one by one, every slow one added its wait).
        let users = [user.to_string()];
        let tries = self.cfg.peers.iter().map(|p| {
            let (path, users) = (path.clone(), &users);
            async move {
                if let Some(got) = self.take_kept(p, users).await { return Some(got); }
                // A peer from before 1.20.1-8: it hands them over and forgets them.
                self.get(p, &path).await?.json::<Vec<RelayMessage>>().await.ok()
            }
        });
        futures_util::future::join_all(tries).await.into_iter().flatten().flatten().collect()
    }

    /// What `peer` keeps for `users`, without it forgetting them before we
    /// have them: it lists them (`/v1/fed/inboxes2`), we confirm what came
    /// (`/v1/fed/inbox_ack`) and only then it deletes. The old endpoints
    /// deleted while answering — a reply lost on the way (a timeout, a
    /// tunnel that dropped) lost the messages with it. None: not supported
    /// there (or not reachable).
    async fn take_kept(&self, peer: &str, users: &[String]) -> Option<Vec<RelayMessage>> {
        let body = serde_json::to_vec(&serde_json::json!({ "users": users })).ok()?;
        let r = self.post(peer, "/v1/fed/inboxes2", body).await?;
        let rows: Vec<serde_json::Value> = r.json().await.ok()?;
        let mut ids = Vec::new();
        let mut out = Vec::new();
        for row in rows {
            if let Some(id) = row["id"].as_i64() { ids.push(id); }
            if let Ok(m) = serde_json::from_value::<RelayMessage>(row["m"].clone()) { out.push(m); }
        }
        if !ids.is_empty() {
            let body = serde_json::to_vec(&serde_json::json!({ "ids": ids })).unwrap_or_default();
            // Not confirmed: they come again next time (the device drops copies by id).
            let _ = self.post(peer, "/v1/fed/inbox_ack", body).await;
        }
        Some(out)
    }

    /// Envelopes peers keep for any of `users` (connected here) — one call per
    /// peer. A push (`forward`) that failed once no longer waits for the
    /// recipient's next reconnect. Peers older than this ignore the call.
    pub async fn collect_inboxes(&self, users: &[String]) -> Vec<RelayMessage> {
        if users.is_empty() { return Vec::new(); }
        let body = serde_json::to_vec(&serde_json::json!({ "users": users })).unwrap_or_default();
        let tries = self.cfg.peers.iter().map(|p| {
            let body = body.clone();
            async move {
                if let Some(got) = self.take_kept(p, users).await { return Some(got); }
                let r = self.post(p, "/v1/fed/inboxes", body).await?;
                r.json::<Vec<RelayMessage>>().await.ok()
            }
        });
        futures_util::future::join_all(tries).await.into_iter().flatten().flatten().collect()
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

/// The network this relay is part of (not a solo relay's name registry).
pub fn current(state: &AppState) -> Option<Arc<Fed>> {
    state.fed.read().ok().and_then(|g| g.clone()).filter(|f| f.cfg.active())
}

/// Whoever we share the name registry with: the network, or — for a solo
/// relay — the network it reserves names in.
pub fn registry(state: &AppState) -> Option<Arc<Fed>> {
    state.fed.read().ok().and_then(|g| g.clone())
}

/// Signed by our secret, from a relay new enough — for the name registry,
/// which solo relays use too.
fn check_registry(state: &AppState, headers: &HeaderMap, method: &str, path: &str, body: &[u8]) -> Result<Arc<Fed>, StatusCode> {
    let fed = registry(state).ok_or(StatusCode::NOT_FOUND)?;
    if !fed.verify(headers, method, path, body) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    if !crate::caller_version(headers, None).as_deref().is_some_and(crate::version_ok) {
        return Err(StatusCode::UPGRADE_REQUIRED);
    }
    Ok(fed)
}

fn my_url(cfg: &FedConfig) -> String {
    if cfg.public_url.is_empty() { crate::public::url() } else { cfg.public_url.clone() }
}

/// `GET /v1/fed/names?since=T` — the name registry (see
/// [`crate::directory::Directory::names_since`]).
pub async fn h_names(State(state): State<AppState>, headers: HeaderMap, Query(q): Query<Since>) -> Result<impl IntoResponse, StatusCode> {
    let fed = check_registry(&state, &headers, "GET", &format!("/v1/fed/names?since={}", q.since), b"")
        .or_else(|_| check_registry(&state, &headers, "GET", "/v1/fed/names", b""))?;
    let dir = state.directory.clone();
    let names = tokio::task::spawn_blocking(move || dir.names_since(q.since, 1000)).await.unwrap_or_default();
    Ok(Json(serde_json::json!({ "home": my_url(&fed.cfg), "mode": if fed.cfg.active() { "network" } else { "solo" }, "names": names })))
}

/// `POST /v1/fed/names/hello {url}` — a solo relay shares the name registry
/// with us; it learns our peers (to reserve its names with all of them).
pub async fn h_names_hello(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Result<Json<serde_json::Value>, StatusCode> {
    let fed = check_registry(&state, &headers, "POST", "/v1/fed/names/hello", &body)?;
    let v: serde_json::Value = serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;
    let url = v["url"].as_str().unwrap_or("").trim().trim_end_matches('/').to_string();
    if fed.cfg.active() && url.starts_with("http") && url != my_url(&fed.cfg) && !fed.cfg.solo_peers.contains(&url) && fed.cfg.solo_peers.len() < 256 {
        let mut c = fed.cfg.clone();
        c.solo_peers.push(url.clone());
        if c.ephemeral || c.save().is_ok() {
            if let Ok(mut g) = state.fed.write() { *g = Fed::new(c); }
            tracing::info!(%url, "name registry: a solo relay joined");
        }
    }
    Ok(Json(serde_json::json!({ "self": my_url(&fed.cfg), "peers": fed.cfg.peers })))
}

/// A solo relay: say hello to the network for the name registry. Returns
/// the relays it told us about that we didn't know.
pub async fn names_hello(fed: &Fed) -> Vec<String> {
    let me = my_url(&fed.cfg);
    let body = serde_json::to_vec(&serde_json::json!({ "url": me })).unwrap_or_default();
    let mut known = Vec::new();
    for p in &fed.cfg.peers {
        if let Some(r) = fed.post(p, "/v1/fed/names/hello", body.clone()).await {
            if let Ok(v) = r.json::<serde_json::Value>().await {
                for x in v["peers"].as_array().into_iter().flatten().chain(std::iter::once(&v["self"])) {
                    if let Some(u) = x.as_str() { known.push(u.trim_end_matches('/').to_string()); }
                }
            }
        }
    }
    known.sort();
    known.dedup();
    known.retain(|u| *u != me && !fed.cfg.peers.contains(u));
    known
}

/// Pull the name registry from one relay (paged).
async fn sync_names(state: &AppState, fed: &Fed, peer: &str, since: &mut std::collections::HashMap<String, i64>) {
    for _ in 0..50 {
        let from = *since.get(peer).unwrap_or(&0);
        let Some(r) = fed.get(peer, &format!("/v1/fed/names?since={from}")).await else { return };
        let Ok(v) = r.json::<serde_json::Value>().await else { return };
        let home = v["home"].as_str().unwrap_or(peer).trim_end_matches('/').to_string();
        let Ok(rows) = serde_json::from_value::<Vec<crate::directory::NameRow>>(v["names"].clone()) else { return };
        if rows.is_empty() { return; }
        let n = rows.len();
        let last = rows.iter().map(|r| r.updated_at).max().unwrap_or(from);
        let dir = state.directory.clone();
        let moved = tokio::task::spawn_blocking(move || rows.iter().filter_map(|r| dir.fed_apply_name(r, &home)).collect::<Vec<_>>()).await.unwrap_or_default();
        for (old, new) in &moved {
            crate::move_live(state, old, new);
        }
        since.insert(peer.to_string(), last);
        if n < 1000 { return; }
    }
}

/// `check` for other modules (fedtunnel.rs).
pub fn check_pub(state: &AppState, headers: &HeaderMap, method: &str, path: &str, body: &[u8]) -> Result<Arc<Fed>, StatusCode> {
    check(state, headers, method, path, body)
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
    let inbox = serde_json::from_slice::<serde_json::Value>(&body).ok().is_some_and(|v| v["inbox"].as_bool() == Some(true));
    if !inbox {
        state.fed_kick.notify_one();
        return Ok(StatusCode::NO_CONTENT);
    }
    // Only queued envelopes: pull them for the people connected here (one pull at a time).
    use std::sync::atomic::{AtomicBool, Ordering};
    static PULLING: AtomicBool = AtomicBool::new(false);
    if !PULLING.swap(true, Ordering::SeqCst) {
        if let Some(fed) = current(&state) {
            tokio::spawn(async move {
                pull_for_connected(&state, &fed).await;
                PULLING.store(false, Ordering::SeqCst);
            });
        } else {
            PULLING.store(false, Ordering::SeqCst);
        }
    }
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

/// Batch of `h_inbox`: what waits here for these users (connected to the caller).
pub async fn h_inboxes(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Result<Json<Vec<RelayMessage>>, StatusCode> {
    check(&state, &headers, "POST", "/v1/fed/inboxes", &body)?;
    let v: serde_json::Value = serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;
    let users = v["users"].as_array().ok_or(StatusCode::BAD_REQUEST)?;
    let mut out = Vec::new();
    for u in users.iter().filter_map(|x| x.as_str()).take(5000) {
        for (id, bytes) in state.directory.inbox_take(u) {
            if let Ok(m) = serde_json::from_slice::<RelayMessage>(&bytes) {
                out.push(m);
            }
            state.directory.inbox_ack(id);
        }
    }
    Ok(Json(out))
}

/// `h_inboxes` that forgets nothing yet: `[{id, m}]`, deleted by `h_inbox_ack`
/// once the caller has them (see `Fed::take_kept`).
pub async fn h_inboxes2(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Result<Json<Vec<serde_json::Value>>, StatusCode> {
    check(&state, &headers, "POST", "/v1/fed/inboxes2", &body)?;
    let v: serde_json::Value = serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;
    let users = v["users"].as_array().ok_or(StatusCode::BAD_REQUEST)?;
    let mut out = Vec::new();
    for u in users.iter().filter_map(|x| x.as_str()).take(5000) {
        for (id, bytes) in state.directory.inbox_take(u) {
            match serde_json::from_slice::<serde_json::Value>(&bytes) {
                Ok(m) => out.push(serde_json::json!({ "id": id, "m": m })),
                Err(_) => state.directory.inbox_ack(id),
            }
        }
    }
    Ok(Json(out))
}

/// The caller has these (`h_inboxes2`): forget them.
pub async fn h_inbox_ack(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Result<StatusCode, StatusCode> {
    check(&state, &headers, "POST", "/v1/fed/inbox_ack", &body)?;
    let v: serde_json::Value = serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;
    for id in v["ids"].as_array().ok_or(StatusCode::BAD_REQUEST)?.iter().filter_map(|x| x.as_i64()).take(100_000) {
        state.directory.inbox_ack(id);
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Something was queued here for a user who isn't connected here: ask the
/// peers to pull now (they take it for whoever is connected to them).
/// Bursts collapse into one poke a second.
pub fn queued_for_absent(state: &AppState) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static BUSY: AtomicBool = AtomicBool::new(false);
    let Some(fed) = current(state) else { return };
    if BUSY.swap(true, Ordering::SeqCst) { return; }
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(700)).await;
        BUSY.store(false, Ordering::SeqCst);
        fed.poke_inbox_all().await;
    });
}

/// Pull what peers queued for the people connected here, and hand it over.
async fn pull_for_connected(state: &AppState, fed: &Fed) {
    let users: Vec<String> = state.users.iter()
        .filter(|e| e.value().try_lock().map(|u| u.tx.iter().any(|t| !t.is_closed())).unwrap_or(true))
        .map(|e| e.key().clone())
        .collect();
    let got = fed.collect_inboxes(&users).await;
    if got.is_empty() { return; }
    tracing::info!(n = got.len(), "federation: pulled queued envelopes for people connected here");
    for m in got {
        if crate::deliver_local(state, &m).await { continue; }
        // Gone meanwhile: keep it here until they're back.
        if let Ok(bytes) = serde_json::to_vec(&m) { let _ = state.directory.inbox_push(&m.to, &bytes); }
    }
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
    Ok(Json(serde_json::json!({ "self": my_id(&fed.cfg), "peers": fed.cfg.peers })))
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
        // Clients connect by address: relays known only by a reverse link aren't listed.
        "servers": cfg.map(|c| c.peers.into_iter().filter(|p| p.starts_with("http")).collect::<Vec<_>>()).unwrap_or_default(),
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
    crate::fedtunnel::spawn(state.clone());
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
            } else if let Some(fed) = registry(&st) {
                let new = names_hello(&fed).await;
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
    names: std::collections::HashMap<String, i64>,
}

pub async fn sync_once(state: &AppState, cursors: &mut Cursors) {
    let Some(fed) = registry(state) else { return };
    if fed.cfg.active() {
        // Messages first: they're what people wait for.
        pull_for_connected(state, &fed).await;
        for peer in fed.cfg.peers.clone() {
            sync_peer(state, &fed, &peer, &mut cursors.users, &mut cursors.backups).await;
            sync_hist(state, &fed, &peer, &mut cursors.hist).await;
        }
        // Solo relays: only their names.
        for peer in fed.cfg.solo_peers.clone() {
            sync_names(state, &fed, &peer, &mut cursors.names).await;
        }
    } else {
        // A solo relay in the name registry: the network's names, nothing else.
        for peer in fed.cfg.peers.clone() {
            sync_names(state, &fed, &peer, &mut cursors.names).await;
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

/// All relay-to-relay routes: mounted in the app, and run in-process for
/// requests that come over a reverse link (fedtunnel.rs).
pub fn routes() -> axum::Router<AppState> {
    use axum::routing::{get, post};
    axum::Router::new()
        .route("/v1/fed/names", get(h_names))
        .route("/v1/fed/names/hello", post(h_names_hello))
        .route("/v1/fed/hist", get(h_hist_list))
        .route("/v1/fed/hist/:user/:k", get(h_hist_get))
        .route("/v1/fed/peers", get(h_peers))
        .route("/v1/fed/hello", post(h_hello))
        .route("/v1/fed/ping", get(h_ping))
        .route("/v1/fed/mail", post(h_mail))
        .route("/v1/fed/deliver", post(h_deliver))
        .route("/v1/fed/poke", post(h_poke))
        .route("/v1/fed/inbox/:user", get(h_inbox))
        .route("/v1/fed/inboxes", post(h_inboxes))
        .route("/v1/fed/inboxes2", post(h_inboxes2))
        .route("/v1/fed/inbox_ack", post(h_inbox_ack))
        .route("/v1/fed/bundle/:user", get(h_bundle))
        .route("/v1/fed/blob/:cid", get(h_blob))
        .route("/v1/fed/users", get(h_users))
        .route("/v1/fed/backups", get(h_backups))
        .route("/v1/fed/backup/:user", get(h_backup))
        .route("/v1/fed/media/:user/:kind", get(h_media))
        .route("/v1/fed/tunnel", post(crate::fedtunnel::h_poll))
        .route("/v1/fed/tunnel/reply", post(crate::fedtunnel::h_reply)
            .layer(axum::extract::DefaultBodyLimit::max(96 * 1024 * 1024)))
}
