//! Several devices per account.
//!
//! Every device has its own identity key, so E2EE sessions exist per
//! *device*, not per account. A device's address on the relay is the plain
//! `user` for the account's primary device and `user~<id>` for the others.
//!
//! * Sending to a person = encrypting the payload once for each of their
//!   devices ([`fanout`]), plus a copy for each of *my* other devices wrapped
//!   in [`SYNC_PREFIX`] so they show what this device sent.
//! * Receiving: the session is looked up by the sender's device address, the
//!   chat by the account ([`base_user`]).
//!
//! Relays that predate multi-device answer 404 to `/v1/devices/…`; then the
//! plain username is the only address, exactly as before.

use std::collections::HashMap;
use std::sync::Mutex as StdMutex;
use std::time::{Duration, Instant};

use aqua_client_core::{DeviceInfo, Transport};
use aqua_core::session::Session;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::state::AppState;

/// A copy of something this device sent, for my other devices:
/// `[sync:v1]{"to":"peer"}\n<payload>`.
pub const SYNC_PREFIX: &str = "[sync:v1]";

const CACHE_TTL: Duration = Duration::from_secs(45);

static CACHE: StdMutex<Option<HashMap<String, (Instant, Vec<String>)>>> = StdMutex::new(None);

/// `alice~a1b2c3` → `alice`.
pub fn base_user(addr: &str) -> &str {
    addr.split('~').next().unwrap_or(addr)
}

/// Short public id of a device key (same as the relay's).
pub fn device_id(dh: &[u8]) -> String {
    hex::encode(&dh[..dh.len().min(6)])
}

/// Forget the cached device list of `user` (a device was added / removed).
pub fn invalidate(user: &str) {
    if let Ok(mut c) = CACHE.lock() {
        if let Some(m) = c.as_mut() {
            m.remove(user);
        }
    }
}

/// Is `addr` a device we don't know yet? (Then the list is refreshed.)
pub fn note_sender(addr: &str) {
    let user = base_user(addr);
    let known = CACHE
        .lock()
        .ok()
        .and_then(|c| c.as_ref().and_then(|m| m.get(user).map(|(_, l)| l.iter().any(|a| a == addr))))
        .unwrap_or(true);
    if !known {
        invalidate(user);
    }
}

/// Every address of `user` (cached for a little while).
pub async fn addresses(transport: &Transport, user: &str) -> Vec<String> {
    if let Ok(c) = CACHE.lock() {
        if let Some((at, list)) = c.as_ref().and_then(|m| m.get(user)) {
            if at.elapsed() < CACHE_TTL {
                return list.clone();
            }
        }
    }
    let list: Vec<String> = match transport.devices(user).await {
        Ok(Some(devs)) if !devs.is_empty() => devs.into_iter().map(|d: DeviceInfo| d.addr).collect(),
        // Old relay, unknown user (bots), or a network hiccup: the username.
        _ => vec![user.to_string()],
    };
    if let Ok(mut c) = CACHE.lock() {
        c.get_or_insert_with(HashMap::new).insert(user.to_string(), (Instant::now(), list.clone()));
    }
    list
}

/// This device's own address.
pub fn my_address(s: &AppState, devices_of_me: &[String]) -> String {
    let me = s.username.clone().unwrap_or_default();
    let mine = format!("{me}~{}", device_id(s.identity.public().dh_pub.as_bytes()));
    if devices_of_me.iter().any(|a| a == &mine) { mine } else { me }
}

/// Encrypt `payload` for one device address and send it, starting an X3DH
/// session with that device first if there is none.
pub async fn send_to_address(app: &AppHandle, addr: &str, payload: &str) -> Result<(), String> {
    let state: State<'_, StdMutex<AppState>> = app.state();
    let fast = {
        let mut s = state.lock().map_err(|e| e.to_string())?;
        if let Some(session) = s.sessions.get_mut(addr) {
            let env = session.encrypt(payload.as_bytes()).map_err(|e| format!("encrypt: {e}"))?;
            if let Some(session) = s.sessions.get(addr) {
                let _ = s.vault.save_session(addr, session);
            }
            Some((s.transport.clone(), env))
        } else {
            None
        }
    };
    if let Some((transport, env)) = fast {
        // No relay right now: the envelope waits in the nearby-delivery
        // outbox (Bluetooth, or the relay once it's back) — see mesh.rs.
        let sent = transport.map(|t| t.send_envelope(addr, &env).is_ok()).unwrap_or(false);
        if !sent {
            if !worth_keeping(payload) {
                return Err("not connected".into());
            }
            crate::mesh::queue(app, addr, &env);
        }
        return Ok(());
    }
    let (transport, identity) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        (s.transport.clone().ok_or("not connected")?, s.identity.clone())
    };
    let bundle = transport.fetch_bundle(addr).await.map_err(|e| format!("fetch_bundle: {e}"))?;
    bundle.verify().map_err(|e| format!("bad bundle: {e}"))?;
    let (session, initial) = Session::start_as_initiator(&identity, &bundle, payload.as_bytes())
        .map_err(|e| format!("start_as_initiator: {e}"))?;
    transport.send_initial(addr, &initial).map_err(|e| format!("send_initial: {e}"))?;
    let mut s = state.lock().map_err(|e| e.to_string())?;
    let _ = s.vault.save_session(addr, &session);
    s.sessions.insert(addr.to_string(), session);
    Ok(())
}

/// Typing, presence, call signalling and handoff only matter right now;
/// they aren't kept for later delivery.
fn worth_keeping(payload: &str) -> bool {
    use crate::commands::{CALL_PREFIX, CTL_PREFIX, GCALL_PREFIX, STATUS_PREFIX, TYPING_PREFIX};
    let inner = unwrap_sync(payload).map(|(_, p)| p).unwrap_or(payload);
    ![CALL_PREFIX, TYPING_PREFIX, STATUS_PREFIX, GCALL_PREFIX, CTL_PREFIX].iter().any(|p| inner.starts_with(p))
}

/// Device addresses of `user` we can reach without the relay: the last
/// known device list, else every device we have a session with.
fn offline_addresses(app: &AppHandle, user: &str) -> Vec<String> {
    if let Ok(c) = CACHE.lock() {
        if let Some((_, list)) = c.as_ref().and_then(|m| m.get(user)) {
            return list.clone();
        }
    }
    let state: State<'_, StdMutex<AppState>> = app.state();
    let Ok(s) = state.lock() else { return Vec::new() };
    let mut v: Vec<String> = s.sessions.keys().filter(|a| base_user(a) == user).cloned().collect();
    v.sort();
    v
}

/// Send `payload` to every device of `peer`. With `sync`, my other devices
/// get a copy too (so they show what I sent). Succeeds when at least one of
/// the peer's devices got it.
pub async fn fanout(app: &AppHandle, peer: &str, payload: &str, sync: bool) -> Result<(), String> {
    let state: State<'_, StdMutex<AppState>> = app.state();
    let (transport, me) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        (s.transport.clone(), s.username.clone().unwrap_or_default())
    };
    let peer = base_user(peer).to_string();
    let Some(transport) = transport else {
        return fanout_offline(app, &peer, &me, payload, sync).await;
    };
    let mut first_err: Option<String> = None;
    let mut ok = 0usize;
    if peer != me {
        for addr in addresses(&transport, &peer).await {
            match send_to_address(app, &addr, payload).await {
                Ok(()) => ok += 1,
                Err(e) => {
                    tracing::warn!(%addr, %e, "fanout: device skipped");
                    first_err.get_or_insert(e);
                }
            }
        }
    }
    if sync || peer == me {
        let copy = if peer == me { payload.to_string() } else { wrap_sync(&peer, payload) };
        to_my_devices(app, &transport, &me, &copy).await;
        if peer == me {
            return Ok(());
        }
    }
    if ok == 0 {
        // A device that disappeared: refresh the list next time.
        invalidate(&peer);
        return Err(first_err.unwrap_or_else(|| "no device to send to".into()));
    }
    Ok(())
}

/// [`fanout`] with no relay: encrypt for every device we have a session
/// with and leave the envelopes in the nearby-delivery outbox.
async fn fanout_offline(app: &AppHandle, peer: &str, me: &str, payload: &str, sync: bool) -> Result<(), String> {
    if !worth_keeping(payload) {
        return Err("not connected to a server".into());
    }
    let mut ok = 0usize;
    if peer != me {
        for addr in offline_addresses(app, peer) {
            if send_to_address(app, &addr, payload).await.is_ok() {
                ok += 1;
            }
        }
    }
    if (sync || peer == me) && !me.is_empty() {
        let copy = if peer == me { payload.to_string() } else { wrap_sync(peer, payload) };
        for addr in offline_addresses(app, me) {
            let _ = send_to_address(app, &addr, &copy).await;
        }
        if peer == me {
            return Ok(());
        }
    }
    if ok == 0 {
        return Err("not connected to a server".into());
    }
    Ok(())
}

/// Send a raw payload to my other devices (group traffic, sync copies).
pub async fn to_my_devices(app: &AppHandle, transport: &Transport, me: &str, payload: &str) {
    if me.is_empty() {
        return;
    }
    let mine = addresses(transport, me).await;
    let my_addr = {
        let state: State<'_, StdMutex<AppState>> = app.state();
        let Ok(s) = state.lock() else { return };
        my_address(&s, &mine)
    };
    for addr in mine.iter().filter(|a| **a != my_addr) {
        if let Err(e) = send_to_address(app, addr, payload).await {
            tracing::debug!(%addr, %e, "sync to own device skipped");
        }
    }
}

/// [`to_my_devices`] for callers that only have the app handle.
pub async fn sync_raw(app: &AppHandle, payload: &str) {
    let state: State<'_, StdMutex<AppState>> = app.state();
    let (transport, me) = {
        let Ok(s) = state.lock() else { return };
        (s.transport.clone(), s.username.clone().unwrap_or_default())
    };
    if let Some(t) = transport {
        to_my_devices(app, &t, &me, payload).await;
    }
}

pub fn wrap_sync(to: &str, payload: &str) -> String {
    format!("{SYNC_PREFIX}{}\n{payload}", serde_json::json!({ "to": to }))
}

/// `[sync:v1]{"to":…}\n<payload>` → (to, payload).
pub fn unwrap_sync(text: &str) -> Option<(String, &str)> {
    let rest = text.strip_prefix(SYNC_PREFIX)?;
    let (head, body) = rest.split_once('\n')?;
    let v: serde_json::Value = serde_json::from_str(head).ok()?;
    let to = v.get("to")?.as_str()?.to_string();
    Some((to, body))
}

/// Seamless handoff: tell my other devices which chat I'm in and what I was
/// typing, so they can pick it up ("Continue here"). Only my own devices
/// get it, end-to-end encrypted like everything else.
#[tauri::command]
pub async fn handoff(app: AppHandle, chat: String, draft: String) -> Result<(), String> {
    if chat.is_empty() || chat.len() > 80 {
        return Err("bad chat".into());
    }
    let draft: String = draft.chars().take(4000).collect();
    let body = format!(
        "{}{}",
        crate::commands::CTL_PREFIX,
        serde_json::json!({ "t": "handoff", "draft": draft, "dev": this_device_name(), "at": chrono_ms() })
    );
    sync_raw(&app, &wrap_sync(&chat, &body)).await;
    Ok(())
}

fn chrono_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

// ---------- device management (Settings → Devices) ------------------------------

#[tauri::command]
pub async fn list_devices(state: State<'_, StdMutex<AppState>>) -> Result<Vec<DeviceInfo>, String> {
    let t = { state.lock().map_err(|e| e.to_string())?.transport.clone() }.ok_or("not connected")?;
    t.my_devices().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn remove_device(state: State<'_, StdMutex<AppState>>, id: String) -> Result<(), String> {
    let (t, me) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        (s.transport.clone().ok_or("not connected")?, s.username.clone().unwrap_or_default())
    };
    t.remove_device(&id).await.map_err(|e| e.to_string())?;
    invalidate(&me);
    Ok(())
}

#[tauri::command]
pub async fn rename_device(state: State<'_, StdMutex<AppState>>, id: String, name: String) -> Result<(), String> {
    let t = { state.lock().map_err(|e| e.to_string())?.transport.clone() }.ok_or("not connected")?;
    t.rename_device(&id, &name).await.map_err(|e| e.to_string())
}

/// A readable name for this device in the account's list.
pub fn this_device_name() -> String {
    let host = std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .or_else(|| std::env::var("HOSTNAME").ok());
    let os = match std::env::consts::OS {
        "linux" => "Linux",
        "windows" => "Windows",
        "macos" => "macOS",
        "android" => "Android",
        "ios" => "iOS",
        o => o,
    };
    match host {
        Some(h) if !cfg!(any(target_os = "android", target_os = "ios")) => format!("{h} ({os})"),
        _ => os.to_string(),
    }
}

/// Things my other devices sent: show them here as mine.
pub fn handle_own_device(app: &AppHandle, text: &str) {
    use crate::commands::*;
    if let Some((to, inner)) = unwrap_sync(text) {
        if let Some(sig) = inner.strip_prefix(CALL_PREFIX) {
            // Answered / declined on another device: stop ringing here.
            let t = serde_json::from_str::<serde_json::Value>(sig)
                .ok()
                .and_then(|v| v.get("type").and_then(|x| x.as_str()).map(str::to_string))
                .unwrap_or_default();
            if t == "answer" || t == "reject" {
                let _ = app.emit("aqua://call_signal", CallSignalEvent {
                    from: to,
                    signal: serde_json::json!({ "type": "hangup", "elsewhere": true }),
                });
            }
            return;
        }
        own_device_message(app, &to, inner);
        return;
    }
    // Group traffic of mine (snapshots, messages) arrives unwrapped.
    let me = {
        let state: State<'_, StdMutex<AppState>> = app.state();
        state.lock().ok().and_then(|s| s.username.clone()).unwrap_or_default()
    };
    route_group_payload_pub(app, &me, text);
}
