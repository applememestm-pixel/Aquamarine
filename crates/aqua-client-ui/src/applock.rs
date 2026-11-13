//! The app's passcode, kept in the vault — not in the webview's storage.
//!
//! It used to live in localStorage only: deleting the webview's data folder
//! (EBWebView on Windows) removed the passcode, and the app opened every
//! chat. Now the vault holds it (encrypted at rest with the device key), the
//! app starts locked when one is set, and while it's locked every command of
//! the app answers "locked" (lib.rs wraps the invoke handler) except the
//! ones here — so a webview without the passcode, or with the lock screen
//! taken off, still gets nothing.
//!
//! The passcode itself never reaches this side: the UI derives a PBKDF2 hash
//! (lock.ts) with the salt kept here and sends that.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex as StdMutex;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::state::AppState;

const KEY: &str = "lock:conf";
static LOCKED: AtomicBool = AtomicBool::new(false);
/// Wrong tries in a row, and when the next try is allowed (unix ms).
static FAILS: StdMutex<(u32, u64)> = StdMutex::new((0, 0));

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Conf {
    pub hash: String,
    pub salt: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub auto_min: u32,
    #[serde(default)]
    pub bio: bool,
}

#[derive(Serialize)]
pub struct LockState {
    has: bool,
    locked: bool,
    salt: String,
    kind: String,
    auto_min: u32,
    bio: bool,
    /// Wait this long (ms) before the next try.
    wait_ms: u64,
}

fn conf_of(s: &AppState) -> Option<Conf> {
    s.vault
        .get_record(KEY)
        .ok()
        .flatten()
        .and_then(|b| serde_json::from_slice::<Conf>(&b).ok())
        .filter(|c| !c.hash.is_empty())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// At start: locked when a passcode is set.
pub fn init(s: &AppState) {
    LOCKED.store(conf_of(s).is_some(), Ordering::SeqCst);
}

pub fn is_locked() -> bool {
    LOCKED.load(Ordering::SeqCst)
}

/// What the app may still do while locked: the lock itself, and crash reports.
pub fn allowed(cmd: &str) -> bool {
    cmd.starts_with("applock_") || matches!(cmd, "report_error" | "report_config")
}

/// Compare without stopping at the first difference.
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

fn hex_ok(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|b| b.is_ascii_hexdigit())
}

#[tauri::command]
pub fn applock_state(state: State<'_, StdMutex<AppState>>) -> Result<LockState, String> {
    let s = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let c = conf_of(&s);
    let wait_ms = FAILS.lock().or_else(|e| Ok::<_, ()>(e.into_inner())).map(|f| f.1.saturating_sub(now_ms())).unwrap_or(0);
    Ok(match c {
        Some(c) => LockState { has: true, locked: is_locked(), salt: c.salt, kind: c.kind, auto_min: c.auto_min, bio: c.bio, wait_ms },
        None => LockState { has: false, locked: false, salt: String::new(), kind: String::new(), auto_min: 0, bio: false, wait_ms: 0 },
    })
}

/// Set (or change) the passcode — only while unlocked.
#[tauri::command]
pub fn applock_set(state: State<'_, StdMutex<AppState>>, hash: String, salt: String, kind: String, auto_min: u32) -> Result<(), String> {
    if is_locked() {
        return Err("locked".into());
    }
    if !hex_ok(&hash, 64) || !hex_ok(&salt, 32) {
        return Err("bad passcode hash".into());
    }
    let s = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let bio = conf_of(&s).map(|c| c.bio).unwrap_or(false);
    let c = Conf { hash: hash.to_lowercase(), salt: salt.to_lowercase(), kind, auto_min, bio };
    s.vault.put_record(KEY, &serde_json::to_vec(&c).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

/// Auto-lock minutes and fingerprint / face — only while unlocked.
#[tauri::command]
pub fn applock_options(state: State<'_, StdMutex<AppState>>, auto_min: Option<u32>, bio: Option<bool>) -> Result<(), String> {
    if is_locked() {
        return Err("locked".into());
    }
    let s = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(mut c) = conf_of(&s) else { return Ok(()) };
    if let Some(m) = auto_min {
        c.auto_min = m;
    }
    if let Some(b) = bio {
        c.bio = b;
    }
    s.vault.put_record(KEY, &serde_json::to_vec(&c).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

/// Remove the passcode — only while unlocked.
#[tauri::command]
pub fn applock_clear(state: State<'_, StdMutex<AppState>>) -> Result<(), String> {
    if is_locked() {
        return Err("locked".into());
    }
    let s = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    s.vault.delete_record(KEY).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn applock_lock(state: State<'_, StdMutex<AppState>>) -> Result<(), String> {
    let s = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if conf_of(&s).is_some() {
        LOCKED.store(true, Ordering::SeqCst);
    }
    Ok(())
}

/// The passcode's hash (lock.ts derives it): right → unlocked. Wrong tries
/// in a row make the next one wait: 5 free, then 30 s, doubling.
#[tauri::command]
pub fn applock_unlock(state: State<'_, StdMutex<AppState>>, hash: String) -> Result<bool, String> {
    let s = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(c) = conf_of(&s) else {
        LOCKED.store(false, Ordering::SeqCst);
        return Ok(true);
    };
    let mut f = FAILS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if now_ms() < f.1 {
        return Err(format!("wait:{}", f.1 - now_ms()));
    }
    let ok = ct_eq(hash.to_lowercase().as_bytes(), c.hash.as_bytes());
    if ok {
        *f = (0, 0);
        LOCKED.store(false, Ordering::SeqCst);
    } else {
        f.0 += 1;
        if f.0 >= 5 {
            let secs = 30u64 << (f.0 - 5).min(8);
            f.1 = now_ms() + secs * 1000;
        }
    }
    Ok(ok)
}

/// Fingerprint / face (phones; the UI asked the system first). Only when
/// it's turned on for this passcode.
#[tauri::command]
pub fn applock_unlock_bio(state: State<'_, StdMutex<AppState>>) -> Result<bool, String> {
    #[cfg(mobile)]
    {
        let s = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if conf_of(&s).is_some_and(|c| c.bio) {
            LOCKED.store(false, Ordering::SeqCst);
            return Ok(true);
        }
    }
    #[cfg(not(mobile))]
    let _ = state;
    Ok(false)
}

#[derive(Serialize)]
pub struct ForgotSent {
    /// Where the code went, half hidden (`v•••@mail.ru`).
    sent_to: String,
}

fn account_of(state: &State<'_, StdMutex<AppState>>) -> Result<(String, String), String> {
    let s = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    match (s.server_url.clone(), s.email.clone()) {
        (Some(u), Some(e)) if !u.is_empty() && !e.is_empty() => Ok((u, e)),
        _ => Err("no e-mail is linked to this account here".into()),
    }
}

/// Forgot the passcode: a code to the account's e-mail. (A relay without
/// mail that hands codes back gets no say here — that would open the lock
/// for anyone.)
#[tauri::command]
pub async fn applock_forgot_send(state: State<'_, StdMutex<AppState>>) -> Result<ForgotSent, String> {
    let (server, email) = account_of(&state)?;
    let r = aqua_client_core::transport::request_email_code_invite(&server, &email, "signin", "", "").await.map_err(|e| e.to_string())?;
    if r.dev_code.is_some() || r.delivery_failed {
        return Err("this server can't send e-mail".into());
    }
    let sent_to = match email.split_once('@') {
        Some((n, d)) => format!("{}•••@{d}", n.chars().next().unwrap_or('•')),
        None => "•••".into(),
    };
    Ok(ForgotSent { sent_to })
}

/// The code from that e-mail: right → the passcode is removed and the app
/// opens (a new one can be set in Settings).
#[tauri::command]
pub async fn applock_forgot_check(state: State<'_, StdMutex<AppState>>, code: String) -> Result<bool, String> {
    let (server, email) = account_of(&state)?;
    if aqua_client_core::verify_email_code(&server, &email, code.trim()).await.is_err() {
        return Ok(false);
    }
    let s = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    s.vault.delete_record(KEY).map_err(|e| e.to_string())?;
    if let Ok(mut f) = FAILS.lock().or_else(|e| Ok::<_, ()>(e.into_inner())) {
        *f = (0, 0);
    }
    LOCKED.store(false, Ordering::SeqCst);
    Ok(true)
}
