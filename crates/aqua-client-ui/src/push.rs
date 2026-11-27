//! Waking this phone while the app is closed (Android, UnifiedPush — no
//! Google): the distributor app the person chose holds one connection; the
//! relay posts a fixed "wake up" to its endpoint (aqua-server push.rs) and
//! the phone shows "New message" — no sender, no text; the app shows the
//! chats when it is opened. The endpoint comes from AquaPush (AquaRelay.kt)
//! and goes to the relay here, after every connection and when it changes.

use std::sync::Mutex as StdMutex;

use tauri::{AppHandle, Manager, State};

use crate::state::AppState;

fn transport(app: &AppHandle) -> Option<std::sync::Arc<aqua_client_core::transport::Transport>> {
    let state: State<'_, StdMutex<AppState>> = app.state();
    let s = state.lock().or_else(|e| Ok::<_, ()>(e.into_inner())).ok()?;
    s.transport.clone()
}

/// Tell the relay where this device is woken (or that it isn't any more).
/// Called after each connection: a relay that lost it, or another relay of
/// this account, learns it again. Older relays (no /v1/me/push): nothing.
pub async fn sync(app: &AppHandle) {
    #[cfg(target_os = "android")]
    {
        let Ok(st) = crate::relay_host::android::push_state() else { return };
        let Some(t) = transport(app) else { return };
        let endpoint = st["endpoint"].as_str().unwrap_or_default();
        let res = if st["on"].as_bool().unwrap_or(false) && !endpoint.is_empty() {
            t.push_set("unifiedpush", endpoint).await.map(|ok| {
                if !ok {
                    tracing::info!("push: this relay has no push (older version)");
                }
            })
        } else {
            t.push_unset().await
        };
        if let Err(e) = res {
            tracing::warn!(%e, "push: the relay didn't take the wake address");
        }
    }
    #[cfg(not(target_os = "android"))]
    let _ = transport(app);
}

/// Settings → Notifications → "Wake through UnifiedPush" (Android).
/// `text`: the notification's text in the UI language.
#[tauri::command]
pub async fn push_unifiedpush_set(app: AppHandle, on: bool, text: String) -> Result<serde_json::Value, String> {
    #[cfg(target_os = "android")]
    {
        let r = crate::relay_host::android::push_set(on, &text)?;
        if on && !r["ok"].as_bool().unwrap_or(false) {
            return Err("no UnifiedPush distributor on this phone — install one (for example ntfy) and try again".into());
        }
        // The distributor answers with the endpoint a moment later.
        let app2 = app.clone();
        tauri::async_runtime::spawn(async move {
            for _ in 0..30 {
                let st = crate::relay_host::android::push_state().unwrap_or_default();
                if !on || st["endpoint"].as_str().is_some_and(|e| !e.is_empty()) {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
            sync(&app2).await;
        });
        return crate::relay_host::android::push_state();
    }
    #[allow(unreachable_code)]
    {
        let _ = (app, on, text);
        Err("waking through UnifiedPush is only on Android".into())
    }
}

/// `{on, endpoint, distributor, distributors}`; `{available: false}` off Android.
#[tauri::command]
pub fn push_unifiedpush_state() -> serde_json::Value {
    #[cfg(target_os = "android")]
    {
        return crate::relay_host::android::push_state().unwrap_or_else(|e| serde_json::json!({ "available": false, "error": e }));
    }
    #[allow(unreachable_code)]
    serde_json::json!({ "available": false })
}
