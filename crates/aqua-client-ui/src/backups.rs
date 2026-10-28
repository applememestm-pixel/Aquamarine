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
    /// The 24-word recovery key, so every device the backup lands on can
    /// show it again (the backup itself opens with the password or this key).
    phrase: Option<String>,
}

/// Backups made before the recovery key was carried along.
#[derive(Deserialize)]
struct PayloadV1 {
    v: u32,
    created: u64,
    username: Option<String>,
    email: Option<String>,
    vault: VaultDump,
    ui: String,
}

fn decode_payload(bytes: &[u8]) -> Result<Payload, String> {
    match bincode::deserialize::<Payload>(bytes) {
        Ok(p) => Ok(p),
        Err(e) => match bincode::deserialize::<PayloadV1>(bytes) {
            Ok(o) => Ok(Payload { v: o.v, created: o.created, username: o.username, email: o.email, vault: o.vault, ui: o.ui, phrase: None }),
            Err(_) => Err(format!("backup contents: {e}")),
        },
    }
}

fn phrase_of(s: &AppState) -> Option<String> {
    s.vault.get_record(PHRASE).ok().flatten().map(|b| String::from_utf8_lossy(&b).trim().to_string()).filter(|p| !p.is_empty())
}

/// Tell the relay about the recovery key (only a hash-derived check goes
/// out), so a forgotten password can be reset with it. Best effort.
async fn push_recovery(transport: Option<std::sync::Arc<Transport>>, phrase: &str) -> Result<(), String> {
    let Some(t) = transport else { return Err("not connected".into()) };
    let check = backup::recovery_check(phrase).map_err(|e| e.to_string())?;
    t.put_recovery(&check).await.map_err(|e| e.to_string())
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
    // The account's recovery key, if this device already has one, stays.
    let existing = { let s = state.lock().map_err(|e| e.to_string())?; phrase_of(&s) };
    // Argon2id with 64 MiB: off the async runtime.
    let (keys, phrase) = tokio::task::spawn_blocking(move || match existing {
        Some(p) => backup::new_keys_with_phrase(&password, &p).unwrap_or_else(|_| backup::new_keys(&password)),
        None => backup::new_keys(&password),
    })
    .await
    .map_err(|e| e.to_string())?;
    // The relay learns about the key only once the user has seen it
    // (`recovery_create`): a key nobody wrote down must not lock them out.
    let s = state.lock().map_err(|e| e.to_string())?;
    s.vault.put_record(KEYS, &serde_json::to_vec(&keys).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    s.vault.put_record(PHRASE, phrase.as_bytes()).map_err(|e| e.to_string())?;
    let _ = s.vault.delete_record(LAST);
    let _ = s.vault.delete_record(HASH);
    Ok(phrase)
}

#[derive(Serialize)]
pub struct RecoveryStatus {
    /// This device knows the 24 words.
    pub local: bool,
    /// The relay has a recovery key for the account (None = couldn't ask).
    pub server: Option<bool>,
}

#[tauri::command]
pub async fn recovery_status(state: State<'_, StdMutex<AppState>>) -> Result<RecoveryStatus, String> {
    let (local, t) = { let s = state.lock().map_err(|e| e.to_string())?; (phrase_of(&s).is_some(), s.transport.clone()) };
    let server = match t { Some(t) => t.recovery_set().await.ok(), None => None };
    Ok(RecoveryStatus { local, server })
}

/// Make sure the account has a recovery key: reuse this device's, or make a
/// new one (the backup is re-wrapped so the new words open it too), and
/// register it on the relay. Call it when the words are on screen.
/// Returns the 24 words.
#[tauri::command]
pub async fn recovery_create(state: State<'_, StdMutex<AppState>>) -> Result<String, String> {
    let (phrase, t) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        let phrase = match phrase_of(&s) {
            Some(p) => p,
            None => {
                let p = backup::new_phrase();
                if let Some(mut keys) = keys_of(&s) {
                    backup::rewrap_recovery(&mut keys, &p).map_err(|e| e.to_string())?;
                    s.vault.put_record(KEYS, &serde_json::to_vec(&keys).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
                    let _ = s.vault.delete_record(HASH); // re-upload with the new header
                    let _ = s.vault.delete_record(LAST);
                }
                s.vault.put_record(PHRASE, p.as_bytes()).map_err(|e| e.to_string())?;
                p
            }
        };
        (phrase, s.transport.clone())
    };
    push_recovery(t, &phrase).await?;
    Ok(phrase)
}

/// Forgotten password, before sign-in: e-mail ticket + the 24 words (empty
/// when the account never had them) set a new password on the relay.
/// The UI then signs in with the new password as usual.
#[tauri::command]
pub async fn password_reset(server_url: String, ticket: String, email: String, phrase: String, password: String) -> Result<String, String> {
    if password.chars().count() < 8 {
        return Err("password must be at least 8 characters".into());
    }
    let phrase = phrase.trim().to_string();
    let (check, secret) = tokio::task::spawn_blocking(move || -> Result<(String, String), String> {
        let check = if phrase.is_empty() { String::new() } else { backup::recovery_check(&phrase).map_err(|e| e.to_string())? };
        Ok((check, backup::auth_secret(&password, &email)))
    })
    .await
    .map_err(|e| e.to_string())??;
    Transport::reset_password(&server_url, &ticket, &check, &secret).await.map_err(|e| {
        let m = e.to_string();
        if m.contains("recovery key") { "the recovery key doesn't match this account".to_string() } else { m }
    })
}

/// The recovery key (24 words) → the check the relay knows; "" for none.
pub async fn recovery_check_of(phrase: Option<String>) -> Result<String, String> {
    match phrase.map(|p| p.trim().to_string()).filter(|p| !p.is_empty()) {
        None => Ok(String::new()),
        Some(p) => tokio::task::spawn_blocking(move || backup::recovery_check(&p).map_err(|e| e.to_string()))
            .await
            .map_err(|e| e.to_string())?,
    }
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
            phrase: phrase_of(&s),
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
    // The recovery key stays: it also resets the account's password.
    for k in [KEYS, LAST, HASH] {
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
/// opens, take its chats, history and settings. The device keeps its OWN
/// identity key and sessions: it signs in as one more device of the
/// account (two devices sharing a key would break each other's sessions).
#[tauri::command]
pub async fn backup_restore(
    state: State<'_, StdMutex<AppState>>,
    server_url: String,
    ticket: String,
    password: String,
    email: String,
    phrase: Option<String>,
    totp: Option<String>,
    recovery: Option<String>,
) -> Result<Restored, String> {
    let secret = backup::auth_secret(&password, &email);
    let totp = totp.unwrap_or_default();
    let check = recovery_check_of(recovery).await?;
    let blob = match Transport::fetch_backup_checked(&server_url, &ticket, &secret, &totp, &check).await {
        Ok(b) => b,
        // Accounts made before secrets: the relay knows the raw password.
        Err(e) if e.to_string().contains("wrong email or password") => {
            Transport::fetch_backup_checked(&server_url, &ticket, &password, &totp, &check).await.map_err(|e| e.to_string())?
        }
        Err(e) => return Err(e.to_string()),
    };
    let Some(blob) = blob else {
        return Ok(Restored { found: false, need_phrase: false, username: None, email: None, ui: String::new() });
    };
    let pw = password.clone();
    let opened = tokio::task::spawn_blocking(move || match phrase {
        Some(p) if !p.trim().is_empty() => backup::open_with_recovery(&blob, &p).map(|r| (r, Some(p))),
        _ => backup::open_with_password(&blob, &pw).map(|r| (r, None)),
    })
    .await
    .map_err(|e| e.to_string())?;
    let ((bytes, mut keys), phrase) = match opened {
        Ok(v) => v,
        Err(backup::BackupError::Key) => {
            return Ok(Restored { found: true, need_phrase: true, username: None, email: None, ui: String::new() })
        }
        Err(e) => return Err(e.to_string()),
    };
    // Opened with the recovery key (password forgotten or changed): wrap the
    // data key under the current password too, so it opens the next backups.
    if phrase.is_some() {
        let pw = password.clone();
        keys = tokio::task::spawn_blocking(move || { backup::rewrap_password(&mut keys, &pw); keys }).await.map_err(|e| e.to_string())?;
    }
    let mut payload = decode_payload(&bytes)?;
    let phrase = phrase.or_else(|| payload.phrase.take());
    let mut s = state.lock().map_err(|e| e.to_string())?;
    // Keep this device's keys: identity, prekeys and per-device sessions.
    let own = s.vault.export_dump(&[]).map_err(|e| e.to_string())?;
    if own.identity.is_some() {
        payload.vault.identity = own.identity;
        payload.vault.sessions = own.sessions;
        payload.vault.signed_prekeys = own.signed_prekeys;
        payload.vault.one_time_prekeys = own.one_time_prekeys;
    }
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
