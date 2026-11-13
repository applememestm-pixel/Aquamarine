//! The TURN relay of my server (aqua-server `turnsrv.rs`, 1.20.1-6): calls
//! connect through any NAT without a setting. `GET /v1/ice` gives short-lived
//! credentials and the port (the host is the server's own, the one this app
//! already uses) or full URLs of another TURN.

use std::sync::Mutex as StdMutex;

use tauri::State;

use crate::state::AppState;

#[tauri::command]
pub async fn relay_ice(state: State<'_, StdMutex<AppState>>) -> Result<Option<serde_json::Value>, String> {
    let server = {
        let s = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        match &s.transport {
            Some(t) => t.server_url().to_string(),
            None => return Ok(None),
        }
    };
    let http = aqua_client_core::transport::http_client().map_err(|e| e.to_string())?;
    let r = http
        .get(format!("{}/v1/ice", server.trim_end_matches('/')))
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| e.without_url().to_string())?;
    if !r.status().is_success() {
        return Ok(None);
    }
    let v: serde_json::Value = r.json().await.map_err(|e| e.without_url().to_string())?;
    let (Some(user), Some(cred)) = (v["username"].as_str(), v["credential"].as_str()) else { return Ok(None) };
    let urls: Vec<String> = if let Some(list) = v["urls"].as_array() {
        list.iter().filter_map(|u| u.as_str()).filter(|u| u.starts_with("turn:") || u.starts_with("turns:")).map(str::to_string).collect()
    } else if let Some(port) = v["port"].as_u64() {
        let host = reqwest::Url::parse(&server).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default();
        if host.is_empty() || host == "127.0.0.1" || host == "localhost" {
            return Ok(None);
        }
        let host = if host.contains(':') && !host.starts_with('[') { format!("[{host}]") } else { host };
        vec![format!("turn:{host}:{port}?transport=udp")]
    } else {
        vec![]
    };
    if urls.is_empty() {
        return Ok(None);
    }
    Ok(Some(serde_json::json!({ "urls": urls, "username": user, "credential": cred })))
}
