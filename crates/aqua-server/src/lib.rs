//! Aquamarine signaling / relay server (library form).
//!
//! Exposes [`app`] so tests can spin the router up on an ephemeral port.
//! The `bin/main.rs` binary is a thin wrapper around this.

use std::{collections::VecDeque, sync::Arc};

use aqua_core::{
    file_transfer::hex_sha256,
    keys::{IdentityPublic, OneTimePreKeyPublic, PreKeyBundle, SignedPreKeyPublic},
};
use axum::{
    body::Bytes,
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        DefaultBodyLimit, Path, Query, State,
    },
    http::{header, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use dashmap::DashMap;
use futures_util::StreamExt;
use serde::Deserialize;
use tokio::sync::{mpsc, Mutex};

pub mod auth;
pub mod packs;
pub mod blobstore;
pub mod fed;
pub mod games;
pub mod directory;
pub mod tgapi;
pub mod site;
// Re-export the wire types so tests + external callers keep working.
pub use aqua_core::wire::{BundleUpload, PayloadKind, RelayMessage};
pub use auth::{AuthState, Caller};
pub use directory::Directory;

// ---------- App state --------------------------------------------------------

pub type UserId = String;

#[derive(Default)]
pub struct UserState {
    pub identity: Option<IdentityPublic>,
    pub signed_prekey: Option<SignedPreKeyPublic>,
    pub opks: VecDeque<OneTimePreKeyPublic>,
    /// Envelopes queued while the user is offline.
    pub inbox: VecDeque<RelayMessage>,
    /// One entry per connected device.
    pub tx: Vec<mpsc::UnboundedSender<RelayMessage>>,
}

/// One inbound message queued for a bot program to pick up via
/// `GET /v1/bot/updates`.
#[derive(Clone, serde::Serialize)]
pub struct BotUpdate {
    pub id: u64,
    pub from: String,
    pub text: String,
    pub ts: u64,
    /// The user picked a button of the bot's inline keyboard (Telegram API
    /// layer): its callback_data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callback: Option<String>,
    /// Message the keyboard belonged to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callback_msg: Option<i64>,
}

#[derive(Default)]
pub struct BotQueue {
    pub next_id: u64,
    pub updates: VecDeque<BotUpdate>,
    /// Wakes long polls when an update arrives.
    pub notify: Arc<tokio::sync::Notify>,
}

/// Token buckets for the bot API: per bot, and per bot → recipient.
#[derive(Default)]
pub struct BotLimits {
    buckets: DashMap<String, (f64, std::time::Instant)>,
}

impl BotLimits {
    /// Take one token from bucket `key` (capacity `cap`, `rate` per second).
    fn take(&self, key: &str, cap: f64, rate: f64) -> bool {
        let now = std::time::Instant::now();
        let mut e = self.buckets.entry(key.to_string()).or_insert((cap, now));
        let (ref mut tokens, ref mut at) = *e;
        *tokens = (*tokens + now.duration_since(*at).as_secs_f64() * rate).min(cap);
        *at = now;
        if *tokens >= 1.0 {
            *tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// Longest text a bot may send (Telegram's limit too).
pub const BOT_TEXT_LIMIT: usize = 4096;

#[derive(Clone)]
pub struct AppState {
    pub users: Arc<DashMap<UserId, Arc<Mutex<UserState>>>>,
    /// user_id → the DH public of the identity that first claimed it.
    /// Subsequent auth-protected requests for the same user_id must
    /// come from the same identity.
    pub bindings: Arc<DashMap<UserId, [u8; 32]>>,
    pub auth: Arc<AuthState>,
    /// Content-addressed encrypted blob store, keyed by SHA-256 hex.
    /// Values are opaque ciphertext — the server cannot read them.
    pub blobs: blobstore::BlobStore,
    /// Persistent registry of @usernames + public profile cards.
    pub directory: Arc<Directory>,
    /// Per-bot inbound message queues (bot username → queue). Ephemeral
    /// — bots are expected to be online and polling.
    pub bot_queues: Arc<DashMap<String, Arc<Mutex<BotQueue>>>>,
    /// Rate limits of the bot API.
    pub bot_limits: Arc<BotLimits>,
    /// Game catalogue (names + icons) for profiles.
    pub games: games::GameDb,
    /// When true, only usernames registered in the directory (email-verified)
    /// may upload a bundle or open a WebSocket. This makes email registration
    /// mandatory — no ephemeral unregistered handles. Toggle off for an open
    /// relay with `AQUA_OPEN_SIGNUP=1`.
    pub require_registration: bool,
    /// Admin panel state: access mode + registration allowlist, persisted
    /// to `aqua-admin.json`. Shared behind an RwLock; never held across await.
    pub admin: Arc<std::sync::RwLock<AdminConfig>>,
    /// Federation with other relays (None = standalone).
    pub fed: fed::FedSlot,
    /// Unix seconds the process started, for the uptime readout.
    pub started_unix: u64,
    /// Woken when a peer says "I changed something": pull from peers now.
    pub fed_kick: Arc<tokio::sync::Notify>,
    /// Telegram Bot API compatibility layer (keyboards, webhooks, ids).
    pub tg: Arc<tgapi::TgState>,
}

/// Admin-tunable server policy (the panel edits this).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct AdminConfig {
    /// "invite-only": only emails/usernames on `allow` may register.
    /// This is plain access control for your own relay (solo mode).
    #[serde(default)]
    pub invite_only: bool,
    /// Lower-cased emails and/or @usernames permitted to register when
    /// `invite_only` is on.
    #[serde(default)]
    pub allow: Vec<String>,
}

const ADMIN_FILE: &str = "aqua-admin.json";

impl AdminConfig {
    fn load() -> Self {
        std::fs::read(ADMIN_FILE)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }
    fn save(&self) {
        if let Ok(b) = serde_json::to_vec_pretty(self) {
            let _ = std::fs::write(ADMIN_FILE, b);
        }
    }
    /// Is this email allowed to register?
    fn permits(&self, email: &str) -> bool {
        if !self.invite_only {
            return true;
        }
        let e = email.trim().to_lowercase();
        self.allow.iter().any(|a| a.trim().to_lowercase() == e)
    }
}

impl AppState {
    pub fn new() -> Self {
        // Registration is mandatory unless an operator opts into an open
        // relay. `AQUA_OPEN_SIGNUP=1` (or true/yes) disables the requirement.
        let open = std::env::var("AQUA_OPEN_SIGNUP")
            .ok()
            .map(|v| matches!(v.trim(), "1" | "true" | "yes" | "on"))
            .unwrap_or(false);
        Self::build(!open)
    }

    /// Open-signup state for tests and open relays: any identity may claim
    /// any unclaimed username without registering.
    pub fn new_open() -> Self {
        Self::build(false)
    }

    /// A relay with its own in-memory directory, blob folder and
    /// federation settings — several can run in one process (tests).
    pub fn isolated(blob_dir: std::path::PathBuf, fed_cfg: Option<fed::FedConfig>) -> Self {
        let mut s = Self::build(true);
        s.directory = Arc::new(Directory::open_memory());
        s.blobs = blobstore::BlobStore::new(blob_dir, std::time::Duration::from_secs(3600));
        s.fed = Arc::new(std::sync::RwLock::new(fed_cfg.and_then(fed::Fed::new)));
        s
    }

    fn build(require_registration: bool) -> Self {
        let directory = Directory::open()
            .map(Arc::new)
            .unwrap_or_else(|e| {
                tracing::error!(error = %e, "failed to open user directory; continuing without persistence");
                // Fall back to an in-memory DB so the server still boots.
                Arc::new(Directory::open_memory())
            });
        Self {
            users: Arc::new(DashMap::new()),
            bindings: Arc::new(DashMap::new()),
            auth: AuthState::new(),
            blobs: blobstore::BlobStore::from_env(),
            directory,
            bot_queues: Arc::new(DashMap::new()),
            bot_limits: Arc::new(BotLimits::default()),
            games: games::GameDb::default(),
            require_registration,
            admin: Arc::new(std::sync::RwLock::new(AdminConfig::load())),
            fed: fed::slot_from_config(),
            started_unix: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            fed_kick: Arc::new(tokio::sync::Notify::new()),
            tg: Arc::new(tgapi::TgState::load()),
        }
    }

    pub fn bot_queue(&self, bot: &str) -> Arc<Mutex<BotQueue>> {
        self.bot_queues
            .entry(bot.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(BotQueue::default())))
            .clone()
    }

    pub fn user(&self, id: &str) -> Arc<Mutex<UserState>> {
        self.users
            .entry(id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(UserState::default())))
            .clone()
    }

    /// Enforce the user_id ↔ identity binding.
    ///
    /// * If `user_id` is unclaimed, claim it for `identity_dh_pub`.
    /// * If already claimed by the same identity, no-op.
    /// * If claimed by someone else, refuse.
    pub fn claim_or_check(
        &self,
        user_id: &str,
        identity_dh_pub: [u8; 32],
    ) -> Result<(), auth::AuthError> {
        // A registered @username (persisted in the directory) is only
        // usable by its owner identity, even across restarts.
        if self.directory.identity_for(user_id).is_some() {
            // The primary device or one of the account's extra devices.
            return if self.directory.device_role(user_id, &identity_dh_pub).is_some() {
                Ok(())
            } else {
                Err(auth::AuthError::UsernameTaken)
            };
        }
        // Not in the directory. With mandatory registration, refuse: the
        // client must complete email registration (which writes the
        // directory row) before it can publish a bundle or open a socket.
        if self.require_registration {
            return Err(auth::AuthError::NotRegistered);
        }
        match self.bindings.entry(user_id.to_string()) {
            dashmap::mapref::entry::Entry::Vacant(v) => {
                v.insert(identity_dh_pub);
                Ok(())
            }
            dashmap::mapref::entry::Entry::Occupied(o) => {
                if o.get() == &identity_dh_pub {
                    Ok(())
                } else {
                    Err(auth::AuthError::UsernameTaken)
                }
            }
        }
    }
}

impl AppState {
    /// Where envelopes for this device go. The primary device keeps the
    /// plain `user` address (what older clients send to); every extra device
    /// is `user~<device id>`. Call after [`Self::claim_or_check`] succeeded.
    pub fn address_for(&self, user_id: &str, identity_dh: &[u8; 32]) -> String {
        match self.directory.device_role(user_id, identity_dh) {
            Some(false) => format!("{user_id}~{}", directory::device_id(identity_dh)),
            _ => user_id.to_string(),
        }
    }

    /// Every address of an account: `user` plus `user~dev` per extra device.
    pub fn addresses_of(&self, user_id: &str) -> Vec<String> {
        let mut out = vec![user_id.to_string()];
        for d in self.directory.devices(user_id) {
            if let Ok(b) = hex::decode(&d.dh) {
                out.push(format!("{user_id}~{}", directory::device_id(&b)));
            }
        }
        out
    }
}

/// `alice~a1b2c3` → `alice`.
pub fn base_user(addr: &str) -> &str {
    addr.split('~').next().unwrap_or(addr)
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

// ---------- HTTP: prekey upload / fetch --------------------------------------

async fn upload_bundle(
    State(state): State<AppState>,
    Caller(dh_pub): Caller,
    Path(user): Path<UserId>,
    Json(body): Json<BundleUpload>,
) -> Result<StatusCode, (StatusCode, String)> {
    let token_fp = short_hex(&dh_pub);
    let bundle_fp = short_hex(body.identity.dh_pub.as_bytes());
    tracing::info!(user = %user, token = %token_fp, bundle = %bundle_fp, "POST /v1/bundle");

    // The uploader must own the identity in the bundle.
    if body.identity.dh_pub.as_bytes() != &dh_pub {
        tracing::warn!(user = %user, token = %token_fp, bundle = %bundle_fp,
            "upload_bundle REJECT: bundle identity ≠ token identity");
        return Err((
            StatusCode::FORBIDDEN,
            format!("bundle identity {bundle_fp} does not match your token identity {token_fp}"),
        ));
    }
    // Claim the username on first upload or verify it's still ours.
    if let Err(e) = state.claim_or_check(&user, dh_pub) {
        let existing = state.bindings.get(&user).map(|r| short_hex(r.value()));
        tracing::warn!(user = %user, token = %token_fp, existing = ?existing, error = %e,
            "upload_bundle REJECT: claim_or_check failed");
        return Err((e.status(), e.to_string()));
    }

    body.signed_prekey.verify(&body.identity).map_err(|e| {
        tracing::warn!(user = %user, error = %e, "upload_bundle REJECT: bad SPK signature");
        (StatusCode::BAD_REQUEST, format!("bad SPK signature: {e}"))
    })?;
    // Each device publishes its own bundle under its own address.
    let user = state.address_for(&user, &dh_pub);
    let user_state = state.user(&user);
    let mut u = user_state.lock().await;
    u.identity = Some(body.identity);
    u.signed_prekey = Some(body.signed_prekey);
    let opk_count = body.one_time_prekeys.len();
    for opk in body.one_time_prekeys {
        u.opks.push_back(opk);
    }
    // Keep only the newest 200 one-time keys.
    while u.opks.len() > 200 {
        u.opks.pop_front();
    }
    persist_prekeys(&state, &user, &u);
    tracing::info!(user = %user, token = %token_fp, opk_count,
        "upload_bundle OK: bundle stored");
    Ok(StatusCode::NO_CONTENT)
}

async fn fetch_bundle(
    State(state): State<AppState>,
    Path(user): Path<UserId>,
) -> Result<Json<PreKeyBundle>, StatusCode> {
    if let Some(b) = local_bundle(&state, &user).await {
        return Ok(Json(b));
    }
    // Uploaded to another relay of the federation?
    if let Some(f) = fed::current(&state) {
        if let Some(v) = f.fetch_bundle(&user).await {
            if let Ok(b) = serde_json::from_value::<PreKeyBundle>(v) {
                return Ok(Json(b));
            }
        }
    }
    Err(StatusCode::NOT_FOUND)
}

/// Prekeys as stored on disk.
#[derive(serde::Serialize, serde::Deserialize)]
struct StoredPrekeys {
    identity: IdentityPublic,
    signed_prekey: SignedPreKeyPublic,
    #[serde(default)]
    opks: Vec<OneTimePreKeyPublic>,
}

fn persist_prekeys(state: &AppState, user: &str, u: &UserState) {
    let (Some(identity), Some(signed_prekey)) = (u.identity, u.signed_prekey.clone()) else { return };
    let rec = StoredPrekeys { identity, signed_prekey, opks: u.opks.iter().cloned().collect() };
    if let Ok(json) = serde_json::to_string(&rec) {
        state.directory.prekeys_put(user, &json);
    }
}

pub(crate) async fn local_bundle(state: &AppState, user: &str) -> Option<PreKeyBundle> {
    let user_state = state.user(&user.to_string());
    let mut u = user_state.lock().await;
    if u.identity.is_none() {
        // After a restart: load what the user uploaded before.
        let rec: StoredPrekeys = serde_json::from_str(&state.directory.prekeys_get(user)?).ok()?;
        u.identity = Some(rec.identity);
        u.signed_prekey = Some(rec.signed_prekey);
        u.opks = rec.opks.into();
    }
    let identity = u.identity?;
    let signed_prekey = u.signed_prekey.clone()?;
    let one_time_prekey = u.opks.pop_front();
    if one_time_prekey.is_some() {
        persist_prekeys(state, user, &u);
    }
    Some(PreKeyBundle { identity, signed_prekey, one_time_prekey })
}

// ---------- WebSocket relay --------------------------------------------------

#[derive(Debug, Deserialize)]
struct WsQuery {
    user: String,
    /// Bearer token from the /v1/auth/verify response.
    token: String,
}

async fn ws_upgrade(
    ws: WebSocketUpgrade,
    Query(q): Query<WsQuery>,
    State(state): State<AppState>,
) -> Result<axum::response::Response, (StatusCode, String)> {
    tracing::info!(user = %q.user, token_prefix = %&q.token.chars().take(12).collect::<String>(),
        "WS upgrade attempt");

    let dh_pub = match auth::verify_ws_token(&state, &q.token) {
        Ok(k) => k,
        Err(e) => {
            tracing::warn!(user = %q.user, error = %e, "WS REJECT: token verification failed");
            return Err((e.status(), e.to_string()));
        }
    };
    let token_fp = short_hex(&dh_pub);
    tracing::info!(user = %q.user, token = %token_fp, "WS: token verified");

    // Claim this username for the token's identity on first use, or
    // verify it still belongs to us.
    if let Err(e) = state.claim_or_check(&q.user, dh_pub) {
        let existing = state.bindings.get(&q.user).map(|r| short_hex(r.value()));
        tracing::warn!(user = %q.user, token = %token_fp, existing = ?existing, error = %e,
            "WS REJECT: claim_or_check failed — username bound to different identity");
        return Err((e.status(), e.to_string()));
    }
    let me = state.address_for(&q.user, &dh_pub);
    tracing::info!(user = %q.user, addr = %me, token = %token_fp, "WS OK: upgrading to WebSocket");
    Ok(ws.on_upgrade(move |sock| ws_run(sock, state, me)))
}

async fn ws_run(mut socket: WebSocket, state: AppState, me: UserId) {
    state.directory.touch_seen(base_user(&me));
    let user_state = state.user(&me);
    let (tx, mut rx) = mpsc::unbounded_channel::<RelayMessage>();
    {
        let mut u = user_state.lock().await;
        u.tx.push(tx);
        while let Some(m) = u.inbox.pop_front() {
            if let Ok(bytes) = serde_json::to_vec(&m) {
                let _ = socket.send(Message::Binary(bytes.into())).await;
            }
        }
        // Envelopes that waited on disk (also across server restarts).
        for (id, bytes) in state.directory.inbox_take(&me) {
            if socket.send(Message::Binary(bytes.into())).await.is_ok() {
                state.directory.inbox_ack(id);
            }
        }
    }
    // …and those that waited on other relays of the federation.
    if let Some(f) = fed::current(&state) {
        for m in f.collect_inbox(&me).await {
            if let Ok(bytes) = serde_json::to_vec(&m) {
                let _ = socket.send(Message::Binary(bytes.into())).await;
            }
        }
    }
    {
    }

    loop {
        tokio::select! {
            m = rx.recv() => match m {
                Some(m) => match serde_json::to_vec(&m) {
                    Ok(bytes) => {
                        if socket.send(Message::Binary(bytes.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(e) => tracing::warn!(?e, "encode relay message"),
                },
                None => break,
            },
            frame = socket.next() => match frame {
                Some(Ok(Message::Binary(bytes))) => match serde_json::from_slice::<RelayMessage>(&bytes) {
                    Ok(mut m) => {
                        m.from = me.clone();
                        deliver(&state, m).await;
                    }
                    Err(e) => tracing::warn!(?e, "decode relay message"),
                },
                Some(Ok(Message::Text(text))) => match serde_json::from_str::<RelayMessage>(&text) {
                    Ok(mut m) => {
                        m.from = me.clone();
                        deliver(&state, m).await;
                    }
                    Err(e) => tracing::warn!(?e, "decode relay message (text)"),
                },
                Some(Ok(Message::Ping(p))) => {
                    let _ = socket.send(Message::Pong(p)).await;
                }
                Some(Ok(_)) => {}
                Some(Err(e)) => {
                    tracing::warn!(?e, "ws recv error");
                    break;
                }
                None => break,
            }
        }
    }

    let mut u = user_state.lock().await;
    u.tx.retain(|t| !t.is_closed());
    drop(u);
    state.directory.touch_seen(base_user(&me));
}

async fn deliver(state: &AppState, msg: RelayMessage) {
    // If the recipient is a registered bot, queue the (plaintext) text
    // into its bot queue instead of the normal user inbox — bots pull
    // via GET /v1/bot/updates, they don't hold a WebSocket.
    if msg.kind == PayloadKind::Plaintext && state.directory.is_bot(&msg.to) {
        // Bots talk to accounts, not devices.
        let mut msg = msg;
        msg.from = base_user(&msg.from).to_string();
        let text: String = String::from_utf8_lossy(&msg.payload).chars().take(BOT_TEXT_LIMIT).collect();
        // Writing to a bot lets it answer; /stop takes that back.
        if text.trim() == "/stop" {
            state.directory.bot_peer_remove(&msg.to, &msg.from);
        } else {
            state.directory.bot_peer_add(&msg.to, &msg.from);
        }
        // A Telegram-style bot's open inline keyboard: "2" / "Yes" picks a button.
        let callback = tgapi::classify(state, &msg.to, &msg.from, &text);
        let q = state.bot_queue(&msg.to);
        let mut q = q.lock().await;
        let id = q.next_id;
        q.next_id += 1;
        q.updates.push_back(BotUpdate {
            id,
            from: msg.from.clone(),
            callback: callback.as_ref().map(|c| c.0.clone()),
            callback_msg: callback.map(|c| c.1),
            text,
            ts: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        });
        // Cap the queue so an idle bot doesn't grow unbounded.
        while q.updates.len() > 1000 { q.updates.pop_front(); }
        q.notify.notify_waiters();
        return;
    }

    if deliver_local(state, &msg).await {
        return;
    }
    // Federation: maybe they're connected to another relay.
    if let Some(f) = fed::current(state) {
        if f.forward(&msg).await {
            return;
        }
    }
    // Kept on disk until the recipient connects (survives restarts; a peer
    // relay collects it if they connect there).
    match serde_json::to_vec(&msg) {
        Ok(bytes) if state.directory.inbox_push(&msg.to, &bytes).is_ok() => {}
        _ => {
            let recipient = state.user(&msg.to);
            recipient.lock().await.inbox.push_back(msg);
        }
    }
}

/// Hand an envelope to the recipient's live connections on THIS relay.
pub(crate) async fn deliver_local(state: &AppState, msg: &RelayMessage) -> bool {
    let recipient = state.user(&msg.to);
    let mut r = recipient.lock().await;
    let mut delivered = false;
    r.tx.retain(|t| !t.is_closed());
    for t in &r.tx {
        if t.send(msg.clone()).is_ok() {
            delivered = true;
        }
    }
    delivered
}

// ---------- Blob storage (encrypted file bodies) ---------------------------

/// Sanity cap on blob size accepted by the server. Larger blobs should
/// be broken into chunks by the client. Matches
/// `aqua_core::file_transfer::MAX_FILE_SIZE` plus AEAD tag headroom.
const BLOB_LIMIT: usize = 210 * 1024 * 1024;

#[derive(Deserialize, Default)]
struct BlobQuery {
    /// 1 = long-lived (group photo / banner), see `blobstore::KEEP_TTL`.
    #[serde(default)]
    keep: u8,
}

async fn upload_blob(
    State(state): State<AppState>,
    Caller(_dh_pub): Caller,
    Path(cid): Path<String>,
    Query(q): Query<BlobQuery>,
    body: Bytes,
) -> Result<StatusCode, (StatusCode, String)> {
    if body.len() > BLOB_LIMIT {
        return Err((StatusCode::PAYLOAD_TOO_LARGE, "blob too large".into()));
    }
    let actual_cid = hex_sha256(&body);
    if actual_cid != cid {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("body sha256 = {actual_cid}, URL says {cid}"),
        ));
    }
    // Content-addressed: a duplicate just refreshes the file's age.
    let store = state.blobs.clone();
    let keep = q.keep == 1;
    if keep && body.len() > blobstore::KEEP_LIMIT {
        return Err((StatusCode::PAYLOAD_TOO_LARGE, "only files up to 4 MiB can be kept".into()));
    }
    tokio::task::spawn_blocking(move || if keep { store.put_keep(&cid, &body) } else { store.put(&cid, &body) })
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn fetch_blob(
    State(state): State<AppState>,
    Path(cid): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    let store = state.blobs.clone();
    let local = { let (store, cid) = (store.clone(), cid.clone()); tokio::task::spawn_blocking(move || store.get(&cid)).await.ok().flatten() };
    let bytes = match local {
        Some(b) => b,
        None => {
            // Uploaded to another relay of the federation: fetch, check, keep.
            let f = fed::current(&state).ok_or(StatusCode::NOT_FOUND)?;
            let (_, b) = f.fetch_bytes(&format!("/v1/fed/blob/{cid}")).await.ok_or(StatusCode::NOT_FOUND)?;
            if hex_sha256(&b) != cid {
                return Err(StatusCode::NOT_FOUND);
            }
            let (s2, c2, b2) = (store.clone(), cid.clone(), b.clone());
            let _ = tokio::task::spawn_blocking(move || s2.put(&c2, &b2)).await;
            b
        }
    };
    Ok((
        [(header::CONTENT_TYPE, "application/octet-stream")],
        bytes,
    ))
}

// ---------- User directory: email registration + search --------------------

#[derive(Deserialize)]
struct EmailRequestBody {
    email: String,
    /// "signup" | "signin" | "reset" (older clients send nothing): lets the
    /// relay say right away that the account exists / doesn't exist.
    #[serde(default)]
    purpose: String,
}

#[derive(serde::Serialize)]
struct EmailRequestResp {
    ok: bool,
    dev_code: Option<String>,
    /// The mail server refused: the code is in the server window / admin panel.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    delivery_failed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

async fn email_request(
    State(state): State<AppState>,
    Json(body): Json<EmailRequestBody>,
) -> Result<Json<EmailRequestResp>, (StatusCode, String)> {
    // Invite-only (solo) relays: refuse before sending a code.
    if !state.admin.read().map(|c| c.permits(&body.email)).unwrap_or(true) {
        return Err((StatusCode::FORBIDDEN, "this relay is invite-only — ask the admin to add your email".into()));
    }
    let exists = state.directory.email_registered(&body.email);
    match body.purpose.as_str() {
        "signup" if exists => return Err((StatusCode::CONFLICT, directory::DirError::AccountExists.to_string())),
        "signin" | "reset" if !exists => return Err((StatusCode::NOT_FOUND, directory::DirError::NoAccount.to_string())),
        _ => {}
    }
    match state.directory.request_code_delivered(&body.email).await {
        Ok(d) => Ok(Json(EmailRequestResp { ok: true, dev_code: d.dev_code, delivery_failed: d.delivery_failed, error: d.error })),
        Err(e) => Err((StatusCode::BAD_REQUEST, e.to_string())),
    }
}

#[derive(Deserialize)]
struct EmailVerifyBody { email: String, code: String }

#[derive(serde::Serialize)]
struct EmailVerifyResp { ok: bool, ticket: String }

async fn email_verify(
    State(state): State<AppState>,
    Json(body): Json<EmailVerifyBody>,
) -> Result<Json<EmailVerifyResp>, (StatusCode, String)> {
    match state.directory.verify_code(&body.email, &body.code) {
        Ok(ticket) => Ok(Json(EmailVerifyResp { ok: true, ticket })),
        Err(e) => Err((StatusCode::BAD_REQUEST, e.to_string())),
    }
}

#[derive(Deserialize)]
struct RegisterBody {
    ticket: String,
    username: String,
    #[serde(default)]
    password: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    about: String,
    #[serde(default)]
    phone: String,
}

async fn register_user(
    State(state): State<AppState>,
    Caller(dh_pub): Caller,
    Json(body): Json<RegisterBody>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .directory
        .register(&body.username, &dh_pub, &body.ticket, &body.display_name, &body.about, &body.phone)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    if !body.password.is_empty() {
        state
            .directory
            .set_password(&body.username, &dh_pub, &body.password)
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    }
    tracing::info!(user = %body.username, "registered via directory");
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct LoginBody {
    ticket: String,
    #[serde(default)]
    password: String,
    /// "Pixel 8", "vpst (Linux)"… shown in the account's device list.
    #[serde(default)]
    device_name: String,
}

#[derive(serde::Serialize)]
struct LoginResp { username: String }

/// Sign in on another device (email ticket + password): the caller's
/// identity key is ADDED to the account as one more device.
async fn login_user(
    State(state): State<AppState>,
    Caller(dh_pub): Caller,
    Json(body): Json<LoginBody>,
) -> Result<Json<LoginResp>, (StatusCode, String)> {
    let username = state
        .directory
        .login_named(&body.ticket, &body.password, &dh_pub, &body.device_name)
        .map_err(|e| (dir_err_status(&e), e.to_string()))?;
    tracing::info!(user = %username, device = %directory::device_id(&dh_pub), "signed in on another device");
    Ok(Json(LoginResp { username }))
}

// ---------- Mini apps (Telegram WebApp) ------------------------------------------

#[derive(Deserialize)]
struct WebAppInitBody {
    bot: String,
    #[serde(default)]
    start_param: String,
}

/// `POST /v1/webapp/init` `{bot}` → `{init_data, signed}` for a mini app the
/// caller opens (signed with the bot's token, as Telegram does).
async fn webapp_init(
    State(state): State<AppState>,
    Caller(dh): Caller,
    Json(body): Json<WebAppInitBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let me = caller_username(&state, &dh)?;
    let bot = body.bot.trim().trim_start_matches('@').to_lowercase();
    if !state.directory.is_bot(&bot) {
        return Err((StatusCode::NOT_FOUND, "not a bot".into()));
    }
    let (init_data, signed) = tgapi::init_data(&state, &bot, &me, Some(body.start_param.as_str()));
    Ok(Json(serde_json::json!({ "init_data": init_data, "signed": signed })))
}

/// `GET /v1/webapp/menu/:bot` — the bot's menu button (`{"type":"web_app",…}` or default).
async fn webapp_menu(State(state): State<AppState>, Path(bot): Path<String>) -> Json<serde_json::Value> {
    Json(tgapi::menu_button(&state, &bot.trim().trim_start_matches('@').to_lowercase()))
}

// ---------- Sealed messages ------------------------------------------------------

#[derive(Deserialize)]
struct CapsuleBody {
    id: String,
    recipient: String,
    /// Hex of the 32-byte message key.
    key: String,
    /// "date" | "inactive"
    kind: String,
    #[serde(default)]
    open_at: i64,
    #[serde(default)]
    days: i64,
    #[serde(default)]
    grace_h: i64,
}

/// `PUT /v1/capsules` — store a sealed message's key with its condition.
async fn capsule_put(State(state): State<AppState>, Caller(dh): Caller, Json(b): Json<CapsuleBody>) -> Result<StatusCode, (StatusCode, String)> {
    let me = caller_username(&state, &dh)?;
    state.directory.capsule_put(&me, &b.id, &b.recipient, &b.key, &b.kind, b.open_at, b.days, b.grace_h)
        .map_err(|e| (dir_err_status(&e), e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /v1/capsules` — mine (without keys).
async fn capsules_mine(State(state): State<AppState>, Caller(dh): Caller) -> Result<Json<Vec<serde_json::Value>>, (StatusCode, String)> {
    let me = caller_username(&state, &dh)?;
    Ok(Json(state.directory.capsules_of(&me)))
}

/// `POST /v1/capsules/:id/open` — the recipient asks; the key once the
/// condition (and the owner's chance to deny) has passed.
async fn capsule_open(State(state): State<AppState>, Caller(dh): Caller, Path(id): Path<String>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let me = caller_username(&state, &dh)?;
    let owner = {
        // Who owns it (to see whether they're online right now).
        state.directory.capsule_owner(&id)
    };
    let online = match owner {
        Some(o) => {
            let mut on = false;
            for addr in state.addresses_of(&o) {
                if let Some(u) = state.users.get(&addr).map(|u| u.value().clone()) {
                    if u.lock().await.tx.iter().any(|t| !t.is_closed()) { on = true; break; }
                }
            }
            on
        }
        None => false,
    };
    match state.directory.capsule_open(&id, &me, online).map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))? {
        Ok(key) => Ok(Json(serde_json::json!({ "state": "open", "key": key }))),
        Err(v) => Ok(Json(v)),
    }
}

#[derive(Deserialize)]
struct CapsuleStop { #[serde(default)] forever: bool }

/// `POST /v1/capsules/:id/stop` `{forever}` — the owner denies the pending
/// request, or cancels the sealed message for good.
async fn capsule_stop(State(state): State<AppState>, Caller(dh): Caller, Path(id): Path<String>, Json(b): Json<CapsuleStop>) -> Result<StatusCode, (StatusCode, String)> {
    let me = caller_username(&state, &dh)?;
    match state.directory.capsule_stop(&me, &id, b.forever) {
        Ok(true) => Ok(StatusCode::NO_CONTENT),
        Ok(false) => Err((StatusCode::NOT_FOUND, "no such sealed message (or already opened)".into())),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

// ---------- Account recovery ------------------------------------------------------

#[derive(Deserialize)]
struct ResetBody {
    ticket: String,
    /// Recovery check derived from the 24-word key ("" if the account has none).
    #[serde(default)]
    recovery: String,
    /// The new password (as the client's auth secret).
    password: String,
}

/// `POST /v1/users/reset` — forgotten password: e-mail ticket + recovery key.
async fn reset_password(
    State(state): State<AppState>,
    Json(body): Json<ResetBody>,
) -> Result<Json<LoginResp>, (StatusCode, String)> {
    let username = state
        .directory
        .reset_password(&body.ticket, &body.recovery, &body.password)
        .map_err(|e| (match e { directory::DirError::BadRecovery => StatusCode::FORBIDDEN, _ => dir_err_status(&e) }, e.to_string()))?;
    Ok(Json(LoginResp { username }))
}

#[derive(Deserialize)]
struct RecoveryBody { check: String }

#[derive(serde::Serialize)]
struct RecoveryState { set: bool }

/// `GET /v1/me/recovery` — does my account have a recovery key?
async fn recovery_get(State(state): State<AppState>, Caller(dh): Caller) -> Result<Json<RecoveryState>, (StatusCode, String)> {
    let me = caller_username(&state, &dh)?;
    Ok(Json(RecoveryState { set: state.directory.has_recovery(&me) }))
}

/// `PUT /v1/me/recovery` `{check}` — register the recovery key's check.
async fn recovery_put(
    State(state): State<AppState>,
    Caller(dh): Caller,
    Json(body): Json<RecoveryBody>,
) -> Result<StatusCode, (StatusCode, String)> {
    let me = caller_username(&state, &dh)?;
    state.directory.set_recovery(&me, &dh, &body.check).map_err(|e| (dir_err_status(&e), e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- Devices ------------------------------------------------------------

#[derive(serde::Serialize)]
struct DeviceView {
    /// Where to send envelopes for this device.
    addr: String,
    id: String,
    primary: bool,
    name: String,
    added: i64,
    /// Connected to this relay right now.
    online: bool,
    /// Only in /v1/me/devices: the device asking.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    current: bool,
}

async fn device_views(state: &AppState, user: &str, me: Option<&[u8; 32]>) -> Option<Vec<DeviceView>> {
    let primary = state.directory.identity_for(user)?;
    let mut list = vec![(primary.to_vec(), String::new(), 0i64, true)];
    for d in state.directory.devices(user) {
        if let Ok(b) = hex::decode(&d.dh) {
            list.push((b, d.name, d.added, false));
        }
    }
    let mut out = Vec::new();
    for (dh, name, added, primary) in list {
        let id = directory::device_id(&dh);
        let addr = if primary { user.to_string() } else { format!("{user}~{id}") };
        let online = match state.users.get(&addr).map(|u| u.value().clone()) {
            Some(u) => u.lock().await.tx.iter().any(|t| !t.is_closed()),
            None => false,
        };
        let current = me.is_some_and(|m| m.as_slice() == dh.as_slice());
        out.push(DeviceView { addr, id, primary, name, added, online, current });
    }
    Some(out)
}

/// `GET /v1/devices/:user` — the devices to encrypt for (public, like bundles).
async fn get_devices(State(state): State<AppState>, Path(user): Path<UserId>) -> Result<Json<Vec<DeviceView>>, StatusCode> {
    let user = base_user(user.trim().trim_start_matches('@')).to_lowercase();
    device_views(&state, &user, None).await.map(Json).ok_or(StatusCode::NOT_FOUND)
}

/// `GET /v1/me/devices` — my devices, with the current one marked.
async fn my_devices(State(state): State<AppState>, Caller(dh): Caller) -> Result<Json<Vec<DeviceView>>, (StatusCode, String)> {
    let me = caller_username(&state, &dh)?;
    device_views(&state, &me, Some(&dh)).await.map(Json).ok_or((StatusCode::NOT_FOUND, "no such account".into()))
}

#[derive(Deserialize)]
struct RenameBody { name: String }

/// `POST /v1/me/devices/:id` `{name}` — rename an extra device.
async fn rename_device(
    State(state): State<AppState>,
    Caller(dh): Caller,
    Path(id): Path<String>,
    Json(body): Json<RenameBody>,
) -> Result<StatusCode, (StatusCode, String)> {
    let me = caller_username(&state, &dh)?;
    match state.directory.rename_device(&me, &id, &body.name) {
        Ok(true) => Ok(StatusCode::NO_CONTENT),
        Ok(false) => Err((StatusCode::NOT_FOUND, "no such device".into())),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

/// `DELETE /v1/me/devices/:id` — sign a device out. Signing out the primary
/// device from another one makes the caller the primary device.
async fn remove_device(
    State(state): State<AppState>,
    Caller(dh): Caller,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    let me = caller_username(&state, &dh)?;
    if directory::device_id(&dh) == id {
        return Err((StatusCode::BAD_REQUEST, "use another device to sign this one out".into()));
    }
    let was_primary = state.directory.identity_for(&me).is_some_and(|p| directory::device_id(&p) == id);
    match state.directory.remove_device(&me, &id, Some(&dh)) {
        Ok(true) => {
            // Drop the device's live connections and queued envelopes.
            let addr = if was_primary { me.clone() } else { format!("{me}~{id}") };
            if let Some(u) = state.users.get(&addr).map(|u| u.value().clone()) {
                let mut u = u.lock().await;
                u.tx.clear();
                u.inbox.clear();
                u.identity = None;
                u.signed_prekey = None;
                u.opks.clear();
            }
            state.directory.prekeys_delete(&addr);
            tracing::info!(user = %me, device = %id, was_primary, "device signed out");
            Ok(StatusCode::NO_CONTENT)
        }
        Ok(false) => Err((StatusCode::NOT_FOUND, "no such device".into())),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

// ---------- Admin: federation ----------------------------------------------

async fn admin_get_fed(State(state): State<AppState>, headers: axum::http::HeaderMap) -> Result<Json<serde_json::Value>, StatusCode> {
    admin_authed(&state, &headers)?;
    let c = fed::FedConfig::load();
    Ok(Json(serde_json::json!({
        "active": fed::current(&state).is_some(),
        "has_secret": !c.secret.is_empty(),
        "peers": c.peers,
        "public_url": c.public_url,
    })))
}

#[derive(Deserialize)]
struct FedUpdate {
    #[serde(default)]
    secret: Option<String>,
    #[serde(default)]
    peers: Vec<String>,
    #[serde(default)]
    public_url: String,
}

async fn admin_set_fed(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(u): Json<FedUpdate>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    admin_authed(&state, &headers).map_err(|c| (c, String::new()))?;
    let mut c = fed::FedConfig::load();
    if let Some(sec) = u.secret.filter(|s| !s.trim().is_empty()) {
        if sec.trim().len() < 16 {
            return Err((StatusCode::BAD_REQUEST, "the shared secret must be at least 16 characters".into()));
        }
        c.secret = sec.trim().to_string();
    }
    c.peers = u.peers.into_iter().map(|p| p.trim().trim_end_matches('/').to_string()).filter(|p| p.starts_with("http")).collect();
    c.public_url = u.public_url.trim().trim_end_matches('/').to_string();
    c.save().map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let next = fed::Fed::new(fed::FedConfig::load());
    let active = next.is_some();
    if let Ok(mut g) = state.fed.write() { *g = next; }
    Ok(Json(serde_json::json!({ "active": active })))
}

// ---------- Encrypted account backups --------------------------------------
//
// The blob is sealed on the device (aqua-client-core::backup); the server
// stores bytes it can't read. Upload / meta / delete need the account's
// identity token; a NEW device fetches with an e-mail ticket + password
// before it has the identity (which is inside the backup).

pub const BACKUP_LIMIT: usize = 64 * 1024 * 1024;

fn caller_username(state: &AppState, dh: &[u8; 32]) -> Result<String, (StatusCode, String)> {
    state.directory.username_for_identity(dh).ok_or((StatusCode::FORBIDDEN, "register a username first".into()))
}

async fn backup_put(
    State(state): State<AppState>,
    Caller(dh_pub): Caller,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let user = caller_username(&state, &dh_pub)?;
    if body.len() > BACKUP_LIMIT {
        return Err((StatusCode::PAYLOAD_TOO_LARGE, "backup is too large".into()));
    }
    if !body.starts_with(b"AQBK1") {
        return Err((StatusCode::BAD_REQUEST, "not an encrypted backup".into()));
    }
    state.directory.put_backup(&user, &body).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let (size, at) = state.directory.backup_meta(&user).unwrap_or((0, 0));
    Ok(Json(serde_json::json!({ "size": size, "updated_at": at })))
}

async fn backup_meta(
    State(state): State<AppState>,
    Caller(dh_pub): Caller,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let user = caller_username(&state, &dh_pub)?;
    Ok(Json(match state.directory.backup_meta(&user) {
        Some((size, at)) => serde_json::json!({ "exists": true, "size": size, "updated_at": at }),
        None => serde_json::json!({ "exists": false }),
    }))
}

async fn backup_delete(
    State(state): State<AppState>,
    Caller(dh_pub): Caller,
) -> Result<StatusCode, (StatusCode, String)> {
    let user = caller_username(&state, &dh_pub)?;
    state.directory.delete_backup(&user).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct BackupFetchBody { ticket: String, password: String }

/// New device: e-mail ticket + password → the encrypted backup (the ticket
/// stays valid for the sign-in that follows).
async fn backup_fetch(
    State(state): State<AppState>,
    Json(b): Json<BackupFetchBody>,
) -> Result<axum::response::Response, (StatusCode, String)> {
    use axum::response::IntoResponse;
    let (user, _) = state.directory.check_login(&b.ticket, &b.password).map_err(|e| (dir_err_status(&e), e.to_string()))?;
    match state.directory.get_backup(&user) {
        Some(data) => Ok(([(axum::http::header::CONTENT_TYPE, "application/octet-stream")], data).into_response()),
        None => Err((StatusCode::NOT_FOUND, "no backup for this account".into())),
    }
}

#[derive(Deserialize)]
struct PasswordBody { password: String }

async fn set_password(
    State(state): State<AppState>,
    Caller(dh_pub): Caller,
    Path(user): Path<UserId>,
    Json(body): Json<PasswordBody>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .directory
        .set_password(&user, &dh_pub, &body.password)
        .map_err(|e| (dir_err_status(&e), e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct SearchQuery { q: String }

async fn search_users(
    State(state): State<AppState>,
    Query(q): Query<SearchQuery>,
) -> Json<Vec<directory::Profile>> {
    Json(state.directory.search(&q.q, 20))
}

async fn get_profile(
    State(state): State<AppState>,
    Path(user): Path<UserId>,
) -> Result<Json<directory::Profile>, StatusCode> {
    state.directory.get_profile(&user).map(Json).ok_or(StatusCode::NOT_FOUND)
}

#[derive(Deserialize)]
struct UpdateProfileBody {
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    about: String,
    #[serde(default)]
    phone: String,
    /// Optional public extras (banner colours, status, pronouns…). Omitted
    /// = keep what is stored.
    #[serde(default)]
    extra: Option<serde_json::Value>,
}

fn dir_err_status(e: &directory::DirError) -> StatusCode {
    match e {
        directory::DirError::NotOwner => StatusCode::FORBIDDEN,
        directory::DirError::Db(_) => StatusCode::INTERNAL_SERVER_ERROR,
        _ => StatusCode::BAD_REQUEST,
    }
}

async fn update_profile(
    State(state): State<AppState>,
    Caller(dh_pub): Caller,
    Path(user): Path<UserId>,
    Json(body): Json<UpdateProfileBody>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .directory
        .update_profile_full(&user, &dh_pub, &body.display_name, &body.about, &body.phone, body.extra.as_ref())
        .map_err(|e| (dir_err_status(&e), e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- Public profile images (avatar / banner) -------------------------

#[derive(serde::Serialize)]
struct MediaVerResp { ok: bool, ver: i64 }

async fn put_profile_media(
    State(state): State<AppState>,
    Caller(dh_pub): Caller,
    Path((user, kind)): Path<(UserId, String)>,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> Result<Json<MediaVerResp>, (StatusCode, String)> {
    let kind = directory::MediaKind::parse(&kind).ok_or((StatusCode::NOT_FOUND, "unknown media kind".into()))?;
    let mime = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .to_string();
    let ver = state
        .directory
        .set_media(&user, &dh_pub, kind, &mime, &body)
        .map_err(|e| (dir_err_status(&e), e.to_string()))?;
    tracing::info!(%user, ?kind, bytes = body.len(), "profile media updated");
    Ok(Json(MediaVerResp { ok: true, ver }))
}

async fn delete_profile_media(
    State(state): State<AppState>,
    Caller(dh_pub): Caller,
    Path((user, kind)): Path<(UserId, String)>,
) -> Result<StatusCode, (StatusCode, String)> {
    let kind = directory::MediaKind::parse(&kind).ok_or((StatusCode::NOT_FOUND, "unknown media kind".into()))?;
    state
        .directory
        .clear_media(&user, &dh_pub, kind)
        .map_err(|e| (dir_err_status(&e), e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn get_profile_media(
    State(state): State<AppState>,
    caller: Option<Caller>,
    Path((user, kind)): Path<(UserId, String)>,
) -> Result<impl IntoResponse, StatusCode> {
    let kind = directory::MediaKind::parse(&kind).ok_or(StatusCode::NOT_FOUND)?;
    // Profile-photo privacy: the owner may limit who can download it.
    let viewer = caller.and_then(|Caller(dh)| state.directory.username_for_identity(&dh));
    if !state.directory.may_see_media(&user, viewer.as_deref()) {
        return Err(StatusCode::NOT_FOUND);
    }
    let (mime, data) = match state.directory.get_media(&user, kind) {
        Some(m) => m,
        None => {
            let f = fed::current(&state).ok_or(StatusCode::NOT_FOUND)?;
            let (mime, data) = f.fetch_bytes(&format!("/v1/fed/media/{user}/{}", kind.as_str())).await.ok_or(StatusCode::NOT_FOUND)?;
            state.directory.fed_store_media(&user, kind, &mime, &data);
            (mime, data)
        }
    };
    Ok((
        [
            (header::CONTENT_TYPE, mime),
            // Private: an ACL may apply, so shared caches must not keep it.
            (header::CACHE_CONTROL, "private, max-age=31536000, immutable".to_string()),
        ],
        data,
    ))
}

async fn put_media_acl(
    State(state): State<AppState>,
    Caller(dh_pub): Caller,
    Path(user): Path<UserId>,
    Json(acl): Json<directory::MediaAcl>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .directory
        .set_media_acl(&user, &dh_pub, &acl)
        .map_err(|e| (dir_err_status(&e), e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- Bot API --------------------------------------------------------

#[derive(Deserialize)]
struct BotRegisterBody {
    username: String,
    #[serde(default)]
    display_name: String,
}

#[derive(serde::Serialize)]
struct BotRegisterResp { ok: bool, username: String, token: String, telegram_token: String }

/// Owner (authenticated user) mints a bot account + token.
async fn bot_register(
    State(state): State<AppState>,
    Caller(dh_pub): Caller,
    Json(body): Json<BotRegisterBody>,
) -> Result<Json<BotRegisterResp>, (StatusCode, String)> {
    let username = body.username.trim().to_lowercase();
    let token = state
        .directory
        .register_bot(&username, &dh_pub, &body.display_name)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    let telegram_token = tgapi::telegram_token(&username, &token);
    Ok(Json(BotRegisterResp { ok: true, username, token, telegram_token }))
}

/// The bot token from `Authorization: Bot <token>` (or `Bearer`), else the
/// `token` query / body field (kept for old scripts; it ends up in logs).
fn bot_token_of(headers: &axum::http::HeaderMap, fallback: &str) -> String {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bot ").or_else(|| v.strip_prefix("Bearer ")))
        .map(|t| t.trim().to_string())
        .filter(|t| t.starts_with("aqbot_"))
        .unwrap_or_else(|| fallback.trim().to_string())
}

/// Resolve the calling bot; a wrong token costs a short delay.
async fn bot_auth(state: &AppState, headers: &axum::http::HeaderMap, fallback: &str) -> Result<String, (StatusCode, String)> {
    let token = bot_token_of(headers, fallback);
    match state.directory.bot_by_token(&token) {
        Some(b) => Ok(b),
        None => {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            Err((StatusCode::UNAUTHORIZED, "bad bot token".into()))
        }
    }
}

#[derive(serde::Serialize)]
struct BotInfo { username: String, token: String, display_name: String, hint: String }

/// List the bots owned by the calling identity (with their tokens, so
/// the owner can copy them).
async fn bot_list(
    State(state): State<AppState>,
    Caller(dh_pub): Caller,
) -> Json<Vec<BotInfo>> {
    let bots = state.directory.list_bots_for_owner(&dh_pub)
        .into_iter()
        .map(|(username, hint)| {
            let display_name = state.directory.display_name_of(&username);
            // The relay keeps only a hash: the token is shown once, at creation.
            BotInfo { username, token: String::new(), display_name, hint }
        })
        .collect();
    Json(bots)
}

#[derive(Deserialize)]
struct BotUpdatesQuery {
    #[serde(default)]
    token: String,
    #[serde(default)]
    after: u64,
    /// Long poll: wait up to this many seconds (max 30) for a new update.
    #[serde(default)]
    timeout: u64,
}

/// A bot program polls for messages addressed to it. Returns updates
/// with id >= `after`; with `timeout` it waits (long poll) until there is
/// one or the time is up.
async fn bot_updates(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Query(q): Query<BotUpdatesQuery>,
) -> Result<Json<Vec<BotUpdate>>, (StatusCode, String)> {
    let bot = bot_auth(&state, &headers, &q.token).await?;
    if !state.bot_limits.take(&format!("poll:{bot}"), 20.0, 5.0) {
        return Err((StatusCode::TOO_MANY_REQUESTS, "polling too fast".into()));
    }
    let queue = state.bot_queue(&bot);
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(q.timeout.min(30));
    loop {
        let notified = {
            let mut qd = queue.lock().await;
            // Asking for `after` acknowledges everything before it.
            while qd.updates.front().is_some_and(|u| u.id < q.after) {
                qd.updates.pop_front();
            }
            if !qd.updates.is_empty() || tokio::time::Instant::now() >= deadline {
                return Ok(Json(qd.updates.iter().cloned().collect()));
            }
            qd.notify.clone()
        };
        let wait = notified.notified();
        let _ = tokio::time::timeout_at(deadline, wait).await;
    }
}

#[derive(Deserialize)]
struct BotTokenQuery {
    #[serde(default)]
    token: String,
}

/// Who am I (for bot programs): `{ username, display_name }`.
async fn bot_me(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Query(q): Query<BotTokenQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let bot = bot_auth(&state, &headers, &q.token).await?;
    let name = state.directory.display_name_of(&bot);
    Ok(Json(serde_json::json!({ "username": bot, "display_name": name, "id": tgapi::tg_id(&bot) })))
}

/// Owner deletes one of their bots.
async fn bot_delete(
    State(state): State<AppState>,
    Caller(dh_pub): Caller,
    Path(name): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    state.directory.delete_bot(&name, &dh_pub).map_err(|e| (dir_err_status(&e), e.to_string()))?;
    state.bot_queues.remove(&name.trim().to_lowercase());
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct BotSendBody {
    #[serde(default)]
    token: String,
    to: String,
    text: String,
}

/// A bot program sends a plaintext message to a user. Bots answer people
/// who wrote to them (and their owner) — they can't cold-message anyone —
/// at most 30 messages a second overall and one a second per chat
/// (bursts of 5).
async fn bot_send(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<BotSendBody>,
) -> Result<StatusCode, (StatusCode, String)> {
    let bot = bot_auth(&state, &headers, &body.token).await?;
    let to = body.to.trim().trim_start_matches('@').to_lowercase();
    if body.text.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "empty text".into()));
    }
    if body.text.chars().count() > BOT_TEXT_LIMIT {
        return Err((StatusCode::PAYLOAD_TOO_LARGE, format!("text is longer than {BOT_TEXT_LIMIT} characters")));
    }
    // The owner may also be connected under a name only the relay's
    // session table knows (open-signup relays).
    let owner_online = state.directory.bot_owner(&bot)
        .is_some_and(|o| state.bindings.get(&to).is_some_and(|b| b.as_slice() == o.as_slice()));
    if !owner_online && !state.directory.bot_may_message(&bot, &to) {
        return Err((StatusCode::FORBIDDEN, format!("@{to} hasn't written to this bot (or sent /stop)")));
    }
    if !state.bot_limits.take(&format!("send:{bot}"), 30.0, 30.0)
        || !state.bot_limits.take(&format!("send:{bot}>{to}"), 5.0, 1.0)
    {
        return Err((StatusCode::TOO_MANY_REQUESTS, "slow down: 30 msg/s per bot, 1 msg/s per chat".into()));
    }
    bot_deliver_text(&state, &bot, &to, &body.text).await;
    Ok(StatusCode::NO_CONTENT)
}

/// A bot's text to a user: every device of the account gets it.
pub(crate) async fn bot_deliver_text(state: &AppState, bot: &str, to: &str, text: &str) {
    for addr in state.addresses_of(to) {
        let msg = RelayMessage {
            from: bot.to_string(),
            to: addr,
            payload: text.as_bytes().to_vec(),
            kind: PayloadKind::Plaintext,
        };
        deliver(state, msg).await;
    }
}

// ---------- Game catalogue ---------------------------------------------------

#[derive(Deserialize)]
struct GameQuery { #[serde(default)] q: String }

async fn games_search(State(state): State<AppState>, Query(q): Query<GameQuery>) -> Json<Vec<games::Hit>> {
    let q = q.q.chars().take(64).collect::<String>();
    let list = state.games.get().await;
    Json(games::search(&list, &q, 20))
}

async fn games_cover(State(state): State<AppState>, Path(id): Path<String>) -> Result<impl IntoResponse, StatusCode> {
    let (mime, bytes) = state.games.cover(&id).await.ok_or(StatusCode::NOT_FOUND)?;
    Ok(([(header::CONTENT_TYPE, mime), (header::CACHE_CONTROL, "public, max-age=604800".to_string())], bytes))
}

async fn games_icon(State(state): State<AppState>, Path(id): Path<String>) -> Result<impl IntoResponse, StatusCode> {
    let b = state.games.icon(&id).await.ok_or(StatusCode::NOT_FOUND)?;
    Ok(([(header::CONTENT_TYPE, "image/png"), (header::CACHE_CONTROL, "public, max-age=604800")], b))
}

// ---------- Server stats (for the control-panel GUI) -----------------------

#[derive(serde::Serialize)]
struct StatsResp {
    ok: bool,
    users: i64,
    bots: i64,
    online: usize,
    require_registration: bool,
}

/// Lightweight, unauthenticated server stats for the admin GUI: how many
/// accounts are registered and how many clients are connected right now.
/// No identities, emails or message content — just counts.
async fn server_stats(State(state): State<AppState>) -> Json<StatsResp> {
    let (users, bots) = state.directory.counts();
    // Count users with at least one live WS sender.
    let mut online = 0usize;
    for entry in state.users.iter() {
        let u = entry.value().lock().await;
        if u.tx.iter().any(|t| !t.is_closed()) {
            online += 1;
        }
    }
    Json(StatsResp { ok: true, users, bots, online, require_registration: state.require_registration })
}

// ---------- Admin panel ------------------------------------------------------
//
// A small web panel (in the client's neon style) for the relay operator.
// Gated by a bearer token from `AQUA_ADMIN_TOKEN`; when that env var is unset
// the panel and its API are disabled. The panel shows live stats and edits
// the access policy (open vs invite-only + a registration allowlist).

pub(crate) fn admin_authed(state: &AppState, headers: &axum::http::HeaderMap) -> Result<(), StatusCode> {
    let want = std::env::var("AQUA_ADMIN_TOKEN").unwrap_or_default();
    let _ = state;
    if want.is_empty() {
        return Err(StatusCode::NOT_FOUND); // panel disabled
    }
    let got = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    // Constant-time-ish compare.
    if got.len() == want.len()
        && got.bytes().zip(want.bytes()).fold(0u8, |a, (x, y)| a | (x ^ y)) == 0
    {
        Ok(())
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

#[derive(serde::Serialize)]
struct AdminStats {
    /// Address the relay listens on (AQUA_LISTEN).
    listen: String,
    /// This machine's LAN address, for phones on the same Wi-Fi.
    lan_ip: Option<String>,
    /// Codes are shown here instead of e-mailed.
    dev_codes: bool,
    /// Recent dev-mode verification codes, newest first.
    codes: Vec<RecentCode>,
    users: usize,
    bots: usize,
    online: usize,
    blobs: usize,
    blob_bytes: u64,
    uptime_secs: u64,
    require_registration: bool,
    invite_only: bool,
    allow: Vec<String>,
}

#[derive(serde::Serialize)]
struct RecentCode {
    email: String,
    code: String,
    at: u64,
}

/// Primary LAN IPv4 of this machine (no packets are sent: connecting a UDP
/// socket only asks the OS which interface would be used).
pub fn lan_ip() -> Option<String> {
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("8.8.8.8:80").ok()?;
    let ip = sock.local_addr().ok()?.ip();
    if ip.is_loopback() || ip.is_unspecified() { None } else { Some(ip.to_string()) }
}

async fn admin_stats(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<AdminStats>, StatusCode> {
    admin_authed(&state, &headers)?;
    let (users, bots) = state.directory.counts();
    let mut online = 0usize;
    for entry in state.users.iter() {
        let u = entry.value().lock().await;
        if u.tx.iter().any(|t| !t.is_closed()) {
            online += 1;
        }
    }
    let (blobs, blob_bytes) = state.blobs.stats();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let cfg = state.admin.read().map(|c| c.clone()).unwrap_or_default();
    Ok(Json(AdminStats {
        listen: std::env::var("AQUA_LISTEN").unwrap_or_else(|_| "127.0.0.1:7777".into()),
        lan_ip: lan_ip(),
        dev_codes: state.directory.dev_codes(),
        codes: state
            .directory
            .recent_codes()
            .into_iter()
            .map(|(email, code, at)| RecentCode { email, code, at })
            .collect(),
        users: users as usize,
        bots: bots as usize,
        online,
        blobs,
        blob_bytes,
        uptime_secs: now.saturating_sub(state.started_unix),
        require_registration: state.require_registration,
        invite_only: cfg.invite_only,
        allow: cfg.allow,
    }))
}

async fn admin_set_config(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<AdminConfig>,
) -> Result<Json<AdminConfig>, StatusCode> {
    admin_authed(&state, &headers)?;
    let clean = AdminConfig {
        invite_only: body.invite_only,
        allow: body
            .allow
            .into_iter()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect(),
    };
    if let Ok(mut c) = state.admin.write() {
        *c = clean.clone();
        c.save();
    }
    Ok(Json(clean))
}

// ---- E-mail delivery (SMTP) settings from the admin panel ----

#[derive(serde::Serialize)]
struct SmtpView {
    /// "env" | "file" | "none"
    source: &'static str,
    host: String,
    port: u16,
    user: String,
    from: String,
    /// Never send the password back — only whether one is stored.
    has_password: bool,
    dev_codes: bool,
}

#[derive(Deserialize)]
struct SmtpUpdate {
    /// false = turn real e-mail off (back to dev codes).
    enabled: bool,
    #[serde(default)] host: String,
    #[serde(default)] port: u16,
    #[serde(default)] user: String,
    #[serde(default)] from: String,
    /// Empty = keep the stored password.
    #[serde(default)] pass: String,
}

#[derive(Deserialize)]
struct SmtpTest { to: String }

fn smtp_view(state: &AppState) -> SmtpView {
    let (cfg, source) = state.directory.smtp();
    let dev = state.directory.dev_codes();
    match cfg {
        Some(c) => SmtpView { source, host: c.host, port: c.port, user: c.user, from: c.from, has_password: !c.pass.is_empty(), dev_codes: dev },
        None => SmtpView { source, host: String::new(), port: 587, user: String::new(), from: String::new(), has_password: false, dev_codes: dev },
    }
}

async fn admin_get_smtp(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<SmtpView>, StatusCode> {
    admin_authed(&state, &headers)?;
    Ok(Json(smtp_view(&state)))
}

async fn admin_set_smtp(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(u): Json<SmtpUpdate>,
) -> Result<Json<SmtpView>, (StatusCode, String)> {
    admin_authed(&state, &headers).map_err(|c| (c, String::new()))?;
    let (current, source) = state.directory.smtp();
    if source == "env" {
        return Err((StatusCode::CONFLICT, "SMTP is set by AQUA_SMTP_* environment variables; change them there".into()));
    }
    let next = if !u.enabled {
        None
    } else {
        let pass = if u.pass.is_empty() { current.map(|c| c.pass).unwrap_or_default() } else { u.pass };
        let user = u.user.trim().to_string();
        if u.host.trim().is_empty() || user.is_empty() || pass.is_empty() {
            return Err((StatusCode::BAD_REQUEST, "server, login and password are required".into()));
        }
        let from = if u.from.trim().is_empty() { user.clone() } else { u.from.trim().to_string() };
        Some(directory::SmtpConfig {
            host: u.host.trim().to_string(),
            port: if u.port == 0 { 587 } else { u.port },
            user,
            pass,
            from,
        })
    };
    state.directory.set_smtp(next).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(smtp_view(&state)))
}

async fn admin_test_smtp(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(t): Json<SmtpTest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    admin_authed(&state, &headers).map_err(|c| (c, String::new()))?;
    match state.directory.send_test_email(&t.to).await {
        Ok(()) => Ok(Json(serde_json::json!({ "ok": true }))),
        Err(e) => Err((StatusCode::BAD_GATEWAY, format!("{e:#}"))),
    }
}

async fn admin_page(State(state): State<AppState>) -> impl IntoResponse {
    if std::env::var("AQUA_ADMIN_TOKEN").unwrap_or_default().is_empty() {
        let _ = state;
        return (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            "Admin panel disabled. Set AQUA_ADMIN_TOKEN and restart.".to_string(),
        );
    }
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        include_str!("admin.html").to_string(),
    )
}

// ---------- Public router constructor ----------------------------------------

pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/v1/stats", get(server_stats))
        .route("/admin", get(admin_page))
        .route("/v1/admin/stats", get(admin_stats))
        .route("/v1/admin/config", post(admin_set_config))
        .route("/v1/admin/smtp", get(admin_get_smtp).post(admin_set_smtp))
        .route("/v1/admin/smtp/test", post(admin_test_smtp))
        .route("/v1/admin/telegram", get(packs::admin_get).post(packs::admin_set))
        .route("/v1/packs", get(packs::list))
        .route("/v1/packs/import", post(packs::import_http))
        .route("/v1/packs/:name", get(packs::manifest))
        .route("/v1/packs/:name/refresh", post(packs::refresh_http))
        .route("/v1/packs/:name/:file", get(packs::file))
        .route("/v1/auth/challenge", post(auth::challenge))
        .route("/v1/auth/verify",    post(auth::verify))
        .route("/v1/auth/email/request", post(email_request))
        .route("/v1/auth/email/verify",  post(email_verify))
        .route("/v1/users/register",     post(register_user))
        .route("/v1/users/search",       get(search_users))
        .route("/v1/users/login",        post(login_user))
        .route("/v1/users/reset",        post(reset_password))
        .route("/v1/webapp/init",        post(webapp_init))
        .route("/v1/capsules",           get(capsules_mine).put(capsule_put))
        .route("/v1/capsules/:id/open",  post(capsule_open))
        .route("/v1/capsules/:id/stop",  post(capsule_stop))
        .route("/v1/webapp/menu/:bot",   get(webapp_menu))
        .route("/v1/me/recovery",        get(recovery_get).put(recovery_put))
        .route("/v1/devices/:user",      get(get_devices))
        .route("/v1/me/devices",         get(my_devices))
        .route("/v1/me/devices/:id",     post(rename_device).delete(remove_device))
        .route("/v1/users/:user/password", post(set_password))
        .route("/v1/users/:user/profile", get(get_profile).post(update_profile))
        .route(
            "/v1/users/:user/media/:kind",
            get(get_profile_media).post(put_profile_media).delete(delete_profile_media)
                .layer(DefaultBodyLimit::max(directory::PROFILE_MEDIA_LIMIT + 1024)),
        )
        .route("/v1/users/:user/media_acl", post(put_media_acl))
        .route("/v1/games/search",       get(games_search))
        .route("/v1/games/icon/:id",     get(games_icon))
        .route("/v1/games/cover/:id",    get(games_cover))
        .route("/v1/bot/register",       post(bot_register))
        .route("/v1/bot/list",           get(bot_list))
        .route("/v1/bot/updates",        get(bot_updates))
        .route("/v1/bot/send",           post(bot_send))
        .route("/v1/bot/me",             get(bot_me))
        .route("/v1/bot/:name",          axum::routing::delete(bot_delete))
        .route("/v1/backup", axum::routing::put(backup_put).get(backup_meta).delete(backup_delete)
            .layer(DefaultBodyLimit::max(BACKUP_LIMIT + 1024)))
        .route("/v1/backup/fetch", post(backup_fetch))
        .route("/v1/fed/peers", get(fed::h_peers))
        .route("/v1/fed/deliver", post(fed::h_deliver))
        .route("/v1/fed/poke", post(fed::h_poke))
        .route("/v1/fed/inbox/:user", get(fed::h_inbox))
        .route("/v1/fed/bundle/:user", get(fed::h_bundle))
        .route("/v1/fed/blob/:cid", get(fed::h_blob))
        .route("/v1/fed/users", get(fed::h_users))
        .route("/v1/fed/backups", get(fed::h_backups))
        .route("/v1/fed/backup/:user", get(fed::h_backup))
        .route("/v1/fed/media/:user/:kind", get(fed::h_media))
        .route("/v1/admin/federation", get(admin_get_fed).post(admin_set_fed))
        .route("/v1/bundle/:user",   post(upload_bundle).get(fetch_bundle))
        .route("/v1/ws",             get(ws_upgrade))
        // Telegram Bot API: /bot<token>/<method> (see tgapi.rs).
        .route("/:bot/:method",      get(tgapi::handle).post(tgapi::handle)
            .layer(DefaultBodyLimit::max(20 * 1024 * 1024)))
        .route(
            "/v1/blob/:cid",
            post(upload_blob).get(fetch_blob)
                .layer(DefaultBodyLimit::max(BLOB_LIMIT)),
        )
        // Everything else: the optional website (AQUA_SITE_DIR, see site.rs).
        .fallback(site::fallback)
        .with_state(state)
}

/// First 8 bytes of an identity DH pubkey, hex-encoded. Handy for logs.
fn short_hex(bytes: &[u8]) -> String {
    hex::encode(&bytes[..bytes.len().min(8)])
}
