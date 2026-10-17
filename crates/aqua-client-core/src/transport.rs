//! WebSocket + HTTP transport to `aqua-server`.
//!
//! Responsibilities:
//!
//! * Sign in with the server via the challenge → verify flow and cache
//!   the resulting bearer token.
//! * Publish and fetch prekey bundles over HTTP.
//! * Hold a live WebSocket to relay [`InitialMessage`]s and
//!   [`Envelope`]s to peers and hand incoming ones back to the caller
//!   as [`Incoming`] events on an mpsc channel.
//!
//! The transport does not run any crypto. Encryption / decryption
//! happens in the caller against the vault-backed `Session` for each
//! peer.

use aqua_core::{
    keys::{IdentityKeyPair, PreKeyBundle},
    wire::{
        auth::{ChallengeRequest, ChallengeResponse, VerifyRequest, VerifyResponse},
        BundleUpload, Envelope, InitialMessage, PayloadKind, RelayMessage,
    },
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use futures_util::{SinkExt, StreamExt};
use std::sync::RwLock;
use thiserror::Error;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

// ---------- Optional outbound proxy --------------------------------------
//
// "Connect through a server": when set, every HTTP request and the relay
// WebSocket are routed through a local SOCKS5 proxy (e.g. one exposed by an
// OpenFlux tunnel). This is a plain client-side proxy setting — Aquamarine
// does not run or know anything about the tunnel itself.

static PROXY: RwLock<Option<String>> = RwLock::new(None);

/// Set (or clear) the SOCKS5 proxy URL, e.g. `socks5://127.0.0.1:1080`.
/// `socks5h://` resolves DNS at the proxy. Pass `None`/empty to go direct.
pub fn set_proxy(url: Option<String>) {
    let v = url.and_then(|u| {
        let u = u.trim().to_string();
        if u.is_empty() { None } else { Some(u) }
    });
    if let Ok(mut g) = PROXY.write() {
        *g = v;
    }
}

/// The currently configured proxy URL, if any.
pub fn proxy() -> Option<String> {
    PROXY.read().ok().and_then(|g| g.clone())
}

/// A reqwest client that honours the configured proxy.
pub fn http_client() -> Result<reqwest::Client, TransportError> {
    let mut b = reqwest::Client::builder()
        .user_agent(concat!("aqua-client-core/", env!("CARGO_PKG_VERSION")));
    if let Some(p) = proxy() {
        b = b.proxy(reqwest::Proxy::all(&p).map_err(TransportError::Http)?);
    } else {
        // Direct unless the user set a proxy in Settings: never pick one up
        // from the environment (VPN clients like to set HTTP(S)_PROXY).
        b = b.no_proxy();
    }
    Ok(b.build()?)
}

/// "error sending request" alone says nothing; append the causes
/// (connection refused / timed out / permission denied …).
fn error_chain(e: &reqwest::Error) -> String {
    let mut out = e.to_string();
    let mut src = std::error::Error::source(e);
    while let Some(s) = src {
        let msg = s.to_string();
        if !out.contains(&msg) {
            out.push_str(": ");
            out.push_str(&msg);
        }
        src = s.source();
    }
    out
}

// ---------- Errors --------------------------------------------------------

#[derive(Debug, Error)]
pub enum TransportError {
    #[error("HTTP: {}", error_chain(.0))]
    Http(#[from] reqwest::Error),
    #[error("WebSocket: {0}")]
    Ws(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("server responded {status}: {body}")]
    Server { status: u16, body: String },
    #[error("bad server payload: {0}")]
    BadPayload(String),
    #[error("crypto: {0}")]
    Crypto(#[from] aqua_core::CryptoError),
    #[error("codec: {0}")]
    Codec(#[from] bincode::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

// ---------- Directory types ------------------------------------------------

/// A public profile card from the server's user directory.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProfileCard {
    pub username: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub about: String,
    #[serde(default)]
    pub phone: String,
    #[serde(default)]
    pub is_bot: bool,
    #[serde(default)]
    pub created_at: i64,
    /// Public extras (banner colours, status, pronouns, …). `{}` on old servers.
    #[serde(default)]
    pub extra: serde_json::Value,
    /// Avatar image version on the server (0 = none).
    #[serde(default)]
    pub avatar_ver: i64,
    /// Banner image version on the server (0 = none).
    #[serde(default)]
    pub banner_ver: i64,
    /// "Favourite track" audio version (0 = none; older servers omit it).
    #[serde(default)]
    pub music_ver: i64,
}

/// Request an email verification code (no auth required). Returns the
/// code itself only when the server is in dev mode.
pub async fn request_email_code(server_url: &str, email: &str) -> Result<Option<String>, TransportError> {
    #[derive(serde::Deserialize)]
    struct Resp { #[serde(default)] dev_code: Option<String> }
    let http = http_client()?;
    let url = format!("{}/v1/auth/email/request", server_url.trim_end_matches('/'));
    let r = http.post(&url).json(&serde_json::json!({ "email": email })).send().await?;
    if !r.status().is_success() {
        return Err(TransportError::Server { status: r.status().as_u16(), body: r.text().await.unwrap_or_default() });
    }
    Ok(r.json::<Resp>().await?.dev_code)
}

/// Verify an email code (no auth required). Returns a registration
/// ticket on success.
pub async fn verify_email_code(server_url: &str, email: &str, code: &str) -> Result<String, TransportError> {
    #[derive(serde::Deserialize)]
    struct Resp { ticket: String }
    let http = http_client()?;
    let url = format!("{}/v1/auth/email/verify", server_url.trim_end_matches('/'));
    let r = http.post(&url).json(&serde_json::json!({ "email": email, "code": code })).send().await?;
    if !r.status().is_success() {
        return Err(TransportError::Server { status: r.status().as_u16(), body: r.text().await.unwrap_or_default() });
    }
    Ok(r.json::<Resp>().await?.ticket)
}

// ---------- Incoming events ----------------------------------------------

#[derive(Debug)]
pub enum Incoming {
    /// A first message from a new peer. Caller must run
    /// `Session::accept_initial` to produce a live session and the
    /// first plaintext.
    Initial { from: String, message: InitialMessage },
    /// A subsequent message on an existing session.
    Envelope { from: String, envelope: Envelope },
    /// An un-encrypted message from a bot.
    Plaintext { from: String, text: String },
    /// The socket closed cleanly.
    Closed,
    /// A transport-level error the caller may want to surface.
    Error(String),
}

// ---------- Transport -----------------------------------------------------

/// Handle to a live transport session. Drop it to disconnect.
pub struct Transport {
    server_url: String,
    username: String,
    /// Bearer token + when it was issued. The server's tokens live one
    /// hour; [`Transport::bearer`] signs in again before that.
    token: std::sync::Mutex<(String, std::time::Instant)>,
    /// Kept to re-sign the auth challenge when the token gets old.
    identity: IdentityKeyPair,
    http: reqwest::Client,
    ws_out: mpsc::UnboundedSender<RelayMessage>,
    /// Kept alive so the WS read/write tasks stay running.
    _write_task: tokio::task::JoinHandle<()>,
    _read_task: tokio::task::JoinHandle<()>,
}

impl std::fmt::Debug for Transport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Transport")
            .field("server_url", &self.server_url)
            .field("username", &self.username)
            .finish_non_exhaustive()
    }
}

impl Transport {
    /// Runs the full sign-in flow, then opens the relay WebSocket.
    /// Returns the transport handle plus an mpsc receiver for
    /// [`Incoming`] events.
    pub async fn connect(
        server_url: &str,
        username: &str,
        identity: &IdentityKeyPair,
    ) -> Result<(Self, mpsc::UnboundedReceiver<Incoming>), TransportError> {
        let http = http_client()?;

        // ---- 1. Sign in ----
        let token = sign_in(&http, server_url, identity).await?;

        // ---- 2. Open the WS (direct, or through the SOCKS5 proxy) ----
        let ws_url = build_ws_url(server_url, username, &token)?;
        let ws = open_ws(&ws_url).await?;
        let (mut ws_write, mut ws_read) = ws.split();

        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<RelayMessage>();
        let (in_tx, in_rx) = mpsc::unbounded_channel::<Incoming>();

        // Writer task: forwards outgoing RelayMessages onto the socket
        // as binary JSON. Ends when the sender is dropped.
        let write_task = tokio::spawn(async move {
            while let Some(msg) = out_rx.recv().await {
                match serde_json::to_vec(&msg) {
                    Ok(bytes) => {
                        if let Err(e) = ws_write.send(Message::Binary(bytes.into())).await {
                            tracing::warn!(?e, "ws write failed; giving up on this connection");
                            break;
                        }
                    }
                    Err(e) => tracing::warn!(?e, "encode outgoing"),
                }
            }
            let _ = ws_write.close().await;
        });

        // Reader task: parses incoming frames, dispatches decoded
        // Initial/Envelope events. Emits Incoming::Closed / Error too.
        let in_tx_r = in_tx.clone();
        let read_task = tokio::spawn(async move {
            while let Some(frame) = ws_read.next().await {
                match frame {
                    Ok(Message::Binary(bytes)) => {
                        dispatch_frame(&in_tx_r, &bytes);
                    }
                    Ok(Message::Text(text)) => {
                        dispatch_frame(&in_tx_r, text.as_bytes());
                    }
                    Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => {}
                    Ok(Message::Close(_)) => {
                        let _ = in_tx_r.send(Incoming::Closed);
                        break;
                    }
                    Ok(_) => {}
                    Err(e) => {
                        let _ = in_tx_r.send(Incoming::Error(format!("ws: {e}")));
                        break;
                    }
                }
            }
        });

        Ok((
            Self {
                server_url: server_url.to_string(),
                username: username.to_string(),
                token: std::sync::Mutex::new((token, std::time::Instant::now())),
                identity: identity.clone(),
                http,
                ws_out: out_tx,
                _write_task: write_task,
                _read_task: read_task,
            },
            in_rx,
        ))
    }

    pub fn username(&self) -> &str { &self.username }
    /// The current bearer token (may be close to expiry — prefer [`Self::bearer`]).
    pub fn token(&self) -> String { self.token.lock().map(|t| t.0.clone()).unwrap_or_default() }

    /// A bearer token that is valid for a while yet: tokens older than
    /// 45 minutes (they expire after 60) are renewed by signing a fresh
    /// challenge. Without this every request failed with "token has
    /// expired" once the app had been open for an hour.
    pub async fn bearer(&self) -> Result<String, TransportError> {
        const RENEW_AFTER: std::time::Duration = std::time::Duration::from_secs(45 * 60);
        if let Ok(t) = self.token.lock() {
            if t.1.elapsed() < RENEW_AFTER {
                return Ok(t.0.clone());
            }
        }
        let fresh = sign_in(&self.http, &self.server_url, &self.identity).await?;
        if let Ok(mut t) = self.token.lock() {
            *t = (fresh.clone(), std::time::Instant::now());
        }
        Ok(fresh)
    }

    pub async fn upload_bundle(&self, bundle: &BundleUpload) -> Result<(), TransportError> {
        let url = format!("{}/v1/bundle/{}", self.server_url.trim_end_matches('/'), self.username);
        let r = self
            .http
            .post(&url)
            .bearer_auth(self.bearer().await?)
            .json(bundle)
            .send()
            .await?;
        if !r.status().is_success() {
            return Err(TransportError::Server {
                status: r.status().as_u16(),
                body: r.text().await.unwrap_or_default(),
            });
        }
        Ok(())
    }

    pub async fn fetch_bundle(&self, peer: &str) -> Result<PreKeyBundle, TransportError> {
        let url = format!("{}/v1/bundle/{}", self.server_url.trim_end_matches('/'), peer);
        let r = self.http.get(&url).send().await?;
        if !r.status().is_success() {
            return Err(TransportError::Server {
                status: r.status().as_u16(),
                body: r.text().await.unwrap_or_default(),
            });
        }
        Ok(r.json::<PreKeyBundle>().await?)
    }

    /// Upload an encrypted blob to the content-addressed store. Server
    /// checks that the SHA-256 of the body matches `cid`.
    pub async fn upload_blob(&self, cid: &str, bytes: Vec<u8>) -> Result<(), TransportError> {
        self.upload_blob_ext(cid, bytes, false).await
    }

    /// `keep`: ask the relay to hold it long-term (group photos, ≤ 4 MiB)
    /// instead of in the hours-long delivery cache.
    pub async fn upload_blob_ext(&self, cid: &str, bytes: Vec<u8>, keep: bool) -> Result<(), TransportError> {
        let url = format!("{}/v1/blob/{}{}", self.server_url.trim_end_matches('/'), cid, if keep { "?keep=1" } else { "" });
        let r = self.http.post(&url).bearer_auth(self.bearer().await?).body(bytes).send().await?;
        if !r.status().is_success() {
            return Err(TransportError::Server {
                status: r.status().as_u16(),
                body: r.text().await.unwrap_or_default(),
            });
        }
        Ok(())
    }

    /// Download an encrypted blob by CID. The caller is expected to
    /// re-verify the SHA-256 before trusting the bytes.
    pub async fn download_blob(&self, cid: &str) -> Result<Vec<u8>, TransportError> {
        let url = format!("{}/v1/blob/{}", self.server_url.trim_end_matches('/'), cid);
        let r = self.http.get(&url).send().await?;
        if !r.status().is_success() {
            return Err(TransportError::Server {
                status: r.status().as_u16(),
                body: r.text().await.unwrap_or_default(),
            });
        }
        Ok(r.bytes().await?.to_vec())
    }

    // ---- User directory (public profile cards + search) ----

    /// Search the server's user directory by username / display name.
    pub async fn search_users(&self, query: &str) -> Result<Vec<ProfileCard>, TransportError> {
        let url = format!("{}/v1/users/search", self.server_url.trim_end_matches('/'));
        let r = self.http.get(&url).query(&[("q", query)]).send().await?;
        if !r.status().is_success() {
            return Err(TransportError::Server { status: r.status().as_u16(), body: r.text().await.unwrap_or_default() });
        }
        Ok(r.json::<Vec<ProfileCard>>().await?)
    }

    /// Fetch a single user's public profile card.
    pub async fn get_profile(&self, username: &str) -> Result<Option<ProfileCard>, TransportError> {
        let url = format!("{}/v1/users/{}/profile", self.server_url.trim_end_matches('/'), username);
        let r = self.http.get(&url).send().await?;
        if r.status().as_u16() == 404 { return Ok(None); }
        if !r.status().is_success() {
            return Err(TransportError::Server { status: r.status().as_u16(), body: r.text().await.unwrap_or_default() });
        }
        Ok(Some(r.json::<ProfileCard>().await?))
    }

    /// Register an account over HTTP WITHOUT opening a relay socket.
    ///
    /// Signs in (challenge → verify) to obtain a bearer token, then calls
    /// `/v1/users/register`. This is the first step of onboarding: with
    /// mandatory registration the server refuses a bundle upload or WS until
    /// the username exists in the directory, so the client must register
    /// before it can `connect`.
    #[allow(clippy::too_many_arguments)]
    pub async fn register_account(
        server_url: &str,
        identity: &IdentityKeyPair,
        ticket: &str,
        username: &str,
        display_name: &str,
        about: &str,
        phone: &str,
        password: &str,
    ) -> Result<(), TransportError> {
        let http = http_client()?;
        let token = sign_in(&http, server_url, identity).await?;
        let url = format!("{}/v1/users/register", server_url.trim_end_matches('/'));
        let body = serde_json::json!({
            "ticket": ticket, "username": username,
            "display_name": display_name, "about": about, "phone": phone,
            "password": password,
        });
        let r = http.post(&url).bearer_auth(&token).json(&body).send().await?;
        if !r.status().is_success() {
            return Err(TransportError::Server {
                status: r.status().as_u16(),
                body: r.text().await.unwrap_or_default(),
            });
        }
        Ok(())
    }

    /// Sign in on this device with an email ticket + password; the server
    /// moves the account to this identity. Returns the username.
    pub async fn login_account(
        server_url: &str,
        identity: &IdentityKeyPair,
        ticket: &str,
        password: &str,
    ) -> Result<String, TransportError> {
        #[derive(serde::Deserialize)]
        struct Resp { username: String }
        let http = http_client()?;
        let token = sign_in(&http, server_url, identity).await?;
        let url = format!("{}/v1/users/login", server_url.trim_end_matches('/'));
        let body = serde_json::json!({ "ticket": ticket, "password": password });
        let r = http.post(&url).bearer_auth(&token).json(&body).send().await?;
        if !r.status().is_success() {
            return Err(TransportError::Server { status: r.status().as_u16(), body: r.text().await.unwrap_or_default() });
        }
        Ok(r.json::<Resp>().await?.username)
    }

    /// Set the account password (already turned into [`crate::backup::auth_secret`]).
    pub async fn set_password_with(
        server_url: &str,
        identity: &IdentityKeyPair,
        username: &str,
        secret: &str,
    ) -> Result<(), TransportError> {
        let http = http_client()?;
        let token = sign_in(&http, server_url, identity).await?;
        let url = format!("{}/v1/users/{}/password", server_url.trim_end_matches('/'), username);
        let r = http.post(&url).bearer_auth(&token).json(&serde_json::json!({ "password": secret })).send().await?;
        if !r.status().is_success() {
            return Err(TransportError::Server { status: r.status().as_u16(), body: r.text().await.unwrap_or_default() });
        }
        Ok(())
    }

    /// New device: download the encrypted backup with an e-mail ticket +
    /// password secret. `Ok(None)` = the account has no backup.
    pub async fn fetch_backup(server_url: &str, ticket: &str, secret: &str) -> Result<Option<Vec<u8>>, TransportError> {
        let http = http_client()?;
        let url = format!("{}/v1/backup/fetch", server_url.trim_end_matches('/'));
        let r = http.post(&url).json(&serde_json::json!({ "ticket": ticket, "password": secret })).send().await?;
        if r.status().as_u16() == 404 {
            return Ok(None);
        }
        if !r.status().is_success() {
            return Err(TransportError::Server { status: r.status().as_u16(), body: r.text().await.unwrap_or_default() });
        }
        Ok(Some(r.bytes().await?.to_vec()))
    }

    /// Upload this account's encrypted backup (replaces the previous one).
    pub async fn put_backup(&self, blob: Vec<u8>) -> Result<serde_json::Value, TransportError> {
        let url = format!("{}/v1/backup", self.server_url.trim_end_matches('/'));
        let r = self.http.put(&url).bearer_auth(self.bearer().await?).body(blob).send().await?;
        if !r.status().is_success() {
            return Err(TransportError::Server { status: r.status().as_u16(), body: r.text().await.unwrap_or_default() });
        }
        Ok(r.json().await?)
    }

    /// `{ exists, size, updated_at }` of the stored backup.
    pub async fn backup_meta(&self) -> Result<serde_json::Value, TransportError> {
        let url = format!("{}/v1/backup", self.server_url.trim_end_matches('/'));
        let r = self.http.get(&url).bearer_auth(self.bearer().await?).send().await?;
        if !r.status().is_success() {
            return Err(TransportError::Server { status: r.status().as_u16(), body: r.text().await.unwrap_or_default() });
        }
        Ok(r.json().await?)
    }

    pub async fn delete_backup(&self) -> Result<(), TransportError> {
        let url = format!("{}/v1/backup", self.server_url.trim_end_matches('/'));
        let r = self.http.delete(&url).bearer_auth(self.bearer().await?).send().await?;
        if !r.status().is_success() {
            return Err(TransportError::Server { status: r.status().as_u16(), body: r.text().await.unwrap_or_default() });
        }
        Ok(())
    }

    /// Register `@username` to this identity, gated by an email ticket.
    pub async fn register_user(
        &self,
        ticket: &str,
        username: &str,
        display_name: &str,
        about: &str,
        phone: &str,
    ) -> Result<(), TransportError> {
        let url = format!("{}/v1/users/register", self.server_url.trim_end_matches('/'));
        let body = serde_json::json!({
            "ticket": ticket, "username": username,
            "display_name": display_name, "about": about, "phone": phone,
        });
        let r = self.http.post(&url).bearer_auth(self.bearer().await?).json(&body).send().await?;
        if !r.status().is_success() {
            return Err(TransportError::Server { status: r.status().as_u16(), body: r.text().await.unwrap_or_default() });
        }
        Ok(())
    }

    /// Update the profile card for the username this transport is signed
    /// in as.
    pub async fn update_profile(&self, display_name: &str, about: &str, phone: &str) -> Result<(), TransportError> {
        let url = format!("{}/v1/users/{}/profile", self.server_url.trim_end_matches('/'), self.username);
        let body = serde_json::json!({ "display_name": display_name, "about": about, "phone": phone });
        let r = self.http.post(&url).bearer_auth(self.bearer().await?).json(&body).send().await?;
        if !r.status().is_success() {
            return Err(TransportError::Server { status: r.status().as_u16(), body: r.text().await.unwrap_or_default() });
        }
        Ok(())
    }

    /// Update the profile card including the public `extra` object.
    pub async fn update_profile_full(
        &self,
        display_name: &str,
        about: &str,
        phone: &str,
        extra: &serde_json::Value,
    ) -> Result<(), TransportError> {
        let url = format!("{}/v1/users/{}/profile", self.server_url.trim_end_matches('/'), self.username);
        let body = serde_json::json!({ "display_name": display_name, "about": about, "phone": phone, "extra": extra });
        let r = self.http.post(&url).bearer_auth(self.bearer().await?).json(&body).send().await?;
        if !r.status().is_success() {
            return Err(TransportError::Server { status: r.status().as_u16(), body: r.text().await.unwrap_or_default() });
        }
        Ok(())
    }

    /// Upload our avatar / banner (`kind` = "avatar" | "banner"). Returns the
    /// new server-side version.
    pub async fn upload_profile_media(&self, kind: &str, mime: &str, bytes: Vec<u8>) -> Result<i64, TransportError> {
        #[derive(serde::Deserialize)]
        struct Resp { ver: i64 }
        let url = format!("{}/v1/users/{}/media/{}", self.server_url.trim_end_matches('/'), self.username, kind);
        let r = self.http.post(&url).bearer_auth(self.bearer().await?)
            .header(reqwest::header::CONTENT_TYPE, mime)
            .body(bytes).send().await?;
        if !r.status().is_success() {
            return Err(TransportError::Server { status: r.status().as_u16(), body: r.text().await.unwrap_or_default() });
        }
        Ok(r.json::<Resp>().await?.ver)
    }

    /// Remove our avatar / banner.
    pub async fn clear_profile_media(&self, kind: &str) -> Result<(), TransportError> {
        let url = format!("{}/v1/users/{}/media/{}", self.server_url.trim_end_matches('/'), self.username, kind);
        let r = self.http.delete(&url).bearer_auth(self.bearer().await?).send().await?;
        if !r.status().is_success() && r.status().as_u16() != 404 {
            return Err(TransportError::Server { status: r.status().as_u16(), body: r.text().await.unwrap_or_default() });
        }
        Ok(())
    }

    /// Who may download our avatar / banner (profile-photo privacy).
    pub async fn set_media_acl(&self, everybody: bool, allow: &[String], deny: &[String]) -> Result<(), TransportError> {
        let url = format!("{}/v1/users/{}/media_acl", self.server_url.trim_end_matches('/'), self.username);
        let body = serde_json::json!({ "everybody": everybody, "allow": allow, "deny": deny });
        let r = self.http.post(&url).bearer_auth(self.bearer().await?).json(&body).send().await?;
        if !r.status().is_success() {
            return Err(TransportError::Server { status: r.status().as_u16(), body: r.text().await.unwrap_or_default() });
        }
        Ok(())
    }

    /// Download anyone's avatar / banner: `(mime, bytes)`, or None if unset.
    pub async fn fetch_profile_media(&self, username: &str, kind: &str, ver: i64) -> Result<Option<(String, Vec<u8>)>, TransportError> {
        let url = format!("{}/v1/users/{}/media/{}?v={}", self.server_url.trim_end_matches('/'), username, kind, ver);
        // Authenticated so a profile-photo privacy list can include us.
        let r = self.http.get(&url).bearer_auth(self.bearer().await?).send().await?;
        if r.status().as_u16() == 404 || r.status().as_u16() == 403 { return Ok(None); }
        if !r.status().is_success() {
            return Err(TransportError::Server { status: r.status().as_u16(), body: r.text().await.unwrap_or_default() });
        }
        let mime = r.headers().get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()).unwrap_or("application/octet-stream").to_string();
        Ok(Some((mime, r.bytes().await?.to_vec())))
    }

    // ---- Bots ----

    /// Register a bot owned by this identity. Returns (username, token).
    pub async fn register_bot(&self, username: &str, display_name: &str) -> Result<(String, String), TransportError> {
        #[derive(serde::Deserialize)]
        struct Resp { username: String, token: String }
        let url = format!("{}/v1/bot/register", self.server_url.trim_end_matches('/'));
        let body = serde_json::json!({ "username": username, "display_name": display_name });
        let r = self.http.post(&url).bearer_auth(self.bearer().await?).json(&body).send().await?;
        if !r.status().is_success() {
            return Err(TransportError::Server { status: r.status().as_u16(), body: r.text().await.unwrap_or_default() });
        }
        let resp: Resp = r.json().await?;
        Ok((resp.username, resp.token))
    }

    // ---- Sticker / emoji packs hosted by the relay ----

    /// `GET /v1/packs` — `{ telegram: bool, packs: [...] }` (raw JSON).
    pub async fn packs_index(&self) -> Result<serde_json::Value, TransportError> {
        let url = format!("{}/v1/packs", self.server_url.trim_end_matches('/'));
        let r = self.http.get(&url).send().await?;
        if !r.status().is_success() {
            return Err(TransportError::Server { status: r.status().as_u16(), body: r.text().await.unwrap_or_default() });
        }
        Ok(r.json().await?)
    }

    /// Ask the relay to import a Telegram pack by link; returns its manifest.
    pub async fn import_pack(&self, link: &str) -> Result<serde_json::Value, TransportError> {
        let url = format!("{}/v1/packs/import", self.server_url.trim_end_matches('/'));
        let r = self.http.post(&url).bearer_auth(self.bearer().await?).json(&serde_json::json!({ "link": link })).send().await?;
        if !r.status().is_success() {
            return Err(TransportError::Server { status: r.status().as_u16(), body: r.text().await.unwrap_or_default() });
        }
        Ok(r.json().await?)
    }

    /// Ask the relay to check a pack against Telegram now; its new manifest.
    pub async fn refresh_pack(&self, name: &str) -> Result<serde_json::Value, TransportError> {
        let url = format!("{}/v1/packs/{}/refresh", self.server_url.trim_end_matches('/'), name);
        let r = self.http.post(&url).bearer_auth(self.bearer().await?).send().await?;
        if !r.status().is_success() {
            return Err(TransportError::Server { status: r.status().as_u16(), body: r.text().await.unwrap_or_default() });
        }
        Ok(r.json().await?)
    }

    /// Manifest of a pack stored on the relay (None if it has no such pack).
    pub async fn pack_manifest(&self, name: &str) -> Result<Option<serde_json::Value>, TransportError> {
        let url = format!("{}/v1/packs/{}", self.server_url.trim_end_matches('/'), name);
        let r = self.http.get(&url).send().await?;
        if r.status().as_u16() == 404 {
            return Ok(None);
        }
        if !r.status().is_success() {
            return Err(TransportError::Server { status: r.status().as_u16(), body: r.text().await.unwrap_or_default() });
        }
        Ok(Some(r.json().await?))
    }

    /// One file of a relay-hosted pack.
    pub async fn pack_file(&self, name: &str, file: &str) -> Result<Vec<u8>, TransportError> {
        let enc: String = file
            .bytes()
            .map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
            .collect();
        let url = format!("{}/v1/packs/{}/{}", self.server_url.trim_end_matches('/'), name, enc);
        let r = self.http.get(&url).send().await?;
        if !r.status().is_success() {
            return Err(TransportError::Server { status: r.status().as_u16(), body: r.text().await.unwrap_or_default() });
        }
        Ok(r.bytes().await?.to_vec())
    }

    /// Search the relay's game catalogue: [(id, name, has_icon)].
    pub async fn games_search(&self, q: &str) -> Result<Vec<(String, String, bool)>, TransportError> {
        #[derive(serde::Deserialize)]
        struct Hit { id: String, name: String, #[serde(default)] has_icon: bool }
        let url = format!("{}/v1/games/search", self.server_url.trim_end_matches('/'));
        let r = self.http.get(&url).query(&[("q", q)]).send().await?;
        if !r.status().is_success() {
            return Err(TransportError::Server { status: r.status().as_u16(), body: r.text().await.unwrap_or_default() });
        }
        let v: Vec<Hit> = r.json().await?;
        Ok(v.into_iter().map(|h| (h.id, h.name, h.has_icon)).collect())
    }

    /// PNG icon of a game from the relay (None when it has none).
    pub async fn game_icon(&self, id: &str) -> Result<Option<Vec<u8>>, TransportError> {
        let url = format!("{}/v1/games/icon/{}", self.server_url.trim_end_matches('/'), id);
        let r = self.http.get(&url).send().await?;
        if r.status().as_u16() == 404 {
            return Ok(None);
        }
        if !r.status().is_success() {
            return Err(TransportError::Server { status: r.status().as_u16(), body: r.text().await.unwrap_or_default() });
        }
        Ok(Some(r.bytes().await?.to_vec()))
    }

    /// List bots owned by this identity. Returns (username, token, display
    /// name, token hint). Current relays keep only a hash, so `token` is
    /// empty and `hint` holds its last 4 characters.
    pub async fn list_bots(&self) -> Result<Vec<(String, String, String, String)>, TransportError> {
        #[derive(serde::Deserialize)]
        struct Row { username: String, #[serde(default)] token: String, #[serde(default)] display_name: String, #[serde(default)] hint: String }
        let url = format!("{}/v1/bot/list", self.server_url.trim_end_matches('/'));
        let r = self.http.get(&url).bearer_auth(self.bearer().await?).send().await?;
        if !r.status().is_success() {
            return Err(TransportError::Server { status: r.status().as_u16(), body: r.text().await.unwrap_or_default() });
        }
        let rows: Vec<Row> = r.json().await?;
        Ok(rows.into_iter().map(|b| (b.username, b.token, b.display_name, b.hint)).collect())
    }

    /// Delete one of this identity's bots (its token stops working).
    pub async fn delete_bot(&self, username: &str) -> Result<(), TransportError> {
        let url = format!("{}/v1/bot/{}", self.server_url.trim_end_matches('/'), username.trim().to_lowercase());
        let r = self.http.delete(&url).bearer_auth(self.bearer().await?).send().await?;
        if !r.status().is_success() {
            return Err(TransportError::Server { status: r.status().as_u16(), body: r.text().await.unwrap_or_default() });
        }
        Ok(())
    }

    pub fn send_initial(&self, to: &str, initial: &InitialMessage) -> Result<(), TransportError> {
        let payload = bincode::serialize(initial)?;
        self.enqueue(to, PayloadKind::Initial, payload)
    }

    pub fn send_envelope(&self, to: &str, envelope: &Envelope) -> Result<(), TransportError> {
        let payload = bincode::serialize(envelope)?;
        self.enqueue(to, PayloadKind::Envelope, payload)
    }

    /// Send an un-encrypted text message. Only used to talk to bots,
    /// which can't do X3DH. The UI marks such chats as not E2EE.
    pub fn send_plaintext(&self, to: &str, text: &str) -> Result<(), TransportError> {
        self.enqueue(to, PayloadKind::Plaintext, text.as_bytes().to_vec())
    }

    fn enqueue(&self, to: &str, kind: PayloadKind, payload: Vec<u8>) -> Result<(), TransportError> {
        let msg = RelayMessage {
            from: String::new(), // server stamps this
            to: to.to_string(),
            payload,
            kind,
        };
        self.ws_out
            .send(msg)
            .map_err(|_| TransportError::BadPayload("transport disconnected".into()))
    }
}

// ---------- Helpers -------------------------------------------------------

async fn sign_in(
    http: &reqwest::Client,
    server_url: &str,
    identity: &IdentityKeyPair,
) -> Result<String, TransportError> {
    let base = server_url.trim_end_matches('/');
    let pub_id = identity.public();
    let dh_pub_b64 = URL_SAFE_NO_PAD.encode(pub_id.dh_pub.as_bytes());

    // 1) Challenge.
    let chal: ChallengeResponse = http
        .post(format!("{base}/v1/auth/challenge"))
        .json(&ChallengeRequest { identity_dh_pub: dh_pub_b64 })
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    // 2) Sign the nonce bytes with the identity's Ed25519 key.
    let nonce_bytes = URL_SAFE_NO_PAD
        .decode(&chal.nonce)
        .map_err(|e| TransportError::BadPayload(format!("bad nonce base64: {e}")))?;
    let sig = identity.sign.sign(&nonce_bytes);

    // 3) Verify.
    let verify: VerifyResponse = http
        .post(format!("{base}/v1/auth/verify"))
        .json(&VerifyRequest {
            identity: pub_id,
            nonce: chal.nonce,
            signature: URL_SAFE_NO_PAD.encode(sig),
        })
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(verify.token)
}

/// Any byte stream we can run a WebSocket over (a plain TCP socket, or a
/// TCP socket tunnelled through the SOCKS5 proxy). Boxed so both paths share
/// one concrete `WebSocketStream` type.
trait Rw: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> Rw for T {}

type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<Box<dyn Rw>>>;

/// (host, port) to dial for a ws:// or wss:// URL.
fn ws_target(ws_url: &str) -> Result<(String, u16), TransportError> {
    let (scheme_default, rest) = if let Some(r) = ws_url.strip_prefix("wss://") {
        (443u16, r)
    } else if let Some(r) = ws_url.strip_prefix("ws://") {
        (80u16, r)
    } else {
        return Err(TransportError::BadPayload(format!("bad ws url {ws_url}")));
    };
    let authority = rest.split(['/', '?']).next().unwrap_or(rest);
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (h.to_string(), p.parse().unwrap_or(scheme_default)),
        None => (authority.to_string(), scheme_default),
    };
    if host.is_empty() {
        return Err(TransportError::BadPayload(format!("no host in {ws_url}")));
    }
    Ok((host, port))
}

/// Open the relay WebSocket, directly or through the configured SOCKS5 proxy.
/// TLS (for wss://) is applied by `client_async_tls` either way.
async fn open_ws(ws_url: &str) -> Result<WsStream, TransportError> {
    let (host, port) = ws_target(ws_url)?;
    let stream: Box<dyn Rw> = match proxy() {
        Some(p) => {
            let proxy_addr = p
                .trim_start_matches("socks5h://")
                .trim_start_matches("socks5://");
            let s = tokio_socks::tcp::Socks5Stream::connect(proxy_addr, (host.as_str(), port))
                .await
                .map_err(|e| TransportError::BadPayload(format!("proxy connect: {e}")))?;
            Box::new(s)
        }
        None => {
            let s = tokio::net::TcpStream::connect((host.as_str(), port))
                .await
                .map_err(|e| TransportError::BadPayload(format!("connect: {e}")))?;
            Box::new(s)
        }
    };
    let (ws, _resp) = tokio_tungstenite::client_async_tls(ws_url, stream).await?;
    Ok(ws)
}

fn build_ws_url(server_url: &str, username: &str, token: &str) -> Result<String, TransportError> {
    let base = server_url.trim_end_matches('/');
    // Flip http://... → ws://... / https://... → wss://...
    let ws_base = if let Some(rest) = base.strip_prefix("http://") {
        format!("ws://{rest}")
    } else if let Some(rest) = base.strip_prefix("https://") {
        format!("wss://{rest}")
    } else {
        return Err(TransportError::BadPayload(format!(
            "server URL must start with http:// or https://, got {base}"
        )));
    };
    // Percent-encode the token (base64url is URL-safe already, but be defensive).
    Ok(format!("{ws_base}/v1/ws?user={username}&token={token}"))
}

fn dispatch_frame(tx: &mpsc::UnboundedSender<Incoming>, bytes: &[u8]) {
    let msg: RelayMessage = match serde_json::from_slice(bytes) {
        Ok(m) => m,
        Err(e) => {
            let _ = tx.send(Incoming::Error(format!("decode relay: {e}")));
            return;
        }
    };
    match msg.kind {
        PayloadKind::Initial => match bincode::deserialize::<InitialMessage>(&msg.payload) {
            Ok(initial) => { let _ = tx.send(Incoming::Initial { from: msg.from, message: initial }); }
            Err(e) => { let _ = tx.send(Incoming::Error(format!("decode initial: {e}"))); }
        },
        PayloadKind::Envelope => match bincode::deserialize::<Envelope>(&msg.payload) {
            Ok(envelope) => { let _ = tx.send(Incoming::Envelope { from: msg.from, envelope }); }
            Err(e) => { let _ = tx.send(Incoming::Error(format!("decode envelope: {e}"))); }
        },
        PayloadKind::Plaintext => {
            let text = String::from_utf8_lossy(&msg.payload).to_string();
            let _ = tx.send(Incoming::Plaintext { from: msg.from, text });
        }
    }
}
