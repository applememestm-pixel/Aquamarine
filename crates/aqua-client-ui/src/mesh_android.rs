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

pub async fn stop(_app: &AppHandle) {
    READY.store(false, Ordering::Relaxed);
    let _ = call_async::<Empty>("stop", serde_json::json!({})).await;
}

pub async fn sync_round(app: &AppHandle, scan: bool) -> Result<(), String> {
    ensure(scan).await?;
    listen(app).await?;
    let devs: Devs = call_async("devices", serde_json::json!({ "scanSecs": if scan { 8 } else { 0 } })).await?;
    let known: std::collections::HashMap<String, u64> = book(app).peers.iter().map(|(k, p)| (k.clone(), p.last)).collect();
    let mut cands: Vec<(u64, String)> = devs
        .devices
        .into_iter()
        .filter(|d| d.bonded || known.contains_key(&d.address) || scan)
        .map(|d| (known.get(&d.address).copied().unwrap_or(0), d.address))
        .collect();
    cands.sort_by(|a, b| b.0.cmp(&a.0));
    if cands.is_empty() {
        return Err("no Bluetooth devices nearby".into());
    }
    let mut met = 0;
    for (_, address) in cands.into_iter().take(10) {
        let conn: Result<Conn, String> =
            call_async("connect", serde_json::json!({ "address": address, "uuid": SERVICE_UUID })).await;
        let Ok(Conn { id: Some(id), .. }) = conn else { continue };
        let app2 = app.clone();
        let addr2 = address.clone();
        let rep = tauri::async_runtime::spawn_blocking(move || {
            let mut link = Link { id, buf: VecDeque::new() };
            run_session(&app2, &mut link, true, &addr2)
        })
        .await
        .map_err(|e| e.to_string())?;
        if rep.error.is_none() {
            met += 1;
        }
    }
    if met == 0 && scan {
        return Err("no contacts with Aquamarine answered nearby".into());
    }
    Ok(())
}
