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
//!
//! Review of #113 (27.11):
//! * the address is checked by the connection's own lookup (`PublicOnly`):
//!   a name checked once and looked up again to connect let DNS rebinding
//!   reach this machine or its LAN; special IPv4 blocks and IPv6 forms that
//!   carry an IPv4 address (mapped, compatible, NAT64, 6to4) are checked too;
//!   redirects are never followed;
//! * a wake held back by the limit is sent when the limit ends (it was lost:
//!   the device woken by the first message may be asleep by the second);
//! * a row of a device that is no longer the account's is dropped, not woken;
//! * the gateway gets this relay's key (`AQUA_PUSH_GATEWAY_KEY`) with each
//!   wake, so it can tell relays apart and refuse strangers.
//! What remains: the time and rate of wakes are visible to the distributor
//! or gateway (and to the provider behind it) — the content is not.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The whole signal: the same bytes every time.
pub const WAKE_BODY: &[u8] = b"{\"aq\":\"wake\"}";
/// At most one wake per device in this long (then one more, if any was held back).
static DEBOUNCE_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(20_000);
fn debounce() -> Duration {
    Duration::from_millis(DEBOUNCE_MS.load(std::sync::atomic::Ordering::Relaxed))
}
pub fn set_debounce_for_tests(d: Duration) {
    DEBOUNCE_MS.store(d.as_millis() as u64, std::sync::atomic::Ordering::Relaxed);
}
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
    use std::net::{IpAddr, Ipv4Addr};
    match ip {
        IpAddr::V4(v) => {
            let o = v.octets();
            !(v.is_private() || v.is_loopback() || v.is_link_local() || v.is_unspecified() || v.is_broadcast() || v.is_documentation()
                || v.is_multicast()
                || o[0] == 0                                  // "this network"
                || o[0] == 100 && (o[1] & 0xc0) == 64         // shared (CGNAT)
                || o[0] == 192 && o[1] == 0 && o[2] == 0      // IETF protocol assignments
                || o[0] == 198 && (o[1] & 0xfe) == 18         // benchmarking
                || o[0] >= 240)                               // reserved
        }
        IpAddr::V6(v) => {
            let s = v.segments();
            let v4 = |a: u16, b: u16| public_ip(IpAddr::V4(Ipv4Addr::new((a >> 8) as u8, a as u8, (b >> 8) as u8, b as u8)));
            if let Some(m) = v.to_ipv4_mapped() {
                return public_ip(IpAddr::V4(m));
            }
            if s[..6] == [0; 6] {
                // ::a.b.c.d (IPv4-compatible) and ::/96 as a whole.
                return false;
            }
            if s[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
                return v4(s[6], s[7]); // NAT64
            }
            if s[0] == 0x2002 {
                return v4(s[1], s[2]); // 6to4
            }
            !(v.is_loopback() || v.is_unspecified() || v.is_multicast()
                || (s[0] & 0xfe00) == 0xfc00                 // unique local
                || (s[0] & 0xffc0) == 0xfe80                 // link local
                || (s[0] & 0xffc0) == 0xfec0                 // site local (old)
                || s[0] == 0x2001 && s[1] == 0x0db8          // documentation
                || s[0] == 0x2001 && s[1] == 0               // Teredo: the IPv4 end is hidden
                || s[..4] == [0x100, 0, 0, 0])               // discard
        }
    }
}

/// The connection's own lookup: every address of the name is public, or
/// none is used. In the client itself, so the address checked is the one
/// connected to (no second lookup to rebind).
pub async fn lookup_public(host: &str) -> Result<Vec<std::net::SocketAddr>, Box<dyn std::error::Error + Send + Sync>> {
    if let Ok(ip) = host.trim_matches(['[', ']']).parse::<std::net::IpAddr>() {
        return if public_ip(ip) || local_ok() { Ok(vec![(ip, 0).into()]) } else { Err("not a public address".into()) };
    }
    let addrs: Vec<_> = tokio::net::lookup_host((host, 0)).await?.collect();
    if addrs.is_empty() {
        return Err("no address".into());
    }
    if !local_ok() && !addrs.iter().all(|a| public_ip(a.ip())) {
        tracing::warn!(%host, "push: a wake endpoint's name points at a private address — not sent");
        return Err("a private address".into());
    }
    Ok(addrs)
}

struct PublicOnly;
impl reqwest::dns::Resolve for PublicOnly {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_string();
        Box::pin(async move {
            let addrs = lookup_public(&host).await?;
            Ok(Box::new(addrs.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

/// Per device: when it was last woken, and whether one more wake waits for
/// the limit to end.
fn recent() -> &'static Mutex<HashMap<String, (Instant, bool)>> {
    static R: std::sync::OnceLock<Mutex<HashMap<String, (Instant, bool)>>> = std::sync::OnceLock::new();
    R.get_or_init(Default::default)
}

/// The client every wake goes through — with its checks (no redirects, the
/// public-only lookup). If it can't be built, no wake is sent (review of
/// #115: a plain default client without them took its place).
fn http() -> Option<&'static reqwest::Client> {
    static C: std::sync::OnceLock<Option<reqwest::Client>> = std::sync::OnceLock::new();
    C.get_or_init(|| match reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::none())
        .dns_resolver(std::sync::Arc::new(PublicOnly))
        .user_agent("aqua-server")
        .build()
    {
        Ok(c) => Some(c),
        Err(e) => {
            tracing::error!(%e, "push: the wake client can't be built — no wakes are sent");
            None
        }
    })
    .as_ref()
}

/// At most this many wakes are sent at once…
static AT_ONCE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(32);
/// …and this many wait or run in all; past it a wake is dropped (the queue
/// on the relay keeps the messages; the device gets them when it connects).
static QUEUED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(4096);
static IN_FLIGHT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
pub fn set_limits_for_tests(at_once: usize, queued: usize) {
    AT_ONCE.store(at_once.max(1), std::sync::atomic::Ordering::SeqCst);
    QUEUED.store(queued.max(1), std::sync::atomic::Ordering::SeqCst);
}
fn permits() -> &'static tokio::sync::Semaphore {
    static S: std::sync::OnceLock<tokio::sync::Semaphore> = std::sync::OnceLock::new();
    S.get_or_init(|| tokio::sync::Semaphore::new(AT_ONCE.load(std::sync::atomic::Ordering::SeqCst)))
}
/// A place among the wakes waiting or running; None when they're full.
struct Slot;
impl Slot {
    fn take() -> Option<Slot> {
        let max = QUEUED.load(std::sync::atomic::Ordering::SeqCst);
        IN_FLIGHT
            .fetch_update(std::sync::atomic::Ordering::SeqCst, std::sync::atomic::Ordering::SeqCst, |n| (n < max).then_some(n + 1))
            .ok()
            .map(|_| Slot)
    }
}
impl Drop for Slot {
    fn drop(&mut self) {
        IN_FLIGHT.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

fn is_device_of(dir: &crate::directory::Directory, user: &str, dev: &str) -> bool {
    hex::decode(dev).ok().and_then(|b| <[u8; 32]>::try_from(b).ok()).is_some_and(|dh| dir.device_role(user, &dh).is_some())
}

/// Forget where devices that are no longer `user`'s were woken (a device
/// signed out, the primary one replaced).
pub fn drop_stale(dir: &crate::directory::Directory, user: &str) {
    for (dev, _, _) in dir.push_endpoints(user) {
        if !is_device_of(dir, user, &dev) {
            dir.push_unregister(user, &dev);
        }
    }
}

/// Something waits here for `user`: wake their registered devices — at once,
/// or, within [`debounce`] of the last wake, once more when it ends. In the
/// background; never blocks delivery.
pub fn wake(state: &crate::AppState, user: &str) {
    let rows = state.directory.push_endpoints(user);
    if rows.is_empty() {
        return;
    }
    // Only devices that are still the account's (signed out → dropped).
    let rows: Vec<_> = rows.into_iter().filter(|(dev, _, _)| {
        let ours = is_device_of(&state.directory, user, dev);
        if !ours { state.directory.push_unregister(user, dev); }
        ours
    }).collect();
    let now = Instant::now();
    let limit = debounce();
    let mut plan = Vec::new();
    {
        let mut g = recent().lock().unwrap_or_else(|e| e.into_inner());
        if g.len() > 50_000 { g.retain(|_, (t, pending)| *pending || now.duration_since(*t) < limit); }
        for row in rows {
            let key = format!("{user} {}", row.0);
            match g.get_mut(&key) {
                Some((t, _)) if now.duration_since(*t) >= limit => { *t = now; plan.push((row, Duration::ZERO, key)); }
                None => { g.insert(key.clone(), (now, false)); plan.push((row, Duration::ZERO, key)); }
                // Held back: one more when the limit ends (not one per message).
                Some((t, pending @ false)) => { *pending = true; plan.push((row, limit - now.duration_since(*t), key)); }
                Some((_, true)) => {}
            }
        }
    }
    let dir = state.directory.clone();
    let user = user.to_string();
    for ((dev, _, _), delay, key) in plan {
        let Some(slot) = Slot::take() else {
            tracing::warn!("push: too many wakes waiting — this one is dropped (the messages wait on the relay)");
            if !delay.is_zero() {
                let mut g = recent().lock().unwrap_or_else(|e| e.into_inner());
                if let Some(e) = g.get_mut(&key) { e.1 = false; }
            }
            continue;
        };
        let (dir, user) = (dir.clone(), user.clone());
        tokio::spawn(async move {
            let _slot = slot;
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
                let mut g = recent().lock().unwrap_or_else(|e| e.into_inner());
                g.insert(key, (Instant::now(), false));
            }
            let Ok(_permit) = permits().acquire().await else { return };
            // Where this device is woken *now* (review of #115: a held-back
            // wake went to the address it was planned for, after the device
            // changed it or stopped being woken), and only if it is still
            // the account's.
            let Some((kind, endpoint)) = dir.push_endpoint(&user, &dev) else { return };
            if !is_device_of(&dir, &user, &dev) {
                dir.push_unregister(&user, &dev);
                return;
            }
            let gone = match kind.as_str() {
                "unifiedpush" => send_unifiedpush(&endpoint).await,
                "gateway" => send_gateway(&endpoint).await,
                _ => false,
            };
            // The distributor / gateway says this address is no more: forget
            // it — only if it is still the one kept (a slow answer about an
            // old address mustn't remove the new one).
            if gone {
                dir.push_unregister_if(&user, &dev, &endpoint);
            }
        });
    }
}

/// POST the fixed body to a UnifiedPush endpoint. True when the endpoint is
/// gone (404 / 410).
async fn send_unifiedpush(endpoint: &str) -> bool {
    // The address itself is checked by the client's lookup (`PublicOnly`).
    if !endpoint_ok(endpoint) {
        return false;
    }
    let Some(c) = http() else { return false };
    match c.post(endpoint).header("TTL", "86400").header("Urgency", "high").header("content-type", "application/octet-stream").body(WAKE_BODY).send().await {
        Ok(r) => matches!(r.status().as_u16(), 404 | 410),
        Err(_) => false,
    }
}

/// Hand a gateway token to the configured gateway. True when the gateway
/// says the token is no more.
async fn send_gateway(token: &str) -> bool {
    let Some(gw) = gateway_url() else { return false };
    let body = serde_json::json!({ "token": token, "wake": true });
    let Some(c) = http() else { return false };
    let mut rq = c.post(format!("{gw}/v1/wake")).json(&body);
    // The gateway tells relays apart by the key it gave this one.
    if let Some(key) = std::env::var("AQUA_PUSH_GATEWAY_KEY").ok().filter(|k| !k.trim().is_empty()) {
        rq = rq.bearer_auth(key.trim());
    }
    match rq.send().await {
        Ok(r) => r.status().as_u16() == 410,
        Err(_) => false,
    }
}

// Here, not in tests/push.rs: those tests allow local addresses for the
// whole process (`allow_local_for_tests`), and these ran beside them.
#[cfg(test)]
mod tests {
    #[test]
    fn endpoints_must_be_public_https() {
        use super::endpoint_ok;
        assert!(endpoint_ok("https://up.example.org/UP?token=abc"));
        assert!(endpoint_ok("https://8.8.8.8/x"));
        for bad in ["http://up.example.org/x", "https://localhost/x", "https://127.0.0.1/x", "https://10.0.0.5/x", "https://192.168.1.1/x",
                    "https://169.254.169.254/latest", "https://[::1]/x", "https://[fd00::1]/x", "https://100.64.0.1/x", "https://user:pw@up.example.org/x",
                    "https://printer.local/x", "https://intranet/x", "ftp://up.example.org/x", "not a url"] {
            assert!(!endpoint_ok(bad), "{bad}");
        }
    }

    /// Review of #113 (27.11): ranges that reach this machine or a private
    /// network through IPv6 or special IPv4 blocks were taken as public.
    #[test]
    fn special_and_embedded_addresses_are_not_public() {
        use super::endpoint_ok;
        for bad in ["https://0.1.2.3/x", "https://224.0.0.1/x", "https://192.0.0.8/x", "https://198.18.0.1/x", "https://240.0.0.1/x",
                    "https://[ff02::1]/x", "https://[2001:db8::1]/x", "https://[::127.0.0.1]/x", "https://[::ffff:10.0.0.1]/x",
                    "https://[64:ff9b::a00:1]/x", "https://[2002:c0a8:101::1]/x", "https://[2002:7f00:1::1]/x", "https://[fec0::1]/x",
                    "https://[2001::1]/x", "https://[100::1]/x"] {
            assert!(!endpoint_ok(bad), "{bad}");
        }
        assert!(endpoint_ok("https://[2606:4700::1111]/x"));
        assert!(endpoint_ok("https://[64:ff9b::808:808]/x"), "NAT64 of a public address");
    }

    /// Review of #113: the name was checked, then looked up again to connect —
    /// a name that answers "public" first and "127.0.0.1" next (DNS rebinding)
    /// got through. Now the connection's own lookup refuses private answers.
    #[tokio::test]
    async fn the_connection_lookup_refuses_private_answers() {
        assert!(super::lookup_public("localhost").await.is_err(), "localhost resolves to a loopback address");
        assert!(super::lookup_public("127.0.0.1").await.is_err());
    }
}
