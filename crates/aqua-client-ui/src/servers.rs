//! The server picker on the sign-in screen: for every relay address the
//! app knows, where it actually is (IP, port, rough location) and whether
//! it answers right now; plus a sweep of the local network for relays
//! (a PC running aqua-server, seen from a phone on the same Wi-Fi).

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use futures_util::future::join_all;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ServerProbe {
    /// The address as given, normalised (`http://host:port`).
    pub url: String,
    pub host: String,
    pub port: u16,
    pub ip: Option<String>,
    /// "loopback" | "lan" | "emulator" | "public"
    pub scope: Option<String>,
    /// Answered `/v1/stats` (it is an Aquamarine relay and it is up).
    pub ok: bool,
    /// Round trip of that request.
    pub ms: Option<u64>,
    pub users: Option<u64>,
    pub online: Option<u64>,
    pub country: Option<String>,
    pub country_code: Option<String>,
    pub city: Option<String>,
}

fn split_url(url: &str) -> Option<(String, String, u16)> {
    let u = url.trim();
    let (scheme, rest) = match u.split_once("://") {
        Some((s, r)) => (s.to_ascii_lowercase(), r),
        None => ("http".to_string(), u),
    };
    let authority = rest.split('/').next().unwrap_or("");
    if authority.is_empty() || authority.ends_with(':') {
        return None;
    }
    let default = if scheme == "https" || scheme == "wss" { 443 } else { 80 };
    let (host, port) = if let Some(v6) = authority.strip_prefix('[') {
        let (h, tail) = v6.split_once(']')?;
        (h.to_string(), tail.strip_prefix(':').and_then(|p| p.parse().ok()).unwrap_or(default))
    } else {
        match authority.rsplit_once(':') {
            Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty() => (h.to_string(), p.parse().ok()?),
            _ => (authority.to_string(), default),
        }
    };
    let http = if scheme == "https" || scheme == "wss" { "https" } else { "http" };
    let shown = if host.contains(':') { format!("[{host}]") } else { host.clone() };
    Some((format!("{http}://{shown}:{port}"), host, port))
}

fn scope_of(ip: &IpAddr) -> &'static str {
    match ip {
        IpAddr::V4(v4) if v4.is_loopback() => "loopback",
        IpAddr::V4(v4) if *v4 == Ipv4Addr::new(10, 0, 2, 2) => "emulator",
        IpAddr::V4(v4) if v4.is_private() || v4.is_link_local() || v4.octets()[0] == 100 && (64..128).contains(&v4.octets()[1]) => "lan",
        IpAddr::V6(v6) if v6.is_loopback() => "loopback",
        IpAddr::V6(v6) if (v6.segments()[0] & 0xfe00) == 0xfc00 || (v6.segments()[0] & 0xffc0) == 0xfe80 => "lan",
        _ => "public",
    }
}

#[derive(Clone, Default)]
struct Geo {
    country: Option<String>,
    country_code: Option<String>,
    city: Option<String>,
}

fn geo_cache() -> &'static Mutex<HashMap<IpAddr, Geo>> {
    static C: std::sync::OnceLock<Mutex<HashMap<IpAddr, Geo>>> = std::sync::OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Rough location of a public IP (country / city) from ipwho.is; cached.
async fn geo(http: &reqwest::Client, ip: IpAddr) -> Geo {
    if let Some(g) = geo_cache().lock().ok().and_then(|c| c.get(&ip).cloned()) {
        return g;
    }
    let url = format!("https://ipwho.is/{ip}?fields=success,country,country_code,city");
    let g = match http.get(url).timeout(Duration::from_secs(4)).send().await {
        Ok(r) => match r.json::<serde_json::Value>().await {
            Ok(v) if v["success"].as_bool() == Some(true) => Geo {
                country: v["country"].as_str().map(str::to_string),
                country_code: v["country_code"].as_str().map(str::to_string),
                city: v["city"].as_str().filter(|s| !s.is_empty()).map(str::to_string),
            },
            _ => Geo::default(),
        },
        Err(_) => return Geo::default(), // offline: don't cache, try again next time
    };
    if let Ok(mut c) = geo_cache().lock() {
        c.insert(ip, g.clone());
    }
    g
}

async fn probe_one(http: reqwest::Client, raw: String) -> Option<ServerProbe> {
    let (url, host, port) = split_url(&raw)?;
    let ip = match host.parse::<IpAddr>() {
        Ok(ip) => Some(ip),
        Err(_) => tokio::time::timeout(Duration::from_secs(3), tokio::net::lookup_host((host.as_str(), port)))
            .await
            .ok()
            .and_then(|r| r.ok())
            .and_then(|mut it| it.next())
            .map(|sa| sa.ip()),
    };
    let t0 = Instant::now();
    let stats = http.get(format!("{url}/v1/stats")).timeout(Duration::from_secs(3)).send().await;
    let ms = t0.elapsed().as_millis() as u64;
    let body = match stats {
        Ok(r) if r.status().is_success() => r.json::<serde_json::Value>().await.ok(),
        _ => None,
    };
    let ok = body.as_ref().and_then(|v| v["ok"].as_bool()).unwrap_or(false);
    let scope = ip.as_ref().map(scope_of);
    let g = match ip {
        Some(ip) if scope == Some("public") => geo(&http, ip).await,
        _ => Geo::default(),
    };
    Some(ServerProbe {
        url,
        host,
        port,
        ip: ip.map(|i| i.to_string()),
        scope: scope.map(str::to_string),
        ok,
        ms: ok.then_some(ms),
        users: body.as_ref().and_then(|v| v["users"].as_u64()),
        online: body.as_ref().and_then(|v| v["online"].as_u64()),
        country: g.country,
        country_code: g.country_code,
        city: g.city,
    })
}

/// Probes every address at once (each gives up after a few seconds).
#[tauri::command]
pub async fn probe_servers(urls: Vec<String>) -> Result<Vec<ServerProbe>, String> {
    let http = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(2))
        .build()
        .map_err(|e| e.to_string())?;
    let mut seen = std::collections::HashSet::new();
    let jobs = urls
        .into_iter()
        .filter(|u| split_url(u).map(|(n, _, _)| seen.insert(n)).unwrap_or(false))
        .take(40)
        .map(|u| probe_one(http.clone(), u));
    Ok(join_all(jobs).await.into_iter().flatten().collect())
}

/// This device's IPv4 address on the local network (no packet is sent).
fn local_ipv4() -> Option<Ipv4Addr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("1.1.1.1:80").ok()?;
    match s.local_addr().ok()?.ip() {
        IpAddr::V4(v4) if !v4.is_loopback() => Some(v4),
        _ => None,
    }
}

/// Looks for relays on port `port` (7777 by default) across this device's
/// /24 network: a TCP knock on every address, then `/v1/stats` on the ones
/// that answered. Takes about a second.
#[tauri::command]
pub async fn scan_lan_relays(port: Option<u16>) -> Result<Vec<String>, String> {
    let port = port.unwrap_or(7777);
    let Some(me) = local_ipv4() else { return Ok(vec![]) };
    let [a, b, c, _] = me.octets();
    let knocks = (1u8..=254).map(|d| async move {
        let addr = SocketAddr::from((Ipv4Addr::new(a, b, c, d), port));
        match tokio::time::timeout(Duration::from_millis(700), tokio::net::TcpStream::connect(addr)).await {
            Ok(Ok(_)) => Some(addr),
            _ => None,
        }
    });
    let open: Vec<SocketAddr> = join_all(knocks).await.into_iter().flatten().collect();
    let http = reqwest::Client::builder().timeout(Duration::from_secs(2)).build().map_err(|e| e.to_string())?;
    let checks = open.into_iter().map(|addr| {
        let http = http.clone();
        async move {
            let url = format!("http://{addr}");
            let ok = match http.get(format!("{url}/v1/stats")).send().await {
                Ok(r) => r.json::<serde_json::Value>().await.ok().and_then(|v| v["ok"].as_bool()).unwrap_or(false),
                Err(_) => false,
            };
            ok.then_some(url)
        }
    });
    Ok(join_all(checks).await.into_iter().flatten().collect())
}

/// One relay from the project's public list (site/servers.json).
#[derive(Debug, Clone, Serialize)]
pub struct PublicServer {
    pub url: String,
    pub name: Option<String>,
    pub note: Option<String>,
}

/// Where the public list is published: the project site, its GitHub Pages
/// address (until the domain has its certificate) and the file in the repo.
const PUBLIC_LISTS: &[&str] = &[
    "https://aquamarinemessenger.pw/servers.json",
    "https://applememestm-pixel.github.io/Aquamarine/servers.json",
    "https://raw.githubusercontent.com/applememestm-pixel/Aquamarine/main/site/servers.json",
];

fn parse_public(v: &serde_json::Value) -> Vec<PublicServer> {
    v["servers"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|e| {
                    let url = e["url"].as_str().or_else(|| e.as_str())?;
                    let (url, _, _) = split_url(url)?;
                    let text = |k: &str| e[k].as_str().map(|s| s.chars().take(80).collect::<String>());
                    Some(PublicServer { url, name: text("name"), note: text("note") })
                })
                .take(100)
                .collect()
        })
        .unwrap_or_default()
}

/// Relays anyone can sign up on, from the project's public list — so a
/// phone on mobile data (no local network to search) still has servers to
/// pick from. The first address that answers wins.
#[tauri::command]
pub async fn public_servers() -> Result<Vec<PublicServer>, String> {
    let http = aqua_client_core::transport::http_client().map_err(|e| e.to_string())?;
    let mut last = String::from("no list");
    for url in PUBLIC_LISTS {
        match http.get(*url).timeout(Duration::from_secs(6)).send().await {
            Ok(r) if r.status().is_success() => match r.bytes().await.ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()) {
                Some(v) => return Ok(parse_public(&v)),
                None => last = format!("{url}: not JSON"),
            },
            Ok(r) => last = format!("{url}: HTTP {}", r.status()),
            Err(e) => last = format!("{url}: {e}"),
        }
    }
    Err(last)
}

/// The ping next to the title on the main page: one round trip to the relay
/// we're on. The connection is kept alive between calls, so it measures the
/// network, not a fresh TCP handshake every time.
#[tauri::command]
pub async fn server_ping(url: String) -> Result<u64, String> {
    static HTTP: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    let (url, _, _) = split_url(&url).ok_or("bad server address")?;
    let http = match HTTP.get() {
        Some(c) => c.clone(),
        None => {
            let c = reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(3))
                .timeout(Duration::from_secs(5))
                .pool_idle_timeout(Duration::from_secs(90))
                .build()
                .map_err(|e| e.to_string())?;
            HTTP.get_or_init(|| c).clone()
        }
    };
    let t0 = Instant::now();
    let r = http.get(format!("{url}/v1/stats")).send().await.map_err(|e| e.to_string())?;
    let ms = t0.elapsed().as_millis() as u64;
    if !r.status().is_success() {
        return Err(format!("HTTP {}", r.status()));
    }
    let _ = r.bytes().await; // drain, so the connection goes back to the pool
    Ok(ms)
}

#[cfg(test)]
mod tests {
    #[test]
    fn public_list() {
        let v = serde_json::json!({ "servers": [
            { "url": "http://203.0.113.10:7777/", "name": "A" },
            "relay.example.org:7777",
            { "url": "not a url at all ::" },
        ]});
        let l = super::parse_public(&v);
        assert_eq!(l[0].url, "http://203.0.113.10:7777");
        assert_eq!(l[0].name.as_deref(), Some("A"));
        assert!(l.len() >= 2);
    }

    use super::*;

    #[test]
    fn urls() {
        assert_eq!(split_url("http://1.2.3.4:7777/").unwrap(), ("http://1.2.3.4:7777".into(), "1.2.3.4".into(), 7777));
        assert_eq!(split_url("relay.example.org").unwrap(), ("http://relay.example.org:80".into(), "relay.example.org".into(), 80));
        assert_eq!(split_url("https://relay.example.org").unwrap().2, 443);
        assert_eq!(split_url("http://[::1]:7777").unwrap(), ("http://[::1]:7777".into(), "::1".into(), 7777));
        assert!(split_url("http://").is_none());
        assert_eq!(scope_of(&"127.0.0.1".parse().unwrap()), "loopback");
        assert_eq!(scope_of(&"192.168.1.5".parse().unwrap()), "lan");
        assert_eq!(scope_of(&"10.0.2.2".parse().unwrap()), "emulator");
        assert_eq!(scope_of(&"8.8.8.8".parse().unwrap()), "public");
    }
}

#[cfg(test)]
mod live {
    /// Needs a relay on 127.0.0.1:7791: `cargo test -p aqua-client-ui servers::live -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn probes_a_running_relay() {
        let r = super::probe_servers(vec!["http://127.0.0.1:7791/".into(), "http://127.0.0.1:9".into()]).await.unwrap();
        eprintln!("{r:#?}");
        assert!(r.iter().any(|p| p.ok && p.scope.as_deref() == Some("loopback")));
        assert!(r.iter().any(|p| !p.ok));
    }
}
