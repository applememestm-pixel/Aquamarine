//! Which of my own devices I trust with my chats.
//!
//! Like Telegram: a new device of the account works at once, and my other
//! devices say "New device signed in: it's me / sign it out". Signed out
//! here or on another device of mine (`[devno:v1]`) — it gets nothing from
//! this device any more. (Until 1.20.1-1 a new device got nothing until it
//! was approved; people found that a wall — the old phone was often gone.)
//!
//! Trust is by the device's key: the id is the first 6 bytes of its identity
//! key ([`crate::multidev::device_id`]), checked against the key of the E2EE
//! session (or of the bundle the session is started with) — not against the
//! address the relay routes to.
//!
//! Devices already on the account when this first runs are trusted once (an
//! update must not lock anybody out of their own phone). `dev-trust.json` in
//! the account folder.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex as StdMutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::state::AppState;

const FILE: &str = "dev-trust.json";

/// `[devok:v1]{"id"}` — my other device approved device `id`.
pub const DEVOK_PREFIX: &str = "[devok:v1]";
/// `[devno:v1]{"id"}` — my other device signed device `id` out.
pub const DEVNO_PREFIX: &str = "[devno:v1]";

#[derive(Default, Serialize, Deserialize)]
struct Trust {
    /// The devices that were there at the start are trusted.
    #[serde(default)]
    init: bool,
    #[serde(default)]
    ok: BTreeSet<String>,
    #[serde(default)]
    pending: BTreeMap<String, Pending>,
    /// Devices I said "not me" to.
    #[serde(default)]
    no: BTreeSet<String>,
}

/// A device of my account waiting for approval.
#[derive(Clone, Serialize, Deserialize)]
pub struct Pending {
    pub id: String,
    pub name: String,
    pub addr: String,
    /// When it showed up (unix s).
    pub at: u64,
    /// When it was added to the account (relay's clock, unix s).
    #[serde(default)]
    pub added: i64,
}

fn load(s: &AppState) -> Trust {
    std::fs::read(s.data_dir.join(FILE)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save(s: &AppState, t: &Trust) {
    if let Ok(b) = serde_json::to_vec(t) {
        let _ = std::fs::write(s.data_dir.join(FILE), b);
    }
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn my_id(s: &AppState) -> String {
    crate::multidev::device_id(s.identity.public().dh_pub.as_bytes())
}

/// May my device with key id `id` get my chats? (This device: always.
/// Before the first look at the account's devices: yes, as before.)
pub fn is_trusted(s: &AppState, id: &str) -> bool {
    if id == my_id(s) {
        return true;
    }
    !load(s).no.contains(id)
}

/// The key id of the device behind my session with `addr`, if there is one.
pub fn session_id(s: &AppState, addr: &str) -> Option<String> {
    s.sessions.get(addr).map(|x| crate::multidev::device_id(x.peer.dh_pub.as_bytes()))
}

/// Look at my account's devices: the first time, trust them all; later,
/// every new one waits for approval (and the UI is told).
pub async fn refresh(app: &AppHandle) {
    let state: State<'_, StdMutex<AppState>> = app.state();
    let Some(t) = state.lock().or_else(|e| Ok::<_, ()>(e.into_inner())).ok().and_then(|s| s.transport.clone()) else { return };
    let devs = match t.my_devices().await {
        Ok(d) => d,
        Err(e) => {
            tracing::debug!(%e, "device trust: no device list (older relay?)");
            return;
        }
    };
    let fresh = {
        let Ok(s) = state.lock().or_else(|e| Ok::<_, ()>(e.into_inner())) else { return };
        let mine = my_id(&s);
        let mut tr = load(&s);
        let mut fresh = false;
        if !tr.init {
            tr.ok.extend(devs.iter().map(|d| d.id.clone()));
            tr.ok.insert(mine.clone());
            tr.init = true;
            tracing::info!(devices = devs.len(), "device trust: the account's devices are trusted");
        } else {
            // A new device works right away; the question is only "was it you?".
            let new: Vec<_> = devs.iter().filter(|d| d.id != mine && !tr.ok.contains(&d.id) && !tr.no.contains(&d.id)).collect();
            for d in new {
                fresh = true;
                tracing::warn!(id = %d.id, name = %d.name, "a new device signed in to my account");
                tr.ok.insert(d.id.clone());
                tr.pending.insert(d.id.clone(), Pending { id: d.id.clone(), name: d.name.clone(), addr: d.addr.clone(), at: now(), added: d.added });
            }
        }
        // Signed out meanwhile: nothing to ask about.
        tr.pending.retain(|id, _| devs.iter().any(|d| &d.id == id));
        save(&s, &tr);
        fresh
    };
    if fresh {
        let _ = app.emit("aqua://device_pending", ());
        // Onto my signed list now, so people take its messages.
        republish(app);
    }
}

/// At connect: look now, then every few minutes (a new device that stays
/// quiet still gets asked about).
pub async fn start(app: &AppHandle) {
    static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    refresh(app).await;
    republish(app);
    if STARTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(180)).await;
            refresh(&app).await;
            republish(&app);
        }
    });
}

/// A message from a device of mine that isn't approved: look at the list
/// again (at most every 20 s), so the question shows up.
pub fn untrusted_seen(app: &AppHandle) {
    static LAST: StdMutex<Option<std::time::Instant>> = StdMutex::new(None);
    if let Ok(mut l) = LAST.lock().or_else(|e| Ok::<_, ()>(e.into_inner())) {
        if l.is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(20)) {
            return;
        }
        *l = Some(std::time::Instant::now());
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move { refresh(&app).await });
}

/// `[devok:v1]` / `[devno:v1]` from an approved device of mine.
pub fn on_control(app: &AppHandle, text: &str) {
    let (ok, body) = match (text.strip_prefix(DEVOK_PREFIX), text.strip_prefix(DEVNO_PREFIX)) {
        (Some(b), _) => (true, b),
        (_, Some(b)) => (false, b),
        _ => return,
    };
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    let Some(id) = v["id"].as_str().filter(|i| i.len() == 12 && i.bytes().all(|b| b.is_ascii_hexdigit())) else { return };
    let state: State<'_, StdMutex<AppState>> = app.state();
    if let Ok(s) = state.lock().or_else(|e| Ok::<_, ()>(e.into_inner())) {
        let mut tr = load(&s);
        tr.pending.remove(id);
        if ok {
            tr.ok.insert(id.to_string());
            tr.no.remove(id);
        } else {
            tr.ok.remove(id);
            tr.no.insert(id.to_string());
        }
        save(&s, &tr);
    }
    let _ = app.emit("aqua://device_pending", ());
}

#[derive(Serialize)]
pub struct TrustView {
    pub pending: Vec<Pending>,
    pub trusted: Vec<String>,
    pub this: String,
}

/// Bring my signed device list (devlists.rs) up to date with the devices I
/// approved — in the background, an older relay just says no.
fn republish(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = crate::devlists::publish(&app, false).await {
            tracing::debug!(%e, "device list not published");
        }
    });
}

#[tauri::command]
pub fn device_trust(state: State<'_, StdMutex<AppState>>) -> Result<TrustView, String> {
    let s = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let t = load(&s);
    Ok(TrustView { pending: t.pending.values().cloned().collect(), trusted: t.ok.into_iter().collect(), this: my_id(&s) })
}

/// "It's me": the device gets my chats from now on, and the history key.
#[tauri::command]
pub async fn device_approve(app: AppHandle, id: String) -> Result<(), String> {
    let addr = {
        let state: State<'_, StdMutex<AppState>> = app.state();
        let s = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut tr = load(&s);
        let addr = tr.pending.remove(&id).map(|p| p.addr);
        tr.ok.insert(id.clone());
        tr.no.remove(&id);
        save(&s, &tr);
        addr
    };
    tracing::info!(%id, "device approved");
    crate::multidev::sync_raw(&app, &format!("{DEVOK_PREFIX}{}", serde_json::json!({ "id": id }))).await;
    republish(&app);
    if let Some(addr) = addr {
        crate::history_sync::on_key_request(&app, &addr);
    }
    let _ = app.emit("aqua://device_pending", ());
    Ok(())
}

/// "Not me": sign the device out of the account.
#[tauri::command]
pub async fn device_decline(app: AppHandle, id: String) -> Result<(), String> {
    let (t, me) = {
        let state: State<'_, StdMutex<AppState>> = app.state();
        let s = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        (s.transport.clone().ok_or("not connected")?, s.username.clone().unwrap_or_default())
    };
    t.remove_device(&id).await.map_err(|e| e.to_string())?;
    crate::multidev::invalidate(&me);
    {
        let state: State<'_, StdMutex<AppState>> = app.state();
        let s = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut tr = load(&s);
        tr.pending.remove(&id);
        tr.ok.remove(&id);
        tr.no.insert(id.clone());
        save(&s, &tr);
    }
    tracing::warn!(%id, "device declined and signed out");
    crate::multidev::sync_raw(&app, &format!("{DEVNO_PREFIX}{}", serde_json::json!({ "id": id }))).await;
    republish(&app);
    let _ = app.emit("aqua://device_pending", ());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_devices_work_until_signed_out() {
        let dir = tempfile::tempdir().unwrap();
        let s = AppState::boot(dir.path().to_path_buf()).unwrap().into_inner().unwrap();
        assert!(is_trusted(&s, "aabbccddeeff"));
        let mut t = load(&s);
        t.init = true;
        t.ok.insert("111111111111".into());
        save(&s, &t);
        assert!(is_trusted(&s, "111111111111"));
        assert!(is_trusted(&s, "aabbccddeeff"), "a new device works at once");
        let mut t = load(&s);
        t.no.insert("aabbccddeeff".into());
        save(&s, &t);
        assert!(!is_trusted(&s, "aabbccddeeff"), "not after \"not me\"");
        assert!(is_trusted(&s, &my_id(&s)), "this device always");
    }
}
