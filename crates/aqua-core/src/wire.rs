//! Serializable envelope types passed over the network.
//!
//! Two categories:
//!
//! 1. **E2EE payloads**: [`InitialMessage`] (first message of a session,
//!    carries X3DH bootstrapping) and [`Envelope`] (subsequent
//!    Double-Ratchet ciphertext). The server sees these as opaque
//!    bytes.
//! 2. **Control messages**: [`BundleUpload`], [`RelayMessage`], plus
//!    the auth request/response types in [`auth`]. Both `aqua-server`
//!    and `aqua-client-core` import these from here so wire changes
//!    happen in one place.

use serde::{Deserialize, Serialize};

use crate::{
    crypto::DhPublic,
    keys::{
        IdentityPublic, OneTimePreKeyPublic, PreKeyId, SignedPreKeyPublic,
    },
    ratchet::Header,
};

// ---------- E2EE payloads --------------------------------------------------

/// The very first message from initiator to responder in a new session.
/// Carries everything the responder needs to reconstruct the X3DH shared
/// secret plus the first Double-Ratchet-encrypted payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitialMessage {
    pub sender_identity: IdentityPublic,
    pub sender_ephemeral: DhPublic,
    /// The one-time prekey id consumed from the recipient's bundle, if
    /// the bundle contained one.
    pub used_opk_id: Option<PreKeyId>,
    /// Ratchet header for the first message. `dh_pub` here is the
    /// initiator's freshly-generated sending DH key.
    pub header: Header,
    /// AEAD ciphertext of the first plaintext.
    #[serde(with = "serde_bytes")]
    pub ciphertext: Vec<u8>,
}

/// A subsequent message on an existing session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub header: Header,
    #[serde(with = "serde_bytes")]
    pub ciphertext: Vec<u8>,
}

// ---------- Control-plane types --------------------------------------------

/// What a client posts to publish their prekey material.
/// `POST /v1/bundle/:user` body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleUpload {
    pub identity: IdentityPublic,
    pub signed_prekey: SignedPreKeyPublic,
    #[serde(default)]
    pub one_time_prekeys: Vec<OneTimePreKeyPublic>,
}

/// One relayed envelope, carried over the WebSocket connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayMessage {
    pub from: String,
    pub to: String,
    #[serde(with = "serde_bytes")]
    pub payload: Vec<u8>,
    pub kind: PayloadKind,
    /// When the relay took it from the sender (unix ms), stamped by the
    /// relay: a message fetched later (the app was closed) still shows the
    /// time it was sent. Older relays leave it out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ts: Option<i64>,
    /// Delivery id (random, set by the sender or the relay). With acks on
    /// (`/v1/ws?...&ack=1`) the relay confirms each message it took and keeps
    /// each one it hands over until the device confirms it; both sides drop
    /// repeats by this id. Older peers leave it out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

impl RelayMessage {
    /// "Got it" for the message with `id`; with no id it's the relay's
    /// greeting that says it sends and expects acks.
    pub fn ack(id: Option<String>) -> Self {
        Self { from: String::new(), to: String::new(), payload: Vec::new(), kind: PayloadKind::Ack, ts: None, id }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PayloadKind {
    Initial,
    Envelope,
    /// Un-encrypted UTF-8 text. Used only for bot ↔ user messaging,
    /// where no interactive X3DH handshake is possible. The UI marks
    /// these conversations as NOT end-to-end encrypted.
    Plaintext,
    /// Delivery confirmation (see [`RelayMessage::id`]); no payload.
    Ack,
}

/// Auth-flow request / response types.
pub mod auth {
    use super::*;

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ChallengeRequest {
        pub identity_dh_pub: String, // base64
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ChallengeResponse {
        pub nonce: String, // base64
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct VerifyRequest {
        pub identity: IdentityPublic,
        pub nonce: String,     // base64
        pub signature: String, // base64
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct VerifyResponse {
        pub token: String,
        pub expires_at: u64,
    }
}
