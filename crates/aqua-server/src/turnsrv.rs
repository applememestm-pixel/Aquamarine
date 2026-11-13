//! A TURN relay built into the server (1.20.1-6) — so calls connect.
//!
//! WebRTC finds a direct path between two phones with STUN, but behind the
//! NAT of a mobile operator (CGNAT) or a strict router there is none, and
//! the call hung on "connecting" forever: the apps had STUN only, a TURN
//! relay was a setting nobody filled in. Now every Aquamarine server is one:
//!
//! * UDP `AQUA_TURN_PORT` (3478 by default; open it on the router / the
//!   firewall like the server's own port). `AQUA_TURN=off` turns it off.
//! * Short-lived credentials (the standard "TURN REST" scheme, 12 hours):
//!   `GET /v1/ice` hands them out, the apps build `turn:<their server's
//!   host>:<port>` from the address they already use — no setting needed.
//! * Another TURN (coturn with `use-auth-secret`): `AQUA_TURN_URLS`
//!   ("turn:turn.example.org:3478,turns:…") + `AQUA_TURN_SECRET`; the
//!   built-in one then stays off.
//!
//! The relay address it gives out is the server's public IP once known
//! (public.rs), else its address in the local network.

use std::net::IpAddr;
use std::sync::Arc;
use std::sync::Mutex;

use axum::Json;

const SECRET_FILE: &str = "aqua-turn.secret";
const LIFE: std::time::Duration = std::time::Duration::from_secs(12 * 3600);

static STARTED: Mutex<Option<(u16, String)>> = Mutex::new(None);

fn off() -> bool {
    std::env::var("AQUA_TURN").map(|v| v == "off" || v == "0").unwrap_or(false)
}

fn external() -> Option<(Vec<String>, String)> {
    let urls: Vec<String> = std::env::var("AQUA_TURN_URLS").ok()?.split([',', ' ']).map(str::trim).filter(|u| !u.is_empty()).map(str::to_string).collect();
    let secret = std::env::var("AQUA_TURN_SECRET").ok().filter(|s| !s.trim().is_empty())?;
    (!urls.is_empty()).then_some((urls, secret))
}

/// This data folder's secret (made once).
fn secret() -> String {
    if let Ok(s) = std::fs::read_to_string(SECRET_FILE) {
        if s.trim().len() >= 32 {
            return s.trim().to_string();
        }
    }
    use rand_core::{OsRng, RngCore};
    let mut b = [0u8; 24];
    OsRng.fill_bytes(&mut b);
    let s = hex::encode(b);
    let _ = std::fs::write(SECRET_FILE, &s);
    s
}

fn relay_ip() -> Option<IpAddr> {
    let from_url = |u: String| -> Option<IpAddr> {
        let host = u.split("://").nth(1)?.split(['/', ':']).next()?.to_string();
        host.parse().ok()
    };
    from_url(crate::public::url()).or_else(|| crate::lan_ip().and_then(|ip| ip.parse().ok()))
}

/// Start the built-in TURN relay (once per process).
pub fn spawn() {
    if off() || external().is_some() {
        return;
    }
    if STARTED.lock().map(|g| g.is_some()).unwrap_or(true) {
        return;
    }
    let port: u16 = std::env::var("AQUA_TURN_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(3478);
    let sec = secret();
    if let Ok(mut g) = STARTED.lock() {
        *g = Some((0, sec.clone()));
    }
    tokio::spawn(async move {
        // The public address is looked up at start: give it a moment.
        for _ in 0..30 {
            if !crate::public::url().is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
        let Some(ip) = relay_ip() else {
            tracing::warn!("TURN: no address to give out — calls through this server's relay are off");
            return;
        };
        let conn = match tokio::net::UdpSocket::bind(("0.0.0.0", port)).await {
            Ok(c) => Arc::new(c),
            Err(e) => {
                tracing::warn!(%e, port, "TURN: the UDP port is taken — calls through this server's relay are off");
                return;
            }
        };
        let cfg = turn::server::config::ServerConfig {
            conn_configs: vec![turn::server::config::ConnConfig {
                conn,
                relay_addr_generator: Box::new(turn::relay::relay_static::RelayAddressGeneratorStatic {
                    relay_address: ip,
                    address: "0.0.0.0".to_owned(),
                    net: Arc::new(util::vnet::net::Net::new(None)),
                }),
            }],
            realm: "aquamarine".to_owned(),
            auth_handler: Arc::new(turn::auth::LongTermAuthHandler::new(sec.clone())),
            channel_bind_timeout: std::time::Duration::from_secs(0),
            alloc_close_notify: None,
        };
        match turn::server::Server::new(cfg).await {
            Ok(server) => {
                tracing::info!(%ip, port, "TURN relay for calls is on (UDP)");
                if let Ok(mut g) = STARTED.lock() {
                    *g = Some((port, sec));
                }
                // Lives as long as the process.
                std::mem::forget(server);
            }
            Err(e) => tracing::warn!(%e, "TURN relay didn't start"),
        }
    });
}

/// `GET /v1/ice` — what the apps add to their ICE servers. `port` + the
/// credentials for the built-in relay (the app puts its server's host in
/// front), or full `urls` for another TURN.
pub async fn ice() -> Json<serde_json::Value> {
    if let Some((urls, secret)) = external() {
        return match turn::auth::generate_long_term_credentials(&secret, LIFE) {
            Ok((u, p)) => Json(serde_json::json!({ "urls": urls, "username": u, "credential": p })),
            Err(_) => Json(serde_json::json!({})),
        };
    }
    let Some((port, secret)) = STARTED.lock().ok().and_then(|g| g.clone()).filter(|(p, _)| *p != 0) else {
        return Json(serde_json::json!({}));
    };
    match turn::auth::generate_long_term_credentials(&secret, LIFE) {
        Ok((u, p)) => Json(serde_json::json!({ "port": port, "username": u, "credential": p })),
        Err(_) => Json(serde_json::json!({})),
    }
}
