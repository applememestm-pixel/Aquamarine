//! Auto-delete timers (Telegram's "auto-delete messages").
//!
//! A chat can have a timer: every message that arrives or is sent after the
//! timer was set disappears from this device that long after it showed up.
//! Messages already in the chat when the timer was set stay.
//!
//! * 1:1 chats: the timer is shared — setting it sends `[ttl:v1]{"secs":N}` to
//!   the other person (and my other devices), whose app applies it too.
//! * Groups: the timer is the group setting `ttl` (seconds), changed by
//!   admins like any other setting and spread with the group snapshot.
//!
//! The relay never stores messages longer than it has to anyway; the timer is
//! about the copies on the devices. Every device deletes its own copy.

use std::collections::HashMap;
use std::sync::Mutex as StdMutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::state::{AppState, ChatMessage, MessageKind};

pub const TTL_PREFIX: &str = "[ttl:v1]";
const RECORD: &str = "autodelete:v1";
/// Stamp of messages that were there before the timer: never deleted.
const EXEMPT: u64 = 0;

#[derive(Default, Serialize, Deserialize)]
pub struct TtlBook {
    /// 1:1 chats: timer in seconds.
    #[serde(default)]
    chats: HashMap<String, u64>,
    /// Per chat: message id → when this device first saw it (unix secs),
    /// or [`EXEMPT`].
    #[serde(default)]
    seen: HashMap<String, HashMap<String, u64>>,
    /// The timer each chat had at the last sweep (spots group changes).
    #[serde(default)]
    known: HashMap<String, u64>,
    #[serde(skip)]
    dirty: bool,
}

pub fn load(vault: &aqua_client_core::vault::Vault) -> TtlBook {
    vault.get_record(RECORD).ok().flatten().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// The timer of a chat, in seconds (0 = off).
pub fn ttl_of(s: &AppState, chat: &str) -> u64 {
    if let Some(g) = s.groups.get(chat) {
        return g.settings.extra.get("ttl").and_then(|v| v.as_u64()).unwrap_or(0);
    }
    s.ttl.chats.get(chat).copied().unwrap_or(0)
}

/// The timer changed: what's in the chat now stays; newer messages expire.
fn on_change(s: &mut AppState, chat: &str, secs: u64) {
    let ids: Vec<String> = s.chats.get(chat).map(|b| b.messages.iter().map(|m| m.id.clone()).collect()).unwrap_or_default();
    let seen = s.ttl.seen.entry(chat.to_string()).or_default();
    if secs == 0 {
        s.ttl.seen.remove(chat);
    } else {
        for id in ids {
            seen.entry(id).or_insert(EXEMPT);
        }
    }
    if secs == 0 { s.ttl.known.remove(chat); } else { s.ttl.known.insert(chat.to_string(), secs); }
    s.ttl.dirty = true;
}

pub fn human(secs: u64) -> String {
    match secs {
        0 => "off".into(),
        s if s % 86_400 == 0 && s >= 86_400 * 7 && s % (86_400 * 7) == 0 && s < 86_400 * 28 => format!("{} wk", s / (86_400 * 7)),
        s if s % 86_400 == 0 => format!("{} d", s / 86_400),
        s if s % 3_600 == 0 => format!("{} h", s / 3_600),
        s if s % 60 == 0 => format!("{} min", s / 60),
        s => format!("{s} s"),
    }
}

fn system_note(s: &mut AppState, chat: &str, text: String) {
    if let Some(book) = s.chats.get_mut(chat) {
        book.messages.push(ChatMessage {
            id: format!("ttl-{}", crate::state::uuid_like()),
            from: "system".into(),
            time: String::new(),
            kind: MessageKind::System { text },
            meta: None,
        });
    }
}

/// Apply a 1:1 timer (mine, from the peer, or from my other device).
/// `by` = who set it ("" = me).
pub fn apply_chat_ttl(s: &mut AppState, chat: &str, secs: u64, by: &str) {
    if s.groups.contains_key(chat) {
        return;
    }
    let secs = secs.min(366 * 86_400);
    let before = s.ttl.chats.get(chat).copied().unwrap_or(0);
    if before == secs {
        return;
    }
    if secs == 0 { s.ttl.chats.remove(chat); } else { s.ttl.chats.insert(chat.to_string(), secs); }
    on_change(s, chat, secs);
    let who = if by.is_empty() { "You".to_string() } else { format!("@{by}") };
    let text = if secs == 0 {
        format!("{who} turned off auto-delete")
    } else {
        format!("{who} set messages to auto-delete after {}", human(secs))
    };
    system_note(s, chat, text);
}

/// `[ttl:v1]{"secs":N}` from `from` (the peer) — or from my other device for
/// the chat with `chat`.
pub fn handle_control(s: &mut AppState, chat: &str, body: &str, by: &str) {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
        let secs = v.get("secs").and_then(|x| x.as_u64()).unwrap_or(0);
        apply_chat_ttl(s, chat, secs, by);
    }
}

/// One pass: stamp new messages, delete the expired ones. Returns the chats
/// that changed.
pub fn sweep(s: &mut AppState) -> Vec<String> {
    let t = now();
    let ids: Vec<String> = s.chats.keys().cloned().collect();
    let mut changed = Vec::new();
    for chat in ids {
        let ttl = ttl_of(s, &chat);
        let known = s.ttl.known.get(&chat).copied().unwrap_or(0);
        if ttl != known {
            on_change(s, &chat, ttl);
        }
        if ttl == 0 {
            continue;
        }
        let Some(book) = s.chats.get_mut(&chat) else { continue };
        let seen = s.ttl.seen.entry(chat.clone()).or_default();
        let mut expired: Vec<String> = Vec::new();
        for m in &book.messages {
            match seen.get(&m.id) {
                None => {
                    seen.insert(m.id.clone(), t);
                    s.ttl.dirty = true;
                }
                Some(&at) if at != EXEMPT && t.saturating_sub(at) >= ttl => {
                    // Service notes about the timer itself stay.
                    if !m.id.starts_with("ttl-") {
                        expired.push(m.id.clone());
                    }
                }
                _ => {}
            }
        }
        if !expired.is_empty() {
            book.messages.retain(|m| !expired.contains(&m.id));
            book.summary.preview = book.messages.last().map(|m| crate::state::preview_for(&m.kind)).unwrap_or_default();
            for id in &expired {
                seen.remove(id);
            }
            s.ttl.dirty = true;
            changed.push(chat.clone());
        }
        // Forget stamps of messages that are gone some other way.
        if seen.len() > book.messages.len() + 64 {
            let live: std::collections::HashSet<&str> = book.messages.iter().map(|m| m.id.as_str()).collect();
            seen.retain(|id, _| live.contains(id.as_str()));
        }
    }
    if s.ttl.dirty {
        if let Ok(b) = serde_json::to_vec(&s.ttl) {
            if s.vault.put_record(RECORD, &b).is_ok() {
                s.ttl.dirty = false;
            }
        }
    }
    changed
}

/// Background sweeper (every 10 s).
pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(10));
        let st = app.state::<StdMutex<AppState>>();
        let changed = match st.lock() {
            Ok(mut s) => sweep(&mut s),
            Err(_) => continue,
        };
        if !changed.is_empty() {
            let _ = app.emit("aqua://chats_updated", ());
            for chat in changed {
                let _ = app.emit("aqua://expired", serde_json::json!({ "peer_id": chat }));
            }
        }
    });
}

#[tauri::command]
pub fn get_chat_ttl(state: State<'_, StdMutex<AppState>>, peer_id: String) -> Result<u64, String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    Ok(ttl_of(&s, &peer_id))
}

/// Set a 1:1 chat's timer and tell the other person (and my devices).
/// Groups change theirs through the group settings.
#[tauri::command]
pub async fn set_chat_ttl(app: AppHandle, state: State<'_, StdMutex<AppState>>, peer_id: String, secs: u64) -> Result<(), String> {
    let live = {
        let mut s = state.lock().map_err(|e| e.to_string())?;
        if s.groups.contains_key(&peer_id) {
            return Err("change a group's timer in its settings".into());
        }
        apply_chat_ttl(&mut s, &peer_id, secs, "");
        s.chats.get(&peer_id).map(|c| c.summary.live).unwrap_or(false) && s.transport.is_some()
    };
    let _ = app.emit("aqua://chats_updated", ());
    if live {
        let payload = format!("{TTL_PREFIX}{}", serde_json::json!({ "secs": secs }));
        crate::multidev::fanout(&app, &peer_id, &payload, true).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::human;
    #[test]
    fn human_durations() {
        assert_eq!(human(0), "off");
        assert_eq!(human(30), "30 s");
        assert_eq!(human(300), "5 min");
        assert_eq!(human(3600), "1 h");
        assert_eq!(human(86_400), "1 d");
        assert_eq!(human(7 * 86_400), "1 wk");
        assert_eq!(human(30 * 86_400), "30 d");
    }
}
