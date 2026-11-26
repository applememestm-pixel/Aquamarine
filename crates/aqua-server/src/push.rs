//! Waking a device that isn't connected (owner, 26.11): an optional push
//! signal, never needed for the network to work.
//!
//! A device registers where it can be woken (`POST /v1/me/push`):
//! * `unifiedpush` — an endpoint of the person's own UnifiedPush distributor
//!   (works without Google; any operator can use it);
//! * `gateway` — an opaque token of a push gateway (the shared FCM gateway,
//!   later APNs); the relay sends the token to the gateway it is set up with
//!   (`AQUA_PUSH_GATEWAY`) and the gateway, not the relay, holds the
//!   provider's credentials.
//!
//! What leaves the relay is only "wake up": a fixed body, no text, no
//! sender, no chat, no keys, no message id. The device then connects and
//! pulls its messages as it always does. Waking is best effort: a provider
//! may drop it (FCM after a force-stop), the queue on the relay stays.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The whole signal: the same bytes every time.
pub const WAKE_BODY: &[u8] = b"{\"aq\":\"wake\"}";
/// At most one wake per device in this long.
const DEBOUNCE: Duration = Duration::from_secs(20);
pub const KINDS: &[&str] = &["unifiedpush", "gateway"];

/// Tests point endpoints at 127.0.0.1 over http; never in production.
static ALLOW_LOCAL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub fn allow_local_for_tests() {
    ALLOW_LOCAL.store(true, std::sync::atomic::Ordering::SeqCst);
}
fn local_ok() -> bool {
    ALLOW_LOCAL.load(std::sync::atomic::Ordering::SeqCst)
}

/// The gateway this relay sends `gateway` tokens to, if any.
pub fn gateway_url() -> Option<String> {
    std::env::var("AQUA_PUSH_GATEWAY").ok().map(|s| s.trim().trim_end_matches('/').to_string()).filter(|s| s.starts_with("https://") || (local_ok() && s.starts_with("http://")))
}

/// A UnifiedPush endpoint the relay may post to: https, a host name or a
/// public address (not this machine or a private network — the relay
/// mustn't become a way into its own LAN), no credentials in it.
pub fn endpoint_ok(url: &str) -> bool {
    let Ok(u) = reqwest::Url::parse(url) else { return false };
    if url.len() > 2048 || !u.username().is_empty() || u.password().is_some() {
        return false;
    }
    match u.scheme() {
        "https" => {}
        "http" if local_ok() => return true,
        _ => return false,
    }
    let Some(host) = u.host_str() else { return false };
    match host.trim_matches(['[', ']']).parse::<std::net::IpAddr>() {
        Ok(ip) => public_ip(ip),
        Err(_) => {
            let d = host.to_ascii_lowercase();
            d != "localhost" && !d.ends_with(".localhost") && !d.ends_with(".local") && !d.ends_with(".internal") && d.contains('.')
        }
    }
}

fn public_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v) => !(v.is_private() || v.is_loopback() || v.is_link_local() || v.is_unspecified() || v.is_broadcast() || v.is_documentation() || v.octets()[0] == 100 && (v.octets()[1] & 0xc0) == 64),
        std::net::IpAddr::V6(v) => !(v.is_loopback() || v.is_unspecified() || (v.segments()[0] & 0xfe00) == 0xfc00 || (v.segments()[0] & 0xffc0) == 0xfe80 || v.to_ipv4_mapped().is_some_and(|m| !public_ip(std::net::IpAddr::V4(m)))),
    }
}

/// Every address the endpoint's host has now is public (checked again at
/// send time: a name that later points into a private network is refused).
async fn resolves_public(url: &str) -> bool {
    if local_ok() {
        return true;
    }
    let Ok(u) = reqwest::Url::parse(url) else { return false };
    let Some(host) = u.host_str().map(|h| h.trim_matches(['[', ']']).to_string()) else { return false };
    let port = u.port_or_known_default().unwrap_or(443);
    let found = tokio::net::lookup_host((host.as_str(), port)).await.map(|a| a.collect::<Vec<_>>());
    match found {
        Ok(addrs) => {
            !addrs.is_empty() && addrs.iter().all(|a| public_ip(a.ip()))
        }
        Err(_) => false,
    }
}

fn recent() -> &'static Mutex<HashMap<String, Instant>> {
    static R: std::sync::OnceLock<Mutex<HashMap<String, Instant>>> = std::sync::OnceLock::new();
    R.get_or_init(Default::default)
}

fn http() -> &'static reqwest::Client {
    static C: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    C.get_or_init(|| reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("aqua-server")
        .build()
        .unwrap_or_default())
}

/// Something waits here for `user`: wake their registered devices (each at
/// most once in [`DEBOUNCE`]). In the background; never blocks delivery.
pub fn wake(state: &crate::AppState, user: &str) {
    let rows = state.directory.push_endpoints(user);
    if rows.is_empty() {
        return;
    }
    let now = Instant::now();
    let due: Vec<_> = {
        let mut g = recent().lock().unwrap_or_else(|e| e.into_inner());
        if g.len() > 50_000 { g.retain(|_, t| now.duration_since(*t) < DEBOUNCE); }
        rows.into_iter().filter(|(dev, _, _)| {
            let key = format!("{user} {dev}");
            match g.get(&key) {
                Some(t) if now.duration_since(*t) < DEBOUNCE => false,
                _ => { g.insert(key, now); true }
            }
        }).collect()
    };
    let dir = state.directory.clone();
    let user = user.to_string();
    tokio::spawn(async move {
        for (dev, kind, endpoint) in due {
            let gone = match kind.as_str() {
                "unifiedpush" => send_unifiedpush(&endpoint).await,
                "gateway" => send_gateway(&endpoint).await,
                _ => false,
            };
            // The distributor / gateway says the endpoint is no more: forget it.
            if gone {
                dir.push_unregister(&user, &dev);
            }
        }
    });
}

/// POST the fixed body to a UnifiedPush endpoint. True when the endpoint is
/// gone (404 / 410).
async fn send_unifiedpush(endpoint: &str) -> bool {
    if !endpoint_ok(endpoint) || !resolves_public(endpoint).await {
        return false;
    }
    match http().post(endpoint).header("TTL", "86400").header("Urgency", "high").header("content-type", "application/octet-stream").body(WAKE_BODY).send().await {
        Ok(r) => matches!(r.status().as_u16(), 404 | 410),
        Err(_) => false,
    }
}

/// Hand a gateway token to the configured gateway. True when the gateway
/// says the token is no more.
async fn send_gateway(token: &str) -> bool {
    let Some(gw) = gateway_url() else { return false };
    let body = serde_json::json!({ "token": token, "wake": true });
    match http().post(format!("{gw}/v1/wake")).json(&body).send().await {
        Ok(r) => r.status().as_u16() == 410,
        Err(_) => false,
    }
}
