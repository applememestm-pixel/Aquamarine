//! The same chats on every device of an account, by themselves.
//!
//! History lives on the devices (end-to-end encryption: the relay never has
//! it), so a second device used to start empty, and a message that reached
//! only one device — a contact on an old app writes to the main device
//! only, a chat started while the other device was off — stayed there.
//!
//! Now each device, a few seconds after it connects and then every few
//! minutes, sends its other devices what changed in its chats since they
//! last confirmed: the changed chats (summary + messages) and the groups,
//! as JSON → DEFLATE → a file encrypted with a fresh key (like any
//! attachment) on the relay, and the key in a `[hist:v1]` message over the
//! E2EE session to that device. The device downloads it, merges every chat
//! (missing messages go in at their place, nothing is doubled) and confirms
//! with `[histack:v1]` and the versions it took. Without a confirmation the
//! same chats go again later (the file on the relay may have expired).
//!
//! Deletions travel too: a deleted chat or message leaves a tombstone (a
//! short hash of the message) that goes with every snapshot, so the other
//! device deletes it as well and nobody sends it back.
//!
//! Only my own devices are ever sent this or believed: the payload is
//! taken only when it comes from my own account (`commands::on_incoming_text`).

use std::collections::{HashMap, HashSet};
use std::sync::Mutex as StdMutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::state::{AppState, ChatBook, ChatMessage, ChatSummary, Group, MessageKind};

pub const HIST_PREFIX: &str = "[hist:v1]";
pub const HISTACK_PREFIX: &str = "[histack:v1]";

const FILE: &str = "hist-sync.json";
const FIRST_DELAY: Duration = Duration::from_secs(8);
const EVERY: Duration = Duration::from_secs(4 * 60);
/// Sent but not confirmed: wait this long before sending again.
const RETRY_AFTER: u64 = 3 * 3600;

/// What each of my other devices has confirmed: chat id → version.
#[derive(Default, Serialize, Deserialize)]
struct SyncState {
    #[serde(default)]
    acked: HashMap<String, HashMap<String, String>>,
    /// device → unix seconds of the last unconfirmed send.
    #[serde(default)]
    pending: HashMap<String, u64>,
    /// Deleted here or on another device of mine: chat id → message hashes.
    #[serde(default)]
    tomb: HashMap<String, Vec<u64>>,
}

/// The tombstones' "version" among the chat versions.
const TOMB_KEY: &str = "\u{0}tomb";

/// Per chat at most this many tombstones (the oldest go first).
const TOMB_MAX: usize = 20_000;

#[derive(Serialize, Deserialize)]
struct ChatSnap {
    summary: ChatSummary,
    messages: Vec<ChatMessage>,
}

#[derive(Serialize, Deserialize)]
struct Snapshot {
    /// Who sent it (to confirm back to).
    from_dev: String,
    chats: Vec<ChatSnap>,
    #[serde(default)]
    groups: Vec<Group>,
    /// chat id → version, echoed in the confirmation.
    vers: HashMap<String, String>,
    /// Deleted messages (see [`tombstone`]).
    #[serde(default)]
    tomb: HashMap<String, Vec<u64>>,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn load(s: &AppState) -> SyncState {
    std::fs::read(s.data_dir.join(FILE)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save(s: &AppState, st: &SyncState) {
    if let Ok(b) = serde_json::to_vec(st) {
        let _ = std::fs::write(s.data_dir.join(FILE), b);
    }
}

/// Chats worth syncing: not devices, not on-the-fly ones.
fn syncable(id: &str) -> bool {
    id != "welcome" && (id.starts_with("dm~") || !id.contains('~'))
}

/// The same message on two devices, whatever local id it got: who, what,
/// when (to the minute — devices may show seconds or not).
fn key_of(m: &ChatMessage, me: &str) -> String {
    let who = if m.from == "me" || m.from == me { "me" } else { m.from.as_str() };
    let when: String = m.time.chars().take(5).collect();
    let what = match &m.kind {
        // An edited message: compare by what it first said.
        MessageKind::Text { text } => m.meta.as_ref().and_then(|x| x.edits.first()).cloned().unwrap_or_else(|| text.clone()),
        MessageKind::File { cid, .. } => format!("file:{cid}"),
        MessageKind::DateSeparator { label } => return format!("date:{label}"),
        other => serde_json::to_string(other).unwrap_or_default(),
    };
    format!("{who}\u{1}{when}\u{1}{what}")
}

/// A short hash of a message, the same on every device and build.
fn hash_of(m: &ChatMessage, me: &str) -> u64 {
    use sha2::Digest;
    let d = sha2::Sha256::digest(key_of(m, me).as_bytes());
    u64::from_be_bytes([d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]])
}

/// These messages of `chat` were deleted on this device: my other devices
/// delete them too and never send them back.
pub fn tombstone(s: &AppState, chat: &str, msgs: &[ChatMessage]) {
    if msgs.is_empty() || !syncable(chat) { return; }
    let me = s.username.clone().unwrap_or_default();
    let mut st = load(s);
    let t = st.tomb.entry(chat.to_string()).or_default();
    for m in msgs {
        let h = hash_of(m, &me);
        if !t.contains(&h) { t.push(h); }
    }
    if t.len() > TOMB_MAX { let cut = t.len() - TOMB_MAX; t.drain(..cut); }
    // The chat changed: everyone gets it again (with the tombstones).
    for a in st.acked.values_mut() { a.remove(chat); }
    save(s, &st);
}

/// A chat's version: how many messages and what the last one is.
fn version(book: &ChatBook, me: &str) -> String {
    let last = book.messages.last().map(|m| key_of(m, me)).unwrap_or_default();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    std::hash::Hash::hash(&last, &mut h);
    format!("{}:{:x}", book.messages.len(), std::hash::Hasher::finish(&h))
}

/// Merge `theirs` into `ours`, keeping our order and putting each message we
/// don't have right after the last one we both have. Identical messages are
/// counted, so two "ok" at the same minute stay two. Returns how many came in.
pub(crate) fn merge(ours: &mut Vec<ChatMessage>, theirs: Vec<ChatMessage>, me: &str) -> usize {
    let keys: Vec<String> = ours.iter().map(|m| key_of(m, me)).collect();
    // Where each key occurs in ours, in order.
    let mut at: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, k) in keys.iter().enumerate() {
        at.entry(k.as_str()).or_default().push(i);
    }
    let mut used: HashSet<usize> = HashSet::new();
    let mut inserts: Vec<(usize, ChatMessage)> = Vec::new(); // (insert before index, message)
    let mut cursor = 0usize; // ours[..cursor] is matched up
    for m in theirs {
        let k = key_of(&m, me);
        let hit = at.get(k.as_str()).and_then(|v| v.iter().copied().find(|&i| !used.contains(&i) && i >= cursor))
            .or_else(|| at.get(k.as_str()).and_then(|v| v.iter().copied().find(|&i| !used.contains(&i))));
        match hit {
            Some(i) => {
                used.insert(i);
                if i >= cursor { cursor = i + 1; }
            }
            None => {
                let mut m = m;
                // Their local copy of a file isn't on this device.
                if let MessageKind::File { saved_at, .. } = &mut m.kind {
                    if saved_at.as_deref().is_some_and(|p| !std::path::Path::new(p).exists()) { *saved_at = None; }
                }
                m.id = format!("h-{}", crate::state::uuid_like());
                inserts.push((cursor, m));
            }
        }
    }
    let n = inserts.len();
    if n == 0 {
        return 0;
    }
    let mut out = Vec::with_capacity(ours.len() + n);
    let mut ins = inserts.into_iter().peekable();
    for (i, m) in std::mem::take(ours).into_iter().enumerate() {
        while ins.peek().is_some_and(|(at, _)| *at <= i) {
            out.push(ins.next().unwrap().1);
        }
        out.push(m);
    }
    out.extend(ins.map(|(_, m)| m));
    *ours = out;
    n
}

/// Start the background sync (after connecting; safe to call again).
pub fn start(app: &AppHandle) {
    static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if STARTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        // Already running: just sync soon (a new connection).
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(FIRST_DELAY).await;
            sync_now(&app).await;
        });
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_DELAY).await;
        loop {
            sync_now(&app).await;
            tokio::time::sleep(EVERY).await;
        }
    });
}

/// Send my other devices the chats they haven't confirmed in this version.
pub async fn sync_now(app: &AppHandle) {
    let state: State<'_, StdMutex<AppState>> = app.state();
    let Some((transport, me)) = state.lock().ok().and_then(|s| Some((s.transport.clone()?, s.username.clone()?))) else { return };
    let addrs = crate::multidev::addresses(&transport, &me).await;
    let mine = match state.lock() { Ok(s) => crate::multidev::my_address(&s, &addrs), Err(_) => return };
    let others: Vec<String> = addrs.into_iter().filter(|a| *a != mine && crate::multidev::base_user(a) == me).collect();
    if others.is_empty() {
        return;
    }
    for dev in others {
        // What this device lacks, as of now.
        let snap = {
            let Ok(s) = state.lock() else { return };
            let st = load(&s);
            if st.pending.get(&dev).is_some_and(|t| now().saturating_sub(*t) < RETRY_AFTER) {
                continue;
            }
            match collect(&s, &st, &dev, &mine) { Some(x) => x, None => continue }
        };
        let n = snap.chats.len();
        match send_snapshot(app, &transport, &dev, &snap).await {
            Ok(()) => {
                tracing::info!(device = %dev, chats = n, "history sent to my other device");
                if let Ok(s) = state.lock() {
                    let mut st = load(&s);
                    st.pending.insert(dev.clone(), now());
                    save(&s, &st);
                }
            }
            Err(e) => tracing::warn!(device = %dev, %e, "history sync: not sent"),
        }
    }
}

/// What `dev` hasn't confirmed: changed chats, all groups, tombstones.
fn collect(s: &AppState, st: &SyncState, dev: &str, mine: &str) -> Option<Snapshot> {
    let me = s.username.clone().unwrap_or_default();
    let acked = st.acked.get(dev);
    let mut chats = Vec::new();
    let mut vers = HashMap::new();
    for (id, book) in &s.chats {
        if !syncable(id) || book.messages.is_empty() || *id == me { continue; }
        let v = version(book, &me);
        if acked.and_then(|a| a.get(id)) == Some(&v) { continue; }
        vers.insert(id.clone(), v);
        chats.push(ChatSnap { summary: book.summary.clone(), messages: book.messages.clone() });
    }
    // Deletions alone are worth a snapshot too.
    let tomb_n: usize = st.tomb.values().map(|v| v.len()).sum();
    let tomb_v = tomb_n.to_string();
    if tomb_n > 0 && acked.and_then(|a| a.get(TOMB_KEY)) != Some(&tomb_v) { vers.insert(TOMB_KEY.to_string(), tomb_v); }
    if chats.is_empty() && !vers.contains_key(TOMB_KEY) { return None; }
    Some(Snapshot { from_dev: mine.to_string(), chats, groups: s.groups.values().cloned().collect(), vers, tomb: st.tomb.clone() })
}

async fn send_snapshot(app: &AppHandle, transport: &aqua_client_core::Transport, dev: &str, snap: &Snapshot) -> Result<(), String> {
    let json = serde_json::to_vec(snap).map_err(|e| e.to_string())?;
    let packed = crate::history::compress(&json);
    let (env, cipher) = aqua_core::file_transfer::encrypt_file(&packed, "aqua-history.json.deflate", Some("application/octet-stream".into()))
        .map_err(|e| e.to_string())?;
    transport.upload_blob_kind(&env.cid, cipher, false, Some("file")).await.map_err(|e| e.to_string())?;
    let payload = format!("{HIST_PREFIX}{}", env.to_message_text());
    crate::multidev::send_to_address(app, dev, &payload).await
}

/// `[hist:v1]…` from my other device: fetch, merge, confirm.
pub fn on_snapshot(app: &AppHandle, body: &str) {
    let Ok(env) = aqua_core::file_transfer::FileEnvelope::from_message_text(body) else { return };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = receive(&app, env).await {
            tracing::warn!(%e, "history from my other device: not taken");
        }
    });
}

async fn receive(app: &AppHandle, env: aqua_core::file_transfer::FileEnvelope) -> Result<(), String> {
    let state: State<'_, StdMutex<AppState>> = app.state();
    let transport = state.lock().map_err(|e| e.to_string())?.transport.clone().ok_or("not connected")?;
    let cipher = transport.download_blob(&env.cid).await.map_err(|e| e.to_string())?;
    let packed = aqua_core::file_transfer::decrypt_file(&env, &cipher).map_err(|e| e.to_string())?;
    let json = crate::history::decompress(&packed).ok_or("bad history file")?;
    let snap: Snapshot = serde_json::from_slice(&json).map_err(|e| e.to_string())?;
    let (added, chats) = {
        let mut s = state.lock().map_err(|e| e.to_string())?;
        let r = apply(&mut s, snap.chats, snap.groups, &snap.tomb);
        r
    };
    if added > 0 || chats > 0 {
        tracing::info!(added, chats, "history merged from my other device");
        let _ = app.emit("aqua://chats_updated", ());
        let _ = app.emit("aqua://groups_updated", ());
    }
    // Confirm what was taken, so the same version isn't sent again.
    let ack = format!("{HISTACK_PREFIX}{}", serde_json::json!({ "vers": snap.vers }));
    crate::multidev::send_to_address(app, &snap.from_dev, &ack).await
}

/// Merge chats and groups from another device of mine. Returns (messages
/// added, chats created).
fn apply(s: &mut AppState, chats: Vec<ChatSnap>, groups: Vec<Group>, their_tomb: &HashMap<String, Vec<u64>>) -> (usize, usize) {
    let me = s.username.clone().unwrap_or_default();
    // Their deletions become ours: drop those messages here, remember them.
    let mut st = load(s);
    for (chat, hashes) in their_tomb {
        let t = st.tomb.entry(chat.clone()).or_default();
        for h in hashes { if !t.contains(h) { t.push(*h); } }
        if t.len() > TOMB_MAX { let cut = t.len() - TOMB_MAX; t.drain(..cut); }
    }
    let tomb: HashMap<String, HashSet<u64>> = st.tomb.iter().map(|(k, v)| (k.clone(), v.iter().copied().collect())).collect();
    save(s, &st);
    let mut emptied = Vec::new();
    for (chat, dead) in &tomb {
        let Some(book) = s.chats.get_mut(chat) else { continue };
        let before = book.messages.len();
        book.messages.retain(|m| !dead.contains(&hash_of(m, &me)));
        if book.messages.len() != before {
            book.summary.preview = book.messages.last().map(|m| crate::state::preview_for(&m.kind)).unwrap_or_default();
            if book.messages.is_empty() && !chat.starts_with("grp_") && chat != crate::state::SAVED_MESSAGES_ID { emptied.push(chat.clone()); }
        }
    }
    for chat in &emptied {
        // A chat deleted on my other device.
        s.chats.remove(chat);
        s.chat_order.retain(|c| c != chat);
    }
    // Groups first: a group chat without its group is just a list.
    for g in groups {
        let newer = s.groups.get(&g.id).map(|cur| g.version > cur.version).unwrap_or(true);
        if newer { s.groups.insert(g.id.clone(), g); }
    }
    let (mut added, mut created) = (0usize, 0usize);
    for mut c in chats {
        let id = c.summary.id.clone();
        if !syncable(&id) || id == me { continue; }
        if let Some(dead) = tomb.get(&id) {
            c.messages.retain(|m| !dead.contains(&hash_of(m, &me)));
            if c.messages.is_empty() { continue; }
        }
        match s.chats.get_mut(&id) {
            Some(book) => {
                let n = merge(&mut book.messages, c.messages, &me);
                if n > 0 {
                    added += n;
                    if let Some(last) = book.messages.last() {
                        book.summary.preview = crate::state::preview_for(&last.kind);
                        book.summary.preview_by_me = last.from == "me" || last.from == me;
                        book.summary.time = last.time.clone();
                    }
                    crate::history::mark_dirty(s, &id);
                }
            }
            None => {
                let mut messages = Vec::new();
                added += merge(&mut messages, c.messages, &me);
                let mut summary = c.summary;
                summary.preview_is_typing = false;
                s.chats.insert(id.clone(), ChatBook { summary, messages });
                if !s.chat_order.contains(&id) { s.chat_order.push(id.clone()); }
                created += 1;
            }
        }
    }
    (added, created)
}

/// `[histack:v1]{vers}` from my other device: these versions are there now.
pub fn on_ack(app: &AppHandle, from_dev: Option<&str>, body: &str) {
    let state: State<'_, StdMutex<AppState>> = app.state();
    let Ok(s) = state.lock() else { return };
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    let Some(vers) = v.get("vers").and_then(|x| x.as_object()) else { return };
    let mut st = load(&s);
    // Without the sender's address (older path), confirm for every device
    // that has a send pending — there's rarely more than one.
    let devs: Vec<String> = match from_dev {
        Some(d) => vec![d.to_string()],
        None => st.pending.keys().cloned().collect(),
    };
    for dev in devs {
        let e = st.acked.entry(dev.clone()).or_default();
        for (id, ver) in vers {
            if let Some(ver) = ver.as_str() { e.insert(id.clone(), ver.to_string()); }
        }
        st.pending.remove(&dev);
    }
    save(&s, &st);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(from: &str, time: &str, text: &str) -> ChatMessage {
        ChatMessage { id: format!("x-{text}"), from: from.into(), time: time.into(), kind: MessageKind::Text { text: text.into() }, meta: None }
    }
    fn texts(v: &[ChatMessage]) -> Vec<String> {
        v.iter().map(|m| match &m.kind { MessageKind::Text { text } => text.clone(), _ => String::new() }).collect()
    }

    #[test]
    fn merge_puts_missing_messages_in_place_and_doubles_nothing() {
        let mut ours = vec![msg("ann", "10:00", "a"), msg("me", "10:01", "b"), msg("ann", "10:05", "e")];
        let theirs = vec![
            msg("ann", "10:00", "a"), msg("vlad", "10:01:33", "b"), // "me" under my name, with seconds
            msg("ann", "10:02", "c"), msg("ann", "10:03", "d"),
            msg("ann", "10:05", "e"), msg("ann", "10:06", "f"),
        ];
        assert_eq!(merge(&mut ours, theirs, "vlad"), 3);
        assert_eq!(texts(&ours), ["a", "b", "c", "d", "e", "f"]);
        // Again: nothing new.
        let again = ours.clone();
        assert_eq!(merge(&mut ours, again, "vlad"), 0);
        assert_eq!(ours.len(), 6);
    }

    fn device(name: &str) -> AppState {
        let dir = std::env::temp_dir().join(format!("aqua-hs-{name}-{}", crate::state::uuid_like()));
        std::fs::create_dir_all(&dir).unwrap();
        let s = AppState::boot(dir).unwrap().into_inner().unwrap();
        let mut s = s;
        s.username = Some("vlad".into());
        s
    }
    fn chat(s: &mut AppState, id: &str, msgs: Vec<ChatMessage>) {
        let summary = crate::state::summary_for(id, "", false, false);
        s.chats.insert(id.into(), ChatBook { summary, messages: msgs });
        s.chat_order.push(id.into());
    }
    fn sync(from: &AppState, to: &mut AppState) -> (usize, usize) {
        let st = load(from);
        let Some(snap) = collect(from, &st, "vlad~b", "vlad") else { return (0, 0) };
        apply(to, snap.chats, snap.groups, &snap.tomb)
    }

    /// A phone joins: it gets the PC's chats; what only the phone had goes to
    /// the PC; a chat deleted on the PC goes from the phone too.
    #[test]
    fn two_devices_end_up_with_the_same_chats() {
        let (mut pc, mut phone) = (device("pc"), device("phone"));
        chat(&mut pc, "ann", vec![msg("ann", "09:00", "hi"), msg("me", "09:01", "hello"), msg("ann", "09:02", "how are you")]);
        chat(&mut pc, "bob", vec![msg("bob", "11:00", "yo")]);
        chat(&mut phone, "ann", vec![msg("ann", "09:00", "hi"), msg("ann", "09:30", "only on the phone")]);
        chat(&mut phone, "ppp", vec![msg("vlad", "08:00", "board")]);

        assert_eq!(sync(&pc, &mut phone), (3, 1));
        assert_eq!(texts(&phone.chats["ann"].messages), ["hi", "hello", "how are you", "only on the phone"]);
        assert!(phone.chats.contains_key("bob"));
        assert_eq!(sync(&phone, &mut pc), (2, 1));
        assert!(pc.chats.contains_key("ppp"));
        assert_eq!(pc.chats["ann"].messages.len(), 4);
        // Nothing new either way now.
        assert_eq!(sync(&pc, &mut phone).0, 0);

        // Deleted on the PC: gone on the phone, and the phone doesn't bring it back.
        let gone = pc.chats["bob"].messages.clone();
        tombstone(&pc, "bob", &gone);
        pc.chats.remove("bob");
        sync(&pc, &mut phone);
        assert!(!phone.chats.contains_key("bob"));
        assert_eq!(sync(&phone, &mut pc).1, 0);
        assert!(!pc.chats.contains_key("bob"));
        // No chat with myself.
        chat(&mut phone, "vlad", vec![msg("vlad", "07:00", "x")]);
        sync(&phone, &mut pc);
        assert!(!pc.chats.contains_key("vlad"));
    }

    #[test]
    fn hashes_are_the_same_with_or_without_seconds() {
        assert_eq!(hash_of(&msg("me", "10:01", "b"), "vlad"), hash_of(&msg("vlad", "10:01:59", "b"), "vlad"));
        assert_ne!(hash_of(&msg("ann", "10:01", "b"), "vlad"), hash_of(&msg("ann", "10:02", "b"), "vlad"));
    }

    #[test]
    fn the_same_words_twice_stay_twice() {
        let mut ours = vec![msg("ann", "10:00", "ok")];
        let theirs = vec![msg("ann", "10:00", "ok"), msg("ann", "10:00", "ok")];
        assert_eq!(merge(&mut ours, theirs, "me"), 1);
        assert_eq!(ours.len(), 2);
    }
}
