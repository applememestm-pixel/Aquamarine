//! Nearby delivery over the local network — the same Wi-Fi or a phone's
//! hotspot, with or without internet. Works on every platform (plain TCP).
//!
//! * Each device listens on TCP 7790 (or the next free of 7791–7794).
//! * Finding each other: a UDP broadcast "AQMESH1 Q" on 7790, answered with
//!   "AQMESH1 A <tcp port>"; addresses that answered before; and, on a
//!   manual search, a quick knock on every address of this /24.
//! * The session on top is the same as over Bluetooth (mesh.rs): only
//!   contacts that prove their identity key get anything, and the side
//!   that was dialled says nothing about itself to a stranger.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use tauri::AppHandle;

use super::{book, run_session};

pub const PORT: u16 = 7790;
const MAGIC_Q: &str = "AQMESH1 Q";
const MAGIC_A: &str = "AQMESH1 A";

static READY: AtomicBool = AtomicBool::new(false);
static STOP: AtomicBool = AtomicBool::new(false);
static TCP_PORT: AtomicU16 = AtomicU16::new(0);
/// Who answered a beacon lately: address → when.
static HEARD: OnceLock<Mutex<HashMap<SocketAddr, Instant>>> = OnceLock::new();

fn heard() -> &'static Mutex<HashMap<SocketAddr, Instant>> {
    HEARD.get_or_init(Default::default)
}

pub fn ready() -> bool {
    READY.load(Ordering::Relaxed)
}

/// This device's IPv4 address on the local network (no packet is sent).
fn local_ipv4() -> Option<Ipv4Addr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    // Any routable address works; nothing is sent. A hotspot without
    // internet still has a default route through the phone.
    s.connect("10.255.255.255:9").or_else(|_| s.connect("1.1.1.1:80")).ok()?;
    match s.local_addr().ok()?.ip() {
        IpAddr::V4(v4) if !v4.is_loopback() && !v4.is_unspecified() => Some(v4),
        _ => None,
    }
}

/// Parse a beacon: `AQMESH1 A 7790` → (true, 7790); `AQMESH1 Q 7790` → (false, 7790).
pub(crate) fn parse_beacon(b: &[u8]) -> Option<(bool, u16)> {
    let s = std::str::from_utf8(b).ok()?.trim();
    let (head, port) = s.rsplit_once(' ')?;
    let port: u16 = port.parse().ok()?;
    if port == 0 {
        return None;
    }
    match head {
        MAGIC_A => Some((true, port)),
        MAGIC_Q => Some((false, port)),
        _ => None,
    }
}

fn tune(s: &TcpStream) {
    let _ = s.set_nodelay(true);
    let _ = s.set_read_timeout(Some(Duration::from_secs(25)));
    let _ = s.set_write_timeout(Some(Duration::from_secs(25)));
}

struct Tcp(TcpStream);
impl Read for Tcp {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> { self.0.read(b) }
}
impl Write for Tcp {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> { self.0.write(b) }
    fn flush(&mut self) -> std::io::Result<()> { self.0.flush() }
}

pub async fn listen(app: &AppHandle) -> Result<(), String> {
    if ready() {
        return Ok(());
    }
    STOP.store(false, Ordering::Relaxed);
    let listener = (PORT..PORT + 5)
        .find_map(|p| TcpListener::bind(("0.0.0.0", p)).ok())
        .ok_or("local network: ports 7790–7794 are taken")?;
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(PORT);
    TCP_PORT.store(port, Ordering::Relaxed);
    let _ = listener.set_nonblocking(true);

    // Incoming sessions.
    let app2 = app.clone();
    std::thread::spawn(move || {
        while !STOP.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((s, peer)) => {
                    let _ = s.set_nonblocking(false);
                    tune(&s);
                    let app3 = app2.clone();
                    std::thread::spawn(move || {
                        let mut link = Tcp(s);
                        run_session(&app3, &mut link, false, &format!("lan:{}", peer.ip()));
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(200)),
                Err(_) => std::thread::sleep(Duration::from_secs(1)),
            }
        }
    });

    // Beacons: answer "who's here?" and remember who answers ours.
    if let Ok(udp) = UdpSocket::bind(("0.0.0.0", PORT)) {
        let _ = udp.set_broadcast(true);
        let _ = udp.set_read_timeout(Some(Duration::from_secs(1)));
        std::thread::spawn(move || {
            let mut buf = [0u8; 64];
            let me = local_ipv4();
            while !STOP.load(Ordering::Relaxed) {
                let Ok((n, from)) = udp.recv_from(&mut buf) else { continue };
                if Some(from.ip()) == me.map(IpAddr::V4) {
                    continue;
                }
                match parse_beacon(&buf[..n]) {
                    Some((false, _)) => {
                        let _ = udp.send_to(format!("{MAGIC_A} {}", TCP_PORT.load(Ordering::Relaxed)).as_bytes(), from);
                    }
                    Some((true, tcp)) => {
                        if let Ok(mut h) = heard().lock() {
                            h.insert(SocketAddr::new(from.ip(), tcp), Instant::now());
                        }
                    }
                    None => {}
                }
            }
        });
    }
    READY.store(true, Ordering::Relaxed);
    tracing::info!(port, "mesh: listening on the local network");
    Ok(())
}

pub fn stop() {
    STOP.store(true, Ordering::Relaxed);
    READY.store(false, Ordering::Relaxed);
}

/// Ask the network who runs Aquamarine; answers land in `heard()`.
fn shout() {
    let Ok(s) = UdpSocket::bind("0.0.0.0:0") else { return };
    let _ = s.set_broadcast(true);
    let _ = s.set_read_timeout(Some(Duration::from_millis(300)));
    let msg = format!("{MAGIC_Q} {}", TCP_PORT.load(Ordering::Relaxed).max(PORT));
    let mut targets = vec![SocketAddr::from((Ipv4Addr::BROADCAST, PORT))];
    if let Some(ip) = local_ipv4() {
        let [a, b, c, _] = ip.octets();
        targets.push(SocketAddr::from((Ipv4Addr::new(a, b, c, 255), PORT)));
    }
    for t in &targets {
        let _ = s.send_to(msg.as_bytes(), t);
    }
    // Answers to a socket that isn't the listener's: read them here too.
    let until = Instant::now() + Duration::from_millis(1200);
    let mut buf = [0u8; 64];
    while Instant::now() < until {
        if let Ok((n, from)) = s.recv_from(&mut buf) {
            if let Some((true, tcp)) = parse_beacon(&buf[..n]) {
                if let Ok(mut h) = heard().lock() {
                    h.insert(SocketAddr::new(from.ip(), tcp), Instant::now());
                }
            }
        }
    }
}

/// Every address of this /24 that accepts a connection on our port.
async fn sweep() -> Vec<SocketAddr> {
    let Some(me) = local_ipv4() else { return vec![] };
    let [a, b, c, d0] = me.octets();
    let knocks = (1u8..=254).filter(|d| *d != d0).map(|d| async move {
        let addr = SocketAddr::from((Ipv4Addr::new(a, b, c, d), PORT));
        match tokio::time::timeout(Duration::from_millis(600), tokio::net::TcpStream::connect(addr)).await {
            Ok(Ok(_)) => Some(addr),
            _ => None,
        }
    });
    futures_util::future::join_all(knocks).await.into_iter().flatten().collect()
}

/// One round over the local network; returns how many contacts answered.
pub async fn sync_round(app: &AppHandle, scan: bool) -> Result<usize, String> {
    listen(app).await?;
    tauri::async_runtime::spawn_blocking(shout).await.map_err(|e| e.to_string())?;
    let me = local_ipv4().map(IpAddr::V4);
    let mut cands: Vec<SocketAddr> = heard()
        .lock()
        .map(|h| h.iter().filter(|(_, t)| t.elapsed() < Duration::from_secs(600)).map(|(a, _)| *a).collect())
        .unwrap_or_default();
    // Places contacts were met before.
    for k in book(app).peers.keys() {
        if let Some(a) = k.strip_prefix("lan:").and_then(|ip| ip.parse::<IpAddr>().ok()) {
            cands.push(SocketAddr::new(a, PORT));
        }
    }
    if scan {
        cands.extend(sweep().await);
    }
    cands.sort();
    cands.dedup();
    cands.retain(|a| Some(a.ip()) != me && !a.ip().is_loopback());
    let mut met = 0;
    for addr in cands.into_iter().take(24) {
        let Ok(Ok(s)) = tokio::time::timeout(Duration::from_secs(3), tokio::net::TcpStream::connect(addr)).await else { continue };
        let Ok(s) = s.into_std() else { continue };
        let _ = s.set_nonblocking(false);
        tune(&s);
        let app2 = app.clone();
        let rep = tauri::async_runtime::spawn_blocking(move || {
            let mut link = Tcp(s);
            run_session(&app2, &mut link, true, &format!("lan:{}", addr.ip()))
        })
        .await
        .map_err(|e| e.to_string())?;
        if rep.error.is_none() {
            met += 1;
        }
    }
    Ok(met)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beacons() {
        assert_eq!(parse_beacon(b"AQMESH1 Q 7790"), Some((false, 7790)));
        assert_eq!(parse_beacon(b"AQMESH1 A 7792\n"), Some((true, 7792)));
        assert_eq!(parse_beacon(b"AQMESH1 A 0"), None);
        assert_eq!(parse_beacon(b"HELLO 7790"), None);
        assert_eq!(parse_beacon(&[0xff, 0xfe]), None);
    }
}
