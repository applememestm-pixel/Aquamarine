//! Notifications while the app is in the background on a phone.
//!
//! The UI shows notifications (ui/src/lib/stores.ts::notifyIncoming), but
//! Android pauses the WebView of an app that isn't on screen: messages kept
//! arriving (the Rust side runs on) and nothing reached the notification
//! shade. Here, with the window in the background, the incoming message is
//! announced from Rust. The UI keeps it in step: whether it's on screen
//! (`notify_front`) and the user's choices (`notify_prefs`: on/off, muted
//! chats, chats with a password).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use tauri::{AppHandle, Manager};

static FRONT: AtomicBool = AtomicBool::new(true);
static PREFS: Mutex<Option<Prefs>> = Mutex::new(None);

#[derive(serde::Deserialize, Default, Clone)]
pub struct Prefs {
    pub enabled: bool,
    pub muted: Vec<String>,
    pub locked: Vec<String>,
    /// "New message in a locked chat", in the UI language.
    pub locked_text: String,
}

#[tauri::command]
pub fn notify_front(front: bool) {
    FRONT.store(front, Ordering::Relaxed);
}

#[tauri::command]
pub fn notify_prefs(prefs: Prefs) {
    if let Ok(mut g) = PREFS.lock() {
        *g = Some(prefs);
    }
}

/// A message arrived in `chat`; called before it's handed to the UI.
pub fn incoming(app: &AppHandle, chat: &str) {
    if !cfg!(mobile) || FRONT.load(Ordering::Relaxed) || chat.starts_with("__") {
        return;
    }
    let Some(p) = PREFS.lock().ok().and_then(|g| g.clone()) else { return };
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
