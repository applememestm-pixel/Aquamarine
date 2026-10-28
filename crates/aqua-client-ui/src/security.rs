//! Sign-in protection of my account, kept by the relay: two-step codes
//! (TOTP, like Google Authenticator) and "no sign-in on new devices".
//! See `aqua-server/src/totp.rs` and `docs/accounts.md`.

use std::sync::Mutex as StdMutex;

use tauri::State;

use crate::state::AppState;

/// One call to `/v1/me/<path>`: over the connection, or — when the relay
/// doesn't let the app connect until two-step codes are set up — signed in
/// over HTTP to `server_url`.
async fn call(state: &State<'_, StdMutex<AppState>>, server_url: Option<String>, method: &str, path: &str, body: Option<serde_json::Value>) -> Result<serde_json::Value, String> {
    let (t, identity, known) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        (s.transport.clone(), s.identity.clone(), s.server_url.clone())
    };
    match (t, server_url.filter(|u| !u.is_empty()).or(known)) {
        (Some(t), _) => t.me_call(method, path, body).await.map_err(err),
        (None, Some(url)) => aqua_client_core::Transport::me_call_at(&url, &identity, method, path, body).await.map_err(err),
        (None, None) => Err("not connected".into()),
    }
}

fn err(e: aqua_client_core::TransportError) -> String {
    let m = e.to_string();
    if m.contains("404") { "this server doesn't support it yet — it needs updating".into() } else { m }
}

/// `{lock, totp, backup_left}`.
#[tauri::command]
pub async fn security_get(state: State<'_, StdMutex<AppState>>, server_url: Option<String>) -> Result<serde_json::Value, String> {
    call(&state, server_url, "GET", "security", None).await
}

#[tauri::command]
pub async fn security_set_lock(state: State<'_, StdMutex<AppState>>, lock: bool, code: Option<String>) -> Result<serde_json::Value, String> {
    let body = serde_json::json!({ "lock": lock, "code": code.unwrap_or_default() });
    call(&state, None, "PUT", "security", Some(body)).await
}

/// `{secret, uri}` for the authenticator app. `code`: with codes already
/// on, a current one moves them to another app.
#[tauri::command]
pub async fn totp_setup(state: State<'_, StdMutex<AppState>>, server_url: Option<String>, code: Option<String>) -> Result<serde_json::Value, String> {
    call(&state, server_url, "POST", "2fa/setup", Some(serde_json::json!({ "code": code.unwrap_or_default() }))).await
}

/// `{backup: [...]}`.
#[tauri::command]
pub async fn totp_enable(state: State<'_, StdMutex<AppState>>, code: String, server_url: Option<String>) -> Result<serde_json::Value, String> {
    call(&state, server_url, "POST", "2fa/enable", Some(serde_json::json!({ "code": code }))).await
}

#[tauri::command]
pub async fn totp_disable(state: State<'_, StdMutex<AppState>>, code: String) -> Result<(), String> {
    call(&state, None, "POST", "2fa/disable", Some(serde_json::json!({ "code": code }))).await.map(|_| ())
}

/// A new set of backup codes: `{backup: [...]}`.
#[tauri::command]
pub async fn totp_backup(state: State<'_, StdMutex<AppState>>, code: String) -> Result<serde_json::Value, String> {
    call(&state, None, "POST", "2fa/backup", Some(serde_json::json!({ "code": code }))).await
}
