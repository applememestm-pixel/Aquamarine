//! Signed device lists of my contacts and of my own account (1.2.0-5,
//! `aqua_core::devlist`).
//!
//! * Sending: once a contact has a signed list, this device encrypts only
//!   to devices on it ([`allowed`], checked against the key of the session
//!   or of the bundle a session starts with — see `multidev::send_inner`).
//!   Since 1.20.1-1 a device the relay lists for the account counts too
//!   (`multidev::device_ok`), like Telegram: a new phone works at once, and
//!   the account's own devices put it on the list by themselves.
//! * Receiving: a message from a device that is neither on the sender's
//!   signed list (after one refresh) nor signed in to their account is dropped.
//! * A new list replaces the accepted one only if a device of the accepted
//!   list signed it. A reset (all the old devices gone) is taken, and the
//!   UI warns, like a changed safety code. Anything else is refused and the
//!   old list stays.
//! * My own list: published by a device that is on the current list (or by
//!   anyone if there is none yet) whenever my approved devices differ from
//!   it ([`publish`]). A device is only dropped from the list once the relay
//!   no longer has it (signed out), so two of my devices that haven't
//!   heard about an approval yet don't take each other's devices away.
//!
//! Accepted lists: `dev-lists.json` in the account folder.

use std::collections::HashMap;
use std::sync::Mutex as StdMutex;
use std::time::{Duration, Instant};

use aqua_client_core::Transport;
use aqua_core::devlist::{Accepted, DeviceKey, SignedDeviceList};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::state::AppState;

const FILE: &str = "dev-lists.json";
const FRESH: Duration = Duration::from_secs(60);
/// List versions from here on are unix seconds (see [`publish`]).
const TIME_VER: u64 = 1_000_000_000;

#[derive(Default, Serialize, Deserialize)]
struct Book {
    #[serde(default)]
    lists: HashMap<String, SignedDeviceList>,
    /// Users whose list was reset (the UI shows a warning until it's seen).
    #[serde(default)]
    resets: HashMap<String, i64>,
}

static BOOK: StdMutex<Option<(std::path::PathBuf, Book)>> = StdMutex::new(None);
static CHECKED: StdMutex<Option<HashMap<String, Instant>>> = StdMutex::new(None);

fn with_book<R>(app: &AppHandle, f: impl FnOnce(&mut Book) -> R) -> Option<R> {
    let dir = {
        let state: State<'_, StdMutex<AppState>> = app.state();
        let s = state.lock().ok()?;
        s.data_dir.clone()
    };
    let mut g = BOOK.lock().ok()?;
    if g.as_ref().is_none_or(|(d, _)| *d != dir) {
        let b: Book = std::fs::read(dir.join(FILE)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        *g = Some((dir, b));
    }
    let (dir, book) = g.as_mut()?;
    let r = f(book);
    if let Ok(b) = serde_json::to_vec(book) {
        let _ = std::fs::write(dir.join(FILE), b);
    }
    Some(r)
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// The accepted list of `user`, if any.
pub fn accepted(app: &AppHandle, user: &str) -> Option<SignedDeviceList> {
    with_book(app, |b| b.lists.get(user).cloned()).flatten()
}

/// May I encrypt to / take messages from this device key of `user`?
/// (No signed list yet — an older app on their side: yes, as before.)
pub fn allowed(app: &AppHandle, user: &str, dh: &[u8]) -> bool {
    accepted(app, user).is_none_or(|l| l.has_dh(dh))
}

/// Look at `user`'s list on the relay (at most once a minute unless
/// `force`) and take it if it chains from the accepted one.
pub async fn refresh(app: &AppHandle, transport: &Transport, user: &str, force: bool) {
    if user.is_empty() || user.starts_with("__") || user.starts_with("grp_") {
        return;
    }
    if !force {
        if let Ok(mut c) = CHECKED.lock() {
            let m = c.get_or_insert_with(HashMap::new);
            if m.get(user).is_some_and(|t| t.elapsed() < FRESH) {
                return;
            }
            m.insert(user.to_string(), Instant::now());
        }
    }
    let Ok(Some(json)) = transport.get_devlist(user).await else { return };
    let Ok(list) = serde_json::from_str::<SignedDeviceList>(&json) else { return };
    let prev = accepted(app, user);
    if prev.as_ref() == Some(&list) {
        return;
    }
    // An account registered again under the same name (other keys, the
    // count starting over) or a list whose earlier versions I never saw:
    // signed later than the one I hold, by a device that one doesn't have.
    // The old devices can't vouch for it — take it like a reset, with the
    // warning, instead of ignoring the person for good. (Whoever could forge
    // this could set the reset flag as well; an old copy replayed by the
    // relay is signed earlier and stays refused.)
    let verdict = match list.accept(user, prev.as_ref()) {
        Err(aqua_core::devlist::ListError::Stale) | Err(aqua_core::devlist::ListError::UnknownSigner)
            if prev.as_ref().is_some_and(|p| list.at > p.at && !p.devices.contains(&list.signer)) => Ok(Accepted::Reset),
        other => other,
    };
    match verdict {
        Ok(how) => {
            with_book(app, |b| {
                b.lists.insert(user.to_string(), list.clone());
                if how == Accepted::Reset {
                    b.resets.insert(user.to_string(), now());
                }
            });
            if how == Accepted::Reset {
                tracing::warn!(%user, "device list reset: all the old devices are gone");
                let _ = app.emit("aqua://devlist_reset", serde_json::json!({ "user": user }));
            }
            crate::multidev::invalidate(user);
        }
        Err(aqua_core::devlist::ListError::Stale) => {}
        // Still signed under their old @username (they renamed and their app
        // hasn't re-signed it yet): nothing to warn about, it just doesn't count.
        Err(aqua_core::devlist::ListError::WrongUser) => {
            tracing::debug!(%user, "device list still under an old name — ignored");
        }
        Err(e) => {
            tracing::warn!(%user, %e, "a device list was refused — the accepted one stays");
            let _ = app.emit("aqua://devlist_refused", serde_json::json!({ "user": user, "why": e.to_string() }));
        }
    }
}

/// Users whose device list was reset and who weren't looked at yet.
#[tauri::command]
pub fn devlist_resets(app: AppHandle) -> Vec<String> {
    with_book(&app, |b| b.resets.keys().cloned().collect()).unwrap_or_default()
}

/// The warning about `user` was seen.
#[tauri::command]
pub fn devlist_reset_seen(app: AppHandle, user: String) {
    with_book(&app, |b| { b.resets.remove(&user); });
}

/// The keys of my own devices I can name without asking anybody: this one,
/// the current list, and my sessions with my other devices.
fn known_keys(s: &AppState, me: &str, cur: Option<&SignedDeviceList>) -> HashMap<String, DeviceKey> {
    let mut out: HashMap<String, DeviceKey> = HashMap::new();
    let mine = DeviceKey::of(&s.identity.public());
    out.insert(mine.id(), mine);
    if let Some(l) = cur {
        for d in &l.devices {
            out.entry(d.id()).or_insert_with(|| d.clone());
        }
    }
    for (addr, sess) in &s.sessions {
        if crate::multidev::base_user(addr) == me {
            let k = DeviceKey::of(&sess.peer);
            out.entry(k.id()).or_insert(k);
        }
    }
    out
}

/// Publish my account's list if my approved devices aren't what it says.
/// `reset`: start over (every old device is lost) — the user asked.
pub async fn publish(app: &AppHandle, reset: bool) -> Result<bool, String> {
    let state: State<'_, StdMutex<AppState>> = app.state();
    let (t, me, identity) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        (s.transport.clone().ok_or("not connected")?, s.username.clone().unwrap_or_default(), s.identity.clone())
    };
    if me.is_empty() {
        return Ok(false);
    }
    let cur: Option<SignedDeviceList> = match t.get_devlist(&me).await {
        Ok(Some(j)) => serde_json::from_str::<SignedDeviceList>(&j).ok().filter(|l| l.verify_self().is_ok() && l.user == me),
        Ok(None) => None,
        Err(e) => return Err(e.to_string()),
    };
    let mine = DeviceKey::of(&identity.public());
    let devs = t.my_devices().await.map_err(|e| e.to_string())?;
    let on_relay: std::collections::HashSet<String> = devs.iter().map(|d| d.id.clone()).collect();
    let mut reset = reset;
    if !reset && cur.as_ref().is_some_and(|c| !c.devices.contains(&mine)) {
        // Not on the list. If a listed device is still signed in, it adds me
        // ("It's me"). If none is — the account was registered again, or
        // every old device signed out — nobody ever will, and people would
        // go on encrypting to devices that are gone: start the list over.
        let alive = cur.as_ref().is_some_and(|c| c.devices.iter().any(|d| d.id() != mine.id() && on_relay.contains(&d.id())));
        if alive {
            return Ok(false);
        }
        tracing::warn!("device list: none of its devices is signed in any more — starting it over");
        reset = true;
    }
    // Versions used to count 1, 2, 3…: an account registered again under
    // the same name began at 1 once more, and everyone who knew the old list
    // took the new one for an old copy. A version is now the time of signing;
    // a list still on the old count is signed again once.
    let legacy = cur.as_ref().is_some_and(|c| c.ver < TIME_VER);
    let (known, wanted_ids) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        let known = known_keys(&s, &me, cur.as_ref());
        let mut ids: Vec<String> = devs.iter().map(|d| d.id.clone()).filter(|id| crate::devtrust::is_trusted(&s, id)).collect();
        if !reset {
            if let Some(c) = &cur {
                ids.extend(c.devices.iter().map(|d| d.id()).filter(|id| on_relay.contains(id)));
            }
        }
        ids.push(mine.id());
        ids.sort();
        ids.dedup();
        (known, ids)
    };
    let mut keys: Vec<DeviceKey> = Vec::new();
    for id in &wanted_ids {
        if let Some(k) = known.get(id) {
            keys.push(k.clone());
            continue;
        }
        // An approved device I have no session with yet: its bundle.
        let Some(addr) = devs.iter().find(|d| &d.id == id).map(|d| d.addr.clone()) else { continue };
        match t.fetch_bundle(&addr).await {
            Ok(b) if b.verify().is_ok() && crate::multidev::device_id(b.identity.dh_pub.as_bytes()) == *id => keys.push(DeviceKey::of(&b.identity)),
            _ => tracing::warn!(%id, "device list: no key for an approved device yet"),
        }
    }
    keys.sort();
    keys.dedup();
    if !reset {
        if let Some(c) = &cur {
            let mut have = c.devices.clone();
            have.sort();
            if have == keys && !legacy {
                return Ok(false);
            }
        }
    }
    let ver = cur.as_ref().map(|c| c.ver + 1).unwrap_or(1).max(now().max(0) as u64);
    // "reset" on the first list and on the one-time re-signing too: whoever
    // still holds a list this device isn't on takes it (with a warning)
    // instead of refusing it for good; everyone else sees no difference.
    let list = SignedDeviceList::sign(&identity, &me, ver, now(), reset || cur.is_none() || legacy, keys);
    let json = serde_json::to_string(&list).map_err(|e| e.to_string())?;
    t.put_devlist(json).await.map_err(|e| e.to_string())?;
    with_book(app, |b| { b.lists.insert(me.clone(), list.clone()); });
    tracing::info!(ver, devices = list.devices.len(), reset, "device list published");
    Ok(true)
}

/// Settings → Devices: "My old devices are lost — start the list over".
#[tauri::command]
pub async fn devlist_reset(app: AppHandle) -> Result<(), String> {
    publish(&app, true).await.map(|_| ())
}

/// What the Devices screen shows: is my list published, how many devices.
#[tauri::command]
pub async fn devlist_status(app: AppHandle) -> Result<serde_json::Value, String> {
    let state: State<'_, StdMutex<AppState>> = app.state();
    let (t, me, mine) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        (s.transport.clone().ok_or("not connected")?, s.username.clone().unwrap_or_default(), DeviceKey::of(&s.identity.public()))
    };
    let cur = t.get_devlist(&me).await.map_err(|e| e.to_string())?
        .and_then(|j| serde_json::from_str::<SignedDeviceList>(&j).ok());
    Ok(serde_json::json!({
        "published": cur.is_some(),
        "ver": cur.as_ref().map(|c| c.ver),
        "devices": cur.as_ref().map(|c| c.devices.len()).unwrap_or(0),
        "this_listed": cur.as_ref().is_some_and(|c| c.devices.contains(&mine)),
    }))
}
