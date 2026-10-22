//! "Server on this device": the relay (aqua-server) running inside the app,
//! on a PC or a phone. Friends connect to it by this device's address; on
//! Android a foreground service (AquaRelay.kt) keeps the process alive with
//! the screen off. The relay's files live in `<app data>/relay/`.

use std::path::PathBuf;
use std::sync::Mutex as StdMutex;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tokio::sync::oneshot;

use crate::state::AppState;

struct Running {
    port: u16,
    admin_token: String,
    stop: Option<oneshot::Sender<()>>,
}

static RUNNING: StdMutex<Option<Running>> = StdMutex::new(None);
static LAST_ERROR: StdMutex<String> = StdMutex::new(String::new());

#[derive(Serialize)]
pub struct RelayHostStatus {
    running: bool,
    port: u16,
    /// http://<LAN ip>:<port> — for phones on the same Wi-Fi.
    lan_url: Option<String>,
    /// http://<white ip>:<port> once known (needs the port forwarded).
    public_url: Option<String>,
    /// The admin panel of this relay (opens on this device only).
    admin_url: Option<String>,
    dir: String,
    error: Option<String>,
    /// Android keeps it running with the screen off (a notification shows).
    background: bool,
}

fn relay_dir(state: &State<'_, StdMutex<AppState>>) -> Result<PathBuf, String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    Ok(s.data_dir.join("relay"))
}

fn status_now(dir: String) -> RelayHostStatus {
    let r = RUNNING.lock().ok();
    let cur = r.as_ref().and_then(|g| g.as_ref().map(|x| (x.port, x.admin_token.clone())));
    let err = LAST_ERROR.lock().map(|e| e.clone()).unwrap_or_default();
    match cur {
        Some((port, tok)) => RelayHostStatus {
            running: true,
            port,
            lan_url: aqua_server::lan_ip().map(|ip| format!("http://{ip}:{port}")),
            public_url: Some(aqua_server::public::url()).filter(|u| !u.is_empty()),
            admin_url: Some(format!("http://127.0.0.1:{port}/admin#token={tok}")),
            dir,
            error: None,
            background: cfg!(target_os = "android"),
        },
        None => RelayHostStatus {
            running: false,
            port: 0,
            lan_url: None,
            public_url: None,
            admin_url: None,
            dir,
            error: Some(err).filter(|e| !e.is_empty()),
            background: cfg!(target_os = "android"),
        },
    }
}

#[tauri::command]
pub fn relay_host_status(state: State<'_, StdMutex<AppState>>) -> Result<RelayHostStatus, String> {
    let dir = relay_dir(&state)?;
    Ok(status_now(dir.display().to_string()))
}

#[tauri::command]
pub async fn relay_host_start(
    app: AppHandle,
    state: State<'_, StdMutex<AppState>>,
    port: Option<u16>,
) -> Result<RelayHostStatus, String> {
    let dir = relay_dir(&state)?;
    if RUNNING.lock().map(|r| r.is_some()).unwrap_or(false) {
        return Ok(status_now(dir.display().to_string()));
    }
    let port = port.filter(|p| *p >= 1024).unwrap_or(7777);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    // The relay keeps its database, blobs and settings next to itself.
    std::env::set_current_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            let msg = if e.kind() == std::io::ErrorKind::AddrInUse {
                format!("port {port} is taken — is another relay (aqua-server) already running on this device?")
            } else {
                format!("port {port}: {e}")
            };
            if let Ok(mut le) = LAST_ERROR.lock() { *le = msg.clone(); }
            return Err(msg);
        }
    };
    let token = std::env::var("AQUA_ADMIN_TOKEN").ok().filter(|t| !t.is_empty()).unwrap_or_else(|| {
        use rand::RngCore;
        let mut b = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut b);
        let t = hex::encode(b);
        std::env::set_var("AQUA_ADMIN_TOKEN", &t);
        t
    });
    std::env::set_var("AQUA_LISTEN", addr.to_string());
    let (tx, rx) = oneshot::channel::<()>();
    if let Ok(mut r) = RUNNING.lock() {
        *r = Some(Running { port, admin_token: token, stop: Some(tx) });
    }
    if let Ok(mut le) = LAST_ERROR.lock() { le.clear(); }
    tauri::async_runtime::spawn(async move {
        let res = aqua_server::serve_embedded(listener, async { let _ = rx.await; }).await;
        if let Err(e) = res {
            tracing::warn!(?e, "relay host stopped with an error");
            if let Ok(mut le) = LAST_ERROR.lock() { *le = e.to_string(); }
        }
        if let Ok(mut r) = RUNNING.lock() { *r = None; }
    });
    tracing::info!(port, "relay host: serving on this device");
    #[cfg(target_os = "android")]
    android::keep_alive(true, port);
    let _ = &app;
    Ok(status_now(dir.display().to_string()))
}

#[tauri::command]
pub async fn relay_host_stop(state: State<'_, StdMutex<AppState>>) -> Result<RelayHostStatus, String> {
    let dir = relay_dir(&state)?;
    let stop = RUNNING.lock().ok().and_then(|mut r| r.as_mut().and_then(|x| x.stop.take()));
    if let Some(tx) = stop {
        let _ = tx.send(());
    }
    // The server task clears RUNNING when it has wound down.
    for _ in 0..30 {
        if RUNNING.lock().map(|r| r.is_none()).unwrap_or(true) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    if let Ok(mut r) = RUNNING.lock() { *r = None; }
    #[cfg(target_os = "android")]
    android::keep_alive(false, 0);
    Ok(status_now(dir.display().to_string()))
}

/// Restart it on launch if it was on (Settings remembers the choice; the UI
/// calls `relay_host_start` then). Kept for symmetry with other features.
pub fn data_dir_of(app: &AppHandle) -> Option<PathBuf> {
    let state: State<'_, StdMutex<AppState>> = app.state();
    state.lock().ok().map(|s| s.data_dir.join("relay"))
}

#[cfg(target_os = "android")]
pub mod android {
    //! The foreground service lives in Kotlin (AquaRelay.kt).
    use std::sync::OnceLock;
    use tauri::plugin::{Builder, PluginHandle, TauriPlugin};
    use tauri::Wry;

    static HANDLE: OnceLock<PluginHandle<Wry>> = OnceLock::new();

    pub fn plugin() -> TauriPlugin<Wry> {
        Builder::new("aquarelay")
            .setup(|_app, api| {
                match api.register_android_plugin("app.aquamarine.messenger", "AquaRelayPlugin") {
                    Ok(h) => { let _ = HANDLE.set(h); }
                    Err(e) => tracing::warn!(?e, "relay host: Android plugin missing"),
                }
                Ok(())
            })
            .build()
    }

    pub fn keep_alive(on: bool, port: u16) {
        let Some(h) = HANDLE.get() else { return };
        let cmd = if on { "start" } else { "stop" };
        if let Err(e) = h.run_mobile_plugin::<serde_json::Value>(cmd, serde_json::json!({ "port": port })) {
            tracing::warn!(%e, "relay host: foreground service {cmd}");
        }
    }
}
