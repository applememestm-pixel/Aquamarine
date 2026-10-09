//! Signed-challenge authentication.
//!
//! Flow:
//!
//! 1. Client POSTs its `IdentityPublic` to `/v1/auth/challenge`. Server
//!    returns 24 random bytes as a base64 nonce and remembers that this
//!    nonce is pending for that identity's DH public.
//! 2. Client signs the raw nonce bytes with its Ed25519 signing key and
//!    POSTs `{ identity, nonce, signature }` to `/v1/auth/verify`.
//!    Server checks that (a) the nonce is one it issued for this
//!    identity and hasn't been used yet, (b) the signature verifies
//!    under the identity's signing key, and (c) the nonce is not
//!    expired. If all pass, it consumes the nonce and issues a bearer
//!    token bound to the identity's DH public and an expiry.
//! 3. Every protected request carries the token as `Authorization:
//!    Bearer <token>`. The token's binding lets the server enforce
//!    that the caller owns the identity they claim to operate under.
//!
//! Token format (base64url, no padding):
//! ```text
//!   [dh_pub    :: 32 bytes]
//!   [expires_at:: 8 bytes big-endian, seconds since UNIX epoch]
//!   [tag       :: 32 bytes HMAC-SHA256(server_key, dh_pub || expires_at)]
//! ```
//! Verifying constant-time compares the tag; no state on the server is
//! required to check a token once issued. Rotating `server_key`
//! invalidates every outstanding token in one step.

use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use aqua_core::{
    crypto::{hmac_sha256, ED25519_SIG_LEN, HMAC_TAG_LEN, X25519_LEN},
};
use axum::{
    async_trait,
    extract::{FromRequestParts, State},
    http::{request::Parts, StatusCode},
    Json,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use dashmap::DashMap;
use rand_core::{OsRng, RngCore};
use subtle::ConstantTimeEq;

// Re-export wire types so integration tests and callers keep working.
pub use aqua_core::wire::auth::{
    ChallengeRequest, ChallengeResponse, VerifyRequest, VerifyResponse,
};

use crate::AppState;

// ---------- Constants ------------------------------------------------------

/// Bytes of randomness in a challenge nonce.
pub const NONCE_LEN: usize = 24;

/// A challenge is valid for this long before the client must ask for a
/// new one.
pub const NONCE_TTL: Duration = Duration::from_secs(60);

/// Tokens issued after a successful verify live this long.
pub const TOKEN_TTL: Duration = Duration::from_secs(60 * 60);

// ---------- State ---------------------------------------------------------

pub struct AuthState {
    pub server_key: [u8; 32],
    pending: DashMap<[u8; NONCE_LEN], Pending>,
}

struct Pending {
    identity_dh_pub: [u8; X25519_LEN],
    issued_at: SystemTime,
}

impl AuthState {
    pub fn new() -> Arc<Self> {
        let mut key = [0u8; 32];
        OsRng.fill_bytes(&mut key);
        Arc::new(Self { server_key: key, pending: DashMap::new() })
    }

    fn issue_nonce(&self, identity_dh_pub: [u8; X25519_LEN]) -> [u8; NONCE_LEN] {
        let mut nonce = [0u8; NONCE_LEN];
        OsRng.fill_bytes(&mut nonce);
        self.pending.insert(
            nonce,
            Pending { identity_dh_pub, issued_at: SystemTime::now() },
        );
        self.prune();
        nonce
    }

    /// Look up and consume a pending challenge.
    fn take_pending(
        &self,
        nonce: &[u8; NONCE_LEN],
        expected_identity: [u8; X25519_LEN],
    ) -> Result<(), AuthError> {
        let entry = self
            .pending
            .remove(nonce)
            .ok_or(AuthError::UnknownNonce)?;
        if entry.1.identity_dh_pub.ct_eq(&expected_identity).unwrap_u8() == 0 {
            return Err(AuthError::IdentityMismatch);
        }
        if entry
            .1
            .issued_at
            .elapsed()
            .map(|d| d > NONCE_TTL)
            .unwrap_or(true)
        {
            return Err(AuthError::NonceExpired);
        }
        Ok(())
    }

    fn prune(&self) {
        let now = SystemTime::now();
        self.pending.retain(|_, p| {
            now.duration_since(p.issued_at)
                .map(|d| d <= NONCE_TTL)
                .unwrap_or(false)
        });
    }

    // ---- Token minting / verification ----

    pub fn mint_token(&self, identity_dh_pub: [u8; X25519_LEN]) -> (String, u64) {
        let expires_at = (SystemTime::now() + TOKEN_TTL)
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let mut msg = Vec::with_capacity(X25519_LEN + 8);
        msg.extend_from_slice(&identity_dh_pub);
        msg.extend_from_slice(&expires_at.to_be_bytes());
        let tag = hmac_sha256(&self.server_key, &msg);

        let mut out = Vec::with_capacity(X25519_LEN + 8 + HMAC_TAG_LEN);
        out.extend_from_slice(&identity_dh_pub);
        out.extend_from_slice(&expires_at.to_be_bytes());
        out.extend_from_slice(&tag);
        (URL_SAFE_NO_PAD.encode(&out), expires_at)
    }

    /// Verify a token. Returns the bound identity DH public on success.
    pub fn verify_token(&self, token: &str) -> Result<[u8; X25519_LEN], AuthError> {
        let raw = URL_SAFE_NO_PAD
            .decode(token.as_bytes())
            .map_err(|_| AuthError::BadToken)?;
        if raw.len() != X25519_LEN + 8 + HMAC_TAG_LEN {
            return Err(AuthError::BadToken);
        }
        let mut dh_pub = [0u8; X25519_LEN];
        dh_pub.copy_from_slice(&raw[..X25519_LEN]);
        let mut exp_bytes = [0u8; 8];
        exp_bytes.copy_from_slice(&raw[X25519_LEN..X25519_LEN + 8]);
        let expires_at = u64::from_be_bytes(exp_bytes);
        let tag_in = &raw[X25519_LEN + 8..];

        let mut signed = Vec::with_capacity(X25519_LEN + 8);
        signed.extend_from_slice(&dh_pub);
        signed.extend_from_slice(&exp_bytes);
        let expected_tag = hmac_sha256(&self.server_key, &signed);

        if expected_tag.ct_eq(tag_in).unwrap_u8() == 0 {
            return Err(AuthError::BadToken);
        }
        let now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if now_secs >= expires_at {
            return Err(AuthError::TokenExpired);
        }
        Ok(dh_pub)
    }
}

// ---------- Errors --------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("nonce is not one we issued (or already used)")]
    UnknownNonce,
    #[error("nonce was issued for a different identity")]
    IdentityMismatch,
    #[error("challenge nonce has expired")]
    NonceExpired,
    #[error("bad signature on challenge")]
    BadSignature,
    #[error("token is invalid or forged")]
    BadToken,
    #[error("token has expired")]
    TokenExpired,
    #[error("missing Authorization header")]
    MissingHeader,
    #[error("Authorization header is malformed")]
    MalformedHeader,
    #[error("this token is bound to a different identity than the target user")]
    WrongIdentityForUser,
    #[error("username already claimed by a different identity")]
    UsernameTaken,
    #[error("this username is not registered — sign up by email first")]
    NotRegistered,
}

impl AuthError {
    pub fn status(&self) -> StatusCode {
        match self {
            AuthError::MissingHeader | AuthError::MalformedHeader => StatusCode::UNAUTHORIZED,
            AuthError::BadToken | AuthError::TokenExpired => StatusCode::UNAUTHORIZED,
            AuthError::WrongIdentityForUser | AuthError::UsernameTaken => StatusCode::FORBIDDEN,
            AuthError::NotRegistered => StatusCode::FORBIDDEN,
            _ => StatusCode::BAD_REQUEST,
        }
    }
}

// ---------- HTTP handlers -------------------------------------------------

pub async fn challenge(
    State(state): State<AppState>,
    Json(body): Json<ChallengeRequest>,
) -> Result<Json<ChallengeResponse>, (StatusCode, String)> {
    let raw = URL_SAFE_NO_PAD
        .decode(body.identity_dh_pub.as_bytes())
        .map_err(|_| (StatusCode::BAD_REQUEST, "bad base64".into()))?;
    let dh_pub: [u8; X25519_LEN] = raw
        .as_slice()
        .try_into()
        .map_err(|_| (StatusCode::BAD_REQUEST, "expected 32-byte DH public".into()))?;

    let nonce = state.auth.issue_nonce(dh_pub);
    Ok(Json(ChallengeResponse { nonce: URL_SAFE_NO_PAD.encode(nonce) }))
}

pub async fn verify(
    State(state): State<AppState>,
    Json(body): Json<VerifyRequest>,
) -> Result<Json<VerifyResponse>, (StatusCode, String)> {
    let dh_pub = *body.identity.dh_pub.as_bytes();
    let fp = hex::encode(&dh_pub[..8]);
    tracing::info!(identity = %fp, "POST /v1/auth/verify");

    let nonce_raw = URL_SAFE_NO_PAD
        .decode(body.nonce.as_bytes())
        .map_err(|_| (StatusCode::BAD_REQUEST, "bad nonce base64".into()))?;
    let nonce_arr: [u8; NONCE_LEN] = nonce_raw
        .as_slice()
        .try_into()
        .map_err(|_| (StatusCode::BAD_REQUEST, "wrong nonce length".into()))?;

    let sig_raw = URL_SAFE_NO_PAD
        .decode(body.signature.as_bytes())
        .map_err(|_| (StatusCode::BAD_REQUEST, "bad signature base64".into()))?;
    let sig_arr: [u8; ED25519_SIG_LEN] = sig_raw.as_slice().try_into().map_err(|_| {
        (StatusCode::BAD_REQUEST, "wrong signature length".into())
    })?;

    // Sig must verify BEFORE consuming the pending nonce, so an attacker
    // guessing DH publics can't force us to burn nonces on failed sigs.
    if let Err(e) = body.identity.sign_pub.verify(&nonce_arr, &sig_arr) {
        tracing::warn!(identity = %fp, error = ?e, "auth/verify REJECT: bad signature");
        return Err((AuthError::BadSignature.status(), AuthError::BadSignature.to_string()));
    }

    if let Err(e) = state.auth.take_pending(&nonce_arr, dh_pub) {
        tracing::warn!(identity = %fp, error = %e, "auth/verify REJECT: pending nonce check failed");
        return Err((e.status(), e.to_string()));
    }

    let (token, expires_at) = state.auth.mint_token(dh_pub);
    tracing::info!(identity = %fp, "auth/verify OK: token minted");
    Ok(Json(VerifyResponse { token, expires_at }))
}

// ---------- Extractor / guard --------------------------------------------

/// Extractor that validates the Bearer token and returns the caller's
/// identity DH public.
pub struct Caller(pub [u8; X25519_LEN]);

#[async_trait]
impl FromRequestParts<AppState> for Caller {
    type Rejection = (StatusCode, String);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let hdr = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .ok_or_else(|| {
                let e = AuthError::MissingHeader;
                (e.status(), e.to_string())
            })?;
        let val = hdr.to_str().map_err(|_| {
            let e = AuthError::MalformedHeader;
            (e.status(), e.to_string())
        })?;
        let token = val.strip_prefix("Bearer ").ok_or_else(|| {
            let e = AuthError::MalformedHeader;
            (e.status(), e.to_string())
        })?;
        let dh_pub = state
            .auth
            .verify_token(token)
            .map_err(|e| (e.status(), e.to_string()))?;
        Ok(Caller(dh_pub))
    }
}

/// Verify a token passed as a `token=...` query parameter. Returns the
/// caller's identity DH public. Used for the WebSocket upgrade, where
/// browser clients cannot attach an `Authorization` header.
pub fn verify_ws_token(state: &AppState, token: &str) -> Result<[u8; X25519_LEN], AuthError> {
    state.auth.verify_token(token)
}

// ---------- Tests ---------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_roundtrip() {
        let auth = AuthState::new();
        let dh_pub = [7u8; X25519_LEN];
        let (tok, exp) = auth.mint_token(dh_pub);
        assert!(exp > 0);
        let out = auth.verify_token(&tok).unwrap();
        assert_eq!(out, dh_pub);
    }

    #[test]
    fn tampered_token_rejected() {
        let auth = AuthState::new();
        let (tok, _) = auth.mint_token([1u8; X25519_LEN]);
        // Flip a byte in the middle (the DH pub part) — HMAC tag no longer matches.
        let mut bytes = URL_SAFE_NO_PAD.decode(&tok).unwrap();
        bytes[3] ^= 0x01;
        let bad = URL_SAFE_NO_PAD.encode(&bytes);
        assert!(matches!(auth.verify_token(&bad), Err(AuthError::BadToken)));
    }

    #[test]
    fn token_from_a_different_server_is_rejected() {
        let a = AuthState::new();
        let b = AuthState::new();
        let (tok, _) = a.mint_token([2u8; X25519_LEN]);
        assert!(matches!(b.verify_token(&tok), Err(AuthError::BadToken)));
    }

    #[test]
    fn pending_nonce_flow() {
        let auth = AuthState::new();
        let dh_pub = [9u8; X25519_LEN];
        let nonce = auth.issue_nonce(dh_pub);
        auth.take_pending(&nonce, dh_pub).unwrap();
        // Second take fails (consumed).
        assert!(matches!(
            auth.take_pending(&nonce, dh_pub),
            Err(AuthError::UnknownNonce)
        ));
    }

    #[test]
    fn pending_nonce_rejects_wrong_identity() {
        let auth = AuthState::new();
        let nonce = auth.issue_nonce([1u8; X25519_LEN]);
        assert!(matches!(
            auth.take_pending(&nonce, [2u8; X25519_LEN]),
            Err(AuthError::IdentityMismatch)
        ));
    }
}
