//! The relay's address on the internet ("white IP").
//!
//! A relay at home is reached as `http://192.168.1.x:7777` on the home
//! network; outside it the same relay answers on the router's public
//! address once the port is forwarded. The relay looks that address up
//! (or takes `AQUA_PUBLIC_URL`) and hands it to clients in
//! `GET /v1/fed/peers` as `self`, so a phone that signed in at home
//! switches to it by itself when it leaves the Wi-Fi.
//!
//! `AQUA_PUBLIC_URL=off` turns the lookup off.

use std::net::Ipv4Addr;
use std::sync::RwLock;
use std::time::Duration;

static URL: RwLock<String> = RwLock::new(String::new());

/// `http://<public ip>:<port>`, or "" while unknown / off.
pub fn url() -> String {
    URL.read().map(|u| u.clone()).unwrap_or_default()
}

fn set(u: String) {
    if let Ok(mut g) = URL.write() {
        *g = u;
    }
}

fn is_public(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_documentation()
        || o[0] == 100 && (64..128).contains(&o[1]) // carrier-grade NAT
        || o[0] >= 224)
}

async fn lookup(http: &reqwest::Client) -> Option<Ipv4Addr> {
    for src in ["https://api.ipify.org", "https://ipv4.icanhazip.com", "https://ifconfig.me/ip"] {
        let Ok(r) = http.get(src).send().await else { continue };
        let Ok(t) = r.text().await else { continue };
        if let Ok(ip) = t.trim().parse::<Ipv4Addr>() {
            return Some(ip);
        }
    }
    None
}

fn show(u: &str, port: u16, ansi: bool) {
    let (dim, bold, end) = if ansi { ("\x1b[38;2;140;150;165m", "\x1b[1m", "\x1b[0m") } else { ("", "", "") };
    println!("  {dim}Internet    {end}{bold}{u}{end}  {dim}(once TCP {port} is forwarded to this PC on the router){end}");
    tracing::info!(url = %u, "public address");
}

/// Look the address up now and every 30 minutes (home IPs change).
/// `listening_everywhere`: bound to 0.0.0.0 — otherwise nobody outside this
/// PC can connect, and there's nothing to announce.
pub fn spawn(port: u16, listening_everywhere: bool, ansi: bool) {
    match std::env::var("AQUA_PUBLIC_URL").map(|v| v.trim().trim_end_matches('/').to_string()) {
        Ok(v) if v == "off" || v == "0" => return,
        Ok(v) if v.starts_with("http://") || v.starts_with("https://") => {
            show(&v, port, ansi);
            set(v);
            return;
        }
        _ => {}
    }
    if !listening_everywhere {
        return;
    }
    tokio::spawn(async move {
        let Ok(http) = reqwest::Client::builder().timeout(Duration::from_secs(8)).build() else { return };
        let mut shown = String::new();
        loop {
            match lookup(&http).await {
                Some(ip) if is_public(ip) => {
                    let u = format!("http://{ip}:{port}");
                    if u != shown {
                        show(&u, port, ansi);
                        shown = u.clone();
                    }
                    set(u);
                }
                _ => {}
            }
            tokio::time::sleep(Duration::from_secs(30 * 60)).await;
        }
    });
}
