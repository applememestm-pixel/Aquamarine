//! Federation through a router: the reverse link.
//!
//! Relays of a network pull from each other over plain HTTP. A relay at
//! home behind a router (no port forwarded, a "grey" address) can reach a
//! public relay, but not the other way round — the public one listed it as
//! "not answering", and the link worked in one direction only.
//!
//! Now every relay also keeps a long poll open to each of its peers:
//!
//! ```text
//! POST /v1/fed/tunnel        {id, addr, v}   → [requests for me]   (≤ 25 s)
//! POST /v1/fed/tunnel/reply  [replies]
//! ```
//!
//! Both are signed with the network's secret like any federation request.
//! When the public relay can't reach a peer directly (or the peer has no
//! address at all: `tunnel:<node id>`), it queues the request — already
//! signed — for that peer's poll; the peer runs it against its own
//! federation routes and posts the answer back. Everything federation does
//! (accounts, backups, history, envelopes, files) then works both ways.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use base64::Engine;
use serde::{Deserialize, Serialize};
use tokio::sync::{oneshot, Notify};

use crate::AppState;

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;
/// A poll older than this: the peer is gone, don't queue for it.
const ALIVE: Duration = Duration::from_secs(70);
const BODY_LIMIT: usize = 64 * 1024 * 1024;

#[derive(Serialize, Deserialize, Clone)]
pub struct WireReq {
    pub rid: u64,
    pub method: String,
    /// Path with the query.
    pub path: String,
    /// Base64.
    pub body: String,
    /// The `x-aqua-fed` signature, made by the asking relay.
    pub sig: String,
}

#[derive(Serialize, Deserialize)]
pub struct WireReply {
    pub rid: u64,
    pub status: u16,
    #[serde(default)]
    pub ct: String,
    pub body: String,
}

/// The public side of one peer's reverse link.
struct Side {
    queue: Mutex<VecDeque<WireReq>>,
    notify: Notify,
    waiters: Mutex<HashMap<u64, oneshot::Sender<WireReply>>>,
    last: Mutex<Instant>,
    /// What to call it in the admin panel (its looked-up address, if any).
    label: Mutex<String>,
}

fn sides() -> &'static Mutex<HashMap<String, Arc<Side>>> {
    static S: std::sync::OnceLock<Mutex<HashMap<String, Arc<Side>>>> = std::sync::OnceLock::new();
    S.get_or_init(Default::default)
}

fn side(peer: &str) -> Option<Arc<Side>> {
    let s = sides().lock().ok()?.get(peer).cloned()?;
    let alive = s.last.lock().map(|l| l.elapsed() < ALIVE).unwrap_or(false);
    alive.then_some(s)
}

/// Does `peer` keep a reverse link to us right now?
pub fn active(peer: &str) -> bool {
    side(peer).is_some()
}

/// Peers with a live reverse link and how to call them: (id, label).
pub fn list() -> Vec<(String, String)> {
    let Ok(g) = sides().lock() else { return Vec::new() };
    g.iter()
        .filter(|(_, s)| s.last.lock().map(|l| l.elapsed() < ALIVE).unwrap_or(false))
        .map(|(k, s)| (k.clone(), s.label.lock().map(|l| l.clone()).unwrap_or_default()))
        .collect()
}

/// Send one signed request to `peer` over its reverse link.
pub async fn request(peer: &str, method: &str, path: &str, body: Vec<u8>, sig: String, wait: Duration) -> Option<reqwest::Response> {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let s = side(peer)?;
    let rid = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let (tx, rx) = oneshot::channel();
    s.waiters.lock().ok()?.insert(rid, tx);
    s.queue.lock().ok()?.push_back(WireReq { rid, method: method.into(), path: path.into(), body: B64.encode(&body), sig });
    s.notify.notify_waiters();
    let reply = match tokio::time::timeout(wait, rx).await {
        Ok(Ok(r)) => r,
        _ => {
            if let Ok(mut w) = s.waiters.lock() { w.remove(&rid); }
            return None;
        }
    };
    let bytes = B64.decode(reply.body).ok()?;
    let mut b = axum::http::Response::builder().status(reply.status);
    if !reply.ct.is_empty() {
        b = b.header("content-type", reply.ct);
    }
    b.body(bytes).ok().map(reqwest::Response::from)
}

#[derive(Deserialize)]
pub struct Poll {
    /// How the polling relay is known in peer lists (its address, or
    /// `tunnel:<node id>` without one).
    pub id: String,
    /// The address it looked up but can't be reached at (panel label).
    #[serde(default)]
    pub addr: String,
    #[serde(default)]
    pub v: String,
}

/// A peer waits here for requests meant for it.
pub async fn h_poll(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Result<Json<Vec<WireReq>>, StatusCode> {
    crate::fed::check_pub(&state, &headers, "POST", "/v1/fed/tunnel", &body)?;
    let p: Poll = serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;
    let id = p.id.trim().trim_end_matches('/').to_string();
    if !(id.starts_with("http") || id.starts_with("tunnel:")) || id.len() > 200 {
        return Err(StatusCode::BAD_REQUEST);
    }
    if !p.v.is_empty() && !crate::version_ok(&p.v) {
        return Err(StatusCode::UPGRADE_REQUIRED);
    }
    let s = {
        let mut g = sides().lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        g.entry(id.clone())
            .or_insert_with(|| Arc::new(Side {
                queue: Mutex::new(VecDeque::new()),
                notify: Notify::new(),
                waiters: Mutex::new(HashMap::new()),
                last: Mutex::new(Instant::now()),
                label: Mutex::new(String::new()),
            }))
            .clone()
    };
    if let Ok(mut l) = s.last.lock() { *l = Instant::now(); }
    if let Ok(mut l) = s.label.lock() { *l = p.addr.trim().trim_end_matches('/').to_string(); }
    // It signed with our secret: it's of our network. Know it by this id,
    // instead of an address of it that never answered.
    crate::fed::adopt_peer(&state, &id, p.addr.trim().trim_end_matches('/'));
    let deadline = Instant::now() + Duration::from_secs(25);
    loop {
        let wait = s.notify.notified();
        let batch: Vec<WireReq> = s.queue.lock().map(|mut q| q.drain(..).collect()).unwrap_or_default();
        if !batch.is_empty() {
            return Ok(Json(batch));
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() || tokio::time::timeout(left, wait).await.is_err() {
            if let Ok(mut l) = s.last.lock() { *l = Instant::now(); }
            return Ok(Json(Vec::new()));
        }
    }
}

/// Answers to the requests handed out by `h_poll`.
pub async fn h_reply(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Result<StatusCode, StatusCode> {
    crate::fed::check_pub(&state, &headers, "POST", "/v1/fed/tunnel/reply", &body)?;
    let replies: Vec<WireReply> = serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;
    let all = sides().lock().map(|g| g.values().cloned().collect::<Vec<_>>()).unwrap_or_default();
    for r in replies {
        for s in &all {
            let tx = s.waiters.lock().ok().and_then(|mut w| w.remove(&r.rid));
            if let Some(tx) = tx {
                let _ = tx.send(r);
                break;
            }
        }
    }
    Ok(StatusCode::NO_CONTENT)
}

// ------------------------------------------------------------ polling side

/// Keep a reverse link open to every peer that has an address. Started
/// with the relay; follows the peer list as it changes.
pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        let running: Arc<Mutex<std::collections::HashSet<String>>> = Default::default();
        let router = crate::fed::routes().with_state(state.clone());
        loop {
            if let Some(fed) = crate::fed::current(&state) {
                for peer in fed.cfg.peers.iter().filter(|p| p.starts_with("http")) {
                    let fresh = running.lock().map(|mut r| r.insert(peer.clone())).unwrap_or(false);
                    if fresh {
                        let (st, peer, running, router) = (state.clone(), peer.clone(), running.clone(), router.clone());
                        tokio::spawn(async move {
                            poll_loop(&st, &peer, router).await;
                            if let Ok(mut r) = running.lock() { r.remove(&peer); }
                        });
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(15)).await;
        }
    });
}

async fn poll_loop(state: &AppState, peer: &str, router: axum::Router) {
    let http = match reqwest::Client::builder()
        .user_agent(concat!("aqua-server/", env!("CARGO_PKG_VERSION")))
        .build()
    {
        Ok(h) => h,
        Err(_) => return,
    };
    let mut fails = 0u32;
    loop {
        let Some(fed) = crate::fed::current(state) else { return };
        if !fed.cfg.peers.iter().any(|p| p == peer) {
            return;
        }
        let body = serde_json::to_vec(&serde_json::json!({
            "id": crate::fed::my_id(&fed.cfg),
            "addr": crate::public::unverified(),
            "v": env!("CARGO_PKG_VERSION"),
        })).unwrap_or_default();
        let r = http.post(format!("{peer}/v1/fed/tunnel"))
            .header("x-aqua-fed", fed.sign("POST", "/v1/fed/tunnel", &body))
            .header("content-type", "application/json")
            .timeout(Duration::from_secs(40))
            .body(body)
            .send()
            .await;
        let reqs: Vec<WireReq> = match r {
            Ok(r) if r.status().is_success() => r.json().await.unwrap_or_default(),
            // An older relay without the route, or the peer is down: try
            // again later, quietly.
            _ => {
                fails = (fails + 1).min(6);
                tokio::time::sleep(Duration::from_secs(5 * u64::from(fails))).await;
                continue;
            }
        };
        fails = 0;
        for q in reqs {
            let (st, peer, http, router) = (state.clone(), peer.to_string(), http.clone(), router.clone());
            tokio::spawn(async move {
                let reply = run_local(router, q).await;
                let Some(fed) = crate::fed::current(&st) else { return };
                let body = serde_json::to_vec(&[reply]).unwrap_or_default();
                let _ = http.post(format!("{peer}/v1/fed/tunnel/reply"))
                    .header("x-aqua-fed", fed.sign("POST", "/v1/fed/tunnel/reply", &body))
                    .header("content-type", "application/json")
                    .timeout(Duration::from_secs(60))
                    .body(body)
                    .send()
                    .await;
            });
        }
    }
}

/// Run a peer's request against our own federation routes (in-process:
/// the signature it carries is checked there as usual).
async fn run_local(router: axum::Router, q: WireReq) -> WireReply {
    use tower::ServiceExt;
    let fail = |status: u16| WireReply { rid: q.rid, status, ct: String::new(), body: String::new() };
    if !q.path.starts_with("/v1/fed/") || q.path.starts_with("/v1/fed/tunnel") {
        return fail(403);
    }
    let Ok(body) = B64.decode(&q.body) else { return fail(400) };
    let Ok(req) = axum::http::Request::builder()
        .method(q.method.as_str())
        .uri(q.path.as_str())
        .header("x-aqua-fed", q.sig.as_str())
        .header("content-type", "application/json")
        .body(axum::body::Body::from(body))
    else {
        return fail(400);
    };
    let resp = match router.oneshot(req).await { Ok(r) => r, Err(e) => match e {} };
    let status = resp.status().as_u16();
    let ct = resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    match axum::body::to_bytes(resp.into_body(), BODY_LIMIT).await {
        Ok(b) => WireReply { rid: q.rid, status, ct, body: B64.encode(&b) },
        Err(_) => fail(502),
    }
}
