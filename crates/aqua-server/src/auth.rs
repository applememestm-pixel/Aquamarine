//! Signed-challenge authentication.
//!
//! Flow:
//!
//! 1. Client POSTs its `IdentityPublic` to `/v1/auth/challenge`. Server
//!    returns 24 random bytes as a base64 nonce and remembers that this
//!    nonce is pending for that identity's DH public.
//! 2. Client signs the raw nonce bytes with its Ed25519 signing key and
//!    POSTs `{ identity, nonce, signature, dh_proof }` to `/v1/auth/verify`.
//!    `dh_proof` (see `aqua_core::wire::auth::dh_proof`) is an HMAC keyed by
//!    DH(identity X25519 secret, the challenge's one-off server key): it
//!    shows the caller owns the X25519 key the token gets bound to, and it
//!    covers the Ed25519 key, which the relay then keeps for that X25519
//!    key (AQUA-SEC-001: before 1.21.0-21 any Ed25519 key was accepted, so
//!    anyone who knew a public X25519 key could get that account's token).
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
    crypto::{hmac_sha256, DhPublic, DhSecret, ED25519_SIG_LEN, HMAC_TAG_LEN, X25519_LEN},
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

/// Most challenges waiting for an answer at once. The challenge endpoint
/// needs no token, so without a cap a flood of requests grows the map
/// without bound for a minute.
pub const MAX_PENDING: usize = 50_000;

// ---------- State ---------------------------------------------------------

pub struct AuthState {
    pub server_key: [u8; 32],
    pending: DashMap<[u8; NONCE_LEN], Pending>,
    max_pending: usize,
}

struct Pending {
    identity_dh_pub: [u8; X25519_LEN],
    issued_at: SystemTime,
    /// One-off X25519 secret for the proof of possession (see
    /// [`aqua_core::wire::auth::dh_proof`]). Zeroized on drop.
    eph: DhSecret,
}

impl AuthState {
    pub fn new() -> Arc<Self> {
        let mut key = [0u8; 32];
        OsRng.fill_bytes(&mut key);
        Arc::new(Self { server_key: key, pending: DashMap::new(), max_pending: MAX_PENDING })
    }

    fn issue_nonce(&self, identity_dh_pub: [u8; X25519_LEN]) -> Option<([u8; NONCE_LEN], DhPublic)> {
        if self.pending.len() >= self.max_pending {
            self.prune();
            if self.pending.len() >= self.max_pending {
                return None;
            }
        }
        let mut nonce = [0u8; NONCE_LEN];
        OsRng.fill_bytes(&mut nonce);
        let eph = DhSecret::generate();
        let eph_pub = eph.public();
        self.pending.insert(
            nonce,
            Pending { identity_dh_pub, issued_at: SystemTime::now(), eph },
        );
        if self.pending.len() % 1024 == 0 {
            self.prune();
        }
        Some((nonce, eph_pub))
    }

    /// Look up and consume a pending challenge.
    fn take_pending(
        &self,
        nonce: &[u8; NONCE_LEN],
        expected_identity: [u8; X25519_LEN],
    ) -> Result<DhSecret, AuthError> {
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
        Ok(entry.1.eph)
    }

    /// Challenges waiting for an answer (tests, panel).
    pub fn pending_len(&self) -> usize {
        self.pending.len()
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
    #[error("no proof of the identity key: update Aquamarine")]
    MissingProof,
    #[error("the proof of the identity key does not match")]
    BadProof,
    #[error("this identity is bound to a different signing key")]
    SigningKeyMismatch,
    #[error("too many sign-ins in progress, try again in a minute")]
    Busy,
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
            AuthError::BadProof | AuthError::MissingProof | AuthError::BadSignature => StatusCode::UNAUTHORIZED,
            AuthError::SigningKeyMismatch => StatusCode::FORBIDDEN,
            AuthError::Busy => StatusCode::SERVICE_UNAVAILABLE,
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

    let (nonce, eph_pub) = state
        .auth
        .issue_nonce(dh_pub)
        .ok_or((AuthError::Busy.status(), AuthError::Busy.to_string()))?;
    Ok(Json(ChallengeResponse {
        nonce: URL_SAFE_NO_PAD.encode(nonce),
        server_eph: Some(URL_SAFE_NO_PAD.encode(eph_pub.as_bytes())),
    }))
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

    // The proof travels with the request; refuse before touching state.
    let proof: [u8; HMAC_TAG_LEN] = match body.dh_proof.as_deref() {
        None => {
            tracing::warn!(identity = %fp, "auth/verify REJECT: no proof of the X25519 key (old app)");
            return Err((AuthError::MissingProof.status(), AuthError::MissingProof.to_string()));
        }
        Some(p) => URL_SAFE_NO_PAD
            .decode(p.as_bytes())
            .ok()
            .and_then(|b| b.try_into().ok())
            .ok_or((StatusCode::BAD_REQUEST, "bad proof".into()))?,
    };

    let eph = match state.auth.take_pending(&nonce_arr, dh_pub) {
        Ok(eph) => eph,
        Err(e) => {
            tracing::warn!(identity = %fp, error = %e, "auth/verify REJECT: pending nonce check failed");
            return Err((e.status(), e.to_string()));
        }
    };

    // AQUA-SEC-001: the signature above only shows the caller holds *some*
    // Ed25519 key. The token is bound to the X25519 key, so the caller must
    // also show it holds that key's secret: only DH(identity, eph) gives the
    // right proof, and the proof covers the Ed25519 key of this request.
    let shared = eph.dh(&body.identity.dh_pub);
    let expected = aqua_core::wire::auth::dh_proof(
        shared.as_bytes(),
        &nonce_arr,
        &dh_pub,
        body.identity.sign_pub.as_bytes(),
    );
    drop(eph);
    let ok = expected.map(|e| e.ct_eq(&proof).unwrap_u8() == 1).unwrap_or(false);
    if !ok {
        tracing::warn!(identity = %fp, "auth/verify REJECT: bad proof of the X25519 key");
        return Err((AuthError::BadProof.status(), AuthError::BadProof.to_string()));
    }

    // The X25519 and Ed25519 keys are now proven to belong together; the
    // first proven pair is kept and any other Ed25519 key is refused.
    match state.directory.bind_identity_sign(&dh_pub, body.identity.sign_pub.as_bytes()) {
        Ok(true) => {}
        Ok(false) => {
            tracing::warn!(identity = %fp, "auth/verify REJECT: identity is bound to another signing key");
            return Err((AuthError::SigningKeyMismatch.status(), AuthError::SigningKeyMismatch.to_string()));
        }
        Err(e) => {
            tracing::error!(identity = %fp, error = %e, "auth/verify: binding store failed");
            return Err((StatusCode::SERVICE_UNAVAILABLE, "storage error".into()));
        }
    }

    // Banned on this server (panel -> People -> Accounts): no token.
    if state.directory.is_banned_dh(&dh_pub) {
        tracing::warn!(identity = %fp, "auth/verify REJECT: banned here");
        return Err((StatusCode::FORBIDDEN, BANNED.into()));
    }
    let (token, expires_at) = state.auth.mint_token(dh_pub);
    tracing::info!(identity = %fp, "auth/verify OK: token minted");
    Ok(Json(VerifyResponse { token, expires_at }))
}

// ---------- Extractor / guard --------------------------------------------

/// Extractor that validates the Bearer token and returns the caller's
/// identity DH public.
pub struct Caller(pub [u8; X25519_LEN]);

/// What a banned account hears (the app shows it as it is).
pub const BANNED: &str = "you are banned on this server";

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
        // A token from before the ban stops working too.
        if state.directory.is_banned_dh(&dh_pub) {
            return Err((StatusCode::FORBIDDEN, BANNED.into()));
        }
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
        let (nonce, _) = auth.issue_nonce(dh_pub).unwrap();
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
        let (nonce, _) = auth.issue_nonce([1u8; X25519_LEN]).unwrap();
        assert!(matches!(
            auth.take_pending(&nonce, [2u8; X25519_LEN]),
            Err(AuthError::IdentityMismatch)
        ));
    }

    #[test]
    fn dh_proof_matches_only_with_the_identity_secret() {
        use aqua_core::wire::auth::dh_proof;
        let id = aqua_core::IdentityKeyPair::generate();
        let other = aqua_core::IdentityKeyPair::generate();
        let auth = AuthState::new();
        let dh = *id.public().dh_pub.as_bytes();
        let (nonce, eph_pub) = auth.issue_nonce(dh).unwrap();
        let eph = auth.take_pending(&nonce, dh).unwrap();
        let server = dh_proof(eph.dh(&id.public().dh_pub).as_bytes(), &nonce, &dh, id.public().sign_pub.as_bytes()).unwrap();
        let client = dh_proof(id.dh.dh(&eph_pub).as_bytes(), &nonce, &dh, id.public().sign_pub.as_bytes()).unwrap();
        assert_eq!(server, client);
        let forged = dh_proof(other.dh.dh(&eph_pub).as_bytes(), &nonce, &dh, id.public().sign_pub.as_bytes()).unwrap();
        assert_ne!(server, forged);
        // The proof covers the Ed25519 key: swapping it changes the proof.
        let swapped = dh_proof(id.dh.dh(&eph_pub).as_bytes(), &nonce, &dh, other.public().sign_pub.as_bytes()).unwrap();
        assert_ne!(server, swapped);
        // A low-order key gives an all-zero secret: no proof at all.
        assert!(dh_proof(&[0u8; 32], &nonce, &dh, id.public().sign_pub.as_bytes()).is_none());
    }

    #[test]
    fn pending_is_capped() {
        let auth = Arc::new(AuthState { max_pending: 64, ..Arc::try_unwrap(AuthState::new()).ok().unwrap() });
        for i in 0..64u64 {
            let mut k = [0u8; X25519_LEN];
            k[..8].copy_from_slice(&i.to_be_bytes());
            assert!(auth.issue_nonce(k).is_some());
        }
        assert!(auth.issue_nonce([3u8; X25519_LEN]).is_none(), "the map stops growing");
    }
}
