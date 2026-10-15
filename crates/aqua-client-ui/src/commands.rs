//! Tauri commands exposed to the frontend.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex as StdMutex},
    time::SystemTime,
};

use aqua_client_core::{PrekeyManager, Transport};
use aqua_core::{
    file_transfer::{decrypt_file, encrypt_file_with_category, FileEnvelope},
    session::Session,
    wire::BundleUpload,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::state::{
    classify_kind, gradient_for, group_summary, hex_fingerprint_from, hhmm_now, preview_for,
    save_groups, summary_for, text_msg_from_me, word_fingerprint_from, AppState, ChatBook,
    ChatMessage, ChatSummary, Group, GroupMember, GroupSettings, IncomingEventPayload, InitInfo,
    MessageKind,
};
use crate::media;

// ---------- Startup -------------------------------------------------------

#[tauri::command]
pub fn init_app(state: State<'_, StdMutex<AppState>>) -> Result<InitInfo, String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    Ok(s.init_info())
}

/// Mark the device as registered after the email-registration wizard
/// finishes. Returns the refreshed init info so the UI can drop the gate.
#[tauri::command]
pub fn complete_registration(
    state: State<'_, StdMutex<AppState>>,
    username: String,
    email: Option<String>,
) -> Result<InitInfo, String> {
    let mut s = state.lock().map_err(|e| e.to_string())?;
    s.mark_registered(username.trim().to_lowercase(), email);
    Ok(s.init_info())
}

/// Log out: forget the account + auto-reconnect hints and drop the live
/// transport. The identity/vault is preserved. Returns refreshed init info
/// (registered=false) so the UI reopens the login window.
#[tauri::command]
pub fn log_out(state: State<'_, StdMutex<AppState>>) -> Result<InitInfo, String> {
    let mut s = state.lock().map_err(|e| e.to_string())?;
    s.log_out();
    Ok(s.init_info())
}

// ---------- Chats & messages ----------------------------------------------

#[tauri::command]
pub fn list_chats(state: State<'_, StdMutex<AppState>>) -> Result<Vec<ChatSummary>, String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    Ok(s.chat_order
        .iter()
        .filter_map(|id| s.chats.get(id).map(|c| c.summary.clone()))
        .collect())
}

#[tauri::command]
pub fn get_messages(
    state: State<'_, StdMutex<AppState>>,
    peer_id: String,
) -> Result<Vec<ChatMessage>, String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    Ok(s.chats.get(&peer_id).map(|c| c.messages.clone()).unwrap_or_default())
}

#[derive(Debug, serde::Serialize)]
pub struct SendResult {
    pub id: String,
    pub time: String,
}

#[tauri::command]
pub async fn send_message(
    state: State<'_, StdMutex<AppState>>,
    peer_id: String,
    text: String,
) -> Result<SendResult, String> {
    // Saved Messages — local-only self-chat, no server round-trip.
    if peer_id == crate::state::SAVED_MESSAGES_ID {
        let (msg_id, msg_time) = {
            let mut s = state.lock().map_err(|e| e.to_string())?;
            let msg = text_msg_from_me(&text);
            let out = (msg.id.clone(), msg.time.clone());
            push_own_message(&mut s, &peer_id, msg, &text);
            let data_dir = s.data_dir.clone();
            if let Some(book) = s.chats.get(&peer_id) {
                crate::state::save_saved_messages(&data_dir, &book.messages);
            }
            out
        };
        return Ok(SendResult { id: msg_id, time: msg_time });
    }

    // Bot chat (plaintext, no E2EE): live && !has_lock. Send the text
    // un-encrypted through the relay; no session / ratchet.
    {
        let is_bot_chat = {
            let s = state.lock().map_err(|e| e.to_string())?;
            s.chats.get(&peer_id).map(|c| c.summary.live && !c.summary.has_lock).unwrap_or(false)
        };
        if is_bot_chat {
            let (transport, out_msg) = {
                let mut s = state.lock().map_err(|e| e.to_string())?;
                let msg = text_msg_from_me(&text);
                push_own_message(&mut s, &peer_id, msg.clone(), &text);
                (s.transport.clone(), msg)
            };
            let Some(transport) = transport else { return Err("not connected to a server".into()); };
            transport.send_plaintext(&peer_id, &text).map_err(|e| format!("send: {e}"))?;
            return Ok(SendResult { id: out_msg.id, time: out_msg.time });
        }
    }

    // Under the lock: validate the chat is live, append local echo,
    // encrypt, persist state. Take transport handle out to send outside
    // the lock.
    let (transport, envelope_opt, out_msg) = {
        let mut s = state.lock().map_err(|e| e.to_string())?;
        let is_live = s.chats.get(&peer_id).map(|c| c.summary.live).unwrap_or(false);
        if !is_live {
            // Welcome / stub chats aren't attached to any real peer.
            // Refuse here so the user doesn't end up chatting with
            // themselves and thinking it's working — surface a clear
            // instruction instead.
            return Err(
                "This chat has no live session. Click the plug icon \
                 bottom-left to connect, then use + to start a real chat."
                    .into(),
            );
        }
        let msg = text_msg_from_me(&text);
        push_own_message(&mut s, &peer_id, msg.clone(), &text);

        let env = {
            let session = s.sessions.get_mut(&peer_id).ok_or("no session for peer")?;
            session.encrypt(text.as_bytes()).map_err(|e| format!("encrypt: {e}"))?
        };
        // Persist advanced ratchet state (re-borrow immutably).
        if let Some(session) = s.sessions.get(&peer_id) {
            if let Err(e) = s.vault.save_session(&peer_id, session) {
                tracing::warn!(?e, "save_session after encrypt");
            }
        }
        (s.transport.clone(), Some(env), msg.clone())
    };

    let Some(env) = envelope_opt else {
        // Should be unreachable now that we refuse non-live sends above.
        return Err("internal: no envelope produced".into());
    };
    let Some(transport) = transport else {
        return Err("not connected to a server".into());
    };
    transport.send_envelope(&peer_id, &env)
        .map_err(|e| format!("send: {e}"))?;
    Ok(SendResult { id: out_msg.id, time: out_msg.time })
}

// ---------- Verification --------------------------------------------------

#[derive(Debug, serde::Serialize)]
pub struct FingerprintView {
    pub words: Vec<String>,
    pub hex: String,
    pub peer_name: String,
}

#[tauri::command]
pub fn verify_fingerprint(
    state: State<'_, StdMutex<AppState>>,
    peer_id: String,
) -> Result<FingerprintView, String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    let dh_pub = if let Some(sess) = s.sessions.get(&peer_id) {
        *sess.peer.dh_pub.as_bytes()
    } else {
        *s.identity.public().dh_pub.as_bytes()
    };
    let name = s.chats.get(&peer_id).map(|c| c.summary.name.clone()).unwrap_or(peer_id);
    Ok(FingerprintView {
        words: word_fingerprint_from(&dh_pub),
        hex:   hex_fingerprint_from(&dh_pub),
        peer_name: name,
    })
}

#[tauri::command]
pub fn session_fingerprint(state: State<'_, StdMutex<AppState>>) -> Result<String, String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    Ok(s.init_info().session_fingerprint)
}

// ---------- Live transport ------------------------------------------------

/// Route all outbound traffic (HTTP + the relay WebSocket) through a SOCKS5
/// proxy — the "connect through a server" option. Pass an empty string to go
/// direct. Takes effect on the next connect; persisted for future launches.
#[tauri::command]
pub fn set_network_proxy(
    state: State<'_, StdMutex<AppState>>,
    url: String,
) -> Result<InitInfo, String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    let trimmed = url.trim();
    let val = if trimmed.is_empty() { None } else { Some(trimmed.to_string()) };
    aqua_client_core::transport::set_proxy(val.clone());
    crate::state::save_proxy(&s.data_dir, val.as_deref());
    Ok(s.init_info())
}

#[tauri::command]
pub async fn connect_to_server(
    app: AppHandle,
    state: State<'_, StdMutex<AppState>>,
    server_url: String,
    username: String,
) -> Result<InitInfo, String> {
    // Build the bundle to upload + clone the identity out of the state.
    let (identity_clone, bundle) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        let pm = PrekeyManager::new(&s.vault, &s.identity);
        pm.check_and_refresh(SystemTime::now())
            .map_err(|e| format!("prekeys: {e}"))?;
        let (spk_secret, _generated_at) = s.vault.latest_signed_prekey()
            .map_err(|e| format!("vault: {e}"))?
            .ok_or("no SPK — vault broken?")?;
        let opk_pubs = s.vault.list_one_time_prekey_publics()
            .map_err(|e| format!("vault: {e}"))?;
        (
            s.identity.clone(),
            BundleUpload {
                identity: s.identity.public(),
                signed_prekey: spk_secret.public(),
                one_time_prekeys: opk_pubs,
            },
        )
    };

    // Sign in + open WS (no state lock across await).
    let (transport, mut incoming) =
        Transport::connect(&server_url, &username, &identity_clone).await
            .map_err(|e| format!("connect: {e}"))?;
    transport.upload_bundle(&bundle).await
        .map_err(|e| format!("upload_bundle: {e}"))?;
    let transport = Arc::new(transport);

    // Publish state and persist the hints so a later launch auto-reconnects.
    {
        let mut s = state.lock().map_err(|e| e.to_string())?;
        s.transport = Some(transport);
        s.username = Some(username);
        s.server_url = Some(server_url);
        s.save_connection_hints();
    }

    // Spawn incoming task.
    let app_bg = app.clone();
    tokio::spawn(async move {
        while let Some(inc) = incoming.recv().await {
            handle_incoming(&app_bg, inc).await;
        }
        tracing::info!("incoming stream ended");
    });

    let info = state.lock().map_err(|e| e.to_string())?.init_info();
    Ok(info)
}

// ---------- User directory: email registration + search --------------------

#[tauri::command]
pub async fn request_email_code(server_url: String, email: String) -> Result<Option<String>, String> {
    aqua_client_core::request_email_code(&server_url, &email)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn verify_email_code(server_url: String, email: String, code: String) -> Result<String, String> {
    aqua_client_core::verify_email_code(&server_url, &email, &code)
        .await
        .map_err(|e| e.to_string())
}

/// Register an account on the server over HTTP, BEFORE connecting the relay.
/// With mandatory registration the server refuses a bundle/WS for an
/// unregistered username, so this must run first; `connect_to_server` comes
/// after. Gated by the email `ticket`.
#[tauri::command]
pub async fn register_account(
    state: State<'_, StdMutex<AppState>>,
    server_url: String,
    username: String,
    ticket: String,
    display_name: String,
    about: String,
    phone: String,
    password: Option<String>,
    email: Option<String>,
) -> Result<(), String> {
    let identity = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.identity.clone()
    };
    // The relay never sees the password itself, only an Argon2id hash of it
    // (aqua_client_core::backup::auth_secret) — the backup key stays ours.
    let secret = match (password.as_deref(), email.as_deref()) {
        (Some(p), Some(e)) if !p.is_empty() && !e.trim().is_empty() => {
            if p.chars().count() < 8 { return Err("password must be at least 8 characters".into()); }
            let (p, e) = (p.to_string(), e.to_string());
            tokio::task::spawn_blocking(move || aqua_client_core::backup::auth_secret(&p, &e)).await.map_err(|e| e.to_string())?
        }
        (Some(p), _) => p.to_string(),
        _ => String::new(),
    };
    Transport::register_account(
        &server_url, &identity, &ticket, &username, &display_name, &about, &phone, &secret,
    )
    .await
    .map_err(|e| e.to_string())
}

/// Federated relays: `{ federated, self, servers }` from GET /v1/fed/peers.
#[tauri::command]
pub async fn fed_peers(server_url: String) -> Result<serde_json::Value, String> {
    let url = format!("{}/v1/fed/peers", server_url.trim_end_matches('/'));
    let http = reqwest::Client::builder().timeout(std::time::Duration::from_secs(6)).build().map_err(|e| e.to_string())?;
    let r = http.get(url).send().await.map_err(|e| e.to_string())?;
    if !r.status().is_success() {
        return Ok(serde_json::json!({ "federated": false, "servers": [] }));
    }
    r.json().await.map_err(|e| e.to_string())
}

/// Sign in to an existing account on this device (email ticket + password).
/// Returns the username; the UI then connects and completes registration
/// locally, exactly like after sign-up.
#[tauri::command]
pub async fn login_account(
    state: State<'_, StdMutex<AppState>>,
    server_url: String,
    ticket: String,
    password: String,
    email: Option<String>,
) -> Result<String, String> {
    let identity = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.identity.clone()
    };
    let Some(email) = email.filter(|e| !e.trim().is_empty()) else {
        return Transport::login_account(&server_url, &identity, &ticket, &password).await.map_err(|e| e.to_string());
    };
    let secret = {
        let (p, e) = (password.clone(), email.clone());
        tokio::task::spawn_blocking(move || aqua_client_core::backup::auth_secret(&p, &e)).await.map_err(|e| e.to_string())?
    };
    match Transport::login_account(&server_url, &identity, &ticket, &secret).await {
        Ok(u) => Ok(u),
        Err(e) if e.to_string().contains("wrong email or password") => {
            // An account from before hashed sign-in: try the raw password once,
            // then switch it to the hash so the relay stops seeing it.
            let u = Transport::login_account(&server_url, &identity, &ticket, &password).await.map_err(|e| e.to_string())?;
            if let Err(e) = Transport::set_password_with(&server_url, &identity, &u, &secret).await {
                tracing::warn!(error = %e, "could not upgrade the stored password");
            }
            Ok(u)
        }
        Err(e) => Err(e.to_string()),
    }
}

/// Register @username to this identity. Requires an already-open
/// transport (so we have an auth token) — call connect_to_server first,
/// then this. Gated by the email `ticket` from verify_email_code.
#[tauri::command]
pub async fn register_username(
    state: State<'_, StdMutex<AppState>>,
    ticket: String,
    username: String,
    display_name: String,
    about: String,
    phone: String,
) -> Result<(), String> {
    let transport = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.transport.clone().ok_or("not connected — connect first")?
    };
    transport.register_user(&ticket, &username, &display_name, &about, &phone)
        .await
        .map_err(|e| e.to_string())
}

#[derive(Debug, serde::Serialize)]
pub struct DirProfile {
    pub username: String,
    pub display_name: String,
    pub about: String,
    pub phone: String,
    pub is_bot: bool,
    /// Public extras: banner colours, status, pronouns, birthday, …
    pub extra: serde_json::Value,
    /// Server version of the avatar / banner image (0 = none).
    pub avatar_ver: i64,
    pub banner_ver: i64,
    pub music_ver: i64,
}

impl From<aqua_client_core::ProfileCard> for DirProfile {
    fn from(c: aqua_client_core::ProfileCard) -> Self {
        DirProfile {
            username: c.username,
            display_name: c.display_name,
            about: c.about,
            phone: c.phone,
            is_bot: c.is_bot,
            extra: if c.extra.is_object() { c.extra } else { serde_json::json!({}) },
            avatar_ver: c.avatar_ver,
            banner_ver: c.banner_ver,
            music_ver: c.music_ver,
        }
    }
}

#[tauri::command]
pub async fn search_directory(
    state: State<'_, StdMutex<AppState>>,
    query: String,
) -> Result<Vec<DirProfile>, String> {
    let transport = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.transport.clone()
    };
    let Some(transport) = transport else { return Ok(Vec::new()); };
    let cards = transport.search_users(&query).await.map_err(|e| e.to_string())?;
    Ok(cards.into_iter().map(DirProfile::from).collect())
}

#[tauri::command]
pub async fn fetch_directory_profile(
    state: State<'_, StdMutex<AppState>>,
    username: String,
) -> Result<Option<DirProfile>, String> {
    let transport = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.transport.clone()
    };
    let Some(transport) = transport else { return Ok(None); };
    let card = transport.get_profile(&username).await.map_err(|e| e.to_string())?;
    Ok(card.map(DirProfile::from))
}

#[tauri::command]
pub async fn publish_profile(
    state: State<'_, StdMutex<AppState>>,
    display_name: String,
    about: String,
    phone: String,
) -> Result<(), String> {
    let transport = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.transport.clone().ok_or("not connected")?
    };
    transport.update_profile(&display_name, &about, &phone)
        .await
        .map_err(|e| e.to_string())
}

/// Publish the full public profile card, including the `extra` object
/// (banner colours, status line, pronouns, birthday, favourite music…).
#[tauri::command]
pub async fn publish_profile_full(
    state: State<'_, StdMutex<AppState>>,
    display_name: String,
    about: String,
    phone: String,
    extra: serde_json::Value,
) -> Result<(), String> {
    let transport = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.transport.clone().ok_or("not connected")?
    };
    transport.update_profile_full(&display_name, &about, &phone, &extra)
        .await
        .map_err(|e| e.to_string())
}

// ---------- Profile images (public avatar / banner) -----------------------

fn check_media_kind(kind: &str) -> Result<(), String> {
    if matches!(kind, "avatar" | "banner" | "music") { Ok(()) } else { Err(format!("unknown profile media kind {kind:?}")) }
}

fn image_mime_for(path: &str) -> Option<&'static str> {
    let l = path.to_ascii_lowercase();
    if l.ends_with(".png") { Some("image/png") }
    else if l.ends_with(".jpg") || l.ends_with(".jpeg") { Some("image/jpeg") }
    else if l.ends_with(".webp") { Some("image/webp") }
    else if l.ends_with(".gif") { Some("image/gif") }
    else { None }
}

fn ext_for_mime(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "audio/mpeg" => "mp3",
        "audio/ogg" => "ogg",
        "audio/opus" => "opus",
        "audio/mp4" => "m4a",
        "audio/aac" => "aac",
        "audio/flac" => "flac",
        "audio/wav" => "wav",
        "audio/webm" => "weba",
        _ => "img",
    }
}

fn profile_cache_dir(data_dir: &std::path::Path) -> PathBuf {
    data_dir.join("profile-cache")
}

/// Cache file for `<user>`'s `<kind>` at server version `ver`. Older
/// versions of the same image are removed.
fn write_profile_cache(data_dir: &std::path::Path, user: &str, kind: &str, ver: i64, mime: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    let dir = profile_cache_dir(data_dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir profile cache: {e}"))?;
    let safe: String = user.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-')).collect();
    let prefix = format!("{safe}_{kind}_");
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().starts_with(&prefix) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let path = dir.join(format!("{prefix}{ver}.{}", ext_for_mime(mime)));
    std::fs::write(&path, bytes).map_err(|e| format!("write profile cache: {e}"))?;
    Ok(path)
}

#[derive(Debug, serde::Serialize)]
pub struct ProfileImageResult {
    pub ver: i64,
    /// Local cached copy (for the asset protocol).
    pub path: String,
}

async fn upload_profile_image(
    state: State<'_, StdMutex<AppState>>,
    kind: String,
    mime: String,
    bytes: Vec<u8>,
) -> Result<ProfileImageResult, String> {
    check_media_kind(&kind)?;
    let limit = if kind == "music" { 10 } else if matches!(mime.as_str(), "image/gif" | "video/mp4" | "video/webm") { 3 } else { 8 };
    if bytes.is_empty() || bytes.len() > limit * 1024 * 1024 {
        return Err(format!("file is empty or larger than {limit} MB"));
    }
    let (transport, data_dir) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        (s.transport.clone().ok_or("not connected — connect to a server to publish your profile")?, s.data_dir.clone())
    };
    let ver = transport.upload_profile_media(&kind, &mime, bytes.clone()).await.map_err(|e| e.to_string())?;
    let path = write_profile_cache(&data_dir, transport.username(), &kind, ver, &mime, &bytes)?;
    Ok(ProfileImageResult { ver, path: path.display().to_string() })
}

/// Upload a picked image file as our public avatar / banner.
#[tauri::command]
pub async fn set_profile_image(
    state: State<'_, StdMutex<AppState>>,
    app: AppHandle,
    kind: String,
    path: String,
) -> Result<ProfileImageResult, String> {
    if is_uri_pick(&path) {
        let bytes = read_picked_bytes(&app, &path)?;
        if bytes.len() > 40 * 1024 * 1024 {
            return Err("image too large (max 40 MB)".into());
        }
        let mime = infer::get(&bytes)
            .map(|k| k.mime_type())
            .filter(|m| matches!(*m, "image/png" | "image/jpeg" | "image/webp" | "image/gif"))
            .ok_or("pick a PNG, JPG, WEBP or GIF image")?;
        return upload_profile_image(state, kind, mime.to_string(), bytes).await;
    }
    let mime = image_mime_for(&path).ok_or("pick a PNG, JPG, WEBP or GIF image")?;
    let md = std::fs::metadata(&path).map_err(|e| format!("stat: {e}"))?;
    if md.len() > 8 * 1024 * 1024 {
        return Err("image too large (max 8 MB)".into());
    }
    let bytes = std::fs::read(&path).map_err(|e| format!("read: {e}"))?;
    upload_profile_image(state, kind, mime.to_string(), bytes).await
}

/// Upload a picked file as profile media: an animated banner (GIF or a
/// short video — re-encoded silent and small with ffmpeg when available)
/// or the "favourite track" audio file.
#[tauri::command]
pub async fn set_profile_media_file(
    state: State<'_, StdMutex<AppState>>,
    kind: String,
    path: String,
) -> Result<ProfileImageResult, String> {
    check_media_kind(&kind)?;
    let lower = path.to_ascii_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("").to_string();
    let audio_mime = match ext.as_str() {
        "mp3" => Some("audio/mpeg"), "ogg" | "oga" => Some("audio/ogg"), "opus" => Some("audio/opus"),
        "m4a" => Some("audio/mp4"), "aac" => Some("audio/aac"), "flac" => Some("audio/flac"),
        "wav" => Some("audio/wav"), "weba" => Some("audio/webm"), _ => None,
    };
    if kind == "music" {
        let mime = audio_mime.ok_or("pick an audio file: mp3, ogg, opus, m4a, aac, flac or wav")?;
        let bytes = tokio::fs::read(&path).await.map_err(|e| format!("read: {e}"))?;
        return upload_profile_image(state, kind, mime.into(), bytes).await;
    }
    if matches!(ext.as_str(), "mp4" | "webm" | "mov" | "mkv" | "m4v") {
        if kind != "banner" {
            return Err("videos can only be a banner".into());
        }
        // Silent, ≤ 8 s, 480 px wide, small: a banner must stay light.
        let out_dir = { state.lock().map_err(|e| e.to_string())?.data_dir.join("outbox") };
        std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
        let out = out_dir.join("banner.webm");
        let mut cmd = tokio::process::Command::new("ffmpeg");
        cmd.args(["-v", "error", "-nostdin", "-y", "-t", "8", "-i"]).arg(&path)
            .args(["-an", "-vf", "scale='min(640,iw)':-2", "-c:v", "libvpx-vp9", "-b:v", "0", "-crf", "42", "-deadline", "good", "-cpu-used", "4", "-row-mt", "1"])
            .arg(&out)
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        {
            cmd.creation_flags(0x0800_0000);
        }
        let (mime, bytes) = match cmd.output().await {
            Ok(o) if o.status.success() => ("video/webm", tokio::fs::read(&out).await.map_err(|e| e.to_string())?),
            // No ffmpeg: upload as is if it already fits.
            _ => (if ext == "webm" { "video/webm" } else { "video/mp4" }, tokio::fs::read(&path).await.map_err(|e| format!("read: {e}"))?),
        };
        return upload_profile_image(state, kind, mime.into(), bytes).await;
    }
    let mime = image_mime_for(&path).ok_or("pick a PNG, JPG, WEBP, GIF or a short video")?;
    let bytes = tokio::fs::read(&path).await.map_err(|e| format!("read: {e}"))?;
    upload_profile_image(state, kind, mime.into(), bytes).await
}

/// Upload already-encoded image bytes (e.g. a cropped avatar from the editor).
#[tauri::command]
pub async fn set_profile_image_bytes(
    state: State<'_, StdMutex<AppState>>,
    kind: String,
    mime: String,
    bytes: Vec<u8>,
) -> Result<ProfileImageResult, String> {
    if image_mime_for(&format!("x.{}", ext_for_mime(&mime))).is_none() {
        return Err(format!("unsupported image type {mime}"));
    }
    upload_profile_image(state, kind, mime, bytes).await
}

#[tauri::command]
pub async fn clear_profile_image(
    state: State<'_, StdMutex<AppState>>,
    kind: String,
) -> Result<(), String> {
    check_media_kind(&kind)?;
    let transport = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.transport.clone().ok_or("not connected")?
    };
    transport.clear_profile_media(&kind).await.map_err(|e| e.to_string())
}

/// Device-local switches: keep deleted messages / edit history, blocked people.
#[tauri::command]
pub fn set_local_prefs(
    state: State<'_, StdMutex<AppState>>,
    keep_deleted: bool,
    keep_edits: bool,
    blocked: Vec<String>,
) -> Result<(), String> {
    let mut s = state.lock().map_err(|e| e.to_string())?;
    s.prefs.keep_deleted = keep_deleted;
    s.prefs.keep_edits = keep_edits;
    s.prefs.blocked = blocked.into_iter().collect();
    Ok(())
}

/// Delete a message in a group as an admin (someone else's message).
#[tauri::command]
pub fn delete_group_message_of(
    state: State<'_, StdMutex<AppState>>,
    gid: String,
    author: String,
    text: String,
) -> Result<(), String> {
    let mut s = state.lock().map_err(|e| e.to_string())?;
    apply_delete(&mut s, &gid, &author, &text);
    Ok(())
}

/// Profile-photo privacy: who may download our avatar / banner.
#[tauri::command]
pub async fn set_photo_privacy(
    state: State<'_, StdMutex<AppState>>,
    everybody: bool,
    allow: Vec<String>,
    deny: Vec<String>,
) -> Result<(), String> {
    let transport = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.transport.clone().ok_or("not connected")?
    };
    transport.set_media_acl(everybody, &allow, &deny).await.map_err(|e| e.to_string())
}

/// Local path of `username`'s avatar / banner at version `ver`, downloading
/// it from the server into the profile cache the first time. `None` when
/// the user has no such image (or we're offline and it isn't cached).
#[tauri::command]
pub async fn profile_media_path(
    state: State<'_, StdMutex<AppState>>,
    username: String,
    kind: String,
    ver: i64,
) -> Result<Option<String>, String> {
    check_media_kind(&kind)?;
    if ver <= 0 {
        return Ok(None);
    }
    let (transport, data_dir) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        (s.transport.clone(), s.data_dir.clone())
    };
    let dir = profile_cache_dir(&data_dir);
    let safe: String = username.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-')).collect();
    let prefix = format!("{safe}_{kind}_{ver}.");
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().starts_with(&prefix) {
                return Ok(Some(e.path().display().to_string()));
            }
        }
    }
    let Some(transport) = transport else { return Ok(None) };
    match transport.fetch_profile_media(&username, &kind, ver).await.map_err(|e| e.to_string())? {
        Some((mime, bytes)) => {
            let path = write_profile_cache(&data_dir, &username, &kind, ver, &mime, &bytes)?;
            Ok(Some(path.display().to_string()))
        }
        None => Ok(None),
    }
}

// ---------- Media library: add / remove ------------------------------------

/// Copy files into the sticker / emoji / GIF library (drag-and-drop onto the
/// picker, "Add to my GIFs" on a received message, …). Returns how many
/// files were added.
#[tauri::command]
pub fn add_media_files(
    state: State<'_, StdMutex<AppState>>,
    target: String,
    pack: Option<String>,
    paths: Vec<String>,
) -> Result<usize, String> {
    let data_dir = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.data_dir.clone()
    };
    media::add_files(&data_dir, &target, pack.as_deref(), &paths)
}

/// Delete one item from the library (only inside the stickers / gifs dirs).
#[tauri::command]
pub fn delete_media_item(
    state: State<'_, StdMutex<AppState>>,
    path: String,
) -> Result<(), String> {
    let data_dir = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.data_dir.clone()
    };
    let p = PathBuf::from(&path);
    let inside = media::path_is_within(&data_dir.join("stickers"), &p) || media::path_is_within(&data_dir.join("gifs"), &p);
    if !inside {
        return Err("not a library item".into());
    }
    std::fs::remove_file(&p).map_err(|e| format!("delete: {e}"))
}

// ---------- Activity (Discord-style rich presence) -------------------------

/// Whether the background detector runs. Toggled from Settings.
pub static ACTIVITY_ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

#[tauri::command]
pub fn set_activity_detection(enabled: bool) {
    ACTIVITY_ENABLED.store(enabled, std::sync::atomic::Ordering::Relaxed);
}

/// What the user is doing right now (empty off Windows or when idle).
#[tauri::command]
pub async fn current_activity() -> Result<Vec<crate::activity::Activity>, String> {
    Ok(crate::activity::detect().await)
}

/// Background loop: re-detect every 15 s and emit `aqua://activity` when
/// the result changes.
pub fn spawn_activity_watcher(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut last: Option<Vec<crate::activity::Activity>> = None;
        loop {
            if ACTIVITY_ENABLED.load(std::sync::atomic::Ordering::Relaxed) {
                let now = crate::activity::detect().await;
                if last.as_ref() != Some(&now) {
                    let _ = app.emit("aqua://activity", &now);
                    last = Some(now);
                }
            } else if last.as_ref().is_some_and(|l| !l.is_empty()) {
                let _ = app.emit("aqua://activity", Vec::<crate::activity::Activity>::new());
                last = Some(Vec::new());
            }
            tokio::time::sleep(std::time::Duration::from_secs(8)).await;
        }
    });
}

// ---------- Bots ----------------------------------------------------------

/// Start a plaintext chat with a bot (no X3DH). Creates a local bot
/// chat (has_lock=false) and sends the first message via the relay.
#[tauri::command]
pub async fn start_bot_chat(
    state: State<'_, StdMutex<AppState>>,
    app: AppHandle,
    bot_username: String,
    first_message: String,
) -> Result<ChatSummary, String> {
    let bot = bot_username.trim().to_lowercase();
    let transport = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.transport.clone().ok_or("not connected")?
    };
    let summary = {
        let mut s = state.lock().map_err(|e| e.to_string())?;
        let (letter, gradient) = gradient_for(&bot);
        let mut summary = summary_for(&bot, &first_message, true, false);
        summary.letter = letter;
        summary.gradient = gradient;
        summary.has_lock = false; // bots are NOT end-to-end encrypted
        summary.online = true;
        let book = ChatBook {
            summary: summary.clone(),
            messages: vec![text_msg_from_me(&first_message)],
        };
        s.chats.insert(bot.clone(), book);
        s.chat_order.retain(|id| id != &bot);
        s.chat_order.insert(0, bot.clone());
        summary
    };
    transport.send_plaintext(&bot, &first_message).map_err(|e| format!("send: {e}"))?;
    let _ = app.emit("aqua://chats_updated", ());
    Ok(summary)
}

#[derive(Debug, serde::Serialize)]
pub struct BotInfo {
    pub username: String,
    /// Empty when this device never saw it (made elsewhere): reset to get one.
    pub token: String,
    pub display_name: String,
    /// Last 4 characters of the token, from the relay.
    pub hint: String,
}

/// Bot tokens live in the encrypted vault (and so in backups): the relay
/// stores only their hashes and can't show them again.
fn bot_token_key(username: &str) -> String { format!("bot_token:{username}") }

/// Register a bot owned by this identity; returns {username, token}.
#[tauri::command]
pub async fn create_bot(
    state: State<'_, StdMutex<AppState>>,
    username: String,
    display_name: String,
) -> Result<BotInfo, String> {
    let transport = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.transport.clone().ok_or("not connected")?
    };
    let display_name = display_name.trim().to_string();
    let (username, token) = transport.register_bot(&username, &display_name)
        .await.map_err(|e| e.to_string())?;
    {
        let s = state.lock().map_err(|e| e.to_string())?;
        let _ = s.vault.put_record(&bot_token_key(&username), token.as_bytes());
    }
    let hint = token.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
    Ok(BotInfo { username, token, display_name, hint })
}

/// List bots owned by this identity (with tokens).
#[tauri::command]
pub async fn list_bots(state: State<'_, StdMutex<AppState>>) -> Result<Vec<BotInfo>, String> {
    let transport = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.transport.clone().ok_or("not connected")?
    };
    let rows = transport.list_bots().await.map_err(|e| e.to_string())?;
    let s = state.lock().map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .map(|(username, token, display_name, hint)| {
            let token = if !token.is_empty() {
                token // an old relay that still returns it
            } else {
                s.vault.get_record(&bot_token_key(&username)).ok().flatten()
                    .map(|b| String::from_utf8_lossy(&b).to_string())
                    .filter(|t| hint.is_empty() || t.ends_with(&hint))
                    .unwrap_or_default()
            };
            BotInfo { username, token, display_name, hint }
        })
        .collect())
}

/// Delete one of my bots on the relay.
#[tauri::command]
pub async fn delete_bot(state: State<'_, StdMutex<AppState>>, username: String) -> Result<(), String> {
    let transport = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.transport.clone().ok_or("not connected")?
    };
    transport.delete_bot(&username).await.map_err(|e| e.to_string())?;
    let s = state.lock().map_err(|e| e.to_string())?;
    let _ = s.vault.delete_record(&bot_token_key(&username.trim().to_lowercase()));
    Ok(())
}

// ---------- File transfer -------------------------------------------------

#[derive(Debug, serde::Serialize)]
pub struct AttachResult {
    pub message_id: String,
    pub cid: String,
    pub size: u64,
}

#[tauri::command]
pub async fn attach_file(
    state: State<'_, StdMutex<AppState>>,
    app: AppHandle,
    peer_id: String,
    file_path: String,
) -> Result<AttachResult, String> {
    attach_file_impl(state, app, peer_id, file_path, None, None).await
}

/// Send a file the way the user chose in the send dialog:
/// - `"file"`: as a document (original bytes, no inline preview anywhere)
/// - `"media"`: as photo / video (inline)
/// - `"compressed"`: a video re-encoded with ffmpeg to ≤ 720p H.264/AAC
///   (much smaller; fails with a clear error when ffmpeg is missing).
#[tauri::command]
pub async fn attach_file_as(
    state: State<'_, StdMutex<AppState>>,
    app: AppHandle,
    peer_id: String,
    file_path: String,
    mode: String,
) -> Result<AttachResult, String> {
    match mode.as_str() {
        "file" => attach_file_impl(state, app, peer_id, file_path, Some("document".into()), None).await,
        "compressed" => {
            let out_dir = { state.lock().map_err(|e| e.to_string())?.data_dir.join("outbox") };
            std::fs::create_dir_all(&out_dir).map_err(|e| format!("mkdir outbox: {e}"))?;
            let stem = PathBuf::from(&file_path).file_stem().and_then(|s| s.to_str()).unwrap_or("video").to_string();
            let out = out_dir.join(format!("{}.mp4", stem.replace(['/', '\\', ':'], "_")));
            let mut cmd = tokio::process::Command::new("ffmpeg");
            cmd.args(["-v", "error", "-nostdin", "-y", "-i"]).arg(&file_path)
                .args(["-vf", "scale='min(1280,iw)':'min(720,ih)':force_original_aspect_ratio=decrease:force_divisible_by=2",
                       "-c:v", "libx264", "-preset", "veryfast", "-crf", "27", "-pix_fmt", "yuv420p",
                       "-c:a", "aac", "-b:a", "128k", "-movflags", "+faststart"])
                .arg(&out)
                .stdin(std::process::Stdio::null())
                .kill_on_drop(true);
            #[cfg(windows)]
            {
                cmd.creation_flags(0x0800_0000);
            }
            let res = cmd.output().await.map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound { "ffmpeg is not installed — sent nothing".to_string() } else { format!("ffmpeg: {e}") }
            })?;
            if !res.status.success() {
                return Err(format!("ffmpeg failed: {}", String::from_utf8_lossy(&res.stderr).lines().last().unwrap_or("")));
            }
            attach_file_impl(state, app, peer_id, out.display().to_string(), None, Some("video/mp4".into())).await
        }
        _ => attach_file_impl(state, app, peer_id, file_path, None, None).await,
    }
}

/// Send bytes made in the UI (a compressed photo) as a file named `name`.
#[tauri::command]
pub async fn attach_bytes(
    state: State<'_, StdMutex<AppState>>,
    app: AppHandle,
    peer_id: String,
    name: String,
    mime: String,
    bytes: Vec<u8>,
) -> Result<AttachResult, String> {
    let out_dir = { state.lock().map_err(|e| e.to_string())?.data_dir.join("outbox") };
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("mkdir outbox: {e}"))?;
    let out = out_dir.join(name.replace(['/', '\\', ':'], "_"));
    tokio::fs::write(&out, &bytes).await.map_err(|e| format!("write: {e}"))?;
    attach_file_impl(state, app, peer_id, out.display().to_string(), None, Some(mime)).await
}

#[tauri::command]
pub async fn send_sticker(
    state: State<'_, StdMutex<AppState>>,
    app: AppHandle,
    peer_id: String,
    file_path: String,
) -> Result<AttachResult, String> {
    // Only allow sources from inside the local stickers dir.
    {
        let s = state.lock().map_err(|e| e.to_string())?;
        let allowed = s.data_dir.join("stickers");
        if !media::path_is_within(&allowed, &PathBuf::from(&file_path)) {
            return Err(format!(
                "sticker path must be under {}",
                allowed.display()
            ));
        }
    }
    attach_file_impl(state, app, peer_id, file_path, Some("sticker".into()), None).await
}

/// Receive raw audio bytes from the frontend (recorded via MediaRecorder),
/// write them to `<data_dir>/voice/<name>`, then push through the same
/// attach pipeline as a sticker, marked `category = "voice"` so the
/// receiver renders an audio bubble labelled "Voice message".
#[tauri::command]
pub async fn send_voice_message(
    state: State<'_, StdMutex<AppState>>,
    app: AppHandle,
    peer_id: String,
    name: String,
    mime: String,
    bytes: Vec<u8>,
) -> Result<AttachResult, String> {
    let voice_dir = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.data_dir.join("voice")
    };
    std::fs::create_dir_all(&voice_dir).map_err(|e| format!("mkdir voice: {e}"))?;
    // Sanitise the filename a bit — strip any path separators so we
    // can't be tricked into writing outside voice_dir.
    let safe_name = name.replace(['/', '\\', ':'], "_");
    let out_path = voice_dir.join(&safe_name);
    std::fs::write(&out_path, &bytes).map_err(|e| format!("write voice: {e}"))?;
    attach_file_impl(
        state,
        app,
        peer_id,
        out_path.display().to_string(),
        Some("voice".into()),
        Some(mime),
    ).await
}

/// Save a user-created sticker PNG into the local stickers library at
/// `<data_dir>/stickers/<pack>/<name>.png`. `pack` and `name` are
/// sanitised; the pack folder is created if missing. Returns the full
/// path that was written, so the frontend can refresh the picker.
#[tauri::command]
pub fn save_sticker(
    state: State<'_, StdMutex<AppState>>,
    pack: String,
    name: String,
    bytes: Vec<u8>,
) -> Result<String, String> {
    if bytes.is_empty() { return Err("empty sticker payload".into()); }
    if bytes.len() > 4 * 1024 * 1024 { return Err("sticker too large (max 4 MB)".into()); }
    let safe = |s: &str| s.chars()
        .filter(|c| !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        .collect::<String>()
        .trim()
        .trim_matches('.')
        .to_string();
    let pack_safe = safe(&pack);
    let name_safe = safe(&name);
    if pack_safe.is_empty() { return Err("pack name required".into()); }
    if name_safe.is_empty() { return Err("sticker name required".into()); }
    let data_dir = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.data_dir.clone()
    };
    let pack_dir = data_dir.join("stickers").join(&pack_safe);
    std::fs::create_dir_all(&pack_dir).map_err(|e| format!("mkdir pack: {e}"))?;
    let lower = name_safe.to_ascii_lowercase();
    let final_name = if lower.ends_with(".png") || lower.ends_with(".webp") {
        name_safe
    } else {
        format!("{name_safe}.png")
    };
    // Never overwrite: "😀_sticker.png" → "😀_sticker-2.png".
    let mut path = pack_dir.join(&final_name);
    let (stem, ext) = final_name.rsplit_once('.').map(|(a, b)| (a.to_string(), b.to_string())).unwrap_or((final_name.clone(), "png".into()));
    let mut n = 2;
    while path.exists() {
        path = pack_dir.join(format!("{stem}-{n}.{ext}"));
        n += 1;
    }
    std::fs::write(&path, &bytes).map_err(|e| format!("write sticker: {e}"))?;
    Ok(path.display().to_string())
}

fn own_pack_dir(state: &State<'_, StdMutex<AppState>>, title: &str) -> Result<PathBuf, String> {
    let data_dir = state.lock().map_err(|e| e.to_string())?.data_dir.clone();
    let root = data_dir.join("stickers");
    let t = title.trim();
    if t.is_empty() || t.contains(['/', '\\']) || t.starts_with('.') {
        return Err("bad pack name".into());
    }
    let dir = root.join(t);
    if !dir.is_dir() || !media::path_is_within(&root, &dir) {
        return Err("no such pack".into());
    }
    Ok(dir)
}

/// Delete a whole sticker pack folder from the local library.
#[tauri::command]
pub fn delete_sticker_pack(state: State<'_, StdMutex<AppState>>, title: String) -> Result<(), String> {
    let dir = own_pack_dir(&state, &title)?;
    std::fs::remove_dir_all(&dir).map_err(|e| format!("delete pack: {e}"))
}

/// Rename a sticker pack (its folder).
#[tauri::command]
pub fn rename_sticker_pack(state: State<'_, StdMutex<AppState>>, title: String, new_title: String) -> Result<(), String> {
    let dir = own_pack_dir(&state, &title)?;
    let n = new_title.trim();
    if n.is_empty() || n.len() > 64 || n.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']) || n.starts_with('.') {
        return Err("bad pack name".into());
    }
    let dest = dir.parent().ok_or("bad pack")?.join(n);
    if dest.exists() {
        return Err("a pack with this name already exists".into());
    }
    std::fs::rename(&dir, &dest).map_err(|e| format!("rename pack: {e}"))
}

/// Import a Telegram sticker pack into the local stickers library.
///
/// Ported from the old PipoHuy messenger. `pack_ref` can be a
/// `t.me/addstickers/<name>` link, a bare `@name`, or the short name;
/// `bot_token` is any Telegram bot token (from @BotFather). The pack is
/// fetched straight from the Telegram Bot API and written as a folder of
/// image files under `<data_dir>/stickers/<title>/` — nothing touches our
/// own relay. Returns a report the UI can show (counts, folder).
#[tauri::command]
pub async fn import_telegram_stickers(
    state: State<'_, StdMutex<AppState>>,
    bot_token: String,
    pack_ref: String,
) -> Result<aqua_client_core::ImportReport, String> {
    let stickers_root = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.data_dir.join("stickers")
    };
    std::fs::create_dir_all(&stickers_root).map_err(|e| format!("mkdir stickers: {e}"))?;
    aqua_client_core::import_telegram_pack(&bot_token, &pack_ref, &stickers_root).await
}

/// What the relay offers for sticker packs: `{ telegram, packs: [...] }`.
#[tauri::command]
pub async fn server_packs(state: State<'_, StdMutex<AppState>>) -> Result<serde_json::Value, String> {
    let t = { state.lock().map_err(|e| e.to_string())?.transport.clone().ok_or("not connected")? };
    t.packs_index().await.map_err(|e| e.to_string())
}

/// Import a pack through the relay (its own Telegram bot token): the relay
/// downloads it once; we copy it into the local sticker library.
#[tauri::command]
pub async fn import_pack_via_server(
    state: State<'_, StdMutex<AppState>>,
    link: String,
) -> Result<aqua_client_core::ImportReport, String> {
    let (t, root) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        (s.transport.clone().ok_or("not connected")?, s.data_dir.join("stickers"))
    };
    let m = t.import_pack(&link).await.map_err(|e| match e {
        aqua_client_core::TransportError::Server { body, .. } if !body.is_empty() => body,
        e => e.to_string(),
    })?;
    save_server_pack(&t, &root, &m).await
}

async fn save_server_pack(
    t: &aqua_client_core::Transport,
    root: &std::path::Path,
    m: &serde_json::Value,
) -> Result<aqua_client_core::ImportReport, String> {
    let name = m["name"].as_str().unwrap_or_default().to_string();
    let title = m["title"].as_str().unwrap_or(&name).to_string();
    let dest = root.join(aqua_client_core::sanitize_pack_folder(&title));
    std::fs::create_dir_all(&dest).map_err(|e| format!("mkdir pack: {e}"))?;
    let files = m["files"].as_array().cloned().unwrap_or_default();
    // Several files at a time (the relay may be far away).
    let wanted: Vec<String> = files
        .iter()
        .filter_map(|f| f["file"].as_str())
        .filter(|f| !f.contains(['/', '\\']) && !f.starts_with('.'))
        .map(str::to_string)
        .collect();
    let mut done: Vec<String> = Vec::new();
    for chunk in wanted.chunks(8) {
        let got = futures_util::future::join_all(chunk.iter().map(|file| {
            let out = dest.join(file);
            let name = name.clone();
            async move {
                if out.exists() {
                    return Some(file.clone());
                }
                let bytes = t.pack_file(&name, file).await.ok()?;
                std::fs::write(&out, bytes).ok()?;
                Some(file.clone())
            }
        }))
        .await;
        done.extend(got.into_iter().flatten());
    }
    let ok = done.len();
    let animated = done.iter().filter(|f| f.ends_with(".tgs")).count();
    let video = done.iter().filter(|f| f.ends_with(".webm")).count();
    let meta = serde_json::json!({ "name": name, "title": title, "type": m["type"].as_str().unwrap_or("regular") });
    let _ = std::fs::write(dest.join("pack.json"), meta.to_string());
    Ok(aqua_client_core::ImportReport {
        pack_name: name,
        pack_title: title,
        dir: dest.display().to_string(),
        imported: ok,
        renderable: ok - animated - video,
        animated,
        video,
        total: files.len(),
    })
}

/// Local path of one file of a relay-hosted pack (a custom-emoji reaction
/// from a pack this device doesn't have). Downloaded once, then cached.
#[tauri::command]
pub async fn pack_file_path(
    state: State<'_, StdMutex<AppState>>,
    name: String,
    file: String,
) -> Result<String, String> {
    let ok_part = |x: &str| !x.is_empty() && !x.contains(['/', '\\']) && !x.starts_with('.');
    if !ok_part(&name) || !ok_part(&file) {
        return Err("bad pack file".into());
    }
    let (t, dir) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        (s.transport.clone(), s.data_dir.join("packcache").join(&name))
    };
    let path = dir.join(&file);
    if path.exists() {
        return Ok(path.display().to_string());
    }
    let t = t.ok_or("not connected")?;
    let bytes = t.pack_file(&name, &file).await.map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

/// Send a "circle" video message — a short self-cam clip recorded in
/// the browser. Writes to `<data_dir>/circles/<name>` and attaches
/// with `category = "circle"` so the receiver renders it as a round
/// video.
#[tauri::command]
pub async fn send_circle_video(
    state: State<'_, StdMutex<AppState>>,
    app: AppHandle,
    peer_id: String,
    name: String,
    mime: String,
    bytes: Vec<u8>,
) -> Result<AttachResult, String> {
    let dir = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.data_dir.join("circles")
    };
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir circles: {e}"))?;
    let safe_name = name.replace(['/', '\\', ':'], "_");
    let out_path = dir.join(&safe_name);
    std::fs::write(&out_path, &bytes).map_err(|e| format!("write circle: {e}"))?;
    attach_file_impl(
        state,
        app,
        peer_id,
        out_path.display().to_string(),
        Some("circle".into()),
        Some(mime),
    ).await
}

/// Accept bytes of a user-edited image from the frontend (produced by
/// the in-app photo editor canvas) and send it through the normal
/// attach pipeline as an image message.
#[tauri::command]
pub async fn send_edited_image(
    state: State<'_, StdMutex<AppState>>,
    app: AppHandle,
    peer_id: String,
    name: String,
    mime: String,
    bytes: Vec<u8>,
) -> Result<AttachResult, String> {
    let img_dir = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.data_dir.join("images")
    };
    std::fs::create_dir_all(&img_dir).map_err(|e| format!("mkdir images: {e}"))?;
    let safe_name = name.replace(['/', '\\', ':'], "_");
    let out_path = img_dir.join(&safe_name);
    std::fs::write(&out_path, &bytes).map_err(|e| format!("write image: {e}"))?;
    attach_file_impl(
        state,
        app,
        peer_id,
        out_path.display().to_string(),
        Some("image".into()),
        Some(mime),
    ).await
}

#[tauri::command]
pub async fn send_gif(
    state: State<'_, StdMutex<AppState>>,
    app: AppHandle,
    peer_id: String,
    file_path: String,
) -> Result<AttachResult, String> {
    {
        let s = state.lock().map_err(|e| e.to_string())?;
        let allowed = s.data_dir.join("gifs");
        if !media::path_is_within(&allowed, &PathBuf::from(&file_path)) {
            return Err(format!("gif path must be under {}", allowed.display()));
        }
    }
    attach_file_impl(state, app, peer_id, file_path, Some("gif".into()), None).await
}

// ---------- Picked files on phones -----------------------------------------
//
// On Android the file picker hands back `content://…` URIs, not paths, so
// std::fs can't open them. Read those through the fs plugin (which resolves
// them with Android's ContentResolver) and, where the rest of the pipeline
// wants a real file, copy the bytes into <data_dir>/picked/ under a name
// whose extension matches the sniffed content type.

fn is_uri_pick(path: &str) -> bool {
    path.starts_with("content://") || path.starts_with("file://")
}

/// Bytes of a user-picked file: plain paths via std::fs, URIs via the fs plugin.
pub(crate) fn read_picked_bytes(app: &AppHandle, path: &str) -> Result<Vec<u8>, String> {
    if !is_uri_pick(path) {
        return std::fs::read(path).map_err(|e| format!("read {path}: {e}"));
    }
    use tauri_plugin_fs::FsExt;
    let url = tauri::Url::parse(path).map_err(|e| format!("bad file URI: {e}"))?;
    if url.scheme() == "file" {
        if let Ok(p) = url.to_file_path() {
            return std::fs::read(&p).map_err(|e| format!("read {}: {e}", p.display()));
        }
    }
    app.fs()
        .read(tauri_plugin_fs::FilePath::Url(url))
        .map_err(|e| format!("read picked file: {e}"))
}

/// A real local path for a picked file (copies URI picks into the app dir).
fn materialize_picked(app: &AppHandle, data_dir: &std::path::Path, path: &str) -> Result<String, String> {
    if !is_uri_pick(path) {
        return Ok(path.to_string());
    }
    let bytes = read_picked_bytes(app, path)?;
    let kind = infer::get(&bytes);
    let ext = kind.map(|k| k.extension()).unwrap_or("bin");
    let stem = match kind.map(|k| k.matcher_type()) {
        Some(infer::MatcherType::Video) => "video",
        Some(infer::MatcherType::Image) => "photo",
        Some(infer::MatcherType::Audio) => "audio",
        _ => "file",
    };
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let dir = data_dir.join("picked");
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {e}"))?;
    let dest = pick_unique_path(&dir, &format!("{stem}-{secs}.{ext}"));
    std::fs::write(&dest, &bytes).map_err(|e| format!("write: {e}"))?;
    Ok(dest.display().to_string())
}


/// Shared implementation for attach_file / send_sticker / send_gif.
/// `category` becomes the inline-render hint on both ends.
async fn attach_file_impl(
    state: State<'_, StdMutex<AppState>>,
    app: AppHandle,
    peer_id: String,
    file_path: String,
    category: Option<String>,
    mime_override: Option<String>,
) -> Result<AttachResult, String> {
    // 1. Read the file from disk (phone picks arrive as content:// URIs).
    let file_path = if is_uri_pick(&file_path) {
        let data_dir = { state.lock().map_err(|e| e.to_string())?.data_dir.clone() };
        materialize_picked(&app, &data_dir, &file_path)?
    } else {
        file_path
    };
    let path = PathBuf::from(&file_path);
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    if size > aqua_core::MAX_FILE_SIZE {
        return Err(format!(
            "file is too large: {} MB (limit {} MB)",
            size / (1024 * 1024),
            aqua_core::MAX_FILE_SIZE / (1024 * 1024)
        ));
    }
    let bytes = std::fs::read(&path).map_err(|e| format!("read {file_path}: {e}"))?;
    let filename = path.file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("attachment")
        .to_string();
    let mime = mime_override.or_else(|| mime_from_extension(&path));

    // Saved Messages — local-only. Build a File message DIRECTLY (not
    // by round-tripping through the file-offer text format, which needs
    // a real key/nonce and would otherwise fall through to a raw text
    // bubble). `saved_at` points at the source file so the UI renders
    // the image/audio/video inline immediately. No encryption, no
    // server round-trip.
    if peer_id == crate::state::SAVED_MESSAGES_ID {
        let fake_cid = format!("local-{}", crate::state::uuid_like());
        let size = bytes.len() as u64;
        // Copy the attachment into <data_dir>/saved-media/ so it (a)
        // persists even if the user moves/deletes the original, and
        // (b) is always inside read_media_bytes' allowed scope so the
        // inline player/preview can load it. Voice notes / circles /
        // edited images already live under data_dir, but a file picked
        // from an arbitrary folder (e.g. Music) would otherwise be
        // outside scope.
        let saved_path = {
            let data_dir = { state.lock().map_err(|e| e.to_string())?.data_dir.clone() };
            let media_dir = data_dir.join("saved-media");
            std::fs::create_dir_all(&media_dir).ok();
            if crate::media::path_is_within(&media_dir, &path) {
                // Already in our media dir (voice/circle/edited) — reuse.
                file_path.clone()
            } else {
                let dest = pick_unique_path(&media_dir, &filename);
                match std::fs::copy(&path, &dest) {
                    Ok(_) => dest.display().to_string(),
                    Err(_) => file_path.clone(), // fall back to original path
                }
            }
        };
        let msg = ChatMessage { meta: None,
            id: format!("m-{}", crate::state::uuid_like()),
            from: "me".into(),
            time: hhmm_now(),
            kind: MessageKind::File {
                cid: fake_cid.clone(),
                key_b64: String::new(),
                nonce_b64: String::new(),
                name: filename.clone(),
                size,
                mime: mime.clone(),
                category: category.clone(),
                saved_at: Some(saved_path),
            },
        };
        let message_id = msg.id.clone();
        {
            let mut s = state.lock().map_err(|e| e.to_string())?;
            let preview = crate::state::one_line_preview(&msg.kind);
            push_own_message(&mut s, &peer_id, msg, &preview);
            let data_dir = s.data_dir.clone();
            if let Some(book) = s.chats.get(&peer_id) {
                crate::state::save_saved_messages(&data_dir, &book.messages);
            }
        }
        let _ = app.emit("aqua://chats_updated", ());
        return Ok(AttachResult { message_id, cid: fake_cid, size });
    }

    // 2. Encrypt + get CID + envelope.
    let (envelope, ciphertext) =
        encrypt_file_with_category(&bytes, &filename, mime, category.clone())
            .map_err(|e| format!("encrypt_file: {e}"))?;
    let cid = envelope.cid.clone();
    let size = envelope.size;

    // 3. Upload the ciphertext blob.
    let transport = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.transport.clone().ok_or("not connected")?
    };
    transport.upload_blob(&cid, ciphertext).await
        .map_err(|e| format!("upload_blob: {e}"))?;

    // 4. Send the offer as a regular E2EE text message through the
    //    peer's session.
    let offer_text = envelope.to_message_text();

    // Groups / channels: the blob is uploaded once; the offer (with the
    // file key) fans out to every member over their own E2EE sessions.
    if peer_id.starts_with("grp_") {
        let r = group_send(app.clone(), state.clone(), peer_id.clone(), offer_text.clone()).await?;
        {
            let mut s = state.lock().map_err(|e| e.to_string())?;
            if let Some(book) = s.chats.get_mut(&peer_id) {
                if let Some(m) = book.messages.iter_mut().find(|m| m.id == r.id) {
                    if let MessageKind::File { saved_at, .. } = &mut m.kind {
                        *saved_at = Some(file_path.clone());
                    }
                }
            }
        }
        let _ = app.emit("aqua://chats_updated", ());
        return Ok(AttachResult { message_id: r.id, cid, size });
    }

    let is_live = state.lock().map_err(|e| e.to_string())?
        .chats.get(&peer_id).map(|c| c.summary.live).unwrap_or(false);
    if !is_live {
        return Err("no live session with peer".into());
    }
    let env = {
        let mut s = state.lock().map_err(|e| e.to_string())?;
        let env = {
            let session = s.sessions.get_mut(&peer_id).ok_or("no session for peer")?;
            session.encrypt(offer_text.as_bytes())
                .map_err(|e| format!("encrypt: {e}"))?
        };
        if let Some(session) = s.sessions.get(&peer_id) {
            let _ = s.vault.save_session(&peer_id, session);
        }
        env
    };
    transport.send_envelope(&peer_id, &env)
        .map_err(|e| format!("send: {e}"))?;

    // 5. Reflect it locally as a File message with saved_at pointing
    //    at the sender's source file (so the UI can render inline
    //    without re-fetching through the blob store).
    let mut msg = text_msg_from_me(&offer_text);
    if let MessageKind::File { saved_at, .. } = &mut msg.kind {
        *saved_at = Some(file_path.clone());
    }
    let message_id = msg.id.clone();
    {
        let mut s = state.lock().map_err(|e| e.to_string())?;
        push_own_message(&mut s, &peer_id, msg, &offer_text);
    }
    let _ = app.emit("aqua://chats_updated", ());
    Ok(AttachResult { message_id, cid, size })
}

// ---------- Local media library ------------------------------------------

#[tauri::command]
pub fn list_local_media(
    state: State<'_, StdMutex<AppState>>,
) -> Result<media::LocalMediaList, String> {
    let dir = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.data_dir.clone()
    };
    Ok(media::scan(&dir))
}

/// Copy a stored attachment to a user-chosen destination (the "Save
/// as…" action on a file card). Source must be a real file; we don't
/// restrict the destination since the user picked it via the native
/// save dialog.
#[tauri::command]
pub fn copy_file_to(source: String, dest: String) -> Result<(), String> {
    std::fs::copy(&source, &dest).map_err(|e| format!("copy: {e}"))?;
    Ok(())
}

/// Read an image/video file the user picked from anywhere on their
/// disk, for the frontend to use as a wallpaper or inside the photo
/// editor. Capped at 8 MB and only allows media extensions so a
/// hostile frontend can't exfiltrate arbitrary files via this
/// command.
#[tauri::command]
pub fn read_user_image(app: AppHandle, path: String) -> Result<Vec<u8>, String> {
    if is_uri_pick(&path) {
        let bytes = read_picked_bytes(&app, &path)?;
        if bytes.len() > 40 * 1024 * 1024 {
            return Err("image too large (max 40 MB)".into());
        }
        if !infer::is_image(&bytes) {
            return Err("not an image file".into());
        }
        return Ok(bytes);
    }
    let p = PathBuf::from(&path);
    let ext = p.extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    if !matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp") {
        return Err(format!("not an image file ({ext})"));
    }
    let md = std::fs::metadata(&p).map_err(|e| format!("stat: {e}"))?;
    if md.len() > 40 * 1024 * 1024 {
        return Err("image too large (max 40 MB)".into());
    }
    std::fs::read(&p).map_err(|e| format!("read: {e}"))
}

/// Read a user-chosen font file (for the theme editor's custom UI font).
/// Only font extensions, capped at 12 MB.
#[tauri::command]
pub fn read_user_font(path: String) -> Result<Vec<u8>, String> {
    let p = PathBuf::from(&path);
    let ext = p.extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    if !matches!(ext.as_str(), "ttf" | "otf" | "woff" | "woff2") {
        return Err(format!("not a font file ({ext})"));
    }
    let md = std::fs::metadata(&p).map_err(|e| format!("stat: {e}"))?;
    if md.len() > 12 * 1024 * 1024 {
        return Err("font too large (max 12 MB)".into());
    }
    std::fs::read(&p).map_err(|e| format!("read: {e}"))
}

/// Read raw bytes from a file for the frontend to render inline.
/// Scoped: path must be inside the data_dir (stickers/gifs/downloads
/// cache) OR the OS Downloads/Aquamarine folder we save received
/// attachments into.
#[tauri::command]
pub fn read_media_bytes(
    state: State<'_, StdMutex<AppState>>,
    path: String,
) -> Result<Vec<u8>, String> {
    let data_dir = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.data_dir.clone()
    };
    let downloads = downloads_dir(&data_dir);
    let candidate = PathBuf::from(&path);
    // Preferred: files inside our own dirs (stickers/gifs/voice/circles/
    // images/saved-media/downloads cache). Those are always allowed.
    let in_scope = media::path_is_within(&data_dir, &candidate)
        || media::path_is_within(&downloads, &candidate);
    // Fallback: a media file the user themselves attached from anywhere
    // on disk, so the SENDER's own inline preview works. This is the
    // user's own machine and they already explicitly picked the file
    // (attach_file / read_user_image read arbitrary paths too), so the
    // only guard we add is a media-extension + size check.
    if !in_scope {
        let ext = candidate.extension().and_then(|s| s.to_str()).map(|s| s.to_ascii_lowercase()).unwrap_or_default();
        let is_media_ext = matches!(ext.as_str(),
            "png"|"jpg"|"jpeg"|"gif"|"webp"|"bmp"|"svg"
            |"mp3"|"m4a"|"aac"|"wav"|"ogg"|"oga"|"opus"|"flac"|"weba"
            |"mp4"|"m4v"|"webm"|"mov"|"mkv"|"avi"
        );
        if !is_media_ext {
            return Err("path is outside the allowed media directories".into());
        }
    }
    // Soft cap at 20MB so a hostile frontend can't force a huge
    // allocation.
    let md = std::fs::metadata(&candidate).map_err(|e| format!("stat: {e}"))?;
    if md.len() > 20 * 1024 * 1024 {
        return Err("file too large to inline".into());
    }
    std::fs::read(&candidate).map_err(|e| format!("read: {e}"))
}

/// Where received files go: `Downloads/Aquamarine` on desktop; on phones
/// the app's own storage. (Android's shared Download folder is not reliably
/// writable without MediaStore, and the webview's asset protocol can only
/// show files inside the app dirs — media saved there never appeared.)
pub(crate) fn downloads_dir(data_dir: &std::path::Path) -> PathBuf {
    #[cfg(target_os = "android")]
    {
        return data_dir.join("downloads");
    }
    #[cfg(target_os = "ios")]
    {
        return data_dir.join("Downloads");
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let _ = data_dir;
        dirs::download_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("Aquamarine")
    }
}

#[derive(Debug, serde::Serialize)]
pub struct DownloadResult {
    pub saved_path: String,
}

#[tauri::command]
pub async fn download_file(
    state: State<'_, StdMutex<AppState>>,
    app: AppHandle,
    peer_id: String,
    message_id: String,
) -> Result<DownloadResult, String> {
    // Extract envelope from the stored message.
    let envelope = {
        let s = state.lock().map_err(|e| e.to_string())?;
        let book = s.chats.get(&peer_id).ok_or("unknown peer")?;
        let msg = book.messages.iter().find(|m| m.id == message_id)
            .ok_or("unknown message id")?;
        match &msg.kind {
            MessageKind::File { cid, key_b64, nonce_b64, name, size, mime, category, .. } => {
                let key = URL_SAFE_NO_PAD.decode(key_b64)
                    .map_err(|e| format!("bad key: {e}"))?;
                let nonce = URL_SAFE_NO_PAD.decode(nonce_b64)
                    .map_err(|e| format!("bad nonce: {e}"))?;
                let key: [u8; 32] = key.try_into().map_err(|_| "bad key length".to_string())?;
                let nonce: [u8; 12] = nonce.try_into().map_err(|_| "bad nonce length".to_string())?;
                FileEnvelope {
                    cid: cid.clone(),
                    key,
                    nonce,
                    filename: name.clone(),
                    size: *size,
                    mime: mime.clone(),
                    category: category.clone(),
                }
            }
            _ => return Err("message is not a file".into()),
        }
    };

    // Download blob.
    let transport = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.transport.clone().ok_or("not connected")?
    };
    let ciphertext = transport.download_blob(&envelope.cid).await
        .map_err(|e| format!("download_blob: {e}"))?;

    // Decrypt.
    let plaintext = decrypt_file(&envelope, &ciphertext)
        .map_err(|e| format!("decrypt_file: {e}"))?;

    // Save to Downloads/Aquamarine.
    let data_dir = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.data_dir.clone()
    };
    let base = downloads_dir(&data_dir);
    std::fs::create_dir_all(&base).map_err(|e| format!("mkdir: {e}"))?;
    let save_path = pick_unique_path(&base, &envelope.filename);
    std::fs::write(&save_path, &plaintext).map_err(|e| format!("write: {e}"))?;
    let saved_str = save_path.to_string_lossy().to_string();

    // Update the message's saved_at.
    {
        let mut s = state.lock().map_err(|e| e.to_string())?;
        if let Some(book) = s.chats.get_mut(&peer_id) {
            if let Some(msg) = book.messages.iter_mut().find(|m| m.id == message_id) {
                if let MessageKind::File { saved_at, .. } = &mut msg.kind {
                    *saved_at = Some(saved_str.clone());
                }
            }
        }
    }
    let _ = app.emit("aqua://chats_updated", ());
    Ok(DownloadResult { saved_path: saved_str })
}

// ---- Article media (videos in carousels, music playlists) ---------------

fn media_cache_dir(data_dir: &std::path::Path) -> PathBuf {
    data_dir.join("media-cache")
}

fn media_ext(mime: Option<&str>, name: &str) -> String {
    let from_name = std::path::Path::new(name).extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase());
    if let Some(e) = from_name.filter(|e| e.len() <= 5 && e.chars().all(|c| c.is_ascii_alphanumeric())) {
        return e;
    }
    match mime.unwrap_or("") {
        "video/mp4" => "mp4", "video/webm" => "webm", "video/quicktime" => "mov", "video/x-matroska" => "mkv",
        "audio/mpeg" => "mp3", "audio/mp4" => "m4a", "audio/ogg" => "ogg", "audio/flac" => "flac",
        "audio/wav" => "wav", "audio/opus" => "opus", "audio/webm" => "weba", "audio/aac" => "aac",
        "image/png" => "png", "image/jpeg" => "jpg", "image/webp" => "webp", "image/gif" => "gif",
        _ => "bin",
    }.into()
}

fn cached_media(dir: &std::path::Path, cid: &str) -> Option<PathBuf> {
    let rd = std::fs::read_dir(dir).ok()?;
    for e in rd.flatten() {
        let n = e.file_name().to_string_lossy().to_string();
        if n.starts_with(cid) && n[cid.len()..].starts_with('.') && !n.ends_with(".part") {
            return Some(e.path());
        }
    }
    None
}

/// Encrypt + upload a video / audio file for an article and return its
/// reference (CID + key). The plaintext is kept in the local media cache so
/// the sender plays it without downloading it back.
#[tauri::command]
pub async fn upload_attachment(
    state: State<'_, StdMutex<AppState>>,
    app: AppHandle,
    peer_id: String,
    file_path: String,
) -> Result<crate::state::ArticleMedia, String> {
    let data_dir = { state.lock().map_err(|e| e.to_string())?.data_dir.clone() };
    let file_path = if is_uri_pick(&file_path) { materialize_picked(&app, &data_dir, &file_path)? } else { file_path };
    let path = PathBuf::from(&file_path);
    let size = std::fs::metadata(&path).map(|m| m.len()).map_err(|e| format!("{file_path}: {e}"))?;
    if size > aqua_core::MAX_FILE_SIZE {
        return Err(format!("file is too large: {} MB (limit {} MB)", size / (1024 * 1024), aqua_core::MAX_FILE_SIZE / (1024 * 1024)));
    }
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("media").to_string();
    let mime = mime_from_extension(&path);
    let kind = match mime.as_deref() {
        Some(m) if m.starts_with("video/") => "video",
        Some(m) if m.starts_with("audio/") => "audio",
        // Group / channel photos and banners (and article images).
        Some(m) if m.starts_with("image/") => "image",
        _ => return Err(format!("{name}: not an image, video or audio file")),
    };
    let cache = media_cache_dir(&data_dir);
    std::fs::create_dir_all(&cache).map_err(|e| format!("mkdir: {e}"))?;
    let ext = media_ext(mime.as_deref(), &name);

    // Saved Messages never leave this device.
    if peer_id == crate::state::SAVED_MESSAGES_ID {
        let cid = format!("local-{}", crate::state::uuid_like());
        tokio::fs::copy(&path, cache.join(format!("{cid}.{ext}"))).await.map_err(|e| format!("copy: {e}"))?;
        return Ok(crate::state::ArticleMedia {
            kind: kind.into(), cid, key_b64: String::new(), nonce_b64: String::new(),
            name, size, mime, poster: None, duration: None,
        });
    }

    let bytes = tokio::fs::read(&path).await.map_err(|e| format!("read {file_path}: {e}"))?;
    let (envelope, ciphertext) = encrypt_file_with_category(&bytes, &name, mime.clone(), Some(kind.into()))
        .map_err(|e| format!("encrypt_file: {e}"))?;
    let transport = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.transport.clone().ok_or("not connected")?
    };
    // Images are group photos / banners: kept by the relay for months.
    let keep = kind == "image" && ciphertext.len() <= 4 * 1024 * 1024;
    transport.upload_blob_ext(&envelope.cid, ciphertext, keep).await.map_err(|e| format!("upload_blob: {e}"))?;
    let _ = tokio::fs::write(cache.join(format!("{}.{ext}", envelope.cid)), &bytes).await;
    Ok(crate::state::ArticleMedia {
        kind: kind.into(),
        cid: envelope.cid.clone(),
        key_b64: URL_SAFE_NO_PAD.encode(envelope.key),
        nonce_b64: URL_SAFE_NO_PAD.encode(envelope.nonce),
        name,
        size,
        mime,
        poster: None,
        duration: None,
    })
}

/// Let the webview preview a media file the user just picked (for posters
/// and the composer preview) even when it lives outside the default asset
/// scope, e.g. on another drive.
#[tauri::command]
pub fn allow_media_preview(app: AppHandle, path: String) -> Result<(), String> {
    let p = PathBuf::from(&path);
    let mime = mime_from_extension(&p).unwrap_or_default();
    if !(mime.starts_with("video/") || mime.starts_with("audio/") || mime.starts_with("image/")) {
        return Err("not a media file".into());
    }
    app.asset_protocol_scope().allow_file(&p).map_err(|e| e.to_string())
}

/// Local path of an article video / track: from the cache, or downloaded
/// from the relay and decrypted on first use.
#[tauri::command]
pub async fn fetch_attachment(
    state: State<'_, StdMutex<AppState>>,
    media: crate::state::ArticleMedia,
) -> Result<String, String> {
    let media = media.sane().ok_or("bad media reference")?;
    let data_dir = { state.lock().map_err(|e| e.to_string())?.data_dir.clone() };
    let cache = media_cache_dir(&data_dir);
    std::fs::create_dir_all(&cache).map_err(|e| format!("mkdir: {e}"))?;
    if let Some(p) = cached_media(&cache, &media.cid) {
        return Ok(p.to_string_lossy().into());
    }
    if media.cid.starts_with("local-") {
        return Err("this file only exists on the device it was added on".into());
    }
    let key: [u8; 32] = URL_SAFE_NO_PAD.decode(&media.key_b64).map_err(|e| format!("bad key: {e}"))?
        .try_into().map_err(|_| "bad key length".to_string())?;
    let nonce: [u8; 12] = URL_SAFE_NO_PAD.decode(&media.nonce_b64).map_err(|e| format!("bad nonce: {e}"))?
        .try_into().map_err(|_| "bad nonce length".to_string())?;
    let envelope = FileEnvelope {
        cid: media.cid.clone(), key, nonce,
        filename: media.name.clone(), size: media.size,
        mime: media.mime.clone(), category: Some(media.kind.clone()),
    };
    let transport = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.transport.clone().ok_or("not connected")?
    };
    let ciphertext = transport.download_blob(&media.cid).await.map_err(|e| format!("download_blob: {e}"))?;
    let plain = decrypt_file(&envelope, &ciphertext).map_err(|e| format!("decrypt_file: {e}"))?;
    let ext = media_ext(media.mime.as_deref(), &media.name);
    let out = cache.join(format!("{}.{ext}", media.cid));
    let part = cache.join(format!("{}.{ext}.part", media.cid));
    tokio::fs::write(&part, &plain).await.map_err(|e| format!("write: {e}"))?;
    tokio::fs::rename(&part, &out).await.map_err(|e| format!("rename: {e}"))?;
    Ok(out.to_string_lossy().into())
}

fn mime_from_extension(path: &std::path::Path) -> Option<String> {
    let ext = path.extension()?.to_str()?.to_lowercase();
    Some(match ext.as_str() {
        // images
        "png"  => "image/png".into(),
        "jpg" | "jpeg" => "image/jpeg".into(),
        "gif"  => "image/gif".into(),
        "webp" => "image/webp".into(),
        "bmp"  => "image/bmp".into(),
        "svg"  => "image/svg+xml".into(),
        // audio
        "mp3"  => "audio/mpeg".into(),
        "m4a"  => "audio/mp4".into(),
        "aac"  => "audio/aac".into(),
        "wav"  => "audio/wav".into(),
        "ogg" | "oga" => "audio/ogg".into(),
        "opus" => "audio/opus".into(),
        "flac" => "audio/flac".into(),
        "weba" => "audio/webm".into(),
        // video
        "mp4" | "m4v" => "video/mp4".into(),
        "webm" => "video/webm".into(),
        "mov"  => "video/quicktime".into(),
        "mkv"  => "video/x-matroska".into(),
        "avi"  => "video/x-msvideo".into(),
        // docs / misc
        "pdf"  => "application/pdf".into(),
        "txt"  => "text/plain".into(),
        "md"   => "text/markdown".into(),
        "json" => "application/json".into(),
        "zip"  => "application/zip".into(),
        _      => return None,
    })
}

fn pick_unique_path(dir: &std::path::Path, filename: &str) -> PathBuf {
    let mut candidate = dir.join(filename);
    if !candidate.exists() {
        return candidate;
    }
    let stem = std::path::Path::new(filename)
        .file_stem().and_then(|s| s.to_str()).unwrap_or(filename);
    let ext = std::path::Path::new(filename)
        .extension().and_then(|s| s.to_str());
    for i in 1..1000 {
        let name = match ext {
            Some(e) => format!("{stem} ({i}).{e}"),
            None    => format!("{stem} ({i})"),
        };
        candidate = dir.join(&name);
        if !candidate.exists() { return candidate; }
    }
    candidate
}

// ---------- Start chat / connect ------------------------------------------

#[tauri::command]
pub async fn start_chat_with(
    state: State<'_, StdMutex<AppState>>,
    app: AppHandle,
    peer_username: String,
    first_message: String,
) -> Result<ChatSummary, String> {
    let transport = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.transport.clone().ok_or("not connected")?
    };
    let bundle = transport.fetch_bundle(&peer_username).await
        .map_err(|e| format!("fetch_bundle: {e}"))?;
    bundle.verify().map_err(|e| format!("bad bundle: {e}"))?;

    // Build the session + initial message outside the lock.
    let identity_clone = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.identity.clone()
    };
    let (session, initial) = Session::start_as_initiator(
        &identity_clone, &bundle, first_message.as_bytes(),
    ).map_err(|e| format!("start_as_initiator: {e}"))?;

    // Send the initial before persisting state — if the wire fails we
    // don't want a session sitting in the vault the peer can't decrypt.
    transport.send_initial(&peer_username, &initial)
        .map_err(|e| format!("send_initial: {e}"))?;

    // Persist + update chat store.
    let summary = {
        let mut s = state.lock().map_err(|e| e.to_string())?;
        s.vault.save_session(&peer_username, &session)
            .map_err(|e| format!("save_session: {e}"))?;
        s.sessions.insert(peer_username.clone(), session);

        let mut summary = summary_for(&peer_username, &first_message, true, false);
        summary.online = true;
        let book = ChatBook {
            summary: summary.clone(),
            messages: vec![text_msg_from_me(&first_message)],
        };
        s.chats.insert(peer_username.clone(), book);
        s.chat_order.retain(|id| id != &peer_username);
        s.chat_order.insert(0, peer_username.clone());
        summary
    };

    let _ = app.emit("aqua://chats_updated", ());
    Ok(summary)
}

// ---------- Call signaling ------------------------------------------------

/// Magic prefix for a WebRTC call signal piggybacked on the E2EE text
/// channel. Payload after the colon is caller-defined JSON (SDP offer,
/// SDP answer, ICE candidate, hangup — the frontend owns the shape).
pub const CALL_PREFIX: &str = "AQUA-CALL:v1:";

/// Control-message prefixes carried over the same E2EE text channel but
/// intercepted on receive so they never appear as chat bubbles.
pub const POLLVOTE_PREFIX: &str = "[pollvote:v1]";
pub const POLLCLOSE_PREFIX: &str = "[pollclose:v1]";
pub const SEEN_PREFIX: &str = "[seen:v1]";
pub const STATUS_PREFIX: &str = "[status:v1]";
pub const REACT_PREFIX: &str = "[react:v1]";
/// Group metadata snapshot (title, members, roles, settings), fanned out
/// to members so everyone converges on the newest `version`.
/// Peer is typing (transient, 1:1). Carries no body.
pub const TYPING_PREFIX: &str = "[typing:v1]";
/// Edit a previously-sent message: JSON `{old, new}` (matched by exact text).
pub const EDIT_PREFIX: &str = "[edit:v1]";
/// Delete a previously-sent message: JSON `{text}` (matched by exact text).
pub const DEL_PREFIX: &str = "[del:v1]";
pub const GRPINFO_PREFIX: &str = "[grpinfo:v1]";
/// A member's own edits that don't need an admin: `{gid, tag}` (member tag,
/// when the group lets members set their own).
pub const GRPSELF_PREFIX: &str = "[grpself:v1]";
/// A join request to a group admin: `{gid, title, note}`.
pub const GRPJOIN_PREFIX: &str = "[grpjoin:v1]";
/// "I left this group": `{gid}` — everyone drops the sender from their copy.
pub const GRPLEAVE_PREFIX: &str = "[grpleave:v1]";
/// Group control (poll votes, reactions): fanned out, never stored.
pub const GRPCTL_PREFIX: &str = "[grpctl:v1]";
/// Group voice / video chat signalling (join, leave, offer, answer, ice).
pub const GCALL_PREFIX: &str = "[gcall:v1]";
/// A group chat message: JSON `{gid, author, body}` where `body` is the
/// usual tagged text (plain / `[reply:v1]` / `[poll:v1]` / custom emoji).
pub const GRPMSG_PREFIX: &str = "[grpmsg:v1]";

/// Send a raw control payload to a peer over the E2EE session WITHOUT
/// recording it as a chat message. Used for poll votes and read
/// receipts. No-op-safe: fails quietly if there's no live session.
#[tauri::command]
pub async fn send_control(
    state: State<'_, StdMutex<AppState>>,
    peer_id: String,
    payload: String,
) -> Result<(), String> {
    let (transport, env) = {
        let mut s = state.lock().map_err(|e| e.to_string())?;
        if s.chats.get(&peer_id).map(|c| !c.summary.live).unwrap_or(true) {
            return Ok(()); // nothing to do without a live session
        }
        let env = {
            let session = s.sessions.get_mut(&peer_id).ok_or("no session for peer")?;
            session.encrypt(payload.as_bytes()).map_err(|e| format!("encrypt: {e}"))?
        };
        if let Some(session) = s.sessions.get(&peer_id) {
            let _ = s.vault.save_session(&peer_id, session);
        }
        (s.transport.clone(), env)
    };
    let Some(transport) = transport else { return Ok(()) };
    transport.send_envelope(&peer_id, &env).map_err(|e| format!("send: {e}"))?;
    Ok(())
}

// ---------- Groups / supergroups / channels --------------------------------

/// Deliver a payload to one member over the E2EE session, establishing a
/// fresh session via X3DH if none exists yet (the payload rides as the
/// initial message). Used for group fan-out.
async fn deliver_to_member(app: &AppHandle, member: &str, payload: &str) -> Result<(), String> {
    let state: State<'_, StdMutex<AppState>> = app.state();

    // Fast path: a live session already exists.
    let fast = {
        let mut s = state.lock().map_err(|e| e.to_string())?;
        if s.sessions.contains_key(member) {
            let env = {
                let session = s.sessions.get_mut(member).unwrap();
                session.encrypt(payload.as_bytes()).map_err(|e| format!("encrypt: {e}"))?
            };
            if let Some(session) = s.sessions.get(member) {
                let _ = s.vault.save_session(member, session);
            }
            Some((s.transport.clone(), env))
        } else {
            None
        }
    };
    if let Some((transport, env)) = fast {
        let transport = transport.ok_or("not connected")?;
        transport.send_envelope(member, &env).map_err(|e| format!("send: {e}"))?;
        return Ok(());
    }

    // Slow path: establish a session, sending the payload as the initial.
    let (transport, identity) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        (s.transport.clone().ok_or("not connected")?, s.identity.clone())
    };
    let bundle = transport.fetch_bundle(member).await.map_err(|e| format!("fetch_bundle: {e}"))?;
    bundle.verify().map_err(|e| format!("bad bundle: {e}"))?;
    let (session, initial) = Session::start_as_initiator(&identity, &bundle, payload.as_bytes())
        .map_err(|e| format!("start_as_initiator: {e}"))?;
    transport.send_initial(member, &initial).map_err(|e| format!("send_initial: {e}"))?;
    {
        let mut s = state.lock().map_err(|e| e.to_string())?;
        let _ = s.vault.save_session(member, &session);
        s.sessions.insert(member.to_string(), session);
    }
    Ok(())
}

#[tauri::command]
pub fn list_groups(state: State<'_, StdMutex<AppState>>) -> Result<Vec<Group>, String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    Ok(s.groups.values().cloned().collect())
}

/// Create a group / supergroup / channel. Fans out the initial `[grpinfo]`
/// snapshot to every member (which also establishes sessions with them).
#[tauri::command]
pub async fn create_group(
    app: AppHandle,
    state: State<'_, StdMutex<AppState>>,
    kind: String,
    title: String,
    description: String,
    members: Vec<String>,
) -> Result<Group, String> {
    let me = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.username.clone().ok_or("not registered")?
    };
    let id = format!("grp_{}", crate::state::uuid_like());
    let (_, gradient) = gradient_for(&title);

    let mut member_list = vec![GroupMember { username: me.clone(), role: "owner".into(), extra: Default::default() }];
    for u in &members {
        let u = u.trim().to_lowercase();
        if !u.is_empty() && u != me && !member_list.iter().any(|m| m.username == u) {
            member_list.push(GroupMember { username: u, role: "member".into(), extra: Default::default() });
        }
    }

    let group = Group {
        id: id.clone(),
        kind: kind.clone(),
        title: title.trim().to_string(),
        description: description.trim().to_string(),
        gradient,
        owner: me.clone(),
        members: member_list,
        settings: GroupSettings::default(),
        created_at: hhmm_now(),
        version: 1,
        extra: Default::default(),
    };

    // Persist + surface as a chat locally.
    {
        let mut s = state.lock().map_err(|e| e.to_string())?;
        s.groups.insert(id.clone(), group.clone());
        save_groups(&s.data_dir, &s.groups);
        if !s.chats.contains_key(&id) {
            s.chats.insert(id.clone(), ChatBook {
                summary: group_summary(&group, &group.description),
                messages: Vec::new(),
            });
            s.chat_order.insert(0, id.clone());
        }
    }

    // Fan the snapshot out to the other members.
    let snapshot = format!("{GRPINFO_PREFIX}{}", serde_json::to_string(&group).unwrap_or_default());
    for m in group.members.iter().filter(|m| m.username != me) {
        if let Err(e) = deliver_to_member(&app, &m.username, &snapshot).await {
            tracing::warn!(member = %m.username, %e, "create_group: snapshot delivery failed");
        }
    }

    let _ = app.emit("aqua://chats_updated", ());
    let _ = app.emit("aqua://groups_updated", ());
    Ok(group)
}

/// Replace a group's metadata (title / settings / members / roles) and fan
/// the new snapshot out. Only owner/admins may call it meaningfully.
#[tauri::command]
pub async fn update_group(
    app: AppHandle,
    state: State<'_, StdMutex<AppState>>,
    group: Group,
) -> Result<Group, String> {
    let (me, mut next) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        (s.username.clone().ok_or("not registered")?, group)
    };
    next.version += 1;

    // Recipients = union of old + new members (so removed members are told).
    let mut recipients: Vec<String> = next.members.iter().map(|m| m.username.clone()).collect();
    {
        let mut s = state.lock().map_err(|e| e.to_string())?;
        if let Some(old) = s.groups.get(&next.id) {
            for m in &old.members {
                if !recipients.contains(&m.username) { recipients.push(m.username.clone()); }
            }
        }
        s.groups.insert(next.id.clone(), next.clone());
        save_groups(&s.data_dir, &s.groups);
        if let Some(book) = s.chats.get_mut(&next.id) {
            book.summary.name = next.title.clone();
            book.summary.gradient = next.gradient.clone();
            book.summary.channel = next.kind == "channel";
        }
    }

    let snapshot = format!("{GRPINFO_PREFIX}{}", serde_json::to_string(&next).unwrap_or_default());
    for m in recipients.iter().filter(|u| *u != &me) {
        if let Err(e) = deliver_to_member(&app, m, &snapshot).await {
            tracing::warn!(member = %m, %e, "update_group: snapshot delivery failed");
        }
    }

    let _ = app.emit("aqua://chats_updated", ());
    let _ = app.emit("aqua://groups_updated", ());
    Ok(next)
}

/// Leave a group locally and drop its chat. (Other members keep it; they'll
/// simply stop receiving from us.)
#[tauri::command]
pub async fn leave_group(
    app: AppHandle,
    state: State<'_, StdMutex<AppState>>,
    gid: String,
) -> Result<(), String> {
    let (me, others) = {
        let mut s = state.lock().map_err(|e| e.to_string())?;
        let me = s.username.clone().unwrap_or_default();
        let others: Vec<String> = s.groups.get(&gid)
            .map(|g| g.members.iter().map(|m| m.username.clone()).filter(|u| u != &me).collect())
            .unwrap_or_default();
        s.groups.remove(&gid);
        save_groups(&s.data_dir, &s.groups);
        s.chats.remove(&gid);
        s.chat_order.retain(|id| id != &gid);
        (me, others)
    };
    let wire = format!("{GRPLEAVE_PREFIX}{}", serde_json::json!({ "gid": gid }));
    for u in others.iter().filter(|u| *u != &me) {
        let _ = deliver_to_member(&app, u, &wire).await;
    }
    let _ = app.emit("aqua://chats_updated", ());
    let _ = app.emit("aqua://groups_updated", ());
    Ok(())
}

/// Set my own member tag in a group (Telegram's member tags / "prefix").
/// Allowed when the group lets members edit their own tags, or I'm an admin.
#[tauri::command]
pub async fn group_set_my_tag(
    app: AppHandle,
    state: State<'_, StdMutex<AppState>>,
    gid: String,
    tag: String,
) -> Result<(), String> {
    let tag: String = tag.trim().chars().take(24).collect();
    let (me, recipients) = {
        let mut s = state.lock().map_err(|e| e.to_string())?;
        let me = s.username.clone().ok_or("not registered")?;
        let g = s.groups.get_mut(&gid).ok_or("unknown group")?;
        if g.perm("edit_own_tags") == "admins" && !g.is_admin(&me) {
            return Err("Only admins can set member tags in this group".into());
        }
        if let Some(m) = g.members.iter_mut().find(|m| m.username == me) {
            m.extra.insert("tag".into(), serde_json::Value::String(tag.clone()));
        }
        let recipients: Vec<String> = g.members.iter().map(|m| m.username.clone()).filter(|u| u != &me).collect();
        save_groups(&s.data_dir, &s.groups);
        (me, recipients)
    };
    let wire = format!("{GRPSELF_PREFIX}{}", serde_json::json!({ "gid": gid, "tag": tag }));
    for u in recipients.iter().filter(|u| *u != &me) {
        let _ = deliver_to_member(&app, u, &wire).await;
    }
    let _ = app.emit("aqua://groups_updated", ());
    Ok(())
}

/// Ask a group's admin to let me in (from a server tag / public link).
#[tauri::command]
pub async fn group_request_join(
    app: AppHandle,
    gid: String,
    admin: String,
    title: String,
    note: String,
) -> Result<(), String> {
    let note: String = note.chars().take(200).collect();
    let wire = format!("{GRPJOIN_PREFIX}{}", serde_json::json!({ "gid": gid, "title": title, "note": note }));
    deliver_to_member(&app, &admin, &wire).await
}

/// Send a message to a group by fanning it out to every other member.
#[tauri::command]
pub async fn group_send(
    app: AppHandle,
    state: State<'_, StdMutex<AppState>>,
    gid: String,
    body: String,
) -> Result<SendResult, String> {
    let (me, group, room) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        let g = s.groups.get(&gid).cloned().ok_or("unknown group")?;
        let room = crate::state::resolve_room(&g, s.active_rooms.get(&gid).map(String::as_str));
        (s.username.clone().ok_or("not registered")?, g, room)
    };

    // Permission: the same rule every receiver applies (state.rs may_post).
    if !crate::state::may_post_in(&group, &me, &body, room.as_ref()) {
        return Err("You can't post this here.".into());
    }
    let room_id = room.map(|r| r.id);

    // Local echo.
    let out = {
        let mut s = state.lock().map_err(|e| e.to_string())?;
        let msg = ChatMessage {
            meta: room_id.clone().map(|r| crate::state::MsgMeta { room: Some(r), ..Default::default() }),
            id: format!("m-{}", crate::state::uuid_like()),
            from: me.clone(),
            time: hhmm_now(),
            kind: classify_kind(&body),
        };
        let out = (msg.id.clone(), msg.time.clone());
        if let Some(book) = s.chats.get_mut(&gid) {
            book.summary.preview = preview_for(&msg.kind);
            book.summary.preview_by_me = true;
            book.summary.time = msg.time.clone();
            book.messages.push(msg);
        }
        if let Some(pos) = s.chat_order.iter().position(|id| id == &gid) {
            if pos > 0 { s.chat_order.remove(pos); s.chat_order.insert(0, gid.clone()); }
        }
        out
    };

    // Fan out.
    let wire = format!(
        "{GRPMSG_PREFIX}{}",
        serde_json::json!({ "gid": gid, "author": me, "body": body, "room": room_id })
    );
    for m in group.members.iter().filter(|m| m.username != me) {
        if let Err(e) = deliver_to_member(&app, &m.username, &wire).await {
            tracing::warn!(member = %m.username, %e, "group_send: delivery failed");
        }
    }

    Ok(SendResult { id: out.0, time: out.1 })
}

/// The room the UI has open in a supergroup: group sends without an
/// explicit room (text, files, stickers, polls) go there.
#[tauri::command]
pub fn set_group_room(state: State<'_, StdMutex<AppState>>, gid: String, room: String) -> Result<(), String> {
    let mut s = state.lock().map_err(|e| e.to_string())?;
    if room.is_empty() { s.active_rooms.remove(&gid); } else { s.active_rooms.insert(gid, room); }
    Ok(())
}

/// Send a group control payload (a poll vote, a reaction) to every member.
/// Nothing is stored in the chat — receivers turn it into the same event a
/// 1:1 control message would raise, tagged with the group id.
#[tauri::command]
pub async fn group_control(
    app: AppHandle,
    state: State<'_, StdMutex<AppState>>,
    gid: String,
    body: String,
) -> Result<(), String> {
    if !(body.starts_with(POLLVOTE_PREFIX) || body.starts_with(REACT_PREFIX) || body.starts_with(POLLCLOSE_PREFIX)
        || body.starts_with(EDIT_PREFIX) || body.starts_with(DEL_PREFIX) || body.starts_with(TYPING_PREFIX)
        || body.starts_with(GCALL_PREFIX)) {
        return Err("unsupported group control".into());
    }
    let (me, group) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        let g = s.groups.get(&gid).cloned().ok_or("unknown group")?;
        (s.username.clone().ok_or("not registered")?, g)
    };
    if !group.is_member(&me) || group.is_banned(&me) {
        return Err("not a member".into());
    }
    let wire = format!("{GRPCTL_PREFIX}{}", serde_json::json!({ "gid": gid, "body": body }));
    for m in group.members.iter().filter(|m| m.username != me) {
        if let Err(e) = deliver_to_member(&app, &m.username, &wire).await {
            tracing::warn!(member = %m.username, %e, "group_control: delivery failed");
        }
    }
    Ok(())
}

/// Group-call signalling to ONE member (offer / answer / ICE). Same wire as
/// `group_control`, so the receiver checks membership the same way.
#[tauri::command]
pub async fn group_control_to(
    app: AppHandle,
    state: State<'_, StdMutex<AppState>>,
    gid: String,
    to: String,
    body: String,
) -> Result<(), String> {
    if !body.starts_with(GCALL_PREFIX) {
        return Err("unsupported group control".into());
    }
    let ok = {
        let s = state.lock().map_err(|e| e.to_string())?;
        let me = s.username.clone().unwrap_or_default();
        s.groups.get(&gid).map(|g| g.is_member(&me) && g.is_member(&to)).unwrap_or(false)
    };
    if !ok {
        return Err("not a member".into());
    }
    let wire = format!("{GRPCTL_PREFIX}{}", serde_json::json!({ "gid": gid, "body": body }));
    deliver_to_member(&app, &to, &wire).await
}

/// Handle a decrypted group control payload (info snapshot or message).
/// Returns true if the text was a group payload and was consumed.
fn route_group_payload(app: &AppHandle, from: &str, text: &str) -> bool {
    if let Some(body) = text.strip_prefix(GRPINFO_PREFIX) {
        let state: State<'_, StdMutex<AppState>> = app.state();
        if let Ok(g) = serde_json::from_str::<Group>(body) {
            let mut s = match state.lock() { Ok(s) => s, Err(_) => return true };
            // Only the group's admins may change it, never backwards in time.
            let accept = crate::state::accept_group_snapshot(s.groups.get(&g.id), &g, from);
            if !accept {
                tracing::warn!(%from, gid = %g.id, "rejected group snapshot from non-admin / stale");
            }
            // Removed from the group → drop it locally.
            let me = s.username.clone().unwrap_or_default();
            if accept && !g.is_member(&me) {
                s.groups.remove(&g.id);
                save_groups(&s.data_dir, &s.groups);
                s.chats.remove(&g.id);
                s.chat_order.retain(|id| id != &g.id);
                drop(s);
                let _ = app.emit("aqua://chats_updated", ());
                let _ = app.emit("aqua://groups_updated", ());
                return true;
            }
            if accept {
                let is_new = !s.chats.contains_key(&g.id);
                if let Some(book) = s.chats.get_mut(&g.id) {
                    book.summary.name = g.title.clone();
                    book.summary.gradient = g.gradient.clone();
                    book.summary.channel = g.kind == "channel";
                } else {
                    s.chats.insert(g.id.clone(), ChatBook {
                        summary: group_summary(&g, &g.description),
                        messages: Vec::new(),
                    });
                    s.chat_order.insert(0, g.id.clone());
                }
                s.groups.insert(g.id.clone(), g);
                save_groups(&s.data_dir, &s.groups);
                drop(s);
                if is_new { let _ = app.emit("aqua://chats_updated", ()); }
                let _ = app.emit("aqua://groups_updated", ());
            }
        }
        return true;
    }

    if let Some(body) = text.strip_prefix(GRPMSG_PREFIX) {
        let state: State<'_, StdMutex<AppState>> = app.state();
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
            let gid = v.get("gid").and_then(|x| x.as_str()).unwrap_or("").to_string();
            // The author is whoever's E2EE session delivered it — a claimed
            // "author" field can't impersonate someone else.
            let author = from.to_string();
            let inner = v.get("body").and_then(|x| x.as_str()).unwrap_or("").to_string();
            if gid.is_empty() { return true; }
            let time = hhmm_now();
            {
                let mut s = match state.lock() { Ok(s) => s, Err(_) => return true };
                // Only accept messages for groups we know about, from members
                // who are allowed to post this (in this room).
                let room = match s.groups.get(&gid) {
                    Some(g) => {
                        let room = crate::state::resolve_room(g, v.get("room").and_then(|x| x.as_str()));
                        if !crate::state::may_post_in(g, &author, &inner, room.as_ref()) { return true; }
                        room
                    }
                    None => return true,
                };
                let kind = classify_kind(&inner);
                let preview = preview_for(&kind);
                let len = s.chats.get(&gid).map(|b| b.messages.len()).unwrap_or(0);
                if let Some(book) = s.chats.get_mut(&gid) {
                    book.messages.push(ChatMessage {
                        meta: room.map(|r| crate::state::MsgMeta { room: Some(r.id), ..Default::default() }),
                        id: format!("g-{time}-{len}"),
                        from: author.clone(),
                        time: time.clone(),
                        kind,
                    });
                    book.summary.preview = preview;
                    book.summary.preview_by_me = false;
                    book.summary.time = time.clone();
                    let is_active = false; // UI decides unread via its own active id
                    if !is_active { book.summary.unread += 1; }
                }
                if let Some(pos) = s.chat_order.iter().position(|id| id == &gid) {
                    if pos > 0 { s.chat_order.remove(pos); s.chat_order.insert(0, gid.clone()); }
                }
            }
            let _ = app.emit("aqua://incoming", IncomingEventPayload {
                peer_id: gid,
                text: inner,
                time,
                is_new_chat: false,
            });
        }
        return true;
    }

    if let Some(body) = text.strip_prefix(GRPSELF_PREFIX) {
        let state: State<'_, StdMutex<AppState>> = app.state();
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
            let gid = v.get("gid").and_then(|x| x.as_str()).unwrap_or("");
            let tag: String = v.get("tag").and_then(|x| x.as_str()).unwrap_or("").chars().take(24).collect();
            let mut s = match state.lock() { Ok(s) => s, Err(_) => return true };
            let mut changed = false;
            if let Some(g) = s.groups.get_mut(gid) {
                let allowed = g.perm("edit_own_tags") != "admins" || g.is_admin(from);
                if allowed {
                    if let Some(m) = g.members.iter_mut().find(|m| m.username == from) {
                        m.extra.insert("tag".into(), serde_json::Value::String(tag));
                        changed = true;
                    }
                }
            }
            if changed {
                save_groups(&s.data_dir, &s.groups);
                drop(s);
                let _ = app.emit("aqua://groups_updated", ());
            }
        }
        return true;
    }

    if let Some(body) = text.strip_prefix(GRPJOIN_PREFIX) {
        let state: State<'_, StdMutex<AppState>> = app.state();
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
            let gid = v.get("gid").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let note: String = v.get("note").and_then(|x| x.as_str()).unwrap_or("").chars().take(200).collect();
            let mut s = match state.lock() { Ok(s) => s, Err(_) => return true };
            let me = s.username.clone().unwrap_or_default();
            let mut changed = false;
            if let Some(g) = s.groups.get_mut(&gid) {
                if g.is_admin(&me) && !g.is_member(from) && !g.is_banned(from) {
                    let list = g.extra.entry("requests").or_insert_with(|| serde_json::json!([]));
                    if let Some(arr) = list.as_array_mut() {
                        if !arr.iter().any(|r| r.get("username").and_then(|u| u.as_str()) == Some(from)) && arr.len() < 500 {
                            arr.push(serde_json::json!({ "username": from, "note": note, "at": hhmm_now() }));
                            changed = true;
                        }
                    }
                }
            }
            if changed {
                save_groups(&s.data_dir, &s.groups);
                drop(s);
                let _ = app.emit("aqua://groups_updated", ());
                let _ = app.emit("aqua://group_join_request", serde_json::json!({ "gid": gid, "from": from }));
            }
        }
        return true;
    }

    if let Some(body) = text.strip_prefix(GRPCTL_PREFIX) {
        let state: State<'_, StdMutex<AppState>> = app.state();
        let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else { return true };
        let gid = v.get("gid").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let inner = v.get("body").and_then(|x| x.as_str()).unwrap_or("");
        let ok = {
            let s = match state.lock() { Ok(s) => s, Err(_) => return true };
            s.groups.get(&gid).map(|g| g.is_member(from) && !g.is_banned(from)).unwrap_or(false)
        };
        if !ok {
            return true;
        }
        if let Some(b) = inner.strip_prefix(POLLVOTE_PREFIX) {
            if let Ok(vote) = serde_json::from_str::<serde_json::Value>(b) {
                let _ = app.emit("aqua://poll_vote", serde_json::json!({ "from": from, "vote": vote, "gid": gid }));
            }
        } else if let Some(b) = inner.strip_prefix(POLLCLOSE_PREFIX) {
            if let Ok(c) = serde_json::from_str::<serde_json::Value>(b) {
                let _ = app.emit("aqua://poll_close", serde_json::json!({ "from": from, "close": c, "gid": gid }));
            }
        } else if let Some(b) = inner.strip_prefix(REACT_PREFIX) {
            if let Ok(r) = serde_json::from_str::<serde_json::Value>(b) {
                let _ = app.emit("aqua://reaction", serde_json::json!({ "from": from, "react": r, "gid": gid }));
            }
        } else if let Some(b) = inner.strip_prefix(GCALL_PREFIX) {
            if let Ok(sig) = serde_json::from_str::<serde_json::Value>(b) {
                let _ = app.emit("aqua://gcall", serde_json::json!({ "from": from, "gid": gid, "signal": sig }));
            }
        } else if inner.starts_with(TYPING_PREFIX) {
            let _ = app.emit("aqua://typing", serde_json::json!({ "from": from, "gid": gid }));
        } else if let Some(b) = inner.strip_prefix(EDIT_PREFIX) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(b) {
                let old = v.get("old").and_then(|x| x.as_str()).unwrap_or("");
                let new = v.get("new").and_then(|x| x.as_str()).unwrap_or("");
                if !old.is_empty() {
                    if let Ok(mut s) = state.lock() { apply_edit(&mut s, &gid, from, old, new); }
                    let _ = app.emit("aqua://chats_updated", ());
                    let _ = app.emit("aqua://incoming", IncomingEventPayload { peer_id: gid.clone(), text: String::new(), time: hhmm_now(), is_new_chat: false });
                }
            }
        } else if let Some(b) = inner.strip_prefix(DEL_PREFIX) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(b) {
                let t = v.get("text").and_then(|x| x.as_str()).unwrap_or("");
                // Admins with the right may delete other people's messages.
                let target = v.get("author").and_then(|x| x.as_str()).unwrap_or(from).to_string();
                if !t.is_empty() {
                    if let Ok(mut s) = state.lock() {
                        let ok = target == from || s.groups.get(&gid).map(|g| crate::state::can_delete_others(g, from)).unwrap_or(false);
                        if ok { apply_delete_ex(&mut s, &gid, &target, t, true); }
                    }
                    let _ = app.emit("aqua://chats_updated", ());
                    let _ = app.emit("aqua://incoming", IncomingEventPayload { peer_id: gid.clone(), text: String::new(), time: hhmm_now(), is_new_chat: false });
                }
            }
        }
        return true;
    }

    if let Some(body) = text.strip_prefix(GRPLEAVE_PREFIX) {
        let state: State<'_, StdMutex<AppState>> = app.state();
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
            let gid = v.get("gid").and_then(|x| x.as_str()).unwrap_or("");
            let mut s = match state.lock() { Ok(s) => s, Err(_) => return true };
            let mut changed = false;
            if let Some(g) = s.groups.get_mut(gid) {
                if g.owner != from {
                    let before = g.members.len();
                    g.members.retain(|m| m.username != from);
                    changed = g.members.len() != before;
                }
            }
            if changed {
                save_groups(&s.data_dir, &s.groups);
                drop(s);
                let _ = app.emit("aqua://groups_updated", ());
            }
        }
        return true;
    }

    false
}

#[tauri::command]
pub async fn send_call_signal(
    state: State<'_, StdMutex<AppState>>,
    peer_id: String,
    signal_json: String,
) -> Result<(), String> {
    let (transport, env) = {
        let mut s = state.lock().map_err(|e| e.to_string())?;
        if s.chats.get(&peer_id).map(|c| !c.summary.live).unwrap_or(true) {
            return Err("no live session with peer".into());
        }
        let plaintext = format!("{CALL_PREFIX}{signal_json}");
        let env = {
            let session = s.sessions.get_mut(&peer_id).ok_or("no session for peer")?;
            session.encrypt(plaintext.as_bytes())
                .map_err(|e| format!("encrypt: {e}"))?
        };
        if let Some(session) = s.sessions.get(&peer_id) {
            let _ = s.vault.save_session(&peer_id, session);
        }
        (s.transport.clone(), env)
    };
    let Some(transport) = transport else { return Err("not connected".into()) };
    transport.send_envelope(&peer_id, &env)
        .map_err(|e| format!("send: {e}"))?;
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CallSignalEvent {
    pub from: String,
    pub signal: serde_json::Value,
}

// ---------- Incoming reader dispatch ---------------------------------------

async fn handle_incoming(app: &AppHandle, inc: aqua_client_core::Incoming) {
    use aqua_client_core::Incoming::*;
    match inc {
        Initial { from, message } => on_incoming_initial(app, from, message).await,
        Envelope { from, envelope } => on_incoming_envelope(app, from, envelope).await,
        Plaintext { from, text } => on_incoming_plaintext(app, from, text).await,
        Closed => {
            tracing::info!("transport closed by server");
            let _ = app.emit("aqua://disconnected", ());
        }
        Error(e) => {
            tracing::warn!(err = %e, "transport error");
            let _ = app.emit("aqua://transport_error", e);
        }
    }
}

/// An un-encrypted message from a bot. There's no Session; we just
/// append it to a (bot) chat, creating one if needed. Bot chats are
/// marked `has_lock = false` so the UI shows them as not E2EE.
async fn on_incoming_plaintext(app: &AppHandle, from: String, text: String) {
    let state: State<'_, StdMutex<AppState>> = app.state();
    let time = hhmm_now();
    let is_new_chat;
    {
        let mut s = match state.lock() { Ok(s) => s, Err(_) => return };
        is_new_chat = !s.chats.contains_key(&from);
        if is_new_chat {
            let (letter, gradient) = gradient_for(&from);
            let summary = ChatSummary {
                id: from.clone(),
                name: from.clone(),
                letter, gradient,
                preview: text.clone(),
                preview_is_typing: false, preview_by_me: false, has_lock: false,
                time: time.clone(),
                unread: 1, online: true, verified: false, channel: false, live: true,
            };
            s.chats.insert(from.clone(), ChatBook { summary, messages: Vec::new() });
            s.chat_order.retain(|id| id != &from);
            s.chat_order.insert(0, from.clone());
        }
        push_peer_message(&mut s, &from, text.clone(), &time);
    }
    let _ = app.emit("aqua://incoming", IncomingEventPayload {
        peer_id: from,
        text,
        time,
        is_new_chat,
    });
}

async fn on_incoming_initial(
    app: &AppHandle,
    from: String,
    message: aqua_core::wire::InitialMessage,
) {
    let state: State<'_, StdMutex<AppState>> = app.state();

    // Consume SPK + optional OPK from the vault under the lock.
    let (identity_clone, spk, opk) = {
        let s = match state.lock() { Ok(s) => s, Err(_) => return };
        let identity = s.identity.clone();
        let spk = match s.vault.latest_signed_prekey() {
            Ok(Some((spk, _))) => spk,
            _ => { tracing::warn!("no SPK for incoming initial"); return }
        };
        let opk = message.used_opk_id
            .and_then(|id| s.vault.take_one_time_prekey(id).ok().flatten());
        drop(s);
        (identity, spk, opk)
    };

    // Run the crypto outside the lock.
    let (session, plain) = match Session::accept_initial(&identity_clone, spk, opk, &message) {
        Ok(x) => x,
        Err(e) => { tracing::warn!(?e, "accept_initial failed"); return }
    };
    let plain_text = String::from_utf8_lossy(&plain).to_string();
    let time = hhmm_now();

    // Store + emit.
    let is_new_chat;
    {
        let mut s = match state.lock() { Ok(s) => s, Err(_) => return };
        if let Err(e) = s.vault.save_session(&from, &session) {
            tracing::warn!(?e, "save_session on accept");
        }
        s.sessions.insert(from.clone(), session);
        is_new_chat = !s.chats.contains_key(&from);
        // A group payload may ride as the very first X3DH message (that's how
        // a new member learns the group exists). Route it rather than opening
        // a 1:1 chat. The session we just stored stays — it's reused for
        // subsequent fan-out both ways.
        let is_group = plain_text.starts_with(GRPINFO_PREFIX) || plain_text.starts_with(GRPMSG_PREFIX);
        if is_group {
            drop(s);
            route_group_payload(app, &from, &plain_text);
            return;
        }
        if is_new_chat {
            let (letter, gradient) = gradient_for(&from);
            let summary = ChatSummary {
                id: from.clone(),
                name: from.clone(),
                letter, gradient,
                preview: plain_text.clone(),
                preview_is_typing: false, preview_by_me: false, has_lock: true,
                time: time.clone(),
                unread: 1, online: true, verified: false, channel: false, live: true,
            };
            s.chats.insert(from.clone(), ChatBook { summary, messages: Vec::new() });
            s.chat_order.retain(|id| id != &from);
            s.chat_order.insert(0, from.clone());
        }
        push_peer_message(&mut s, &from, plain_text.clone(), &time);
    }

    let _ = app.emit("aqua://incoming", IncomingEventPayload {
        peer_id: from.clone(),
        text: plain_text,
        time,
        is_new_chat,
    });
}

async fn on_incoming_envelope(
    app: &AppHandle,
    from: String,
    envelope: aqua_core::wire::Envelope,
) {
    let state: State<'_, StdMutex<AppState>> = app.state();
    // Decrypt and persist state. Keep this scope narrow so we don't
    // hold the state lock across the subsequent emit.
    let text = {
        let mut s = match state.lock() { Ok(s) => s, Err(_) => return };
        let plain = {
            let Some(session) = s.sessions.get_mut(&from) else {
                tracing::warn!(peer = %from, "envelope for unknown session");
                return;
            };
            match session.decrypt(&envelope) {
                Ok(p) => p,
                Err(e) => { tracing::warn!(?e, "decrypt failed"); return }
            }
        };
        if let Some(session) = s.sessions.get(&from) {
            if let Err(e) = s.vault.save_session(&from, session) {
                tracing::warn!(?e, "save_session after decrypt");
            }
        }
        String::from_utf8_lossy(&plain).to_string()
    };

    // Blocked people: everything is dropped except group traffic (their
    // messages in shared groups stay visible, like Telegram).
    let blocked = state.lock().map(|s| s.prefs.blocked.contains(&from)).unwrap_or(false);
    if blocked && !(text.starts_with(GRPMSG_PREFIX) || text.starts_with(GRPINFO_PREFIX) || text.starts_with(GRPCTL_PREFIX)
        || text.starts_with(GRPLEAVE_PREFIX) || text.starts_with(GRPSELF_PREFIX)) {
        return;
    }

    // Dispatch based on payload prefix.
    if let Some(signal_body) = text.strip_prefix(CALL_PREFIX) {
        // Call signals bypass the chat log entirely — the CallModal
        // on the frontend consumes them.
        match serde_json::from_str::<serde_json::Value>(signal_body) {
            Ok(signal) => {
                let _ = app.emit("aqua://call_signal", CallSignalEvent { from, signal });
            }
            Err(e) => tracing::warn!(?e, "bad call signal json"),
        }
        return;
    }

    // Poll votes — control messages, not chat entries. The frontend
    // tallies them per poll id.
    if let Some(body) = text.strip_prefix(POLLVOTE_PREFIX) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
            let _ = app.emit("aqua://poll_vote", serde_json::json!({ "from": from, "vote": v }));
        }
        return;
    }

    // Poll closed by its author.
    if let Some(body) = text.strip_prefix(POLLCLOSE_PREFIX) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
            let _ = app.emit("aqua://poll_close", serde_json::json!({ "from": from, "close": v }));
        }
        return;
    }

    // Read receipts — peer opened the chat and has seen our messages.
    if text.strip_prefix(SEEN_PREFIX).is_some() {
        let _ = app.emit("aqua://seen", serde_json::json!({ "from": from }));
        return;
    }

    // Presence — peer's current status / now-playing. Control message,
    // not a chat entry.
    if let Some(body) = text.strip_prefix(STATUS_PREFIX) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
            let _ = app.emit("aqua://presence", serde_json::json!({ "from": from, "presence": v }));
        }
        return;
    }

    // Reaction — peer added/removed an emoji reaction, keyed by a stable
    // content key both sides compute identically. Control message.
    if let Some(body) = text.strip_prefix(REACT_PREFIX) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
            let _ = app.emit("aqua://reaction", serde_json::json!({ "from": from, "react": v }));
        }
        return;
    }

    // Typing indicator — transient, never a chat entry.
    if text.strip_prefix(TYPING_PREFIX).is_some() {
        let _ = app.emit("aqua://typing", serde_json::json!({ "from": from }));
        return;
    }

    // Edit — replace a prior message from this peer whose text matches `old`.
    if let Some(body) = text.strip_prefix(EDIT_PREFIX) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
            let old = v.get("old").and_then(|x| x.as_str()).unwrap_or("");
            let new = v.get("new").and_then(|x| x.as_str()).unwrap_or("");
            if !old.is_empty() {
                let mut s = match state.lock() { Ok(s) => s, Err(_) => return };
                apply_edit(&mut s, &from, &from, old, new);
            }
            let _ = app.emit("aqua://chats_updated", ());
            let _ = app.emit("aqua://incoming", IncomingEventPayload {
                peer_id: from, text: String::new(), time: hhmm_now(), is_new_chat: false,
            });
        }
        return;
    }

    // Delete — remove a prior message from this peer whose text matches.
    if let Some(body) = text.strip_prefix(DEL_PREFIX) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
            let t = v.get("text").and_then(|x| x.as_str()).unwrap_or("");
            if !t.is_empty() {
                let mut s = match state.lock() { Ok(s) => s, Err(_) => return };
                apply_delete_ex(&mut s, &from, &from, t, true);
            }
            let _ = app.emit("aqua://chats_updated", ());
            let _ = app.emit("aqua://incoming", IncomingEventPayload {
                peer_id: from, text: String::new(), time: hhmm_now(), is_new_chat: false,
            });
        }
        return;
    }

    // Group metadata / group message — routed into the group's chat.
    if route_group_payload(app, &from, &text) {
        return;
    }

    // Normal (text or file) message path.
    let time = hhmm_now();
    {
        let mut s = match state.lock() { Ok(s) => s, Err(_) => return };
        push_peer_message(&mut s, &from, text.clone(), &time);
    }
    let _ = app.emit("aqua://incoming", IncomingEventPayload {
        peer_id: from,
        text,
        time,
        is_new_chat: false,
    });
}

// ---------- Local chat mutations ------------------------------------------

fn push_own_message(s: &mut AppState, peer_id: &str, msg: ChatMessage, _text: &str) {
    if let Some(book) = s.chats.get_mut(peer_id) {
        book.summary.preview = preview_for(&msg.kind);
        book.summary.preview_by_me = true;
        book.summary.preview_is_typing = false;
        book.summary.has_lock = book.summary.live;
        book.summary.time = hhmm_now();
        book.messages.push(msg);
    }
    if let Some(pos) = s.chat_order.iter().position(|id| id == peer_id) {
        if pos > 0 {
            s.chat_order.remove(pos);
            s.chat_order.insert(0, peer_id.to_string());
        }
    }
}

// ---------- Edit / delete message helpers ----------------------------------

/// Replace the text of the most recent message in `chat_id` authored by
/// `author` whose current text equals `old`. Edits Text and Reply kinds.
/// Does message author `from` match `author`? ("me" also matches our own
/// username — group echoes are stored under the username.)
fn author_is(s_user: &Option<String>, from: &str, author: &str) -> bool {
    from == author || (author == "me" && s_user.as_deref() == Some(from)) || (from == "me" && s_user.as_deref() == Some(author))
}

fn apply_edit(s: &mut AppState, chat_id: &str, author: &str, old: &str, new: &str) -> bool {
    let mut changed = false;
    let me = s.username.clone();
    let keep = s.prefs.keep_edits;
    if let Some(book) = s.chats.get_mut(chat_id) {
        for m in book.messages.iter_mut().rev() {
            if !author_is(&me, &m.from, author) { continue; }
            let hit = matches!(&m.kind, MessageKind::Text { text } if text == old)
                || matches!(&m.kind, MessageKind::Reply { text, .. } if text == old);
            if hit {
                if keep {
                    m.meta.get_or_insert_with(Default::default).edits.push(old.to_string());
                }
                match &mut m.kind {
                    MessageKind::Text { text } => *text = new.to_string(),
                    MessageKind::Reply { text, .. } => *text = new.to_string(),
                    _ => {}
                }
                changed = true;
                break;
            }
        }
        if changed {
            if let Some(last) = book.messages.last() {
                book.summary.preview = preview_for(&last.kind);
            }
        }
    }
    changed
}

/// Remove the most recent message in `chat_id` authored by `author` whose
/// current text equals `text_match`.
fn apply_delete(s: &mut AppState, chat_id: &str, author: &str, text_match: &str) -> bool {
    apply_delete_ex(s, chat_id, author, text_match, false)
}

/// `remote`: the deletion came from someone else — with "keep deleted" on,
/// the message stays, flagged as deleted.
fn apply_delete_ex(s: &mut AppState, chat_id: &str, author: &str, text_match: &str, remote: bool) -> bool {
    let me = s.username.clone();
    let keep = remote && s.prefs.keep_deleted;
    if let Some(book) = s.chats.get_mut(chat_id) {
        if let Some(pos) = book.messages.iter().rposition(|m| {
            author_is(&me, &m.from, author)
                && !m.meta.as_ref().map(|x| x.deleted).unwrap_or(false)
                && (matches!(&m.kind, MessageKind::Text { text } if text == text_match)
                    || matches!(&m.kind, MessageKind::Reply { text, .. } if text == text_match))
        }) {
            if keep {
                book.messages[pos].meta.get_or_insert_with(Default::default).deleted = true;
                return true;
            }
            book.messages.remove(pos);
            book.summary.preview = book
                .messages
                .last()
                .map(|m| preview_for(&m.kind))
                .unwrap_or_default();
            return true;
        }
    }
    false
}

/// Edit one of my own messages locally. The frontend separately relays an
/// `[edit:v1]` control to the peer for non-local chats.
#[tauri::command]
pub fn edit_message(
    state: State<'_, StdMutex<AppState>>,
    peer_id: String,
    old_text: String,
    new_text: String,
) -> Result<(), String> {
    let mut s = state.lock().map_err(|e| e.to_string())?;
    apply_edit(&mut s, &peer_id, "me", &old_text, &new_text);
    if peer_id == crate::state::SAVED_MESSAGES_ID {
        let data_dir = s.data_dir.clone();
        if let Some(book) = s.chats.get(&peer_id) {
            crate::state::save_saved_messages(&data_dir, &book.messages);
        }
    }
    Ok(())
}

/// Delete one of my own messages locally. The frontend separately relays a
/// `[del:v1]` control to the peer for non-local chats.
#[tauri::command]
pub fn delete_message(
    state: State<'_, StdMutex<AppState>>,
    peer_id: String,
    text: String,
) -> Result<(), String> {
    let mut s = state.lock().map_err(|e| e.to_string())?;
    apply_delete(&mut s, &peer_id, "me", &text);
    if peer_id == crate::state::SAVED_MESSAGES_ID {
        let data_dir = s.data_dir.clone();
        if let Some(book) = s.chats.get(&peer_id) {
            crate::state::save_saved_messages(&data_dir, &book.messages);
        }
    }
    Ok(())
}

/// Remove messages from this device by id ("delete for me") — works for
/// every kind (media, articles, polls…), unlike the text-matched
/// `delete_message`, which also tells the peer.
#[tauri::command]
pub fn delete_messages_local(
    state: State<'_, StdMutex<AppState>>,
    peer_id: String,
    ids: Vec<String>,
) -> Result<usize, String> {
    let mut s = state.lock().map_err(|e| e.to_string())?;
    let data_dir = s.data_dir.clone();
    let Some(book) = s.chats.get_mut(&peer_id) else { return Ok(0) };
    let before = book.messages.len();
    book.messages.retain(|m| !ids.contains(&m.id));
    let removed = before - book.messages.len();
    if removed > 0 {
        book.summary.preview = book.messages.last().map(|m| preview_for(&m.kind)).unwrap_or_default();
        if peer_id == crate::state::SAVED_MESSAGES_ID {
            crate::state::save_saved_messages(&data_dir, &book.messages);
        }
    }
    Ok(removed)
}

fn push_peer_message(s: &mut AppState, peer_id: &str, text: String, time: &str) {
    let kind = classify_kind(&text);
    let preview = preview_for(&kind);
    if let Some(book) = s.chats.get_mut(peer_id) {
        let msg_len = book.messages.len();
        book.messages.push(ChatMessage { meta: None,
            id: format!("in-{time}-{msg_len}"),
            from: peer_id.to_string(),
            time: time.to_string(),
            kind,
        });
        book.summary.preview = preview;
        book.summary.preview_by_me = false;
        book.summary.preview_is_typing = false;
        book.summary.has_lock = true;
        book.summary.time = time.to_string();
        book.summary.unread = book.summary.unread.saturating_add(1);
    }
    if let Some(pos) = s.chat_order.iter().position(|id| id == peer_id) {
        if pos > 0 {
            s.chat_order.remove(pos);
            s.chat_order.insert(0, peer_id.to_string());
        }
    }
}
