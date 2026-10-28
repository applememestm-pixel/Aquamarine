//! Sign-in protection of my account, kept by the relay: two-step codes
//! (TOTP, like Google Authenticator) and "no sign-in on new devices".
//! See `aqua-server/src/totp.rs` and `docs/accounts.md`.

use std::sync::Mutex as StdMutex;

use tauri::State;

use crate::state::AppState;

fn transport(state: &State<'_, StdMutex<AppState>>) -> Result<std::sync::Arc<aqua_client_core::Transport>, String> {
    state.lock().map_err(|e| e.to_string())?.transport.clone().ok_or_else(|| "not connected".to_string())
}

fn err(e: aqua_client_core::TransportError) -> String {
    let m = e.to_string();
    if m.contains("404") { "this server doesn't support it yet — it needs updating".into() } else { m }
}

/// `{lock, totp, backup_left}`.
#[tauri::command]
pub async fn security_get(state: State<'_, StdMutex<AppState>>) -> Result<serde_json::Value, String> {
    transport(&state)?.me_call("GET", "security", None).await.map_err(err)
}

#[tauri::command]
pub async fn security_set_lock(state: State<'_, StdMutex<AppState>>, lock: bool, code: Option<String>) -> Result<serde_json::Value, String> {
    let body = serde_json::json!({ "lock": lock, "code": code.unwrap_or_default() });
    transport(&state)?.me_call("PUT", "security", Some(body)).await.map_err(err)
}

/// `{secret, uri}` for the authenticator app.
#[tauri::command]
pub async fn totp_setup(state: State<'_, StdMutex<AppState>>) -> Result<serde_json::Value, String> {
    transport(&state)?.me_call("POST", "2fa/setup", Some(serde_json::json!({}))).await.map_err(err)
}

/// `{backup: [...]}`.
#[tauri::command]
pub async fn totp_enable(state: State<'_, StdMutex<AppState>>, code: String) -> Result<serde_json::Value, String> {
    transport(&state)?.me_call("POST", "2fa/enable", Some(serde_json::json!({ "code": code }))).await.map_err(err)
}

#[tauri::command]
pub async fn totp_disable(state: State<'_, StdMutex<AppState>>, code: String) -> Result<(), String> {
    transport(&state)?.me_call("POST", "2fa/disable", Some(serde_json::json!({ "code": code }))).await.map_err(err).map(|_| ())
}

/// A new set of backup codes: `{backup: [...]}`.
#[tauri::command]
pub async fn totp_backup(state: State<'_, StdMutex<AppState>>, code: String) -> Result<serde_json::Value, String> {
    transport(&state)?.me_call("POST", "2fa/backup", Some(serde_json::json!({ "code": code }))).await.map_err(err)
}
