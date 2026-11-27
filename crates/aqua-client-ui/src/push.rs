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

/// What was last told to the relay: (connection, woken?, endpoint).
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
static TOLD: std::sync::Mutex<Option<(usize, bool, String)>> = std::sync::Mutex::new(None);

/// Must the relay be told (again)? On a new connection, when woken turns on
/// or off, and when the endpoint changes while on.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn needs_telling(told: Option<&(usize, bool, String)>, conn: usize, want: bool, endpoint: &str) -> bool {
    !told.is_some_and(|(c, w, e)| *c == conn && *w == want && (!want || e == endpoint))
}

/// Tell the relay where this device is woken (or that it isn't any more).
/// Called after each connection — a relay that lost it, or another relay of
/// this account, learns it again — then every minute while connected, so an
/// endpoint the distributor sends late, changes or takes back (review of
/// #115: only 15 s were waited) reaches the relay too. Older relays (no
/// /v1/me/push): nothing. Each account slot has its own registration.
pub async fn sync(app: &AppHandle) {
    #[cfg(target_os = "android")]
    {
        let Some(t) = transport(app) else { return };
        let id = std::sync::Arc::as_ptr(&t) as usize;
        let instance = crate::accounts::active_slot(app);
        let Ok(st) = crate::relay_host::android::push_state(&instance) else { return };
        let on = st["on"].as_bool().unwrap_or(false);
        let endpoint = st["endpoint"].as_str().unwrap_or_default().to_string();
        let want = on && !endpoint.is_empty();
        let told = TOLD.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if !needs_telling(told.as_ref(), id, want, &endpoint) {
            return;
        }
        let res = if want {
            t.push_set("unifiedpush", &endpoint).await.map(|ok| {
                if !ok {
                    tracing::info!("push: this relay has no push (older version)");
                }
            })
        } else {
            t.push_unset().await
        };
        match res {
            Ok(()) => *TOLD.lock().unwrap_or_else(|e| e.into_inner()) = Some((id, want, endpoint)),
            Err(e) => tracing::warn!(%e, "push: the relay didn't take the wake address"),
        }
    }
    #[cfg(not(target_os = "android"))]
    let _ = transport(app);
}

/// After a connection: sync now, then every minute while this connection
/// lasts (a new connection starts its own).
pub async fn watch(app: AppHandle) {
    let Some(first) = transport(&app) else { return };
    loop {
        sync(&app).await;
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        match transport(&app) {
            Some(t) if std::sync::Arc::ptr_eq(&t, &first) => {}
            _ => return,
        }
    }
}

/// Logging out of this account: the relay stops waking this device for it,
/// and the distributor forgets this account's registration.
pub fn on_log_out(app: &AppHandle, t: Option<std::sync::Arc<aqua_client_core::transport::Transport>>) {
    #[cfg(target_os = "android")]
    {
        let _ = crate::relay_host::android::push_set(&crate::accounts::active_slot(app), false, "");
        *TOLD.lock().unwrap_or_else(|e| e.into_inner()) = None;
        if let Some(t) = t {
            tauri::async_runtime::spawn(async move {
                if let Err(e) = t.push_unset().await {
                    tracing::warn!(%e, "push: the relay wasn't told on log-out");
                }
            });
        }
    }
    #[cfg(not(target_os = "android"))]
    let _ = (app, t);
}

/// Settings → Notifications → "Wake through UnifiedPush" (Android).
/// `text`: the notification's text in the UI language.
#[tauri::command]
pub async fn push_unifiedpush_set(app: AppHandle, on: bool, text: String) -> Result<serde_json::Value, String> {
    #[cfg(target_os = "android")]
    {
        let instance = crate::accounts::active_slot(&app);
        let r = crate::relay_host::android::push_set(&instance, on, &text)?;
        if on && !r["ok"].as_bool().unwrap_or(false) {
            return Err("no UnifiedPush distributor on this phone — install one (for example ntfy) and try again".into());
        }
        // The distributor answers with the endpoint a moment later; if later
        // still, `watch` sends it within a minute.
        let (app2, inst2) = (app.clone(), instance.clone());
        tauri::async_runtime::spawn(async move {
            for _ in 0..30 {
                let st = crate::relay_host::android::push_state(&inst2).unwrap_or_default();
                if !on || st["endpoint"].as_str().is_some_and(|e| !e.is_empty()) {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
            sync(&app2).await;
        });
        return crate::relay_host::android::push_state(&instance);
    }
    #[allow(unreachable_code)]
    {
        let _ = (app, on, text);
        Err("waking through UnifiedPush is only on Android".into())
    }
}

/// `{on, endpoint, distributor, distributors}`; `{available: false}` off Android.
#[tauri::command]
pub fn push_unifiedpush_state(app: AppHandle) -> serde_json::Value {
    #[cfg(target_os = "android")]
    {
        return crate::relay_host::android::push_state(&crate::accounts::active_slot(&app)).unwrap_or_else(|e| serde_json::json!({ "available": false, "error": e }));
    }
    #[allow(unreachable_code)]
    {
        let _ = app;
        serde_json::json!({ "available": false })
    }
}

#[cfg(test)]
mod tests {
    /// What was told to the relay is per connection and per state: a change
    /// of endpoint, or off, is told again; the same twice isn't.
    #[test]
    fn told_again_only_on_change() {
        use super::needs_telling;
        let t = (1usize, true, "https://up/a".to_string());
        assert!(needs_telling(None, 1, true, "https://up/a"), "first time");
        assert!(!needs_telling(Some(&t), 1, true, "https://up/a"), "the same twice isn't");
        assert!(needs_telling(Some(&t), 1, true, "https://up/b"), "a late or changed endpoint is told");
        assert!(needs_telling(Some(&t), 1, false, ""), "taken back (off) is told");
        assert!(needs_telling(Some(&t), 2, true, "https://up/a"), "a new connection tells again");
        let off = (1usize, false, String::new());
        assert!(!needs_telling(Some(&off), 1, false, ""), "off twice isn't");
    }
}
