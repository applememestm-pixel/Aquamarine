//! Aquamarine E2EE core.
//!
//! Layers, bottom to top:
//!
//! * [`crypto`] — thin wrapper around audited primitives (X25519, Ed25519,
//!   ChaCha20-Poly1305, HKDF, HMAC-SHA256). This is where the raw math
//!   lives; everything above it treats it as a black box.
//! * [`keys`]   — long-term & one-time key material, prekey bundles.
//! * [`x3dh`]   — the initial key agreement between two parties.
//! * [`ratchet`] — Double Ratchet: turns the shared secret from X3DH into
//!   a stream of forward-secret, post-compromise-secure message keys.
//! * [`wire`]   — serializable envelopes and bundle types passed over the
//!   network.
//!
//! Design notes:
//!
//! * We keep our own protocol implementation instead of pulling in
//!   `libsignal` or `vodozemac`. The primitives underneath are still the
//!   same well-audited crates; only the glue is ours. This makes the code
//!   auditable in a single afternoon, at the cost of not being wire
//!   compatible with Signal or Matrix.
//! * Every module has unit tests that exercise happy path + adversarial
//!   cases (bad signatures, mismatched keys, out-of-order messages,
//!   replays).

#![forbid(unsafe_code)]
#![deny(rust_2018_idioms)]
#![warn(missing_debug_implementations)]

pub mod crypto;
pub mod keys;
pub mod x3dh;
pub mod ratchet;
pub mod wire;
pub mod session;
pub mod file_transfer;

pub use crypto::CryptoError;
pub use file_transfer::{decrypt_file, encrypt_file, encrypt_file_with_category, hex_sha256, FileEnvelope, FileError, MAX_FILE_SIZE, OFFER_PREFIX};
pub use keys::{IdentityKeyPair, IdentityPublic, OneTimePreKey, PreKeyBundle, SignedPreKey};
pub use ratchet::{DoubleRatchet, Header, RatchetError};
pub use session::{Session, SessionError};
pub use wire::{BundleUpload, Envelope, InitialMessage, PayloadKind, RelayMessage};
pub use x3dh::{x3dh_initiator, x3dh_responder, X3DHError};
