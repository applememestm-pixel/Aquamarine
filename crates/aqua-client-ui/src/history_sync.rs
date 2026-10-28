//! The same chats on every device of an account — stored by the relay,
//! encrypted so that only the account's devices can read them.
//!
//! History lives on the devices (end-to-end encryption), so a second device
//! used to start empty, and a message that reached only one device (a
//! contact on an old app writes to the main device only) stayed there. Now:
//!
//! * The account has a **history key** (32 random bytes) that only its
//!   devices know: the first device makes it and the relay keeps just its id
//!   (`kid`); a new device asks my other devices for it over E2EE
//!   (`[hkeyreq:v1]` → `[hkey:v1]`), or finds it in the backup it restored.
//! * Every chat is stored on the relay as pages of 200 messages plus a card
//!   (name, look, deletions), and the groups as one record — each JSON →
//!   DEFLATE → ChaCha20-Poly1305 with the history key; the record's name is
//!   a keyed hash, so the relay doesn't learn who the chats are with.
//!   `PUT /v1/me/hist/:k`; the relay numbers every change (`seq`).
//! * A device uploads what changed within ~2 s and pokes my other devices
//!   (`[hpoke:v1]` over E2EE); they fetch "everything since N" at once. A new
//!   device fetches everything right after signing in. Every minute it looks
//!   anyway, in case a poke was missed.
//! * Merging puts the messages it doesn't have at their place, never twice.
//!   Deleted chats and messages leave tombstones (8 bytes of a hash) in the
//!   chat's card, so the other devices delete them too and nothing returns.
//!
//! Only my own devices are believed: the control messages are taken only
//! from my account (`commands::on_incoming_text`).

use std::collections::{HashMap, HashSet};
use std::sync::Mutex as StdMutex;
use std::time::Duration;

use base64::Engine;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::state::{AppState, ChatBook, ChatMessage, ChatSummary, Group, MessageKind};

pub const HKEYREQ_PREFIX: &str = "[hkeyreq:v1]";
pub const HKEY_PREFIX: &str = "[hkey:v1]";
pub const HPOKE_PREFIX: &str = "[hpoke:v1]";

const KEY_REC: &str = "histsync:key";
const FILE: &str = "hist-sync.json";
const PAGE: usize = 200;
/// Per chat at most this many tombstones (the oldest go first).
const TOMB_MAX: usize = 20_000;

static PULL_NOW: tokio::sync::Notify = tokio::sync::Notify::const_new();
/// chat id → (version of what was last checked for upload); "\0groups" too.
static PUSHED: StdMutex<Option<HashMap<String, u64>>> = StdMutex::new(None);

#[derive(Default, Serialize, Deserialize)]
struct SyncState {
    /// server url → the last seq seen there.
    #[serde(default)]
    cursor: HashMap<String, i64>,
    /// record key → hash of what's on the relay (as far as this device knows).
    #[serde(default)]
    uploaded: HashMap<String, u64>,
    /// Deleted here or on another device of mine: chat id → message hashes.
    #[serde(default)]
    tomb: HashMap<String, Vec<u64>>,
}

/// The lasting part of a chat's card (not unread counts, previews, online…).
#[derive(Clone, Serialize, Deserialize, Hash)]
struct Card {
    id: String,
    name: String,
    letter: String,
    gradient: String,
    has_lock: bool,
    verified: bool,
    channel: bool,
    live: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "t")]
enum Rec {
    Page { chat: String, page: u32, msgs: Vec<ChatMessage> },
    Meta { chat: String, card: Option<Card>, #[serde(default)] tomb: Vec<u64> },
    Groups { groups: Vec<Group> },
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

fn h64(bytes: &[u8]) -> u64 {
    use sha2::Digest;
    let d = sha2::Sha256::digest(bytes);
    u64::from_be_bytes([d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]])
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
    h64(key_of(m, me).as_bytes())
}

/// What changed in a chat, cheaply: count, last message, card.
fn version(book: &ChatBook, me: &str) -> u64 {
    let last = book.messages.last().map(|m| key_of(m, me)).unwrap_or_default();
    let card = serde_json::to_string(&card_of(&book.summary)).unwrap_or_default();
    h64(format!("{}\u{1}{last}\u{1}{card}", book.messages.len()).as_bytes())
}

fn card_of(s: &ChatSummary) -> Card {
    Card { id: s.id.clone(), name: s.name.clone(), letter: s.letter.clone(), gradient: s.gradient.clone(),
           has_lock: s.has_lock, verified: s.verified, channel: s.channel, live: s.live }
}

// ---------- the history key ----------

fn history_key(s: &AppState) -> Option<[u8; 32]> {
    s.vault.get_record(KEY_REC).ok().flatten().and_then(|b| <[u8; 32]>::try_from(b.as_slice()).ok())
}

fn kid_of(key: &[u8; 32]) -> String {
    use sha2::Digest;
    hex::encode(&sha2::Sha256::new().chain_update(b"aqua-hist-kid\0").chain_update(key).finalize()[..16])
}

/// The relay's name for a record: a keyed hash, so it can't tell the chats.
fn rec_key(key: &[u8; 32], label: &str) -> String {
    use sha2::Digest;
    hex::encode(&sha2::Sha256::new().chain_update(key).chain_update(b"\0").chain_update(label.as_bytes()).finalize()[..20])
}

fn seal(key: &[u8; 32], k: &str, plain: &[u8]) -> Option<Vec<u8>> {
    let mut nonce = [0u8; 12];
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut nonce);
    let ct = aqua_core::crypto::aead_encrypt(key, &nonce, k.as_bytes(), &crate::history::compress(plain)).ok()?;
    Some([nonce.as_slice(), &ct].concat())
}

fn open(key: &[u8; 32], k: &str, blob: &[u8]) -> Option<Vec<u8>> {
    if blob.len() < 12 { return None; }
    let nonce: [u8; 12] = blob[..12].try_into().ok()?;
    let packed = aqua_core::crypto::aead_decrypt(key, &nonce, k.as_bytes(), &blob[12..]).ok()?;
    crate::history::decompress(&packed)
}

/// Make sure this device has the account's history key: it's here, or it's
/// made now (first device), or my other devices are asked for it.
async fn ensure_key(app: &AppHandle, transport: &aqua_client_core::Transport, me: &str) -> Option<[u8; 32]> {
    let state: State<'_, StdMutex<AppState>> = app.state();
    if let Some(k) = state.lock().ok().and_then(|s| history_key(&s)) {
        return Some(k);
    }
    match transport.hist_get("kid").await {
        Ok(None) => {
            let mut key = [0u8; 32];
            rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut key);
            match transport.hist_put("kid", kid_of(&key).into_bytes()).await {
                Ok(_) => {
                    let s = state.lock().ok()?;
                    s.vault.put_record(KEY_REC, &key).ok()?;
                    tracing::info!("history key made (first device of the account)");
                    return Some(key);
                }
                Err(e) => tracing::info!(%e, "history key: another device was first"),
            }
        }
        Ok(Some(_)) => {}
        Err(e) => {
            tracing::warn!(%e, "history store not reachable (older relay?)");
            return None;
        }
    }
    // Another device has it: ask them (they answer within seconds if online).
    crate::multidev::to_my_devices(app, transport, me, HKEYREQ_PREFIX).await;
    None
}

/// `[hkeyreq:v1]` from my other device: give it the key.
pub fn on_key_request(app: &AppHandle, from_addr: &str) {
    let state: State<'_, StdMutex<AppState>> = app.state();
    let Some(key) = state.lock().ok().and_then(|s| history_key(&s)) else { return };
    let payload = format!("{HKEY_PREFIX}{}", serde_json::json!({ "k": base64::engine::general_purpose::STANDARD.encode(key) }));
    let (app, addr) = (app.clone(), from_addr.to_string());
    tauri::async_runtime::spawn(async move {
        if let Err(e) = crate::multidev::send_to_own(&app, &addr, &payload).await {
            tracing::warn!(%e, "history key not sent to my device");
        }
    });
}

/// `[hkey:v1]` from my other device: keep it if it's the account's key.
pub fn on_key(app: &AppHandle, body: &str) {
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    let Some(key) = v["k"].as_str()
        .and_then(|b| base64::engine::general_purpose::STANDARD.decode(b).ok())
        .and_then(|b| <[u8; 32]>::try_from(b.as_slice()).ok()) else { return };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state: State<'_, StdMutex<AppState>> = app.state();
        let Some(t) = state.lock().ok().and_then(|s| s.transport.clone()) else { return };
        // Only the key whose id the relay has (the account's one).
        let ok = matches!(t.hist_get("kid").await, Ok(Some(kid)) if kid == kid_of(&key).as_bytes());
        if !ok { return; }
        if let Ok(s) = state.lock() {
            if history_key(&s).is_none() { let _ = s.vault.put_record(KEY_REC, &key); }
        }
        tracing::info!("history key received from my other device");
        PULL_NOW.notify_one();
    });
}

/// `[hpoke:v1]` from my other device: it just stored something — fetch now.
pub fn on_poke() {
    PULL_NOW.notify_one();
}

/// These messages of `chat` were deleted on this device: my other devices
/// delete them too and never bring them back.
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
    save(s, &st);
    if let Ok(mut p) = PUSHED.lock() { p.get_or_insert_with(HashMap::new).remove(chat); }
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


// ---------- the loop ----------

/// Start syncing (after connecting; calling it again just syncs now).
pub fn start(app: &AppHandle) {
    static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    PULL_NOW.notify_one();
    if STARTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut last_pull = std::time::Instant::now() - Duration::from_secs(3600);
        loop {
            let pull_due = last_pull.elapsed() >= Duration::from_secs(60);
            let poked = tokio::time::timeout(Duration::from_secs(2), PULL_NOW.notified()).await.is_ok();
            let state: State<'_, StdMutex<AppState>> = app.state();
            let Some((t, me)) = state.lock().ok().and_then(|s| Some((s.transport.clone()?, s.username.clone()?))) else { continue };
            let Some(key) = ensure_key(&app, &t, &me).await else {
                // Waiting for the key from my other device.
                tokio::time::sleep(Duration::from_secs(8)).await;
                continue;
            };
            if poked || pull_due {
                if let Err(e) = pull(&app, &t, &key).await { tracing::warn!(%e, "history: fetch failed"); }
                last_pull = std::time::Instant::now();
            }
            match push(&app, &t, &key).await {
                Ok(0) => {}
                Ok(n) => {
                    tracing::info!(records = n, "history stored on the relay");
                    crate::multidev::to_my_devices(&app, &t, &me, HPOKE_PREFIX).await;
                }
                Err(e) => tracing::warn!(%e, "history: store failed"),
            }
        }
    });
}

/// One record to store: key, plaintext, its hash.
type Out = (String, Vec<u8>, u64);

/// What's new here goes to the relay. Returns how many records went.
async fn push(app: &AppHandle, t: &aqua_client_core::Transport, key: &[u8; 32]) -> Result<usize, String> {
    let state: State<'_, StdMutex<AppState>> = app.state();
    let (outs, versions) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        let me = s.username.clone().unwrap_or_default();
        let st = load(&s);
        let pushed = PUSHED.lock().map_err(|e| e.to_string())?.clone().unwrap_or_default();
        let mut outs: Vec<Out> = Vec::new();
        let mut versions = HashMap::new();
        let add = |label: String, rec: &Rec, outs: &mut Vec<Out>| {
            let Ok(plain) = serde_json::to_vec(rec) else { return };
            let k = rec_key(key, &label);
            let h = h64(&plain);
            if st.uploaded.get(&k) != Some(&h) { outs.push((k, plain, h)); }
        };
        for (id, book) in &s.chats {
            if !syncable(id) || *id == me { continue; }
            let v = version(book, &me);
            if pushed.get(id) == Some(&v) { continue; }
            versions.insert(id.clone(), v);
            for (i, chunk) in book.messages.chunks(PAGE).enumerate() {
                add(format!("p:{id}:{i}"), &Rec::Page { chat: id.clone(), page: i as u32, msgs: chunk.to_vec() }, &mut outs);
            }
            let tomb = st.tomb.get(id).cloned().unwrap_or_default();
            add(format!("m:{id}"), &Rec::Meta { chat: id.clone(), card: Some(card_of(&book.summary)), tomb }, &mut outs);
        }
        // Chats deleted here: their card carries the tombstones.
        for (id, tomb) in &st.tomb {
            if s.chats.contains_key(id) || pushed.contains_key(&format!("\u{0}gone:{id}")) { continue; }
            versions.insert(format!("\u{0}gone:{id}"), tomb.len() as u64);
            add(format!("m:{id}"), &Rec::Meta { chat: id.clone(), card: None, tomb: tomb.clone() }, &mut outs);
        }
        let mut groups: Vec<&Group> = s.groups.values().collect();
        groups.sort_by(|a, b| a.id.cmp(&b.id));
        let gv = h64(&serde_json::to_vec(&groups).unwrap_or_default());
        if pushed.get("\u{0}groups") != Some(&gv) {
            versions.insert("\u{0}groups".into(), gv);
            add("groups".into(), &Rec::Groups { groups: groups.into_iter().cloned().collect() }, &mut outs);
        }
        (outs, versions)
    };
    let mut sent = 0usize;
    let mut done: Vec<(String, u64)> = Vec::new();
    for (k, plain, h) in outs {
        let Some(blob) = seal(key, &k, &plain) else { continue };
        match t.hist_put(&k, blob).await {
            Ok(_) => { done.push((k, h)); sent += 1; }
            Err(e) => {
                // Not stored: try again next round (versions aren't marked).
                save_uploaded(app, &done);
                return Err(e.to_string());
            }
        }
    }
    save_uploaded(app, &done);
    if let Ok(mut p) = PUSHED.lock() { p.get_or_insert_with(HashMap::new).extend(versions); }
    Ok(sent)
}

fn save_uploaded(app: &AppHandle, done: &[(String, u64)]) {
    if done.is_empty() { return; }
    let state: State<'_, StdMutex<AppState>> = app.state();
    if let Ok(s) = state.lock() {
        let mut st = load(&s);
        for (k, h) in done { st.uploaded.insert(k.clone(), *h); }
        save(&s, &st);
    };
}

/// Everything stored since we last looked, merged in.
async fn pull(app: &AppHandle, t: &aqua_client_core::Transport, key: &[u8; 32]) -> Result<(), String> {
    let state: State<'_, StdMutex<AppState>> = app.state();
    let server = t.server_url().trim_end_matches('/').to_string();
    let mut st = state.lock().map(|s| load(&s)).map_err(|e| e.to_string())?;
    let mut since = st.cursor.get(&server).copied().unwrap_or(0);
    let mut changed = false;
    loop {
        let list = t.hist_list(since).await.map_err(|e| e.to_string())?;
        if list.is_empty() { break; }
        let n = list.len();
        for (k, seq) in list {
            since = since.max(seq);
            if k == "kid" { continue; }
            let Ok(Some(blob)) = t.hist_get(&k).await else { continue };
            let Some(plain) = open(key, &k, &blob) else { continue };
            let Ok(rec) = serde_json::from_slice::<Rec>(&plain) else { continue };
            let mut s = state.lock().map_err(|e| e.to_string())?;
            let local = apply(&mut s, &mut st, rec, key);
            // What's on the relay is what we have now (or we upload the
            // merge next round — once, then it settles).
            st.uploaded.insert(k.clone(), local.unwrap_or_else(|| h64(&plain)));
            changed = true;
        }
        st.cursor.insert(server.clone(), since);
        if let Ok(s) = state.lock() {
            // Tombstones made meanwhile on this device stay.
            let disk = load(&s);
            for (chat, t) in disk.tomb {
                let mine = st.tomb.entry(chat).or_default();
                for h in t { if !mine.contains(&h) { mine.push(h); } }
            }
            save(&s, &st);
        }
        if n < 2000 { break; }
    }
    if changed {
        let _ = app.emit("aqua://chats_updated", ());
        let _ = app.emit("aqua://groups_updated", ());
    }
    Ok(())
}

/// Merge one record. Returns the hash of the local record with the same
/// key if the merge changed nothing here (so it isn't uploaded back).
fn apply(s: &mut AppState, st: &mut SyncState, rec: Rec, key: &[u8; 32]) -> Option<u64> {
    let me = s.username.clone().unwrap_or_default();
    match rec {
        Rec::Groups { groups } => {
            for g in groups {
                let newer = s.groups.get(&g.id).map(|cur| g.version > cur.version).unwrap_or(true);
                if newer { s.groups.insert(g.id.clone(), g); }
            }
            None
        }
        Rec::Meta { chat, card, tomb } => {
            if !syncable(&chat) || chat == me { return None; }
            let t = st.tomb.entry(chat.clone()).or_default();
            for h in &tomb { if !t.contains(h) { t.push(*h); } }
            if t.len() > TOMB_MAX { let cut = t.len() - TOMB_MAX; t.drain(..cut); }
            let dead: HashSet<u64> = t.iter().copied().collect();
            let mut removed = false;
            if let Some(book) = s.chats.get_mut(&chat) {
                let before = book.messages.len();
                book.messages.retain(|m| !dead.contains(&hash_of(m, &me)));
                if book.messages.len() != before {
                    book.summary.preview = book.messages.last().map(|m| crate::state::preview_for(&m.kind)).unwrap_or_default();
                    crate::history::mark_dirty(s, &chat);
                    removed = s.chats.get(&chat).is_some_and(|b| b.messages.is_empty());
                }
            }
            if removed && card.is_none() && !chat.starts_with("grp_") && chat != crate::state::SAVED_MESSAGES_ID {
                // Deleted on my other device.
                s.chats.remove(&chat);
                s.chat_order.retain(|c| *c != chat);
                return None;
            }
            match (s.chats.get_mut(&chat), card) {
                (Some(book), Some(c)) => {
                    // Their card names the chat; the rest stays ours.
                    if book.summary.name == book.summary.id && c.name != c.id { book.summary.name = c.name; }
                    let local = Rec::Meta { chat: chat.clone(), card: Some(card_of(&book.summary)), tomb: st.tomb.get(&chat).cloned().unwrap_or_default() };
                    serde_json::to_vec(&local).ok().map(|p| h64(&p))
                }
                (None, Some(c)) if !c.id.is_empty() => {
                    let mut summary = crate::state::summary_for(&chat, "", false, false);
                    summary.name = c.name; summary.letter = c.letter; summary.gradient = c.gradient;
                    summary.has_lock = c.has_lock; summary.verified = c.verified; summary.channel = c.channel; summary.live = c.live;
                    s.chats.insert(chat.clone(), ChatBook { summary, messages: Vec::new() });
                    if !s.chat_order.contains(&chat) { s.chat_order.push(chat.clone()); }
                    None
                }
                _ => None,
            }
        }
        Rec::Page { chat, page, mut msgs } => {
            if !syncable(&chat) || chat == me { return None; }
            if let Some(dead) = st.tomb.get(&chat) {
                let dead: HashSet<u64> = dead.iter().copied().collect();
                msgs.retain(|m| !dead.contains(&hash_of(m, &me)));
            }
            if !s.chats.contains_key(&chat) {
                if msgs.is_empty() { return None; }
                let summary = crate::state::summary_for(&chat, "", false, false);
                s.chats.insert(chat.clone(), ChatBook { summary, messages: Vec::new() });
                if !s.chat_order.contains(&chat) { s.chat_order.push(chat.clone()); }
            }
            let book = s.chats.get_mut(&chat)?;
            let n = merge(&mut book.messages, msgs, &me);
            if n > 0 {
                if let Some(last) = book.messages.last() {
                    book.summary.preview = crate::state::preview_for(&last.kind);
                    book.summary.preview_by_me = last.from == "me" || last.from == me;
                    book.summary.time = last.time.clone();
                }
                crate::history::mark_dirty(s, &chat);
                return None;
            }
            // Nothing new: our page with this number counts as stored.
            let chunk = book.messages.chunks(PAGE).nth(page as usize)?.to_vec();
            let _ = key;
            serde_json::to_vec(&Rec::Page { chat, page, msgs: chunk }).ok().map(|p| h64(&p))
        }
    }
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
        let again = ours.clone();
        assert_eq!(merge(&mut ours, again, "vlad"), 0);
        assert_eq!(ours.len(), 6);
    }

    #[test]
    fn the_same_words_twice_stay_twice() {
        let mut ours = vec![msg("ann", "10:00", "ok")];
        let theirs = vec![msg("ann", "10:00", "ok"), msg("ann", "10:00", "ok")];
        assert_eq!(merge(&mut ours, theirs, "me"), 1);
        assert_eq!(ours.len(), 2);
    }

    #[test]
    fn records_open_only_with_the_key_and_their_own_name() {
        let key = [7u8; 32];
        let k = rec_key(&key, "p:ann:0");
        let blob = seal(&key, &k, b"hello").unwrap();
        assert_eq!(open(&key, &k, &blob).as_deref(), Some(&b"hello"[..]));
        assert!(open(&[8u8; 32], &k, &blob).is_none(), "another key");
        assert!(open(&key, &rec_key(&key, "p:bob:0"), &blob).is_none(), "moved to another name");
        assert_ne!(rec_key(&key, "p:ann:0"), rec_key(&[8u8; 32], "p:ann:0"), "names depend on the key");
    }

    fn device(name: &str) -> AppState {
        let dir = std::env::temp_dir().join(format!("aqua-hs-{name}-{}", crate::state::uuid_like()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut s = AppState::boot(dir).unwrap().into_inner().unwrap();
        s.username = Some("vlad".into());
        s
    }
    fn chat(s: &mut AppState, id: &str, msgs: Vec<ChatMessage>) {
        let summary = crate::state::summary_for(id, "", false, false);
        s.chats.insert(id.into(), ChatBook { summary, messages: msgs });
        s.chat_order.push(id.into());
    }
    /// What `push` would store (all records of `from`), applied on `to` like `pull`.
    fn relay(from: &AppState, to: &mut AppState) {
        let key = [1u8; 32];
        let me = "vlad";
        let st_from = load(from);
        let mut recs: Vec<Rec> = Vec::new();
        for (id, book) in &from.chats {
            if !syncable(id) || id == me { continue; }
            for (i, c) in book.messages.chunks(PAGE).enumerate() { recs.push(Rec::Page { chat: id.clone(), page: i as u32, msgs: c.to_vec() }); }
            recs.push(Rec::Meta { chat: id.clone(), card: Some(card_of(&book.summary)), tomb: st_from.tomb.get(id).cloned().unwrap_or_default() });
        }
        for (id, tomb) in &st_from.tomb {
            if !from.chats.contains_key(id) { recs.push(Rec::Meta { chat: id.clone(), card: None, tomb: tomb.clone() }); }
        }
        let mut st = load(to);
        for r in recs { apply(to, &mut st, r, &key); }
        save(to, &st);
    }

    /// A phone joins: it gets the PC's chats, the PC gets what only the
    /// phone had, a chat deleted on the PC goes from the phone too and
    /// doesn't come back, and nothing becomes a chat with myself.
    #[test]
    fn two_devices_end_up_with_the_same_chats() {
        let (mut pc, mut phone) = (device("pc"), device("phone"));
        chat(&mut pc, "ann", vec![msg("ann", "09:00", "hi"), msg("me", "09:01", "hello"), msg("ann", "09:02", "how are you")]);
        chat(&mut pc, "bob", vec![msg("bob", "11:00", "yo")]);
        let long: Vec<ChatMessage> = (0..450).map(|i| msg("cat", &format!("12:{:02}", i % 60), &format!("m{i}"))).collect();
        chat(&mut pc, "cat", long);
        chat(&mut phone, "ann", vec![msg("ann", "09:00", "hi"), msg("ann", "09:30", "only on the phone")]);
        chat(&mut phone, "ppp", vec![msg("vlad", "08:00", "board")]);

        relay(&pc, &mut phone);
        assert_eq!(texts(&phone.chats["ann"].messages), ["hi", "hello", "how are you", "only on the phone"]);
        assert!(phone.chats.contains_key("bob"));
        assert_eq!(phone.chats["cat"].messages.len(), 450, "all three pages");
        relay(&phone, &mut pc);
        assert!(pc.chats.contains_key("ppp"));
        assert_eq!(pc.chats["ann"].messages.len(), 4);
        relay(&pc, &mut phone);
        assert_eq!(phone.chats["ann"].messages.len(), 4, "nothing doubled");

        let gone = pc.chats["bob"].messages.clone();
        tombstone(&pc, "bob", &gone);
        pc.chats.remove("bob");
        relay(&pc, &mut phone);
        assert!(!phone.chats.contains_key("bob"));
        relay(&phone, &mut pc);
        assert!(!pc.chats.contains_key("bob"), "doesn't come back");

        chat(&mut phone, "vlad", vec![msg("vlad", "07:00", "x")]);
        relay(&phone, &mut pc);
        assert!(!pc.chats.contains_key("vlad"));
    }
}
