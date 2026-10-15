//! Shared app state for the Tauri backend.
//!
//! Wraps identity, vault, prekey manager, transport, per-peer sessions,
//! and the in-memory chat store the UI reads from.
//!
//! Concurrency model: `AppState` itself is stored in `Mutex<AppState>`
//! by the Tauri runtime; the mutex is held only for quick snapshotting
//! and mutation. Long-running async work (transport I/O) uses cloned
//! `Arc<Transport>` handles taken out of the state without keeping the
//! mutex locked across await points.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex as StdMutex},
    time::SystemTime,
};

use aqua_client_core::{MasterKey, PrekeyManager, Transport, Vault};
use aqua_core::{
    keys::IdentityKeyPair,
    session::Session,
};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

/// Identifier of the built-in "Saved Messages" self-chat.
pub const SAVED_MESSAGES_ID: &str = "__saved__";

/// Name of the on-disk file holding Saved Messages. Lives next to the
/// vault; readable only to this user since the data directory is in
/// the OS app-data location.
pub const SAVED_MESSAGES_FILE: &str = "saved-messages.json";

// ---------- Wire types for Tauri commands ----------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSummary {
    pub id: String,
    pub name: String,
    pub letter: String,
    pub gradient: String,
    pub preview: String,
    pub preview_is_typing: bool,
    pub preview_by_me: bool,
    pub has_lock: bool,
    pub time: String,
    pub unread: u32,
    pub online: bool,
    pub verified: bool,
    pub channel: bool,
    /// True when this chat has a real E2EE Session behind it (as opposed
    /// to a seeded mock).
    pub live: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub from: String,
    pub time: String,
    pub kind: MessageKind,
    /// Local-only extras (kept-deleted flag, edit history).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<MsgMeta>,
}

/// AyuGram-style local history: what a message used to say, and whether the
/// sender deleted it (kept on this device when "Keep deleted" is on).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MsgMeta {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub deleted: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edits: Vec<String>,
    /// Supergroup room (Discord-style channel) the message belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room: Option<String>,
}

/// Device-local behaviour switches set from the UI.
#[derive(Debug, Clone, Default)]
pub struct LocalPrefs {
    /// Keep messages the sender deleted (shown as "deleted").
    pub keep_deleted: bool,
    /// Remember previous versions of edited messages.
    pub keep_edits: bool,
    /// People whose messages, calls and controls are dropped.
    pub blocked: std::collections::HashSet<String>,
}

/// A video or audio file attached to an article. Same crypto as a normal
/// file offer: the blob sits encrypted on the relay, the key travels only
/// inside the (E2EE) article message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArticleMedia {
    /// "video" | "audio" | "image"
    pub kind: String,
    pub cid: String,
    #[serde(default)]
    pub key_b64: String,
    #[serde(default)]
    pub nonce_b64: String,
    pub name: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub mime: Option<String>,
    /// Small still frame for videos (data:image/…).
    #[serde(default)]
    pub poster: Option<String>,
    /// Seconds, when the sender knew it.
    #[serde(default)]
    pub duration: Option<f64>,
}

impl ArticleMedia {
    /// Only well-formed references survive parsing (no remote URLs, no
    /// path tricks in the CID).
    pub fn sane(mut self) -> Option<Self> {
        if self.kind != "video" && self.kind != "audio" && self.kind != "image" { return None; }
        let cid_ok = (self.cid.len() == 64 && self.cid.chars().all(|c| c.is_ascii_hexdigit()))
            || (self.cid.starts_with("local-") && self.cid.len() <= 64
                && self.cid.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
        if !cid_ok { return None; }
        if self.poster.as_deref().is_some_and(|p| !p.starts_with("data:image/") || p.len() > 400_000) {
            self.poster = None;
        }
        // `name` stays byte-exact: it is the AEAD associated data of the blob.
        Some(self)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "content", rename_all = "snake_case")]
pub enum MessageKind {
    Text { text: String },
    Code { code: String, language: String },
    /// A shared file. Sender puts a file offer through the E2EE session
    /// as the usual text payload; the receiver parses it and turns it
    /// into this variant. `cid` uniquely names the ciphertext blob on
    /// the server; `key` + `nonce` decrypt it locally.
    File {
        cid: String,
        key_b64: String,
        nonce_b64: String,
        name: String,
        size: u64,
        mime: Option<String>,
        /// `"sticker"`, `"gif"`, `"image"`, `"video"`, or `None` for a
        /// generic file. Drives inline rendering on the receiver side.
        #[serde(default)]
        category: Option<String>,
        /// Locally-cached path if the receiver has already downloaded
        /// and decrypted the blob, OR the sender's own source path for
        /// outgoing attachments. Lets the UI render inline previews.
        saved_at: Option<String>,
    },
    /// Long-form "article" message — Telegram-style post with a title
    /// and a markdown body. On the wire this travels as a plain text
    /// payload tagged `[article:v1]{json}` so old clients just see the
    /// raw JSON and we don't need a protocol bump.
    Article {
        title: String,
        body: String,
        cover_data_url: Option<String>,
        /// Photos for `[[gallery:…]]` carousels in the body (data: URLs).
        /// Added in v0.1.1; older senders simply don't have it.
        #[serde(default)]
        images: Vec<String>,
        /// Optional subtitle / lead line under the title.
        #[serde(default)]
        subtitle: Option<String>,
        /// Videos (in carousels as `v<N>`) and music (the article's
        /// playlist): encrypted blobs on the relay, referenced by CID + key.
        #[serde(default)]
        media: Vec<ArticleMedia>,
    },
    /// A poll — Telegram-style. Travels as a plain-text payload tagged
    /// `[poll:v1]{json}` so it syncs through the normal E2EE message path
    /// and old clients just see JSON. Votes are separate control messages
    /// (`[pollvote:v1]`) keyed by this poll's `id`, so a 1:1 chat tallies
    /// both sides live.
    Poll {
        id: String,
        question: String,
        options: Vec<String>,
        #[serde(default)]
        multi: bool,
        /// Quiz mode: one right answer (`correct`), revealed after voting.
        #[serde(default)]
        quiz: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        correct: Option<u32>,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        explanation: String,
        /// Public poll: everyone sees who voted for what.
        #[serde(default)]
        public: bool,
        /// Auto-close time (ms since epoch).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        close_at: Option<i64>,
    },
    /// A reply that quotes an earlier message. Travels as a plain-text
    /// payload tagged `[reply:v1]{json}` (same envelope trick as polls/
    /// articles), so the quoted snippet rides along and both sides render
    /// the reply header without needing a shared message id.
    Reply {
        quoted_author: String,
        quoted_text: String,
        text: String,
    },
    System { text: String },
    DateSeparator { label: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InitInfo {
    pub has_identity: bool,
    /// True only once the user has completed the email-registration wizard
    /// on this device. Distinct from merely having a saved connection hint:
    /// a leftover dev handle must NOT count as a real account.
    pub registered: bool,
    pub username: Option<String>,
    pub email: Option<String>,
    pub session_fingerprint: String,
    pub opk_available: usize,
    pub spk_rotates_in_days: u32,
    pub connected: bool,
    pub server_url: Option<String>,
    /// Current outbound SOCKS5 proxy, if the user routes through one.
    pub proxy: Option<String>,
    /// Full path of the vault directory this instance is using.
    /// Shown in the UI so two instances can be visually confirmed to
    /// have distinct identities (different directories ⇒ different
    /// vault.sqlite ⇒ different identity).
    pub data_dir: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IncomingEventPayload {
    pub peer_id: String,
    pub text: String,
    pub time: String,
    pub is_new_chat: bool,
}

// ---------- Chat book -----------------------------------------------------

pub struct ChatBook {
    pub summary: ChatSummary,
    pub messages: Vec<ChatMessage>,
}

// ---------- Groups / supergroups / channels --------------------------------
//
// A group is a client-side concept layered over the existing 1:1 E2EE
// sessions: to post, the sender encrypts the message once per member and
// relays N sealed envelopes (fan-out). The server never learns the group
// exists — it only forwards pairwise envelopes it can't read. Membership,
// roles and settings are the group's own state, distributed to members as
// `[grpinfo:v1]` control messages and persisted locally in groups.json.

pub const GROUPS_FILE: &str = "groups.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupMember {
    pub username: String,
    /// "owner" | "admin" | "member"
    pub role: String,
    /// Everything else the UI keeps per member — member tag (Telegram's
    /// "prefix"), admin title, admin rights, restrictions… Round-tripped
    /// untouched so new fields need no backend change.
    #[serde(flatten, default)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupSettings {
    /// Each permission is "all" (everyone) or "admins" (admins only).
    pub send_messages: String,
    pub send_media: String,
    pub send_stickers: String,
    pub send_polls: String,
    pub add_members: String,
    pub pin_messages: String,
    pub change_info: String,
    /// Minimum seconds between a member's messages (0 = off).
    pub slow_mode: u32,
    /// Channels: append the admin's name to posts.
    pub sign_messages: bool,
    /// Public (joinable by link) vs private (invite only).
    pub is_public: bool,
    pub public_link: String,
    /// New members can read history from before they joined.
    pub history_visible: bool,
    /// The rest of Telegram's group/channel settings (granular media
    /// permissions, reactions, protected content, join requests, auto-delete,
    /// server tag…), owned by the UI.
    #[serde(flatten, default)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Default for GroupSettings {
    fn default() -> Self {
        GroupSettings {
            send_messages: "all".into(),
            send_media: "all".into(),
            send_stickers: "all".into(),
            send_polls: "all".into(),
            add_members: "all".into(),
            pin_messages: "admins".into(),
            change_info: "admins".into(),
            slow_mode: 0,
            sign_messages: false,
            is_public: false,
            public_link: String::new(),
            history_visible: true,
            extra: Default::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub id: String,
    /// "group" | "supergroup" | "channel"
    pub kind: String,
    pub title: String,
    pub description: String,
    pub gradient: String,
    pub owner: String,
    pub members: Vec<GroupMember>,
    pub settings: GroupSettings,
    pub created_at: String,
    /// Bumped on every metadata change so members converge on the newest.
    pub version: u64,
    /// Banned users, join requests, recent-actions log, invite links… (UI-owned).
    #[serde(flatten, default)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Group {
    pub fn role_of(&self, username: &str) -> &str {
        self.members
            .iter()
            .find(|m| m.username == username)
            .map(|m| m.role.as_str())
            .unwrap_or("member")
    }
    pub fn is_admin(&self, username: &str) -> bool {
        matches!(self.role_of(username), "owner" | "admin")
    }
    pub fn is_member(&self, username: &str) -> bool {
        self.members.iter().any(|m| m.username == username)
    }
    pub fn is_banned(&self, username: &str) -> bool {
        self.extra
            .get("banned")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().any(|x| x.as_str() == Some(username)))
            .unwrap_or(false)
    }
    /// A setting from `settings.extra` that is "all" or "admins" (default all).
    pub fn perm(&self, key: &str) -> &str {
        self.settings.extra.get(key).and_then(|v| v.as_str()).unwrap_or("all")
    }
}

/// Should `from`'s snapshot `next` replace what we have (`cur`)?
/// Only admins of the current version may change a group; only the owner may
/// hand ownership away; a group we don't know must come from its own admin.
pub fn accept_group_snapshot(cur: Option<&Group>, next: &Group, from: &str) -> bool {
    match cur {
        None => next.is_admin(from),
        Some(cur) => {
            if next.version < cur.version {
                return false;
            }
            if !cur.is_admin(from) {
                // A plain member may only *add* members, when the group allows it.
                return cur.settings.add_members == "all" && cur.is_member(from) && only_adds_members(cur, next);
            }
            if next.owner != cur.owner && from != cur.owner {
                return false;
            }
            // An admin can't demote or remove the owner.
            if from != cur.owner && next.role_of(&cur.owner) != "owner" {
                return false;
            }
            true
        }
    }
}

/// `next` == `cur` plus some new plain members (and a bumped version).
fn only_adds_members(cur: &Group, next: &Group) -> bool {
    let mut trimmed = next.clone();
    let added: Vec<&GroupMember> = next.members.iter().filter(|m| !cur.is_member(&m.username)).collect();
    if added.is_empty() || added.iter().any(|m| m.role != "member" || !m.extra.is_empty()) {
        return false;
    }
    trimmed.members.retain(|m| cur.is_member(&m.username));
    trimmed.version = cur.version;
    serde_json::to_value(&trimmed).ok() == serde_json::to_value(cur).ok()
}

/// Is `author` allowed to post `body` into `g`? Mirrors the sender-side check
/// so a modified client can't bypass channel / admin-only / media rules.
/// May `who` delete other members' messages in `g`? (owner, or an admin
/// whose rights don't explicitly exclude it)
pub fn can_delete_others(g: &Group, who: &str) -> bool {
    let Some(m) = g.members.iter().find(|m| m.username == who) else { return false };
    match m.role.as_str() {
        "owner" => true,
        "admin" => m.extra.get("rights").and_then(|r| r.get("delete_messages")).and_then(|v| v.as_bool()).unwrap_or(true),
        _ => false,
    }
}

/// A supergroup room (Discord-style channel), stored in the group snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Room {
    pub id: String,
    pub name: String,
    /// "text" | "voice"
    #[serde(default = "room_text")]
    pub kind: String,
    /// Only admins may post (announcements).
    #[serde(default)]
    pub readonly: bool,
}
fn room_text() -> String { "text".into() }

/// The rooms of a supergroup with rooms turned on (empty otherwise).
pub fn rooms_of(g: &Group) -> Vec<Room> {
    if g.kind != "supergroup" || !g.settings.extra.get("rooms_enabled").and_then(|v| v.as_bool()).unwrap_or(false) {
        return Vec::new();
    }
    g.extra.get("rooms").and_then(|v| serde_json::from_value::<Vec<Room>>(v.clone()).ok()).unwrap_or_default()
}

/// Which room a message to `g` lands in: the requested one if it exists,
/// else the first text room; None when the group has no rooms.
pub fn resolve_room(g: &Group, wanted: Option<&str>) -> Option<Room> {
    let rooms: Vec<Room> = rooms_of(g).into_iter().filter(|r| r.kind == "text").collect();
    if rooms.is_empty() {
        return None;
    }
    wanted.and_then(|w| rooms.iter().find(|r| r.id == w).cloned()).or_else(|| rooms.first().cloned())
}

/// `may_post` plus the room rule (read-only rooms are for admins).
pub fn may_post_in(g: &Group, author: &str, body: &str, room: Option<&Room>) -> bool {
    may_post(g, author, body) && !(room.is_some_and(|r| r.readonly) && !g.is_admin(author))
}

pub fn may_post(g: &Group, author: &str, body: &str) -> bool {
    if !g.is_member(author) || g.is_banned(author) {
        return false;
    }
    let admin = g.is_admin(author);
    if admin {
        return true;
    }
    let muted = g.members.iter().find(|m| m.username == author)
        .and_then(|m| m.extra.get("muted")).and_then(|v| v.as_bool()).unwrap_or(false);
    if muted {
        return false;
    }
    if g.kind == "channel" || g.settings.send_messages == "admins" {
        return false;
    }
    if body.starts_with("[poll:v1]") && g.settings.send_polls == "admins" {
        return false;
    }
    true
}

pub fn load_groups(dir: &std::path::Path) -> HashMap<String, Group> {
    let path = dir.join(GROUPS_FILE);
    let Ok(bytes) = std::fs::read(&path) else { return HashMap::new(); };
    serde_json::from_slice::<Vec<Group>>(&bytes)
        .map(|v| v.into_iter().map(|g| (g.id.clone(), g)).collect())
        .unwrap_or_default()
}

pub fn save_groups(_dir: &std::path::Path, _groups: &HashMap<String, Group>) {
    // Groups are kept in the encrypted vault (history.rs flush).
}

/// Build a chat summary for a group so it shows in the sidebar list.
pub fn group_summary(g: &Group, preview: &str) -> ChatSummary {
    let letter = g.title.chars().next()
        .map(|c| c.to_uppercase().next().unwrap_or('#').to_string())
        .unwrap_or_else(|| "#".into());
    ChatSummary {
        id: g.id.clone(),
        name: g.title.clone(),
        letter,
        gradient: g.gradient.clone(),
        preview: preview.into(),
        preview_is_typing: false,
        preview_by_me: false,
        has_lock: true,
        time: hhmm_now(),
        unread: 0,
        online: false,
        verified: g.kind == "channel",
        channel: g.kind == "channel",
        live: true,
    }
}

// ---------- App state ------------------------------------------------------

pub struct AppState {
    pub data_dir: PathBuf,
    pub vault: Vault,
    pub identity: IdentityKeyPair,
    /// True once email registration has completed on this device.
    pub registered: bool,
    pub username: Option<String>,
    pub email: Option<String>,
    pub server_url: Option<String>,
    pub transport: Option<Arc<Transport>>,
    /// One live `Session` per peer.
    pub sessions: HashMap<String, Session>,
    /// UI-side chat store (summaries + messages).
    pub chats: HashMap<String, ChatBook>,
    pub chat_order: Vec<String>,
    /// Groups / supergroups / channels this account belongs to.
    pub groups: HashMap<String, Group>,
    pub prefs: LocalPrefs,
    /// Content hash of each chat as last written to the vault (history.rs).
    pub hist_hashes: HashMap<String, u64>,
    /// Cheap per-chat fingerprints from the last flush (history.rs).
    pub hist_fp: HashMap<String, u64>,
    /// Room the UI has open per supergroup; sends without a room go there.
    pub active_rooms: HashMap<String, String>,
}

impl AppState {
    pub fn boot(data_dir: PathBuf) -> Result<StdMutex<Self>> {
        std::fs::create_dir_all(&data_dir).ok();
        crate::media::ensure_dirs(&data_dir);
        // Apply the saved outbound proxy (if any) before any network use.
        aqua_client_core::transport::set_proxy(load_proxy(&data_dir));
        let vault_path = data_dir.join("vault.sqlite");
        let master_key = load_or_create_master_key(&data_dir)
            .context("loading master key")?;
        let vault = Vault::open(&vault_path, master_key).context("opening vault")?;

        // Identity: load or create.
        let identity = match vault.load_identity()? {
            Some(id) => id,
            None => {
                let id = IdentityKeyPair::generate();
                vault.save_identity(&id)?;
                id
            }
        };

        // Ensure SPK + OPKs exist.
        {
            let pm = PrekeyManager::new(&vault, &identity);
            pm.check_and_refresh(SystemTime::now()).ok();
        }

        // Rehydrate existing sessions from disk into memory.
        let mut sessions = HashMap::new();
        let mut chats = HashMap::new();
        let mut chat_order = Vec::new();
        for peer_id in vault.list_session_peer_ids().unwrap_or_default() {
            if let Ok(Some(sess)) = vault.load_session(&peer_id) {
                let summary = summary_for(&peer_id, /*preview*/ "", /*by_me*/ false, /*typing*/ false);
                chats.insert(peer_id.clone(), ChatBook { summary, messages: Vec::new() });
                chat_order.push(peer_id.clone());
                sessions.insert(peer_id, sess);
            }
        }

        // Stored history (encrypted in the vault): messages + summaries.
        let mut hist_hashes = HashMap::new();
        for (id, summary, messages) in crate::history::load_all(&vault) {
            // Not "written" yet as far as the flush knows: 0 never matches.
            hist_hashes.insert(id.clone(), 0);
            match chats.get_mut(&id) {
                Some(book) => { book.summary = summary; book.messages = messages; }
                None => {
                    chats.insert(id.clone(), ChatBook { summary, messages });
                    chat_order.push(id);
                }
            }
        }

        // Saved Messages stays on top.
        if let Some(pos) = chat_order.iter().position(|c| c == SAVED_MESSAGES_ID) {
            let id = chat_order.remove(pos);
            chat_order.insert(0, id);
        }

        // Load the last-connection hints (server_url + username) from
        // connection.json next to the vault, if present. The hints make
        // auto-reconnect possible — the frontend reads them off
        // init_info() and re-calls connect_to_server without prompting.
        let (saved_url, saved_user) = load_connection_hints(&data_dir);

        // The real account record. `registered` is the single source of
        // truth for "this device has a logged-in account". A stale
        // connection hint (saved_user) from a dev connect does NOT make
        // the user registered — otherwise the login window never shows and
        // the app silently enters a handle the user never created.
        let account = load_account(&data_dir);
        let registered = account.registered;
        let (username, email) = if registered {
            // Registered: the account handle is canonical; the connection
            // hint only supplies the handle for pre-account installs.
            (account.username.clone().or(saved_user.clone()), account.email.clone())
        } else {
            // Not registered: ignore any leftover handle entirely so the
            // entry gate opens the registration/login window.
            (None, None)
        };

        // "Saved Messages" — a built-in self-chat that's always live
        // regardless of whether the user is connected to a server.
        // Lets you jot things down, test the UI with any file / sticker,
        // and have a one-click "chat with myself" without spinning up
        // a second instance. Classic Telegram feature.
        //
        // Nothing goes over the wire for this chat: messages are
        // persisted to the local vault only. The vault is already
        // encrypted-at-rest with ChaCha20-Poly1305, so there's no
        // security loss by skipping the ratchet here.
        if !chats.contains_key(SAVED_MESSAGES_ID) {
            let saved = load_saved_messages(&data_dir);
            let (preview, time) = saved
                .last()
                .map(|m| (one_line_preview(&m.kind), m.time.clone()))
                .unwrap_or_else(|| ("Note to self — only you see this".into(), "".into()));
            let summary = ChatSummary {
                id: SAVED_MESSAGES_ID.into(),
                name: "Saved Messages".into(),
                letter: "☁".into(),
                gradient: "linear-gradient(135deg, #7B5CFF 0%, #00E5C7 100%)".into(),
                preview,
                preview_is_typing: false, preview_by_me: true, has_lock: true,
                time,
                unread: 0, online: true, verified: true, channel: false,
                live: true,
            };
            chats.insert(SAVED_MESSAGES_ID.into(), ChatBook { summary, messages: saved });
            chat_order.insert(0, SAVED_MESSAGES_ID.into());
        }

        // If nothing but Saved Messages, seed a small on-boarding welcome.
        if chats.len() == 1 && chats.contains_key(SAVED_MESSAGES_ID) {
            let welcome = ChatBook {
                summary: ChatSummary {
                    id: "welcome".into(),
                    name: "Welcome".into(),
                    letter: "◆".into(),
                    gradient: "linear-gradient(135deg, #00E5C7 0%, #7B5CFF 100%)".into(),
                    preview: "click Connect at the bottom, then start a chat".into(),
                    preview_is_typing: false, preview_by_me: false, has_lock: false,
                    time: "now".into(), unread: 0, online: false, verified: true,
                    channel: true, live: false,
                },
                messages: vec![
                    date("w-date", "Getting started"),
                    system_msg("w-sys",
                        "Session initialised. Identity + prekey material stored in your local encrypted vault."),
                    text_from("w1", "welcome", "0",
                        "Just want to try everything right now? Open <b>☁ Saved Messages</b> at the top of the sidebar — \
                         it's a self-chat that always works, no server needed. Send text, files, stickers, GIFs — everything renders inline. \
                         Nothing leaves your device."),
                    text_from("w2", "welcome", "0",
                        "To chat with someone else: start the relay with <code>cargo run -p aqua-server</code> (or <code>.\\run-server.ps1</code>), \
                         click the plug icon at the bottom of the sidebar to Connect, then hit the <b>+</b> next to the search bar to add a contact."),
                    text_from("w3", "welcome", "0",
                        "Attach files with the <b>paperclip</b> (Ctrl / Shift for multiple), or drag &amp; drop from your file explorer anywhere into the chat. \
                         Images, GIFs and videos preview inline; everything else lands as a download card. The sticker icon opens your local stickers / GIFs — \
                         drop files into the folders shown there and reopen the picker."),
                ],
            };
            chats.insert("welcome".into(), welcome);
            chat_order.push("welcome".into());
        }

        // Load groups/channels and surface each as a sidebar chat.
        let groups = crate::history::load_groups(&vault).unwrap_or_else(|| load_groups(&data_dir));
        for g in groups.values() {
            if !chats.contains_key(&g.id) {
                chats.insert(g.id.clone(), ChatBook {
                    summary: group_summary(g, &g.description),
                    messages: Vec::new(),
                });
                chat_order.push(g.id.clone());
            }
        }

        Ok(StdMutex::new(Self {
            data_dir,
            vault,
            identity,
            registered,
            username,
            email,
            server_url: saved_url,
            transport: None,
            sessions,
            chats,
            chat_order,
            groups,
            prefs: LocalPrefs::default(),
            hist_hashes,
            active_rooms: HashMap::new(),
            hist_fp: HashMap::new(),
        }))
    }

    /// Persist the current (server_url, username) to connection.json so
    /// a later launch can auto-reconnect without the user re-entering
    /// the dialog. Idempotent; errors are logged, not propagated.
    pub fn save_connection_hints(&self) {
        if let (Some(url), Some(user)) = (self.server_url.as_ref(), self.username.as_ref()) {
            save_connection_hints(&self.data_dir, url, user);
        }
    }

    /// Record a completed email registration. This is what flips the entry
    /// gate off for good on this device.
    pub fn mark_registered(&mut self, username: String, email: Option<String>) {
        self.registered = true;
        self.username = Some(username.clone());
        self.email = email.clone();
        save_account(&self.data_dir, &Account {
            registered: true,
            username: Some(username),
            email,
        });
    }

    /// Log out: forget the account + auto-reconnect hints and drop the live
    /// transport so the entry gate reopens. The identity/vault is kept, so
    /// logging back in with the same handle reuses the same keys.
    pub fn log_out(&mut self) {
        self.registered = false;
        self.username = None;
        self.email = None;
        self.transport = None;
        clear_account_file(&self.data_dir);
        let _ = std::fs::remove_file(self.data_dir.join(CONNECTION_FILE));
    }

    pub fn init_info(&self) -> InitInfo {
        let opk = self.vault.opk_count().unwrap_or(0);
        InitInfo {
            has_identity: true,
            registered: self.registered,
            username: self.username.clone(),
            email: self.email.clone(),
            session_fingerprint: short_fingerprint(&self.identity),
            opk_available: opk,
            spk_rotates_in_days: 7,
            connected: self.transport.is_some(),
            server_url: self.server_url.clone(),
            proxy: aqua_client_core::transport::proxy(),
            data_dir: self.data_dir.display().to_string(),
        }
    }
}

// ---------- Saved Messages persistence -------------------------------------

/// Load the Saved Messages chat log from disk. Tolerant: a missing or
/// malformed file returns an empty log, never fails.
pub fn load_saved_messages(data_dir: &std::path::Path) -> Vec<ChatMessage> {
    let path = data_dir.join(SAVED_MESSAGES_FILE);
    let Ok(bytes) = std::fs::read(&path) else { return Vec::new(); };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

/// Persist the Saved Messages chat log to disk. Errors are logged.
pub fn save_saved_messages(_data_dir: &std::path::Path, _msgs: &[ChatMessage]) {
    // Saved Messages now live in the encrypted vault with the rest of the
    // history (history.rs flushes them); nothing is written in the clear.
}

/// Short one-line preview for a message kind, used in chat-list cards.
/// `[ce:pack/😏_001.webp]` custom-emoji tokens → the emoji itself, for
/// chat-list previews and notifications.
pub fn humanize_tokens(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find("[ce:") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 4..];
        match after.find(']') {
            Some(j) if !after[..j].contains(char::is_whitespace) => {
                let file = after[..j].rsplit('/').next().unwrap_or("");
                let emoji = file.split('_').next().filter(|e| !e.is_empty() && !e.is_ascii()).unwrap_or("🙂");
                out.push_str(emoji);
                rest = &after[j + 1..];
            }
            _ => {
                out.push_str("[ce:");
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

pub fn one_line_preview(kind: &MessageKind) -> String {
    match kind {
        MessageKind::Text { text } => humanize_tokens(text.lines().next().unwrap_or("")),
        MessageKind::Code { .. } => "📄 code snippet".into(),
        MessageKind::File { name, category, .. } => {
            let icon = match category.as_deref() {
                Some("sticker") => return "🎨 Sticker".into(),
                Some("gif")     => "🎞️",
                Some("image")   => "🖼️",
                Some("video")   => "📹",
                _               => "📎",
            };
            format!("{icon} {name}")
        }
        MessageKind::Article { title, .. } => format!("📰 {title}"),
        MessageKind::Poll { question, .. } => format!("📊 {question}"),
        MessageKind::Reply { text, .. } => text.clone(),
        MessageKind::System { text } => text.clone(),
        MessageKind::DateSeparator { label } => label.clone(),
    }
}

// ---------- Connection hints -----------------------------------------------
//
// `connection.json` holds the last successful (server_url, username) so
// the next launch can auto-reconnect. The file sits next to the vault,
// is written only after a successful connect, and is tolerant of
// corruption / missing fields: on any parse error we silently fall
// back to no hints and the UI shows the manual connect dialog.
//
// This is NOT credential storage — the actual auth material is the
// identity key in the (keyring-wrapped) vault. The file is a UI
// convenience and nothing else.

const CONNECTION_FILE: &str = "connection.json";
const PROXY_FILE: &str = "proxy.txt";

/// Load the saved SOCKS5 proxy URL (e.g. `socks5://127.0.0.1:1080`), if the
/// user configured "connect through a server". Missing/blank ⇒ direct.
pub fn load_proxy(dir: &std::path::Path) -> Option<String> {
    let s = std::fs::read_to_string(dir.join(PROXY_FILE)).ok()?;
    let s = s.trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

/// Persist (or clear) the proxy URL.
pub fn save_proxy(dir: &std::path::Path, url: Option<&str>) {
    let path = dir.join(PROXY_FILE);
    match url.map(str::trim).filter(|s| !s.is_empty()) {
        Some(u) => { let _ = std::fs::write(&path, u); }
        None => { let _ = std::fs::remove_file(&path); }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct ConnectionHints {
    #[serde(default)]
    server_url: Option<String>,
    #[serde(default)]
    username: Option<String>,
}

fn load_connection_hints(dir: &std::path::Path) -> (Option<String>, Option<String>) {
    let path = dir.join(CONNECTION_FILE);
    let Ok(bytes) = std::fs::read(&path) else { return (None, None); };
    match serde_json::from_slice::<ConnectionHints>(&bytes) {
        Ok(h) => (h.server_url, h.username),
        Err(e) => {
            tracing::warn!(?e, "connection.json parse failed; ignoring");
            (None, None)
        }
    }
}

fn save_connection_hints(dir: &std::path::Path, server_url: &str, username: &str) {
    let path = dir.join(CONNECTION_FILE);
    let hints = ConnectionHints {
        server_url: Some(server_url.to_string()),
        username: Some(username.to_string()),
    };
    match serde_json::to_vec_pretty(&hints) {
        Ok(bytes) => {
            if let Err(e) = std::fs::write(&path, bytes) {
                tracing::warn!(?e, path = %path.display(), "connection.json write failed");
            }
        }
        Err(e) => tracing::warn!(?e, "connection.json serialize failed"),
    }
}

// ---------- Account record -------------------------------------------------
//
// `account.json` records that email registration completed on this device.
// It is the single source of truth for "logged in", separate from the
// connection hint (which is only an auto-reconnect convenience). Written
// after the registration wizard finishes, deleted on logout. Not credential
// storage — the auth material is still the vault-held identity key.

const ACCOUNT_FILE: &str = "account.json";

#[derive(Debug, Serialize, Deserialize, Default)]
struct Account {
    #[serde(default)]
    registered: bool,
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    email: Option<String>,
}

fn load_account(dir: &std::path::Path) -> Account {
    let path = dir.join(ACCOUNT_FILE);
    let Ok(bytes) = std::fs::read(&path) else { return Account::default(); };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

fn save_account(dir: &std::path::Path, acct: &Account) {
    let path = dir.join(ACCOUNT_FILE);
    match serde_json::to_vec_pretty(acct) {
        Ok(bytes) => {
            if let Err(e) = std::fs::write(&path, bytes) {
                tracing::warn!(?e, path = %path.display(), "account.json write failed");
            }
        }
        Err(e) => tracing::warn!(?e, "account.json serialize failed"),
    }
}

fn clear_account_file(dir: &std::path::Path) {
    let _ = std::fs::remove_file(dir.join(ACCOUNT_FILE));
}

// ---------- Master key persistence -----------------------------------------
//
// Priority: OS keyring (Credential Manager / Keychain / Secret Service),
// then — only if the keyring call fails — a plain `master.key` file in
// the vault directory. The file-based path keeps dev / headless CI
// working, but the keyring is where production users should hold the
// key: a laptop whose disk is cloned without the account's keychain
// cannot decrypt the vault.
//
// On first run with an existing file (upgrade path from the earlier
// release), we read the file, promote the key into the keyring, and
// delete the file to close the window where it was readable in two
// places.

// Phones have no `keyring` backend we can rely on; the app sandbox is
// private to us, so the vault key lives in the `master.key` file there.
#[cfg(any(target_os = "android", target_os = "ios", aqua_mobilecheck))]
mod keyring {
    #[derive(Debug)]
    pub enum Error { NoEntry, Unsupported }
    pub struct Entry;
    impl Entry {
        pub fn new(_service: &str, _user: &str) -> Result<Entry, Error> { Err(Error::Unsupported) }
        pub fn get_password(&self) -> Result<String, Error> { Err(Error::NoEntry) }
        pub fn set_password(&self, _v: &str) -> Result<(), Error> { Err(Error::Unsupported) }
    }
}

const KEYRING_SERVICE: &str = "app.aquamarine.desktop";
const KEYRING_USER:    &str = "vault-master-key";

fn load_or_create_master_key(dir: &std::path::Path) -> Result<MasterKey> {
    use aqua_client_core::MASTER_KEY_LEN;

    let legacy_path = dir.join("master.key");
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).ok();

    // 1. Prefer the keyring if it holds a usable value.
    if let Some(entry) = &entry {
        match entry.get_password() {
            Ok(hex_str) => match decode_key(&hex_str) {
                Some(arr) => {
                    // If a legacy file somehow still exists, clean it up
                    // so there isn't a plaintext key on disk alongside
                    // an authoritative keyring entry.
                    if legacy_path.exists() {
                        if let Err(e) = std::fs::remove_file(&legacy_path) {
                            tracing::warn!(?e, "could not remove stale master.key file");
                        }
                    }
                    return Ok(MasterKey::from_bytes(arr));
                }
                None => tracing::warn!("master key in keyring is malformed; ignoring"),
            },
            Err(keyring::Error::NoEntry) => { /* fall through to migrate / generate */ }
            Err(e) => tracing::warn!(?e, "keyring read failed; will try fallback paths"),
        }
    }

    // 2. Legacy file: read it, try to promote into the keyring, delete
    //    on successful promotion.
    if legacy_path.exists() {
        let bytes = std::fs::read(&legacy_path)?;
        let arr: [u8; MASTER_KEY_LEN] = bytes
            .try_into()
            .map_err(|_| anyhow!("master.key has unexpected length"))?;
        if let Some(entry) = &entry {
            match entry.set_password(&hex::encode(arr)) {
                Ok(()) => {
                    if let Err(e) = std::fs::remove_file(&legacy_path) {
                        tracing::warn!(?e, "promoted master key to keyring but could not delete the legacy file");
                    } else {
                        tracing::info!("migrated vault master key from file to OS keyring");
                    }
                }
                Err(e) => tracing::warn!(?e, "keyring write failed; keeping master.key file"),
            }
        }
        return Ok(MasterKey::from_bytes(arr));
    }

    // 3. Fresh install: generate a key and store it in the keyring if
    //    possible; otherwise write it to disk.
    std::fs::create_dir_all(dir)?;
    let key = MasterKey::generate();
    let stored_in_keyring = entry
        .as_ref()
        .and_then(|e| e.set_password(&hex::encode(key.0)).ok())
        .is_some();
    if stored_in_keyring {
        tracing::info!("new vault master key stored in OS keyring");
    } else {
        std::fs::write(&legacy_path, key.0)?;
        tracing::warn!(
            path = %legacy_path.display(),
            "OS keyring unavailable; master key stored at this path on disk"
        );
    }
    Ok(key)
}

fn decode_key(hex_str: &str) -> Option<[u8; aqua_client_core::MASTER_KEY_LEN]> {
    let bytes = hex::decode(hex_str.trim()).ok()?;
    bytes.try_into().ok()
}

// ---------- Fingerprint helpers -------------------------------------------

pub fn short_fingerprint(id: &IdentityKeyPair) -> String {
    let pub_id = id.public();
    let bytes = pub_id.dh_pub.as_bytes();
    let hex = hex::encode(&bytes[..8]);
    let chunks: Vec<String> = hex.as_bytes().chunks(4)
        .map(|c| std::str::from_utf8(c).unwrap().to_string()).collect();
    chunks.join(" ")
}

const WORDS: &[&str] = &[
    "quartz", "harbor", "meridian", "oxide", "jasmine", "rift", "signal", "plume",
    "cobalt", "syntax", "fjord", "opal", "helix", "nova", "cypress", "flux",
    "onyx", "prism", "vector", "tundra", "ember", "azure", "granite", "loom",
];

pub fn word_fingerprint_from(dh_pub: &[u8; 32]) -> Vec<String> {
    (0..8).map(|i| WORDS[(dh_pub[i] as usize) % WORDS.len()].to_string()).collect()
}

pub fn hex_fingerprint_from(dh_pub: &[u8; 32]) -> String {
    let hex_str = hex::encode(dh_pub);
    let groups: Vec<String> = hex_str.as_bytes().chunks(4)
        .map(|c| std::str::from_utf8(c).unwrap().to_string()).collect();
    groups.join(" ")
}

// ---------- Chat helpers --------------------------------------------------

pub fn summary_for(peer_id: &str, preview: &str, by_me: bool, typing: bool) -> ChatSummary {
    let (letter, gradient) = gradient_for(peer_id);
    ChatSummary {
        id: peer_id.into(),
        name: peer_id.into(),
        letter,
        gradient,
        preview: preview.into(),
        preview_is_typing: typing,
        preview_by_me: by_me,
        has_lock: true,
        time: hhmm_now(),
        unread: 0,
        online: false,
        verified: false,
        channel: false,
        live: true,
    }
}

/// Deterministic avatar (letter + gradient) from a peer id.
pub fn gradient_for(peer_id: &str) -> (String, String) {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(peer_id.as_bytes());
    const PAIRS: &[(&str, &str)] = &[
        ("#00E5C7", "#7B5CFF"),
        ("#FF7CAC", "#FFB067"),
        ("#4ADE80", "#22D3EE"),
        ("#C084FC", "#F472B6"),
        ("#22D3EE", "#6366F1"),
        ("#FFB067", "#FF4D6D"),
        ("#F472B6", "#FBBF24"),
        ("#60A5FA", "#A855F7"),
    ];
    let (a, b) = PAIRS[(hash[0] as usize) % PAIRS.len()];
    let letter = peer_id
        .chars()
        .next()
        .map(|c| c.to_uppercase().next().unwrap_or('?').to_string())
        .unwrap_or_else(|| "?".into());
    (letter, format!("linear-gradient(135deg, {a} 0%, {b} 100%)"))
}

pub fn hhmm_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let mins_total = (secs / 60) % (24 * 60);
    let h = (mins_total / 60) as u32;
    let m = (mins_total % 60) as u32;
    format!("{:02}:{:02}", h, m)
}

// ---------- Small message constructors ------------------------------------

pub fn text_msg_from_me(text: &str) -> ChatMessage {
    ChatMessage { meta: None,
        id: format!("m-{}", uuid_like()),
        from: "me".into(),
        time: hhmm_now(),
        kind: classify_kind(text),
    }
}

pub fn text_msg_from_peer(peer_id: &str, text: &str) -> ChatMessage {
    ChatMessage { meta: None,
        id: format!("m-{}", uuid_like()),
        from: peer_id.into(),
        time: hhmm_now(),
        kind: classify_kind(text),
    }
}

/// Turn plaintext into the appropriate `MessageKind`:
///   - a file offer if the `[file:v1 ...]` prefix matches
///   - an article if the `[article:v1]{json}` prefix matches
///   - otherwise plain text
pub fn classify_kind(text: &str) -> MessageKind {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

    // Article tag — plain-text envelope so backwards-compatible with
    // old clients (they just see the JSON as text).
    const ARTICLE_PREFIX: &str = "[article:v1]";
    if let Some(json_part) = text.strip_prefix(ARTICLE_PREFIX) {
        #[derive(serde::Deserialize)]
        struct ArticleWire {
            title: String,
            body: String,
            #[serde(default)]
            cover_data_url: Option<String>,
            #[serde(default)]
            images: Vec<String>,
            #[serde(default)]
            subtitle: Option<String>,
            #[serde(default)]
            media: Vec<ArticleMedia>,
        }
        if let Ok(a) = serde_json::from_str::<ArticleWire>(json_part.trim_start()) {
            return MessageKind::Article {
                title: a.title,
                body: a.body,
                cover_data_url: a.cover_data_url,
                // Only inline images are accepted (no remote fetches).
                images: a.images.into_iter().filter(|u| u.starts_with("data:image/")).take(60).collect(),
                subtitle: a.subtitle.filter(|s| !s.trim().is_empty()),
                media: a.media.into_iter().filter_map(ArticleMedia::sane).take(40).collect(),
            };
        }
    }

    // Reply tag — quoted reply, same plain-text-envelope trick.
    const REPLY_PREFIX: &str = "[reply:v1]";
    if let Some(json_part) = text.strip_prefix(REPLY_PREFIX) {
        #[derive(serde::Deserialize)]
        struct ReplyWire {
            #[serde(default)]
            quoted_author: String,
            #[serde(default)]
            quoted_text: String,
            text: String,
        }
        if let Ok(r) = serde_json::from_str::<ReplyWire>(json_part.trim_start()) {
            return MessageKind::Reply {
                quoted_author: r.quoted_author,
                quoted_text: r.quoted_text,
                text: r.text,
            };
        }
    }

    // Poll tag — same plain-text-envelope trick as articles.
    const POLL_PREFIX: &str = "[poll:v1]";
    if let Some(json_part) = text.strip_prefix(POLL_PREFIX) {
        #[derive(serde::Deserialize)]
        struct PollWire {
            id: String,
            question: String,
            options: Vec<String>,
            #[serde(default)]
            multi: bool,
            #[serde(default)]
            quiz: bool,
            #[serde(default)]
            correct: Option<u32>,
            #[serde(default)]
            explanation: String,
            #[serde(default)]
            public: bool,
            #[serde(default)]
            close_at: Option<i64>,
        }
        if let Ok(p) = serde_json::from_str::<PollWire>(json_part.trim_start()) {
            if !p.options.is_empty() {
                let n = p.options.len() as u32;
                return MessageKind::Poll {
                    id: p.id,
                    question: p.question,
                    options: p.options,
                    multi: p.multi && !p.quiz,
                    quiz: p.quiz,
                    correct: p.correct.filter(|c| *c < n),
                    explanation: p.explanation.chars().take(400).collect(),
                    public: p.public,
                    close_at: p.close_at,
                };
            }
        }
    }

    match aqua_core::FileEnvelope::from_message_text(text) {
        Ok(env) => MessageKind::File {
            cid: env.cid,
            key_b64: URL_SAFE_NO_PAD.encode(env.key),
            nonce_b64: URL_SAFE_NO_PAD.encode(env.nonce),
            name: env.filename,
            size: env.size,
            mime: env.mime,
            category: env.category,
            saved_at: None,
        },
        Err(_) => MessageKind::Text { text: text.into() },
    }
}

/// Short human preview of a message for the chat-list line.
pub fn preview_for(kind: &MessageKind) -> String {
    match kind {
        MessageKind::Text { text } => humanize_tokens(text),
        MessageKind::Code { .. }   => "[code]".into(),
        MessageKind::File { name, category, mime, .. } => {
            let icon = match category.as_deref() {
                Some("sticker") => return "🎨 Sticker".into(),
                Some("gif") => "🎞️",
                _ => match mime.as_deref() {
                    Some(m) if m.starts_with("image/") => "🖼️",
                    Some(m) if m.starts_with("video/") => "📹",
                    _ => "📎",
                },
            };
            format!("{icon} {name}")
        }
        MessageKind::Article { title, .. } => format!("📰 {title}"),
        MessageKind::Poll { question, .. } => format!("📊 {question}"),
        MessageKind::Reply { text, .. } => text.clone(),
        MessageKind::System { text } => text.clone(),
        MessageKind::DateSeparator { label } => label.clone(),
    }
}

fn text_from(id: &str, from: &str, time: &str, text: &str) -> ChatMessage {
    ChatMessage { meta: None,
        id: id.into(), from: from.into(), time: time.into(),
        kind: MessageKind::Text { text: text.into() },
    }
}
fn date(id: &str, label: &str) -> ChatMessage {
    ChatMessage { meta: None,
        id: id.into(), from: "system".into(), time: "".into(),
        kind: MessageKind::DateSeparator { label: label.into() },
    }
}
fn system_msg(id: &str, text: &str) -> ChatMessage {
    ChatMessage { meta: None,
        id: id.into(), from: "system".into(), time: "".into(),
        kind: MessageKind::System { text: text.into() },
    }
}

pub fn uuid_like() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let n = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{n:x}")
}


#[cfg(test)]
mod preview_tests {
    #[test]
    fn custom_emoji_tokens_become_emoji() {
        use super::humanize_tokens as h;
        assert_eq!(h("hi [ce:eltaller47/😏_001.webp][ce:eltaller47/🔥_050.webp]!"), "hi 😏🔥!");
        assert_eq!(h("[ce:pack/x_002.tgs]"), "🙂");
        assert_eq!(h("broken [ce:no end"), "broken [ce:no end");
        assert_eq!(h("plain"), "plain");
    }
}

#[cfg(test)]
mod group_rule_tests {

    #[test]
    fn article_media_refs_are_sanitised() {
        let hex = "a".repeat(64);
        let txt = format!(r#"[article:v1]{{"title":"T","body":"[[gallery:0,v0]]","cover_data_url":null,"media":[
            {{"kind":"video","cid":"{hex}","key_b64":"k","nonce_b64":"n","name":"clip.mp4","size":5,"poster":"data:image/jpeg;base64,AA"}},
            {{"kind":"audio","cid":"local-abc-123","name":"song.mp3"}},
            {{"kind":"video","cid":"../../etc/passwd","name":"x.mp4"}},
            {{"kind":"exe","cid":"{hex}","name":"x.exe"}},
            {{"kind":"video","cid":"{hex}","name":"y.mp4","poster":"https://tracker.example/p.png"}}
        ]}}"#);
        match classify_kind(&txt) {
            MessageKind::Article { media, .. } => {
                assert_eq!(media.len(), 3);
                assert_eq!(media[0].kind, "video");
                assert!(media[0].poster.is_some());
                assert_eq!(media[1].cid, "local-abc-123");
                assert!(media[2].poster.is_none(), "remote poster must be dropped");
            }
            other => panic!("not an article: {other:?}"),
        }
    }

    #[test]
    fn article_v1_with_and_without_photos() {
        // Old senders: no images / subtitle.
        let old = classify_kind(r#"[article:v1]{"title":"T","body":"B","cover_data_url":null}"#);
        match old {
            MessageKind::Article { title, images, subtitle, .. } => {
                assert_eq!(title, "T");
                assert!(images.is_empty());
                assert!(subtitle.is_none());
            }
            other => panic!("not an article: {other:?}"),
        }
        // New: photos for carousels; remote URLs are dropped.
        let new = classify_kind(r#"[article:v1]{"title":"T","subtitle":"S","body":"[[gallery:0,1]]","cover_data_url":null,"images":["data:image/webp;base64,AAA","https://evil.example/x.png","data:image/jpeg;base64,BBB"]}"#);
        match new {
            MessageKind::Article { images, subtitle, .. } => {
                assert_eq!(images, vec!["data:image/webp;base64,AAA".to_string(), "data:image/jpeg;base64,BBB".to_string()]);
                assert_eq!(subtitle.as_deref(), Some("S"));
            }
            other => panic!("not an article: {other:?}"),
        }
    }

    use super::*;

    fn member(u: &str, role: &str) -> GroupMember {
        GroupMember { username: u.into(), role: role.into(), extra: Default::default() }
    }
    fn group() -> Group {
        Group {
            id: "grp_1".into(), kind: "group".into(), title: "T".into(), description: String::new(),
            gradient: String::new(), owner: "own".into(),
            members: vec![member("own", "owner"), member("adm", "admin"), member("bob", "member")],
            settings: GroupSettings::default(), created_at: String::new(), version: 5, extra: Default::default(),
        }
    }

    #[test]
    fn snapshots_need_an_admin() {
        let cur = group();
        let mut next = cur.clone();
        next.version = 6;
        next.title = "renamed".into();
        assert!(accept_group_snapshot(Some(&cur), &next, "adm"));
        assert!(accept_group_snapshot(Some(&cur), &next, "own"));
        assert!(!accept_group_snapshot(Some(&cur), &next, "bob"), "members can't rename");
        assert!(!accept_group_snapshot(Some(&cur), &next, "stranger"));
        let mut stale = next.clone();
        stale.version = 4;
        assert!(!accept_group_snapshot(Some(&cur), &stale, "own"));
        // Admin can't take ownership or demote the owner.
        let mut coup = next.clone();
        coup.owner = "adm".into();
        assert!(!accept_group_snapshot(Some(&cur), &coup, "adm"));
        let mut demote = next.clone();
        demote.members[0].role = "member".into();
        assert!(!accept_group_snapshot(Some(&cur), &demote, "adm"));
        // A new group must come from one of its own admins.
        assert!(accept_group_snapshot(None, &cur, "own"));
        assert!(!accept_group_snapshot(None, &cur, "bob"));
    }

    #[test]
    fn members_may_only_add_people_when_allowed() {
        let cur = group();
        let mut add = cur.clone();
        add.version = 6;
        add.members.push(member("new", "member"));
        assert!(accept_group_snapshot(Some(&cur), &add, "bob"));
        let mut sneaky = add.clone();
        sneaky.title = "x".into();
        assert!(!accept_group_snapshot(Some(&cur), &sneaky, "bob"));
        let mut promote = add.clone();
        promote.members.push(member("evil", "admin"));
        assert!(!accept_group_snapshot(Some(&cur), &promote, "bob"));
        let mut closed = cur.clone();
        closed.settings.add_members = "admins".into();
        let mut add2 = closed.clone();
        add2.version = 6;
        add2.members.push(member("new", "member"));
        assert!(!accept_group_snapshot(Some(&closed), &add2, "bob"));
    }

    #[test]
    fn posting_rules() {
        let mut g = group();
        assert!(may_post(&g, "bob", "hi"));
        assert!(!may_post(&g, "stranger", "hi"));
        g.extra.insert("banned".into(), serde_json::json!(["bob"]));
        assert!(!may_post(&g, "bob", "hi"));
        g.extra.clear();
        g.settings.send_messages = "admins".into();
        assert!(!may_post(&g, "bob", "hi"));
        assert!(may_post(&g, "adm", "hi"));
        g.settings.send_messages = "all".into();
        g.kind = "channel".into();
        assert!(!may_post(&g, "bob", "hi"));
        assert!(may_post(&g, "own", "hi"));
        g.kind = "group".into();
        g.settings.send_polls = "admins".into();
        assert!(!may_post(&g, "bob", "[poll:v1]{}"));
        g.settings.send_polls = "all".into();
        g.members[2].extra.insert("muted".into(), serde_json::json!(true));
        assert!(!may_post(&g, "bob", "hi"));
    }

    #[test]
    fn extras_round_trip() {
        let mut g = group();
        g.settings.extra.insert("reactions".into(), serde_json::json!("some"));
        g.members[2].extra.insert("tag".into(), serde_json::json!("designer"));
        g.extra.insert("clan".into(), serde_json::json!({ "text": "AQUA" }));
        let back: Group = serde_json::from_str(&serde_json::to_string(&g).unwrap()).unwrap();
        assert_eq!(back.settings.extra["reactions"], "some");
        assert_eq!(back.members[2].extra["tag"], "designer");
        assert_eq!(back.extra["clan"]["text"], "AQUA");
        // Old files without the new fields still load.
        let old = r#"{"id":"g","kind":"group","title":"t","description":"","gradient":"","owner":"o","members":[{"username":"o","role":"owner"}],"settings":{"send_messages":"all","send_media":"all","send_stickers":"all","send_polls":"all","add_members":"all","pin_messages":"admins","change_info":"admins","slow_mode":0,"sign_messages":false,"is_public":false,"public_link":"","history_visible":true},"created_at":"","version":1}"#;
        assert!(serde_json::from_str::<Group>(old).is_ok());
    }
}
