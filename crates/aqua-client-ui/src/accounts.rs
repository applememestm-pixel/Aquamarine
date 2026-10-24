//! Several accounts on one device (hold your avatar on the main page to
//! switch). Each account is a whole data directory — vault, history, media —
//! so nothing mixes: the first account is the app's data directory itself,
//! the others live in `accounts/<slot>/` under it, and `accounts.json` there
//! names the active one. The data directory is picked once, at start, so a
//! switch rewrites the pointer and relaunches the app.
//!
//! The UI keeps per-account things in the webview's localStorage too (pins,
//! folders, privacy, …): on a switch it hands that over and gets the other
//! account's copy back (`webstore.json` in each directory).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

/// The app's data directory before any account is picked (the first
/// account's home, and where `accounts/` and the pointer live).
pub struct Base(pub PathBuf);

const POINTER: &str = "accounts.json";
const STORE: &str = "webstore.json";

#[derive(Serialize, Deserialize, Default)]
struct Pointer {
    #[serde(default)]
    active: String,
}

fn valid_slot(s: &str) -> bool {
    s.is_empty() || (s.len() <= 16 && s.chars().all(|c| c.is_ascii_alphanumeric()))
}

fn dir_of(base: &Path, slot: &str) -> PathBuf {
    if slot.is_empty() { base.to_path_buf() } else { base.join("accounts").join(slot) }
}

fn active(base: &Path) -> String {
    let p: Pointer = std::fs::read(base.join(POINTER))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    if valid_slot(&p.active) && dir_of(base, &p.active).is_dir() { p.active } else { String::new() }
}

fn set_active(base: &Path, slot: &str) -> Result<(), String> {
    let bytes = serde_json::to_vec(&Pointer { active: slot.to_string() }).map_err(|e| e.to_string())?;
    std::fs::write(base.join(POINTER), bytes).map_err(|e| e.to_string())
}

/// The data directory of the account the app starts with.
pub fn resolve(base: &Path) -> PathBuf {
    dir_of(base, &active(base))
}

fn slots(base: &Path) -> Vec<String> {
    let mut out = vec![String::new()];
    if let Ok(rd) = std::fs::read_dir(base.join("accounts")) {
        let mut more: Vec<String> = rd
            .flatten()
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|s| !s.is_empty() && valid_slot(s))
            .collect();
        more.sort();
        out.extend(more);
    }
    out
}

fn read_json(path: PathBuf) -> serde_json::Value {
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

#[derive(Serialize)]
pub struct AccountSlot {
    pub slot: String,
    pub registered: bool,
    pub username: Option<String>,
    pub server_url: Option<String>,
    pub active: bool,
}

fn base_of(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app.try_state::<Base>().ok_or("accounts unavailable")?.0.clone())
}

#[tauri::command]
pub fn accounts_list(app: AppHandle) -> Result<Vec<AccountSlot>, String> {
    let base = base_of(&app)?;
    let cur = active(&base);
    Ok(slots(&base)
        .into_iter()
        .map(|slot| {
            let dir = dir_of(&base, &slot);
            let acct = read_json(dir.join("account.json"));
            let conn = read_json(dir.join("connection.json"));
            let registered = acct["registered"].as_bool().unwrap_or(false);
            let username = acct["username"].as_str().or_else(|| conn["username"].as_str()).map(str::to_string);
            AccountSlot {
                active: slot == cur,
                registered,
                username: if registered { username } else { None },
                server_url: conn["server_url"].as_str().map(str::to_string),
                slot,
            }
        })
        .collect())
}

#[derive(Serialize)]
pub struct Switched {
    pub slot: String,
    /// The other account's saved localStorage (JSON), if it has one.
    pub store: Option<String>,
}

/// Make `slot` the account the app starts with ("new" makes a fresh one).
/// `store` is this account's localStorage, kept for when it comes back.
/// Takes effect on `app_relaunch`.
#[tauri::command]
pub fn account_switch(app: AppHandle, slot: String, store: String) -> Result<Switched, String> {
    let base = base_of(&app)?;
    let cur = active(&base);
    let target = if slot == "new" {
        (1..1000).map(|n| format!("a{n}")).find(|s| !dir_of(&base, s).exists()).ok_or("too many accounts")?
    } else {
        slot
    };
    if !valid_slot(&target) {
        return Err("bad account".into());
    }
    let dir = dir_of(&base, &target);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    if serde_json::from_str::<serde_json::Value>(&store).is_ok() {
        let _ = std::fs::write(dir_of(&base, &cur).join(STORE), store.as_bytes());
    }
    let back = std::fs::read_to_string(dir.join(STORE)).ok();
    set_active(&base, &target)?;
    Ok(Switched { slot: target, store: back })
}

/// Drop an account slot that never finished signing in (an "Add account"
/// that was abandoned). Signed-in accounts are removed by logging out first.
#[tauri::command]
pub fn account_forget(app: AppHandle, slot: String) -> Result<(), String> {
    let base = base_of(&app)?;
    if slot.is_empty() || !valid_slot(&slot) || slot == active(&base) {
        return Err("this account can't be removed".into());
    }
    let dir = dir_of(&base, &slot);
    if read_json(dir.join("account.json"))["registered"].as_bool().unwrap_or(false) {
        return Err("log out of this account first".into());
    }
    std::fs::remove_dir_all(dir).map_err(|e| e.to_string())
}

/// Start the app over (the account switch takes effect).
#[tauri::command]
pub fn app_relaunch(app: AppHandle) {
    #[cfg(target_os = "android")]
    {
        let _ = app;
        crate::relay_host::android::relaunch();
    }
    #[cfg(not(target_os = "android"))]
    app.restart();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_and_pointer() {
        let base = std::env::temp_dir().join(format!("aqua-acc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        assert_eq!(resolve(&base), base);
        // A pointer to a missing slot falls back to the first account.
        set_active(&base, "a1").unwrap();
        assert_eq!(resolve(&base), base);
        std::fs::create_dir_all(base.join("accounts/a1")).unwrap();
        assert_eq!(resolve(&base), base.join("accounts/a1"));
        assert_eq!(slots(&base), vec![String::new(), "a1".to_string()]);
        // Nothing outside accounts/ can be named.
        assert!(!valid_slot("../x"));
        let _ = std::fs::remove_dir_all(&base);
    }
}
