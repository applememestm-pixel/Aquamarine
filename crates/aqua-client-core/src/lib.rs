//! Non-UI client logic for Aquamarine.
//!
//! * [`vault`]     — encrypted-at-rest local store (SQLite + ChaCha20-Poly1305).
//!                   Holds the identity, per-peer sessions, signed prekey
//!                   history, and the one-time prekey pool.
//! * [`prekeys`]   — signed-prekey rotation policy and one-time prekey
//!                   pool top-up. Pure logic; no network I/O.
//! * [`transport`] — HTTP + WebSocket client for `aqua-server`. Handles
//!                   auth, bundle publish/fetch, and envelope relay.
//!
//! `aqua-core` is re-exported so downstream crates only need to depend
//! on `aqua-client-core`.

#![forbid(unsafe_code)]

pub use aqua_core;

pub mod vault;
pub use vault::{MasterKey, Vault, VaultError, MASTER_KEY_LEN};

pub mod prekeys;
pub use prekeys::{
    PrekeyError, PrekeyManager, RefreshOutcome, OPK_MIN_POOL, OPK_TARGET, SPK_ROTATION_PERIOD,
};

pub mod transport;
pub use transport::{
    request_email_code, request_email_code_ex, verify_email_code, CodeRequest, DeviceInfo, Incoming, ProfileCard, Transport, TransportError,
};

pub mod telegram;
pub mod backup;
pub use telegram::{import_pack as import_telegram_pack, sanitize_folder as sanitize_pack_folder, ImportReport};
