//! Send and receive files over Bluetooth (OBEX Object Push), like the
//! "Send via Bluetooth" of a phone.
//!
//! Linux talks to BlueZ over D-Bus: devices and the adapter on the system
//! bus (`org.bluez`), transfers through `obexd` on the session bus
//! (`org.bluez.obex`, package `bluez-obex` on Arch). To receive, the app
//! registers an OBEX agent: each incoming file is shown to the user
//! (accept / decline) and saved to `~/Downloads/Aquamarine Bluetooth`.
//! Other platforms report "not supported".
//!
//! Events: `aqua://bt-progress` {id, name, sent, size, status, path?, incoming},
//! `aqua://bt-incoming` {id, name, size, from}.

use serde::Serialize;

#[derive(Debug, Clone, Serialize, Default)]
pub struct BtStatus {
    /// This build can do Bluetooth at all.
    pub supported: bool,
    /// There is an adapter.
    pub adapter: bool,
    pub powered: bool,
    /// obexd answers on the session bus.
    pub obex: bool,
    /// Our agent receives files.
    pub receiving: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BtDevice {
    pub address: String,
    pub name: String,
    pub icon: String,
    pub paired: bool,
    pub connected: bool,
    /// Advertises Object Push (or nothing at all — many phones don't list it).
    pub push: bool,
    pub rssi: Option<i16>,
}

#[cfg(target_os = "linux")]
mod imp {
    use super::{BtDevice, BtStatus};
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::{Mutex, OnceLock};
    use std::time::Duration;
    use tauri::{AppHandle, Emitter};
    use tokio::sync::oneshot;
    use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};
    use zbus::Connection;

    const OPP_UUID: &str = "00001105-0000-1000-8000-00805f9b34fb";
    const AGENT_PATH: &str = "/org/aquamarine/obexagent";

    static RECEIVING: OnceLock<Mutex<bool>> = OnceLock::new();
    static PENDING: OnceLock<Mutex<HashMap<String, oneshot::Sender<bool>>>> = OnceLock::new();
    static SESSION: OnceLock<tokio::sync::Mutex<Option<Connection>>> = OnceLock::new();

    fn pending() -> &'static Mutex<HashMap<String, oneshot::Sender<bool>>> { PENDING.get_or_init(Default::default) }
    fn receiving() -> bool { RECEIVING.get().map(|m| *m.lock().unwrap_or_else(|e| e.into_inner())).unwrap_or(false) }

    fn s(v: Option<&OwnedValue>) -> String {
        match v.map(|v| &**v) {
            Some(Value::Str(x)) => x.to_string(),
            Some(Value::ObjectPath(p)) => p.to_string(),
            _ => String::new(),
        }
    }
    fn b(v: Option<&OwnedValue>) -> bool { matches!(v.map(|v| &**v), Some(Value::Bool(true))) }
    fn u(v: Option<&OwnedValue>) -> u64 {
        match v.map(|v| &**v) {
            Some(Value::U64(x)) => *x,
            Some(Value::U32(x)) => *x as u64,
            Some(Value::I64(x)) => (*x).max(0) as u64,
            _ => 0,
        }
    }
    fn strs(v: Option<&OwnedValue>) -> Vec<String> {
        match v.map(|v| &**v) {
            Some(Value::Array(a)) => a.iter().filter_map(|x| if let Value::Str(s) = x { Some(s.to_string()) } else { None }).collect(),
            _ => Vec::new(),
        }
    }

    type Objects = HashMap<OwnedObjectPath, HashMap<zbus::names::OwnedInterfaceName, HashMap<String, OwnedValue>>>;

    async fn objects(sys: &Connection) -> zbus::Result<Objects> {
        let om = zbus::fdo::ObjectManagerProxy::builder(sys).destination("org.bluez")?.path("/")?.build().await?;
        Ok(om.get_managed_objects().await?)
    }
    fn iface<'a>(m: &'a HashMap<zbus::names::OwnedInterfaceName, HashMap<String, OwnedValue>>, name: &str) -> Option<&'a HashMap<String, OwnedValue>> {
        m.iter().find(|(k, _)| k.as_str() == name).map(|(_, v)| v)
    }
    fn adapter_path(objs: &Objects) -> Option<(OwnedObjectPath, bool)> {
        let mut v: Vec<_> = objs.iter().filter_map(|(p, m)| iface(m, "org.bluez.Adapter1").map(|a| (p.clone(), b(a.get("Powered"))))).collect();
        v.sort_by(|a, b| a.0.as_str().cmp(b.0.as_str()));
        v.into_iter().next()
    }

    async fn session() -> zbus::Result<Connection> {
        let cell = SESSION.get_or_init(|| tokio::sync::Mutex::new(None));
        let mut g = cell.lock().await;
        if let Some(c) = g.as_ref() {
            return Ok(c.clone());
        }
        let c = Connection::session().await?;
        *g = Some(c.clone());
        Ok(c)
    }

    async fn obex_alive(ses: &Connection) -> bool {
        // D-Bus activation starts obexd on demand; Ping makes it start.
        let Ok(p) = zbus::Proxy::new(ses, "org.bluez.obex", "/org/bluez/obex", "org.freedesktop.DBus.Peer").await else { return false };
        tokio::time::timeout(Duration::from_secs(4), p.call_method("Ping", &())).await.map(|r| r.is_ok()).unwrap_or(false)
    }

    pub async fn status() -> BtStatus {
        let mut st = BtStatus { supported: true, ..Default::default() };
        match Connection::system().await {
            Ok(sys) => match objects(&sys).await {
                Ok(objs) => {
                    if let Some((_, powered)) = adapter_path(&objs) {
                        st.adapter = true;
                        st.powered = powered;
                    }
                }
                Err(e) => st.error = Some(format!("BlueZ: {e}")),
            },
            Err(e) => st.error = Some(format!("system bus: {e}")),
        }
        if let Ok(ses) = session().await {
            st.obex = obex_alive(&ses).await;
        }
        st.receiving = receiving();
        st
    }

    pub async fn power(on: bool) -> Result<(), String> {
        let sys = Connection::system().await.map_err(|e| e.to_string())?;
        let objs = objects(&sys).await.map_err(|e| e.to_string())?;
        let (path, _) = adapter_path(&objs).ok_or("no Bluetooth adapter")?;
        let p = zbus::fdo::PropertiesProxy::builder(&sys).destination("org.bluez").map_err(|e| e.to_string())?
            .path(path).map_err(|e| e.to_string())?.build().await.map_err(|e| e.to_string())?;
        p.set(zbus::names::InterfaceName::from_static_str_unchecked("org.bluez.Adapter1"), "Powered", Value::from(on))
            .await.map_err(|e| e.to_string())
    }

    pub async fn devices(scan_secs: u64) -> Result<Vec<BtDevice>, String> {
        let sys = Connection::system().await.map_err(|e| e.to_string())?;
        if scan_secs > 0 {
            let objs = objects(&sys).await.map_err(|e| e.to_string())?;
            if let Some((ad, _)) = adapter_path(&objs) {
                if let Ok(p) = zbus::Proxy::new(&sys, "org.bluez", ad, "org.bluez.Adapter1").await {
                    if p.call_method("StartDiscovery", &()).await.is_ok() {
                        tokio::time::sleep(Duration::from_secs(scan_secs.min(20))).await;
                        let _ = p.call_method("StopDiscovery", &()).await;
                    }
                }
            }
        }
        let objs = objects(&sys).await.map_err(|e| e.to_string())?;
        let mut out: Vec<BtDevice> = objs.values().filter_map(|m| {
            let d = iface(m, "org.bluez.Device1")?;
            let uuids = strs(d.get("UUIDs"));
            let name = s(d.get("Alias"));
            let address = s(d.get("Address"));
            if address.is_empty() {
                return None;
            }
            Some(BtDevice {
                name: if name.is_empty() { address.clone() } else { name },
                address,
                icon: s(d.get("Icon")),
                paired: b(d.get("Paired")),
                connected: b(d.get("Connected")),
                push: uuids.is_empty() || uuids.iter().any(|x| x.eq_ignore_ascii_case(OPP_UUID)),
                rssi: match d.get("RSSI").map(|v| &**v) { Some(Value::I16(x)) => Some(*x), _ => None },
            })
        }).collect();
        // Phones and computers first, then paired, then by name.
        let rank = |d: &BtDevice| (!(d.icon.starts_with("phone") || d.icon.starts_with("computer")), !d.paired, d.name.to_lowercase());
        out.sort_by_key(rank);
        Ok(out)
    }

    fn emit_progress(app: &AppHandle, id: &str, name: &str, sent: u64, size: u64, status: &str, path: Option<&str>, incoming: bool) {
        let _ = app.emit("aqua://bt-progress", serde_json::json!({
            "id": id, "name": name, "sent": sent, "size": size, "status": status, "path": path, "incoming": incoming,
        }));
    }

    /// Follow a transfer until it completes or fails.
    async fn watch(app: &AppHandle, ses: &Connection, transfer: &OwnedObjectPath, id: &str, name: &str, path: Option<&str>, incoming: bool) -> Result<(), String> {
        let p = zbus::fdo::PropertiesProxy::builder(ses).destination("org.bluez.obex").map_err(|e| e.to_string())?
            .path(transfer.clone()).map_err(|e| e.to_string())?.build().await.map_err(|e| e.to_string())?;
        let iface = zbus::names::InterfaceName::from_static_str_unchecked("org.bluez.obex.Transfer1");
        let mut size = 0;
        let mut sent = 0;
        let mut last_emit = 0u64;
        for _ in 0..(6 * 3600 * 4) {
            let props = match p.get_all(iface.clone()).await {
                Ok(m) => m,
                // obexd drops the object right after it completes.
                Err(_) => {
                    if sent > 0 && (size == 0 || sent >= size) {
                        emit_progress(app, id, name, size.max(sent), size.max(sent), "complete", path, incoming);
                        return Ok(());
                    }
                    emit_progress(app, id, name, sent, size, "error", path, incoming);
                    return Err("the transfer disappeared".into());
                }
            };
            size = u(props.get("Size")).max(size);
            sent = u(props.get("Transferred")).max(sent);
            let status = s(props.get("Status"));
            match status.as_str() {
                "complete" => {
                    emit_progress(app, id, name, size.max(sent), size.max(sent), "complete", path, incoming);
                    return Ok(());
                }
                "error" => {
                    emit_progress(app, id, name, sent, size, "error", path, incoming);
                    return Err("the other device stopped the transfer".into());
                }
                _ => {
                    if sent != last_emit || last_emit == 0 {
                        last_emit = sent.max(1);
                        emit_progress(app, id, name, sent, size, if status.is_empty() { "queued" } else { &status }, path, incoming);
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        Err("timed out".into())
    }

    pub async fn send(app: AppHandle, address: String, files: Vec<String>, id: String) -> Result<(), String> {
        if !address.chars().all(|c| c.is_ascii_hexdigit() || c == ':') || address.len() != 17 {
            return Err("bad device address".into());
        }
        let ses = session().await.map_err(|e| e.to_string())?;
        if !obex_alive(&ses).await {
            return Err("obexd is not running — install bluez-obex (Arch: pacman -S bluez-obex) and enable Bluetooth".into());
        }
        let client = zbus::Proxy::new(&ses, "org.bluez.obex", "/org/bluez/obex", "org.bluez.obex.Client1").await.map_err(|e| e.to_string())?;
        let mut args: HashMap<&str, Value> = HashMap::new();
        args.insert("Target", Value::from("opp"));
        let reply = tokio::time::timeout(Duration::from_secs(40), client.call_method("CreateSession", &(address.as_str(), args)))
            .await.map_err(|_| "the device did not answer — is Bluetooth on and is it nearby?".to_string())?
            .map_err(|e| friendly(&e.to_string()))?;
        let sess: OwnedObjectPath = reply.body().deserialize().map_err(|e| e.to_string())?;
        let push = zbus::Proxy::new(&ses, "org.bluez.obex", sess.clone(), "org.bluez.obex.ObjectPush1").await.map_err(|e| e.to_string())?;
        let mut result = Ok(());
        for (i, f) in files.iter().enumerate() {
            let name = std::path::Path::new(f).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let fid = format!("{id}-{i}");
            emit_progress(&app, &fid, &name, 0, std::fs::metadata(f).map(|m| m.len()).unwrap_or(0), "queued", None, false);
            let r = match push.call_method("SendFile", &(f.as_str(),)).await {
                Ok(m) => {
                    let (t, _props): (OwnedObjectPath, HashMap<String, OwnedValue>) = m.body().deserialize().map_err(|e| e.to_string())?;
                    watch(&app, &ses, &t, &fid, &name, None, false).await
                }
                Err(e) => {
                    emit_progress(&app, &fid, &name, 0, 0, "error", None, false);
                    Err(friendly(&e.to_string()))
                }
            };
            if r.is_err() {
                result = r;
                break;
            }
        }
        let _ = client.call_method("RemoveSession", &(ObjectPath::from(&sess),)).await;
        result
    }

    fn friendly(e: &str) -> String {
        if e.contains("Rejected") || e.contains("Forbidden") || e.contains("Not Authorized") {
            "the other device declined".into()
        } else if e.contains("Connection refused") || e.contains("Host is down") || e.contains("Unable to") {
            "could not connect — turn on Bluetooth on the other device and make it visible".into()
        } else {
            e.to_string()
        }
    }

    pub fn answer(id: &str, accept: bool) {
        if let Some(tx) = pending().lock().unwrap_or_else(|e| e.into_inner()).remove(id) {
            let _ = tx.send(accept);
        }
    }

    pub fn inbox() -> PathBuf {
        let base = std::env::var_os("XDG_DOWNLOAD_DIR").map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Downloads")))
            .unwrap_or_else(std::env::temp_dir);
        base.join("Aquamarine Bluetooth")
    }
    fn safe_name(n: &str) -> String {
        let n: String = n.chars().map(|c| if c == '/' || c == '\\' || c == '\0' || c.is_control() { '_' } else { c }).collect();
        let n = n.trim_start_matches('.').trim();
        if n.is_empty() { "file".into() } else { n.chars().take(120).collect() }
    }

    struct Agent {
        app: AppHandle,
    }

    #[zbus::interface(name = "org.bluez.obex.Agent1")]
    impl Agent {
        async fn authorize_push(&self, transfer: OwnedObjectPath, #[zbus(connection)] conn: &Connection) -> zbus::fdo::Result<String> {
            let props = async {
                let p = zbus::fdo::PropertiesProxy::builder(conn).destination("org.bluez.obex").ok()?.path(transfer.clone()).ok()?.build().await.ok()?;
                p.get_all(zbus::names::InterfaceName::from_static_str_unchecked("org.bluez.obex.Transfer1")).await.ok()
            }.await.unwrap_or_default();
            let name = safe_name(&s(props.get("Name")));
            let size = u(props.get("Size"));
            let session = s(props.get("Session"));
            // Who is sending: the session's Destination (MAC) → its alias.
            let mut from = String::new();
            if let Ok(sp) = zbus::fdo::PropertiesProxy::builder(conn).destination("org.bluez.obex").and_then(|b| b.path(session.as_str())) {
                if let Ok(sp) = sp.build().await {
                    if let Ok(v) = sp.get(zbus::names::InterfaceName::from_static_str_unchecked("org.bluez.obex.Session1"), "Destination").await {
                        from = s(Some(&v));
                    }
                }
            }
            if let Ok(list) = devices(0).await {
                if let Some(d) = list.iter().find(|d| d.address.eq_ignore_ascii_case(&from)) {
                    from = d.name.clone();
                }
            }
            let id = format!("in-{}", rand::random::<u32>());
            let (tx, rx) = oneshot::channel();
            pending().lock().unwrap_or_else(|e| e.into_inner()).insert(id.clone(), tx);
            let _ = self.app.emit("aqua://bt-incoming", serde_json::json!({ "id": id, "name": name, "size": size, "from": from }));
            let ok = matches!(tokio::time::timeout(Duration::from_secs(60), rx).await, Ok(Ok(true)));
            pending().lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
            if !ok {
                return Err(zbus::fdo::Error::AccessDenied("declined".into()));
            }
            let dir = inbox();
            std::fs::create_dir_all(&dir).map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
            let mut path = dir.join(&name);
            let (stem, ext) = match name.rsplit_once('.') { Some((a, b)) if !a.is_empty() => (a.to_string(), format!(".{b}")), _ => (name.clone(), String::new()) };
            let mut n = 1;
            while path.exists() {
                path = dir.join(format!("{stem} ({n}){ext}"));
                n += 1;
            }
            let full = path.display().to_string();
            let app = self.app.clone();
            let c = conn.clone();
            let (id2, name2, full2) = (id.clone(), name.clone(), full.clone());
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(200)).await;
                let _ = watch(&app, &c, &transfer, &id2, &name2, Some(&full2), true).await;
            });
            Ok(full)
        }
        fn cancel(&self) {}
        fn release(&self) {
            if let Some(m) = RECEIVING.get() {
                *m.lock().unwrap_or_else(|e| e.into_inner()) = false;
            }
        }
    }

    /// Register the receiving agent with obexd (if Bluetooth is there at all).
    pub async fn start_receiving(app: AppHandle) -> Result<(), String> {
        let ses = session().await.map_err(|e| e.to_string())?;
        if !obex_alive(&ses).await {
            return Err("obexd is not running".into());
        }
        let first = ses.object_server().at(AGENT_PATH, Agent { app }).await.map_err(|e| e.to_string())?;
        let mgr = zbus::Proxy::new(&ses, "org.bluez.obex", "/org/bluez/obex", "org.bluez.obex.AgentManager1").await.map_err(|e| e.to_string())?;
        if let Err(e) = mgr.call_method("RegisterAgent", &(ObjectPath::try_from(AGENT_PATH).map_err(|e| e.to_string())?,)).await {
            if !first {
                // Registered by us already.
            } else {
                // Another agent (blueman, GNOME…) receives files already.
                let _ = ses.object_server().remove::<Agent, _>(AGENT_PATH).await;
                return Err(format!("another app receives Bluetooth files: {e}"));
            }
        }
        *RECEIVING.get_or_init(|| Mutex::new(false)).lock().unwrap_or_else(|e| e.into_inner()) = true;
        Ok(())
    }
}

// ---------- commands ----------

#[tauri::command]
pub async fn bt_status() -> BtStatus {
    #[cfg(target_os = "linux")]
    { imp::status().await }
    #[cfg(not(target_os = "linux"))]
    { BtStatus { supported: false, error: Some("Bluetooth transfers work in the Linux app for now".into()), ..Default::default() } }
}

#[tauri::command]
pub async fn bt_devices(scan: Option<u64>) -> Result<Vec<BtDevice>, String> {
    #[cfg(target_os = "linux")]
    { imp::devices(scan.unwrap_or(0)).await }
    #[cfg(not(target_os = "linux"))]
    { let _ = scan; Err("not supported on this platform".into()) }
}

#[tauri::command]
pub async fn bt_power(on: bool) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    { imp::power(on).await }
    #[cfg(not(target_os = "linux"))]
    { let _ = on; Err("not supported on this platform".into()) }
}

/// Send files to a device; progress comes as `aqua://bt-progress` events.
#[tauri::command]
pub async fn bt_send(app: tauri::AppHandle, address: String, files: Vec<String>) -> Result<String, String> {
    let id = format!("out-{}", rand::random::<u32>());
    if files.is_empty() || files.len() > 50 {
        return Err("pick 1–50 files".into());
    }
    #[cfg(target_os = "linux")]
    {
        let id2 = id.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = imp::send(app.clone(), address, files, id2.clone()).await {
                use tauri::Emitter;
                let _ = app.emit("aqua://bt-progress", serde_json::json!({ "id": id2, "status": "failed", "error": e }));
            }
        });
        Ok(id)
    }
    #[cfg(not(target_os = "linux"))]
    { let _ = (app, address); Err("not supported on this platform".into()) }
}

/// Accept or decline an incoming file.
#[tauri::command]
pub fn bt_answer(id: String, accept: bool) {
    #[cfg(target_os = "linux")]
    imp::answer(&id, accept);
    #[cfg(not(target_os = "linux"))]
    let _ = (id, accept);
}

/// Start receiving (called once the UI is up).
#[tauri::command]
pub async fn bt_receive(app: tauri::AppHandle) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    { imp::start_receiving(app).await }
    #[cfg(not(target_os = "linux"))]
    { let _ = app; Err("not supported on this platform".into()) }
}

/// Open the folder received files go to.
#[tauri::command]
pub fn bt_show_inbox() -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let d = imp::inbox();
        let _ = std::fs::create_dir_all(&d);
        std::process::Command::new("xdg-open").arg(&d).spawn().map(|_| ()).map_err(|e| e.to_string())
    }
    #[cfg(not(target_os = "linux"))]
    { Err("not supported on this platform".into()) }
}
