//! Nearby delivery over BlueZ: an RFCOMM profile (`org.bluez.Profile1`)
//! with Aquamarine's service UUID. BlueZ publishes the SDP record, accepts
//! incoming connections and, for `Device1.ConnectProfile`, dials out —
//! either way the socket arrives in `NewConnection`.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, OwnedFd};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use tauri::AppHandle;
use tokio::sync::oneshot;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};
use zbus::Connection;

use super::{book, run_session, SyncReport, SERVICE_UUID};

pub const SUPPORTED: bool = true;
const PROFILE_PATH: &str = "/org/aquamarine/mesh";
/// Fixed RFCOMM channel for our SDP record (BlueZ wants one for a vendor UUID).
const CHANNEL: u16 = 26;

static READY: AtomicBool = AtomicBool::new(false);
static SYS: OnceLock<tokio::sync::Mutex<Option<Connection>>> = OnceLock::new();
/// Connections we dialled, by device path: who waits for the result.
static DIALLING: OnceLock<Mutex<HashMap<String, oneshot::Sender<SyncReport>>>> = OnceLock::new();

fn dialling() -> &'static Mutex<HashMap<String, oneshot::Sender<SyncReport>>> {
    DIALLING.get_or_init(Default::default)
}

pub fn ready() -> bool {
    READY.load(Ordering::Relaxed)
}

async fn system() -> Result<Connection, String> {
    let cell = SYS.get_or_init(|| tokio::sync::Mutex::new(None));
    let mut g = cell.lock().await;
    if let Some(c) = g.as_ref() {
        return Ok(c.clone());
    }
    let c = Connection::system().await.map_err(|e| format!("system bus: {e}"))?;
    *g = Some(c.clone());
    Ok(c)
}

fn mac_of(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).trim_start_matches("dev_").replace('_', ":")
}

/// A blocking byte stream over the RFCOMM socket BlueZ handed us.
struct FdStream(std::fs::File);

impl FdStream {
    fn new(fd: OwnedFd) -> Self {
        let raw = fd.as_raw_fd();
        // SAFETY: plain fcntl / setsockopt on a socket we own.
        unsafe {
            let flags = libc::fcntl(raw, libc::F_GETFL);
            if flags >= 0 {
                libc::fcntl(raw, libc::F_SETFL, flags & !libc::O_NONBLOCK);
            }
            // A peer that stops answering mustn't hold the thread forever.
            let tv = libc::timeval { tv_sec: 25, tv_usec: 0 };
            let p = &tv as *const libc::timeval as *const libc::c_void;
            let n = std::mem::size_of::<libc::timeval>() as libc::socklen_t;
            libc::setsockopt(raw, libc::SOL_SOCKET, libc::SO_RCVTIMEO, p, n);
            libc::setsockopt(raw, libc::SOL_SOCKET, libc::SO_SNDTIMEO, p, n);
        }
        Self(std::fs::File::from(fd))
    }
}
impl Read for FdStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> { self.0.read(buf) }
}
impl Write for FdStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> { self.0.write(buf) }
    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}

struct Profile {
    app: AppHandle,
}

#[zbus::interface(name = "org.bluez.Profile1")]
impl Profile {
    fn release(&self) {
        READY.store(false, Ordering::Relaxed);
    }

    fn new_connection(&self, device: OwnedObjectPath, fd: zbus::zvariant::OwnedFd, _props: HashMap<String, OwnedValue>) -> zbus::fdo::Result<()> {
        let fd: OwnedFd = fd.into();
        let waiter = dialling().lock().ok().and_then(|mut m| m.remove(device.as_str()));
        let app = self.app.clone();
        let mac = mac_of(device.as_str());
        std::thread::spawn(move || {
            let mut link = FdStream::new(fd);
            let rep = run_session(&app, &mut link, waiter.is_some(), &mac);
            if let Some(tx) = waiter {
                let _ = tx.send(rep);
            }
        });
        Ok(())
    }

    fn request_disconnection(&self, _device: OwnedObjectPath) {}
}

pub async fn listen(app: &AppHandle) -> Result<(), String> {
    if ready() {
        return Ok(());
    }
    let sys = system().await?;
    let path = ObjectPath::try_from(PROFILE_PATH).map_err(|e| e.to_string())?;
    let _ = sys.object_server().remove::<Profile, _>(PROFILE_PATH).await;
    sys.object_server().at(PROFILE_PATH, Profile { app: app.clone() }).await.map_err(|e| e.to_string())?;
    let mgr = zbus::Proxy::new(&sys, "org.bluez", "/org/bluez", "org.bluez.ProfileManager1").await.map_err(|e| format!("BlueZ: {e}"))?;
    let mut opts: HashMap<&str, Value> = HashMap::new();
    opts.insert("Name", Value::from("Aquamarine"));
    opts.insert("Channel", Value::from(CHANNEL));
    opts.insert("RequireAuthentication", Value::from(false));
    opts.insert("RequireAuthorization", Value::from(false));
    opts.insert("AutoConnect", Value::from(false));
    match mgr.call_method("RegisterProfile", &(path, SERVICE_UUID, opts)).await {
        Ok(_) => {}
        Err(e) if e.to_string().contains("AlreadyExists") => {}
        Err(e) => {
            let _ = sys.object_server().remove::<Profile, _>(PROFILE_PATH).await;
            return Err(friendly(&e.to_string()));
        }
    }
    READY.store(true, Ordering::Relaxed);
    tracing::info!("mesh: listening for nearby contacts (RFCOMM {CHANNEL})");
    Ok(())
}

pub async fn stop(_app: &AppHandle) {
    if let Ok(sys) = system().await {
        if let Ok(mgr) = zbus::Proxy::new(&sys, "org.bluez", "/org/bluez", "org.bluez.ProfileManager1").await {
            if let Ok(p) = ObjectPath::try_from(PROFILE_PATH) {
                let _ = mgr.call_method("UnregisterProfile", &(p,)).await;
            }
        }
        let _ = sys.object_server().remove::<Profile, _>(PROFILE_PATH).await;
    }
    READY.store(false, Ordering::Relaxed);
}

fn friendly(e: &str) -> String {
    if e.contains("ServiceUnknown") || e.contains("org.bluez was not provided") {
        "Bluetooth service isn't running (systemctl start bluetooth)".into()
    } else {
        format!("Bluetooth: {e}")
    }
}

type Objects = HashMap<OwnedObjectPath, HashMap<zbus::names::OwnedInterfaceName, HashMap<String, OwnedValue>>>;

fn iface<'a>(m: &'a HashMap<zbus::names::OwnedInterfaceName, HashMap<String, OwnedValue>>, name: &str) -> Option<&'a HashMap<String, OwnedValue>> {
    m.iter().find(|(k, _)| k.as_str() == name).map(|(_, v)| v)
}
fn s(v: Option<&OwnedValue>) -> String {
    match v.map(|v| &**v) {
        Some(Value::Str(x)) => x.to_string(),
        _ => String::new(),
    }
}
fn b(v: Option<&OwnedValue>) -> bool {
    matches!(v.map(|v| &**v), Some(Value::Bool(true)))
}
fn strs(v: Option<&OwnedValue>) -> Vec<String> {
    match v.map(|v| &**v) {
        Some(Value::Array(a)) => a.iter().filter_map(|x| if let Value::Str(s) = x { Some(s.to_string()) } else { None }).collect(),
        _ => Vec::new(),
    }
}

async fn objects(sys: &Connection) -> Result<Objects, String> {
    let om = zbus::fdo::ObjectManagerProxy::builder(sys)
        .destination("org.bluez").map_err(|e| e.to_string())?
        .path("/").map_err(|e| e.to_string())?
        .build().await.map_err(|e| friendly(&e.to_string()))?;
    om.get_managed_objects().await.map_err(|e| friendly(&e.to_string()))
}

/// One round: (optionally look for new devices, then) dial every nearby
/// device that may be a contact and swap messages.
pub async fn sync_round(app: &AppHandle, scan: bool) -> Result<(), String> {
    listen(app).await?;
    let sys = system().await?;
    let mut objs = objects(&sys).await?;
    let adapter = objs
        .iter()
        .filter(|(_, m)| iface(m, "org.bluez.Adapter1").is_some())
        .map(|(p, m)| (p.clone(), b(iface(m, "org.bluez.Adapter1").and_then(|a| a.get("Powered")))))
        .min_by(|a, b| a.0.as_str().cmp(b.0.as_str()));
    let Some((adapter, powered)) = adapter else { return Err("no Bluetooth adapter".into()) };
    if !powered {
        return Err("Bluetooth is off".into());
    }
    if scan {
        if let Ok(ad) = zbus::Proxy::new(&sys, "org.bluez", adapter.as_str(), "org.bluez.Adapter1").await {
            let _ = ad.call_method("StartDiscovery", &()).await;
            tokio::time::sleep(Duration::from_secs(8)).await;
            let _ = ad.call_method("StopDiscovery", &()).await;
        }
        objs = objects(&sys).await?;
    }
    let known: HashMap<String, u64> = book(app).peers.iter().map(|(k, p)| (k.clone(), p.last)).collect();
    let mut cands: Vec<(u64, String)> = objs
        .iter()
        .filter_map(|(p, m)| {
            let d = iface(m, "org.bluez.Device1")?;
            if !p.as_str().starts_with(adapter.as_str()) {
                return None;
            }
            let mac = s(d.get("Address"));
            let icon = s(d.get("Icon"));
            let ours = strs(d.get("UUIDs")).iter().any(|u| u.eq_ignore_ascii_case(SERVICE_UUID));
            let seen = known.get(&mac).copied();
            // Phones and computers that are paired, advertise us, or answered before.
            let kind_ok = icon.is_empty() || icon.starts_with("phone") || icon.starts_with("computer");
            if ours || seen.is_some() || (b(d.get("Paired")) && kind_ok) {
                Some((seen.unwrap_or(if ours { 1 } else { 0 }), p.to_string()))
            } else {
                None
            }
        })
        .collect();
    cands.sort_by(|a, b| b.0.cmp(&a.0));
    if cands.is_empty() {
        return Err("no Aquamarine devices nearby — pair the phone or PC once in Bluetooth settings".into());
    }
    let mut met = 0;
    for (_, path) in cands.into_iter().take(8) {
        let (tx, rx) = oneshot::channel();
        if let Ok(mut m) = dialling().lock() {
            m.insert(path.clone(), tx);
        }
        let dev = match zbus::Proxy::new(&sys, "org.bluez", path.as_str(), "org.bluez.Device1").await {
            Ok(d) => d,
            Err(_) => continue,
        };
        let call = tokio::time::timeout(Duration::from_secs(20), dev.call_method("ConnectProfile", &(SERVICE_UUID,))).await;
        match call {
            Ok(Ok(_)) => {
                if let Ok(Ok(rep)) = tokio::time::timeout(Duration::from_secs(90), rx).await {
                    if rep.error.is_none() {
                        met += 1;
                    }
                }
            }
            _ => {
                if let Ok(mut m) = dialling().lock() {
                    m.remove(&path);
                }
            }
        }
    }
    if met == 0 && scan {
        return Err("no contacts with Aquamarine answered nearby".into());
    }
    Ok(())
}
