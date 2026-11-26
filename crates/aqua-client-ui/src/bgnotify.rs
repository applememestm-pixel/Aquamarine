//! Native message notifications and the background Android widget.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::{Action, ActionType, NotificationExt};

use crate::state::AppState;

const READ: &str = "aqua-message-read";
const REPLY: &str = "aqua-message-reply";
const CHAT_ACTIONS: &str = "aqua-chat-actions";
const DIRECT_ACTIONS: &str = "aqua-direct-actions";

/// When the page last said "on screen" (ms since the epoch). A paused WebView
/// can't say "gone" — on Android `visibilitychange` often never fires before
/// the pause — so the page repeats it every 1.5 s and silence means background.
static FRONT_AT: AtomicU64 = AtomicU64::new(0);
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
fn in_front() -> bool {
    #[cfg(target_os = "android")]
    {
        ANDROID_FRONT.load(Ordering::Relaxed)
    }
    #[cfg(not(target_os = "android"))]
    {
        now_ms().saturating_sub(FRONT_AT.load(Ordering::Relaxed)) < 4_000
    }
}

#[cfg(target_os = "android")]
static ANDROID_FRONT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(target_os = "android")]
#[no_mangle]
extern "system" fn Java_app_aquamarine_messenger_MainActivity_nativeSetNotificationForeground(
    _env: *mut std::ffi::c_void,
    _activity: *mut std::ffi::c_void,
    front: u8,
) {
    ANDROID_FRONT.store(front != 0, Ordering::Relaxed);
}
static PREFS: Mutex<Option<Prefs>> = Mutex::new(None);

#[derive(serde::Deserialize, Default, Clone)]
pub struct Prefs {
    pub enabled: bool,
    pub muted: Vec<String>,
    pub locked: Vec<String>,
    /// "New message in a locked chat", in the UI language.
    pub locked_text: String,
    pub new_text: String,
    pub preview: String,
    pub read_text: String,
    pub reply_text: String,
    pub restricted: bool,
    pub denied: Vec<String>,
    pub allowed: Vec<String>,
    /// Home-screen widget, kept fresh from here while the page sleeps.
    #[serde(default)]
    pub archived: Vec<String>,
    #[serde(default)]
    pub widget_text: bool,
    #[serde(default)]
    pub widget_empty: String,
    /// "Incoming call", in the UI language.
    #[serde(default)]
    pub call_text: String,
}

#[tauri::command]
pub fn notify_front(front: bool) {
    FRONT_AT.store(if front { now_ms() } else { 0 }, Ordering::Relaxed);
}

fn prefs() -> Option<Prefs> {
    PREFS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

#[tauri::command]
pub async fn notify_prefs(app: AppHandle, prefs: Prefs) {
    let old = {
        let mut saved = PREFS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        saved.replace(prefs.clone())
    };
    if old
        .as_ref()
        .is_none_or(|p| p.read_text != prefs.read_text || p.reply_text != prefs.reply_text)
    {
        let read = Action::builder(READ, &prefs.read_text)
            .foreground(false)
            .build();
        let reply = Action::builder(REPLY, &prefs.reply_text)
            .foreground(true)
            .build();
        let direct = if cfg!(target_os = "android") {
            Action::builder(REPLY, &prefs.reply_text)
                .foreground(false)
                .input(true)
                .input_button_title(&prefs.reply_text)
                .build()
        } else {
            reply.clone()
        };
        if let Err(e) = app.notification().register_action_types(vec![
            ActionType::builder(CHAT_ACTIONS)
                .actions(vec![read.clone(), reply])
                .build(),
            ActionType::builder(DIRECT_ACTIONS)
                .actions(vec![read, direct])
                .build(),
            ActionType::builder("aqua-read-only")
                .actions(vec![Action::builder(READ, &prefs.read_text)
                    .foreground(false)
                    .build()])
                .build(),
        ]) {
            tracing::warn!("register message notification actions: {e}");
        }
    }
    let hidden: Vec<_> = prefs
        .locked
        .iter()
        .filter(|id| old.as_ref().is_none_or(|p| !p.locked.contains(id)))
        .map(String::as_str)
        .collect();
    remove_chats(&app, hidden);
    if cfg!(target_os = "android") {
        widget(&app, &prefs);
    }
}

fn notification_id(account: &str, chat: &str) -> i32 {
    let mut h = DefaultHasher::new();
    (account, chat).hash(&mut h);
    // Keep the IDs positive and away from the plugin's small default IDs.
    ((h.finish() & 0x3fff_ffff) | 0x4000_0000) as i32
}

fn remove_chats(app: &AppHandle, chats: Vec<&str>) {
    if chats.is_empty() {
        return;
    }
    let state = app.state::<Mutex<AppState>>();
    let account = state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .username
        .clone();
    let Some(account) = account else { return };
    let ids = chats
        .into_iter()
        .map(|chat| notification_id(&account, chat))
        .collect();
    #[cfg(windows)]
    windows_toast::remove(app, ids);
    #[cfg(not(windows))]
    let _ = app.notification().remove_active(ids);
}

pub fn clear_chat(app: &AppHandle, chat: &str) {
    remove_chats(app, vec![chat]);
    if cfg!(target_os = "android") {
        if let Some(p) = prefs() {
            widget(app, &p);
        }
    }
}

pub fn hide_on_lock(app: &AppHandle) {
    let state = app.state::<Mutex<AppState>>();
    let chats: Vec<String> = state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .chats
        .keys()
        .cloned()
        .collect();
    remove_chats(app, chats.iter().map(String::as_str).collect());
    if cfg!(target_os = "android") {
        if let Some(p) = prefs() {
            widget(app, &p);
        } else {
            let _ = crate::relay_host::widget_update(
                serde_json::json!({ "title": "Aquamarine", "unread": 0, "rows": [] }).to_string(),
            );
        }
    }
}

fn allowed(p: &Prefs, peer: &str) -> bool {
    peer.starts_with("grp_")
        || peer.starts_with("dm~")
        || p.allowed.iter().any(|id| id == peer)
        || (!p.restricted && !p.denied.iter().any(|id| id == peer))
}

fn show_message(app: &AppHandle, p: &Prefs, peer: &str, title: Option<String>, preview: &str) {
    if !p.enabled || peer.starts_with("__") || !allowed(p, peer) {
        return;
    }
    let (account, name, is_group, can_reply) = {
        let state = app.state::<Mutex<AppState>>();
        let s = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (Some(account), Some(book)) = (&s.username, s.chats.get(peer)) else {
            return;
        };
        let group = s.groups.get(peer);
        let can_reply = group.is_none_or(|g| !book.summary.channel || g.is_admin(account));
        (
            account.clone(),
            book.summary.name.clone(),
            group.is_some(),
            can_reply,
        )
    };
    let hidden = crate::applock::is_locked() || p.locked.iter().any(|id| id == peer);
    let (title, body) = if hidden {
        ("Aquamarine".into(), p.locked_text.clone())
    } else {
        let title = if p.preview == "none" {
            "Aquamarine".into()
        } else {
            title.unwrap_or(name)
        };
        let body = if p.preview == "full" && !preview.trim().is_empty() {
            preview.chars().take(160).collect()
        } else {
            p.new_text.clone()
        };
        (title, body)
    };
    let id = notification_id(&account, peer);
    let actions = !hidden && p.preview != "none";
    #[cfg(windows)]
    {
        let _ = is_group;
        let route = actions.then_some((account, peer.to_owned()));
        if let Err(e) = windows_toast::show(app, id, &title, &body, p, route, can_reply, hidden) {
            tracing::debug!("show message notification: {e}");
        }
    }
    #[cfg(not(windows))]
    {
        let mut note = app
            .notification()
            .builder()
            .id(id)
            .title(title)
            .body(body)
            .auto_cancel();
        // A hidden notification carries no route.
        if actions {
            let action_type = if !can_reply {
                "aqua-read-only"
            } else if is_group {
                CHAT_ACTIONS
            } else {
                DIRECT_ACTIONS
            };
            note = note
                .action_type_id(action_type)
                .extra("aqua_peer", peer)
                .extra("aqua_account", account);
        }
        #[cfg(target_os = "android")]
        let note = note.icon("ic_stat_aquamarine");
        if let Err(e) = note.show() {
            tracing::debug!("show message notification: {e}");
        }
    }
}

#[tauri::command]
pub async fn notify_permission(app: AppHandle, ask: bool) -> Result<bool, String> {
    use tauri_plugin_notification::PermissionState;
    let notification = app.notification();
    let permission = notification.permission_state().map_err(|e| e.to_string())?;
    if permission == PermissionState::Granted {
        return Ok(true);
    }
    if !ask {
        return Ok(false);
    }
    notification
        .request_permission()
        .map(|p| p == PermissionState::Granted)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn notify_message(app: AppHandle, peer: String, title: String, preview: String) {
    if let Some(p) = prefs() {
        show_message(&app, &p, &peer, Some(title), &preview);
    }
}

/// Phones: a call started or stopped ringing. While the app is not on screen a
/// ringtone alone can go unnoticed, so say it in a notification too. Whose
/// calls may ring at all is decided by the page before it gets here.
#[tauri::command]
pub async fn notify_call(app: AppHandle, peer: String, ringing: bool) {
    let key = format!("call~{peer}");
    if !ringing {
        remove_chats(&app, vec![key.as_str()]);
        return;
    }
    if !cfg!(mobile) || in_front() {
        return;
    }
    let Some(p) = prefs() else { return };
    if !p.enabled {
        return;
    }
    let (account, name) = {
        let state = app.state::<Mutex<AppState>>();
        let s = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(account) = s.username.clone() else {
            return;
        };
        let name = s.chats.get(&peer).map(|book| book.summary.name.clone());
        (account, name.unwrap_or_else(|| peer.clone()))
    };
    let hidden = crate::applock::is_locked() || p.locked.contains(&peer);
    let title = if hidden || p.preview == "none" {
        "Aquamarine".into()
    } else {
        name
    };
    let note = app
        .notification()
        .builder()
        .id(notification_id(&account, &key))
        .title(title)
        .body(p.call_text)
        .auto_cancel();
    #[cfg(target_os = "android")]
    let note = note.icon("ic_stat_aquamarine");
    if let Err(e) = note.show() {
        tracing::debug!("show call notification: {e}");
    }
}

pub fn incoming(app: &AppHandle, chat: &str, preview: &str) {
    if !cfg!(mobile) || in_front() || chat.starts_with("__") {
        return;
    }
    let Some(p) = prefs() else { return };
    widget(app, &p);
    if !p.muted.iter().any(|id| id == chat) {
        show_message(app, &p, chat, None, preview);
    }
}

fn can_act(app: &AppHandle, account: &str, peer: &str) -> bool {
    if crate::applock::is_locked() {
        return false;
    }
    let Some(p) = prefs() else { return false };
    if !p.enabled
        || p.preview == "none"
        || p.locked.iter().any(|id| id == peer)
        || !allowed(&p, peer)
    {
        return false;
    }
    let state = app.state::<Mutex<AppState>>();
    let s = state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    s.username.as_deref() == Some(account) && s.chats.contains_key(peer)
}

pub fn listen(app: AppHandle) {
    let handle = app.clone();
    if let Err(e) = app.notification().on_action(move |event| {
        let Some(note) = event.notification() else {
            return;
        };
        let Some(peer) = note
            .extra()
            .get("aqua_peer")
            .and_then(|v| v.as_str())
            .map(str::to_owned)
        else {
            return;
        };
        let Some(account) = note
            .extra()
            .get("aqua_account")
            .and_then(|v| v.as_str())
            .map(str::to_owned)
        else {
            return;
        };
        let action = event.action_id().to_owned();
        let input = event
            .input_value()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        perform_action(handle.clone(), account, peer, action, input);
    }) {
        tracing::warn!("listen for message notification actions: {e}");
    }
}

fn perform_action(
    app: AppHandle,
    account: String,
    peer: String,
    action: String,
    input: Option<String>,
) {
    tauri::async_runtime::spawn(async move {
        if !can_act(&app, &account, &peer) {
            return;
        }
        let state = app.state::<Mutex<AppState>>();
        match (action.as_str(), input) {
            (READ, _) => {
                if crate::commands::mark_chat_read(app.clone(), state, peer.clone()).is_ok() {
                    let _ = app.emit("aqua://notification-read", peer);
                }
            }
            (REPLY, Some(text)) if cfg!(target_os = "android") && !peer.starts_with("grp_") => {
                if text.len() > 16 * 1024 || crate::commands::is_control_text(&text) {
                    return;
                }
                match crate::commands::send_message(app.clone(), state, peer.clone(), text.clone())
                    .await
                {
                    Ok(_) => {
                        let _ = crate::commands::mark_chat_read(app.clone(), app.state(), peer);
                    }
                    Err(_) if can_act(&app, &account, &peer) => open_chat(&app, &peer, Some(text)),
                    Err(_) => {}
                }
            }
            (REPLY, _) => open_chat(&app, &peer, None),
            _ => {}
        }
    });
}

fn open_chat(app: &AppHandle, peer: &str, draft: Option<String>) {
    #[cfg(desktop)]
    crate::tray::show_main(app);
    let _ = app.emit(
        "aqua://notification-reply",
        serde_json::json!({ "peer_id": peer, "draft": draft }),
    );
}

/// The widget the page would draw (ui/src/lib/widget.ts), from Rust.
fn widget(app: &AppHandle, p: &Prefs) {
    let Some(st) = app.try_state::<std::sync::Mutex<crate::state::AppState>>() else {
        return;
    };
    let app_locked = crate::applock::is_locked();
    let rows: Vec<(String, String, u32)> = {
        let Ok(s) = st.lock().or_else(|e| Ok::<_, ()>(e.into_inner())) else {
            return;
        };
        s.chat_order
            .iter()
            .filter(|id| !id.starts_with("__") && !p.muted.contains(id) && !p.archived.contains(id))
            .filter_map(|id| s.chats.get(id).map(|b| (id, b)))
            .filter(|(_, b)| b.summary.unread > 0)
            .map(|(id, b)| {
                let text = if p.widget_text && !app_locked && !p.locked.contains(id) {
                    b.summary
                        .preview
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                        .chars()
                        .take(80)
                        .collect()
                } else {
                    String::new()
                };
                let name = if p.locked.contains(id) {
                    p.locked_text.clone()
                } else {
                    b.summary.name.clone()
                };
                (name, text, b.summary.unread)
            })
            .collect()
    };
    let total: u32 = rows.iter().map(|r| r.2).sum();
    let data = serde_json::json!({
        "title": "Aquamarine",
        "unread": total,
        "empty": p.widget_empty,
        "rows": rows.iter().take(if app_locked { 0 } else { 3 }).map(|(n, t, u)| serde_json::json!({ "n": n, "p": t, "u": u })).collect::<Vec<_>>(),
    });
    let _ = crate::relay_host::widget_update(data.to_string());
}

#[cfg(windows)]
mod windows_toast;
