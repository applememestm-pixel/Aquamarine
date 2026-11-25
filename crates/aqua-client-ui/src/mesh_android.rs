//! Nearby delivery on Android: the RFCOMM sockets live in Kotlin
//! (`android/AquaMesh.kt`, copied into the generated project), this side
//! drives them through a small Tauri mobile plugin. Insecure RFCOMM, so
//! devices don't need to be paired — contacts prove themselves with their
//! identity keys inside the session.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};
use tauri::plugin::{Builder, PluginHandle, TauriPlugin};
use tauri::{AppHandle, Wry};

use super::{book, run_session, SERVICE_UUID};

pub const SUPPORTED: bool = true;

static READY: AtomicBool = AtomicBool::new(false);
static LISTEN_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static ROUND_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static HANDLE: OnceLock<PluginHandle<Wry>> = OnceLock::new();

pub fn ready() -> bool {
    READY.load(Ordering::Relaxed)
}

/// Registered in `lib.rs`; hands us the Kotlin side.
pub fn plugin() -> TauriPlugin<Wry> {
    Builder::new("aquamesh")
        .setup(|_app, api| {
            match api.register_android_plugin("app.aquamarine.messenger", "AquaMeshPlugin") {
                Ok(h) => {
                    let _ = HANDLE.set(h);
                }
                Err(e) => tracing::warn!(?e, "mesh: Android plugin missing"),
            }
            Ok(())
        })
        .build()
}

fn h() -> Result<&'static PluginHandle<Wry>, String> {
    HANDLE.get().ok_or_else(|| "Bluetooth plugin not loaded".to_string())
}

fn call<T: serde::de::DeserializeOwned>(cmd: &str, args: impl Serialize) -> Result<T, String> {
    h()?.run_mobile_plugin::<T>(cmd, args).map_err(|e| e.to_string())
}

async fn call_async<T: serde::de::DeserializeOwned + Send + 'static>(cmd: &'static str, args: serde_json::Value) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(move || call::<T>(cmd, args)).await.map_err(|e| e.to_string())?
}

#[derive(Deserialize)]
struct State {
    supported: bool,
    enabled: bool,
    permitted: bool,
}

#[derive(Deserialize)]
struct Conn {
    id: Option<i64>,
    address: Option<String>,
}

#[derive(Deserialize)]
struct Chunk {
    data: Option<String>,
    eof: Option<bool>,
}

#[derive(Deserialize)]
struct Dev {
    address: String,
    #[serde(default)]
    bonded: bool,
    #[serde(default)]
    discovered: bool,
}

#[derive(Deserialize)]
struct Devs {
    devices: Vec<Dev>,
}

#[derive(Deserialize)]
struct Empty {}

/// The socket as a blocking byte stream: reads and writes go through the plugin.
struct Link {
    id: i64,
    buf: VecDeque<u8>,
}

impl Read for Link {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.buf.is_empty() {
            let c: Chunk = call("read", serde_json::json!({ "id": self.id, "max": 65536, "timeoutMs": 25000 }))
                .map_err(std::io::Error::other)?;
            if c.eof.unwrap_or(false) {
                return Ok(0);
            }
            let bytes = B64.decode(c.data.unwrap_or_default()).map_err(std::io::Error::other)?;
            if bytes.is_empty() {
                return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "timed out"));
            }
            self.buf.extend(bytes);
        }
        let n = out.len().min(self.buf.len());
        for (i, b) in self.buf.drain(..n).enumerate() {
            out[i] = b;
        }
        Ok(n)
    }
}

impl Write for Link {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        let _: Empty = call("write", serde_json::json!({ "id": self.id, "data": B64.encode(data) })).map_err(std::io::Error::other)?;
        Ok(data.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        let _ = call::<Empty>("close", serde_json::json!({ "id": self.id }));
    }
}

/// Bluetooth on and allowed? `ask`: show the system permission prompt
/// (only when the user asked for a sync, never at app start).
async fn ensure(ask: bool) -> Result<(), String> {
    let mut st: State = call_async("state", serde_json::json!({})).await?;
    if !st.supported { return Err("Bluetooth is unavailable".into()); }
    if !st.permitted && ask {
        st = call_async("ask", serde_json::json!({})).await?;
    }
    if !st.permitted {
        return Err("allow \"Nearby devices\" for Aquamarine to use Bluetooth".into());
    }
    if !st.enabled {
        return Err("Bluetooth is off".into());
    }
    Ok(())
}

pub async fn listen(app: &AppHandle) -> Result<(), String> {
    let _guard = LISTEN_LOCK.lock().await;
    if ready() {
        return Ok(());
    }
    ensure(false).await?;
    let _: Empty = call_async("listen", serde_json::json!({ "uuid": SERVICE_UUID, "name": "Aquamarine" })).await?;
    READY.store(true, Ordering::Relaxed);
    // Accept loop: each incoming connection runs a session on its own thread.
    let app2 = app.clone();
    std::thread::spawn(move || {
        while READY.load(Ordering::Relaxed) {
            match call::<Conn>("accept", serde_json::json!({ "timeoutMs": 30000 })) {
                Ok(Conn { id: Some(id), address }) => {
                    let app3 = app2.clone();
                    std::thread::spawn(move || {
                        let mut link = Link { id, buf: VecDeque::new() };
                        run_session(&app3, &mut link, false, &address.unwrap_or_default());
                    });
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!(%e, "mesh: accept");
                    std::thread::sleep(std::time::Duration::from_secs(5));
                }
            }
        }
    });
    tracing::info!("mesh: listening for nearby contacts");
    Ok(())
}

/// Hold (or let go of) Android's Wi-Fi multicast lock while listening on
/// the local network (AquaMesh.kt `lanLock`): without it broadcasts are
/// dropped on many phones.
pub fn lan_lock(on: bool) {
    let _ = call::<Empty>("lanLock", serde_json::json!({ "on": on }));
    if on { refresh_wifi(); }
}

/// The Wi-Fi network (AquaMesh.kt `wifiHandle`), 0 when there's none.
static WIFI: std::sync::Mutex<(u64, Option<std::net::Ipv4Addr>)> = std::sync::Mutex::new((0, None));

/// Ask Android which network is Wi-Fi (it changes when Wi-Fi does).
pub fn refresh_wifi() {
    #[derive(Deserialize)]
    struct Wifi { handle: u64, ipv4: Option<std::net::Ipv4Addr> }
    let wifi = call::<Wifi>("wifiHandle", serde_json::json!({}))
        .map(|wifi| (wifi.handle, wifi.ipv4)).unwrap_or_default();
    *WIFI.lock().unwrap_or_else(|error| error.into_inner()) = wifi;
}

pub fn wifi_ipv4() -> Option<std::net::Ipv4Addr> {
    WIFI.lock().unwrap_or_else(|error| error.into_inner()).1
}

#[link(name = "android")]
extern "C" {
    /// NDK (API 23): this socket's traffic goes over `network`.
    fn android_setsocknetwork(network: u64, fd: std::os::raw::c_int) -> std::os::raw::c_int;
}

/// A socket of the local network goes over Wi-Fi even when the default
/// network is mobile data (Wi-Fi without internet): before, broadcasts and
/// connections to neighbours left through mobile data and found nobody.
pub fn on_wifi(fd: std::os::raw::c_int) {
    let h = WIFI.lock().unwrap_or_else(|error| error.into_inner()).0;
    if h != 0 {
        // SAFETY: plain NDK call on a socket we own; a failure leaves it as it was.
        unsafe { android_setsocknetwork(h, fd); }
    }
}

pub async fn stop(_app: &AppHandle) {
    let _guard = LISTEN_LOCK.lock().await;
    READY.store(false, Ordering::Relaxed);
    let _ = call_async::<Empty>("stop", serde_json::json!({})).await;
}

pub async fn prepare(app: &AppHandle) -> Result<(), String> {
    ensure(true).await?;
    listen(app).await?;
    let _: Empty = call_async("visible", serde_json::json!({})).await?;
    Ok(())
}

pub async fn sync_round(app: &AppHandle, scan: bool) -> Result<(), String> {
    let _guard = ROUND_LOCK.lock().await;
    ensure(scan).await?;
    listen(app).await?;
    let known: std::collections::HashMap<String, u64> = book(app)
        .peers.iter().map(|(address, peer)| (address.to_uppercase(), peer.last)).collect();
    let direct = super::direct_addrs(app);
    let mut attempted = std::collections::HashSet::new();
    let mut met = 0;
    let mut last_error = None;

    // Try saved contact addresses before a full scan, then prefer devices seen in it.
    for duration in [0, 18] {
        if duration > 0 && (!scan || met > 0) { break; }
        let mut candidates = Vec::new();
        if duration == 0 {
            candidates.extend(direct.iter().map(|address| (u64::MAX, address.to_uppercase())));
            candidates.extend(known.iter().filter(|(address, _)| super::norm_mac(address).is_some())
                .map(|(address, seen)| (*seen, address.clone())));
        } else {
            let devs: Devs = call_async("devices", serde_json::json!({ "scanSecs": duration })).await?;
            candidates.extend(devs.devices.into_iter().map(|device| {
                let rank = match (device.discovered, device.bonded) {
                    (true, true) => 3,
                    (true, false) => 2,
                    (false, true) => 1,
                    (false, false) => 0,
                };
                (rank, device.address.to_uppercase())
            }));
        }
        candidates.sort_unstable_by(|a, b| b.cmp(a));
        for (_, address) in candidates.into_iter().filter(|(_, address)| attempted.insert(address.clone())).take(10) {
            let conn: Result<Conn, String> =
                call_async("connect", serde_json::json!({ "address": address, "uuid": SERVICE_UUID })).await;
            let id = match conn {
                Ok(Conn { id: Some(id), .. }) => id,
                Ok(_) => continue,
                Err(error) => {
                    if known.contains_key(&address) || direct.iter().any(|candidate| candidate.eq_ignore_ascii_case(&address)) {
                        super::note_attempt(app, &address, &error);
                        last_error = Some(error);
                    }
                    continue;
                }
            };
            let app = app.clone();
            let report = tauri::async_runtime::spawn_blocking(move || {
                let mut link = Link { id, buf: VecDeque::new() };
                run_session(&app, &mut link, true, &address)
            }).await.map_err(|e| e.to_string())?;
            if let Some(error) = report.error {
                last_error = Some(error);
            } else {
                met += 1;
            }
        }
    }
    if met == 0 && scan {
        return Err(last_error.unwrap_or_else(|| {
            if attempted.is_empty() { "no Bluetooth devices nearby".into() }
            else { "no contacts with Aquamarine answered nearby".into() }
        }));
    }
    Ok(())
}

/// Android doesn't tell apps their own Bluetooth address: the other side
/// learns it from the first connection.
pub fn own_address() -> Option<String> {
    None
}
