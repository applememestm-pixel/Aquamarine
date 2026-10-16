//! Encrypted account backups (Settings → Backups, and restore on sign-in).
//!
//! What goes into a backup: the whole vault — identity keys, sessions,
//! prekeys, chat history, groups — plus the UI's settings (theme,
//! contacts, folders…). Sealed on the device by `aqua_client_core::backup`;
//! the relay only stores the ciphertext. Media files are not included.

use std::sync::Mutex as StdMutex;

use aqua_client_core::{backup, vault::VaultDump, Transport};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::state::AppState;

const KEYS: &str = "backup:keys";
const PHRASE: &str = "backup:phrase";
const LAST: &str = "backup:last";
/// SHA-256 of the last uploaded contents (skip identical uploads).
const HASH: &str = "backup:hash";
/// Automatic backups at most this often.
const AUTO_EVERY_SECS: u64 = 6 * 3600;

#[derive(Serialize, Deserialize)]
struct Payload {
    v: u32,
    created: u64,
    username: Option<String>,
    email: Option<String>,
    vault: VaultDump,
    /// The UI's own settings (localStorage), as JSON.
    ui: String,
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct Last {
    pub at: u64,
    pub size: u64,
}

#[derive(Serialize)]
pub struct BackupStatus {
    pub enabled: bool,
    pub last: Option<Last>,
    pub has_phrase: bool,
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn keys_of(s: &AppState) -> Option<backup::Keys> {
    s.vault.get_record(KEYS).ok().flatten().and_then(|b| serde_json::from_slice(&b).ok())
}

fn last_of(s: &AppState) -> Option<Last> {
    s.vault.get_record(LAST).ok().flatten().and_then(|b| serde_json::from_slice(&b).ok())
}

fn status_of(s: &AppState) -> BackupStatus {
    BackupStatus {
        enabled: keys_of(s).is_some(),
        last: last_of(s),
        has_phrase: s.vault.get_record(PHRASE).ok().flatten().is_some(),
    }
}

#[tauri::command]
pub fn backup_status(state: State<'_, StdMutex<AppState>>) -> Result<BackupStatus, String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    Ok(status_of(&s))
}

/// Turn backups on with the account password. Returns the 24-word
/// recovery key (also kept in the vault, so Settings can show it again).
#[tauri::command]
pub async fn backup_enable(state: State<'_, StdMutex<AppState>>, password: String) -> Result<String, String> {
    if password.chars().count() < 8 {
        return Err("password must be at least 8 characters".into());
    }
    // Argon2id with 64 MiB: off the async runtime.
    let (keys, phrase) = tokio::task::spawn_blocking(move || backup::new_keys(&password)).await.map_err(|e| e.to_string())?;
    let s = state.lock().map_err(|e| e.to_string())?;
    s.vault.put_record(KEYS, &serde_json::to_vec(&keys).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    s.vault.put_record(PHRASE, phrase.as_bytes()).map_err(|e| e.to_string())?;
    let _ = s.vault.delete_record(LAST);
    let _ = s.vault.delete_record(HASH);
    Ok(phrase)
}

#[tauri::command]
pub fn backup_phrase(state: State<'_, StdMutex<AppState>>) -> Result<Option<String>, String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    Ok(s.vault.get_record(PHRASE).ok().flatten().map(|b| String::from_utf8_lossy(&b).to_string()))
}

/// Make and upload a backup. Without `force`, only if the last one is
/// older than six hours (the UI calls this periodically).
#[tauri::command]
pub async fn backup_now(state: State<'_, StdMutex<AppState>>, ui: String, force: bool) -> Result<BackupStatus, String> {
    // Only the vault snapshot is taken under the app lock; serialising,
    // compressing and sealing (the slow part) run on a worker thread so the
    // UI and message handling never wait for a backup.
    let (keys, payload, transport, prev) = {
        let mut s = state.lock().map_err(|e| e.to_string())?;
        let Some(keys) = keys_of(&s) else { return Err("backups are off".into()) };
        if !force && last_of(&s).is_some_and(|l| now().saturating_sub(l.at) < AUTO_EVERY_SECS) {
            return Ok(status_of(&s));
        }
        let Some(t) = s.transport.clone() else { return Err("not connected".into()) };
        crate::history::flush(&mut s);
        let payload = Payload {
            v: 1,
            created: 0,
            username: s.username.clone(),
            email: s.email.clone(),
            vault: s.vault.export_dump(&["backup:"]).map_err(|e| e.to_string())?,
            ui,
        };
        let prev = s.vault.get_record(HASH).ok().flatten().unwrap_or_default();
        (keys, payload, t, prev)
    };
    let sealed = tokio::task::spawn_blocking(move || -> Result<Option<(Vec<u8>, Vec<u8>)>, String> {
        // Nothing changed since the last upload: don't send the same copy
        // again (and don't make every relay of the network copy it).
        let bytes = bincode::serialize(&payload).map_err(|e| e.to_string())?;
        let hash = <sha2::Sha256 as sha2::Digest>::digest(&bytes).to_vec();
        if hash == prev {
            return Ok(None);
        }
        let mut payload = payload;
        payload.created = now();
        let bytes = bincode::serialize(&payload).map_err(|e| e.to_string())?;
        Ok(Some((backup::seal(&keys, &bytes), hash)))
    })
    .await
    .map_err(|e| e.to_string())??;
    let Some((blob, hash)) = sealed else {
        let s = state.lock().map_err(|e| e.to_string())?;
        let size = last_of(&s).map(|l| l.size).unwrap_or(0);
        let _ = s.vault.put_record(LAST, &serde_json::to_vec(&Last { at: now(), size }).unwrap_or_default());
        return Ok(status_of(&s));
    };
    let size = blob.len() as u64;
    transport.put_backup(blob).await.map_err(|e| e.to_string())?;
    let s = state.lock().map_err(|e| e.to_string())?;
    let _ = s.vault.put_record(LAST, &serde_json::to_vec(&Last { at: now(), size }).unwrap_or_default());
    let _ = s.vault.put_record(HASH, &hash);
    Ok(status_of(&s))
}

/// Turn backups off and delete the copy on the server.
#[tauri::command]
pub async fn backup_disable(state: State<'_, StdMutex<AppState>>) -> Result<(), String> {
    let t = { state.lock().map_err(|e| e.to_string())?.transport.clone() };
    if let Some(t) = t {
        t.delete_backup().await.map_err(|e| e.to_string())?;
    }
    let s = state.lock().map_err(|e| e.to_string())?;
    for k in [KEYS, PHRASE, LAST, HASH] {
        let _ = s.vault.delete_record(k);
    }
    Ok(())
}

#[derive(Serialize)]
pub struct Restored {
    /// false = the account has no backup (sign in normally).
    pub found: bool,
    /// true = the password didn't open it; ask for the recovery key.
    pub need_phrase: bool,
    pub username: Option<String>,
    pub email: Option<String>,
    pub ui: String,
}

/// New device, before sign-in: fetch the account's backup and, if it
/// opens, replace this device's (empty) vault with it — identity
/// included, so the account and every conversation continue as before.
#[tauri::command]
pub async fn backup_restore(
    state: State<'_, StdMutex<AppState>>,
    server_url: String,
    ticket: String,
    password: String,
    email: String,
    phrase: Option<String>,
) -> Result<Restored, String> {
    let secret = backup::auth_secret(&password, &email);
    let blob = match Transport::fetch_backup(&server_url, &ticket, &secret).await {
        Ok(b) => b,
        // Accounts made before secrets: the relay knows the raw password.
        Err(_) => Transport::fetch_backup(&server_url, &ticket, &password).await.map_err(|e| e.to_string())?,
    };
    let Some(blob) = blob else {
        return Ok(Restored { found: false, need_phrase: false, username: None, email: None, ui: String::new() });
    };
    let opened = tokio::task::spawn_blocking(move || match phrase {
        Some(p) if !p.trim().is_empty() => backup::open_with_recovery(&blob, &p).map(|r| (r, Some(p))),
        _ => backup::open_with_password(&blob, &password).map(|r| (r, None)),
    })
    .await
    .map_err(|e| e.to_string())?;
    let ((bytes, keys), phrase) = match opened {
        Ok(v) => v,
        Err(backup::BackupError::Key) => {
            return Ok(Restored { found: true, need_phrase: true, username: None, email: None, ui: String::new() })
        }
        Err(e) => return Err(e.to_string()),
    };
    let payload: Payload = bincode::deserialize(&bytes).map_err(|e| format!("backup contents: {e}"))?;
    let mut s = state.lock().map_err(|e| e.to_string())?;
    s.vault.import_dump(&payload.vault, &[]).map_err(|e| e.to_string())?;
    let _ = s.vault.put_record(KEYS, &serde_json::to_vec(&keys).unwrap_or_default());
    if let Some(p) = phrase {
        let _ = s.vault.put_record(PHRASE, p.trim().as_bytes());
    }
    // Rebuild the in-memory state from the restored vault.
    let data_dir = s.data_dir.clone();
    let fresh = AppState::boot(data_dir).map_err(|e| e.to_string())?.into_inner().map_err(|e| e.to_string())?;
    *s = fresh;
    Ok(Restored { found: true, need_phrase: false, username: payload.username, email: payload.email, ui: payload.ui })
}
