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
pub mod directory;
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
}

#[derive(Default)]
pub struct BotQueue {
    pub next_id: u64,
    pub updates: VecDeque<BotUpdate>,
}

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
    pub blobs: Arc<DashMap<String, Bytes>>,
    /// Persistent registry of @usernames + public profile cards.
    pub directory: Arc<Directory>,
    /// Per-bot inbound message queues (bot username → queue). Ephemeral
    /// — bots are expected to be online and polling.
    pub bot_queues: Arc<DashMap<String, Arc<Mutex<BotQueue>>>>,
    /// When true, only usernames registered in the directory (email-verified)
    /// may upload a bundle or open a WebSocket. This makes email registration
    /// mandatory — no ephemeral unregistered handles. Toggle off for an open
    /// relay with `AQUA_OPEN_SIGNUP=1`.
    pub require_registration: bool,
    /// Admin panel state: access mode + registration allowlist, persisted
    /// to `aqua-admin.json`. Shared behind an RwLock; never held across await.
    pub admin: Arc<std::sync::RwLock<AdminConfig>>,
    /// Unix seconds the process started, for the uptime readout.
    pub started_unix: u64,
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
            blobs: Arc::new(DashMap::new()),
            directory,
            bot_queues: Arc::new(DashMap::new()),
            require_registration,
            admin: Arc::new(std::sync::RwLock::new(AdminConfig::load())),
            started_unix: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
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
        if let Some(owner) = self.directory.identity_for(user_id) {
            return if owner == identity_dh_pub {
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
    let user_state = state.user(&user);
    let mut u = user_state.lock().await;
    u.identity = Some(body.identity);
    u.signed_prekey = Some(body.signed_prekey);
    let opk_count = body.one_time_prekeys.len();
    for opk in body.one_time_prekeys {
        u.opks.push_back(opk);
    }
    tracing::info!(user = %user, token = %token_fp, opk_count,
        "upload_bundle OK: bundle stored");
    Ok(StatusCode::NO_CONTENT)
}

async fn fetch_bundle(
    State(state): State<AppState>,
    Path(user): Path<UserId>,
) -> Result<Json<PreKeyBundle>, StatusCode> {
    let user_state = state.user(&user);
    let mut u = user_state.lock().await;
    let identity = u.identity.ok_or(StatusCode::NOT_FOUND)?;
    let signed_prekey = u.signed_prekey.clone().ok_or(StatusCode::NOT_FOUND)?;
    let one_time_prekey = u.opks.pop_front();
    Ok(Json(PreKeyBundle {
        identity,
        signed_prekey,
        one_time_prekey,
    }))
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
    tracing::info!(user = %q.user, token = %token_fp, "WS OK: upgrading to WebSocket");
    let me = q.user;
    Ok(ws.on_upgrade(move |sock| ws_run(sock, state, me)))
}

async fn ws_run(mut socket: WebSocket, state: AppState, me: UserId) {
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
}

async fn deliver(state: &AppState, msg: RelayMessage) {
    // If the recipient is a registered bot, queue the (plaintext) text
    // into its bot queue instead of the normal user inbox — bots pull
    // via GET /v1/bot/updates, they don't hold a WebSocket.
    if msg.kind == PayloadKind::Plaintext && state.directory.is_bot(&msg.to) {
        let q = state.bot_queue(&msg.to);
        let mut q = q.lock().await;
        let id = q.next_id;
        q.next_id += 1;
        let text = String::from_utf8_lossy(&msg.payload).to_string();
        q.updates.push_back(BotUpdate {
            id,
            from: msg.from.clone(),
            text,
            ts: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        });
        // Cap the queue so an idle bot doesn't grow unbounded.
        while q.updates.len() > 1000 { q.updates.pop_front(); }
        return;
    }

    let recipient = state.user(&msg.to);
    let mut r = recipient.lock().await;
    let mut delivered = false;
    r.tx.retain(|t| !t.is_closed());
    for t in &r.tx {
        if t.send(msg.clone()).is_ok() {
            delivered = true;
        }
    }
    if !delivered {
        r.inbox.push_back(msg);
    }
}

// ---------- Blob storage (encrypted file bodies) ---------------------------

/// Sanity cap on blob size accepted by the server. Larger blobs should
/// be broken into chunks by the client. Matches
/// `aqua_core::file_transfer::MAX_FILE_SIZE` plus AEAD tag headroom.
const BLOB_LIMIT: usize = 210 * 1024 * 1024;

async fn upload_blob(
    State(state): State<AppState>,
    Caller(_dh_pub): Caller,
    Path(cid): Path<String>,
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
    // Content-addressed: skip re-store on duplicate id.
    state.blobs.entry(cid).or_insert(body);
    Ok(StatusCode::NO_CONTENT)
}

async fn fetch_blob(
    State(state): State<AppState>,
    Path(cid): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    let Some(entry) = state.blobs.get(&cid) else {
        return Err(StatusCode::NOT_FOUND);
    };
    let bytes = entry.value().clone();
    Ok((
        [(header::CONTENT_TYPE, "application/octet-stream")],
        bytes,
    ))
}

// ---------- User directory: email registration + search --------------------

#[derive(Deserialize)]
struct EmailRequestBody { email: String }

#[derive(serde::Serialize)]
struct EmailRequestResp { ok: bool, dev_code: Option<String> }

async fn email_request(
    State(state): State<AppState>,
    Json(body): Json<EmailRequestBody>,
) -> Result<Json<EmailRequestResp>, (StatusCode, String)> {
    // Invite-only (solo) relays: refuse before sending a code.
    if !state.admin.read().map(|c| c.permits(&body.email)).unwrap_or(true) {
        return Err((StatusCode::FORBIDDEN, "this relay is invite-only — ask the admin to add your email".into()));
    }
    match state.directory.request_code(&body.email) {
        Ok(dev_code) => Ok(Json(EmailRequestResp { ok: true, dev_code })),
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
}

#[derive(serde::Serialize)]
struct LoginResp { username: String }

/// Sign in on a new device (email ticket + password) — the username moves
/// to the caller's identity key.
async fn login_user(
    State(state): State<AppState>,
    Caller(dh_pub): Caller,
    Json(body): Json<LoginBody>,
) -> Result<Json<LoginResp>, (StatusCode, String)> {
    let username = state
        .directory
        .login(&body.ticket, &body.password, &dh_pub)
        .map_err(|e| (dir_err_status(&e), e.to_string()))?;
    tracing::info!(user = %username, "signed in on a new device");
    Ok(Json(LoginResp { username }))
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
    let (mime, data) = state.directory.get_media(&user, kind).ok_or(StatusCode::NOT_FOUND)?;
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
struct BotRegisterResp { ok: bool, username: String, token: String }

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
    Ok(Json(BotRegisterResp { ok: true, username, token }))
}

#[derive(serde::Serialize)]
struct BotInfo { username: String, token: String }

/// List the bots owned by the calling identity (with their tokens, so
/// the owner can copy them).
async fn bot_list(
    State(state): State<AppState>,
    Caller(dh_pub): Caller,
) -> Json<Vec<BotInfo>> {
    let bots = state.directory.list_bots_for_owner(&dh_pub)
        .into_iter()
        .map(|(username, token)| BotInfo { username, token })
        .collect();
    Json(bots)
}

#[derive(Deserialize)]
struct BotUpdatesQuery {
    token: String,
    #[serde(default)]
    after: u64,
}

/// A bot program polls for messages addressed to it. Returns updates
/// with id > `after`. (Simple immediate return — the bot loops with a
/// short sleep; a real long-poll can come later.)
async fn bot_updates(
    State(state): State<AppState>,
    Query(q): Query<BotUpdatesQuery>,
) -> Result<Json<Vec<BotUpdate>>, (StatusCode, String)> {
    let bot = state.directory.bot_by_token(&q.token)
        .ok_or((StatusCode::UNAUTHORIZED, "bad bot token".into()))?;
    let queue = state.bot_queue(&bot);
    let qd = queue.lock().await;
    let out: Vec<BotUpdate> = qd.updates.iter().filter(|u| u.id >= q.after).cloned().collect();
    Ok(Json(out))
}

#[derive(Deserialize)]
struct BotSendBody {
    token: String,
    to: String,
    text: String,
}

/// A bot program sends a plaintext message to a user.
async fn bot_send(
    State(state): State<AppState>,
    Json(body): Json<BotSendBody>,
) -> Result<StatusCode, (StatusCode, String)> {
    let bot = state.directory.bot_by_token(&body.token)
        .ok_or((StatusCode::UNAUTHORIZED, "bad bot token".into()))?;
    let msg = RelayMessage {
        from: bot,
        to: body.to.trim().to_lowercase(),
        payload: body.text.into_bytes(),
        kind: PayloadKind::Plaintext,
    };
    deliver(&state, msg).await;
    Ok(StatusCode::NO_CONTENT)
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
    let blobs = state.blobs.len();
    let blob_bytes: u64 = state.blobs.iter().map(|e| e.value().len() as u64).sum();
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
        .route("/v1/packs/:name/:file", get(packs::file))
        .route("/v1/auth/challenge", post(auth::challenge))
        .route("/v1/auth/verify",    post(auth::verify))
        .route("/v1/auth/email/request", post(email_request))
        .route("/v1/auth/email/verify",  post(email_verify))
        .route("/v1/users/register",     post(register_user))
        .route("/v1/users/search",       get(search_users))
        .route("/v1/users/login",        post(login_user))
        .route("/v1/users/:user/password", post(set_password))
        .route("/v1/users/:user/profile", get(get_profile).post(update_profile))
        .route(
            "/v1/users/:user/media/:kind",
            get(get_profile_media).post(put_profile_media).delete(delete_profile_media)
                .layer(DefaultBodyLimit::max(directory::PROFILE_MEDIA_LIMIT + 1024)),
        )
        .route("/v1/users/:user/media_acl", post(put_media_acl))
        .route("/v1/bot/register",       post(bot_register))
        .route("/v1/bot/list",           get(bot_list))
        .route("/v1/bot/updates",        get(bot_updates))
        .route("/v1/bot/send",           post(bot_send))
        .route("/v1/bundle/:user",   post(upload_bundle).get(fetch_bundle))
        .route("/v1/ws",             get(ws_upgrade))
        .route(
            "/v1/blob/:cid",
            post(upload_blob).get(fetch_blob)
                .layer(DefaultBodyLimit::max(BLOB_LIMIT)),
        )
        .with_state(state)
}

/// First 8 bytes of an identity DH pubkey, hex-encoded. Handy for logs.
fn short_hex(bytes: &[u8]) -> String {
    hex::encode(&bytes[..bytes.len().min(8)])
}
