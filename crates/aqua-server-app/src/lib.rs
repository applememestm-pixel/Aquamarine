//! Aquamarine Server — the relay (aqua-server) as an app of its own
//! (1.2.0-5). Until then it lived inside the messenger ("Server on this
//! device"); now the messenger is only a messenger, and whoever hosts a
//! server installs this.
//!
//! * Opening the app starts the server (it remembers if it was stopped).
//! * The window shows how to reach it and the relay's own admin panel.
//! * Computers: closing the window keeps it running in the tray, and it can
//!   start with the system. Android: a foreground service keeps it alive
//!   with the screen off (AquaServer.kt).
//! * Data: the same folder the console server uses (`server-data.txt`), so
//!   switching to the app keeps every account. On a first start it also
//!   takes over the folder of the old in-app server of the messenger.

mod updates;

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use tokio::sync::oneshot;

const CONF: &str = "server-app.json";
const DEFAULT_PORT: u16 = 7777;

#[derive(Clone, Serialize, Deserialize)]
struct Conf {
    #[serde(default)]
    port: u16,
    /// Data folder; "" = the default for this device.
    #[serde(default)]
    dir: String,
    /// Run the server when the app opens (off once the user stopped it).
    #[serde(default = "yes")]
    run: bool,
    /// Computers: closing the window keeps the server in the tray.
    #[serde(default = "yes")]
    tray: bool,
    /// Install new versions of this app by itself (updates.rs).
    #[serde(default = "yes")]
    auto_update: bool,
}

fn yes() -> bool {
    true
}

impl Default for Conf {
    fn default() -> Self {
        Self { port: DEFAULT_PORT, dir: String::new(), run: true, tray: true, auto_update: true }
    }
}

struct Running {
    port: u16,
    token: String,
    stop: Option<oneshot::Sender<()>>,
}

static RUNNING: Mutex<Option<Running>> = Mutex::new(None);
static LAST_ERROR: Mutex<String> = Mutex::new(String::new());
static TRAY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

fn conf_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join(CONF))
}

fn load(app: &AppHandle) -> Conf {
    conf_path(app)
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save(app: &AppHandle, c: &Conf) {
    if let Some(p) = conf_path(app) {
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(p, serde_json::to_vec_pretty(c).unwrap_or_default());
    }
}

/// The messenger's old in-app server kept its data here (relay_host.rs of
/// 1.2.0-4 and earlier): `<data>/app.aquamarine.desktop/relay`, or the
/// folder named in `relay-dir.txt` there.
fn old_in_app_dir() -> Option<PathBuf> {
    let base = dirs::data_dir()?.join("app.aquamarine.desktop");
    let chosen = std::fs::read_to_string(base.join("relay-dir.txt")).ok().map(|s| PathBuf::from(s.trim())).filter(|p| p.is_absolute());
    let d = chosen.unwrap_or_else(|| base.join("relay"));
    aqua_server::datadir::has_data(&d).then_some(d)
}

/// Where the server keeps its data on this device.
fn data_dir(app: &AppHandle, c: &Conf) -> PathBuf {
    if !c.dir.trim().is_empty() {
        return PathBuf::from(c.dir.trim());
    }
    if cfg!(any(target_os = "android", target_os = "ios")) {
        return app.path().app_data_dir().map(|d| d.join("relay")).unwrap_or_else(|_| PathBuf::from("relay"));
    }
    if let Some(d) = aqua_server::datadir::remembered() {
        return d;
    }
    if let Some(d) = old_in_app_dir() {
        let _ = aqua_server::datadir::remember(&d);
        return d;
    }
    aqua_server::datadir::default_dir()
}

#[derive(Serialize)]
pub struct Status {
    running: bool,
    port: u16,
    /// This device (and the admin panel's address).
    local_url: Option<String>,
    /// Phones and computers on the same Wi-Fi.
    lan_url: Option<String>,
    /// From the internet, once the relay knows its address (port forwarded).
    public_url: Option<String>,
    dir: String,
    has_data: bool,
    error: Option<String>,
    mobile: bool,
    tray: bool,
    autostart: Option<bool>,
}

fn status_of(app: &AppHandle) -> Status {
    let c = load(app);
    let dir = data_dir(app, &c);
    let cur = RUNNING.lock().ok().and_then(|g| g.as_ref().map(|r| (r.port, r.token.clone())));
    let err = LAST_ERROR.lock().map(|e| e.clone()).unwrap_or_default();
    #[cfg(desktop)]
    let autostart = {
        use tauri_plugin_autostart::ManagerExt;
        app.autolaunch().is_enabled().ok()
    };
    #[cfg(mobile)]
    let autostart = None;
    let (running, port) = match &cur {
        Some((p, _)) => (true, *p),
        None => (false, if c.port >= 1024 { c.port } else { DEFAULT_PORT }),
    };
    Status {
        running,
        port,
        local_url: running.then(|| format!("http://127.0.0.1:{port}")),
        lan_url: if running { aqua_server::lan_ip().map(|ip| format!("http://{ip}:{port}")) } else { None },
        public_url: if running { Some(aqua_server::public::url()).filter(|u| !u.is_empty()) } else { None },
        has_data: aqua_server::datadir::has_data(&dir),
        dir: dir.display().to_string(),
        error: (!running && !err.is_empty()).then_some(err),
        mobile: cfg!(mobile),
        tray: c.tray,
        autostart,
    }
}

async fn start_inner(app: &AppHandle, port: Option<u16>) -> Result<Status, String> {
    if RUNNING.lock().map(|r| r.is_some()).unwrap_or(false) {
        return Ok(status_of(app));
    }
    let mut c = load(app);
    let port = port.or(Some(c.port)).filter(|p| *p >= 1024).unwrap_or(DEFAULT_PORT);
    let dir = data_dir(app, &c);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    // Every file of the relay is a path relative to its data folder.
    std::env::set_current_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    #[cfg(desktop)]
    let _ = aqua_server::datadir::remember(&dir);
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            let msg = if e.kind() == std::io::ErrorKind::AddrInUse {
                format!("port {port} is taken — is another Aquamarine server (the console one?) already running?")
            } else {
                format!("port {port}: {e}")
            };
            if let Ok(mut le) = LAST_ERROR.lock() {
                *le = msg.clone();
            }
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
        *r = Some(Running { port, token, stop: Some(tx) });
    }
    if let Ok(mut le) = LAST_ERROR.lock() {
        le.clear();
    }
    tauri::async_runtime::spawn(async move {
        if let Err(e) = aqua_server::serve_embedded(listener, async {
            let _ = rx.await;
        })
        .await
        {
            tracing::warn!(?e, "the server stopped with an error");
            if let Ok(mut le) = LAST_ERROR.lock() {
                *le = e.to_string();
            }
        }
        if let Ok(mut r) = RUNNING.lock() {
            *r = None;
        }
    });
    #[cfg(target_os = "android")]
    android::keep_alive(true, port);
    c.port = port;
    c.run = true;
    save(app, &c);
    tracing::info!(port, dir = %dir.display(), "Aquamarine Server is running");
    Ok(status_of(app))
}

async fn stop_inner(app: &AppHandle, remember: bool) -> Status {
    let stop = RUNNING.lock().ok().and_then(|mut r| r.as_mut().and_then(|x| x.stop.take()));
    if let Some(tx) = stop {
        let _ = tx.send(());
    }
    for _ in 0..30 {
        if RUNNING.lock().map(|r| r.is_none()).unwrap_or(true) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    if let Ok(mut r) = RUNNING.lock() {
        *r = None;
    }
    #[cfg(target_os = "android")]
    android::keep_alive(false, 0);
    if remember {
        let mut c = load(app);
        c.run = false;
        save(app, &c);
    }
    status_of(app)
}

#[tauri::command]
fn status(app: AppHandle) -> Status {
    status_of(&app)
}

#[tauri::command]
async fn start(app: AppHandle, port: Option<u16>) -> Result<Status, String> {
    start_inner(&app, port).await
}

#[tauri::command]
async fn stop(app: AppHandle) -> Status {
    stop_inner(&app, true).await
}

/// A new port: saved, and the server restarts on it if it runs.
#[tauri::command]
async fn set_port(app: AppHandle, port: u16) -> Result<Status, String> {
    if port < 1024 {
        return Err("use a port from 1024 to 65535".into());
    }
    let mut c = load(&app);
    c.port = port;
    save(&app, &c);
    if RUNNING.lock().map(|r| r.is_some()).unwrap_or(false) {
        stop_inner(&app, false).await;
        return start_inner(&app, Some(port)).await;
    }
    Ok(status_of(&app))
}

/// Another data folder (not while it runs). `copy`: bring the current data
/// along; otherwise the folder is used as it is (an older server's folder).
#[tauri::command]
fn set_dir(app: AppHandle, path: String, copy: bool) -> Result<Status, String> {
    if RUNNING.lock().map(|r| r.is_some()).unwrap_or(false) {
        return Err("stop the server first".into());
    }
    let to = PathBuf::from(path.trim());
    if !to.is_absolute() {
        return Err("choose a folder".into());
    }
    let mut c = load(&app);
    let from = data_dir(&app, &c);
    std::fs::create_dir_all(&to).map_err(|e| format!("{}: {e}", to.display()))?;
    let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    if copy && canon(&from) != canon(&to) && aqua_server::datadir::has_data(&from) && !aqua_server::datadir::has_data(&to) {
        if canon(&to).starts_with(canon(&from)) {
            return Err("the new folder can't be inside the current one".into());
        }
        copy_tree(&from, &to).map_err(|e| e.to_string())?;
    }
    c.dir = to.display().to_string();
    save(&app, &c);
    #[cfg(desktop)]
    let _ = aqua_server::datadir::remember(&to);
    Ok(status_of(&app))
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)?.flatten() {
        let (src, dst) = (e.path(), to.join(e.file_name()));
        let md = e.metadata()?;
        if md.is_dir() {
            copy_tree(&src, &dst)?;
        } else if md.is_file() {
            std::fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

/// Computers: a folder picker.
#[tauri::command]
async fn pick_dir(app: AppHandle) -> Option<String> {
    #[cfg(desktop)]
    {
        use tauri_plugin_dialog::DialogExt;
        let (tx, rx) = oneshot::channel();
        app.dialog().file().pick_folder(move |p| {
            let _ = tx.send(p.and_then(|p| p.into_path().ok()).map(|p| p.display().to_string()));
        });
        return rx.await.ok().flatten();
    }
    #[allow(unreachable_code)]
    {
        let _ = app;
        None
    }
}

#[tauri::command]
fn set_autostart(app: AppHandle, on: bool) -> Result<Status, String> {
    #[cfg(desktop)]
    {
        use tauri_plugin_autostart::ManagerExt;
        let a = app.autolaunch();
        if on { a.enable() } else { a.disable() }.map_err(|e| e.to_string())?;
    }
    #[cfg(mobile)]
    let _ = on;
    Ok(status_of(&app))
}

#[tauri::command]
fn set_tray(app: AppHandle, on: bool) -> Status {
    let mut c = load(&app);
    c.tray = on;
    save(&app, &c);
    TRAY.store(on, std::sync::atomic::Ordering::Relaxed);
    status_of(&app)
}

/// The window's own panel talks to the running relay through this: one call
/// to `/v1/admin/…` with this run's token. No browser, no web page inside.
#[tauri::command]
async fn admin(method: String, path: String, body: Option<serde_json::Value>) -> Result<serde_json::Value, String> {
    if !path.starts_with("/v1/admin/") || path.contains("..") {
        return Err("not an admin path".into());
    }
    let (port, token) = RUNNING
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|r| (r.port, r.token.clone())))
        .ok_or("the server isn't running")?;
    let url = format!("http://127.0.0.1:{port}{path}");
    let c = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?;
    let m = reqwest::Method::from_bytes(method.to_uppercase().as_bytes()).map_err(|e| e.to_string())?;
    let mut rq = c.request(m, &url).bearer_auth(&token);
    if let Some(b) = body {
        rq = rq.json(&b);
    }
    let r = rq.send().await.map_err(|e| e.to_string())?;
    let ok = r.status().is_success();
    let code = r.status().as_u16();
    let text = r.text().await.unwrap_or_default();
    if !ok {
        return Err(if text.trim().is_empty() { format!("HTTP {code}") } else { text });
    }
    Ok(serde_json::from_str(&text).unwrap_or(serde_json::Value::Null))
}

#[derive(Serialize)]
struct UpdateView {
    #[serde(flatten)]
    st: updates::UpdState,
    auto: bool,
}

#[tauri::command]
fn update_status(app: AppHandle) -> UpdateView {
    UpdateView { st: updates::snapshot(), auto: load(&app).auto_update }
}

#[tauri::command]
fn set_auto_update(app: AppHandle, on: bool) -> UpdateView {
    let mut c = load(&app);
    c.auto_update = on;
    save(&app, &c);
    update_status(app)
}

/// "Check now" / "Update": check, download, and install when asked.
#[tauri::command]
async fn update_now(app: AppHandle, install: bool) -> Result<UpdateView, String> {
    if install && updates::snapshot().state == "ready" {
        updates::install_now(&app)?;
    } else {
        updates::run_once(&app, install).await;
    }
    Ok(update_status(app))
}

/// The release page of the newer version.
#[tauri::command]
fn open_notes(app: AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let url = updates::snapshot().notes_url.filter(|u| u.starts_with("https://github.com/")).ok_or("no release page")?;
    app.opener().open_url(url, None::<&str>).map_err(|e| e.to_string())
}

/// The step-by-step guide (router, public IP) in the browser.
#[tauri::command]
fn open_guide(app: AppHandle, page: Option<String>) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    // Only our own guides: self-hosting (default) or the error codes.
    let d = if page.as_deref() == Some("errors") { "errors" } else { "self-hosting" };
    app.opener()
        .open_url(format!("https://applememestm-pixel.github.io/Aquamarine/docs.html?d={d}"), None::<&str>)
        .map_err(|e| e.to_string())
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        #[cfg(desktop)]
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

#[cfg(desktop)]
fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
    let open = MenuItem::with_id(app, "open", "Open Aquamarine Server", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Stop the server and quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png"))?;
    TrayIconBuilder::with_id("main")
        .icon(icon)
        .tooltip("Aquamarine Server")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, e| match e.id.as_ref() {
            "open" => show_main(app),
            "quit" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    stop_inner(&app, false).await;
                    app.exit(0);
                });
            }
            _ => {}
        })
        .on_tray_icon_event(|t, e| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                show_main(t.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

#[cfg(target_os = "android")]
mod android {
    //! The foreground service lives in Kotlin (android/AquaServer.kt).
    use std::sync::OnceLock;
    use tauri::plugin::{Builder, PluginHandle, TauriPlugin};
    use tauri::Wry;

    static HANDLE: OnceLock<PluginHandle<Wry>> = OnceLock::new();

    pub fn plugin() -> TauriPlugin<Wry> {
        Builder::new("aquaserver")
            .setup(|_app, api| {
                match api.register_android_plugin("app.aquamarine.server", "AquaServerPlugin") {
                    Ok(h) => {
                        let _ = HANDLE.set(h);
                    }
                    Err(e) => tracing::warn!(?e, "server app: Android plugin missing"),
                }
                Ok(())
            })
            .build()
    }

    pub fn keep_alive(on: bool, port: u16) {
        let Some(h) = HANDLE.get() else { return };
        let cmd = if on { "start" } else { "stop" };
        if let Err(e) = h.run_mobile_plugin::<serde_json::Value>(cmd, serde_json::json!({ "port": port })) {
            tracing::warn!(%e, "server app: foreground service {cmd}");
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    {
        use tracing_subscriber::prelude::*;
        let _ = tracing_subscriber::registry()
            .with(tracing_subscriber::fmt::layer().with_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into())))
            // Server failures go to the developer right away (aqua_server::report).
            .with(aqua_server::report::ErrorLayer { app: "aquamarine-server-app" })
            .try_init();
    }
    aqua_server::report::install_panic_hook("aquamarine-server-app");
    let builder = tauri::Builder::default();
    // One copy at a time: the autostart one and a click on the shortcut
    // used to race for the port ("port busy", then fine after a restart).
    // A second launch just brings the running window forward.
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main(app)));
    let builder = builder.plugin(tauri_plugin_dialog::init()).plugin(tauri_plugin_opener::init());
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec!["--hidden"])));
    #[cfg(target_os = "android")]
    let builder = builder.plugin(android::plugin());
    let res = builder
        .setup(|app| {
            let handle = app.handle().clone();
            let c = load(&handle);
            TRAY.store(c.tray, std::sync::atomic::Ordering::Relaxed);
            #[cfg(desktop)]
            {
                if let Err(e) = build_tray(&handle) {
                    tracing::warn!(?e, "no tray icon");
                }
                // Started with the system: straight to the tray.
                if std::env::args().any(|a| a == "--hidden") {
                    if let Some(w) = handle.get_webview_window("main") {
                        let _ = w.hide();
                    }
                }
            }
            updates::spawn(handle.clone(), |a| load(a).auto_update);
            if c.run {
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = start_inner(&handle, None).await {
                        tracing::warn!(%e, "the server didn't start");
                    }
                });
            }
            Ok(())
        })
        .on_window_event(|w, e| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = e {
                if cfg!(desktop) && w.label() == "main" && TRAY.load(std::sync::atomic::Ordering::Relaxed) {
                    api.prevent_close();
                    let _ = w.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![status, start, stop, set_port, set_dir, pick_dir, set_autostart, set_tray, open_guide, admin, update_status, set_auto_update, update_now, open_notes])
        .run(tauri::generate_context!());
    if let Err(e) = res {
        eprintln!("aquamarine server: {e:?}");
    }
}
