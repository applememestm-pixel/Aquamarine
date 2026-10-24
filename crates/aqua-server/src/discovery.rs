//! Finding other relays — for the admin panel's "Servers" list, the same
//! ways the app finds them: the local network (a TCP knock on every address
//! of this /24, then `/v1/stats` on the ones that answer), the project's
//! public list (site/servers.json), and — in fed.rs — whom the peers know.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::{Duration, Instant};

use futures_util::future::join_all;

const PUBLIC_LISTS: &[&str] = &[
    "https://aquamarinemessenger.pw/servers.json",
    "https://applememestm-pixel.github.io/Aquamarine/servers.json",
];

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(4))
        .build()
        .unwrap_or_default()
}

/// Relays answering on `port` in this machine's /24 (not this machine).
pub async fn lan_relays(port: u16) -> Vec<String> {
    let Some(IpAddr::V4(me)) = crate::lan_ip().and_then(|s| s.parse().ok()) else { return Vec::new() };
    let [a, b, c, own] = me.octets();
    let knocks = (1u8..=254).filter(|d| *d != own).map(|d| async move {
        let addr = SocketAddr::from((Ipv4Addr::new(a, b, c, d), port));
        tokio::time::timeout(Duration::from_millis(400), tokio::net::TcpStream::connect(addr))
            .await
            .ok()
            .and_then(|r| r.ok())
            .map(|_| format!("http://{a}.{b}.{c}.{d}:{port}"))
    });
    let open: Vec<String> = join_all(knocks).await.into_iter().flatten().collect();
    let http = http();
    let checks = open.into_iter().map(|u| {
        let http = http.clone();
        async move {
            let ok = http.get(format!("{u}/v1/stats")).send().await.ok()?.json::<serde_json::Value>().await.ok()?["ok"].as_bool()?;
            ok.then_some(u)
        }
    });
    join_all(checks).await.into_iter().flatten().collect()
}

/// Addresses from the project's public list.
pub async fn public_list() -> Vec<String> {
    let http = http();
    for u in PUBLIC_LISTS {
        let Ok(r) = http.get(*u).send().await else { continue };
        let Ok(v) = r.json::<serde_json::Value>().await else { continue };
        let Some(list) = v["servers"].as_array() else { continue };
        return list
            .iter()
            .filter_map(|x| x.as_str().or_else(|| x["url"].as_str()).map(|s| s.trim().trim_end_matches('/').to_string()))
            .filter(|s| s.starts_with("http"))
            .collect();
    }
    Vec::new()
}

/// (answers, round trip ms, accounts, online now) of a relay.
pub async fn probe(url: &str) -> (bool, Option<u64>, Option<u64>, Option<u64>) {
    let t0 = Instant::now();
    let Ok(r) = http().get(format!("{}/v1/stats", url.trim_end_matches('/'))).send().await else { return (false, None, None, None) };
    let ms = t0.elapsed().as_millis() as u64;
    let Ok(v) = r.json::<serde_json::Value>().await else { return (false, None, None, None) };
    let ok = v["ok"].as_bool().unwrap_or(false);
    (ok, ok.then_some(ms), v["users"].as_u64(), v["online"].as_u64())
}
