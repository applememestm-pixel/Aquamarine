//! "Server on this device": the relay (aqua-server) running inside the app,
//! on a PC or a phone. Friends connect to it by this device's address; on
//! Android a foreground service (AquaRelay.kt) keeps the process alive with
//! the screen off. The relay's files live in `<app data>/relay/` unless a
//! folder was chosen (Settings → Server on this device → Data folder; asked
//! on the first start), remembered in `<app data>/relay-dir.txt`.

use std::path::PathBuf;
use std::sync::Mutex as StdMutex;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tokio::sync::oneshot;


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
    /// The folder was chosen (or data is already there): no need to ask.
    dir_chosen: bool,
    error: Option<String>,
    /// Android keeps it running with the screen off (a notification shows).
    background: bool,
}

/// The server belongs to the device, not to whichever account is open:
/// it lives in the app's base data directory (accounts.rs).
const DIR_FILE: &str = "relay-dir.txt";

fn relay_dir(base: &State<'_, crate::accounts::Base>) -> Result<PathBuf, String> {
    Ok(chosen_dir(&base.0).unwrap_or_else(|| base.0.join("relay")))
}

fn chosen_dir(base: &std::path::Path) -> Option<PathBuf> {
    let s = std::fs::read_to_string(base.join(DIR_FILE)).ok()?;
    let p = PathBuf::from(s.trim());
    (!s.trim().is_empty() && p.is_absolute()).then_some(p)
}

fn dir_chosen(base: &std::path::Path) -> bool {
    chosen_dir(base).is_some() || aqua_server::datadir::has_data(&base.join("relay"))
}

/// Pick the relay's data folder. Not while it runs. `copy`: bring the
/// current data along (the old folder is left as it is).
#[tauri::command]
pub fn relay_host_set_dir(base: State<'_, crate::accounts::Base>, path: String, copy: bool) -> Result<RelayHostStatus, String> {
    if RUNNING.lock().map(|r| r.is_some()).unwrap_or(false) {
        return Err("stop the server first".into());
    }
    let to = if path.trim().is_empty() { base.0.join("relay") } else { PathBuf::from(path.trim()) };
    if !to.is_absolute() { return Err("choose a folder".into()); }
    let from = relay_dir(&base)?;
    std::fs::create_dir_all(&to).map_err(|e| format!("{}: {e}", to.display()))?;
    let canon = |p: &std::path::Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    if copy && canon(&from) != canon(&to) && aqua_server::datadir::has_data(&from) {
        if canon(&to).starts_with(canon(&from)) { return Err("the new folder can't be inside the current one".into()); }
        copy_tree(&from, &to).map_err(|e| e.to_string())?;
    }
    std::fs::write(base.0.join(DIR_FILE), to.display().to_string()).map_err(|e| e.to_string())?;
    let mut st = status_now(to.display().to_string());
    st.dir_chosen = true;
    Ok(st)
}

fn copy_tree(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)?.flatten() {
        let (src, dst) = (e.path(), to.join(e.file_name()));
        let md = e.metadata()?;
        if md.is_dir() { copy_tree(&src, &dst)?; } else if md.is_file() { std::fs::copy(&src, &dst)?; }
    }
    Ok(())
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
            dir_chosen: true,
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
            dir_chosen: true,
            error: Some(err).filter(|e| !e.is_empty()),
            background: cfg!(target_os = "android"),
        },
    }
}

#[tauri::command]
pub fn relay_host_status(base: State<'_, crate::accounts::Base>) -> Result<RelayHostStatus, String> {
    let dir = relay_dir(&base)?;
    let mut st = status_now(dir.display().to_string());
    st.dir_chosen = dir_chosen(&base.0);
    Ok(st)
}

#[tauri::command]
pub async fn relay_host_start(
    app: AppHandle,
    base: State<'_, crate::accounts::Base>,
    port: Option<u16>,
) -> Result<RelayHostStatus, String> {
    let dir = relay_dir(&base)?;
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
pub async fn relay_host_stop(base: State<'_, crate::accounts::Base>) -> Result<RelayHostStatus, String> {
    let dir = relay_dir(&base)?;
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
    app.try_state::<crate::accounts::Base>().map(|b| b.0.join("relay"))
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

    /// Start the app over in a fresh process (another account was picked).
    pub fn relaunch() {
        let Some(h) = HANDLE.get() else { return };
        if let Err(e) = h.run_mobile_plugin::<serde_json::Value>("relaunch", serde_json::json!({})) {
            tracing::warn!(%e, "relaunch");
        }
    }
}
