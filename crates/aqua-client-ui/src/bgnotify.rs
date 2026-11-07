//! Notifications while the app is in the background on a phone.
//!
//! The UI shows notifications (ui/src/lib/stores.ts::notifyIncoming), but
//! Android pauses the WebView of an app that isn't on screen: messages kept
//! arriving (the Rust side runs on) and nothing reached the notification
//! shade. Here, with the window in the background, the incoming message is
//! announced from Rust. The UI keeps it in step: whether it's on screen
//! (`notify_front`) and the user's choices (`notify_prefs`: on/off, muted
//! chats, chats with a password).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use tauri::{AppHandle, Manager};

/// When the page last said "on screen" (ms since the epoch). A paused WebView
/// can't say "gone" — on Android `visibilitychange` often never fires before
/// the pause — so the page repeats it every 1.5 s and silence means background.
static FRONT_AT: AtomicU64 = AtomicU64::new(0);
fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}
fn in_front() -> bool {
    now_ms().saturating_sub(FRONT_AT.load(Ordering::Relaxed)) < 4_000
}
static PREFS: Mutex<Option<Prefs>> = Mutex::new(None);

#[derive(serde::Deserialize, Default, Clone)]
pub struct Prefs {
    pub enabled: bool,
    pub muted: Vec<String>,
    pub locked: Vec<String>,
    /// "New message in a locked chat", in the UI language.
    pub locked_text: String,
    /// Home-screen widget, kept fresh from here while the page sleeps.
    #[serde(default)]
    pub archived: Vec<String>,
    #[serde(default)]
    pub widget_text: bool,
    #[serde(default)]
    pub widget_empty: String,
}

#[tauri::command]
pub fn notify_front(front: bool) {
    FRONT_AT.store(if front { now_ms() } else { 0 }, Ordering::Relaxed);
}

#[tauri::command]
pub fn notify_prefs(prefs: Prefs) {
    if let Ok(mut g) = PREFS.lock() {
        *g = Some(prefs);
    }
}

/// A message arrived in `chat`; called before it's handed to the UI.
pub fn incoming(app: &AppHandle, chat: &str) {
    if !cfg!(mobile) || in_front() || chat.starts_with("__") {
        return;
    }
    let Some(p) = PREFS.lock().ok().and_then(|g| g.clone()) else { return };
    widget(app, &p);
    if !p.enabled || p.muted.iter().any(|m| m == chat) {
        return;
    }
    let (title, body) = {
        let Some(st) = app.try_state::<std::sync::Mutex<crate::state::AppState>>() else { return };
        let Ok(s) = st.lock() else { return };
        let Some(book) = s.chats.get(chat) else { return };
        (book.summary.name.clone(), book.summary.preview.clone())
    };
    let (title, body) = if p.locked.iter().any(|m| m == chat) { ("Aquamarine".to_string(), p.locked_text) } else { (title, body) };
    use tauri_plugin_notification::NotificationExt;
    let _ = app.notification().builder().title(title).body(body.chars().take(160).collect::<String>()).show();
}

/// The widget the page would draw (ui/src/lib/widget.ts), from Rust.
fn widget(app: &AppHandle, p: &Prefs) {
    let Some(st) = app.try_state::<std::sync::Mutex<crate::state::AppState>>() else { return };
    let rows: Vec<(String, String, u32)> = {
        let Ok(s) = st.lock() else { return };
        s.chat_order.iter()
            .filter(|id| !id.starts_with("__") && !p.muted.contains(id) && !p.archived.contains(id))
            .filter_map(|id| s.chats.get(id).map(|b| (id, b)))
            .filter(|(_, b)| b.summary.unread > 0)
            .map(|(id, b)| {
                let text = if p.widget_text && !p.locked.contains(id) {
                    b.summary.preview.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(80).collect()
                } else { String::new() };
                (b.summary.name.clone(), text, b.summary.unread)
            })
            .collect()
    };
    let total: u32 = rows.iter().map(|r| r.2).sum();
    let data = serde_json::json!({
        "title": "Aquamarine",
        "unread": total,
        "empty": p.widget_empty,
        "rows": rows.iter().take(3).map(|(n, t, u)| serde_json::json!({ "n": n, "p": t, "u": u })).collect::<Vec<_>>(),
    });
    let _ = crate::relay_host::widget_update(data.to_string());
}
