//! Federation: several relays act as one network.
//!
//! Peers share a secret (`AQUA_FED_SECRET` / admin panel) and list each
//! other (`AQUA_FED_PEERS`). Every relay-to-relay request is signed:
//!
//! ```text
//! x-aqua-fed: <unix ms>.<hex HMAC-SHA256(secret, METHOD \n PATH \n ms \n SHA256(body))>
//! ```
//!
//! Since 1.21.0-23 (AQUA-SEC-009) each relay also has its own Ed25519 node
//! key. Relays learn each other's keys from signed hellos and keep them
//! (`FedConfig::pins`); a request to a relay whose key we know carries
//!
//! ```text
//! x-aqua-fed2: <unix ms>.<nonce hex>.<sender id>.<recipient id>.<hex Ed25519 sig>
//!   over "aqua-fed-v2\nMETHOD\nPATH\nms\nnonce\nsender\nrecipient\nSHA256(body)"
//! ```
//!
//! — bound to sender and recipient, taken once. Once every peer's key is
//! known ("strict"), a request signed only with the shared secret is
//! refused except for hello / ping / the name registry: knowing the secret
//! no longer lets anyone speak as a relay of the network.
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
/// This relay's node key when the config file doesn't hold it.
const NODE_KEY_FILE: &str = "aqua-fed-node.key";

/// Write a file only its owner can read (the node key).
fn write_private(path: &str, text: &str) {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        if let Ok(mut f) = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path) {
            let _ = f.write_all(text.as_bytes());
        }
    }
    #[cfg(not(unix))]
    {
        let _ = std::fs::write(path, text);
    }
}
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
    /// This relay's Ed25519 node key (secret, hex). Made on first start.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub node_key: String,
    /// Keys of peer relays, learnt from their signed hellos.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pins: Vec<Pin>,
    /// Node ids the admin revoked: never accepted again.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub revoked: Vec<String>,
    /// After a key change: the new public key signed by the old one, sent
    /// with hellos so peers move their pin.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub rotated: String,
}

/// A peer relay's node key as we keep it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pin {
    /// How we reach it (its address or `tunnel:<node>`).
    pub url: String,
    /// Its node id (`node_id_of(key)`).
    pub id: String,
    /// Its Ed25519 public key, hex.
    pub key: String,
}

/// A node id: the first 16 bytes of SHA-256 of the public key, hex.
pub fn node_id_of(pubkey: &[u8; 32]) -> String {
    hex::encode(&Sha256::digest(pubkey)[..16])
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
        // The node key must outlive restarts even when the network is set by
        // environment variables (no config file): a new key at every start
        // would look like an impostor to peers that pinned the old one.
        if c.node_key.is_empty() {
            c.node_key = std::fs::read_to_string(NODE_KEY_FILE).ok().map(|s| s.trim().to_string()).filter(|s| s.len() == 64).unwrap_or_else(|| {
                let k = hex::encode(aqua_core::crypto::SignSecret::generate().to_bytes());
                write_private(NODE_KEY_FILE, &k);
                k
            });
        }
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
        // The node key (after a rotation) is kept where `load` reads it.
        if !self.node_key.is_empty() {
            write_private(NODE_KEY_FILE, &self.node_key);
        }
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
    key: aqua_core::crypto::SignSecret,
}

pub type FedSlot = Arc<std::sync::RwLock<Option<Arc<Fed>>>>;

pub fn slot_from_config() -> FedSlot {
    Arc::new(std::sync::RwLock::new(Fed::new(FedConfig::load())))
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn v2_message(method: &str, path: &str, ts: i64, nonce: &str, from: &str, to: &str, body: &[u8]) -> String {
    format!("aqua-fed-v2\n{method}\n{path}\n{ts}\n{nonce}\n{from}\n{to}\n{}", hex::encode(Sha256::digest(body)))
}

fn mac(secret: &str, method: &str, path: &str, ts: i64, body: &[u8]) -> String {
    let msg = format!("{method}\n{path}\n{ts}\n{}", hex::encode(Sha256::digest(body)));
    hex::encode(aqua_core::crypto::hmac_sha256(secret.as_bytes(), msg.as_bytes()))
}

impl Fed {
    pub fn new(mut cfg: FedConfig) -> Option<Arc<Self>> {
        if !cfg.active() && !cfg.registry() {
            return None;
        }
        // A config made in memory (tests, the panel before saving): a key now.
        let key = hex::decode(&cfg.node_key).ok().and_then(|b| <[u8; 32]>::try_from(b).ok()).map(aqua_core::crypto::SignSecret::from_bytes);
        let key = match key {
            Some(k) => k,
            None => {
                let k = aqua_core::crypto::SignSecret::generate();
                cfg.node_key = hex::encode(k.to_bytes());
                k
            }
        };
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .user_agent(concat!("aqua-server/", env!("CARGO_PKG_VERSION")))
            .build()
            .ok()?;
        Some(Arc::new(Self { cfg, http, key }))
    }

    /// The `x-aqua-fed` header for a request (fedtunnel.rs signs its polls).
    pub fn sign(&self, method: &str, path: &str, body: &[u8]) -> String {
        self.header(method, path, body)
    }

    /// This relay's node id and public key (hex).
    pub fn node_id(&self) -> String {
        node_id_of(self.key.public().as_bytes())
    }
    pub fn node_pub(&self) -> String {
        hex::encode(self.key.public().as_bytes())
    }

    /// The key we keep for `peer` (by address), if any.
    pub fn pin_for(&self, peer: &str) -> Option<&Pin> {
        let peer = peer.trim_end_matches('/');
        self.cfg.pins.iter().find(|p| p.url == peer)
    }

    /// Every configured peer's key is known: requests signed only with the
    /// shared secret are refused (except hello / ping / names).
    pub fn strict(&self) -> bool {
        // Also after the last keyed peer was revoked (pins of others or the
        // revocation list remain): a revoked relay can't step back to the secret.
        (!self.cfg.pins.is_empty() || !self.cfg.revoked.is_empty()) && self.cfg.peers.iter().all(|p| self.pin_for(p).is_some())
    }

    /// The `x-aqua-fed2` header for a request to `peer`, when we know its key.
    pub fn sign2(&self, peer: &str, method: &str, path: &str, body: &[u8]) -> Option<String> {
        let to = self.pin_for(peer)?.id.clone();
        let mut n = [0u8; 12];
        rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut n);
        let nonce = hex::encode(n);
        let ts = now_ms();
        let from = self.node_id();
        let sig = self.key.sign(v2_message(method, path, ts, &nonce, &from, &to, body).as_bytes());
        Some(format!("{ts}.{nonce}.{from}.{to}.{}", hex::encode(sig)))
    }

    /// Check an `x-aqua-fed2` header: `Some(sender id)` when it's good.
    fn verify2(&self, h: &str, method: &str, path: &str, body: &[u8]) -> Option<String> {
        let mut it = h.splitn(5, '.');
        let (ts, nonce, from, to, sig) = (it.next()?, it.next()?, it.next()?, it.next()?, it.next()?);
        let ts: i64 = ts.parse().ok()?;
        if (now_ms() - ts).abs() > SKEW_MS || to != self.node_id() || self.cfg.revoked.iter().any(|r| r == from) {
            return None;
        }
        let pin = self.cfg.pins.iter().find(|p| p.id == from)?;
        let key: [u8; 32] = hex::decode(&pin.key).ok()?.try_into().ok()?;
        let sig: [u8; 64] = hex::decode(sig).ok()?.try_into().ok()?;
        aqua_core::crypto::SignPublic::from_bytes(key)
            .verify(v2_message(method, path, ts, nonce, from, to, body).as_bytes(), &sig)
            .ok()?;
        first_use(&format!("v2:{from}:{nonce}"), ts).then(|| from.to_string())
    }

    /// Add both signatures to a request to `peer`.
    fn signed(&self, rb: reqwest::RequestBuilder, peer: &str, method: &str, path: &str, body: &[u8]) -> reqwest::RequestBuilder {
        let rb = rb.header("x-aqua-fed", self.header(method, path, body));
        match self.sign2(peer, method, path, body) {
            Some(s) => rb.header("x-aqua-fed2", s),
            None => rb,
        }
    }

    /// What a hello carries about this relay's key: its public key, a
    /// signature by it over our address and key (so nobody passes off a key
    /// they don't hold), and — after a key change — the new key signed by
    /// the old one.
    pub fn hello_key_fields(&self) -> serde_json::Value {
        let url = my_id(&self.cfg);
        let key = self.node_pub();
        let sig = hex::encode(self.key.sign(format!("aqua-fed-hello\n{url}\n{key}").as_bytes()));
        serde_json::json!({ "node_key": key, "node_sig": sig, "rotated": self.cfg.rotated })
    }

    /// Over the peer's reverse link, when that's the way to it: it has no
    /// address, or its address stopped answering and it polls us.
    async fn via_tunnel(&self, peer: &str, method: &str, path: &str, body: &[u8], wait: Duration) -> Option<Option<reqwest::Response>> {
        let tunnel_only = peer.starts_with("tunnel:");
        if !tunnel_only && !(is_down(peer) && crate::fedtunnel::active(peer)) {
            return None;
        }
        let sig = self.header(method, path, body);
        let sig2 = self.sign2(peer, method, path, body).unwrap_or_default();
        let r = crate::fedtunnel::request(peer, method, path, body.to_vec(), sig, sig2, wait).await;
        Some(r.filter(|r| r.status().is_success()))
    }

    fn header(&self, method: &str, path: &str, body: &[u8]) -> String {
        // Strictly increasing: two equal requests in one millisecond got the
        // same signature, and receivers now take each signature once.
        static LAST: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);
        let now = now_ms();
        let ts = match LAST.fetch_update(std::sync::atomic::Ordering::SeqCst, std::sync::atomic::Ordering::SeqCst, |l| Some(now.max(l + 1))) {
            Ok(prev) => now.max(prev + 1),
            Err(_) => now,
        };
        format!("{ts}.{}", mac(&self.cfg.secret, method, path, ts, body))
    }

    pub fn verify(&self, headers: &HeaderMap, method: &str, path: &str, body: &[u8]) -> bool {
        // A relay whose key we know signs with it (AQUA-SEC-009).
        // A sender we don't know yet (it learnt our key first) falls back to
        // the shared secret below, like a relay without keys; a known
        // sender with a bad v2 signature is refused.
        if let Some(h2) = headers.get("x-aqua-fed2").and_then(|v| v.to_str().ok()) {
            let from = h2.split('.').nth(2).unwrap_or("");
            if self.cfg.revoked.iter().any(|r| r == from) {
                return false;
            }
            if self.cfg.pins.iter().any(|p| p.id == from) {
                return self.verify2(h2, method, path, body).is_some();
            }
        }
        // Only the shared secret: fine while we don't know every peer's key
        // (older relays), and always for introducing oneself.
        let base = path.split('?').next().unwrap_or(path);
        let intro = matches!(base, "/v1/fed/hello" | "/v1/fed/ping" | "/v1/fed/names" | "/v1/fed/names/hello");
        if self.strict() && !intro {
            tracing::warn!(%path, "federation: a request signed only with the shared secret — refused (every peer has a key)");
            return false;
        }
        let Some(h) = headers.get("x-aqua-fed").and_then(|v| v.to_str().ok()) else { return false };
        let Some((ts, sig)) = h.split_once('.') else { return false };
        let Ok(ts) = ts.parse::<i64>() else { return false };
        if (now_ms() - ts).abs() > SKEW_MS {
            return false;
        }
        let want = mac(&self.cfg.secret, method, path, ts, body);
        if !bool::from(subtle::ConstantTimeEq::ct_eq(want.as_bytes(), sig.as_bytes())) {
            return false;
        }
        // AQUA-SEC-009: a signed request was good for its whole ±5-minute
        // window, as many times as anyone sent it again. Each signature is
        // now taken once (signatures cover the time, so a retry signs anew).
        first_use(sig, ts)
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
                .and_then(|v| v["version"].as_str().map(crate::relay_version_ok))
                .unwrap_or(false),
            // Unreachable now: don't decide for 10 minutes on that.
            Err(_) => { mark_down(peer); return false }
        };
        if !ok {
            tracing::warn!(%peer, "federation: this relay is older than {} — not talking to it", crate::MIN_RELAY);
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
        match self.signed(self.http.get(format!("{peer}{path}")), peer, "GET", path, b"").send().await {
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
        let sent = self.signed(self.http.post(format!("{peer}{path}")), peer, "POST", path, &body)
            .header("content-type", "application/json")
            .timeout(Duration::from_secs(4))
            .body(body.clone()).send().await;
        match sent {
            Ok(r) => r.status().is_success().then_some(r),
            Err(_) => { mark_down(peer); self.tunnel_fallback(peer, "POST", path, &body).await }
        }
    }

    /// A signed POST to one peer with its answer as is — status and text,
    /// errors included (stars.rs passes the star relay's own message on).
    pub async fn post_answer(&self, peer: &str, path: &str, body: Vec<u8>) -> Option<(u16, String)> {
        let sent = self.signed(self.http.post(format!("{peer}{path}")), peer, "POST", path, &body)
            .header("content-type", "application/json")
            .timeout(Duration::from_secs(10))
            .body(body.clone()).send().await;
        match sent {
            Ok(r) => { let st = r.status().as_u16(); Some((st, r.text().await.unwrap_or_default())) }
            Err(_) => {
                let r = self.tunnel_fallback(peer, "POST", path, &body).await?;
                let st = r.status().as_u16();
                Some((st, r.text().await.unwrap_or_default()))
            }
        }
    }

    /// The direct way failed: the reverse link, if the peer keeps one.
    async fn tunnel_fallback(&self, peer: &str, method: &str, path: &str, body: &[u8]) -> Option<reqwest::Response> {
        if !crate::fedtunnel::active(peer) {
            return None;
        }
        mark_down(peer);
        let sig = self.header(method, path, body);
        let sig2 = self.sign2(peer, method, path, body).unwrap_or_default();
        crate::fedtunnel::request(peer, method, path, body.to_vec(), sig, sig2, Duration::from_secs(30)).await.filter(|r| r.status().is_success())
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
        let mut hello = serde_json::json!({ "url": me, "peers": self.cfg.peers });
        if let (Some(o), serde_json::Value::Object(k)) = (hello.as_object_mut(), self.hello_key_fields()) { o.extend(k); }
        let body = serde_json::to_vec(&hello).unwrap_or_default();
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

    /// Say hello to one relay (after learning its key: so it learns ours).
    pub async fn hello_to(&self, peer: &str) {
        let mut hello = serde_json::json!({ "url": my_id(&self.cfg), "peers": self.cfg.peers });
        if let (Some(o), serde_json::Value::Object(k)) = (hello.as_object_mut(), self.hello_key_fields()) { o.extend(k); }
        let _ = self.post(peer, "/v1/fed/hello", serde_json::to_vec(&hello).unwrap_or_default()).await;
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
            let r = self.signed(self.http.post(format!("{p}/v1/fed/mail")), p, "POST", "/v1/fed/mail", &body)
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

    pub(crate) async fn blob_response(&self, peer: &str, cid: &str) -> Option<reqwest::Response> {
        let path = format!("/v1/fed/blob/{cid}");
        if self.modern(peer).await {
            if let Ok(response) = self.signed(self.http.get(format!("{peer}{path}")), peer, "GET", &path, b"")
                .timeout(Duration::from_secs(1200)).send().await {
                return response.status().is_success().then_some(response);
            }
        }
        self.get(peer, &path).await
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
    if !crate::caller_version(headers, None).as_deref().is_some_and(crate::relay_version_ok) {
        return Err(StatusCode::UPGRADE_REQUIRED);
    }
    Ok(fed)
}

fn my_url(cfg: &FedConfig) -> String {
    if cfg.public_url.is_empty() { crate::public::url() } else { cfg.public_url.clone() }
}

/// `GET /v1/fed/names?since=T` — the name registry (see
/// [`crate::directory::Directory::names_since`]).
pub async fn h_names(State(state): State<AppState>, headers: HeaderMap, uri: axum::http::Uri, Query(q): Query<Since>) -> Result<impl IntoResponse, StatusCode> {
    let fed = check_registry(&state, &headers, "GET", signed_path(&uri), b"")
        .or_else(|_| check_registry(&state, &headers, "GET", &format!("/v1/fed/names?since={}", q.since), b""))
        .or_else(|_| check_registry(&state, &headers, "GET", "/v1/fed/names", b""))?;
    let dir = state.directory.clone();
    let names = tokio::task::spawn_blocking(move || dir.names_after(q.since, q.after.as_deref(), 1000)).await.unwrap_or_default();
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

/// Signatures seen within the clock window: `true` the first time only.
fn first_use(sig: &str, ts: i64) -> bool {
    static SEEN: std::sync::Mutex<Option<std::collections::HashMap<String, i64>>> = std::sync::Mutex::new(None);
    let Ok(mut g) = SEEN.lock().or_else(|e| Ok::<_, ()>(e.into_inner())) else { return true };
    let seen = g.get_or_insert_with(std::collections::HashMap::new);
    let now = now_ms();
    if seen.len() > 20_000 {
        seen.retain(|_, t| (now - *t).abs() <= SKEW_MS);
    }
    if seen.len() > 200_000 {
        // Flooded with valid signatures (only a holder of the secret can make
        // them): stop remembering rather than grow without end.
        seen.clear();
    }
    seen.insert(sig.to_string(), ts).is_none()
}

/// Where we are in a peer's rows: `(updated_at, key)` of the last row read
/// (REV-001). The key breaks ties between equal timestamps, so a page that
/// ends inside a run of them goes on right after its last row — no row is
/// skipped or read twice. Relays before 1.21.0-23 ignore `after` and answer
/// `updated_at > since`: with them it works as before.
pub type Cursor = (i64, String);
type CursorMap = std::collections::HashMap<String, Cursor>;

/// `?since=T&after=KEY` (the key percent-encoded; empty key: no `after`).
fn cursor_query(c: &Cursor) -> String {
    if c.1.is_empty() {
        return format!("since={}", c.0);
    }
    let mut key = String::new();
    for b in c.1.bytes() {
        if b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-' { key.push(b as char) } else { key.push_str(&format!("%{b:02X}")) }
    }
    format!("since={}&after={key}", c.0)
}

/// The signed path of a cursor request as it arrived (with its query).
fn signed_path(uri: &axum::http::Uri) -> &str {
    uri.path_and_query().map(|p| p.as_str()).unwrap_or(uri.path())
}

/// Pull the name registry from one relay (paged).
async fn sync_names(state: &AppState, fed: &Fed, peer: &str, since: &mut CursorMap) {
    for _ in 0..50 {
        let from = since.get(peer).cloned().unwrap_or_default();
        let Some(r) = fed.get(peer, &format!("/v1/fed/names?{}", cursor_query(&from))).await else { return };
        let Ok(v) = r.json::<serde_json::Value>().await else { return };
        let home = v["home"].as_str().unwrap_or(peer).trim_end_matches('/').to_string();
        let Ok(rows) = serde_json::from_value::<Vec<crate::directory::NameRow>>(v["names"].clone()) else { return };
        if rows.is_empty() { return; }
        let n = rows.len();
        let last = rows.last().map(|r| (r.updated_at, r.username.clone())).unwrap_or(from);
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
    if !crate::caller_version(headers, None).as_deref().is_some_and(crate::relay_version_ok) {
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
    // A bot of this relay: its queue (a user of another relay wrote to it).
    let delivered = crate::deliver_to_bot(&state, &msg).await || crate::deliver_local(&state, &msg).await;
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
    let (file, len) = tokio::task::spawn_blocking(move || store.open_blob(&cid))
        .await.ok().flatten().ok_or(StatusCode::NOT_FOUND)?;
    Ok(crate::blob_upload::response(file, len))
}

#[derive(Deserialize)]
pub struct Since {
    #[serde(default)] since: i64,
    /// The key of the last row read at `since` (REV-001); older relays send none.
    #[serde(default)] after: Option<String>,
}

pub async fn h_users(State(state): State<AppState>, headers: HeaderMap, uri: axum::http::Uri, Query(q): Query<Since>) -> Result<impl IntoResponse, StatusCode> {
    check(&state, &headers, "GET", signed_path(&uri), b"")?;
    Ok(Json(state.directory.fed_users_after(q.since, q.after.as_deref())))
}

pub async fn h_backups(State(state): State<AppState>, headers: HeaderMap, uri: axum::http::Uri, Query(q): Query<Since>) -> Result<impl IntoResponse, StatusCode> {
    check(&state, &headers, "GET", signed_path(&uri), b"")?;
    Ok(Json(state.directory.fed_backups_after(q.since, q.after.as_deref())))
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

/// Navidrome of this relay for a relay of the network that has none: what
/// works on one server works on all (the owner). Read-only methods only.
pub async fn h_music_rest(State(state): State<AppState>, headers: HeaderMap, Path(method): Path<String>,
    axum::extract::RawQuery(q): axum::extract::RawQuery) -> Result<impl IntoResponse, StatusCode> {
    let path = match &q { Some(q) => format!("/v1/fed/music/rest/{method}?{q}"), None => format!("/v1/fed/music/rest/{method}") };
    check(&state, &headers, "GET", &path, b"")?;
    let v = crate::music::local_rest(&method, q.as_deref()).await.map_err(|_| StatusCode::NOT_FOUND)?;
    Ok(Json(v))
}

/// A cover / track of this relay's Navidrome for another relay (whole file).
pub async fn h_music_file(State(state): State<AppState>, headers: HeaderMap, Path((what, id)): Path<(String, String)>) -> Result<impl IntoResponse, StatusCode> {
    check(&state, &headers, "GET", &format!("/v1/fed/music/file/{what}/{id}"), b"")?;
    let (mime, data) = crate::music::local_file(&what, &id).await.ok_or(StatusCode::NOT_FOUND)?;
    Ok(([(axum::http::header::CONTENT_TYPE, mime)], data))
}

/// A public group / channel @name of this relay, for another relay of the
/// network (names are one namespace across it; lib.rs group_by_name).
pub async fn h_group_by_name(State(state): State<AppState>, headers: HeaderMap, Path(name): Path<String>) -> Result<impl IntoResponse, StatusCode> {
    check(&state, &headers, "GET", &format!("/v1/fed/groups/by-name/{name}"), b"")?;
    state.directory.group_by_name(&name).map(Json).ok_or(StatusCode::NOT_FOUND)
}

/// This relay's public groups / channels matching `q`, for the network's search.
pub async fn h_group_search(State(state): State<AppState>, headers: HeaderMap, axum::extract::RawQuery(q): axum::extract::RawQuery) -> Result<impl IntoResponse, StatusCode> {
    let path = match &q { Some(q) => format!("/v1/fed/groups/search?{q}"), None => "/v1/fed/groups/search".to_string() };
    check(&state, &headers, "GET", &path, b"")?;
    let query = q.as_deref().unwrap_or("").split('&').find_map(|kv| kv.strip_prefix("q=")).map(crate::music::pct).unwrap_or_default();
    Ok(Json(state.directory.search_groups(&query, 20)))
}

/// Ask every relay of the network (in parallel, a few seconds each) a JSON path.
impl Fed {
    pub async fn ask_all(&self, path: &str) -> Vec<serde_json::Value> {
        let asks = self.cfg.peers.iter().map(|p| async move {
            tokio::time::timeout(Duration::from_secs(5), self.music_json_at(p, path)).await.ok().flatten()
        });
        futures_util::future::join_all(asks).await.into_iter().flatten().collect()
    }
}

/// What this relay's Navidrome is called in the network (music.rs merges
/// every relay's into one): its tag and a name to show next to a track.
pub async fn h_music_info(State(state): State<AppState>, headers: HeaderMap) -> Result<impl IntoResponse, StatusCode> {
    check(&state, &headers, "GET", "/v1/fed/music/info", b"")?;
    crate::music::local_info().map(Json).ok_or(StatusCode::NOT_FOUND)
}

impl Fed {
    /// The relays of the network (their addresses), to ask each of them.
    pub fn peer_list(&self) -> Vec<String> {
        self.cfg.peers.clone()
    }

    /// One relay's answer to a music path.
    pub async fn music_json_at(&self, peer: &str, path: &str) -> Option<serde_json::Value> {
        self.get(peer, path).await?.json::<serde_json::Value>().await.ok()
    }

    /// A file from one relay (its Navidrome's track or cover).
    pub async fn fetch_bytes_at(&self, peer: &str, path: &str) -> Option<(String, Vec<u8>)> {
        let r = self.get(peer, path).await?;
        let mime = r.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("application/octet-stream").to_string();
        Some((mime, r.bytes().await.ok()?.to_vec()))
    }

}

#[derive(Deserialize)]
pub struct Hello {
    pub(crate) url: String,
    #[serde(default)] peers: Vec<String>,
    /// Its node key (hex), a signature by it over url and key, and after a
    /// key change the new key signed by the old one (1.21.0-23).
    #[serde(default)] node_key: String,
    #[serde(default)] node_sig: String,
    #[serde(default)] rotated: String,
}

/// A relay of the network says hello: it's added to our peers if new, and
/// gets the list of relays we know (that's how they find each other).
/// History records changed after `since` (ms): `[[user, key, updated_at]]`.
pub async fn h_hist_list(State(state): State<AppState>, headers: HeaderMap, uri: axum::http::Uri, Query(q): Query<Since>) -> Result<impl IntoResponse, StatusCode> {
    check(&state, &headers, "GET", signed_path(&uri), b"")?;
    Ok(Json(state.directory.fed_hist_after(q.since, q.after.as_deref())))
}

pub async fn h_hist_get(State(state): State<AppState>, headers: HeaderMap, Path((user, k)): Path<(String, String)>) -> Result<impl IntoResponse, StatusCode> {
    check(&state, &headers, "GET", &format!("/v1/fed/hist/{user}/{k}"), b"")?;
    state.directory.hist_get(&user, &k).ok_or(StatusCode::NOT_FOUND)
}

pub async fn h_hello(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Result<Json<serde_json::Value>, StatusCode> {
    let fed = check(&state, &headers, "POST", "/v1/fed/hello", &body)?;
    let h: Hello = serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;
    crate::journal::visitor(h.url.trim_end_matches('/'), "relay");
    if let Some(pin) = pin_from_hello(&fed, &h) {
        let url = pin.url.clone();
        if set_pin(&state, pin) {
            // It now knows... not yet our key: tell it, so it can sign too.
            if let Some(f) = current(&state) {
                tokio::spawn(async move { f.hello_to(&url).await; });
            }
        }
    }
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

/// The key a hello brings, when it may be kept (pinned):
/// * it must be signed by itself over the sender's address (possession);
/// * a relay we know under another key moves only with that key's
///   signature over the new one (`rotated`); otherwise it's refused;
/// * revoked node ids never come back.
fn pin_from_hello(fed: &Fed, h: &Hello) -> Option<Pin> {
    let url = h.url.trim().trim_end_matches('/').to_string();
    let key: [u8; 32] = hex::decode(h.node_key.trim()).ok()?.try_into().ok()?;
    let sig: [u8; 64] = hex::decode(h.node_sig.trim()).ok()?.try_into().ok()?;
    let pubk = aqua_core::crypto::SignPublic::from_bytes(key);
    pubk.verify(format!("aqua-fed-hello\n{url}\n{}", hex::encode(key)).as_bytes(), &sig).ok()?;
    let id = node_id_of(&key);
    if fed.cfg.revoked.contains(&id) {
        tracing::warn!(%url, %id, "federation: hello from a revoked node key — ignored");
        return None;
    }
    let pin = Pin { url: url.clone(), id, key: hex::encode(key) };
    match fed.pin_for(&url) {
        None => Some(pin),
        Some(old) if old.key == pin.key => None,
        Some(old) => {
            let ok = hex::decode(&old.key).ok().and_then(|b| <[u8; 32]>::try_from(b).ok()).zip(hex::decode(h.rotated.trim()).ok().and_then(|b| <[u8; 64]>::try_from(b).ok()))
                .is_some_and(|(ok, rs)| aqua_core::crypto::SignPublic::from_bytes(ok).verify(format!("aqua-fed-rotate\n{}", pin.key).as_bytes(), &rs).is_ok());
            if ok {
                tracing::info!(%url, "federation: a peer changed its node key (signed by the old one)");
                Some(pin)
            } else {
                tracing::warn!(%url, "federation: a hello with ANOTHER node key and no signature by the known one — ignored");
                None
            }
        }
    }
}

/// Keep `pin` (replacing the one for its address). True when it changed.
pub fn set_pin(state: &AppState, pin: Pin) -> bool {
    let Some(cur) = registry(state) else { return false };
    let mut c = cur.cfg.clone();
    if c.pins.iter().any(|p| *p == pin) { return false; }
    c.pins.retain(|p| p.url != pin.url);
    c.pins.push(pin);
    if c.ephemeral || c.save().is_ok() {
        let next = Fed::new(c);
        if let Ok(mut g) = state.fed.write() { *g = next; }
        return true;
    }
    false
}

/// Admin: a new node key. The old one signs the new one; hellos carry that,
/// so peers move their pins. Returns the new node id.
pub async fn rotate_key(state: &AppState) -> Option<String> {
    let cur = registry(state)?;
    let new = aqua_core::crypto::SignSecret::generate();
    let new_pub = hex::encode(new.public().as_bytes());
    let mut c = cur.cfg.clone();
    c.rotated = hex::encode(cur.key.sign(format!("aqua-fed-rotate\n{new_pub}").as_bytes()));
    c.node_key = hex::encode(new.to_bytes());
    if !(c.ephemeral || c.save().is_ok()) { return None; }
    let next = Fed::new(c)?;
    let id = next.node_id();
    if let Ok(mut g) = state.fed.write() { *g = Some(next.clone()); }
    for p in next.cfg.peers.clone() { next.hello_to(&p).await; }
    Some(id)
}

/// Admin: never accept this node id again (and drop its pin and address).
pub fn revoke(state: &AppState, id: &str) -> bool {
    let Some(cur) = registry(state) else { return false };
    let mut c = cur.cfg.clone();
    let urls: Vec<String> = c.pins.iter().filter(|p| p.id == id).map(|p| p.url.clone()).collect();
    c.pins.retain(|p| p.id != id);
    c.peers.retain(|p| !urls.contains(p));
    if !c.revoked.iter().any(|r| r == id) { c.revoked.push(id.to_string()); }
    if c.ephemeral || c.save().is_ok() {
        let next = Fed::new(c);
        if let Ok(mut g) = state.fed.write() { *g = next; }
        return true;
    }
    false
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
    users: CursorMap,
    backups: CursorMap,
    hist: CursorMap,
    names: CursorMap,
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
async fn sync_hist(state: &AppState, fed: &Fed, peer: &str, since: &mut CursorMap) {
    loop {
        let from = since.get(peer).cloned().unwrap_or_default();
        let Some(r) = fed.get(peer, &format!("/v1/fed/hist?{}", cursor_query(&from))).await else { return };
        let Ok(rows) = r.json::<Vec<(String, String, i64)>>().await else { return };
        if rows.is_empty() { return; }
        let n = rows.len();
        // The cursor moves row by row, and stops before a record we couldn't
        // fetch, so the next sync tries it again (AQUA-SEC-010).
        let mut cur = from;
        for (user, k, at) in &rows {
            if crate::directory::Directory::valid_name(user) && k.len() <= 64 {
                let got = match fed.get(peer, &format!("/v1/fed/hist/{user}/{k}")).await {
                    Some(b) => match b.bytes().await {
                        Ok(bytes) => { state.directory.fed_apply_hist(user, k, &bytes, *at); true }
                        Err(_) => false,
                    },
                    None => false,
                };
                if !got {
                    since.insert(peer.to_string(), cur);
                    return;
                }
            }
            cur = (*at, format!("{user}/{k}"));
        }
        since.insert(peer.to_string(), cur);
        if n < 1000 { return; }
    }
}

async fn sync_peer(
    state: &AppState,
    fed: &Fed,
    peer: &str,
    since_users: &mut CursorMap,
    since_backups: &mut CursorMap,
) {
    // Accounts (paged: 500 per call).
    loop {
        let since = since_users.get(peer).cloned().unwrap_or_default();
        let Some(r) = fed.get(peer, &format!("/v1/fed/users?{}", cursor_query(&since))).await else { return };
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
        let last = rows.last().map(|u| (u.updated_at, u.username.clone())).unwrap_or(since);
        since_users.insert(peer.to_string(), last);
        if applied > 0 {
            tracing::info!(peer, applied, "federation: accounts updated");
        }
        if rows.len() < 500 {
            break;
        }
    }
    // Encrypted backups.
    // Paged like the rest (200 per call: only the first page was read), and
    // the cursor moves only past backups we have (AQUA-SEC-010).
    for _ in 0..50 {
        let since = since_backups.get(peer).cloned().unwrap_or_default();
        let Some(r) = fed.get(peer, &format!("/v1/fed/backups?{}", cursor_query(&since))).await else { return };
        let Ok(list) = r.json::<Vec<(String, i64)>>().await else { return };
        let n = list.len();
        let mut last = since.clone();
        for (user, at) in list {
            let have = state.directory.backup_meta(&user).is_some_and(|(_, ours)| ours >= at);
            let ok = have || match fed.get(peer, &format!("/v1/fed/backup/{user}")).await {
                Some(b) => match b.bytes().await {
                    Ok(bytes) => { state.directory.fed_apply_backup(&user, &bytes, at); true }
                    Err(_) => false,
                },
                None => false,
            };
            if !ok {
                since_backups.insert(peer.to_string(), last);
                return;
            }
            last = (at, user);
        }
        let moved = last != since;
        since_backups.insert(peer.to_string(), last);
        if n < 200 || !moved { return; }
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
        .route("/v1/fed/music/info", get(h_music_info))
        .route("/v1/fed/groups/by-name/:name", get(h_group_by_name))
        .route("/v1/fed/groups/search", get(h_group_search))
        .route("/v1/fed/music/rest/:method", get(h_music_rest))
        .route("/v1/fed/music/file/:what/:id", get(h_music_file))
        .route("/v1/fed/tunnel", post(crate::fedtunnel::h_poll))
        .route("/v1/fed/tunnel/reply", post(crate::fedtunnel::h_reply)
            .layer(axum::extract::DefaultBodyLimit::max(96 * 1024 * 1024)))
}

// ---------- bots of other relays (owner: every bot works on every server) ----

/// `GET /v1/fed/bot/:name` — a bot of this relay, for the network: its
/// profile, "/" commands and menu button. 404 if it isn't one here.
pub async fn h_bot(State(state): State<AppState>, headers: HeaderMap, Path(name): Path<String>) -> Result<Json<serde_json::Value>, StatusCode> {
    let name = name.trim().trim_start_matches('@').to_lowercase();
    check(&state, &headers, "GET", &format!("/v1/fed/bot/{name}"), b"")?;
    if !state.directory.is_bot(&name) {
        return Err(StatusCode::NOT_FOUND);
    }
    let profile = state.directory.get_profile(&name).ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(serde_json::json!({
        "profile": profile,
        "commands": state.tg.commands.get(&name).map(|c| c.clone()).unwrap_or(serde_json::json!([])),
        "menu": crate::tgapi::menu_button(&state, &name),
    })))
}

/// `POST /v1/fed/webapp/init {bot, user, start_param}` — initData for one of
/// this relay's bots, for a user of another relay of the network (that relay
/// checked who they are; the request is signed for the network).
pub async fn h_webapp_init(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Result<Json<serde_json::Value>, StatusCode> {
    check(&state, &headers, "POST", "/v1/fed/webapp/init", &body)?;
    let v: serde_json::Value = serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;
    let bot = v["bot"].as_str().unwrap_or("").trim().trim_start_matches('@').to_lowercase();
    let user = v["user"].as_str().unwrap_or("").trim().to_lowercase();
    if bot.is_empty() || user.is_empty() || !state.directory.is_bot(&bot) {
        return Err(StatusCode::NOT_FOUND);
    }
    let sp = v["start_param"].as_str().unwrap_or("");
    let (init_data, signed) = crate::tgapi::init_data(&state, &bot, &user, Some(sp));
    Ok(Json(serde_json::json!({ "init_data": init_data, "signed": signed })))
}

type BotCache = std::collections::HashMap<String, (std::time::Instant, Option<(String, serde_json::Value)>)>;
static BOTS: std::sync::Mutex<Option<BotCache>> = std::sync::Mutex::new(None);

/// A bot that runs on another relay of the network: (that relay, its info).
/// Asked of every peer at once; remembered 5 minutes (a miss — 1 minute).
async fn find_bot(state: &AppState, name: &str) -> Option<(String, serde_json::Value)> {
    let name = name.trim().trim_start_matches('@').to_lowercase();
    if name.is_empty() || name.len() > 64 || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return None;
    }
    if let Some((at, hit)) = BOTS.lock().ok().and_then(|g| g.as_ref().and_then(|m| m.get(&name).cloned())) {
        let ttl = if hit.is_some() { 300 } else { 60 };
        if at.elapsed() < Duration::from_secs(ttl) {
            return hit;
        }
    }
    let f = current(state)?;
    let path = format!("/v1/fed/bot/{name}");
    let asks = f.peer_list().into_iter().map(|p| {
        let f = f.clone();
        let path = path.clone();
        async move {
            let v = tokio::time::timeout(Duration::from_secs(6), f.music_json_at(&p, &path)).await.ok().flatten()?;
            v.get("profile").is_some_and(|x| x.is_object()).then_some((p, v))
        }
    });
    let hit = futures_util::future::join_all(asks).await.into_iter().flatten().next();
    if let Ok(mut g) = BOTS.lock() {
        let m = g.get_or_insert_with(std::collections::HashMap::new);
        if m.len() > 2000 { m.clear(); }
        m.insert(name, (std::time::Instant::now(), hit.clone()));
    }
    hit
}

/// The info of a bot of another relay (`{profile, commands, menu}`).
pub async fn remote_bot(state: &AppState, name: &str) -> Option<serde_json::Value> {
    find_bot(state, name).await.map(|(_, v)| v)
}

/// initData for a bot of another relay, signed there, for `user` of this one.
pub async fn remote_webapp_init(state: &AppState, bot: &str, user: &str, start_param: &str) -> Option<serde_json::Value> {
    let (peer, _) = find_bot(state, bot).await?;
    let f = current(state)?;
    let body = serde_json::json!({ "bot": bot, "user": user, "start_param": start_param }).to_string().into_bytes();
    let (st, text) = f.post_answer(&peer, "/v1/fed/webapp/init", body).await?;
    if !(200..300).contains(&st) {
        return None;
    }
    serde_json::from_str(&text).ok()
}

#[cfg(test)]
mod cursor_tests {

    fn fed(url: &str) -> std::sync::Arc<super::Fed> {
        super::Fed::new(super::FedConfig { secret: "a-long-shared-federation-secret".into(), peers: vec![], public_url: url.into(), mode: "network".into(), ephemeral: true, ..Default::default() }).unwrap()
    }
    fn with_pins(f: &super::Fed, pins: Vec<super::Pin>, peers: Vec<String>) -> std::sync::Arc<super::Fed> {
        let mut c = f.cfg.clone();
        c.pins = pins;
        c.peers = peers;
        super::Fed::new(c).unwrap()
    }
    fn pin_of(f: &super::Fed, url: &str) -> super::Pin {
        super::Pin { url: url.into(), id: f.node_id(), key: f.node_pub() }
    }

    /// AQUA-SEC-009: a v2 signature is bound to its recipient and path and
    /// is taken once; a relay that knows every peer's key refuses requests
    /// signed only with the shared secret (except introductions).
    #[test]
    fn node_signatures_bind_recipient_and_refuse_secret_only() {
        let (a, b, c) = (fed("http://a"), fed("http://b"), fed("http://c"));
        let a = with_pins(&a, vec![pin_of(&b, "http://b"), pin_of(&c, "http://c")], vec!["http://b".into(), "http://c".into()]);
        let b = with_pins(&b, vec![pin_of(&a, "http://a")], vec!["http://a".into()]);
        let c = with_pins(&c, vec![pin_of(&a, "http://a")], vec!["http://a".into()]);
        let mut h = axum::http::HeaderMap::new();
        h.insert("x-aqua-fed2", a.sign2("http://b", "GET", "/v1/fed/users?since=0", b"").unwrap().parse().unwrap());
        // The same signature at C (another recipient): refused.
        assert!(!c.verify(&h, "GET", "/v1/fed/users?since=0", b""), "signed for B, not for C");
        // At B, for another path: refused; for its path: once.
        assert!(!b.verify(&h, "GET", "/v1/fed/backups?since=0", b""));
        assert!(b.verify(&h, "GET", "/v1/fed/users?since=0", b""));
        assert!(!b.verify(&h, "GET", "/v1/fed/users?since=0", b""), "replayed");
        // Only the shared secret: B knows all its peers' keys → refused,
        // except for hello / ping.
        assert!(b.strict());
        let mut s = axum::http::HeaderMap::new();
        s.insert("x-aqua-fed", a.header("GET", "/v1/fed/users?since=0", b"").parse().unwrap());
        assert!(!b.verify(&s, "GET", "/v1/fed/users?since=0", b""));
        let mut p = axum::http::HeaderMap::new();
        p.insert("x-aqua-fed", a.header("GET", "/v1/fed/ping", b"").parse().unwrap());
        assert!(b.verify(&p, "GET", "/v1/fed/ping", b""));
        // A known sender id with a forged signature: refused.
        let forged = fed("http://evil");
        let evil = with_pins(&forged, vec![super::Pin { url: "http://b".into(), id: b.node_id(), key: b.node_pub() }], vec![]);
        let fake = evil.sign2("http://b", "GET", "/v1/fed/users?since=0", b"").unwrap().replacen(&evil.node_id(), &a.node_id(), 1);
        let mut f2 = axum::http::HeaderMap::new();
        f2.insert("x-aqua-fed2", fake.parse().unwrap());
        assert!(!b.verify(&f2, "GET", "/v1/fed/users?since=0", b""), "a known id with another key's signature");
    }

    /// A hello pins a self-signed key; another key for the same address
    /// moves the pin only with the old key's signature; revoked ids stay out.
    #[test]
    fn hello_pins_rotate_only_when_signed_by_the_old_key() {
        let (a, b) = (fed("http://a"), fed("http://b"));
        let hello = |f: &super::Fed| -> super::Hello {
            let mut v = serde_json::json!({ "url": super::my_id(&f.cfg), "peers": [] });
            if let (Some(o), serde_json::Value::Object(k)) = (v.as_object_mut(), f.hello_key_fields()) { o.extend(k); }
            serde_json::from_value(v).unwrap()
        };
        let pin = super::pin_from_hello(&b, &hello(&a)).expect("first key is pinned");
        assert_eq!(pin.id, a.node_id());
        let b = with_pins(&b, vec![pin.clone()], vec!["http://a".into()]);
        assert!(super::pin_from_hello(&b, &hello(&a)).is_none(), "same key: nothing to change");
        // An impostor at A's address with its own key: refused.
        let imp = fed("http://a");
        assert!(super::pin_from_hello(&b, &hello(&imp)).is_none());
        // A real rotation: the old key signs the new one.
        let new = aqua_core::crypto::SignSecret::generate();
        let mut c = a.cfg.clone();
        c.rotated = hex::encode(a.key.sign(format!("aqua-fed-rotate\n{}", hex::encode(new.public().as_bytes())).as_bytes()));
        c.node_key = hex::encode(new.to_bytes());
        let a2 = super::Fed::new(c).unwrap();
        let moved = super::pin_from_hello(&b, &hello(&a2)).expect("signed rotation is accepted");
        assert_eq!(moved.id, a2.node_id());
        // Revoked: never again.
        let mut rc = b.cfg.clone();
        rc.revoked.push(a2.node_id());
        let b2 = super::Fed::new(rc).unwrap();
        assert!(super::pin_from_hello(&b2, &hello(&a2)).is_none());
    }

    #[test]
    fn a_signature_is_taken_once() {
        let ts = super::now_ms();
        assert!(super::first_use("sig-a", ts));
        assert!(!super::first_use("sig-a", ts), "replayed");
        assert!(super::first_use("sig-b", ts));
    }

    #[test]
    fn cursor_queries_encode_the_key() {
        assert_eq!(super::cursor_query(&(5, String::new())), "since=5");
        assert_eq!(super::cursor_query(&(5, "bob".into())), "since=5&after=bob");
        assert_eq!(super::cursor_query(&(5, "bob/k 1".into())), "since=5&after=bob%2Fk%201");
    }
}
