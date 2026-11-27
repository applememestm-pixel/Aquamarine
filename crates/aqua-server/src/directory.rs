//! Persistent user directory — the registry of `@username`s, their
//! bound identity, optional e-mail verification, and the Discord-style
//! profile card other clients can look up and search.
//!
//! Everything here is **public directory data** — display names, bios,
//! phone numbers people chose to share. It is NOT end-to-end encrypted,
//! because its whole job is to be searchable by other users. The
//! sensitive material (message contents, keys) never touches this
//! table.
//!
//! Storage is a small SQLite DB next to the server binary (path from
//! `AQUA_DIR_DB`, default `aqua-directory.sqlite`). It persists across
//! restarts so `@username`s stay stable — unlike the in-memory bundle
//! and blob stores, which clients re-populate on reconnect.
//!
//! ## Registration flow
//!
//! 1. `POST /v1/auth/email/request { email }` — server generates a
//!    6-digit code, stores it with a short TTL, and "sends" it. In dev
//!    mode (default, or `AQUA_DEV_CODES=1`) the code is logged to the
//!    server console and returned in the response as `dev_code` so you
//!    can test without a real mail server. Wire real SMTP in
//!    `send_email_code` later.
//! 2. `POST /v1/auth/email/verify { email, code }` — on match, returns
//!    a one-time `ticket` that proves the e-mail was verified.
//! 3. `POST /v1/users/register` (Bearer token) `{ ticket, username,
//!    display_name, about, phone }` — binds `@username` to the caller's
//!    identity, stores the profile + verified email. The username is
//!    reserved forever to that identity.

use std::{
    collections::HashMap,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use rand_core::{OsRng, RngCore};
use rusqlite::OptionalExtension;
use rusqlite::Connection;

/// A code is valid for this long after it is issued.
const CODE_TTL_SECS: u64 = 10 * 60;
/// A verification ticket is valid for this long.
const TICKET_TTL_SECS: u64 = 30 * 60;

/// SMTP delivery config. Comes from the AQUA_SMTP_* env vars, or from
/// `aqua-smtp.json` (written by the admin panel). Without it the server stays
/// in dev-code mode (codes shown in the console / admin panel).
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct SmtpConfig {
    pub host: String,
    pub user: String,
    pub pass: String,
    pub from: String,
    pub port: u16,
}

/// Where the admin panel keeps the SMTP settings (next to aqua-admin.json).
pub const SMTP_FILE: &str = "aqua-smtp.json";

impl SmtpConfig {
    fn from_env() -> Option<Self> {
        let host = std::env::var("AQUA_SMTP_HOST").ok()?;
        let user = std::env::var("AQUA_SMTP_USER").ok()?;
        let pass = std::env::var("AQUA_SMTP_PASS").ok()?;
        let from = std::env::var("AQUA_SMTP_FROM").unwrap_or_else(|_| user.clone());
        let port = std::env::var("AQUA_SMTP_PORT").ok()
            .and_then(|p| p.parse().ok()).unwrap_or(587);
        Some(Self { host, user, pass, from, port })
    }

    fn from_file() -> Option<Self> {
        let raw = std::fs::read_to_string(SMTP_FILE).ok()?;
        serde_json::from_str(&raw).ok()
    }

    /// `From:` header: show "Aquamarine" as the sender name.
    fn from_header(&self) -> String {
        if self.from.contains('<') { self.from.clone() } else { format!("Aquamarine <{}>", self.from) }
    }
}

/// What "Clear the server" empties (see [`Directory::wipe`]).
#[derive(Debug, Default, Clone, serde::Deserialize)]
pub struct Wipe {
    #[serde(default)]
    pub accounts: bool,
    /// Every person's account and everything of theirs, but the bots (and
    /// the chats people had with them) stay.
    #[serde(default)]
    pub people: bool,
    #[serde(default)]
    pub messages: bool,
    #[serde(default)]
    pub history: bool,
    #[serde(default)]
    pub backups: bool,
    #[serde(default)]
    pub profiles: bool,
    #[serde(default)]
    pub capsules: bool,
    #[serde(default)]
    pub bots: bool,
    #[serde(default)]
    pub marks: bool,
    #[serde(default)]
    pub codes: bool,
    /// Files (photos, videos, documents in chats): the blob store.
    #[serde(default)]
    pub files: bool,
    /// Sticker and emoji packs (aqua-packs).
    #[serde(default)]
    pub packs: bool,
    /// The server's journal and error reports.
    #[serde(default)]
    pub logs: bool,
}

pub struct Directory {
    db: Mutex<Connection>,
    /// AQUA_DEV_CODES=1: codes go back in API answers (tests, local
    /// experiments). Anything else: never (AQUA-SEC-003).
    dev_forced: Option<bool>,
    /// Key for the stored form of e-mail codes (AQUA-SEC-005): the table
    /// keeps HMAC(key, email|code), never the code. Random per start, so a
    /// restart voids outstanding codes (they last 10 minutes anyway).
    otp_key: [u8; 32],
    /// When Some, verification codes are sent via real SMTP.
    smtp: std::sync::RwLock<Option<SmtpConfig>>,
    /// "env" | "file" | "none" — where the SMTP settings came from.
    smtp_source: std::sync::RwLock<&'static str>,
    /// Last few dev-mode codes (email, code, unix secs) for the admin panel.
    recent: Mutex<std::collections::VecDeque<(String, String, u64)>>,
    /// "It's me" links from the code e-mails (link token → request), so the
    /// app fills the code in by itself once the link is tapped.
    links: Mutex<std::collections::HashMap<String, MagicLink>>,
}

/// One "It's me" link: tapped (`confirmed`) → the app that asked with
/// `nonce` gets the code. Kept in memory for the code's lifetime.
#[derive(Debug, Clone)]
struct MagicLink {
    email: String,
    code: String,
    nonce: String,
    confirmed: bool,
    at: u64,
}

/// One account row as relays exchange it (password = the stored hash).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FedUser {
    pub username: String,
    pub identity_dh: String,
    pub email: Option<String>,
    pub display_name: String,
    pub about: String,
    pub phone: String,
    pub is_bot: bool,
    pub created_at: i64,
    pub extra: String,
    pub avatar_ver: i64,
    pub banner_ver: i64,
    pub media_acl: String,
    pub password: String,
    pub music_ver: i64,
    pub updated_at: i64,
    /// Extra devices (JSON); older relays don't send it.
    #[serde(default = "no_devices")]
    pub devices: String,
    /// Hash of the recovery check; older relays don't send it.
    #[serde(default)]
    pub recovery: String,
    /// The account's ID (see [`uid_for`]); older relays don't send it.
    #[serde(default)]
    pub uid: String,
    /// Sign-in protection ([`Security`], JSON); older relays don't send it.
    #[serde(default)]
    pub security: String,
    /// Signed device list (JSON, `aqua_core::devlist`); older relays don't send it.
    #[serde(default)]
    pub devlist: String,
    /// The account's seal (JSON [`Seal`]): a device of the account signed its
    /// security fields. Older relays don't send it.
    #[serde(default)]
    pub seal: String,
}

/// "These are my account's key, password hash, two-step settings, devices,
/// recovery check and device list": signed by a device of the account
/// (Ed25519 over [`seal_message`]). Once an account has one, relays apply a
/// change of those fields from another relay only with a seal over the new
/// values by a device the account already had — so a relay of the network
/// can't change someone's key, password or devices by itself (1.21.0-23).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Seal {
    /// The signing device's identity DH key (hex).
    pub signer: String,
    /// Its Ed25519 key (hex).
    pub sign: String,
    /// The signature (hex).
    pub sig: String,
}

/// The security fields of an account as kept here (seal checks).
struct LocalSec {
    identity_dh: String,
    password: String,
    security: String,
    devices: String,
    recovery: String,
    devlist: String,
    seal: String,
}

/// What a seal signs: the security fields of an account, in this order.
pub fn seal_message(uid: &str, identity_dh_hex: &str, password: &str, security: &str, devices: &str, recovery: &str, devlist: &str) -> String {
    use sha2::Digest;
    let devlist_hash = hex::encode(sha2::Sha256::digest(devlist.as_bytes()));
    format!("aqua-account-seal-v1\n{uid}\n{identity_dh_hex}\n{password}\n{security}\n{devices}\n{recovery}\n{devlist_hash}")
}

/// Sign-in protection of an account: two-step codes and "no new devices".
/// Kept as JSON in `users.security` and replicated with the account.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Security {
    /// Sign-in on new devices is turned off.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub lock: bool,
    /// TOTP secret (base32); empty = two-step verification is off.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub totp: String,
    /// A secret being set up (until the first code confirms it).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub pending: String,
    /// Unused backup codes (hashes, see [`crate::totp::backup_hash`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub backup: Vec<String>,
    /// The last time step whose code was used (a code works once).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub last: u64,
    /// How many 30-second steps the account's authenticator runs ahead of
    /// (or behind) this relay's clock — see [`crate::totp::verify_near`].
    #[serde(default, skip_serializing_if = "is_zero_i64")]
    pub drift: i64,
    /// Linked Telegram account (user id): confirming a sign-in in Telegram
    /// is a second factor too (1.2.0-4, `tglogin.rs`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tg: String,
    /// Its @username or name, to show in the settings.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tg_name: String,
}

impl Security {
    /// A second factor is on (authenticator app or Telegram): the account
    /// may move to other devices and keep its data on the relay.
    pub fn portable(&self) -> bool {
        !self.totp.is_empty() || !self.tg.is_empty()
    }
}

fn is_zero(v: &u64) -> bool {
    *v == 0
}

fn is_zero_i64(v: &i64) -> bool {
    *v == 0
}

/// What a signed-in device sees of [`Security`].
#[derive(Debug, Clone, serde::Serialize)]
pub struct SecurityState {
    pub lock: bool,
    pub totp: bool,
    pub backup_left: usize,
    /// The relay requires two-step verification (set by the caller).
    pub required: bool,
    /// Linked Telegram (@username or name), if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tg: Option<String>,
}

/// An account's ID, like Telegram's: a 10-digit number that never changes,
/// whatever the @username. Relays compare accounts by it when they join, so
/// two different people with the same @name both keep their accounts.
/// Derived from the identity key the account was created with, so every
/// relay computes the same ID for rows from before IDs existed.
/// Any bans at all (so most servers skip the lookup on every request).
static ANY_BANS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn uid_for(identity_dh: &[u8]) -> String {
    use sha2::Digest;
    let h = sha2::Sha256::new().chain_update(b"aqua-uid\0").chain_update(identity_dh).finalize();
    let n = u64::from_be_bytes([h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]]);
    (1_000_000_000 + n % 9_000_000_000).to_string()
}

/// One reserved @name of the name registry (`/v1/fed/names`): what solo
/// relays and the network tell each other about their accounts — no keys,
/// no profile, just enough that a name isn't taken twice.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NameRow {
    pub username: String,
    pub uid: String,
    /// The relay the account lives on ("" = the one answering).
    #[serde(default)]
    pub home: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// How far ahead of our clock a peer's change time may be (ms).
pub const FED_SKEW_MS: i64 = 5 * 60 * 1000;
/// Two relays gave the same @name within this many seconds: the older
/// account keeps it (later, a claim can't move an account of ours).
pub const NAME_RACE_SECS: i64 = 10 * 60;

/// An account deleted on its relay (A7): signed by that relay's node key
/// over `aqua-fed-tomb-v1\n{uid}\n{username}\n{created_at}\n{deleted_at}\n{origin}`.
/// It covers that generation of the account (`created_at`): a copy of it from
/// a relay that didn't hear of the deletion isn't taken again; someone who
/// signs up anew (later `created_at`) is.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Tomb {
    pub uid: String,
    pub created_at: i64,
    pub username: String,
    pub deleted_at: i64,
    /// Node id of the relay that deleted it ("" — not in a network then).
    pub origin: String,
    pub sig: String,
    /// When it came here (ms): the order it is handed on in.
    #[serde(default)]
    pub got_at: i64,
}

impl Tomb {
    pub fn message(&self) -> String {
        format!("aqua-fed-tomb-v1\n{}\n{}\n{}\n{}\n{}", self.uid, self.username, self.created_at, self.deleted_at, self.origin)
    }
}

/// What applying a peer's account row did here.
#[derive(Debug, Default, PartialEq)]
pub struct FedApplied {
    pub applied: bool,
    /// The @name the row has here (another one when it was displaced).
    pub name: String,
    /// A local account lost its @name to an older account with the same
    /// name from the peer: (old name, the name it has now).
    pub displaced: Option<(String, String)>,
    /// A sealed account's security fields came without a valid seal and
    /// were kept as they are here.
    pub refused_security: bool,
    /// It couldn't be written (a database error, not a decision): the sync
    /// cursor must not move past it.
    pub failed: bool,
}

fn no_devices() -> String { "[]".into() }

/// Most devices an account may have besides its primary one.
pub const MAX_DEVICES: usize = 10;

/// One extra device of an account. The primary device is the row's
/// `identity_dh`; each device has its own identity key and its own E2EE
/// sessions (senders encrypt to every device separately).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Device {
    /// Hex of the device's identity DH public key.
    pub dh: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub added: i64,
}

/// Short public id of a device: the first 6 bytes of its identity key, hex.
pub fn device_id(dh: &[u8]) -> String {
    hex::encode(&dh[..dh.len().min(6)])
}

#[derive(Debug, thiserror::Error)]
pub enum DirError {
    #[error("database error: {0}")]
    Db(String),
    #[error("username is already taken")]
    UsernameTaken,
    #[error("username must be 3–32 chars, a-z 0-9 _ . only")]
    BadUsername,
    #[error("no verification code was requested for this email")]
    NoCode,
    #[error("the code is wrong or has expired")]
    BadCode,
    #[error("the registration ticket is invalid or has expired")]
    BadTicket,
    #[error("that email is already registered to another username")]
    EmailTaken,
    #[error("you are not the owner of this username")]
    NotOwner,
    #[error("{0}")]
    BadProfile(String),
    #[error("wrong email or password")]
    BadLogin,
    #[error("password must be at least 8 characters")]
    WeakPassword,
    #[error("one account can own at most 20 bots")]
    TooManyBots,
    #[error("an account with this e-mail already exists — sign in instead")]
    AccountExists,
    #[error("there's no account with this e-mail")]
    NoAccount,
    #[error("the recovery key doesn't match this account")]
    BadRecovery,
    #[error("the @username can be changed once a day")]
    RenameTooSoon,
    #[error("another value is already stored there")]
    Conflict,
    #[error("the account's stored history is full")]
    TooLarge,
    #[error("sign-in on new devices is turned off for this account")]
    DevicesLocked,
    #[error("two-step verification code required")]
    NeedTotp,
    #[error("wrong two-step verification code")]
    BadTotp,
    #[error("wrong @username or password")]
    BadNameLogin,
    #[error("this account lives on one device — turn on two-step verification there to sign in on another one")]
    DeviceBound,
    /// This device's key already has an account here (one per key): said
    /// to the key's holder only (the request is signed with it).
    #[error("this device already has an account here: @{0} — sign in to it")]
    KeyHasAccount(String),
    #[error("too many wrong attempts — try again in 15 minutes")]
    TooManyTries,
    #[error("too many codes asked for this address — wait a little and try again")]
    TooManyCodes,
    #[error("confirm the sign-in in Telegram")]
    NeedTelegram,
    #[error("that's not the Telegram account linked to this account")]
    WrongTelegram,
}

/// Wrong two-step codes per e-mail ticket before the ticket is dropped.
const MAX_MFA_FAILS: i64 = 5;

/// After a @username change the old name forwards to the new one (and stays
/// reserved for its owner) this long — Telegram keeps old names a while too.
pub const RENAME_KEEP_SECS: u64 = 30 * 24 * 3600;

/// All of one account's stored history together, at most.
pub const HIST_LIMIT: i64 = 1024 * 1024 * 1024;
/// Stored history of an account without two-step verification: it lives on
/// one device and can't be moved, so it gets a small share of the space.
pub const HIST_LIMIT_PLAIN: i64 = 64 * 1024 * 1024;

/// Wrong passwords / codes per @username before sign-in is paused.
const MAX_NAME_FAILS: u32 = 10;
const NAME_FAIL_WINDOW: u64 = 15 * 60;

fn name_fails() -> &'static Mutex<HashMap<String, (u32, u64)>> {
    static F: std::sync::OnceLock<Mutex<HashMap<String, (u32, u64)>>> = std::sync::OnceLock::new();
    F.get_or_init(|| Mutex::new(HashMap::new()))
}

impl From<rusqlite::Error> for DirError {
    fn from(e: rusqlite::Error) -> Self { DirError::Db(e.to_string()) }
}

/// A public profile card, as returned by the lookup + search endpoints.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Profile {
    pub username: String,
    pub display_name: String,
    pub about: String,
    pub phone: String,
    pub is_bot: bool,
    pub created_at: i64,
    /// Free-form public profile extras published by the owner (banner
    /// colours, status line, pronouns, birthday, …). Always a JSON object.
    pub extra: serde_json::Value,
    /// Version of the uploaded avatar image; 0 = none. Bumps on change so
    /// clients can cache `GET /v1/users/:u/media/avatar` by version.
    pub avatar_ver: i64,
    /// Version of the uploaded banner image; 0 = none.
    pub banner_ver: i64,
    /// Version of the "favourite track" audio file; 0 = none.
    pub music_ver: i64,
    /// The account's ID (never changes; see [`uid_for`]).
    pub uid: String,
    /// Confirmed by this relay's @verify bot (a check mark by the name).
    pub verified: bool,
}

/// Columns selected for a [`Profile`], in the order `profile_from_row` reads.
const PROFILE_COLS: &str =
    "username, display_name, about, phone, is_bot, created_at, extra, avatar_ver, banner_ver, music_ver, uid, \
     EXISTS(SELECT 1 FROM verified v WHERE v.uid = users.uid AND users.uid <> '')";

fn profile_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Profile> {
    let extra: String = r.get(6)?;
    Ok(Profile {
        username: r.get(0)?,
        display_name: r.get(1)?,
        about: r.get(2)?,
        phone: r.get(3)?,
        is_bot: r.get::<_, i64>(4)? != 0,
        created_at: r.get(5)?,
        extra: serde_json::from_str(&extra).unwrap_or_else(|_| serde_json::json!({})),
        avatar_ver: r.get(7)?,
        banner_ver: r.get(8)?,
        music_ver: r.get(9)?,
        uid: r.get(10)?,
        verified: r.get::<_, i64>(11).unwrap_or(0) != 0,
    })
}

// ---- passwords: PBKDF2-HMAC-SHA256, 120k rounds, random 16-byte salt ----

const PBKDF2_ROUNDS: u32 = 120_000;

/// Bots one identity may own.
pub const MAX_BOTS_PER_OWNER: i64 = 20;

/// Stored form of a bot token (192 random bits, so a plain hash is enough).
pub fn bot_token_hash(token: &str) -> String {
    use sha2::Digest;
    hex::encode(sha2::Sha256::digest(token.as_bytes()))
}

fn token_hint(token: &str) -> String {
    token.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect()
}

fn pbkdf2_sha256(password: &[u8], salt: &[u8], rounds: u32) -> [u8; 32] {
    use hmac::{Hmac, Mac};
    type H = Hmac<sha2::Sha256>;
    let mut mac = <H as Mac>::new_from_slice(password).expect("hmac key");
    mac.update(salt);
    mac.update(&1u32.to_be_bytes());
    let mut u: [u8; 32] = mac.finalize().into_bytes().into();
    let mut out = u;
    for _ in 1..rounds {
        let mut mac = <H as Mac>::new_from_slice(password).expect("hmac key");
        mac.update(&u);
        u = mac.finalize().into_bytes().into();
        for (o, x) in out.iter_mut().zip(u.iter()) { *o ^= x; }
    }
    out
}

/// What the relay stores of a recovery check.
fn recovery_hash(check: &str) -> String {
    use sha2::Digest;
    hex::encode(sha2::Sha256::digest(format!("aquamarine/recovery/v1|{check}").as_bytes()))
}

/// Constant-time byte comparison.
fn subtle_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub fn hash_password(password: &str) -> String {
    let mut salt = [0u8; 16];
    OsRng.fill_bytes(&mut salt);
    let h = pbkdf2_sha256(password.as_bytes(), &salt, PBKDF2_ROUNDS);
    format!("pbkdf2${PBKDF2_ROUNDS}${}${}", hex::encode(salt), hex::encode(h))
}

pub fn verify_password(password: &str, stored: &str) -> bool {
    let parts: Vec<&str> = stored.split('$').collect();
    if parts.len() != 4 || parts[0] != "pbkdf2" { return false; }
    let (Ok(rounds), Ok(salt), Ok(want)) = (parts[1].parse::<u32>(), hex::decode(parts[2]), hex::decode(parts[3])) else { return false };
    let got = pbkdf2_sha256(password.as_bytes(), &salt, rounds);
    // constant-time compare
    want.len() == got.len() && want.iter().zip(got.iter()).fold(0u8, |a, (x, y)| a | (x ^ y)) == 0
}

/// Profile-photo privacy: `everybody` minus `deny`, or only `allow`.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct MediaAcl {
    #[serde(default)]
    pub everybody: bool,
    #[serde(default)]
    pub allow: Vec<String>,
    #[serde(default)]
    pub deny: Vec<String>,
}
pub const MEDIA_ACL_MAX: usize = 5000;

/// Largest avatar / banner image the server stores.
pub const PROFILE_MEDIA_LIMIT: usize = 10 * 1024 * 1024;
/// Animated avatars / banners (GIF, short silent video) must stay light.
pub const ANIMATED_MEDIA_LIMIT: usize = 3 * 1024 * 1024;
/// Static avatar / banner images.
pub const IMAGE_MEDIA_LIMIT: usize = 8 * 1024 * 1024;
/// Largest `extra` JSON blob accepted.
pub const PROFILE_EXTRA_LIMIT: usize = 8 * 1024;

/// Which profile image. Besides the main avatar and banner a profile keeps
/// up to 4 more avatars (`avatar2`…`avatar5`) and 2 more banners
/// (`banner2`, `banner3`) that the profile pages through; their versions
/// live in the profile's `extra` (the app writes them), not in columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind { Avatar, Banner, Music, Avatar2, Avatar3, Avatar4, Avatar5, Banner2, Banner3 }

impl MediaKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "avatar" => Some(Self::Avatar),
            "banner" => Some(Self::Banner),
            "music" => Some(Self::Music),
            "avatar2" => Some(Self::Avatar2),
            "avatar3" => Some(Self::Avatar3),
            "avatar4" => Some(Self::Avatar4),
            "avatar5" => Some(Self::Avatar5),
            "banner2" => Some(Self::Banner2),
            "banner3" => Some(Self::Banner3),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Avatar => "avatar", Self::Banner => "banner", Self::Music => "music",
            Self::Avatar2 => "avatar2", Self::Avatar3 => "avatar3", Self::Avatar4 => "avatar4", Self::Avatar5 => "avatar5",
            Self::Banner2 => "banner2", Self::Banner3 => "banner3",
        }
    }
    fn ver_col(self) -> Option<&'static str> {
        match self {
            Self::Avatar => Some("avatar_ver"), Self::Banner => Some("banner_ver"), Self::Music => Some("music_ver"),
            _ => None,
        }
    }
    fn is_avatar(self) -> bool { matches!(self, Self::Avatar | Self::Avatar2 | Self::Avatar3 | Self::Avatar4 | Self::Avatar5) }
    fn is_banner(self) -> bool { matches!(self, Self::Banner | Self::Banner2 | Self::Banner3) }
}

pub(crate) fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn random_key() -> [u8; 32] {
    let mut k = [0u8; 32];
    OsRng.fill_bytes(&mut k);
    k
}

/// Wrong codes a code survives; the next wrong one voids it (AQUA-SEC-005).
pub const CODE_MAX_FAILS: i64 = 5;
/// Shortest wait between two codes for one address.
pub const CODE_RESEND_SECS: u64 = 30;
/// Most codes for one address per hour.
pub const CODE_MAX_PER_HOUR: i64 = 10;

impl Directory {
    /// The stored form of an e-mail code: HMAC(otp_key, email | code), hex.
    fn code_tag(&self, email: &str, code: &str) -> String {
        hex::encode(aqua_core::crypto::hmac_sha256(&self.otp_key, format!("{email}|{}", code.trim()).as_bytes()))
    }

    /// In-memory fallback used when the on-disk DB can't be opened.
    /// Non-persistent, but keeps the server functional.
    pub fn open_memory() -> Self {
        let db = Connection::open_in_memory().expect("in-memory sqlite");
        Self::init_schema(&db).expect("init schema");
        Self {
            db: Mutex::new(db),
            dev_forced: Some(true),
            otp_key: random_key(),
            smtp: std::sync::RwLock::new(None),
            smtp_source: std::sync::RwLock::new("none"),
            recent: Mutex::new(Default::default()),
            links: Mutex::new(std::collections::HashMap::new()),
        }
    }

    fn init_schema(db: &Connection) -> rusqlite::Result<()> {
        // SQLite's own lower() folds ASCII only: "Полина" stayed "Полина"
        // and the query "полина" missed it. ulower() is Rust's Unicode one.
        use rusqlite::functions::FunctionFlags;
        db.create_scalar_function(
            "ulower", 1,
            FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
            |ctx| Ok(ctx.get::<Option<String>>(0)?.map(|s| s.to_lowercase())),
        )?;
        db.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS users (
                username       TEXT PRIMARY KEY,
                identity_dh    BLOB NOT NULL,
                email          TEXT,
                display_name   TEXT NOT NULL DEFAULT '',
                about          TEXT NOT NULL DEFAULT '',
                phone          TEXT NOT NULL DEFAULT '',
                is_bot         INTEGER NOT NULL DEFAULT 0,
                created_at     INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_users_email ON users(email);
            CREATE TABLE IF NOT EXISTS fed_tombstones (
                uid        TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                username   TEXT NOT NULL,
                deleted_at INTEGER NOT NULL,
                origin     TEXT NOT NULL,
                sig        TEXT NOT NULL,
                got_at     INTEGER NOT NULL,
                PRIMARY KEY (uid, created_at)
            );
            CREATE INDEX IF NOT EXISTS idx_fed_tombs_got ON fed_tombstones(got_at);
            CREATE TABLE IF NOT EXISTS fed_cursors (
                peer   TEXT NOT NULL,
                stream TEXT NOT NULL,
                since  INTEGER NOT NULL,
                after  TEXT NOT NULL DEFAULT '',
                at     INTEGER NOT NULL,
                PRIMARY KEY (peer, stream)
            );
            CREATE TABLE IF NOT EXISTS email_codes (
                email      TEXT PRIMARY KEY,
                code       TEXT NOT NULL,
                ticket     TEXT,
                issued_at  INTEGER NOT NULL,
                verified_at INTEGER
            );
            CREATE TABLE IF NOT EXISTS inbox (
                id      INTEGER PRIMARY KEY AUTOINCREMENT,
                user    TEXT NOT NULL,
                msg     BLOB NOT NULL,
                at      INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_inbox_user ON inbox(user, id);
            CREATE TABLE IF NOT EXISTS capsules (
                id          TEXT PRIMARY KEY,
                owner       TEXT NOT NULL,
                recipient   TEXT NOT NULL,
                key         TEXT NOT NULL,
                kind        TEXT NOT NULL,
                open_at     INTEGER NOT NULL DEFAULT 0,
                days        INTEGER NOT NULL DEFAULT 0,
                grace_h     INTEGER NOT NULL DEFAULT 0,
                created     INTEGER NOT NULL,
                requested   INTEGER NOT NULL DEFAULT 0,
                released    INTEGER NOT NULL DEFAULT 0,
                cancelled   INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_capsules_owner ON capsules(owner);
            CREATE TABLE IF NOT EXISTS last_seen (
                username TEXT PRIMARY KEY,
                at       INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS backups (
                username   TEXT PRIMARY KEY,
                data       BLOB NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS bot_keys (
                token_hash TEXT PRIMARY KEY,
                username   TEXT NOT NULL UNIQUE,
                owner_dh   BLOB NOT NULL,
                created_at INTEGER NOT NULL,
                hint       TEXT NOT NULL DEFAULT ''
            );
            CREATE INDEX IF NOT EXISTS idx_botkey_owner ON bot_keys(owner_dh);
            CREATE TABLE IF NOT EXISTS bot_peers (
                bot  TEXT NOT NULL,
                user TEXT NOT NULL,
                at   INTEGER NOT NULL,
                PRIMARY KEY (bot, user)
            );
            -- Names reserved by accounts of other relays (1.2.0-4): solo
            -- relays and the network share only (uid, @name), so one @name
            -- belongs to one account everywhere. Never shown in search.
            CREATE TABLE IF NOT EXISTS fed_names (
                username   TEXT PRIMARY KEY,
                uid        TEXT NOT NULL,
                home       TEXT NOT NULL DEFAULT '',
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_fed_names_upd ON fed_names(updated_at);
            CREATE INDEX IF NOT EXISTS idx_fed_names_uid ON fed_names(uid);
            CREATE TABLE IF NOT EXISTS prekeys (
                username   TEXT PRIMARY KEY,
                data       TEXT NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS profile_media (
                username   TEXT NOT NULL,
                kind       TEXT NOT NULL,
                mime       TEXT NOT NULL,
                data       BLOB NOT NULL,
                PRIMARY KEY (username, kind)
            );
            -- @username changes: for RENAME_KEEP_SECS the old name forwards
            -- to the new one and nobody else can take it.
            -- Encrypted chat history of an account, shared by its devices
            -- (client history_sync.rs): opaque keys and blobs; `seq` grows
            -- per account so a device asks "what changed since N".
            CREATE TABLE IF NOT EXISTS hist_store (
                username   TEXT NOT NULL,
                k          TEXT NOT NULL,
                seq        INTEGER NOT NULL,
                data       BLOB NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (username, k)
            );
            CREATE INDEX IF NOT EXISTS idx_hist_seq ON hist_store(username, seq);
            CREATE INDEX IF NOT EXISTS idx_hist_upd ON hist_store(updated_at);
            CREATE TABLE IF NOT EXISTS renames (
                old_name TEXT PRIMARY KEY,
                new_name TEXT NOT NULL,
                at       INTEGER NOT NULL
            );
            -- Accounts confirmed by the @verify bot (verify.rs), by account ID,
            -- so a rename keeps the check mark. Each relay keeps its own.
            CREATE TABLE IF NOT EXISTS verified (
                uid TEXT PRIMARY KEY,
                at  INTEGER NOT NULL
            );
            -- Sign-ups per address (a hash of the IP, not the IP): one
            -- account per address (lib.rs::signup_ip_check).
            CREATE TABLE IF NOT EXISTS signup_ips (
                ip TEXT PRIMARY KEY,
                n  INTEGER NOT NULL,
                at INTEGER NOT NULL
            );
            -- People the admin banned here (panel -> People -> Accounts): by
            -- account ID, with every identity key the account had then
            -- (devices too), so none of them gets a token here again.
            -- @names of public groups and channels: the same namespace as
            -- people and bots. The group itself lives on its members' devices
            -- only; this says which group (id), what it's called and whom to
            -- ask to join (its owner).
            CREATE TABLE IF NOT EXISTS group_names (
                username TEXT PRIMARY KEY,
                gid      TEXT NOT NULL UNIQUE,
                title    TEXT NOT NULL DEFAULT '',
                kind     TEXT NOT NULL DEFAULT 'group',
                owner    TEXT NOT NULL,
                at       INTEGER NOT NULL
            );
            -- AQUA-SEC-001 (1.21.0-21): the Ed25519 key each X25519 identity
            -- proved it belongs with at sign-in (auth.rs::verify). The first
            -- proven pair is kept; any other signing key for the same
            -- X25519 key is refused at sign-in, bundle upload and device
            -- lists. A new table, so older databases just gain it.
            CREATE TABLE IF NOT EXISTS identity_signs (
                dh   BLOB PRIMARY KEY,
                sign BLOB NOT NULL,
                at   INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS bans (
                uid  TEXT PRIMARY KEY,
                dhs  TEXT NOT NULL DEFAULT '',
                name TEXT NOT NULL DEFAULT '',
                at   INTEGER NOT NULL
            );
            "#,
        )?;
        if db.query_row("SELECT 1 FROM bans LIMIT 1", [], |_| Ok(())).is_ok() {
            ANY_BANS.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        // Columns added after the first release. SQLite has no
        // "ADD COLUMN IF NOT EXISTS", so ignore the duplicate-column error
        // on databases that already have them.
        for stmt in [
            "ALTER TABLE users ADD COLUMN extra TEXT NOT NULL DEFAULT '{}'",
            "ALTER TABLE users ADD COLUMN avatar_ver INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE users ADD COLUMN banner_ver INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE users ADD COLUMN media_acl TEXT NOT NULL DEFAULT ''",
            "ALTER TABLE users ADD COLUMN password TEXT NOT NULL DEFAULT ''",
            "ALTER TABLE users ADD COLUMN music_ver INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE users ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0",
            // Extra devices of the account (JSON list of {dh, name, added}).
            "ALTER TABLE users ADD COLUMN devices TEXT NOT NULL DEFAULT '[]'",
            // SHA-256 of the account's recovery check (see `set_recovery`).
            "ALTER TABLE users ADD COLUMN recovery TEXT NOT NULL DEFAULT ''",
            // The account's ID (see `uid_for`); filled by `fill_uids`.
            "ALTER TABLE users ADD COLUMN uid TEXT NOT NULL DEFAULT ''",
            // Two-step codes and "no new devices" (JSON, see `Security`).
            "ALTER TABLE users ADD COLUMN security TEXT NOT NULL DEFAULT '{}'",
            // The account's device list signed by its devices (1.2.0-5, JSON).
            "ALTER TABLE users ADD COLUMN devlist TEXT NOT NULL DEFAULT ''",
            // The account's seal by one of its devices (JSON `Seal`, 1.21.0-23).
            "ALTER TABLE users ADD COLUMN seal TEXT NOT NULL DEFAULT ''",
            // The ticket passed the two-step code (1) or the recovery key (2).
            "ALTER TABLE email_codes ADD COLUMN mfa INTEGER NOT NULL DEFAULT 0",
            // Wrong e-mail codes for the current code (AQUA-SEC-005, 1.21.0-21).
            "ALTER TABLE email_codes ADD COLUMN fails INTEGER NOT NULL DEFAULT 0",
            // Codes sent in the current hour and when that hour began.
            "ALTER TABLE email_codes ADD COLUMN sends INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE email_codes ADD COLUMN sends_at INTEGER NOT NULL DEFAULT 0",
            // Wrong two-step codes with this ticket.
            "ALTER TABLE email_codes ADD COLUMN mfa_fail INTEGER NOT NULL DEFAULT 0",
        ] {
            if let Err(e) = db.execute(stmt, []) {
                if !e.to_string().contains("duplicate column") {
                    return Err(e);
                }
            }
        }
        // Bot tokens used to be stored as they are; keep only their hashes.
        let old: bool = db
            .query_row("SELECT 1 FROM sqlite_master WHERE type='table' AND name='bot_tokens'", [], |_| Ok(true))
            .unwrap_or(false);
        if old {
            let rows: Vec<(String, String, Vec<u8>, i64)> = {
                let mut st = db.prepare("SELECT token, username, owner_dh, created_at FROM bot_tokens")?;
                let it = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
                it.filter_map(|r| r.ok()).collect()
            };
            for (token, username, owner, at) in rows {
                db.execute(
                    "INSERT OR IGNORE INTO bot_keys (token_hash, username, owner_dh, created_at, hint) VALUES (?1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![bot_token_hash(&token), username, owner, at, token_hint(&token)],
                )?;
            }
            db.execute("DROP TABLE bot_tokens", [])?;
        }
        // Federation: every change to a user row bumps updated_at, so peers
        // can pull "everything since T". Each change gets a time after every
        // other row's (1.21.0-25): with "this second + random ms" a row made
        // in the same second could land before a peer's cursor (kept on disk
        // now) and never be pulled. A row applied from a peer carries
        // its own updated_at and is left alone (WHEN clause).
        // Triggers of a build before 1.21.0-25 ("this second + random ms")
        // are replaced once, in one transaction: two processes opening the
        // same file at once (a restart, tests) collided between DROP and CREATE.
        let old_trigger = db.query_row("SELECT sql FROM sqlite_master WHERE type='trigger' AND name='users_touch_ins'", [], |r| r.get::<_, String>(0))
            .map(|sql| !sql.contains("julianday")).unwrap_or(false);
        if old_trigger {
            db.execute_batch("BEGIN IMMEDIATE; DROP TRIGGER IF EXISTS users_touch_ins; DROP TRIGGER IF EXISTS users_touch_upd; COMMIT;")?;
        }
        db.execute_batch(
            r#"
            CREATE TRIGGER IF NOT EXISTS users_touch_ins AFTER INSERT ON users
            WHEN NEW.updated_at = 0
            BEGIN UPDATE users SET updated_at = max(CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), (SELECT COALESCE(MAX(updated_at), 0) + 1 FROM users)) WHERE username = NEW.username; END;
            CREATE TRIGGER IF NOT EXISTS users_touch_upd AFTER UPDATE ON users
            WHEN NEW.updated_at = OLD.updated_at
            BEGIN UPDATE users SET updated_at = max(OLD.updated_at + 1, CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), (SELECT COALESCE(MAX(updated_at), 0) + 1 FROM users)) WHERE username = NEW.username; END;
            CREATE INDEX IF NOT EXISTS idx_users_updated ON users(updated_at);
            CREATE INDEX IF NOT EXISTS idx_users_uid ON users(uid);
            "#,
        )?;
        Self::fill_uids(db);
        Ok(())
    }

    /// Give every account without an ID its ID (new rows, rows from before
    /// IDs). Peers compute the same IDs, so this changes nothing between them.
    fn fill_uids(db: &Connection) {
        let rows: Vec<(String, Vec<u8>, i64)> = match db.prepare("SELECT username, identity_dh, is_bot FROM users WHERE uid = ''") {
            Ok(mut st) => st
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .map(|rows| rows.flatten().collect())
                .unwrap_or_default(),
            Err(_) => return,
        };
        for (name, dh, bot) in rows {
            // A bot's key is its owner's: its ID mixes in its name.
            let uid = if bot != 0 { uid_for(&[dh.as_slice(), b"bot:", name.as_bytes()].concat()) } else { uid_for(&dh) };
            let _ = db.execute("UPDATE users SET uid=?1 WHERE username=?2", rusqlite::params![uid, name]);
        }
    }

    /// The account's ID.
    pub fn uid_of(&self, username: &str) -> Option<String> {
        let db = self.db.lock().unwrap();
        db.query_row("SELECT uid FROM users WHERE username=?1", rusqlite::params![username.trim().to_lowercase()], |r| r.get(0)).ok()
    }

    /// The @username an ID has now (people find each other by ID too).
    pub fn username_by_uid(&self, uid: &str) -> Option<String> {
        let db = self.db.lock().unwrap();
        db.query_row("SELECT username FROM users WHERE uid=?1", rusqlite::params![uid.trim()], |r| r.get(0)).ok()
    }

    /// A free @name for an account that lost its own to an older account
    /// when relays joined: `name_1234` (the end of its ID), longer if taken.
    /// The same on every relay, so they agree without asking each other.
    fn displaced_name(db: &Connection, name: &str, uid: &str) -> String {
        for n in [4usize, 6, uid.len()] {
            let tail = &uid[uid.len().saturating_sub(n)..];
            let base: String = name.chars().take(32 - tail.len() - 1).collect();
            let cand = format!("{base}_{tail}");
            let other: Option<String> = db
                .query_row("SELECT uid FROM users WHERE username=?1", rusqlite::params![cand], |r| r.get(0))
                .ok();
            if other.is_none_or(|o| o == uid) {
                return cand;
            }
        }
        format!("u{uid}")
    }

    pub fn open() -> anyhow::Result<Self> {
        let path = std::env::var("AQUA_DIR_DB").unwrap_or_else(|_| "aqua-directory.sqlite".into());
        Self::open_path(&path)
    }

    /// [`Self::open`] of a given file (tests: a restart with the same data).
    pub fn open_path(path: &str) -> anyhow::Result<Self> {
        let db = Connection::open(path)?;
        // Every message passes through the inbox table (stored, then deleted
        // on the device's "got it"), behind one lock shared by every request.
        // The default rollback journal syncs the disk several times per
        // write — on a home server's SD card or HDD that's tens of ms each,
        // and everything else waited. WAL + NORMAL: one sync per checkpoint,
        // still crash-safe (a power cut may lose only the last moments).
        let _ = db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;");
        let _ = db.busy_timeout(std::time::Duration::from_secs(5));
        Self::init_schema(&db)?;
        // SMTP: env vars win, else the admin panel's aqua-smtp.json.
        let (smtp, source) = match SmtpConfig::from_env() {
            Some(c) => (Some(c), "env"),
            None => match SmtpConfig::from_file() {
                Some(c) => (Some(c), "file"),
                None => (None, "none"),
            },
        };
        // Dev mode (code returned in the API answer) only with an explicit
        // AQUA_DEV_CODES=1 (AQUA-SEC-003: before 1.21.0-21 it was on
        // whenever no mail server was set, so anyone could get the code for
        // any address — and reset that account's password). Without mail,
        // codes still go to the server console and the admin panel for the
        // operator to pass on.
        let dev_forced = Some(std::env::var("AQUA_DEV_CODES").is_ok_and(|v| v.trim() == "1"));
        let dev_codes = dev_forced.unwrap_or(smtp.is_none());
        tracing::info!(%path, dev_codes, smtp = smtp.is_some(), "user directory opened");
        Ok(Self {
            db: Mutex::new(db),
            dev_forced,
            otp_key: random_key(),
            smtp: std::sync::RwLock::new(smtp),
            smtp_source: std::sync::RwLock::new(source),
            recent: Mutex::new(Default::default()),
            links: Mutex::new(std::collections::HashMap::new()),
        })
    }

    // ---- email verification ----

    /// Generate + store a 6-digit code for `email`. Returns the code
    /// only when running in dev mode (so the UI can show it); otherwise
    /// returns None and the code is delivered out of band.
    pub fn request_code(&self, email: &str) -> Result<Option<String>, DirError> {
        let (email, code) = self.issue_code(email)?;
        self.send_email_code(&email, &code);
        Ok(if self.dev_codes() { Some(code) } else { None })
    }

    /// Like [`Self::request_code`], but waits for the mail server. When the
    /// e-mail can't be sent, the code goes to the server console and the admin
    /// panel (the operator can pass it on) and `delivery_failed` says so.
    pub async fn request_code_delivered(&self, email: &str) -> Result<CodeDelivery, DirError> {
        self.request_code_delivered_ex(email, None, None).await
    }

    /// As `request_code_delivered`; with the app's `nonce` and the address
    /// the app reached this relay at (`link_base`), the e-mail also carries
    /// an "It's me" link: tapping it lets that app fill the code in itself
    /// (`poll_link`).
    pub async fn request_code_delivered_ex(&self, email: &str, nonce: Option<&str>, link_base: Option<&str>) -> Result<CodeDelivery, DirError> {
        let (email, code, link) = self.prepare_code(email, nonce, link_base)?;
        self.deliver_code(email, code, link).await
    }

    /// Mail is configured here (not dev mode).
    pub fn has_smtp(&self) -> bool {
        self.smtp.read().map(|c| c.is_some()).unwrap_or(false) && !self.dev_codes()
    }

    /// Send a sign-up code with this relay's mail server (for itself, or for
    /// a relay of the network that has none).
    pub async fn send_code_mail(&self, to: &str, code: &str, link: Option<&str>) -> anyhow::Result<()> {
        let cfg = self.smtp.read().ok().and_then(|c| c.clone()).ok_or_else(|| anyhow::anyhow!("no mail server here"))?;
        tokio::time::timeout(std::time::Duration::from_secs(60), send_smtp_any_port(&cfg, to, code, link))
            .await
            .map_err(|_| anyhow::anyhow!("the mail server didn't answer in time"))?
    }

    /// A fresh code for `email` (and its "It's me" link) → (email, code, link).
    pub fn prepare_code(&self, email: &str, nonce: Option<&str>, link_base: Option<&str>) -> Result<(String, String, Option<String>), DirError> {
        let (email, code) = self.issue_code(email)?;
        let link = match (nonce.map(str::trim).filter(|n| n.len() >= 16 && n.len() <= 128), link_base) {
            (Some(nonce), Some(base)) => {
                let mut t = [0u8; 18];
                OsRng.fill_bytes(&mut t);
                let token = hex::encode(t);
                if let Ok(mut m) = self.links.lock() {
                    let now = now_secs();
                    m.retain(|_, l| now.saturating_sub(l.at) <= CODE_TTL_SECS && l.email != email);
                    m.insert(token.clone(), MagicLink { email: email.clone(), code: code.clone(), nonce: nonce.to_string(), confirmed: false, at: now });
                }
                Some(format!("{}/v1/auth/email/link/{token}", base.trim_end_matches('/')))
            }
            _ => None,
        };
        Ok((email, code, link))
    }

    /// Mail a prepared code (or, in dev mode, show it in the panel).
    pub async fn deliver_code(&self, email: String, code: String, link: Option<String>) -> Result<CodeDelivery, DirError> {
        let cfg = self.smtp.read().ok().and_then(|c| c.clone());
        let Some(cfg) = cfg else {
            self.send_email_code(&email, &code);
            return Ok(CodeDelivery { dev_code: self.dev_codes().then_some(code), delivery_failed: false, error: None });
        };
        let res = tokio::time::timeout(std::time::Duration::from_secs(60), send_smtp_any_port(&cfg, &email, &code, link.as_deref())).await;
        let err = match res {
            Ok(Ok(())) => {
                tracing::info!(%email, "verification code emailed");
                return Ok(CodeDelivery { dev_code: self.dev_codes().then_some(code), delivery_failed: false, error: None });
            }
            Ok(Err(e)) => e.to_string(),
            Err(_) => "the mail server didn't answer in time".to_string(),
        };
        tracing::error!(%email, error = %err, "SMTP send failed");
        tracing::warn!(%email, %code, "📧 E-MAIL NOT SENT — give this code to the user (also in the admin panel)");
        if let Ok(mut r) = self.recent.lock() {
            r.push_front((email.clone(), code.clone(), now_secs()));
            r.truncate(20);
        }
        Ok(CodeDelivery { dev_code: self.dev_codes().then_some(code), delivery_failed: true, error: Some(err) })
    }

    /// A code another relay of the network mailed after all: off the panel's
    /// "couldn't be sent" list.
    pub fn forget_unsent(&self, email: &str, code: &str) {
        if let Ok(mut r) = self.recent.lock() {
            r.retain(|c| !(c.0 == email && c.1 == code));
        }
    }

    /// The e-mail an "It's me" link is for (still valid), masked for the page.
    pub fn link_email(&self, token: &str) -> Option<String> {
        let m = self.links.lock().ok()?;
        let l = m.get(token).filter(|l| now_secs().saturating_sub(l.at) <= CODE_TTL_SECS)?;
        let (user, host) = l.email.split_once('@')?;
        let shown: String = user.chars().take(2).collect();
        Some(format!("{shown}•••@{host}"))
    }

    /// The link was tapped: the app waiting on it gets the code.
    pub fn confirm_link(&self, token: &str) -> bool {
        let Ok(mut m) = self.links.lock() else { return false };
        match m.get_mut(token) {
            Some(l) if now_secs().saturating_sub(l.at) <= CODE_TTL_SECS => { l.confirmed = true; true }
            _ => false,
        }
    }

    /// The app asks whether its link was tapped: the code, once it was.
    /// Only the app that asked for the code (its `nonce`) gets it.
    pub fn poll_link(&self, email: &str, nonce: &str) -> Option<String> {
        let email = email.trim().to_lowercase();
        let m = self.links.lock().ok()?;
        m.values()
            .find(|l| l.email == email && l.nonce == nonce && l.confirmed && now_secs().saturating_sub(l.at) <= CODE_TTL_SECS)
            .map(|l| l.code.clone())
    }

    /// Generate and store a fresh code for `email` → (normalized email, code).
    fn issue_code(&self, email: &str) -> Result<(String, String), DirError> {
        let email = email.trim().to_lowercase();
        if !email.contains('@') || email.len() < 5 {
            return Err(DirError::BadCode);
        }
        // Uniform 6 digits (rejection sampling: `% 1_000_000` of a u32 is
        // slightly biased toward low codes).
        let code = loop {
            let mut n = [0u8; 4];
            OsRng.fill_bytes(&mut n);
            let v = u32::from_be_bytes(n);
            if v < 4_294_000_000 {
                break format!("{:06}", v % 1_000_000);
            }
        };
        let now = now_secs();
        {
            let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
            // AQUA-SEC-005: a fresh code also resets the wrong-code count, so
            // codes per address are limited — or guessing just asks again.
            // In explicit dev mode the code goes back to whoever asks, so a
            // limit would protect nothing (and tests ask many times).
            if self.dev_codes() {
            } else if let Ok((issued_at, sends, sends_at)) = db.query_row(
                "SELECT issued_at, sends, sends_at FROM email_codes WHERE email=?1",
                rusqlite::params![email],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)),
            ) {
                if now.saturating_sub(issued_at as u64) < CODE_RESEND_SECS {
                    return Err(DirError::TooManyCodes);
                }
                if now.saturating_sub(sends_at as u64) < 3600 && sends >= CODE_MAX_PER_HOUR {
                    return Err(DirError::TooManyCodes);
                }
            }
            db.execute(
                "INSERT INTO email_codes (email, code, ticket, issued_at, verified_at, fails, sends, sends_at)
                 VALUES (?1, ?2, NULL, ?3, NULL, 0, 1, ?3)
                 ON CONFLICT(email) DO UPDATE SET code=?2, ticket=NULL, issued_at=?3, verified_at=NULL, mfa=0, mfa_fail=0, fails=0,
                   sends=CASE WHEN ?3 - sends_at < 3600 THEN sends + 1 ELSE 1 END,
                   sends_at=CASE WHEN ?3 - sends_at < 3600 THEN sends_at ELSE ?3 END",
                rusqlite::params![email, self.code_tag(&email, &code), now as i64],
            )?;
        }
        Ok((email, code))
    }

    /// Check a code; on success mint + store a one-time ticket.
    pub fn verify_code(&self, email: &str, code: &str) -> Result<String, DirError> {
        let email = email.trim().to_lowercase();
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let row = db
            .query_row(
                "SELECT code, issued_at, fails FROM email_codes WHERE email=?1",
                rusqlite::params![email],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)),
            )
            .map_err(|_| DirError::NoCode)?;
        let (stored, issued_at, fails) = row;
        // Used (emptied) or voided by too many wrong tries.
        if stored.is_empty() {
            return Err(DirError::BadCode);
        }
        if fails >= CODE_MAX_FAILS {
            return Err(DirError::TooManyTries);
        }
        if now_secs().saturating_sub(issued_at as u64) > CODE_TTL_SECS {
            return Err(DirError::BadCode);
        }
        let want = self.code_tag(&email, code);
        if !subtle_eq(want.as_bytes(), stored.as_bytes()) {
            db.execute("UPDATE email_codes SET fails = fails + 1 WHERE email=?1", rusqlite::params![email])?;
            return Err(DirError::BadCode);
        }
        let mut t = [0u8; 18];
        OsRng.fill_bytes(&mut t);
        let ticket = hex::encode(t);
        // The code is used up: one code, one ticket (AQUA-SEC-005).
        db.execute(
            "UPDATE email_codes SET code='', ticket=?1, verified_at=?2, mfa=0, mfa_fail=0, fails=0 WHERE email=?3",
            rusqlite::params![ticket, now_secs() as i64, email],
        )?;
        Ok(ticket)
    }

    /// Deliver the verification code. Real SMTP when configured,
    /// otherwise a dev log line.
    fn send_email_code(&self, email: &str, code: &str) {
        let cfg = self.smtp.read().ok().and_then(|c| c.clone());
        let Some(cfg) = cfg else {
            tracing::warn!(%email, %code, "📧 EMAIL VERIFICATION CODE (dev mode — no real mail sent)");
            if let Ok(mut r) = self.recent.lock() {
                r.push_front((email.to_string(), code.to_string(), now_secs()));
                r.truncate(20);
            }
            return;
        };
        let email = email.to_string();
        let code = code.to_string();
        // Fire-and-forget on the tokio runtime; failures are logged.
        tokio::spawn(async move {
            if let Err(e) = send_smtp_any_port(&cfg, &email, &code, None).await {
                tracing::error!(%email, error = %e, "SMTP send failed");
            } else {
                tracing::info!(%email, "verification code emailed");
            }
        });
    }

    /// Recent dev-mode codes, newest first (admin panel).
    pub fn recent_codes(&self) -> Vec<(String, String, u64)> {
        // A code past its 10 minutes is no use to anyone: off the panel.
        let now = now_secs();
        self.recent.lock().map(|r| r.iter().filter(|c| now.saturating_sub(c.2) <= CODE_TTL_SECS).cloned().collect()).unwrap_or_default()
    }

    /// True when codes go back in the API answer (explicit dev mode only).
    pub fn dev_codes(&self) -> bool {
        self.dev_forced.unwrap_or(false)
    }

    /// Current SMTP settings (password included — callers must not leak it)
    /// and where they came from.
    pub fn smtp(&self) -> (Option<SmtpConfig>, &'static str) {
        let c = self.smtp.read().ok().and_then(|c| c.clone());
        let src = self.smtp_source.read().map(|s| *s).unwrap_or("none");
        (c, src)
    }

    /// Replace the SMTP settings at runtime (admin panel) and persist them.
    /// `None` switches back to dev mode and deletes the file.
    pub fn set_smtp(&self, cfg: Option<SmtpConfig>) -> anyhow::Result<()> {
        match &cfg {
            Some(c) => std::fs::write(SMTP_FILE, serde_json::to_string_pretty(c)?)?,
            None => { let _ = std::fs::remove_file(SMTP_FILE); }
        }
        if let Ok(mut s) = self.smtp_source.write() { *s = if cfg.is_some() { "file" } else { "none" }; }
        if let Ok(mut s) = self.smtp.write() { *s = cfg; }
        Ok(())
    }

    /// Send a sample code e-mail with the current settings (admin "Send test").
    pub async fn send_test_email(&self, to: &str) -> anyhow::Result<()> {
        let cfg = self.smtp.read().ok().and_then(|c| c.clone())
            .ok_or_else(|| anyhow::anyhow!("e-mail delivery is not configured"))?;
        send_smtp_any_port(&cfg, to.trim(), "123456", None).await
    }

    // ---- registration ----

    /// Validate a username slug.
    /// A @username as it may appear in a path (federation).
    pub fn valid_name(u: &str) -> bool { Self::valid_username(u) }

    fn valid_username(u: &str) -> bool {
        let len = u.chars().count();
        len >= 3 && len <= 32
            && u.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.')
    }

    /// Register (or re-confirm) a username for an identity. Requires a
    /// valid e-mail ticket. If the username already exists and belongs
    /// to this identity, the profile is updated; if it belongs to
    /// someone else, this fails.
    #[allow(clippy::too_many_arguments)]
    pub fn register(
        &self,
        username: &str,
        identity_dh: &[u8; 32],
        ticket: &str,
        display_name: &str,
        about: &str,
        phone: &str,
    ) -> Result<(), DirError> {
        let username = username.trim().to_lowercase();
        if !Self::valid_username(&username) {
            return Err(DirError::BadUsername);
        }
        let db = self.db.lock().unwrap();

        // Resolve the ticket → email.
        let (email, verified_at): (String, Option<i64>) = db
            .query_row(
                "SELECT email, verified_at FROM email_codes WHERE ticket=?1",
                rusqlite::params![ticket],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|_| DirError::BadTicket)?;
        match verified_at {
            Some(ts) if now_secs().saturating_sub(ts as u64) <= TICKET_TTL_SECS => {}
            _ => return Err(DirError::BadTicket),
        }

        // Is this email already bound to a different username?
        if let Ok(existing_user) = db.query_row(
            "SELECT username FROM users WHERE email=?1",
            rusqlite::params![email],
            |r| r.get::<_, String>(0),
        ) {
            if existing_user != username {
                return Err(DirError::EmailTaken);
            }
        }

        // Someone's old name, still forwarding to them: not free yet.
        let reserved: bool = db
            .query_row(
                "SELECT 1 FROM renames WHERE old_name=?1 AND at > ?2",
                rusqlite::params![username, now_secs() as i64 - RENAME_KEEP_SECS as i64],
                |_| Ok(()),
            )
            .is_ok();
        if reserved || Self::reserved_elsewhere(&db, &username, &uid_for(identity_dh)) {
            return Err(DirError::UsernameTaken);
        }

        // Username free, or already ours?
        if let Ok(owner) = db.query_row(
            "SELECT identity_dh FROM users WHERE username=?1",
            rusqlite::params![username],
            |r| r.get::<_, Vec<u8>>(0),
        ) {
            if owner.as_slice() != identity_dh.as_slice() {
                return Err(DirError::UsernameTaken);
            }
        }

        db.execute(
            "INSERT INTO users (username, identity_dh, email, display_name, about, phone, is_bot, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7)
             ON CONFLICT(username) DO UPDATE SET
                 email=?3, display_name=?4, about=?5, phone=?6",
            rusqlite::params![
                username,
                identity_dh.as_slice(),
                email,
                display_name.trim(),
                about.trim(),
                phone.trim(),
                now_secs() as i64,
            ],
        )?;
        Self::fill_uids(&db);
        // Consume the ticket so it can't be reused.
        let _ = db.execute("UPDATE email_codes SET ticket=NULL WHERE ticket=?1", rusqlite::params![ticket]);
        tracing::info!(%username, "user registered");
        Ok(())
    }

    /// Update the profile of a username the caller owns.
    pub fn update_profile(
        &self,
        username: &str,
        identity_dh: &[u8; 32],
        display_name: &str,
        about: &str,
        phone: &str,
    ) -> Result<(), DirError> {
        self.update_profile_full(username, identity_dh, display_name, about, phone, None)
    }

    /// Like [`update_profile`](Self::update_profile), optionally replacing the
    /// public `extra` JSON object (banner colours, status, pronouns, …).
    pub fn update_profile_full(
        &self,
        username: &str,
        identity_dh: &[u8; 32],
        display_name: &str,
        about: &str,
        phone: &str,
        extra: Option<&serde_json::Value>,
    ) -> Result<(), DirError> {
        let username = username.trim().to_lowercase();
        let db = self.db.lock().unwrap();
        Self::check_owner(&db, &username, identity_dh)?;
        db.execute(
            "UPDATE users SET display_name=?2, about=?3, phone=?4 WHERE username=?1",
            rusqlite::params![username, display_name.trim(), about.trim(), phone.trim()],
        )?;
        if let Some(extra) = extra {
            if !extra.is_object() {
                return Err(DirError::BadProfile("extra must be a JSON object".into()));
            }
            let text = extra.to_string();
            if text.len() > PROFILE_EXTRA_LIMIT {
                return Err(DirError::BadProfile("profile extras are too large".into()));
            }
            db.execute("UPDATE users SET extra=?2 WHERE username=?1", rusqlite::params![username, text])?;
        }
        Ok(())
    }

    /// The account's primary device or any of its extra devices (a phone
    /// signed in next to the PC may change the avatar too).
    fn check_owner(db: &Connection, username: &str, identity_dh: &[u8; 32]) -> Result<(), DirError> {
        let (owner, devices): (Vec<u8>, Option<String>) = db
            .query_row(
                "SELECT identity_dh, devices FROM users WHERE username=?1",
                rusqlite::params![username],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|_| DirError::NotOwner)?;
        if owner.as_slice() == identity_dh.as_slice() {
            return Ok(());
        }
        let hex = hex::encode(identity_dh);
        let list: Vec<Device> = devices.as_deref().and_then(|d| serde_json::from_str(d).ok()).unwrap_or_default();
        if list.iter().any(|d| d.dh == hex) {
            return Ok(());
        }
        Err(DirError::NotOwner)
    }

    // ---- profile images (avatar / banner) ----

    /// Store the owner's avatar or banner. Returns the new version number.
    /// Images are public: anyone can fetch them, like the rest of the card.
    pub fn set_media(
        &self,
        username: &str,
        identity_dh: &[u8; 32],
        kind: MediaKind,
        mime: &str,
        data: &[u8],
    ) -> Result<i64, DirError> {
        let username = username.trim().to_lowercase();
        let mime = mime.trim().to_ascii_lowercase();
        let mb = |n: usize| n / 1024 / 1024;
        let pic = kind.is_avatar() || kind.is_banner();
        let limit = match (kind, mime.as_str()) {
            (_, "image/png" | "image/jpeg" | "image/webp") if pic => IMAGE_MEDIA_LIMIT,
            // Animated: GIF anywhere, short silent video for banners — light only.
            (_, "image/gif") if pic => ANIMATED_MEDIA_LIMIT,
            (_, "video/webm" | "video/mp4") if kind.is_banner() => ANIMATED_MEDIA_LIMIT,
            (MediaKind::Music, "audio/mpeg" | "audio/ogg" | "audio/opus" | "audio/mp4" | "audio/aac" | "audio/flac" | "audio/wav" | "audio/webm") => PROFILE_MEDIA_LIMIT,
            _ => return Err(DirError::BadProfile(format!("unsupported type {mime:?} for {}", kind.as_str()))),
        };
        if data.is_empty() || data.len() > limit {
            return Err(DirError::BadProfile(format!("file is empty or larger than {} MB", mb(limit))));
        }
        let db = self.db.lock().unwrap();
        Self::check_owner(&db, &username, identity_dh)?;
        db.execute(
            "INSERT INTO profile_media (username, kind, mime, data) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(username, kind) DO UPDATE SET mime=?3, data=?4",
            rusqlite::params![username, kind.as_str(), mime, data],
        )?;
        let Some(col) = kind.ver_col() else { return Ok(now_secs() as i64) };
        let old: i64 = db
            .query_row(&format!("SELECT {col} FROM users WHERE username=?1"), rusqlite::params![username], |r| r.get(0))
            .unwrap_or(0);
        let ver = (now_secs() as i64).max(old + 1);
        db.execute(&format!("UPDATE users SET {col}=?2 WHERE username=?1"), rusqlite::params![username, ver])?;
        Ok(ver)
    }

    /// Remove the owner's avatar or banner.
    pub fn clear_media(&self, username: &str, identity_dh: &[u8; 32], kind: MediaKind) -> Result<(), DirError> {
        let username = username.trim().to_lowercase();
        let db = self.db.lock().unwrap();
        Self::check_owner(&db, &username, identity_dh)?;
        db.execute("DELETE FROM profile_media WHERE username=?1 AND kind=?2", rusqlite::params![username, kind.as_str()])?;
        if let Some(col) = kind.ver_col() {
            db.execute(&format!("UPDATE users SET {col}=0 WHERE username=?1"), rusqlite::params![username])?;
        }
        Ok(())
    }

    /// Set (or change) the account password. Owner only.
    pub fn set_password(&self, username: &str, identity_dh: &[u8; 32], password: &str) -> Result<(), DirError> {
        if password.chars().count() < 8 {
            return Err(DirError::WeakPassword);
        }
        let username = username.trim().to_lowercase();
        let db = self.db.lock().unwrap();
        Self::check_owner(&db, &username, identity_dh)?;
        db.execute("UPDATE users SET password=?2 WHERE username=?1", rusqlite::params![username, hash_password(password)])?;
        Ok(())
    }

    /// Sign in on a new device: an email ticket (proves the inbox) plus the
    /// password (if the account has one). Moves the username to this
    /// device's identity key and returns the username. Like Telegram /
    /// Discord: whoever controls the email + password owns the account.
    /// Check an e-mail ticket + password without changing anything.
    /// Returns (username, email).
    pub fn check_login(&self, ticket: &str, password: &str) -> Result<(String, String), DirError> {
        let db = self.db.lock().unwrap();
        let (email, verified_at): (String, Option<i64>) = db
            .query_row(
                "SELECT email, verified_at FROM email_codes WHERE ticket=?1",
                rusqlite::params![ticket],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|_| DirError::BadTicket)?;
        match verified_at {
            Some(ts) if now_secs().saturating_sub(ts as u64) <= TICKET_TTL_SECS => {}
            _ => return Err(DirError::BadTicket),
        }
        let (username, stored): (String, String) = db
            .query_row(
                "SELECT username, password FROM users WHERE email=?1 AND is_bot=0",
                rusqlite::params![email],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|_| DirError::BadLogin)?;
        if !stored.is_empty() && !verify_password(password, &stored) {
            return Err(DirError::BadLogin);
        }
        Ok((username, email))
    }

    /// Sign in on another device: the device is ADDED to the account (each
    /// device keeps its own keys; the old ones stay signed in). With all
    /// device slots taken, the oldest extra device makes room.
    pub fn login(&self, ticket: &str, password: &str, identity_dh: &[u8; 32]) -> Result<String, DirError> {
        self.login_named(ticket, password, identity_dh, "")
    }

    pub fn login_named(&self, ticket: &str, password: &str, identity_dh: &[u8; 32], name: &str) -> Result<String, DirError> {
        self.login_checked(ticket, password, identity_dh, name, "", "")
    }

    /// Sign-up without e-mail: an @username and a password. The account is
    /// bound to this device's identity key; it can be moved to other
    /// devices only after two-step verification is on ([`Self::sign_in_plain`]).
    pub fn register_plain(&self, username: &str, identity_dh: &[u8; 32], password: &str, display_name: &str) -> Result<(), DirError> {
        let username = username.trim().to_lowercase();
        if !Self::valid_username(&username) {
            return Err(DirError::BadUsername);
        }
        if password.chars().count() < 8 {
            return Err(DirError::WeakPassword);
        }
        let db = self.db.lock().unwrap();
        let reserved = db
            .query_row(
                "SELECT 1 FROM renames WHERE old_name=?1 AND at > ?2",
                rusqlite::params![username, now_secs() as i64 - RENAME_KEEP_SECS as i64],
                |_| Ok(()),
            )
            .is_ok();
        if reserved {
            return Err(DirError::UsernameTaken);
        }
        if let Ok(owner) = db.query_row("SELECT identity_dh FROM users WHERE username=?1", rusqlite::params![username], |r| r.get::<_, Vec<u8>>(0)) {
            if owner.as_slice() != identity_dh.as_slice() {
                return Err(DirError::UsernameTaken);
            }
        }
        // One account per identity key — and which one, so the app can say
        // so (it said "@new is taken", which looked like a broken sign-up).
        if let Ok(other) = db.query_row("SELECT username FROM users WHERE identity_dh=?1", rusqlite::params![identity_dh.as_slice()], |r| r.get::<_, String>(0)) {
            if other != username {
                return Err(DirError::KeyHasAccount(other));
            }
        }
        if Self::reserved_elsewhere(&db, &username, &uid_for(identity_dh)) {
            return Err(DirError::UsernameTaken);
        }
        db.execute(
            "INSERT INTO users (username, identity_dh, email, display_name, about, phone, is_bot, created_at, password)
             VALUES (?1, ?2, NULL, ?3, '', '', 0, ?4, ?5)
             ON CONFLICT(username) DO UPDATE SET display_name=?3, password=?5",
            rusqlite::params![username, identity_dh.as_slice(), display_name.trim(), now_secs() as i64, hash_password(password)],
        )?;
        Self::fill_uids(&db);
        tracing::info!(%username, "user registered (no e-mail)");
        Ok(())
    }

    /// Join a relay with an account of the network (a checked pass): the
    /// same @name and ID as there, this device's key. The password is
    /// optional (the account lives on this device until 2FA is on here).
    pub fn register_joined(&self, username: &str, uid: &str, identity_dh: &[u8; 32], password: &str, display_name: &str) -> Result<(), DirError> {
        let username = username.trim().to_lowercase();
        if !Self::valid_username(&username) {
            return Err(DirError::BadUsername);
        }
        if !password.is_empty() && password.chars().count() < 8 {
            return Err(DirError::WeakPassword);
        }
        let uid = if uid.len() == 10 && uid.bytes().all(|b| b.is_ascii_digit()) { uid.to_string() } else { uid_for(identity_dh) };
        let db = self.db.lock().unwrap();
        if let Ok(owner) = db.query_row("SELECT identity_dh FROM users WHERE username=?1", rusqlite::params![username], |r| r.get::<_, Vec<u8>>(0)) {
            // AQUA-SEC-002: joining never replaces the key an account already
            // has here (before 1.21.0-21 a matching ID was enough, so anyone
            // with a pass wrote their key over the account's own and the
            // change spread through the network).
            if owner.as_slice() != identity_dh.as_slice() {
                return Err(DirError::UsernameTaken);
            }
        }
        if let Ok(other) = db.query_row("SELECT username FROM users WHERE identity_dh=?1", rusqlite::params![identity_dh.as_slice()], |r| r.get::<_, String>(0)) {
            if other != username {
                return Err(DirError::UsernameTaken);
            }
        }
        if Self::reserved_elsewhere(&db, &username, &uid) {
            return Err(DirError::UsernameTaken);
        }
        let pw = if password.is_empty() { String::new() } else { hash_password(password) };
        db.execute(
            "INSERT INTO users (username, identity_dh, email, display_name, about, phone, is_bot, created_at, password, uid)
             VALUES (?1, ?2, NULL, ?3, '', '', 0, ?4, ?5, ?6)
             ON CONFLICT(username) DO UPDATE SET display_name=?3, password=CASE WHEN ?5 = '' THEN password ELSE ?5 END, uid=?6",
            rusqlite::params![username, identity_dh.as_slice(), display_name.trim(), now_secs() as i64, pw, uid],
        )?;
        tracing::info!(%username, %uid, "joined with an account of the network");
        Ok(())
    }

    /// Sign in on another device with @username + password. Only accounts
    /// with two-step verification can do this (and need the code, a backup
    /// code or the recovery key); without it an account stays on the device
    /// it was made on — there is no way to recover it.
    pub fn sign_in_plain(&self, username: &str, password: &str, identity_dh: &[u8; 32], name: &str, code: &str, recovery: &str) -> Result<String, DirError> {
        self.sign_in_plain_tg(username, password, identity_dh, name, code, recovery, None)
    }

    /// [`Self::sign_in_plain`]; `tg` — the Telegram user id a `tglogin`
    /// ticket confirmed (a second factor when it's the linked one).
    #[allow(clippy::too_many_arguments)]
    pub fn sign_in_plain_tg(&self, username: &str, password: &str, identity_dh: &[u8; 32], name: &str, code: &str, recovery: &str, tg: Option<&str>) -> Result<String, DirError> {
        // Accounts from before 1.2.0-1 may still sign in with their e-mail.
        let login = username.trim().to_lowercase();
        let username = if !login.starts_with('@') && login.contains('@') {
            let db = self.db.lock().unwrap();
            db.query_row("SELECT username FROM users WHERE email=?1 AND is_bot=0", rusqlite::params![login], |r| r.get::<_, String>(0))
                .unwrap_or(login)
        } else {
            login.trim_start_matches('@').to_string()
        };
        let now = now_secs();
        {
            let mut f = name_fails().lock().unwrap();
            f.retain(|_, (_, at)| now.saturating_sub(*at) < NAME_FAIL_WINDOW);
            if f.get(&username).is_some_and(|(n, _)| *n >= MAX_NAME_FAILS) {
                return Err(DirError::TooManyTries);
            }
        }
        let fail = |e: DirError| -> DirError {
            let mut f = name_fails().lock().unwrap();
            let v = f.entry(username.clone()).or_insert((0, now));
            v.0 += 1;
            v.1 = now;
            e
        };
        let db = self.db.lock().unwrap();
        let row: Option<(String, String, String)> = db
            .query_row(
                "SELECT password, security, recovery FROM users WHERE username=?1 AND is_bot=0",
                rusqlite::params![username],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .ok();
        let Some((stored, json, rec)) = row else { return Err(fail(DirError::BadNameLogin)) };
        if stored.is_empty() || !verify_password(password, &stored) {
            return Err(fail(DirError::BadNameLogin));
        }
        drop(db);
        if self.device_role(&username, identity_dh).is_some() {
            return Ok(username);
        }
        let db = self.db.lock().unwrap();
        let mut sec: Security = serde_json::from_str(&json).unwrap_or_default();
        if !sec.portable() {
            // These keys are another account's here (renamed in the network,
            // another name than remembered): say whose, not "another device".
            drop(db);
            if let Some(other) = self.username_for_identity(identity_dh).filter(|o| *o != username) {
                return Err(DirError::KeyHasAccount(other));
            }
            return Err(DirError::DeviceBound);
        }
        let by_recovery = !recovery.trim().is_empty();
        let by_tg = tg.is_some_and(|t| !sec.tg.is_empty() && t == sec.tg);
        if by_recovery {
            if rec.is_empty() || !subtle_eq(recovery_hash(recovery.trim()).as_bytes(), rec.as_bytes()) {
                return Err(fail(DirError::BadRecovery));
            }
        } else if by_tg {
            if sec.lock {
                return Err(DirError::DevicesLocked);
            }
        } else {
            if sec.lock {
                return Err(DirError::DevicesLocked);
            }
            if tg.is_some() {
                return Err(fail(DirError::WrongTelegram));
            }
            if sec.totp.is_empty() {
                return Err(DirError::NeedTelegram);
            }
            if code.trim().is_empty() {
                return Err(DirError::NeedTotp);
            }
            if !Self::check_code(&mut sec, code) {
                return Err(fail(DirError::BadTotp));
            }
            Self::put_security(&db, &username, &sec)?;
        }
        drop(db);
        self.add_device(&username, identity_dh, name)?;
        name_fails().lock().unwrap().remove(&username);
        tracing::info!(%username, "signed in on another device (no e-mail)");
        Ok(username)
    }

    /// `name` belongs to an account of another relay (name registry).
    fn reserved_elsewhere(db: &Connection, name: &str, my_uid: &str) -> bool {
        db.query_row("SELECT uid FROM fed_names WHERE username=?1", rusqlite::params![name], |r| r.get::<_, String>(0))
            .is_ok_and(|uid| uid != my_uid)
            || db.query_row("SELECT 1 FROM group_names WHERE username=?1", rusqlite::params![name], |_| Ok(())).is_ok()
    }

    /// A public group / channel takes `name` (its owner, from their account):
    /// free in the whole namespace (people, bots, the network's names, other
    /// groups), or already this group's. Its other name, if any, goes.
    pub fn claim_group_name(&self, name: &str, gid: &str, title: &str, kind: &str, owner: &str) -> Result<String, DirError> {
        let name = name.trim().trim_start_matches('@').to_lowercase();
        if !Self::valid_username(&name) || !gid.starts_with("grp_") || gid.len() > 64 {
            return Err(DirError::BadUsername);
        }
        let db = self.db.lock().unwrap();
        let person = db.query_row("SELECT 1 FROM users WHERE username=?1", rusqlite::params![name], |_| Ok(())).is_ok()
            || db.query_row("SELECT 1 FROM fed_names WHERE username=?1", rusqlite::params![name], |_| Ok(())).is_ok()
            || db.query_row("SELECT 1 FROM renames WHERE old_name=?1 AND at > ?2", rusqlite::params![name, now_secs() as i64 - RENAME_KEEP_SECS as i64], |_| Ok(())).is_ok();
        let holder: Option<(String, String)> = db.query_row("SELECT gid, owner FROM group_names WHERE username=?1", rusqlite::params![name], |r| Ok((r.get(0)?, r.get(1)?))).ok();
        if person || holder.as_ref().is_some_and(|(g, _)| g != gid) {
            return Err(DirError::UsernameTaken);
        }
        // This group's name is its owner's to change.
        if let Ok(o) = db.query_row("SELECT owner FROM group_names WHERE gid=?1", rusqlite::params![gid], |r| r.get::<_, String>(0)) {
            if o != owner { return Err(DirError::NotOwner); }
        }
        db.execute("DELETE FROM group_names WHERE gid=?1", rusqlite::params![gid])?;
        let kind = if kind == "channel" { "channel" } else { "group" };
        let title: String = title.trim().chars().take(128).collect();
        db.execute("INSERT INTO group_names (username, gid, title, kind, owner, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![name, gid, title, kind, owner, now_secs() as i64])?;
        Ok(name)
    }

    /// The group's owner gives its @name up (made private, or cleared).
    pub fn release_group_name(&self, gid: &str, owner: &str) -> bool {
        let db = self.db.lock().unwrap();
        db.execute("DELETE FROM group_names WHERE gid=?1 AND owner=?2", rusqlite::params![gid, owner]).unwrap_or(0) > 0
    }

    fn group_row(r: &rusqlite::Row) -> rusqlite::Result<serde_json::Value> {
        Ok(serde_json::json!({ "username": r.get::<_, String>(0)?, "gid": r.get::<_, String>(1)?, "title": r.get::<_, String>(2)?,
            "kind": r.get::<_, String>(3)?, "admin": r.get::<_, String>(4)? }))
    }

    /// A public group / channel by its @name.
    pub fn group_by_name(&self, name: &str) -> Option<serde_json::Value> {
        let name = name.trim().trim_start_matches('@').to_lowercase();
        let db = self.db.lock().unwrap();
        db.query_row("SELECT username, gid, title, kind, owner FROM group_names WHERE username=?1", rusqlite::params![name], Self::group_row).ok()
    }

    /// Public groups and channels whose @name or title has `q`.
    pub fn search_groups(&self, q: &str, limit: usize) -> Vec<serde_json::Value> {
        let q = q.trim().trim_start_matches('@').to_lowercase();
        if q.is_empty() { return Vec::new(); }
        let like = format!("%{}%", q.replace('%', "").replace('_', "\\_"));
        let db = self.db.lock().unwrap();
        let Ok(mut st) = db.prepare(
            "SELECT username, gid, title, kind, owner FROM group_names WHERE username LIKE ?1 ESCAPE '\\' OR lower(title) LIKE ?1 ESCAPE '\\'
             ORDER BY (username = ?2) DESC, length(username) LIMIT ?3",
        ) else { return Vec::new() };
        st.query_map(rusqlite::params![like, q, limit as i64], Self::group_row).map(|it| it.filter_map(|x| x.ok()).collect()).unwrap_or_default()
    }

    /// The name registry as this relay knows it, changed after `since`:
    /// its own accounts (and, in a network, the network's — `home` "") and
    /// the names it learnt from other relays. Oldest change first.
    pub fn names_since(&self, since: i64, limit: usize) -> Vec<NameRow> {
        self.names_after(since, None, limit)
    }

    /// Like [`Self::names_since`] from the cursor `(since, after)`: rows after
    /// it in `(updated_at, username)` order (REV-001: with `updated_at` alone a
    /// page boundary inside equal timestamps skipped rows). `after = None`
    /// is the old `> since` (relays before 1.21.0-23 send no `after`).
    pub fn names_after(&self, since: i64, after: Option<&str>, limit: usize) -> Vec<NameRow> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let Ok(mut st) = db.prepare(
            "SELECT username, uid, home, created_at, updated_at FROM (
               SELECT username, uid, '' AS home, created_at, updated_at FROM users WHERE is_bot=0 AND uid IS NOT NULL AND uid<>''
               UNION ALL
               SELECT username, uid, home, created_at, updated_at FROM fed_names)
             WHERE updated_at > ?1 OR (?3 IS NOT NULL AND updated_at = ?1 AND username > ?3)
             ORDER BY updated_at, username LIMIT ?2",
        ) else { return Vec::new() };
        st.query_map(rusqlite::params![since, limit as i64, after], |r| {
            Ok(NameRow { username: r.get(0)?, uid: r.get(1)?, home: r.get(2)?, created_at: r.get(3)?, updated_at: r.get(4)? })
        })
        .map(|it| it.flatten().collect())
        .unwrap_or_default()
    }

    /// A reserved name from another relay. The older account keeps a name
    /// (on a tie, the smaller ID): when that's theirs and an account HERE
    /// has the name, ours is renamed (`имя_` + the end of its ID) and
    /// `(old, new)` is returned so live connections follow.
    pub fn fed_apply_name(&self, n: &NameRow, home: &str) -> Option<(String, String)> {
        let name = n.username.trim().to_lowercase();
        if !Self::valid_username(&name) || n.uid.len() != 10 || !n.uid.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let mut db = self.db.lock().unwrap();
        // One of ours (it came back to us through another relay).
        if db.query_row("SELECT 1 FROM users WHERE uid=?1", rusqlite::params![n.uid], |_| Ok(())).is_ok() {
            return None;
        }
        // A deleted account's name (A7): not taken again, and it holds no
        // claim on the name against anyone here.
        if Self::tombed(&db, &n.uid, n.created_at) {
            return None;
        }
        let home = if n.home.is_empty() { home } else { n.home.as_str() };
        // A8: a claim on a name one of ours holds. An older claim renamed our
        // account, and any relay of the registry could make one up ("created
        // at 0"). A real older claim is known before a name is taken here
        // (sign-up checks the registry), except when both were taken at
        // nearly the same time on two relays: only then — both made within
        // NAME_RACE_SECS of each other, ours not older than that — does the
        // older one win. Otherwise ours keeps the name, and the claim isn't
        // recorded (it would be handed on to the rest of the registry).
        let holder: Option<(String, i64, i64)> = db
            .query_row("SELECT uid, created_at, is_bot FROM users WHERE username=?1", rusqlite::params![name], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .ok();
        if let Some((local_uid, local_created, bot)) = &holder {
            let we_win = *bot != 0 || (*local_created, local_uid.as_str()) <= (n.created_at, n.uid.as_str());
            let race = (local_created - n.created_at).abs() <= NAME_RACE_SECS && now_secs() as i64 - local_created <= NAME_RACE_SECS;
            if we_win || !race {
                if !we_win {
                    tracing::warn!(%name, claim_uid = %n.uid, "name registry: an older claim on a name taken here — ours keeps it");
                }
                return None;
            }
        }
        // Renamed there: its old name is free again.
        let _ = db.execute("DELETE FROM fed_names WHERE uid=?1 AND username<>?2", rusqlite::params![n.uid, name]);
        let _ = db.execute(
            "INSERT INTO fed_names (username, uid, home, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(username) DO UPDATE SET uid=excluded.uid, home=excluded.home, created_at=excluded.created_at, updated_at=excluded.updated_at
             WHERE fed_names.uid = excluded.uid AND fed_names.home <> excluded.home
                OR excluded.created_at < fed_names.created_at
                OR (excluded.created_at = fed_names.created_at AND excluded.uid < fed_names.uid)",
            rusqlite::params![name, n.uid, home, n.created_at, crate::now_ms()],
        );
        let (local_uid, _, _) = holder?;
        let to = Self::displaced_name(&db, &name, &local_uid);
        if Self::move_account(&mut db, &name, &to, false).is_err() {
            return None;
        }
        tracing::warn!(user = %name, %to, %home, "name registry: an older account elsewhere has this name — ours is renamed");
        Some((name, to))
    }

    /// The account's signed device list (JSON), if one was published.
    pub fn devlist(&self, username: &str) -> Option<String> {
        let db = self.db.lock().unwrap();
        db.query_row("SELECT devlist FROM users WHERE username=?1", rusqlite::params![username], |r| r.get::<_, String>(0))
            .ok()
            .filter(|s| !s.is_empty())
    }

    /// Keep the Ed25519 key an X25519 identity proved at sign-in. `Ok(true)`
    /// when it is the kept key (or the first one), `Ok(false)` when this
    /// X25519 key is already bound to a different Ed25519 key.
    pub fn bind_identity_sign(&self, dh: &[u8; 32], sign: &[u8; 32]) -> Result<bool, DirError> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        db.execute(
            "INSERT INTO identity_signs (dh, sign, at) VALUES (?1, ?2, ?3) ON CONFLICT(dh) DO NOTHING",
            rusqlite::params![dh.as_slice(), sign.as_slice(), now_secs() as i64],
        )?;
        let kept: Vec<u8> = db.query_row(
            "SELECT sign FROM identity_signs WHERE dh=?1",
            rusqlite::params![dh.as_slice()],
            |r| r.get(0),
        )?;
        Ok(kept.as_slice() == sign.as_slice())
    }

    /// The Ed25519 key `dh` proved it belongs with, if it ever signed in here.
    pub fn identity_sign(&self, dh: &[u8; 32]) -> Option<[u8; 32]> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        db.query_row("SELECT sign FROM identity_signs WHERE dh=?1", rusqlite::params![dh.as_slice()], |r| r.get::<_, Vec<u8>>(0))
            .ok()
            .and_then(|v| v.try_into().ok())
    }

    /// Store a signed device list a device of the account published. The
    /// relay can't sign one; it only refuses obvious junk: a list for
    /// another account, a bad signature, a signer that isn't a device of the
    /// account, or an older version (unless it's a reset).
    pub fn put_devlist(&self, username: &str, json: &str) -> Result<(), DirError> {
        let list: aqua_core::devlist::SignedDeviceList = serde_json::from_str(json).map_err(|_| DirError::BadProfile("not a device list".into()))?;
        if list.user != username {
            return Err(DirError::NotOwner);
        }
        list.verify_self().map_err(|e| DirError::BadProfile(e.to_string()))?;
        let signer: [u8; 32] = hex::decode(&list.signer.dh).ok().and_then(|b| b.try_into().ok()).ok_or(DirError::BadProfile("bad signer key".into()))?;
        if self.device_role(username, &signer).is_none() {
            return Err(DirError::NotOwner);
        }
        // AQUA-SEC-001: the list is signed with the Ed25519 key it names
        // itself, so that key must be the one the signer's X25519 key proved
        // it belongs with at sign-in. Otherwise anyone holding a token for
        // the account could sign a list with a key of their own.
        let signer_sign = list.signer.sign_pub().ok_or(DirError::BadProfile("bad signer key".into()))?;
        if self.identity_sign(&signer) != Some(*signer_sign.as_bytes()) {
            return Err(DirError::NotOwner);
        }
        if let Some(cur) = self.devlist(username).and_then(|j| serde_json::from_str::<aqua_core::devlist::SignedDeviceList>(&j).ok()) {
            if list.ver <= cur.ver {
                return Err(DirError::Conflict);
            }
        }
        let db = self.db.lock().unwrap();
        db.execute("UPDATE users SET devlist=?2 WHERE username=?1", rusqlite::params![username, json])?;
        Ok(())
    }

    /// Two-step verification is on for this account.
    pub fn has_2fa(&self, username: &str) -> bool {
        let db = self.db.lock().unwrap();
        Self::security_of(&db, username).map(|s| s.portable()).unwrap_or(false)
    }

    /// Link a Telegram account (from a confirmed `tglogin` ticket).
    pub fn tg_link(&self, username: &str, tg_id: &str, tg_name: &str) -> Result<(), DirError> {
        let db = self.db.lock().unwrap();
        let mut sec = Self::security_of(&db, username)?;
        sec.tg = tg_id.to_string();
        sec.tg_name = tg_name.chars().take(64).collect();
        Self::put_security(&db, username, &sec)
    }

    /// Unlink Telegram. Without the authenticator app too, the account
    /// becomes bound to its devices again.
    pub fn tg_unlink(&self, username: &str) -> Result<(), DirError> {
        let db = self.db.lock().unwrap();
        let mut sec = Self::security_of(&db, username)?;
        sec.tg.clear();
        sec.tg_name.clear();
        Self::put_security(&db, username, &sec)
    }

    /// [`Self::login_named`] with the two-step `code` and/or the recovery
    /// key's check (see [`Self::sign_in_gate`]).
    pub fn login_checked(&self, ticket: &str, password: &str, identity_dh: &[u8; 32], name: &str, code: &str, recovery: &str) -> Result<String, DirError> {
        let (username, _) = self.check_login(ticket, password)?;
        self.sign_in_gate(ticket, &username, Some(identity_dh), code, recovery)?;
        self.add_device(&username, identity_dh, name)?;
        let db = self.db.lock().unwrap();
        // One-time ticket.
        db.execute("UPDATE email_codes SET ticket=NULL WHERE ticket=?1", rusqlite::params![ticket])?;
        Ok(username)
    }

    /// Sign-in protection for a device that isn't on the account yet:
    /// "no new devices" ([`Security::lock`]) and the two-step code. Passed
    /// once for an e-mail `ticket`, it isn't asked again for that ticket
    /// (the backup is fetched, then the device signs in). `recovery` — the
    /// recovery key's check — opens both: the way back for someone who lost
    /// every device and the phone with the codes.
    pub fn sign_in_gate(&self, ticket: &str, username: &str, identity_dh: Option<&[u8; 32]>, code: &str, recovery: &str) -> Result<(), DirError> {
        if identity_dh.is_some_and(|dh| self.device_role(username, dh).is_some()) {
            return Ok(());
        }
        let db = self.db.lock().unwrap();
        let (mfa, fails): (i64, i64) = db
            .query_row("SELECT mfa, mfa_fail FROM email_codes WHERE ticket=?1", rusqlite::params![ticket], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|_| DirError::BadTicket)?;
        if mfa == 2 {
            return Ok(());
        }
        let (json, rec): (String, String) = db
            .query_row("SELECT security, recovery FROM users WHERE username=?1", rusqlite::params![username], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|_| DirError::BadLogin)?;
        let mut sec: Security = serde_json::from_str(&json).unwrap_or_default();
        let fail = |db: &Connection| -> DirError {
            let n = fails + 1;
            let _ = db.execute("UPDATE email_codes SET mfa_fail=?2 WHERE ticket=?1", rusqlite::params![ticket, n]);
            if n >= MAX_MFA_FAILS {
                let _ = db.execute("UPDATE email_codes SET ticket=NULL WHERE ticket=?1", rusqlite::params![ticket]);
            }
            DirError::BadTotp
        };
        if fails >= MAX_MFA_FAILS {
            let _ = db.execute("UPDATE email_codes SET ticket=NULL WHERE ticket=?1", rusqlite::params![ticket]);
            return Err(DirError::BadTicket);
        }
        if !recovery.trim().is_empty() {
            if !rec.is_empty() && subtle_eq(recovery_hash(recovery.trim()).as_bytes(), rec.as_bytes()) {
                db.execute("UPDATE email_codes SET mfa=2 WHERE ticket=?1", rusqlite::params![ticket])?;
                tracing::info!(%username, "sign-in protection opened with the recovery key");
                return Ok(());
            }
            let _ = fail(&db);
            return Err(DirError::BadRecovery);
        }
        if sec.lock {
            return Err(DirError::DevicesLocked);
        }
        if sec.totp.is_empty() || mfa == 1 {
            return Ok(());
        }
        if code.trim().is_empty() {
            return Err(DirError::NeedTotp);
        }
        if !Self::check_code(&mut sec, code) {
            return Err(fail(&db));
        }
        db.execute("UPDATE users SET security=?2 WHERE username=?1", rusqlite::params![username, serde_json::to_string(&sec).unwrap_or_default()])?;
        db.execute("UPDATE email_codes SET mfa=1 WHERE ticket=?1", rusqlite::params![ticket])?;
        Ok(())
    }

    /// A two-step code or an unused backup code (which is used up).
    fn check_code(sec: &mut Security, code: &str) -> bool {
        if crate::totp::looks_like_backup(code) {
            let h = crate::totp::backup_hash(code);
            if let Some(i) = sec.backup.iter().position(|b| subtle_eq(b.as_bytes(), h.as_bytes())) {
                sec.backup.remove(i);
                return true;
            }
            return false;
        }
        match crate::totp::verify_near(&sec.totp, code, sec.last, sec.drift, 2) {
            Some(step) => {
                sec.last = step;
                sec.drift = crate::totp::drift_of(step);
                true
            }
            None => false,
        }
    }

    fn security_of(db: &Connection, username: &str) -> Result<Security, DirError> {
        let json: String = db
            .query_row("SELECT security FROM users WHERE username=?1", rusqlite::params![username], |r| r.get(0))
            .map_err(|_| DirError::NotOwner)?;
        Ok(serde_json::from_str(&json).unwrap_or_default())
    }

    fn put_security(db: &Connection, username: &str, sec: &Security) -> Result<(), DirError> {
        db.execute("UPDATE users SET security=?2 WHERE username=?1", rusqlite::params![username, serde_json::to_string(sec).unwrap_or_default()])?;
        Ok(())
    }

    pub fn security_state(&self, username: &str) -> SecurityState {
        let db = self.db.lock().unwrap();
        let sec = Self::security_of(&db, username).unwrap_or_default();
        SecurityState { lock: sec.lock, totp: !sec.totp.is_empty(), backup_left: sec.backup.len(), required: false,
            tg: (!sec.tg.is_empty()).then(|| if sec.tg_name.is_empty() { "Telegram".to_string() } else { sec.tg_name.clone() }) }
    }

    /// Turn sign-in on new devices off or back on. Turning it back on asks
    /// for a two-step code when those are on.
    pub fn set_device_lock(&self, username: &str, lock: bool, code: &str) -> Result<(), DirError> {
        let db = self.db.lock().unwrap();
        let mut sec = Self::security_of(&db, username)?;
        if sec.lock == lock {
            return Ok(());
        }
        if !lock && !sec.totp.is_empty() && !Self::check_code(&mut sec, code) {
            return Err(if code.trim().is_empty() { DirError::NeedTotp } else { DirError::BadTotp });
        }
        sec.lock = lock;
        Self::put_security(&db, username, &sec)
    }

    /// Start setting up two-step codes: a new secret (base32) for the app.
    pub fn totp_setup(&self, username: &str) -> Result<String, DirError> {
        self.totp_setup_with(username, "")
    }

    /// [`Self::totp_setup`]; with codes already on, a current `code` starts
    /// moving them to a new app (the old secret works until it's confirmed).
    pub fn totp_setup_with(&self, username: &str, code: &str) -> Result<String, DirError> {
        let db = self.db.lock().unwrap();
        let mut sec = Self::security_of(&db, username)?;
        if !sec.totp.is_empty() && !Self::check_code(&mut sec, code) {
            return Err(if code.trim().is_empty() { DirError::NeedTotp } else { DirError::BadTotp });
        }
        sec.pending = crate::totp::new_secret();
        Self::put_security(&db, username, &sec)?;
        Ok(sec.pending)
    }

    /// The first code from the app confirms the secret: two-step codes are
    /// on. Returns the backup codes (shown once).
    pub fn totp_enable(&self, username: &str, code: &str) -> Result<Vec<String>, DirError> {
        let db = self.db.lock().unwrap();
        let mut sec = Self::security_of(&db, username)?;
        if sec.pending.is_empty() {
            return Err(DirError::BadProfile("start the setup first".into()));
        }
        // The first code may come from a phone whose clock differs from
        // this relay's by minutes: look ±5 minutes and remember the gap.
        let Some(step) = crate::totp::verify_near(&sec.pending, code, 0, 0, 10) else { return Err(DirError::BadTotp) };
        let (shown, stored) = crate::totp::new_backup_codes();
        sec.totp = std::mem::take(&mut sec.pending);
        sec.last = step;
        sec.drift = crate::totp::drift_of(step);
        sec.backup = stored;
        Self::put_security(&db, username, &sec)?;
        Ok(shown)
    }

    /// Turn two-step codes off (a current code or a backup code).
    pub fn totp_disable(&self, username: &str, code: &str) -> Result<(), DirError> {
        let db = self.db.lock().unwrap();
        let mut sec = Self::security_of(&db, username)?;
        if sec.totp.is_empty() {
            return Ok(());
        }
        if !Self::check_code(&mut sec, code) {
            return Err(DirError::BadTotp);
        }
        sec.totp.clear();
        sec.pending.clear();
        sec.backup.clear();
        sec.last = 0;
        Self::put_security(&db, username, &sec)
    }

    /// A new set of backup codes (the old ones stop working).
    pub fn totp_new_backup(&self, username: &str, code: &str) -> Result<Vec<String>, DirError> {
        let db = self.db.lock().unwrap();
        let mut sec = Self::security_of(&db, username)?;
        if sec.totp.is_empty() {
            return Err(DirError::BadProfile("two-step verification is off".into()));
        }
        if !Self::check_code(&mut sec, code) {
            return Err(DirError::BadTotp);
        }
        let (shown, stored) = crate::totp::new_backup_codes();
        sec.backup = stored;
        Self::put_security(&db, username, &sec)?;
        Ok(shown)
    }

    // ---- account recovery ----

    /// Is this e-mail bound to an account (people, not bots)?
    pub fn email_registered(&self, email: &str) -> bool {
        let email = email.trim().to_lowercase();
        let db = self.db.lock().unwrap();
        db.query_row("SELECT 1 FROM users WHERE email=?1 AND is_bot=0", rusqlite::params![email], |_| Ok(()))
            .is_ok()
    }

    /// Store the account's recovery check. The client derives it from the
    /// 24-word recovery key (the same key that opens the encrypted backup);
    /// the relay keeps only its SHA-256, so the words never reach it.
    /// Any signed-in device of the account may set it.
    pub fn set_recovery(&self, username: &str, identity_dh: &[u8; 32], check: &str) -> Result<(), DirError> {
        let check = check.trim();
        if check.len() < 32 || check.len() > 128 {
            return Err(DirError::BadProfile("bad recovery check".into()));
        }
        let username = username.trim().to_lowercase();
        if self.device_role(&username, identity_dh).is_none() {
            return Err(DirError::NotOwner);
        }
        let db = self.db.lock().unwrap();
        db.execute("UPDATE users SET recovery=?2 WHERE username=?1", rusqlite::params![username, recovery_hash(check)])?;
        Ok(())
    }

    /// Does the account have a recovery key?
    pub fn has_recovery(&self, username: &str) -> bool {
        let db = self.db.lock().unwrap();
        db.query_row("SELECT recovery FROM users WHERE username=?1", rusqlite::params![username.trim().to_lowercase()], |r| r.get::<_, String>(0))
            .map(|r| !r.is_empty())
            .unwrap_or(false)
    }

    /// Forgotten password: an e-mail ticket plus — when the account has one —
    /// the recovery check set a new password. The ticket stays valid, so the
    /// client goes on to sign in (and restore its backup) with the new one.
    /// Returns the username.
    pub fn reset_password(&self, ticket: &str, check: &str, new_secret: &str) -> Result<String, DirError> {
        if new_secret.chars().count() < 8 {
            return Err(DirError::WeakPassword);
        }
        let db = self.db.lock().unwrap();
        let (email, verified_at): (String, Option<i64>) = db
            .query_row(
                "SELECT email, verified_at FROM email_codes WHERE ticket=?1",
                rusqlite::params![ticket],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|_| DirError::BadTicket)?;
        match verified_at {
            Some(ts) if now_secs().saturating_sub(ts as u64) <= TICKET_TTL_SECS => {}
            _ => return Err(DirError::BadTicket),
        }
        let (username, stored): (String, String) = db
            .query_row(
                "SELECT username, recovery FROM users WHERE email=?1 AND is_bot=0",
                rusqlite::params![email],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|_| DirError::NoAccount)?;
        if !stored.is_empty() {
            if !subtle_eq(recovery_hash(check.trim()).as_bytes(), stored.as_bytes()) {
                return Err(DirError::BadRecovery);
            }
        }
        db.execute("UPDATE users SET password=?2 WHERE username=?1", rusqlite::params![username, hash_password(new_secret)])?;
        // The same ticket then signs this device in once (`login_checked`
        // uses it up) — the app's "reset, then sign in" flow.
        tracing::info!(%username, with_key = !stored.is_empty(), "password reset");
        Ok(username)
    }

    // ---- sealed messages ("message to a trusted person") ----

    /// The user was connected just now (any device).
    pub fn touch_seen(&self, username: &str) {
        let db = self.db.lock().unwrap();
        let _ = db.execute(
            "INSERT INTO last_seen (username, at) VALUES (?1, ?2) ON CONFLICT(username) DO UPDATE SET at=?2",
            rusqlite::params![username.trim().to_lowercase(), now_secs() as i64],
        );
    }

    pub fn last_seen(&self, username: &str) -> Option<i64> {
        let db = self.db.lock().unwrap();
        db.query_row("SELECT at FROM last_seen WHERE username=?1", rusqlite::params![username.trim().to_lowercase()], |r| r.get(0)).ok()
    }

    /// Store the key of a sealed message. The recipient already has the
    /// ciphertext (sent end-to-end encrypted); the relay has only the key —
    /// neither can read it alone until the condition is met.
    #[allow(clippy::too_many_arguments)]
    pub fn capsule_put(&self, owner: &str, id: &str, recipient: &str, key: &str, kind: &str, open_at: i64, days: i64, grace_h: i64) -> Result<(), DirError> {
        let ok = id.len() >= 8 && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            && key.len() == 64 && key.chars().all(|c| c.is_ascii_hexdigit())
            && matches!(kind, "date" | "inactive")
            && (0..=24 * 30).contains(&grace_h) && (0..=3650).contains(&days);
        if !ok || (kind == "inactive" && days < 1) || (kind == "date" && open_at <= 0) {
            return Err(DirError::BadProfile("bad sealed message".into()));
        }
        let db = self.db.lock().unwrap();
        let n: i64 = db.query_row("SELECT COUNT(*) FROM capsules WHERE owner=?1 AND cancelled=0", rusqlite::params![owner], |r| r.get(0)).unwrap_or(0);
        if n >= 50 {
            return Err(DirError::BadProfile("at most 50 sealed messages".into()));
        }
        db.execute(
            "INSERT INTO capsules (id, owner, recipient, key, kind, open_at, days, grace_h, created) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![id, owner, recipient.trim().to_lowercase(), key.to_lowercase(), kind, open_at, days, grace_h, now_secs() as i64],
        ).map_err(|_| DirError::BadProfile("this sealed message already exists".into()))?;
        Ok(())
    }

    /// The recipient asks to open: `Ok(Ok(key))` when it opens, `Ok(Err(state))` otherwise
    /// (`state` says what it waits for). The first request of a capsule that
    /// needs a grace period starts the clock; the owner can deny meanwhile.
    pub fn capsule_open(&self, id: &str, caller: &str, owner_online: bool) -> Result<Result<String, serde_json::Value>, DirError> {
        let db = self.db.lock().unwrap();
        let row = db.query_row(
            "SELECT owner, recipient, key, kind, open_at, days, grace_h, requested, released, cancelled FROM capsules WHERE id=?1",
            rusqlite::params![id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, i64>(4)?,
                    r.get::<_, i64>(5)?, r.get::<_, i64>(6)?, r.get::<_, i64>(7)?, r.get::<_, i64>(8)?, r.get::<_, i64>(9)?)),
        ).map_err(|_| DirError::NotOwner)?;
        let (owner, recipient, key, kind, open_at, days, grace_h, requested, released, cancelled) = row;
        if recipient != caller.trim().to_lowercase() {
            return Err(DirError::NotOwner);
        }
        if cancelled != 0 {
            return Ok(Err(serde_json::json!({ "state": "cancelled" })));
        }
        if released != 0 {
            return Ok(Ok(key));
        }
        let now = now_secs() as i64;
        // The condition itself.
        let met = match kind.as_str() {
            "date" => now * 1000 >= open_at,
            _ => {
                let seen: i64 = db.query_row("SELECT at FROM last_seen WHERE username=?1", rusqlite::params![owner], |r| r.get(0)).unwrap_or(0);
                !owner_online && seen > 0 && now - seen >= days * 86_400
            }
        };
        if !met {
            return Ok(Err(match kind.as_str() {
                "date" => serde_json::json!({ "state": "sealed", "until": open_at }),
                _ => serde_json::json!({ "state": "sealed", "inactive_days": days }),
            }));
        }
        // Then the confirmation window, if any.
        if grace_h > 0 {
            if requested == 0 {
                db.execute("UPDATE capsules SET requested=?2 WHERE id=?1", rusqlite::params![id, now])?;
                return Ok(Err(serde_json::json!({ "state": "waiting", "until": (now + grace_h * 3600) * 1000 })));
            }
            if now < requested + grace_h * 3600 {
                return Ok(Err(serde_json::json!({ "state": "waiting", "until": (requested + grace_h * 3600) * 1000 })));
            }
        }
        db.execute("UPDATE capsules SET released=?2 WHERE id=?1", rusqlite::params![id, now])?;
        Ok(Ok(key))
    }

    pub fn capsule_owner(&self, id: &str) -> Option<String> {
        let db = self.db.lock().unwrap();
        db.query_row("SELECT owner FROM capsules WHERE id=?1", rusqlite::params![id], |r| r.get(0)).ok()
    }

    /// The owner's sealed messages (keys left out).
    pub fn capsules_of(&self, owner: &str) -> Vec<serde_json::Value> {
        let db = self.db.lock().unwrap();
        let Ok(mut st) = db.prepare("SELECT id, recipient, kind, open_at, days, grace_h, created, requested, released, cancelled FROM capsules WHERE owner=?1 ORDER BY created DESC") else { return Vec::new() };
        st.query_map(rusqlite::params![owner], |r| Ok(serde_json::json!({
            "id": r.get::<_, String>(0)?, "recipient": r.get::<_, String>(1)?, "kind": r.get::<_, String>(2)?,
            "open_at": r.get::<_, i64>(3)?, "days": r.get::<_, i64>(4)?, "grace_h": r.get::<_, i64>(5)?,
            "created": r.get::<_, i64>(6)? * 1000, "requested": r.get::<_, i64>(7)? * 1000,
            "released": r.get::<_, i64>(8)? * 1000, "cancelled": r.get::<_, i64>(9)? != 0,
        }))).map(|rows| rows.flatten().collect()).unwrap_or_default()
    }

    /// Owner: cancel for good (`forever`) or just deny the pending request.
    pub fn capsule_stop(&self, owner: &str, id: &str, forever: bool) -> Result<bool, DirError> {
        let db = self.db.lock().unwrap();
        let sql = if forever {
            "UPDATE capsules SET cancelled=1 WHERE id=?1 AND owner=?2 AND released=0"
        } else {
            "UPDATE capsules SET requested=0 WHERE id=?1 AND owner=?2 AND released=0"
        };
        Ok(db.execute(sql, rusqlite::params![id, owner])? > 0)
    }

    // ---- devices ----

    /// The extra (non-primary) devices of an account.
    pub fn devices(&self, username: &str) -> Vec<Device> {
        let username = username.trim().to_lowercase();
        let db = self.db.lock().unwrap();
        let raw: String = db
            .query_row("SELECT devices FROM users WHERE username=?1", rusqlite::params![username], |r| r.get(0))
            .unwrap_or_default();
        serde_json::from_str(&raw).unwrap_or_default()
    }

    fn put_devices(&self, username: &str, list: &[Device]) -> Result<(), DirError> {
        let json = serde_json::to_string(list).unwrap_or_else(|_| "[]".into());
        let db = self.db.lock().unwrap();
        db.execute("UPDATE users SET devices=?2 WHERE username=?1", rusqlite::params![username, json])?;
        Ok(())
    }

    /// Does this identity key belong to the account? `Some(true)` = primary
    /// device, `Some(false)` = extra device, `None` = not this account's.
    pub fn device_role(&self, username: &str, identity_dh: &[u8; 32]) -> Option<bool> {
        let primary = self.identity_for(username)?;
        if &primary == identity_dh {
            return Some(true);
        }
        let hex = hex::encode(identity_dh);
        self.devices(username).iter().any(|d| d.dh == hex).then_some(false)
    }

    /// Add a device (no-op if it's already there or is the primary one).
    pub fn add_device(&self, username: &str, identity_dh: &[u8; 32], name: &str) -> Result<(), DirError> {
        let username = username.trim().to_lowercase();
        if self.device_role(&username, identity_dh).is_some() {
            return Ok(());
        }
        let mut list = self.devices(&username);
        while list.len() >= MAX_DEVICES {
            list.remove(0);
        }
        let added = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        let name: String = name.trim().chars().take(48).collect();
        list.push(Device { dh: hex::encode(identity_dh), name, added });
        self.put_devices(&username, &list)
    }

    /// Sign a device out of the account. Removing the primary device makes
    /// `successor` (another device of the account) the primary one.
    pub fn remove_device(&self, username: &str, dev: &str, successor: Option<&[u8; 32]>) -> Result<bool, DirError> {
        let username = username.trim().to_lowercase();
        let Some(primary) = self.identity_for(&username) else { return Ok(false) };
        let mut list = self.devices(&username);
        if device_id(&primary) == dev {
            let Some(next) = successor else { return Ok(false) };
            let hex = hex::encode(next);
            if !list.iter().any(|d| d.dh == hex) {
                return Ok(false);
            }
            list.retain(|d| d.dh != hex);
            let devices = serde_json::to_string(&list).map_err(|_| DirError::BadUsername)?;
            let mut conn = self.db.lock().unwrap();
            let db = conn.transaction()?;
            let changed = db.execute(
                "UPDATE users SET identity_dh=?2, devices=?3 WHERE username=?1 AND identity_dh=?4",
                rusqlite::params![username, next.as_slice(), devices, primary.as_slice()],
            )?;
            if changed == 0 { return Ok(false); }
            db.execute("UPDATE bot_keys SET owner_dh=?2 WHERE owner_dh=?1", rusqlite::params![primary.as_slice(), next.as_slice()])?;
            db.execute("UPDATE users SET identity_dh=?2 WHERE is_bot=1 AND identity_dh=?1", rusqlite::params![primary.as_slice(), next.as_slice()])?;
            db.commit()?;
            return Ok(true);
        }
        let before = list.len();
        list.retain(|d| hex::decode(&d.dh).map(|b| device_id(&b) != dev).unwrap_or(true));
        if list.len() == before {
            return Ok(false);
        }
        self.put_devices(&username, &list)?;
        Ok(true)
    }

    /// Rename an extra device.
    pub fn rename_device(&self, username: &str, dev: &str, name: &str) -> Result<bool, DirError> {
        let mut list = self.devices(username);
        let mut hit = false;
        for d in list.iter_mut() {
            if hex::decode(&d.dh).map(|b| device_id(&b) == dev).unwrap_or(false) {
                d.name = name.trim().chars().take(48).collect();
                hit = true;
            }
        }
        if hit {
            self.put_devices(&username.trim().to_lowercase(), &list)?;
        }
        Ok(hit)
    }

    // ---- federation: replicate accounts between relays ----

    /// User rows changed after `since` (ms), oldest first, at most 500.
    pub fn fed_users_since(&self, since: i64) -> Vec<FedUser> {
        self.fed_users_after(since, None)
    }

    /// Accounts after the cursor `(since, after)` in `(updated_at, username)`
    /// order, 500 at most (REV-001; `None` = the old `> since`).
    pub fn fed_users_after(&self, since: i64, after: Option<&str>) -> Vec<FedUser> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let Ok(mut st) = db.prepare(
            "SELECT username, identity_dh, email, display_name, about, phone, is_bot, created_at,
                    extra, avatar_ver, banner_ver, media_acl, password, music_ver, updated_at, devices, recovery, uid, security, devlist, seal
             FROM users WHERE is_bot = 0 AND (updated_at > ?1 OR (?2 IS NOT NULL AND updated_at = ?1 AND username > ?2))
             ORDER BY updated_at, username LIMIT 500",
        ) else { return Vec::new() };
        st.query_map(rusqlite::params![since, after], |r| {
            Ok(FedUser {
                username: r.get(0)?,
                identity_dh: hex::encode(r.get::<_, Vec<u8>>(1)?),
                email: r.get(2)?,
                display_name: r.get(3)?,
                about: r.get(4)?,
                phone: r.get(5)?,
                is_bot: r.get::<_, i64>(6)? != 0,
                created_at: r.get(7)?,
                extra: r.get(8)?,
                avatar_ver: r.get(9)?,
                banner_ver: r.get(10)?,
                media_acl: r.get(11)?,
                password: r.get(12)?,
                music_ver: r.get(13)?,
                updated_at: r.get(14)?,
                devices: r.get(15)?,
                recovery: r.get(16)?,
                uid: r.get(17)?,
                security: r.get(18)?,
                devlist: r.get(19)?,
                seal: r.get(20)?,
            })
        })
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
    }

    /// The seal message of `username` as it is stored here.
    pub fn seal_message_of(&self, username: &str) -> Option<String> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        db.query_row(
            "SELECT uid, identity_dh, password, security, devices, recovery, devlist FROM users WHERE username=?1 AND is_bot=0",
            rusqlite::params![username],
            |r| Ok(seal_message(&r.get::<_, String>(0)?, &hex::encode(r.get::<_, Vec<u8>>(1)?), &r.get::<_, String>(2)?, &r.get::<_, String>(3)?, &r.get::<_, String>(4)?, &r.get::<_, String>(5)?, &r.get::<_, String>(6)?)),
        )
        .ok()
    }

    /// Store a seal a device of `username` made (`signer_dh` signed in with
    /// `signer_sign`). The signature must be over the account's current
    /// security fields, and the signer one of its devices.
    pub fn set_seal(&self, username: &str, signer_dh: &[u8; 32], signer_sign: &[u8; 32], sig_hex: &str) -> Result<(), DirError> {
        let msg = self.seal_message_of(username).ok_or(DirError::NotOwner)?;
        if self.device_role(username, signer_dh).is_none() {
            return Err(DirError::NotOwner);
        }
        let sig: [u8; 64] = hex::decode(sig_hex.trim()).ok().and_then(|b| b.try_into().ok()).ok_or(DirError::BadProfile("bad signature".into()))?;
        aqua_core::crypto::SignPublic::from_bytes(*signer_sign).verify(msg.as_bytes(), &sig).map_err(|_| DirError::BadProfile("the seal doesn't match the account".into()))?;
        let seal = Seal { signer: hex::encode(signer_dh), sign: hex::encode(signer_sign), sig: hex::encode(sig) };
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        db.execute("UPDATE users SET seal=?2 WHERE username=?1", rusqlite::params![username, serde_json::to_string(&seal).unwrap_or_default()])?;
        Ok(())
    }

    /// Does `incoming` carry a seal over its own security fields by a device
    /// the account had here (its key, its devices; the Ed25519 key known
    /// from the device list kept here, from a sign-in here, or the seal kept
    /// here)?
    fn seal_ok(&self, db: &Connection, local: &LocalSec, incoming: &FedUser, uid: &str) -> bool {
        let Ok(seal) = serde_json::from_str::<Seal>(&incoming.seal) else { return false };
        let msg = seal_message(uid, &incoming.identity_dh, &incoming.password, &incoming.security, &incoming.devices, &incoming.recovery, &incoming.devlist);
        let (Some(sign), Some(sig)) = (hex::decode(&seal.sign).ok().and_then(|b| <[u8; 32]>::try_from(b).ok()), hex::decode(&seal.sig).ok().and_then(|b| <[u8; 64]>::try_from(b).ok())) else { return false };
        if aqua_core::crypto::SignPublic::from_bytes(sign).verify(msg.as_bytes(), &sig).is_err() {
            return false;
        }
        // The signer: the account's key or one of its devices, as kept here.
        let local_devices: Vec<Device> = serde_json::from_str(&local.devices).unwrap_or_default();
        if seal.signer != local.identity_dh && !local_devices.iter().any(|d| d.dh == seal.signer) {
            return false;
        }
        // …with the Ed25519 key it is known by here.
        let in_list = serde_json::from_str::<aqua_core::devlist::SignedDeviceList>(&local.devlist)
            .is_ok_and(|l| l.devices.iter().any(|k| k.dh == seal.signer && k.sign == seal.sign));
        let in_seal = serde_json::from_str::<Seal>(&local.seal).is_ok_and(|s| s.signer == seal.signer && s.sign == seal.sign);
        let signed_in = hex::decode(&seal.signer).ok().and_then(|b| <[u8; 32]>::try_from(b).ok()).is_some_and(|dh| {
            db.query_row("SELECT sign FROM identity_signs WHERE dh=?1", rusqlite::params![dh.as_slice()], |r| r.get::<_, Vec<u8>>(0))
                .is_ok_and(|s| s.as_slice() == sign.as_slice())
        });
        in_list || in_seal || signed_in
    }

    /// Apply a peer's row if it is newer than ours.
    ///
    /// Accounts are matched by ID ([`uid_for`]), not by @name: the same ID
    /// under a new name is a rename; the same name under another ID is a
    /// different person (two relays that ran alone and then joined). Then
    /// both accounts stay: the older one keeps the name, the newer one gets
    /// `name_1234` on every relay and its owner picks a new name.
    pub fn fed_apply_user(&self, u: &FedUser) -> FedApplied {
        self.fed_apply_user_from(u, "")
    }

    /// [`Self::fed_apply_user`] for a row pulled from the relay with node id
    /// `origin` ("" when not known by key).
    pub fn fed_apply_user_from(&self, u: &FedUser, origin: &str) -> FedApplied {
        let mut out = FedApplied::default();
        let Ok(dh) = hex::decode(&u.identity_dh) else { return out };
        if dh.len() != 32 || !Self::valid_username(&u.username) {
            return out;
        }
        // A8: bots aren't accounts relays exchange (a peer could make one
        // and take a name the bot rules keep); a row "changed in the future"
        // would stay newer than every real change here (the update trigger
        // only counts up from it) — both refused.
        if u.is_bot {
            tracing::warn!(user = %u.username, %origin, "federation: a bot account from a peer — refused");
            return out;
        }
        if u.updated_at > crate::now_ms() + FED_SKEW_MS {
            tracing::warn!(user = %u.username, %origin, "federation: an account changed in the future — refused");
            return out;
        }
        let uid = if u.uid.len() == 10 && u.uid.bytes().all(|b| b.is_ascii_digit()) { u.uid.clone() } else { uid_for(&dh) };
        // Banned here: a peer doesn't bring them back.
        if self.is_banned_uid(&uid) || self.is_banned_dh(&dh) {
            return out;
        }
        let mut db = self.db.lock().unwrap();
        // Deleted (a tombstone of this generation, A7): a relay that didn't
        // hear of it doesn't bring it back. A later sign-up is another one.
        if Self::tombed(&db, &uid, u.created_at) {
            return out;
        }
        // This account here, under whatever name.
        let mine: Option<(String, i64)> = db
            .query_row("SELECT username, updated_at FROM users WHERE uid=?1 AND is_bot=0", rusqlite::params![uid], |r| Ok((r.get(0)?, r.get(1)?)))
            .ok();
        if mine.as_ref().is_some_and(|(_, t)| *t >= u.updated_at) {
            return out;
        }
        // Someone else here with this @name?
        let mut name = u.username.clone();
        let holder: Option<(String, i64, i64)> = db
            .query_row(
                "SELECT uid, created_at, is_bot FROM users WHERE username=?1",
                rusqlite::params![name],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .ok();
        if let Some((their_uid, their_created, bot)) = holder.filter(|h| h.0 != uid) {
            // Older account keeps the name; on a tie, the smaller ID.
            let they_win = bot != 0 || (their_created, their_uid.as_str()) <= (u.created_at, uid.as_str());
            if they_win {
                name = Self::displaced_name(&db, &u.username, &uid);
                if mine.as_ref().is_some_and(|(n, _)| *n == name) {
                    // Already here under that name: just update it below.
                } else if db.query_row("SELECT 1 FROM users WHERE username=?1", rusqlite::params![name], |_| Ok(())).is_ok() {
                    return out;
                }
            } else {
                // Ours must move: rename it everywhere it's stored here.
                let to = Self::displaced_name(&db, &u.username, &their_uid);
                if Self::move_account(&mut db, &u.username, &to, false).is_err() {
                    out.failed = true;
                    return out;
                }
                tracing::warn!(user = %u.username, %to, "federation: an older account from a peer has this name — ours is renamed");
                out.displaced = Some((u.username.clone(), to));
            }
        }
        // The same account under another name here (renamed on its home
        // relay, or displaced): the old row goes, the old name forwards —
        // unless it now belongs to someone else.
        if let Some((old, _)) = mine.filter(|(n, _)| *n != name) {
            let _ = db.execute("DELETE FROM users WHERE username=?1", rusqlite::params![old]);
            if out.displaced.as_ref().is_none_or(|(n, _)| *n != old) && old != u.username {
                let _ = db.execute(
                    "INSERT INTO renames (old_name, new_name, at) VALUES (?1, ?2, ?3) ON CONFLICT(old_name) DO UPDATE SET new_name=?2, at=?3",
                    rusqlite::params![old, name, now_secs() as i64],
                );
            }
        }
        // Legacy rows (no ID match) of the same identity under another name.
        let renamed: Vec<String> = {
            let mut st = match db.prepare("SELECT username FROM users WHERE identity_dh=?1 AND username<>?2 AND is_bot=0 AND uid=?3") {
                Ok(st) => st,
                Err(_) => { out.failed = true; return out }
            };
            st.query_map(rusqlite::params![dh, name, uid], |r| r.get::<_, String>(0))
                .map(|rows| rows.flatten().collect())
                .unwrap_or_default()
        };
        for old in renamed {
            let _ = db.execute("DELETE FROM users WHERE username=?1", rusqlite::params![old]);
        }
        // An e-mail moving to another username elsewhere (a rename of this
        // same account): drop it from the old row first. A8: an e-mail that
        // belongs to ANOTHER account here (another ID) stays with it — a row
        // from a peer took it over before, and with it sign-in by code and
        // password reset of that account. The incoming row goes without it.
        let mut u = u.clone();
        if let Some(email) = u.email.clone() {
            let other: Option<String> = db.query_row("SELECT uid FROM users WHERE email=?1 AND username<>?2", rusqlite::params![email, name], |r| r.get(0)).ok();
            match other {
                Some(o) if o != uid && !o.is_empty() => {
                    tracing::warn!(user = %u.username, %origin, "federation: a peer's account names an e-mail of another account here — not taken");
                    u.email = None;
                }
                _ => { let _ = db.execute("UPDATE users SET email=NULL WHERE email=?1 AND username<>?2", rusqlite::params![email, name]); }
            }
        }
        // A sealed account: its security fields change only with a seal by a
        // device it already had (see `Seal`). Otherwise they stay as they are
        // here and only the rest (profile, media versions) is applied.
        let local: Option<LocalSec> = db.query_row(
            "SELECT identity_dh, password, security, devices, recovery, devlist, seal FROM users WHERE uid=?1 AND is_bot=0",
            rusqlite::params![uid],
            |r| Ok(LocalSec { identity_dh: hex::encode(r.get::<_, Vec<u8>>(0)?), password: r.get(1)?, security: r.get(2)?, devices: r.get(3)?, recovery: r.get(4)?, devlist: r.get(5)?, seal: r.get(6)? }),
        ).ok();
        let mut dh = dh;
        if let Some(l) = local.as_ref().filter(|l| !l.seal.is_empty()) {
            // Older relays send '' for fields they don't know: "keep ours".
            let keep = |inc: &str, mine: &str| if inc.is_empty() { mine.to_string() } else { inc.to_string() };
            u.recovery = keep(&u.recovery, &l.recovery);
            u.security = keep(&u.security, &l.security);
            u.devlist = keep(&u.devlist, &l.devlist);
            let changed = u.identity_dh != l.identity_dh || u.password != l.password || u.security != l.security
                || u.devices != l.devices || u.recovery != l.recovery || u.devlist != l.devlist;
            if changed && !self.seal_ok(&db, l, &u, &uid) {
                tracing::warn!(user = %u.username, "federation: a change of a sealed account's key / password / devices without its devices' seal — not applied");
                u.identity_dh = l.identity_dh.clone();
                u.password = l.password.clone();
                u.security = l.security.clone();
                u.devices = l.devices.clone();
                u.recovery = l.recovery.clone();
                u.devlist = l.devlist.clone();
                u.seal = l.seal.clone();
                dh = hex::decode(&l.identity_dh).unwrap_or(dh);
                out.refused_security = true;
            }
        }
        let u = &u;
        out.name = name.clone();
        out.applied = db.execute(
            "INSERT INTO users (username, identity_dh, email, display_name, about, phone, is_bot, created_at,
                                extra, avatar_ver, banner_ver, media_acl, password, music_ver, updated_at, devices, recovery, uid, security, devlist, seal)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, CASE WHEN ?19 = '' THEN '{}' ELSE ?19 END, ?20, ?21)
             ON CONFLICT(username) DO UPDATE SET identity_dh=?2, email=?3, display_name=?4, about=?5, phone=?6,
                 is_bot=?7, created_at=?8, extra=?9, avatar_ver=?10, banner_ver=?11, media_acl=?12, password=?13,
                 music_ver=?14, updated_at=?15, devices=?16,
                 recovery=CASE WHEN ?17 = '' THEN recovery ELSE ?17 END, uid=?18,
                 security=CASE WHEN ?19 = '' THEN security ELSE ?19 END,
                 devlist=CASE WHEN ?20 = '' THEN devlist ELSE ?20 END,
                 seal=CASE WHEN ?21 = '' THEN seal ELSE ?21 END",
            rusqlite::params![name, dh, u.email, u.display_name, u.about, u.phone, u.is_bot as i64, u.created_at,
                u.extra, u.avatar_ver, u.banner_ver, u.media_acl, u.password, u.music_ver, u.updated_at, u.devices, u.recovery, uid, u.security, u.devlist, u.seal],
        )
        .is_ok();
        out.failed = !out.applied;
        out
    }

    /// Move everything stored under `old` to `new` (account row, queues,
    /// backups, keys, media). `forward` keeps `old` pointing at `new`.
    fn move_account(db: &mut Connection, old: &str, new: &str, forward: bool) -> rusqlite::Result<()> {
        let now = now_secs() as i64;
        let tx = db.transaction()?;
        tx.execute("UPDATE users SET username=?1 WHERE username=?2", rusqlite::params![new, old])?;
        for (table, col) in [("last_seen", "username"), ("backups", "username"), ("prekeys", "username"), ("profile_media", "username"),
                             ("capsules", "owner"), ("capsules", "recipient"), ("bot_peers", "user"), ("hist_store", "username")] {
            tx.execute(&format!("UPDATE OR IGNORE {table} SET {col}=?1 WHERE {col}=?2"), rusqlite::params![new, old])?;
        }
        // Waiting envelopes, to the account and to each of its devices (user~dev).
        tx.execute("UPDATE inbox SET user=?1 WHERE user=?2", rusqlite::params![new, old])?;
        tx.execute(
            "UPDATE inbox SET user=?1 || substr(user, length(?2) + 1) WHERE substr(user, 1, length(?2) + 1) = ?2 || '~'",
            rusqlite::params![new, old],
        )?;
        // Older names of this account now point at the newest one.
        tx.execute("UPDATE renames SET new_name=?1 WHERE new_name=?2", rusqlite::params![new, old])?;
        tx.execute("DELETE FROM renames WHERE old_name=?1", rusqlite::params![new])?;
        if forward {
            tx.execute(
                "INSERT INTO renames (old_name, new_name, at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(old_name) DO UPDATE SET new_name=?2, at=?3",
                rusqlite::params![old, new, now],
            )?;
        } else {
            tx.execute("DELETE FROM renames WHERE old_name=?1", rusqlite::params![old])?;
        }
        tx.commit()
    }

    // ---- encrypted history shared by an account's devices ----

    /// Store one record. `only_new`: refuse to replace a different value
    /// (the key id, `kid`: the first device's key wins). Returns the seq.
    /// Bytes of history stored for the account (quota.rs).
    pub fn hist_total(&self, username: &str) -> i64 {
        let db = self.db.lock().unwrap();
        db.query_row("SELECT COALESCE(SUM(length(data)), 0) FROM hist_store WHERE username=?1", rusqlite::params![username], |r| r.get(0)).unwrap_or(0)
    }

    /// `limit`: the account's history share (quota.rs, by level).
    pub fn hist_put(&self, username: &str, k: &str, data: &[u8], only_new: bool, limit: i64) -> Result<i64, DirError> {
        let db = self.db.lock().unwrap();
        if only_new {
            let cur: Option<Vec<u8>> = db
                .query_row("SELECT data FROM hist_store WHERE username=?1 AND k=?2", rusqlite::params![username, k], |r| r.get(0))
                .ok();
            if let Some(cur) = cur {
                return if cur == data {
                    db.query_row("SELECT seq FROM hist_store WHERE username=?1 AND k=?2", rusqlite::params![username, k], |r| r.get(0)).map_err(Into::into)
                } else {
                    Err(DirError::Conflict)
                };
            }
        }
        let total: i64 = db
            .query_row("SELECT COALESCE(SUM(length(data)), 0) FROM hist_store WHERE username=?1 AND k<>?2", rusqlite::params![username, k], |r| r.get(0))
            .unwrap_or(0);
        // The account's own key never counts (a full share must not lock it out).
        if k != "kid" && total + data.len() as i64 > limit {
            return Err(DirError::TooLarge);
        }
        let seq: i64 = db
            .query_row("SELECT COALESCE(MAX(seq), 0) + 1 FROM hist_store WHERE username=?1", rusqlite::params![username], |r| r.get(0))
            .unwrap_or(1);
        db.execute(
            "INSERT INTO hist_store (username, k, seq, data, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(username, k) DO UPDATE SET seq=?3, data=?4, updated_at=?5",
            rusqlite::params![username, k, seq, data, crate::now_ms()],
        )?;
        Ok(seq)
    }

    /// Records changed after `since`: (key, seq, size), oldest first.
    pub fn hist_list(&self, username: &str, since: i64, limit: usize) -> Vec<(String, i64, i64)> {
        let db = self.db.lock().unwrap();
        let Ok(mut st) = db.prepare("SELECT k, seq, length(data) FROM hist_store WHERE username=?1 AND seq>?2 ORDER BY seq LIMIT ?3") else { return Vec::new() };
        st.query_map(rusqlite::params![username, since, limit as i64], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default()
    }

    pub fn hist_get(&self, username: &str, k: &str) -> Option<Vec<u8>> {
        let db = self.db.lock().unwrap();
        db.query_row("SELECT data FROM hist_store WHERE username=?1 AND k=?2", rusqlite::params![username, k], |r| r.get(0)).ok()
    }

    /// Federation: (username, key, updated_at) changed after `since` (ms).
    pub fn fed_hist_since(&self, since: i64) -> Vec<(String, String, i64)> {
        self.fed_hist_after(since, None)
    }

    /// History records after the cursor `(since, "user/key")` (REV-001).
    pub fn fed_hist_after(&self, since: i64, after: Option<&str>) -> Vec<(String, String, i64)> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let Ok(mut st) = db.prepare(
            "SELECT username, k, updated_at FROM hist_store
             WHERE updated_at > ?1 OR (?2 IS NOT NULL AND updated_at = ?1 AND (username || '/' || k) > ?2)
             ORDER BY updated_at, username || '/' || k LIMIT 1000",
        ) else { return Vec::new() };
        st.query_map(rusqlite::params![since, after], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default()
    }

    /// Federation: a peer's record, if newer than ours (it gets a new local seq).
    /// Is this generation of account `uid` (created at `created_at`) deleted?
    fn tombed(db: &Connection, uid: &str, created_at: i64) -> bool {
        db.query_row("SELECT 1 FROM fed_tombstones WHERE uid=?1 AND created_at>=?2 LIMIT 1", rusqlite::params![uid, created_at], |_| Ok(()))
            .is_ok()
    }

    /// `(uid, created_at)` of a person's account here (for its tombstone).
    pub fn account_generation(&self, username: &str) -> Option<(String, i64)> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        db.query_row("SELECT uid, created_at FROM users WHERE username=?1 AND is_bot=0", rusqlite::params![username], |r| Ok((r.get(0)?, r.get(1)?))).ok()
    }

    /// Keep a tombstone. Ok(true) when it is new here.
    pub fn add_tomb(&self, t: &Tomb) -> rusqlite::Result<bool> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let n = db.execute(
            "INSERT OR IGNORE INTO fed_tombstones (uid, created_at, username, deleted_at, origin, sig, got_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            rusqlite::params![t.uid, t.created_at, t.username, t.deleted_at, t.origin, t.sig, crate::now_ms()],
        )?;
        Ok(n > 0)
    }

    /// A tombstone from the network (signature checked by the caller): kept,
    /// and the generation it covers removed here — the account (if this one
    /// is that generation or older) and its name in the registry. Returns
    /// the @name removed here, if any. Taking it twice changes nothing.
    pub fn apply_tomb(&self, t: &Tomb) -> rusqlite::Result<Option<String>> {
        if !self.add_tomb(t)? {
            return Ok(None);
        }
        let local: Option<String> = {
            let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
            let _ = db.execute("DELETE FROM fed_names WHERE uid=?1 AND created_at<=?2", rusqlite::params![t.uid, t.created_at]);
            db.query_row("SELECT username FROM users WHERE uid=?1 AND is_bot=0 AND created_at<=?2", rusqlite::params![t.uid, t.created_at], |r| r.get(0)).optional()?
        };
        Ok(local.filter(|n| self.delete_person(n)))
    }

    /// Tombstones after the cursor `(got_at, "uid:created_at")`, in the
    /// order they came here — ours and those handed on to us.
    pub fn tombs_after(&self, since: i64, after: Option<&str>, limit: usize) -> Vec<Tomb> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let Ok(mut st) = db.prepare(
            "SELECT uid, created_at, username, deleted_at, origin, sig, got_at FROM fed_tombstones
             WHERE sig <> '' AND (got_at > ?1 OR (?2 IS NOT NULL AND got_at = ?1 AND (uid || ':' || created_at) > ?2))
             ORDER BY got_at, uid || ':' || created_at LIMIT ?3",
        ) else { return Vec::new() };
        st.query_map(rusqlite::params![since, after, limit as i64], |r| Ok(Tomb {
            uid: r.get(0)?, created_at: r.get(1)?, username: r.get(2)?, deleted_at: r.get(3)?, origin: r.get(4)?, sig: r.get(5)?, got_at: r.get(6)?,
        })).map(|it| it.filter_map(Result::ok).collect()).unwrap_or_default()
    }

    /// How many tombstones signed by `origin` came here since `since_ms`.
    pub fn tombs_from_since(&self, origin: &str, since_ms: i64) -> usize {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        db.query_row("SELECT COUNT(*) FROM fed_tombstones WHERE origin=?1 AND got_at>?2", rusqlite::params![origin, since_ms], |r| r.get::<_, i64>(0))
            .unwrap_or(0) as usize
    }

    /// Is the account with ID `uid` deleted, with no later sign-up of it
    /// known (in the name registry)? A pass for it isn't taken.
    pub fn uid_deleted(&self, uid: &str) -> bool {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let Ok(t) = db.query_row("SELECT MAX(created_at) FROM fed_tombstones WHERE uid=?1", rusqlite::params![uid], |r| r.get::<_, Option<i64>>(0)) else { return false };
        let Some(t) = t else { return false };
        let later = db.query_row("SELECT 1 FROM fed_names WHERE uid=?1 AND created_at>?2", rusqlite::params![uid, t], |_| Ok(())).is_ok()
            || db.query_row("SELECT 1 FROM users WHERE uid=?1 AND created_at>?2", rusqlite::params![uid, t], |_| Ok(())).is_ok();
        !later
    }

    /// Where federation sync is in `peer`'s `stream` (users, backups, hist,
    /// names, tombs): `(since, after)`. Kept on disk, so a restart doesn't
    /// pull everything again from the start (A6).
    pub fn fed_cursor(&self, peer: &str, stream: &str) -> Option<(i64, String)> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        db.query_row("SELECT since, after FROM fed_cursors WHERE peer=?1 AND stream=?2", rusqlite::params![peer, stream], |r| Ok((r.get(0)?, r.get(1)?))).ok()
    }

    /// Keep where sync is in `peer`'s `stream`. False when it couldn't be
    /// written (the caller doesn't go on as if it were).
    pub fn set_fed_cursor(&self, peer: &str, stream: &str, since: i64, after: &str) -> bool {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        db.execute(
            "INSERT INTO fed_cursors(peer, stream, since, after, at) VALUES(?1,?2,?3,?4,?5)
             ON CONFLICT(peer, stream) DO UPDATE SET since=excluded.since, after=excluded.after, at=excluded.at",
            rusqlite::params![peer, stream, since, after, now_secs() as i64],
        ).is_ok()
    }

    /// A peer moved to a new address (same key): its cursors go with it.
    pub fn fed_cursors_move(&self, from: &str, to: &str) {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let _ = db.execute("DELETE FROM fed_cursors WHERE peer=?1", rusqlite::params![to]);
        let _ = db.execute("UPDATE fed_cursors SET peer=?2 WHERE peer=?1", rusqlite::params![from, to]);
    }

    /// Forget where sync is with `peer` (revoked; or the admin's resync):
    /// all of it, or one stream.
    pub fn fed_cursors_drop(&self, peer: Option<&str>, stream: Option<&str>) {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let _ = db.execute(
            "DELETE FROM fed_cursors WHERE (?1 IS NULL OR peer=?1) AND (?2 IS NULL OR stream=?2)",
            rusqlite::params![peer, stream],
        );
    }

    /// Ok(true): taken; Ok(false): ours is as new; Err: couldn't be written.
    pub fn fed_apply_hist(&self, username: &str, k: &str, data: &[u8], updated_at: i64) -> rusqlite::Result<bool> {
        let db = self.db.lock().unwrap();
        let ours: Option<i64> = db
            .query_row("SELECT updated_at FROM hist_store WHERE username=?1 AND k=?2", rusqlite::params![username, k], |r| r.get(0))
            .optional()?;
        if ours.is_some_and(|t| t >= updated_at) {
            return Ok(false);
        }
        let seq: i64 = db
            .query_row("SELECT COALESCE(MAX(seq), 0) + 1 FROM hist_store WHERE username=?1", rusqlite::params![username], |r| r.get(0))?;
        db.execute(
            "INSERT INTO hist_store (username, k, seq, data, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(username, k) DO UPDATE SET seq=?3, data=?4, updated_at=?5",
            rusqlite::params![username, k, seq, data, updated_at],
        )?;
        Ok(true)
    }

    /// (username, updated_at) of backups changed after `since` (seconds).
    pub fn fed_backups_since(&self, since: i64) -> Vec<(String, i64)> {
        self.fed_backups_after(since, None)
    }

    /// Backups after the cursor `(since, username)` (REV-001).
    pub fn fed_backups_after(&self, since: i64, after: Option<&str>) -> Vec<(String, i64)> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let Ok(mut st) = db.prepare(
            "SELECT username, updated_at FROM backups
             WHERE updated_at > ?1 OR (?2 IS NOT NULL AND updated_at = ?1 AND username > ?2)
             ORDER BY updated_at, username LIMIT 200",
        ) else { return Vec::new() };
        st.query_map(rusqlite::params![since, after], |r| Ok((r.get(0)?, r.get(1)?)))
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default()
    }

    /// Store a peer's backup if it is newer than ours.
    /// Ok(true): taken; Ok(false): ours is as new; Err: couldn't be written.
    pub fn fed_apply_backup(&self, username: &str, data: &[u8], updated_at: i64) -> rusqlite::Result<bool> {
        let db = self.db.lock().unwrap();
        let ours: Option<i64> = db
            .query_row("SELECT updated_at FROM backups WHERE username=?1", rusqlite::params![username], |r| r.get(0))
            .optional()?;
        if ours.is_some_and(|t| t >= updated_at) {
            return Ok(false);
        }
        db.execute(
            "INSERT INTO backups (username, data, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(username) DO UPDATE SET data=?2, updated_at=?3",
            rusqlite::params![username, data, updated_at],
        )?;
        Ok(true)
    }

    /// Something to compare: (newest account change, newest backup). When
    /// it moves, this relay tells its peers to pull right away.
    pub fn change_mark(&self) -> (i64, i64) {
        let db = self.db.lock().unwrap();
        let u = db.query_row("SELECT COALESCE(MAX(updated_at), 0) FROM users", [], |r| r.get(0)).unwrap_or(0);
        let b = db.query_row("SELECT COALESCE(MAX(updated_at), 0) FROM backups", [], |r| r.get(0)).unwrap_or(0);
        (u, b)
    }

    /// (avatar, banner, music) versions of a user's profile media.
    pub fn media_versions(&self, username: &str) -> Option<(i64, i64, i64)> {
        let db = self.db.lock().unwrap();
        db.query_row(
            "SELECT avatar_ver, banner_ver, music_ver FROM users WHERE username=?1",
            rusqlite::params![username],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .ok()
    }

    // ---- prekeys: kept on disk so a restart doesn't make users unreachable ----

    pub fn prekeys_put(&self, username: &str, json: &str) {
        let db = self.db.lock().unwrap();
        let _ = db.execute(
            "INSERT INTO prekeys (username, data, updated_at) VALUES (?1, ?2, CAST(strftime('%s','now') AS INTEGER))
             ON CONFLICT(username) DO UPDATE SET data=?2, updated_at=CAST(strftime('%s','now') AS INTEGER)",
            rusqlite::params![username, json],
        );
    }

    pub fn prekeys_delete(&self, username: &str) {
        let db = self.db.lock().unwrap();
        let _ = db.execute("DELETE FROM prekeys WHERE username=?1", rusqlite::params![username]);
    }

    pub fn prekeys_get(&self, username: &str) -> Option<String> {
        let db = self.db.lock().unwrap();
        db.query_row("SELECT data FROM prekeys WHERE username=?1", rusqlite::params![username], |r| r.get(0)).ok()
    }

    /// Store a profile image / track fetched from a peer.
    pub fn fed_store_media(&self, username: &str, kind: MediaKind, mime: &str, data: &[u8]) -> bool {
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT INTO profile_media (username, kind, mime, data) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(username, kind) DO UPDATE SET mime=?3, data=?4",
            rusqlite::params![username, kind.as_str(), mime, data],
        ).is_ok()
    }

    /// An account from a peer is in, its media couldn't be written: put our
    /// media versions back and make the row a moment older, so the next
    /// round takes the row — and fetches the media — again.
    pub fn fed_media_unset(&self, username: &str, versions: (i64, i64, i64)) {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let _ = db.execute(
            "UPDATE users SET avatar_ver=?2, banner_ver=?3, music_ver=?4, updated_at=updated_at-1 WHERE username=?1",
            rusqlite::params![username, versions.0, versions.1, versions.2],
        );
    }

    // ---- offline inbox (opaque envelopes waiting for their recipient) ----

    /// Envelopes kept at most this long for someone who never connects.
    pub const INBOX_TTL_SECS: u64 = 30 * 24 * 3600;

    /// Returns the row id ([`Self::inbox_ack`] removes it).
    pub fn inbox_push(&self, user: &str, msg: &[u8]) -> Result<i64, DirError> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        db.execute("INSERT INTO inbox (user, msg, at) VALUES (?1, ?2, ?3)", rusqlite::params![user, msg, now_secs() as i64])?;
        let id = db.last_insert_rowid();
        // AQUA-SEC-011 (owner, 25.11: the oldest go): at most INBOX_MAX_ROWS
        // envelopes and INBOX_MAX_BYTES per address; past that the oldest
        // are dropped, so one flooded inbox can't fill the relay's disk.
        // Checked every 256 inserts per address to keep the hot path cheap.
        if id % 256 == 0 {
            let (n, bytes): (i64, i64) = db
                .query_row("SELECT COUNT(*), COALESCE(SUM(length(msg)), 0) FROM inbox WHERE user=?1", rusqlite::params![user], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap_or((0, 0));
            if n > Self::INBOX_MAX_ROWS || bytes > Self::INBOX_MAX_BYTES {
                let keep_rows = Self::INBOX_MAX_ROWS.min(if bytes > Self::INBOX_MAX_BYTES && n > 0 { n * Self::INBOX_MAX_BYTES / bytes } else { n });
                let dropped = db.execute(
                    "DELETE FROM inbox WHERE user=?1 AND id NOT IN (SELECT id FROM inbox WHERE user=?1 ORDER BY id DESC LIMIT ?2)",
                    rusqlite::params![user, keep_rows.max(1)],
                ).unwrap_or(0);
                if dropped > 0 {
                    tracing::warn!(%user, dropped, "inbox full — the oldest envelopes were dropped");
                    crate::journal::log(user, "inbox-full", &format!("{dropped}"));
                }
            }
        }
        Ok(id)
    }

    /// Most envelopes kept for one address.
    pub const INBOX_MAX_ROWS: i64 = 20_000;
    /// Most bytes kept for one address.
    pub const INBOX_MAX_BYTES: i64 = 256 * 1024 * 1024;

    /// Envelopes waiting for `user` (tests, panel).
    pub fn inbox_len(&self, user: &str) -> i64 {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        db.query_row("SELECT COUNT(*) FROM inbox WHERE user=?1", rusqlite::params![user], |r| r.get(0)).unwrap_or(0)
    }

    /// Everything waiting for `user`, oldest first (rows stay until acked).
    pub fn inbox_take(&self, user: &str) -> Vec<(i64, Vec<u8>)> {
        let db = self.db.lock().unwrap();
        let cutoff = now_secs().saturating_sub(Self::INBOX_TTL_SECS) as i64;
        let _ = db.execute("DELETE FROM inbox WHERE at < ?1", rusqlite::params![cutoff]);
        let Ok(mut st) = db.prepare("SELECT id, msg FROM inbox WHERE user=?1 ORDER BY id") else { return Vec::new() };
        st.query_map(rusqlite::params![user], |r| Ok((r.get(0)?, r.get(1)?)))
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default()
    }

    /// Write the WAL back into the database file (before copying it).
    pub fn checkpoint(&self) {
        let db = self.db.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
    }

    pub fn inbox_ack(&self, id: i64) {
        let db = self.db.lock().unwrap();
        let _ = db.execute("DELETE FROM inbox WHERE id=?1", rusqlite::params![id]);
    }

    // ---- encrypted account backups (opaque to the server) ----

    pub fn put_backup(&self, username: &str, data: &[u8]) -> Result<(), DirError> {
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT INTO backups (username, data, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(username) DO UPDATE SET data=?2, updated_at=?3",
            rusqlite::params![username, data, now_secs() as i64],
        )?;
        Ok(())
    }

    /// (size, updated_at) of the stored backup.
    pub fn backup_meta(&self, username: &str) -> Option<(i64, i64)> {
        let db = self.db.lock().unwrap();
        db.query_row(
            "SELECT length(data), updated_at FROM backups WHERE username=?1",
            rusqlite::params![username],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok()
    }

    pub fn get_backup(&self, username: &str) -> Option<Vec<u8>> {
        let db = self.db.lock().unwrap();
        db.query_row("SELECT data FROM backups WHERE username=?1", rusqlite::params![username], |r| r.get(0)).ok()
    }

    pub fn delete_backup(&self, username: &str) -> Result<(), DirError> {
        let db = self.db.lock().unwrap();
        db.execute("DELETE FROM backups WHERE username=?1", rusqlite::params![username])?;
        Ok(())
    }

    /// Who may download the owner's avatar / banner (Telegram's "Profile
    /// photo" privacy). Empty / missing = everybody.
    pub fn set_media_acl(&self, username: &str, identity_dh: &[u8; 32], acl: &MediaAcl) -> Result<(), DirError> {
        let username = username.trim().to_lowercase();
        if acl.allow.len() + acl.deny.len() > MEDIA_ACL_MAX {
            return Err(DirError::BadProfile("privacy list is too long".into()));
        }
        let clean = |v: &[String]| -> Vec<String> {
            v.iter().map(|u| u.trim().to_lowercase()).filter(|u| !u.is_empty() && u.len() <= 64).collect()
        };
        let acl = MediaAcl { everybody: acl.everybody, allow: clean(&acl.allow), deny: clean(&acl.deny) };
        let text = if acl.everybody && acl.deny.is_empty() { String::new() } else { serde_json::to_string(&acl).unwrap_or_default() };
        let db = self.db.lock().unwrap();
        Self::check_owner(&db, &username, identity_dh)?;
        db.execute("UPDATE users SET media_acl=?2 WHERE username=?1", rusqlite::params![username, text])?;
        Ok(())
    }

    /// May `viewer` (None = anonymous) see `owner`'s profile images?
    pub fn may_see_media(&self, owner: &str, viewer: Option<&str>) -> bool {
        let owner = owner.trim().to_lowercase();
        let db = self.db.lock().unwrap();
        let text: String = db
            .query_row("SELECT media_acl FROM users WHERE username=?1", rusqlite::params![owner], |r| r.get(0))
            .unwrap_or_default();
        drop(db);
        if text.is_empty() {
            return true;
        }
        let Ok(acl) = serde_json::from_str::<MediaAcl>(&text) else { return true };
        let Some(v) = viewer.map(|v| v.trim().to_lowercase()) else { return acl.everybody && acl.deny.is_empty() };
        if v == owner {
            return true;
        }
        if acl.deny.iter().any(|d| *d == v) {
            return false;
        }
        acl.everybody || acl.allow.iter().any(|a| *a == v)
    }

    /// The username registered to an identity key, if any.
    pub fn username_for_identity(&self, identity_dh: &[u8; 32]) -> Option<String> {
        let db = self.db.lock().unwrap();
        let primary: Option<String> = db.query_row(
            "SELECT username FROM users WHERE identity_dh=?1 AND is_bot=0 LIMIT 1",
            rusqlite::params![identity_dh.as_slice()],
            |r| r.get(0),
        )
        .ok();
        if primary.is_some() {
            return primary;
        }
        // An extra device of some account.
        let hex = hex::encode(identity_dh);
        db.query_row(
            "SELECT username FROM users WHERE is_bot=0 AND instr(devices, ?1) > 0 LIMIT 1",
            rusqlite::params![format!("\"{hex}\"")],
            |r| r.get(0),
        )
        .ok()
    }

    /// A registered device manages bots under its account's primary identity.
    pub fn bot_account_identity(&self, caller: &[u8; 32]) -> [u8; 32] {
        self.username_for_identity(caller)
            .and_then(|username| self.identity_for(&username))
            .unwrap_or(*caller)
    }

    /// Change an account's @username (its primary device only). Everything
    /// stored under the old name moves; the old name forwards to the new one
    /// for `RENAME_KEEP_SECS` and stays reserved for this account meanwhile.
    pub fn rename_user(&self, old: &str, identity_dh: &[u8; 32], new: &str) -> Result<String, DirError> {
        let old = old.trim().to_lowercase();
        let new = new.trim().trim_start_matches('@').to_lowercase();
        if !Self::valid_username(&new) {
            return Err(DirError::BadUsername);
        }
        if self.device_role(&old, identity_dh) != Some(true) || self.is_bot(&old) {
            return Err(DirError::NotOwner);
        }
        if new == old {
            return Ok(new);
        }
        let now = now_secs() as i64;
        let mut db = self.db.lock().unwrap();
        // Taken by someone, or someone's recent old name.
        let taken: bool = db.query_row("SELECT 1 FROM users WHERE username=?1", rusqlite::params![new], |_| Ok(())).is_ok();
        let my_uid: String = db.query_row("SELECT uid FROM users WHERE username=?1", rusqlite::params![old], |r| r.get(0)).unwrap_or_default();
        if taken || Self::reserved_elsewhere(&db, &new, &my_uid) {
            return Err(DirError::UsernameTaken);
        }
        let reserved_for: Option<String> = db
            .query_row(
                "SELECT new_name FROM renames WHERE old_name=?1 AND at > ?2",
                rusqlite::params![new, now - RENAME_KEEP_SECS as i64],
                |r| r.get(0),
            )
            .ok();
        if reserved_for.is_some_and(|n| n != old) {
            return Err(DirError::UsernameTaken);
        }
        // Not more often than once a day (it's announced to every contact).
        let last: Option<i64> = db
            .query_row("SELECT max(at) FROM renames WHERE new_name=?1", rusqlite::params![old], |r| r.get(0))
            .ok()
            .flatten();
        if last.is_some_and(|t| now - t < 24 * 3600) {
            return Err(DirError::RenameTooSoon);
        }
        Self::move_account(&mut db, &old, &new, true)?;
        Ok(new)
    }

    // ---- check marks (verify.rs) ----

    /// Has this account ID the check mark here?
    pub fn is_verified_uid(&self, uid: &str) -> bool {
        let db = self.db.lock().unwrap();
        db.query_row("SELECT 1 FROM verified WHERE uid=?1", rusqlite::params![uid], |_| Ok(())).is_ok()
    }

    /// Give or take the check mark by account ID (any relay's account).
    pub fn set_verified_uid(&self, uid: &str, on: bool) -> bool {
        let db = self.db.lock().unwrap();
        let r = if on {
            db.execute("INSERT INTO verified (uid, at) VALUES (?1, ?2) ON CONFLICT(uid) DO NOTHING", rusqlite::params![uid, now_secs() as i64])
        } else {
            db.execute("DELETE FROM verified WHERE uid=?1", rusqlite::params![uid])
        };
        r.is_ok()
    }

    /// The ID of `name` here or on another relay this one knows the names
    /// of (name registry).
    pub fn uid_anywhere(&self, name: &str) -> Option<String> {
        let name = name.trim().trim_start_matches('@').to_lowercase();
        if let Some(u) = self.uid_of(&name).filter(|u| !u.is_empty()) {
            return Some(u);
        }
        let db = self.db.lock().unwrap();
        db.query_row("SELECT uid FROM fed_names WHERE username=?1", rusqlite::params![name], |r| r.get::<_, String>(0)).ok().filter(|u| !u.is_empty())
    }

    /// The @name of an ID here or in the name registry.
    pub fn name_anywhere(&self, uid: &str) -> Option<String> {
        if let Some(n) = self.username_by_uid(uid) {
            return Some(n);
        }
        let db = self.db.lock().unwrap();
        db.query_row("SELECT username FROM fed_names WHERE uid=?1", rusqlite::params![uid], |r| r.get::<_, String>(0)).ok()
    }

    /// Give or take the check mark; false when there's no such account.
    pub fn set_verified(&self, username: &str, on: bool) -> bool {
        let Some(uid) = self.uid_of(username).filter(|u| !u.is_empty()) else { return false };
        let db = self.db.lock().unwrap();
        let r = if on {
            db.execute("INSERT INTO verified (uid, at) VALUES (?1, ?2) ON CONFLICT(uid) DO NOTHING", rusqlite::params![uid, now_secs() as i64])
        } else {
            db.execute("DELETE FROM verified WHERE uid=?1", rusqlite::params![uid])
        };
        r.is_ok()
    }

    /// "Clear the server" (panel → Settings): empties what's ticked.
    /// `accounts` is everything people left here — the accounts with their
    /// keys, profiles, two-step codes, backups, synced history, waiting
    /// messages, capsules, bots and check marks; the rest are pieces of it.
    /// Server settings (aqua-admin.json, mail, Telegram, music, network) stay.
    /// Returns (what, rows removed).
    pub fn wipe(&self, w: &Wipe) -> Vec<(&'static str, usize)> {
        let db = self.db.lock().unwrap();
        let mut out = Vec::new();
        let mut run = |what: &'static str, sql: &str| {
            let n = db.execute(sql, []).unwrap_or(0);
            out.push((what, n));
        };
        if w.accounts {
            for (what, table) in [
                ("accounts", "users"), ("keys", "prekeys"), ("profile pictures", "profile_media"), ("last seen", "last_seen"),
                ("backups", "backups"), ("synced history", "hist_store"), ("waiting messages", "inbox"), ("capsules", "capsules"),
                ("bot tokens", "bot_keys"), ("bot chats", "bot_peers"), ("renames", "renames"), ("check marks", "verified"),
                ("e-mail codes", "email_codes"), ("network names", "fed_names"), ("sign-up addresses", "signup_ips"),
            ] {
                run(what, &format!("DELETE FROM {table}"));
            }
        } else if w.people {
            // People go first; then whatever isn't a remaining (bot) account's.
            const GONE: &str = "NOT IN (SELECT username FROM users)";
            run("accounts", "DELETE FROM users WHERE is_bot=0");
            for (what, sql) in [
                ("keys", format!("DELETE FROM prekeys WHERE username {GONE}")),
                ("profile pictures", format!("DELETE FROM profile_media WHERE username {GONE}")),
                ("last seen", format!("DELETE FROM last_seen WHERE username {GONE}")),
                ("backups", format!("DELETE FROM backups WHERE username {GONE}")),
                ("synced history", format!("DELETE FROM hist_store WHERE username {GONE}")),
                ("waiting messages", format!("DELETE FROM inbox WHERE user {GONE}")),
                ("capsules", "DELETE FROM capsules".to_string()),
                ("bot chats", format!("DELETE FROM bot_peers WHERE user {GONE}")),
                ("renames", "DELETE FROM renames".to_string()),
                ("check marks", "DELETE FROM verified WHERE uid NOT IN (SELECT uid FROM users)".to_string()),
                ("e-mail codes", "DELETE FROM email_codes".to_string()),
                ("network names", format!("DELETE FROM fed_names WHERE username {GONE}")),
                ("sign-up addresses", "DELETE FROM signup_ips".to_string()),
            ] {
                run(what, &sql);
            }
        } else {
            if w.messages { run("waiting messages", "DELETE FROM inbox"); }
            if w.history { run("synced history", "DELETE FROM hist_store"); }
            if w.backups { run("backups", "DELETE FROM backups"); }
            if w.profiles { run("profile pictures", "DELETE FROM profile_media"); }
            if w.capsules { run("capsules", "DELETE FROM capsules"); }
            if w.bots {
                run("bot chats", "DELETE FROM bot_peers");
                run("bot tokens", "DELETE FROM bot_keys");
                run("bots", "DELETE FROM users WHERE is_bot=1");
            }
            if w.marks { run("check marks", "DELETE FROM verified"); }
            if w.codes { run("e-mail codes", "DELETE FROM email_codes"); }
        }
        drop(run);
        // Give the space back to the disk.
        let _ = db.execute_batch("VACUUM");
        out
    }

    /// Every person registered here (not bots), newest first — the panel's
    /// "Accounts" list. In a full server network that's the network's.
    pub fn accounts_list(&self) -> Vec<serde_json::Value> {
        let db = self.db.lock().unwrap();
        let Ok(mut st) = db.prepare(
            "SELECT u.username, u.display_name, u.uid, u.created_at, COALESCE(u.email, ''), COALESCE(l.at, 0), u.devices,
                    (SELECT 1 FROM bans b WHERE b.uid = u.uid)
             FROM users u LEFT JOIN last_seen l ON l.username = u.username WHERE u.is_bot = 0 ORDER BY u.created_at DESC",
        ) else { return Vec::new() };
        st.query_map([], |r| {
            let devices: String = r.get(6)?;
            let n = serde_json::from_str::<Vec<serde_json::Value>>(&devices).map(|v| v.len()).unwrap_or(0);
            Ok(serde_json::json!({
                "username": r.get::<_, String>(0)?, "name": r.get::<_, String>(1)?, "uid": r.get::<_, String>(2)?,
                "created_at": r.get::<_, i64>(3)?, "email": r.get::<_, String>(4)?, "seen": r.get::<_, i64>(5)?,
                "devices": n + 1, "banned": r.get::<_, Option<i64>>(7)?.is_some(),
            }))
        })
        .map(|it| it.filter_map(|x| x.ok()).collect())
        .unwrap_or_default()
    }

    /// Banned here, including accounts already deleted (they'd come back).
    pub fn bans_list(&self) -> Vec<serde_json::Value> {
        let db = self.db.lock().unwrap();
        let Ok(mut st) = db.prepare("SELECT uid, name, at FROM bans ORDER BY at DESC") else { return Vec::new() };
        st.query_map([], |r| Ok(serde_json::json!({ "uid": r.get::<_, String>(0)?, "username": r.get::<_, String>(1)?, "at": r.get::<_, i64>(2)? })))
            .map(|it| it.filter_map(|x| x.ok()).collect())
            .unwrap_or_default()
    }

    /// Can `username` (a person of the network: an account row here, local
    /// or copied) send through a peer? Not unknown, not banned, not a bot
    /// (A8: a relay of the network named any sender it liked).
    pub fn may_send_from_peer(&self, username: &str) -> bool {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let row: Option<(String, Vec<u8>, i64)> = db
            .query_row("SELECT uid, identity_dh, is_bot FROM users WHERE username=?1", rusqlite::params![username], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .ok();
        drop(db);
        match row {
            Some((uid, dh, 0)) => !self.is_banned_uid(&uid) && !self.is_banned_dh(&dh),
            _ => false,
        }
    }

    /// Ban `username` (a person, not a bot): its ID and every identity key it
    /// has now. Returns the ID, or None when there's no such person.
    pub fn ban(&self, username: &str) -> Option<String> {
        let name = username.trim().trim_start_matches('@').to_lowercase();
        let row: Option<(String, Vec<u8>, String)> = {
            let db = self.db.lock().unwrap();
            db.query_row("SELECT uid, identity_dh, devices FROM users WHERE username=?1 AND is_bot=0", rusqlite::params![name], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .ok()
        };
        let (uid, dh, devices) = row?;
        let uid = if uid.is_empty() { uid_for(&dh) } else { uid };
        let mut dhs = vec![hex::encode(&dh)];
        if let Ok(list) = serde_json::from_str::<Vec<Device>>(&devices) {
            dhs.extend(list.into_iter().map(|d| d.dh));
        }
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT INTO bans (uid, dhs, name, at) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(uid) DO UPDATE SET dhs=?2, name=?3",
            rusqlite::params![uid, dhs.join(","), name, now_secs() as i64],
        )
        .ok()?;
        ANY_BANS.store(true, std::sync::atomic::Ordering::Relaxed);
        Some(uid)
    }

    /// Lift a ban (by account ID or @name); false when there was none.
    pub fn unban(&self, who: &str) -> bool {
        let w = who.trim().trim_start_matches('@').to_lowercase();
        let db = self.db.lock().unwrap();
        let n = db.execute("DELETE FROM bans WHERE uid=?1 OR name=?1", rusqlite::params![w]).unwrap_or(0);
        let left: i64 = db.query_row("SELECT COUNT(*) FROM bans", [], |r| r.get(0)).unwrap_or(0);
        ANY_BANS.store(left > 0, std::sync::atomic::Ordering::Relaxed);
        n > 0
    }

    /// Is this identity key (an account's or one of its devices') banned here?
    pub fn is_banned_dh(&self, dh: &[u8]) -> bool {
        if !ANY_BANS.load(std::sync::atomic::Ordering::Relaxed) {
            return false;
        }
        let db = self.db.lock().unwrap();
        db.query_row(
            "SELECT 1 FROM bans WHERE uid=?1 OR instr(dhs, ?2) > 0",
            rusqlite::params![uid_for(dh), hex::encode(dh)],
            |_| Ok(()),
        )
        .is_ok()
    }

    pub fn is_banned_uid(&self, uid: &str) -> bool {
        if !ANY_BANS.load(std::sync::atomic::Ordering::Relaxed) {
            return false;
        }
        let db = self.db.lock().unwrap();
        db.query_row("SELECT 1 FROM bans WHERE uid=?1", rusqlite::params![uid], |_| Ok(())).is_ok()
    }

    /// The admin deletes one person and everything they left here: the
    /// account, keys, profile pictures, backups, synced history, waiting
    /// messages (also to their devices), capsules, bot chats, check mark,
    /// network name. Their bots stay. Returns false when there's no such person.
    pub fn delete_person(&self, username: &str) -> bool {
        let name = username.trim().trim_start_matches('@').to_lowercase();
        let db = self.db.lock().unwrap();
        let Ok(uid) = db.query_row("SELECT uid FROM users WHERE username=?1 AND is_bot=0", rusqlite::params![name], |r| r.get::<_, String>(0)) else {
            return false;
        };
        let dev = format!("{name}~%");
        for sql in [
            "DELETE FROM users WHERE username=?1",
            "DELETE FROM prekeys WHERE username=?1",
            "DELETE FROM profile_media WHERE username=?1",
            "DELETE FROM last_seen WHERE username=?1",
            "DELETE FROM backups WHERE username=?1",
            "DELETE FROM hist_store WHERE username=?1",
            "DELETE FROM inbox WHERE user=?1",
            "DELETE FROM capsules WHERE owner=?1 OR recipient=?1",
            "DELETE FROM bot_peers WHERE user=?1",
            "DELETE FROM fed_names WHERE username=?1",
            "DELETE FROM renames WHERE new_name=?1",
        ] {
            let _ = db.execute(sql, rusqlite::params![name]);
        }
        let _ = db.execute("DELETE FROM inbox WHERE user LIKE ?1", rusqlite::params![dev]);
        if !uid.is_empty() {
            let _ = db.execute("DELETE FROM verified WHERE uid=?1", rusqlite::params![uid]);
        }
        true
    }

    /// Who has a check mark here (current @names).
    pub fn verified_names(&self) -> Vec<String> {
        let db = self.db.lock().unwrap();
        // Accounts of other relays too (the star relay's marks hold everywhere).
        let Ok(mut st) = db.prepare(
            "SELECT COALESCE(u.username, f.username, 'ID ' || v.uid) FROM verified v
             LEFT JOIN users u ON u.uid = v.uid AND u.is_bot = 0 LEFT JOIN fed_names f ON f.uid = v.uid ORDER BY v.at",
        ) else { return Vec::new() };
        st.query_map([], |r| r.get::<_, String>(0)).map(|it| it.filter_map(|x| x.ok()).collect()).unwrap_or_default()
    }

    /// The account's e-mail (lower-case), if it signed up with one.
    /// Accounts here with this e-mail (Boosty stars, stars.rs).
    pub fn usernames_by_email(&self, email: &str) -> Vec<String> {
        let db = self.db.lock().unwrap();
        let Ok(mut q) = db.prepare("SELECT username FROM users WHERE lower(trim(email))=?1 AND is_bot=0") else { return Vec::new() };
        q.query_map(rusqlite::params![email.trim().to_lowercase()], |r| r.get::<_, String>(0))
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default()
    }

    /// Every person's (name, e-mail) — to find the owner (verify.rs).
    pub fn accounts_with_email(&self) -> Vec<(String, String)> {
        let db = self.db.lock().unwrap();
        let Ok(mut q) = db.prepare("SELECT username, email FROM users WHERE email IS NOT NULL AND email<>'' AND is_bot=0") else { return Vec::new() };
        q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map(|rows| rows.flatten().map(|(n, e)| (n, e.trim().to_lowercase())).collect())
            .unwrap_or_default()
    }

    pub fn email_of(&self, username: &str) -> Option<String> {
        let db = self.db.lock().unwrap();
        db.query_row("SELECT email FROM users WHERE username=?1", rusqlite::params![username.trim().to_lowercase()], |r| r.get::<_, Option<String>>(0))
            .ok()
            .flatten()
            .map(|e| e.trim().to_lowercase())
            .filter(|e| !e.is_empty())
    }

    /// The current name of a renamed account (`old` forwards for a while).
    pub fn renamed_to(&self, old: &str) -> Option<String> {
        let db = self.db.lock().unwrap();
        db.query_row(
            "SELECT new_name FROM renames WHERE old_name=?1 AND at > ?2",
            rusqlite::params![old.trim().to_lowercase(), now_secs() as i64 - RENAME_KEEP_SECS as i64],
            |r| r.get(0),
        )
        .ok()
    }

    /// Fetch a user's avatar / banner: `(mime, bytes)`.
    pub fn get_media(&self, username: &str, kind: MediaKind) -> Option<(String, Vec<u8>)> {
        let username = username.trim().to_lowercase();
        let db = self.db.lock().unwrap();
        db.query_row(
            "SELECT mime, data FROM profile_media WHERE username=?1 AND kind=?2",
            rusqlite::params![username, kind.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok()
    }

    // ---- lookup / search ----

    pub fn get_profile(&self, username: &str) -> Option<Profile> {
        let username = username.trim().to_lowercase();
        let db = self.db.lock().unwrap();
        db.query_row(
            &format!("SELECT {PROFILE_COLS} FROM users WHERE username=?1"),
            rusqlite::params![username],
            profile_from_row,
        ).ok()
    }

    /// Prefix / substring search over username + display name.
    pub fn search(&self, query: &str, limit: usize) -> Vec<Profile> {
        // "@ivan" is "ivan"; "_" and "%" are literal characters of a name.
        let q = query.trim().trim_start_matches('@').to_lowercase();
        if q.is_empty() { return Vec::new(); }
        let esc = q.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
        let like = format!("%{esc}%");
        // An old @username still forwarding: show the account under its new one.
        let renamed = self.renamed_to(&q);
        let db = self.db.lock().unwrap();
        let sql = format!(
            "SELECT {PROFILE_COLS} FROM users
             WHERE username LIKE ?1 ESCAPE '\\' OR ulower(display_name) LIKE ?1 ESCAPE '\\' OR username = ?4 OR uid = ?3
             ORDER BY (username = ?3 OR username = ?4 OR uid = ?3) DESC, username LIMIT ?2"
        );
        let mut stmt = match db.prepare(&sql) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let rows = stmt.query_map(rusqlite::params![like, limit as i64, q, renamed.unwrap_or_default()], profile_from_row);
        match rows {
            Ok(it) => it.filter_map(|r| r.ok()).collect(),
            Err(_) => Vec::new(),
        }
    }

    // ---- bots ----

    /// Register a bot account owned by `owner_dh`. The bot is a normal
    /// directory user with `is_bot = 1`; a random 192-bit token is minted
    /// for the bot program. Only its SHA-256 is stored, so the token is
    /// shown once (the owner's client keeps it in its encrypted vault).
    /// The username must be free, or already a bot owned by the same
    /// identity, in which case the token is rotated.
    pub fn register_bot(
        &self,
        username: &str,
        owner_dh: &[u8; 32],
        display_name: &str,
    ) -> Result<String, DirError> {
        self.register_bot_with_limit(username, owner_dh, display_name, MAX_BOTS_PER_OWNER)
    }

    pub(crate) fn register_bot_with_limit(
        &self,
        username: &str,
        owner_dh: &[u8; 32],
        display_name: &str,
        max: i64,
    ) -> Result<String, DirError> {
        let username = username.trim().to_lowercase();
        if !Self::valid_username(&username) {
            return Err(DirError::BadUsername);
        }
        let display_name: String = display_name.trim().chars().take(64).collect();
        let mut connection = self.db.lock().unwrap();
        // Reserve the writer before reading the name and quota. This lets
        // SQLite wait for a concurrent writer instead of failing on upgrade.
        let db = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        // A public group's / channel's @name.
        if db.query_row("SELECT 1 FROM group_names WHERE username=?1", rusqlite::params![username], |_| Ok(())).is_ok() {
            return Err(DirError::UsernameTaken);
        }
        // If the username exists and isn't a bot owned by us, refuse.
        let mut existing = false;
        if let Ok((owner, is_bot)) = db.query_row(
            "SELECT identity_dh, is_bot FROM users WHERE username=?1",
            rusqlite::params![username],
            |r| Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, i64>(1)?)),
        ) {
            if is_bot == 0 || owner.as_slice() != owner_dh.as_slice() {
                return Err(DirError::UsernameTaken);
            }
            existing = true;
        }
        if !existing {
            let n: i64 = db
                .query_row("SELECT COUNT(*) FROM bot_keys WHERE owner_dh=?1", rusqlite::params![owner_dh.as_slice()], |r| r.get(0))?;
            if n >= max {
                return Err(DirError::TooManyBots);
            }
        }
        // Upsert the bot as a directory user.
        db.execute(
            "INSERT INTO users (username, identity_dh, email, display_name, about, phone, is_bot, created_at)
             VALUES (?1, ?2, NULL, ?3, '', '', 1, ?4)
             ON CONFLICT(username) DO UPDATE SET display_name=?3, is_bot=1",
            rusqlite::params![username, owner_dh.as_slice(), display_name, now_secs() as i64],
        )?;
        Self::fill_uids(&db);
        // Rotate any old token for this bot, mint a fresh one.
        db.execute("DELETE FROM bot_keys WHERE username=?1", rusqlite::params![username])?;
        let mut t = [0u8; 24];
        OsRng.fill_bytes(&mut t);
        let token = format!("aqbot_{}", hex::encode(t));
        db.execute(
            "INSERT INTO bot_keys (token_hash, username, owner_dh, created_at, hint) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![bot_token_hash(&token), username, owner_dh.as_slice(), now_secs() as i64, token_hint(&token)],
        )?;
        db.commit()?;
        tracing::info!(%username, "bot registered");
        Ok(token)
    }

    /// Delete a bot the caller owns (its account, token and contacts).
    pub fn delete_bot(&self, username: &str, owner_dh: &[u8; 32]) -> Result<(), DirError> {
        let username = username.trim().to_lowercase();
        let db = self.db.lock().unwrap();
        let owner: Vec<u8> = db
            .query_row("SELECT owner_dh FROM bot_keys WHERE username=?1", rusqlite::params![username], |r| r.get(0))
            .map_err(|_| DirError::NotOwner)?;
        if owner.as_slice() != owner_dh.as_slice() {
            return Err(DirError::NotOwner);
        }
        db.execute("DELETE FROM bot_keys WHERE username=?1", rusqlite::params![username])?;
        db.execute("DELETE FROM bot_peers WHERE bot=?1", rusqlite::params![username])?;
        db.execute("DELETE FROM users WHERE username=?1 AND is_bot=1", rusqlite::params![username])?;
        tracing::info!(%username, "bot deleted");
        Ok(())
    }

    /// How many accounts were made from this address (`ip` is a hash).
    pub fn signups_from(&self, ip: &str) -> u32 {
        let db = self.db.lock().unwrap();
        // Counted over the last day: `at` is the latest sign-up.
        let day_ago = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0) - 86_400;
        db.query_row("SELECT n FROM signup_ips WHERE ip=?1 AND at > ?2", rusqlite::params![ip, day_ago], |r| r.get::<_, i64>(0)).map(|n| n as u32).unwrap_or(0)
    }

    /// An account was made from this address.
    pub fn note_signup(&self, ip: &str) {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        let db = self.db.lock().unwrap();
        let _ = db.execute(
            "INSERT INTO signup_ips (ip, n, at) VALUES (?1, 1, ?2) ON CONFLICT(ip) DO UPDATE SET n = CASE WHEN at > ?2 - 86400 THEN n + 1 ELSE 1 END, at = ?2",
            rusqlite::params![ip, now],
        );
    }

    /// The server's admin deletes any bot (the panel → Bots). The relay's
    /// own bots (`system`) are made again at start, so they stay.
    pub fn admin_delete_bot(&self, username: &str, system: &[u8; 32]) -> Result<bool, DirError> {
        let username = username.trim().to_lowercase();
        let db = self.db.lock().unwrap();
        let owner: Option<Vec<u8>> = db
            .query_row("SELECT owner_dh FROM bot_keys WHERE username=?1", rusqlite::params![username], |r| r.get(0))
            .ok();
        if owner.as_deref() == Some(system.as_slice()) {
            return Err(DirError::NotOwner);
        }
        db.execute("DELETE FROM bot_keys WHERE username=?1", rusqlite::params![username])?;
        db.execute("DELETE FROM bot_peers WHERE bot=?1", rusqlite::params![username])?;
        let n = db.execute("DELETE FROM users WHERE username=?1 AND is_bot=1", rusqlite::params![username])?;
        tracing::info!(%username, "bot deleted by the admin");
        Ok(n > 0)
    }

    /// The relay's own bots (verification, music, stars) — not deletable.
    pub fn system_bots(&self, system: &[u8; 32]) -> Vec<String> {
        let db = self.db.lock().unwrap();
        let Ok(mut stmt) = db.prepare("SELECT username FROM bot_keys WHERE owner_dh=?1") else { return Vec::new() };
        let rows = stmt.query_map(rusqlite::params![system.to_vec()], |r| r.get::<_, String>(0));
        match rows { Ok(it) => it.filter_map(|r| r.ok()).collect(), Err(_) => Vec::new() }
    }

    /// `user` wrote to `bot`: from now on the bot may answer them.
    pub fn bot_peer_add(&self, bot: &str, user: &str) {
        let db = self.db.lock().unwrap();
        let _ = db.execute(
            "INSERT INTO bot_peers (bot, user, at) VALUES (?1, ?2, ?3) ON CONFLICT(bot, user) DO UPDATE SET at=?3",
            rusqlite::params![bot, user, now_secs() as i64],
        );
    }

    /// The identity that owns `bot`.
    pub fn bot_owner(&self, bot: &str) -> Option<Vec<u8>> {
        let db = self.db.lock().unwrap();
        db.query_row("SELECT owner_dh FROM bot_keys WHERE username=?1", rusqlite::params![bot], |r| r.get(0)).ok()
    }

    /// `user` blocked / stopped `bot` (sent /stop): no more messages.
    pub fn bot_peer_remove(&self, bot: &str, user: &str) {
        let db = self.db.lock().unwrap();
        let _ = db.execute("DELETE FROM bot_peers WHERE bot=?1 AND user=?2", rusqlite::params![bot, user]);
    }

    /// A bot may message people who wrote to it first, and its owner.
    pub fn bot_may_message(&self, bot: &str, user: &str) -> bool {
        if bot == user {
            return true; // a console bot echoing to itself
        }
        let db = self.db.lock().unwrap();
        let peer = db
            .query_row("SELECT 1 FROM bot_peers WHERE bot=?1 AND user=?2", rusqlite::params![bot, user], |_| Ok(()))
            .is_ok();
        peer || db
            .query_row(
                "SELECT 1 FROM bot_keys k JOIN users u ON u.identity_dh = k.owner_dh AND u.is_bot = 0
                 WHERE k.username=?1 AND u.username=?2",
                rusqlite::params![bot, user],
                |_| Ok(()),
            )
            .is_ok()
    }

    /// Everyone who wrote to the bot (and didn't send /stop).
    pub fn bot_peers_of(&self, bot: &str) -> Vec<String> {
        let db = self.db.lock().unwrap();
        let Ok(mut st) = db.prepare("SELECT user FROM bot_peers WHERE bot=?1 ORDER BY at DESC LIMIT 5000") else { return Vec::new() };
        st.query_map(rusqlite::params![bot], |r| r.get::<_, String>(0))
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default()
    }

    /// The @username of the bot's owner.
    pub fn bot_owner_name(&self, bot: &str) -> Option<String> {
        let db = self.db.lock().unwrap();
        db.query_row(
            "SELECT u.username FROM bot_keys k JOIN users u ON u.identity_dh = k.owner_dh AND u.is_bot = 0 WHERE k.username=?1",
            rusqlite::params![bot],
            |r| r.get(0),
        )
        .ok()
    }

    /// Display name of a user / bot ("" when unknown).
    pub fn display_name_of(&self, username: &str) -> String {
        let db = self.db.lock().unwrap();
        db.query_row("SELECT display_name FROM users WHERE username=?1", rusqlite::params![username], |r| r.get(0))
            .unwrap_or_default()
    }

    /// Resolve a bot token → its username.
    pub fn bot_by_token(&self, token: &str) -> Option<String> {
        if !token.starts_with("aqbot_") || token.len() > 80 {
            return None;
        }
        let db = self.db.lock().unwrap();
        db.query_row(
            "SELECT username FROM bot_keys WHERE token_hash=?1",
            rusqlite::params![bot_token_hash(token)],
            |r| r.get::<_, String>(0),
        ).ok()
    }

    /// Is there an account (user or bot) with this name?
    pub fn user_exists(&self, username: &str) -> bool {
        let username = username.trim().to_lowercase();
        let db = self.db.lock().unwrap();
        db.query_row("SELECT 1 FROM users WHERE username=?1", rusqlite::params![username], |_| Ok(())).is_ok()
    }

    /// Is this username a registered bot?
    pub fn is_bot(&self, username: &str) -> bool {
        let username = username.trim().to_lowercase();
        let db = self.db.lock().unwrap();
        db.query_row(
            "SELECT is_bot FROM users WHERE username=?1",
            rusqlite::params![username],
            |r| r.get::<_, i64>(0),
        ).map(|v| v != 0).unwrap_or(false)
    }

    /// List bots owned by an identity: (username, last 4 characters of
    /// the token) — the token itself is not stored.
    pub fn list_bots_for_owner(&self, owner_dh: &[u8; 32]) -> Vec<(String, String)> {
        let db = self.db.lock().unwrap();
        let mut stmt = match db.prepare(
            "SELECT u.username, u.display_name, bk.hint
             FROM bot_keys bk JOIN users u ON u.username = bk.username
             WHERE bk.owner_dh = ?1 ORDER BY u.username",
        ) { Ok(s) => s, Err(_) => return Vec::new() };
        let rows = stmt.query_map(rusqlite::params![owner_dh.as_slice()], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(2)?))
        });
        match rows { Ok(it) => it.filter_map(|r| r.ok()).collect(), Err(_) => Vec::new() }
    }

    /// Every bot: (username, display name). For the admin panel.
    pub fn all_bots(&self) -> Vec<(String, String)> {
        let db = self.db.lock().unwrap();
        let Ok(mut stmt) = db.prepare("SELECT username, display_name FROM users WHERE is_bot=1 ORDER BY username") else { return Vec::new() };
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?.unwrap_or_default())));
        match rows { Ok(it) => it.filter_map(|r| r.ok()).collect(), Err(_) => Vec::new() }
    }

    /// (total registered users incl. bots, bot count). For the admin GUI.
    pub fn counts(&self) -> (i64, i64) {
        let db = self.db.lock().unwrap();
        let users: i64 = db
            .query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))
            .unwrap_or(0);
        let bots: i64 = db
            .query_row("SELECT COUNT(*) FROM users WHERE is_bot=1", [], |r| r.get(0))
            .unwrap_or(0);
        (users, bots)
    }

    /// Return the identity bound to a username, if registered. Lets the
    /// relay enforce that a registered `@username` is only usable by its
    /// owner identity even across restarts.
    pub fn identity_for(&self, username: &str) -> Option<[u8; 32]> {
        let username = username.trim().to_lowercase();
        let db = self.db.lock().unwrap();
        let v: Vec<u8> = db
            .query_row(
                "SELECT identity_dh FROM users WHERE username=?1",
                rusqlite::params![username],
                |r| r.get(0),
            )
            .ok()?;
        v.as_slice().try_into().ok()
    }
}

/// Send a verification code via SMTP using lettre.
/// The verification e-mail (plain text + HTML), ready to send.
fn build_code_email(cfg: &SmtpConfig, to: &str, code: &str, link: Option<&str>) -> anyhow::Result<lettre::Message> {
    use lettre::message::{Message, MultiPart};
    let link_text = link.map(|l| format!("Или нажмите — код подставится в приложении сам: {l}\nOr tap — the app fills the code in by itself.\n\n")).unwrap_or_default();
    let text = format!(
        "Aquamarine\n\n\
         Ваш код подтверждения: {code}\n\
         Your verification code: {code}\n\n\
         {link_text}\
         Код действует 10 минут. Если вы его не запрашивали, просто проигнорируйте это письмо.\n\
         It expires in 10 minutes. If you didn't request it, ignore this email."
    );
    let link_html = link.map(|l| format!(r#"<tr><td align="center" style="padding:0 28px 20px">
<a href="{l}" style="display:inline-block;padding:13px 26px;border-radius:12px;background:#00E5C7;background-image:linear-gradient(135deg,#00E5C7,#7B5CFF);color:#06070C;font-weight:800;font-size:15px;text-decoration:none">Это я — подставить код</a>
<div style="color:#6b7488;font-size:12px;padding-top:8px">It's me — fill the code in the app</div>
</td></tr>"#)).unwrap_or_default();
    let spaced: String = code.chars().map(|c| c.to_string()).collect::<Vec<_>>().join("&#8202;");
    let html = format!(r#"<!doctype html><html><body style="margin:0;padding:0;background:#06070C">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="background:#06070C;padding:32px 12px;font-family:Segoe UI,Roboto,Helvetica,Arial,sans-serif">
<tr><td align="center">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="max-width:440px;background:#0C0F17;border:1px solid #2b3346;border-radius:18px">
<tr><td style="padding:28px 28px 8px">
<table role="presentation" cellpadding="0" cellspacing="0"><tr>
<td style="width:40px;height:40px;border-radius:12px;background:#00E5C7;background-image:linear-gradient(135deg,#00E5C7,#7B5CFF);color:#06070C;font-weight:900;font-size:19px;text-align:center;vertical-align:middle">A</td>
<td style="padding-left:12px;color:#eef1f7;font-size:18px;font-weight:800">Aquamarine</td></tr></table>
</td></tr>
<tr><td style="padding:14px 28px 0;color:#eef1f7;font-size:16px;font-weight:700">Ваш код подтверждения</td></tr>
<tr><td style="padding:2px 28px 0;color:#6b7488;font-size:13px">Your verification code</td></tr>
<tr><td style="padding:18px 28px">
<div style="background:#141926;border:1px solid #1f2535;border-radius:14px;padding:18px;text-align:center;font-family:Consolas,Menlo,monospace;font-size:34px;font-weight:800;letter-spacing:8px;color:#00E5C7">{spaced}</div>
</td></tr>
{link_html}
<tr><td style="padding:0 28px 26px;color:#aab2c5;font-size:13px;line-height:1.55">
Код действует 10 минут. Если вы его не запрашивали — просто проигнорируйте письмо.<br>
<span style="color:#6b7488">It expires in 10 minutes. If you didn't request it, ignore this email.</span>
</td></tr>
</table>
<div style="color:#6b7488;font-size:11px;padding-top:14px">Aquamarine · end-to-end encrypted messenger</div>
</td></tr></table></body></html>"#);
    Ok(Message::builder()
        .from(cfg.from_header().parse()?)
        .to(to.parse()?)
        .subject(format!("{code} — код Aquamarine"))
        .multipart(MultiPart::alternative_plain_html(text, html))?)
}

/// Result of asking for a code.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CodeDelivery {
    /// Only in dev mode (no SMTP): the code itself.
    pub dev_code: Option<String>,
    /// SMTP is set up but the mail couldn't be sent: the code is in the
    /// server console / admin panel.
    pub delivery_failed: bool,
    pub error: Option<String>,
}

/// Send over the configured port; if that fails, over the other one
/// (587 STARTTLS ↔ 465 TLS). VPNs and some networks break one of them —
/// "incomplete response" on 587 is the usual sign.
///
/// IPv4 addresses go first: smtp.gmail.com answers with an IPv6 address
/// first, and on a machine whose IPv6 is broken the connection hung until
/// the whole attempt ran out of time ("the mail server didn't answer in
/// time") without ever trying IPv4. The certificate is still checked
/// against the server's name.
async fn send_smtp_any_port(cfg: &SmtpConfig, to: &str, code: &str, link: Option<&str>) -> anyhow::Result<()> {
    let alt = match cfg.port { 465 => Some(587), 587 | 25 | 2525 => Some(465), _ => None };
    let mut errors: Vec<String> = Vec::new();
    for port in std::iter::once(cfg.port).chain(alt) {
        let addrs = smtp_addrs(&cfg.host, port).await;
        // Per address: a short wait, so a dead address leaves time for the next.
        let targets: Vec<Option<String>> = if addrs.is_empty() { vec![None] } else { addrs.into_iter().map(Some).collect() };
        for target in targets {
            let mut c = cfg.clone();
            c.port = port;
            let at = target.clone().unwrap_or_else(|| cfg.host.clone());
            match tokio::time::timeout(std::time::Duration::from_secs(10), send_smtp(&c, target.as_deref(), to, code, link)).await {
                Ok(Ok(())) => {
                    if port != cfg.port {
                        tracing::info!(port, "SMTP works on the other port — consider switching to it in the admin panel");
                    }
                    return Ok(());
                }
                Ok(Err(e)) => {
                    let s = e.to_string();
                    // Wrong login or password: other addresses won't help.
                    if s.contains("535") || s.to_lowercase().contains("authentication") {
                        return Err(anyhow::anyhow!("the mail server refused the login or password: {s}"));
                    }
                    tracing::warn!(%at, port, error = %s, "SMTP attempt failed");
                    errors.push(format!("{at}:{port}: {s}"));
                }
                Err(_) => {
                    tracing::warn!(%at, port, "SMTP attempt timed out");
                    errors.push(format!("{at}:{port}: no answer"));
                }
            }
        }
    }
    Err(anyhow::anyhow!("couldn't reach the mail server ({}) — a firewall, the provider or a VPN may block ports 587/465", errors.join("; ")))
}

/// The server's addresses, IPv4 first (at most two of each family).
async fn smtp_addrs(host: &str, port: u16) -> Vec<String> {
    let Ok(Ok(it)) = tokio::time::timeout(std::time::Duration::from_secs(5), tokio::net::lookup_host((host, port))).await else {
        return Vec::new();
    };
    let all: Vec<std::net::SocketAddr> = it.collect();
    let v4 = all.iter().filter(|a| a.is_ipv4()).take(2);
    let v6 = all.iter().filter(|a| a.is_ipv6()).take(1);
    v4.chain(v6).map(|a| a.ip().to_string()).collect()
}

/// `at`: connect to this address instead of resolving `cfg.host` (the TLS
/// certificate is checked against `cfg.host` either way).
async fn send_smtp(cfg: &SmtpConfig, at: Option<&str>, to: &str, code: &str, link: Option<&str>) -> anyhow::Result<()> {
    use lettre::{
        transport::smtp::authentication::Credentials,
        AsyncSmtpTransport, AsyncTransport, Tokio1Executor,
    };
    let email = build_code_email(cfg, to, code, link)?;
    // Gmail shows app passwords as "abcd efgh ijkl mnop" and people paste them
    // like that; the real password has no spaces (Gmail answers 535 otherwise).
    let pass: String = cfg.pass.chars().filter(|c| !c.is_whitespace()).collect();
    let creds = Credentials::new(cfg.user.trim().to_string(), pass);
    use lettre::transport::smtp::client::{Tls, TlsParameters};
    // 465 = implicit TLS (SSL); anything else (587) = STARTTLS.
    let params = TlsParameters::new(cfg.host.clone())?;
    let tls = if cfg.port == 465 { Tls::Wrapper(params) } else { Tls::Required(params) };
    let mailer: AsyncSmtpTransport<Tokio1Executor> = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(at.unwrap_or(&cfg.host))
        .port(cfg.port)
        .tls(tls)
        .credentials(creds)
        .timeout(Some(std::time::Duration::from_secs(10)))
        .build();
    mailer.send(email).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> Directory { Directory::open_memory() }

    fn peer_user(name: &str, uid: &str, created_at: i64, updated_at: i64) -> FedUser {
        let dh = aqua_core::keys::IdentityKeyPair::generate();
        FedUser {
            username: name.into(), identity_dh: hex::encode(dh.public().dh_pub.as_bytes()), email: None, display_name: name.into(),
            about: String::new(), phone: String::new(), is_bot: false, created_at, extra: String::new(), avatar_ver: 0, banner_ver: 0,
            media_acl: String::new(), password: String::new(), music_ver: 0, updated_at, devices: "[]".into(), recovery: String::new(),
            uid: uid.into(), security: String::new(), devlist: String::new(), seal: String::new(),
        }
    }
    fn tomb(uid: &str, name: &str, created_at: i64) -> Tomb {
        Tomb { uid: uid.into(), created_at, username: name.into(), deleted_at: created_at + 100, origin: "o".into(), sig: "s".into(), got_at: 0 }
    }

    /// A7: a tombstone covers the generation it names: a copy of that
    /// account (or an older one) from a relay that didn't hear of the
    /// deletion isn't taken again; a later sign-up with the same ID is.
    #[test]
    fn tombstone_blocks_old_generation_but_not_reregistration() {
        let d = dir();
        let u = peer_user("gone", "1000000001", 1000, 5000);
        assert!(d.fed_apply_user(&u).applied);
        assert_eq!(d.apply_tomb(&tomb("1000000001", "gone", 1000)).unwrap().as_deref(), Some("gone"), "deleted here");
        assert!(d.identity_for("gone").is_none());
        assert_eq!(d.apply_tomb(&tomb("1000000001", "gone", 1000)).unwrap(), None, "twice: nothing more");
        // An old relay sends it again, even "changed later".
        let again = FedUser { updated_at: 9_000, ..u.clone() };
        let r = d.fed_apply_user(&again);
        assert!(!r.applied && !r.failed, "not taken, and not an error (the cursor moves on)");
        assert!(d.identity_for("gone").is_none());
        // A new sign-up (same ID, later creation): taken.
        let new = FedUser { created_at: 2000, updated_at: 9_500, ..peer_user("gone", "1000000001", 2000, 9_500) };
        assert!(d.fed_apply_user(&new).applied);
        assert!(d.identity_for("gone").is_some());
        assert!(!d.uid_deleted("1000000001"), "signed up again: a pass for it works");
        assert!(d.uid_deleted("1000000009") == false);
        d.add_tomb(&tomb("1000000009", "x", 10)).unwrap();
        assert!(d.uid_deleted("1000000009"));
    }

    /// A7: a deleted account's name from the registry isn't taken again and
    /// claims nothing against a local account with that name.
    #[test]
    fn tombstone_blocks_name_registry_resurrection() {
        let d = dir();
        let row = NameRow { username: "taken".into(), uid: "2000000001".into(), home: "http://old".into(), created_at: 100, updated_at: 200 };
        d.fed_apply_name(&row, "http://old");
        assert!(d.names_after(0, None, 10).iter().any(|r| r.username == "taken"));
        d.apply_tomb(&tomb("2000000001", "taken", 100)).unwrap();
        assert!(!d.names_after(0, None, 10).iter().any(|r| r.uid == "2000000001"), "gone from the registry");
        d.fed_apply_name(&row, "http://old");
        assert!(!d.names_after(0, None, 10).iter().any(|r| r.uid == "2000000001"), "an old relay can't bring it back");
        // Someone here takes the name later; the old (deleted, older) one doesn't move them.
        let mine = peer_user("taken", "2000000002", 500, 600);
        assert!(d.fed_apply_user(&mine).applied);
        assert_eq!(d.fed_apply_name(&row, "http://old"), None);
        assert!(d.identity_for("taken").is_some());
    }

    /// Several processes open one database with the triggers of an older
    /// build at once (a restart, the tests): each opens, the new triggers
    /// end up in place (CI caught "trigger already exists").
    #[test]
    fn old_triggers_are_replaced_by_many_openers_at_once() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("d.sqlite").to_string_lossy().to_string();
        drop(Directory::open_path(&path).unwrap());
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch("DROP TRIGGER users_touch_ins; DROP TRIGGER users_touch_upd;
                CREATE TRIGGER users_touch_ins AFTER INSERT ON users WHEN NEW.updated_at = 0
                BEGIN UPDATE users SET updated_at = CAST(strftime('%s','now') AS INTEGER) * 1000 + (abs(random()) % 1000) WHERE username = NEW.username; END;
                CREATE TRIGGER users_touch_upd AFTER UPDATE ON users WHEN NEW.updated_at = OLD.updated_at
                BEGIN UPDATE users SET updated_at = max(OLD.updated_at + 1, CAST(strftime('%s','now') AS INTEGER) * 1000) WHERE username = NEW.username; END;").unwrap();
        }
        let threads: Vec<_> = (0..8).map(|_| { let p = path.clone(); std::thread::spawn(move || Directory::open_path(&p).map(|_| ()).map_err(|e| e.to_string())) }).collect();
        for t in threads { t.join().unwrap().unwrap(); }
        let c = Connection::open(&path).unwrap();
        for name in ["users_touch_ins", "users_touch_upd"] {
            let sql: String = c.query_row("SELECT sql FROM sqlite_master WHERE type='trigger' AND name=?1", [name], |r| r.get(0)).unwrap();
            assert!(sql.contains("julianday"), "{name}: {sql}");
        }
    }

    /// Rows made here one after another get increasing change times: one
    /// made in the same second used to get a random millisecond, could land
    /// before a peer's cursor and was never pulled.
    #[test]
    fn local_changes_get_increasing_times() {
        let d = dir();
        let db = d.db.lock().unwrap();
        let mut last = 0i64;
        for i in 0..200 {
            db.execute("INSERT INTO users (username, identity_dh, created_at) VALUES (?1, ?2, 1)", rusqlite::params![format!("n{i}"), vec![i as u8; 32]]).unwrap();
            let t: i64 = db.query_row("SELECT updated_at FROM users WHERE username=?1", rusqlite::params![format!("n{i}")], |r| r.get(0)).unwrap();
            assert!(t > last, "{i}: {t} after {last}");
            last = t;
        }
        db.execute("UPDATE users SET display_name='x' WHERE username='n0'", []).unwrap();
        let t: i64 = db.query_row("SELECT updated_at FROM users WHERE username='n0'", [], |r| r.get(0)).unwrap();
        assert!(t > last, "an update of an old row comes after the rest too");
    }

    /// A8: a peer's row "changed in the future" is refused (it would stay
    /// newer than every real change here).
    #[test]
    fn future_updated_at_is_refused() {
        let d = dir();
        let now = crate::now_ms();
        let r = d.fed_apply_user(&peer_user("tomorrow", "3000000001", 1, now + 24 * 3600 * 1000));
        assert!(!r.applied && !r.failed);
        assert!(d.identity_for("tomorrow").is_none());
        assert!(d.fed_apply_user(&peer_user("today", "3000000002", 1, now + 1000)).applied, "a little clock skew is fine");
    }

    /// A8: relays don't hand bots to each other as accounts.
    #[test]
    fn bot_rows_from_peers_are_refused() {
        let d = dir();
        let mut u = peer_user("fakebot", "3000000003", 1, crate::now_ms());
        u.is_bot = true;
        let r = d.fed_apply_user(&u);
        assert!(!r.applied && !r.failed);
        assert!(d.identity_for("fakebot").is_none());
    }

    /// A8: a peer's row can't take over the e-mail of another account here
    /// (sign-in by code and password reset go by e-mail); a rename of the
    /// same account still moves it.
    #[test]
    fn a_peer_row_cannot_take_another_accounts_email() {
        let d = dir();
        let mut mine = peer_user("owner", "3000000004", 1, crate::now_ms() - 10_000);
        mine.email = Some("owner@x.com".into());
        assert!(d.fed_apply_user(&mine).applied);
        let mut thief = peer_user("thief", "3000000005", 1, crate::now_ms());
        thief.email = Some("owner@x.com".into());
        assert!(d.fed_apply_user(&thief).applied);
        assert_eq!(d.email_of("owner").as_deref(), Some("owner@x.com"), "stays with its account");
        assert_eq!(d.email_of("thief"), None);
        // The same account renamed on its relay: the e-mail goes with it.
        let mut renamed = mine.clone();
        renamed.username = "owner2".into();
        renamed.updated_at = crate::now_ms();
        assert!(d.fed_apply_user(&renamed).applied);
        assert_eq!(d.email_of("owner2").as_deref(), Some("owner@x.com"));
    }

    /// A8: a made-up registry claim with an old creation time doesn't rename
    /// an account taken here long ago (only a near-simultaneous one).
    #[test]
    fn a_forged_old_name_claim_does_not_rename_ours() {
        let d = dir();
        let now = now_secs() as i64;
        let mine = peer_user("longtime", "3000000006", now - 3600, crate::now_ms() - 1000);
        assert!(d.fed_apply_user(&mine).applied);
        let claim = NameRow { username: "longtime".into(), uid: "3000000007".into(), home: "http://evil".into(), created_at: 0, updated_at: 1 };
        assert_eq!(d.fed_apply_name(&claim, "http://evil"), None);
        assert!(d.identity_for("longtime").is_some(), "ours keeps it");
        // Taken on two relays at about the same time: the older one wins, as before.
        let fresh = peer_user("racename", "3000000008", now - 5, crate::now_ms() - 1000);
        assert!(d.fed_apply_user(&fresh).applied);
        let older = NameRow { username: "racename".into(), uid: "3000000009".into(), home: "http://b".into(), created_at: now - 30, updated_at: 1 };
        assert!(d.fed_apply_name(&older, "http://b").is_some(), "near-simultaneous: the older wins");
        // A claim far older than a fresh account of ours isn't a race.
        let fresh2 = peer_user("fresh2", "3000000010", now - 5, crate::now_ms() - 1000);
        assert!(d.fed_apply_user(&fresh2).applied);
        let forged = NameRow { username: "fresh2".into(), uid: "3000000011".into(), home: "http://evil".into(), created_at: 0, updated_at: 1 };
        assert_eq!(d.fed_apply_name(&forged, "http://evil"), None);
        assert!(!d.names_after(0, None, 100).iter().any(|r| r.uid == "3000000011"), "and not recorded to be handed on");
    }

    /// A6: where sync is with a peer survives a restart (same file), moves
    /// with the peer's address and is dropped on request.
    #[test]
    fn cursors_persist_and_move_with_url() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("d.sqlite").to_string_lossy().to_string();
        {
            let d = Directory::open_path(&path).unwrap();
            assert!(d.fed_cursor("http://a", "users").is_none());
            assert!(d.set_fed_cursor("http://a", "users", 5, "bob"));
            assert!(d.set_fed_cursor("http://a", "hist", 7, "bob/k"));
            assert!(d.set_fed_cursor("http://a", "users", 9, "carol"));
        }
        let d = Directory::open_path(&path).unwrap();
        assert_eq!(d.fed_cursor("http://a", "users"), Some((9, "carol".into())), "after a restart");
        d.fed_cursors_move("http://a", "http://a2");
        assert!(d.fed_cursor("http://a", "users").is_none());
        assert_eq!(d.fed_cursor("http://a2", "hist"), Some((7, "bob/k".into())));
        d.fed_cursors_drop(Some("http://a2"), Some("hist"));
        assert!(d.fed_cursor("http://a2", "hist").is_none());
        assert!(d.fed_cursor("http://a2", "users").is_some());
        d.fed_cursors_drop(None, None);
        assert!(d.fed_cursor("http://a2", "users").is_none());
    }

    /// AQUA-SEC-011: a flooded inbox keeps the newest INBOX_MAX_ROWS.
    #[test]
    fn inbox_keeps_the_newest_when_full() {
        let d = dir();
        let total = Directory::INBOX_MAX_ROWS + 600;
        let mut last = 0;
        for i in 0..total {
            last = d.inbox_push("flood", format!("m{i}").as_bytes()).unwrap();
        }
        let n = d.inbox_len("flood");
        assert!(n <= Directory::INBOX_MAX_ROWS + 256, "{n}");
        assert!(n >= Directory::INBOX_MAX_ROWS - 256, "{n}");
        let rows = d.inbox_take("flood");
        assert_eq!(rows.last().map(|r| r.0), Some(last), "the newest stays");
        assert_eq!(rows.last().map(|r| r.1.clone()), Some(format!("m{}", total - 1).into_bytes()));
        assert_eq!(d.inbox_len("other"), 0);
    }

    /// AQUA-SEC-005: with dev codes off (the default since 1.21.0-21) a code
    /// is stored hashed, works once, dies after 5 wrong tries, and codes per
    /// address are limited.
    #[test]
    fn email_codes_are_hashed_single_use_and_limited() {
        let mut d = dir();
        d.dev_forced = Some(false);
        let (email, code) = d.issue_code("Otp@X.com").unwrap();
        assert_eq!(email, "otp@x.com");
        let stored: String = d.db.lock().unwrap().query_row("SELECT code FROM email_codes WHERE email=?1", [&email], |r| r.get(0)).unwrap();
        assert_ne!(stored, code, "the code itself is never stored");
        assert_eq!(stored.len(), 64);
        // Resend too soon.
        assert!(matches!(d.issue_code(&email), Err(DirError::TooManyCodes)));
        // Right code once, then it's used up.
        assert!(d.verify_code(&email, &code).is_ok());
        assert!(matches!(d.verify_code(&email, &code), Err(DirError::BadCode)));

        // A new code (after the resend pause) and 5 wrong tries void it,
        // even for the right code.
        d.db.lock().unwrap().execute("UPDATE email_codes SET issued_at = issued_at - 60 WHERE email=?1", [&email]).unwrap();
        let (_, code2) = d.issue_code(&email).unwrap();
        let wrong = if code2 == "000000" { "000001" } else { "000000" };
        for _ in 0..CODE_MAX_FAILS {
            assert!(matches!(d.verify_code(&email, wrong), Err(DirError::BadCode)));
        }
        assert!(matches!(d.verify_code(&email, &code2), Err(DirError::TooManyTries)));

        // At most CODE_MAX_PER_HOUR codes an hour.
        for _ in 2..CODE_MAX_PER_HOUR {
            d.db.lock().unwrap().execute("UPDATE email_codes SET issued_at = issued_at - 60 WHERE email=?1", [&email]).unwrap();
            d.issue_code(&email).unwrap();
        }
        d.db.lock().unwrap().execute("UPDATE email_codes SET issued_at = issued_at - 60 WHERE email=?1", [&email]).unwrap();
        assert!(matches!(d.issue_code(&email), Err(DirError::TooManyCodes)));
        assert!(!d.dev_codes());
    }

    /// "Полина" is found by "Полина" and "полина", not only by "олина".
    #[test]
    fn search_folds_cyrillic_case() {
        let d = dir();
        d.register_plain("polly", &[7u8; 32], "pass-word-1", "Полина").unwrap();
        for q in ["Полина", "полина", "ПОЛ", "олина"] {
            assert_eq!(d.search(q, 10).len(), 1, "{q}");
        }
    }

    /// The mail server drops the connection (as Gmail does through some VPNs):
    /// the request still succeeds, says the mail failed, and the operator
    /// gets the code — the requester never does.
    #[tokio::test]
    async fn failed_mail_puts_the_code_in_the_admin_panel() {
        let mut d = dir();
        d.dev_forced = None; // a real relay with SMTP set up
        // A "mail server" that accepts and closes at once.
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move { loop { if let Ok((s, _)) = l.accept().await { drop(s); } } });
        d.smtp.write().unwrap().replace(SmtpConfig { host: "127.0.0.1".into(), user: "me@x.com".into(),
            pass: "p".into(), from: "me@x.com".into(), port });
        let r = d.request_code_delivered("Who@X.com").await.unwrap();
        assert!(r.delivery_failed);
        assert!(r.dev_code.is_none(), "the requester must not get the code");
        let recent = d.recent_codes();
        assert_eq!(recent[0].0, "who@x.com");
        assert!(d.verify_code("who@x.com", &recent[0].1).is_ok());
    }

    #[test]
    fn code_email_builds() {
        let cfg = SmtpConfig { host: "smtp.example.com".into(), user: "me@example.com".into(),
            pass: "x".into(), from: "me@example.com".into(), port: 587 };
        let m = build_code_email(&cfg, "you@example.com", "042517", Some("http://relay.example:7777/v1/auth/email/link/ab12")).unwrap();
        let raw = String::from_utf8(m.formatted()).unwrap();
        assert!(raw.contains("From: Aquamarine <me@example.com>"));
        assert!(raw.contains("multipart/alternative"));
        assert!(raw.contains("042517"));
        if let Ok(p) = std::env::var("AQUA_DUMP_EMAIL") { std::fs::write(p, raw).unwrap(); }
    }

    #[test]
    fn rename_moves_the_account_and_keeps_the_old_name() {
        let d = dir();
        let (a, b) = ([7u8; 32], [8u8; 32]);
        let code = d.request_code("ren@x.com").unwrap().unwrap();
        let t = d.verify_code("ren@x.com", &code).unwrap();
        d.register("oldname", &a, &t, "Ren", "", "").unwrap();
        d.inbox_push("oldname", b"m1").unwrap();
        d.inbox_push("oldname~dev2", b"m2").unwrap();
        // Only the owner, only a valid free name.
        assert!(matches!(d.rename_user("oldname", &b, "newname"), Err(DirError::NotOwner)));
        assert!(matches!(d.rename_user("oldname", &a, "x"), Err(DirError::BadUsername)));
        assert_eq!(d.rename_user("oldname", &a, "@NewName").unwrap(), "newname");
        assert!(d.user_exists("newname") && !d.user_exists("oldname"));
        assert_eq!(d.username_for_identity(&a).as_deref(), Some("newname"));
        assert_eq!(d.renamed_to("oldname").as_deref(), Some("newname"));
        assert_eq!(d.inbox_take("newname").len(), 1);
        assert_eq!(d.inbox_take("newname~dev2").len(), 1);
        // The old name is reserved: nobody else registers it.
        let code = d.request_code("other@x.com").unwrap().unwrap();
        let t = d.verify_code("other@x.com", &code).unwrap();
        assert!(matches!(d.register("oldname", &b, &t, "", "", ""), Err(DirError::UsernameTaken)));
        // Search: with or without "@", and by the old name.
        assert_eq!(d.search("@newn", 5).first().map(|p| p.username.as_str()), Some("newname"));
        assert_eq!(d.search("@oldname", 5).first().map(|p| p.username.as_str()), Some("newname"));
        assert!(d.search("new_name", 5).is_empty(), "_ is a literal, not a wildcard");
        // Once a day.
        assert!(matches!(d.rename_user("newname", &a, "third"), Err(DirError::RenameTooSoon)));
    }

    fn reg(d: &Directory, name: &str, id: &[u8; 32], email: &str, created: i64) {
        let code = d.request_code(email).unwrap().unwrap();
        let t = d.verify_code(email, &code).unwrap();
        d.register(name, id, &t, "", "", "").unwrap();
        d.db.lock().unwrap().execute("UPDATE users SET created_at=?1 WHERE username=?2", rusqlite::params![created, name]).unwrap();
    }

    fn sync(from: &Directory, to: &Directory) -> Vec<FedApplied> {
        from.fed_users_since(0).iter().map(|u| to.fed_apply_user(u)).collect()
    }

    /// Two relays ran alone, each got its own @same, then they joined: both
    /// people keep their accounts (matched by ID); the older one keeps the
    /// name, the newer becomes same_NNNN — the same on both relays.
    #[test]
    fn joining_relays_keep_both_accounts_with_one_name() {
        let (x, y) = (dir(), dir());
        let (old, new) = ([3u8; 32], [4u8; 32]);
        reg(&x, "same", &new, "new@x.com", 2000);
        reg(&y, "same", &old, "old@y.com", 1000);
        let (uo, un) = (uid_for(&old), uid_for(&new));
        assert_eq!(uo.len(), 10);
        assert_eq!(x.uid_of("same"), Some(un.clone()));
        // Y learns X's newer account: it lands under the displaced name.
        let r = sync(&x, &y);
        let moved = format!("same_{}", &un[6..]);
        assert_eq!(r[0].name, moved);
        assert!(r[0].displaced.is_none());
        assert_eq!(y.username_by_uid(&un), Some(moved.clone()));
        assert_eq!(y.username_by_uid(&uo).as_deref(), Some("same"));
        // X learns Y's older account: its own one moves, the name goes to it.
        let r = sync(&y, &x);
        assert!(r.iter().any(|a| a.displaced == Some(("same".into(), moved.clone()))));
        assert_eq!(x.username_by_uid(&un), Some(moved.clone()));
        assert_eq!(x.username_by_uid(&uo).as_deref(), Some("same"));
        assert_eq!(x.username_for_identity(&new), Some(moved.clone()));
        assert!(x.renamed_to("same").is_none(), "the name isn't forwarded to the moved account");
        // Search by ID.
        assert_eq!(x.search(&un, 5).first().map(|p| p.username.clone()), Some(moved.clone()));
        // The moved account picks a new name; peers follow it by ID.
        x.db.lock().unwrap().execute("DELETE FROM renames", []).unwrap();
        assert_eq!(x.rename_user(&moved, &new, "fresh").unwrap(), "fresh");
        sync(&x, &y);
        assert_eq!(y.username_by_uid(&un).as_deref(), Some("fresh"));
        assert!(!y.user_exists(&moved));
        assert_eq!(y.username_by_uid(&uo).as_deref(), Some("same"));
    }

    #[test]
    fn history_records_are_listed_by_seq_and_the_key_id_is_written_once() {
        let d = dir();
        assert_eq!(d.hist_put("vlad", "kid", b"K1", true, HIST_LIMIT).unwrap(), 1);
        assert!(matches!(d.hist_put("vlad", "kid", b"K2", true, HIST_LIMIT), Err(DirError::Conflict)));
        assert_eq!(d.hist_put("vlad", "kid", b"K1", true, HIST_LIMIT).unwrap(), 1, "the same value is fine");
        assert_eq!(d.hist_put("vlad", "a1", b"page", false, HIST_LIMIT).unwrap(), 2);
        assert_eq!(d.hist_put("vlad", "b2", b"meta", false, HIST_LIMIT).unwrap(), 3);
        assert_eq!(d.hist_put("vlad", "a1", b"page v2", false, HIST_LIMIT).unwrap(), 4);
        let all: Vec<String> = d.hist_list("vlad", 0, 100).into_iter().map(|r| r.0).collect();
        assert_eq!(all, ["kid", "b2", "a1"]);
        assert_eq!(d.hist_list("vlad", 3, 100).len(), 1);
        assert_eq!(d.hist_get("vlad", "a1").as_deref(), Some(&b"page v2"[..]));
        assert!(d.hist_list("ann", 0, 100).is_empty(), "per account");
        // Federation: a newer copy comes in with a fresh local seq.
        assert!(d.fed_apply_hist("vlad", "b2", b"meta v2", crate::now_ms() + 1000).unwrap());
        assert!(!d.fed_apply_hist("vlad", "b2", b"old", 1).unwrap());
        assert_eq!(d.hist_list("vlad", 4, 100)[0].0, "b2");
    }

    #[tokio::test]
    async fn its_me_link_gives_the_code_to_the_asking_app_only() {
        let d = dir();
        let nonce = "n0nce-of-the-app-0123456789";
        d.request_code_delivered_ex("Link@X.com", Some(nonce), Some("http://relay:7777")).await.unwrap();
        let token = d.links.lock().unwrap().keys().next().cloned().unwrap();
        assert_eq!(d.link_email(&token).as_deref(), Some("li•••@x.com"));
        assert_eq!(d.poll_link("link@x.com", nonce), None); // not tapped yet
        assert!(d.confirm_link(&token));
        let code = d.poll_link("link@x.com", nonce).expect("code after the tap");
        assert_eq!(d.poll_link("link@x.com", "another-app-nonce-0000000"), None);
        assert!(d.verify_code("link@x.com", &code).is_ok());
        assert!(!d.confirm_link("nope"));
    }

    #[test]
    fn full_registration_flow() {
        let d = dir();
        let id = [1u8; 32];
        // Request + verify an email code.
        let code = d.request_code("vlad@example.com").unwrap().expect("dev code");
        let ticket = d.verify_code("vlad@example.com", &code).unwrap();
        // Register a username.
        d.register("vpst", &id, &ticket, "Vlad P", "hi there", "+7 999").unwrap();
        // Username is now bound to this identity.
        assert_eq!(d.identity_for("vpst"), Some(id));
        // Profile is retrievable.
        let p = d.get_profile("vpst").unwrap();
        assert_eq!(p.display_name, "Vlad P");
        assert_eq!(p.phone, "+7 999");
        // Search finds it by username and display name.
        assert_eq!(d.search("vps", 10).len(), 1);
        assert_eq!(d.search("vlad", 10).len(), 1);
        assert_eq!(d.search("nobody", 10).len(), 0);
    }

    #[test]
    fn two_step_codes_and_no_new_devices() {
        use crate::totp;
        let d = dir();
        let id = [7u8; 32];
        let code = d.request_code("s@x.com").unwrap().unwrap();
        let t = d.verify_code("s@x.com", &code).unwrap();
        d.register("sec", &id, &t, "Sec", "", "").unwrap();
        d.set_password("sec", &id, "correct horse").unwrap();
        let ticket = |d: &Directory| {
            let c = d.request_code("s@x.com").unwrap().unwrap();
            d.verify_code("s@x.com", &c).unwrap()
        };
        let now_code = |secret: &str| format!("{:06}", totp::code_at(&totp::unbase32(secret).unwrap(), totp::now_step()));

        // Two-step codes on: the first code from the app confirms the secret.
        let secret = d.totp_setup("sec").unwrap();
        assert!(matches!(d.totp_enable("sec", "000000x"), Err(DirError::BadTotp)));
        let backup = d.totp_enable("sec", &now_code(&secret)).unwrap();
        assert_eq!(backup.len(), totp::BACKUP_CODES);
        assert!(d.security_state("sec").totp);
        // That code is used up: signing in needs a new one — a backup code here.
        let t2 = ticket(&d);
        let phone = [8u8; 32];
        assert!(matches!(d.login_checked(&t2, "correct horse", &phone, "Phone", "", ""), Err(DirError::NeedTotp)));
        assert!(matches!(d.login_checked(&t2, "correct horse", &phone, "Phone", "123456", ""), Err(DirError::BadTotp)));
        // The wrong password is refused before the code is even looked at.
        assert!(matches!(d.login_checked(&t2, "wrong pass!", &phone, "Phone", &backup[0], ""), Err(DirError::BadLogin)));
        // The backup fetch passes the code once; the sign-in after it doesn't ask again.
        d.sign_in_gate(&t2, "sec", None, &backup[0], "").unwrap();
        assert_eq!(d.login_checked(&t2, "correct horse", &phone, "Phone", "", "").unwrap(), "sec");
        assert_eq!(d.security_state("sec").backup_left, totp::BACKUP_CODES - 1);
        // A backup code works once.
        let t3 = ticket(&d);
        assert!(matches!(d.login_checked(&t3, "correct horse", &[9u8; 32], "", &backup[0], ""), Err(DirError::BadTotp)));
        // Five wrong codes drop the ticket.
        for _ in 0..4 {
            let _ = d.login_checked(&t3, "correct horse", &[9u8; 32], "", "111111", "");
        }
        assert!(d.login_checked(&t3, "correct horse", &[9u8; 32], "", &backup[1], "").is_err());

        // No new devices: even the right code doesn't help; devices already
        // on the account still sign in.
        d.set_device_lock("sec", true, "").unwrap();
        let t4 = ticket(&d);
        assert!(matches!(d.login_checked(&t4, "correct horse", &[10u8; 32], "", &backup[1], ""), Err(DirError::DevicesLocked)));
        assert!(matches!(d.sign_in_gate(&t4, "sec", None, &backup[1], ""), Err(DirError::DevicesLocked)));
        assert_eq!(d.login_checked(&t4, "correct horse", &phone, "Phone", "", "").unwrap(), "sec");
        // Turning it back on asks for a code.
        assert!(matches!(d.set_device_lock("sec", false, ""), Err(DirError::NeedTotp)));
        // The recovery key opens both.
        let check = "c".repeat(64);
        d.set_recovery("sec", &id, &check).unwrap();
        let t5 = ticket(&d);
        assert!(matches!(d.login_checked(&t5, "correct horse", &[11u8; 32], "", "", &"x".repeat(64)), Err(DirError::BadRecovery)));
        assert_eq!(d.login_checked(&t5, "correct horse", &[11u8; 32], "Laptop", "", &check).unwrap(), "sec");

        // Replicated with the account.
        let row = d.fed_users_since(0).into_iter().find(|u| u.username == "sec").unwrap();
        let sec: Security = serde_json::from_str(&row.security).unwrap();
        assert!(sec.lock && !sec.totp.is_empty());
        let other = dir();
        assert!(other.fed_apply_user(&row).applied);
        assert!(other.security_state("sec").lock);

        // Off again with a backup code.
        d.totp_disable("sec", &backup[2]).unwrap();
        d.set_device_lock("sec", false, "").unwrap();
        let st = d.security_state("sec");
        assert!(!st.lock && !st.totp && st.backup_left == 0);
    }

    #[test]
    fn profile_media_and_extras() {
        let d = dir();
        let id = [7u8; 32];
        let code = d.request_code("m@x.com").unwrap().unwrap();
        let t = d.verify_code("m@x.com", &code).unwrap();
        d.register("media", &id, &t, "Media", "", "").unwrap();
        let p = d.get_profile("media").unwrap();
        assert_eq!((p.avatar_ver, p.banner_ver), (0, 0));
        assert_eq!(p.extra, serde_json::json!({}));

        // Extras round-trip; non-objects and strangers are refused.
        let extra = serde_json::json!({ "status": "coding", "banner_a": "#112233" });
        d.update_profile_full("media", &id, "Media", "bio", "", Some(&extra)).unwrap();
        assert_eq!(d.get_profile("media").unwrap().extra["status"], "coding");
        assert!(d.update_profile_full("media", &id, "M", "", "", Some(&serde_json::json!([1]))).is_err());
        assert!(matches!(d.update_profile_full("media", &[9u8; 32], "x", "", "", None), Err(DirError::NotOwner)));
        // Another device of the account (a phone next to the PC) may edit too.
        d.add_device("media", &[5u8; 32], "Phone").unwrap();
        d.update_profile_full("media", &[5u8; 32], "Media", "bio", "", Some(&extra)).unwrap();
        assert!(d.set_media("media", &[5u8; 32], MediaKind::Avatar, "image/png", b"png").is_ok());
        // Plain update keeps extras.
        d.update_profile("media", &id, "Media2", "bio", "").unwrap();
        assert_eq!(d.get_profile("media").unwrap().extra["status"], "coding");

        // Avatar upload bumps the version, is readable by anyone, clears.
        let v1 = d.set_media("media", &id, MediaKind::Avatar, "image/png", b"png-bytes").unwrap();
        assert!(v1 > 0);
        assert_eq!(d.get_profile("media").unwrap().avatar_ver, v1);
        assert_eq!(d.get_media("media", MediaKind::Avatar).unwrap(), ("image/png".to_string(), b"png-bytes".to_vec()));
        let v2 = d.set_media("media", &id, MediaKind::Avatar, "image/gif", b"gif").unwrap();
        assert!(v2 > v1);
        assert!(d.set_media("media", &[9u8; 32], MediaKind::Banner, "image/png", b"x").is_err());
        assert!(d.set_media("media", &id, MediaKind::Banner, "text/html", b"x").is_err());
        d.set_media("media", &id, MediaKind::Banner, "image/jpeg", b"jpg").unwrap();
        // More avatars and banners: stored and served like the main ones.
        d.set_media("media", &id, MediaKind::Avatar3, "image/webp", b"a3").unwrap();
        assert_eq!(d.get_media("media", MediaKind::Avatar3).unwrap().1, b"a3".to_vec());
        assert!(d.set_media("media", &id, MediaKind::Avatar2, "video/mp4", b"v").is_err());
        assert!(d.set_media("media", &id, MediaKind::Banner2, "video/mp4", b"v").is_ok());
        assert_eq!(MediaKind::parse("banner4"), None);
        d.clear_media("media", &id, MediaKind::Avatar).unwrap();
        let p = d.get_profile("media").unwrap();
        assert_eq!(p.avatar_ver, 0);
        assert!(p.banner_ver > 0);
        assert!(d.get_media("media", MediaKind::Avatar).is_none());

        // Password + login on a new device.
        assert!(d.set_password("media", &id, "short").is_err());
        d.set_password("media", &id, "correct horse").unwrap();
        let code = d.request_code("m@x.com").unwrap().unwrap();
        let t2 = d.verify_code("m@x.com", &code).unwrap();
        let new_dev = [8u8; 32];
        assert!(matches!(d.login(&t2, "wrong pass!", &new_dev), Err(DirError::BadLogin)));
        assert_eq!(d.login(&t2, "correct horse", &new_dev).unwrap(), "media");
        // The new device is added; the first one stays the primary device.
        assert_eq!(d.identity_for("media"), Some(id));
        assert_eq!(d.device_role("media", &new_dev), Some(false));
        assert_eq!(d.device_role("media", &id), Some(true));
        assert_eq!(d.username_for_identity(&new_dev).as_deref(), Some("media"));
        assert!(d.login(&t2, "correct horse", &new_dev).is_err()); // ticket is single-use
        // Sign the extra device out again.
        assert!(d.remove_device("media", &device_id(&new_dev), None).unwrap());
        assert_eq!(d.device_role("media", &new_dev), None);

        // Profile-photo privacy.
        assert!(d.may_see_media("media", None));
        assert_eq!(d.username_for_identity(&id).as_deref(), Some("media"));
        let acl = MediaAcl { everybody: false, allow: vec!["Friend".into()], deny: vec![] };
        assert!(d.set_media_acl("media", &[9u8; 32], &acl).is_err());
        d.set_media_acl("media", &id, &acl).unwrap();
        assert!(d.may_see_media("media", Some("friend")));
        assert!(d.may_see_media("media", Some("media")));
        assert!(!d.may_see_media("media", Some("stranger")));
        assert!(!d.may_see_media("media", None));
        let acl = MediaAcl { everybody: true, allow: vec![], deny: vec!["ex".into()] };
        d.set_media_acl("media", &id, &acl).unwrap();
        assert!(d.may_see_media("media", Some("stranger")));
        assert!(!d.may_see_media("media", Some("ex")));
        d.set_media_acl("media", &id, &MediaAcl { everybody: true, ..Default::default() }).unwrap();
        assert!(d.may_see_media("media", None));
    }

    #[test]
    fn group_names_share_the_namespace() {
        let d = dir();
        d.register_plain("papa", &[9u8; 32], "pass-word-1", "").unwrap();
        // A person's name can't be a channel's, and back.
        assert!(matches!(d.claim_group_name("papa", "grp_1", "Papa", "channel", "papa"), Err(DirError::UsernameTaken)));
        assert_eq!(d.claim_group_name("@CeZeOs", "grp_1", "CeZeOs", "channel", "papa").unwrap(), "cezeos");
        assert!(matches!(d.register_plain("cezeos", &[8u8; 32], "pass-word-2", ""), Err(DirError::UsernameTaken)));
        // Another group can't take it; its owner renames it, the old name goes.
        assert!(matches!(d.claim_group_name("cezeos", "grp_2", "X", "group", "mama"), Err(DirError::UsernameTaken)));
        assert!(matches!(d.claim_group_name("other", "grp_1", "CeZeOs", "channel", "mama"), Err(DirError::NotOwner)));
        d.claim_group_name("cezeos_ch", "grp_1", "CeZeOs", "channel", "papa").unwrap();
        assert!(d.group_by_name("cezeos").is_none());
        assert_eq!(d.group_by_name("@CEZEOS_CH").unwrap()["gid"], "grp_1");
        assert_eq!(d.search_groups("cez", 5).len(), 1);
        assert!(d.release_group_name("grp_1", "papa"));
        assert!(d.group_by_name("cezeos_ch").is_none());
    }

    #[test]
    fn one_account_per_key_names_it() {
        let d = dir();
        d.register_plain("papa", &[9u8; 32], "pass-word-1", "").unwrap();
        // Another name with the same key: which account the key has.
        assert!(matches!(d.register_plain("papa2", &[9u8; 32], "pass-word-1", ""), Err(DirError::KeyHasAccount(n)) if n == "papa"));
        // Signing in to another account with these keys: whose they are.
        d.register_plain("mama", &[8u8; 32], "pass-word-2", "").unwrap();
        assert!(matches!(d.sign_in_plain("mama", "pass-word-2", &[9u8; 32], "", "", ""), Err(DirError::KeyHasAccount(n)) if n == "papa"));
        // A fresh key: the account lives on another device.
        assert!(matches!(d.sign_in_plain("mama", "pass-word-2", &[7u8; 32], "", "", ""), Err(DirError::DeviceBound)));
    }

    #[test]
    fn username_taken_by_other_identity() {
        let d = dir();
        let code = d.request_code("a@x.com").unwrap().unwrap();
        let t1 = d.verify_code("a@x.com", &code).unwrap();
        d.register("shared", &[1u8; 32], &t1, "A", "", "").unwrap();

        let code2 = d.request_code("b@x.com").unwrap().unwrap();
        let t2 = d.verify_code("b@x.com", &code2).unwrap();
        let err = d.register("shared", &[2u8; 32], &t2, "B", "", "");
        assert!(matches!(err, Err(DirError::UsernameTaken)));
    }

    #[test]
    fn wrong_code_rejected() {
        let d = dir();
        d.request_code("c@x.com").unwrap();
        assert!(matches!(d.verify_code("c@x.com", "000000"), Err(DirError::BadCode)));
    }

    #[test]
    fn ticket_required_and_single_use() {
        let d = dir();
        // Registering with a bogus ticket fails.
        assert!(matches!(
            d.register("ghost", &[9u8; 32], "deadbeef", "", "", ""),
            Err(DirError::BadTicket)
        ));
        // A real ticket works once, then is consumed.
        let code = d.request_code("d@x.com").unwrap().unwrap();
        let t = d.verify_code("d@x.com", &code).unwrap();
        d.register("realuser", &[9u8; 32], &t, "", "", "").unwrap();
        // Same ticket again → rejected.
        assert!(matches!(
            d.register("realuser2", &[9u8; 32], &t, "", "", ""),
            Err(DirError::BadTicket)
        ));
    }

    #[test]
    fn bot_registration_and_token() {
        let d = dir();
        let owner = [5u8; 32];
        let token = d.register_bot("echobot", &owner, "Echo Bot").unwrap();
        assert!(token.starts_with("aqbot_"));
        // Token resolves back to the bot username.
        assert_eq!(d.bot_by_token(&token).as_deref(), Some("echobot"));
        // The bot is flagged + owned.
        assert!(d.is_bot("echobot"));
        assert_eq!(d.identity_for("echobot"), Some(owner));
        // Appears in the owner's bot list.
        let bots = d.list_bots_for_owner(&owner);
        assert_eq!(bots.len(), 1);
        assert_eq!(bots[0].0, "echobot");
        // Shows up in search with is_bot set.
        let found = d.search("echo", 10);
        assert_eq!(found.len(), 1);
        assert!(found[0].is_bot);
        // Another identity can't steal the bot username.
        assert!(matches!(
            d.register_bot("echobot", &[6u8; 32], "x"),
            Err(DirError::UsernameTaken)
        ));
    }

    #[test]
    fn bot_at_quota_can_rotate_its_token() {
        let d = dir();
        let owner = [5u8; 32];
        let original = d.register_bot_with_limit("echobot", &owner, "Echo", 1).unwrap();
        assert!(matches!(
            d.register_bot_with_limit("secondbot", &owner, "Second", 1),
            Err(DirError::TooManyBots)
        ));
        assert!(matches!(
            d.register_bot_with_limit("echobot", &[6u8; 32], "Other", 1),
            Err(DirError::UsernameTaken)
        ));
        let replacement = d.register_bot_with_limit("echobot", &owner, "Echo", 0).unwrap();
        assert_ne!(original, replacement);
        assert!(d.bot_by_token(&original).is_none());
        assert_eq!(d.bot_by_token(&replacement).as_deref(), Some("echobot"));
        assert_eq!(d.list_bots_for_owner(&owner).len(), 1);
    }

    #[test]
    fn bad_username_rejected() {
        let d = dir();
        let code = d.request_code("e@x.com").unwrap().unwrap();
        let t = d.verify_code("e@x.com", &code).unwrap();
        assert!(matches!(
            d.register("ab", &[1u8; 32], &t, "", "", ""),
            Err(DirError::BadUsername)
        ));
    }

    #[test]
    fn inbox_backup_and_check_login() {
        let d = Directory::open_memory();
        let id = [9u8; 32];
        let code = d.request_code("bk@x.com").unwrap().unwrap();
        let t = d.verify_code("bk@x.com", &code).unwrap();
        d.register("bkuser", &id, &t, "B", "", "").unwrap();
        d.set_password("bkuser", &id, "aqv2$0123456789abcdef").unwrap();
        // check_login: no side effects, ticket stays valid
        let code = d.request_code("bk@x.com").unwrap().unwrap();
        let t = d.verify_code("bk@x.com", &code).unwrap();
        assert!(matches!(d.check_login(&t, "wrong"), Err(DirError::BadLogin)));
        assert_eq!(d.check_login(&t, "aqv2$0123456789abcdef").unwrap().0, "bkuser");
        assert_eq!(d.identity_for("bkuser"), Some(id));
        assert_eq!(d.login(&t, "aqv2$0123456789abcdef", &[7u8; 32]).unwrap(), "bkuser");
        assert!(d.check_login(&t, "aqv2$0123456789abcdef").is_err(), "ticket is one-time after login");
        // backups
        assert!(d.backup_meta("bkuser").is_none());
        d.put_backup("bkuser", b"AQBK1....").unwrap();
        assert_eq!(d.backup_meta("bkuser").unwrap().0, 9);
        assert_eq!(d.get_backup("bkuser").unwrap(), b"AQBK1....");
        d.delete_backup("bkuser").unwrap();
        assert!(d.get_backup("bkuser").is_none());
        // inbox
        d.inbox_push("bkuser", b"one").unwrap();
        d.inbox_push("bkuser", b"two").unwrap();
        d.inbox_push("other", b"x").unwrap();
        let got = d.inbox_take("bkuser");
        assert_eq!(got.iter().map(|(_, m)| m.as_slice()).collect::<Vec<_>>(), vec![&b"one"[..], &b"two"[..]]);
        d.inbox_ack(got[0].0);
        assert_eq!(d.inbox_take("bkuser").len(), 1);
    }
}
