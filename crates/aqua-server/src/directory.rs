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
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use rand_core::{OsRng, RngCore};
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

pub struct Directory {
    db: Mutex<Connection>,
    /// AQUA_DEV_CODES: Some(true) forces dev mode even with SMTP, Some(false)
    /// forbids it. None = dev mode exactly when no SMTP is configured.
    dev_forced: Option<bool>,
    /// When Some, verification codes are sent via real SMTP.
    smtp: std::sync::RwLock<Option<SmtpConfig>>,
    /// "env" | "file" | "none" — where the SMTP settings came from.
    smtp_source: std::sync::RwLock<&'static str>,
    /// Last few dev-mode codes (email, code, unix secs) for the admin panel.
    recent: Mutex<std::collections::VecDeque<(String, String, u64)>>,
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
}

impl From<rusqlite::Error> for DirError {
    fn from(e: rusqlite::Error) -> Self { DirError::Db(e.to_string()) }
}

/// A public profile card, as returned by the lookup + search endpoints.
#[derive(Debug, Clone, serde::Serialize)]
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
}

/// Columns selected for a [`Profile`], in the order `profile_from_row` reads.
const PROFILE_COLS: &str =
    "username, display_name, about, phone, is_bot, created_at, extra, avatar_ver, banner_ver";

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
    })
}

// ---- passwords: PBKDF2-HMAC-SHA256, 120k rounds, random 16-byte salt ----

const PBKDF2_ROUNDS: u32 = 120_000;

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
pub const PROFILE_MEDIA_LIMIT: usize = 8 * 1024 * 1024;
/// Largest `extra` JSON blob accepted.
pub const PROFILE_EXTRA_LIMIT: usize = 8 * 1024;

/// Which profile image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind { Avatar, Banner }

impl MediaKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "avatar" => Some(Self::Avatar),
            "banner" => Some(Self::Banner),
            _ => None,
        }
    }
    fn as_str(self) -> &'static str {
        match self { Self::Avatar => "avatar", Self::Banner => "banner" }
    }
    fn ver_col(self) -> &'static str {
        match self { Self::Avatar => "avatar_ver", Self::Banner => "banner_ver" }
    }
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl Directory {
    /// In-memory fallback used when the on-disk DB can't be opened.
    /// Non-persistent, but keeps the server functional.
    pub fn open_memory() -> Self {
        let db = Connection::open_in_memory().expect("in-memory sqlite");
        Self::init_schema(&db).expect("init schema");
        Self {
            db: Mutex::new(db),
            dev_forced: Some(true),
            smtp: std::sync::RwLock::new(None),
            smtp_source: std::sync::RwLock::new("none"),
            recent: Mutex::new(Default::default()),
        }
    }

    fn init_schema(db: &Connection) -> rusqlite::Result<()> {
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
            CREATE TABLE IF NOT EXISTS email_codes (
                email      TEXT PRIMARY KEY,
                code       TEXT NOT NULL,
                ticket     TEXT,
                issued_at  INTEGER NOT NULL,
                verified_at INTEGER
            );
            CREATE TABLE IF NOT EXISTS bot_tokens (
                token      TEXT PRIMARY KEY,
                username   TEXT NOT NULL,
                owner_dh   BLOB NOT NULL,
                created_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_bot_owner ON bot_tokens(owner_dh);
            CREATE TABLE IF NOT EXISTS profile_media (
                username   TEXT NOT NULL,
                kind       TEXT NOT NULL,
                mime       TEXT NOT NULL,
                data       BLOB NOT NULL,
                PRIMARY KEY (username, kind)
            );
            "#,
        )?;
        // Columns added after the first release. SQLite has no
        // "ADD COLUMN IF NOT EXISTS", so ignore the duplicate-column error
        // on databases that already have them.
        for stmt in [
            "ALTER TABLE users ADD COLUMN extra TEXT NOT NULL DEFAULT '{}'",
            "ALTER TABLE users ADD COLUMN avatar_ver INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE users ADD COLUMN banner_ver INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE users ADD COLUMN media_acl TEXT NOT NULL DEFAULT ''",
            "ALTER TABLE users ADD COLUMN password TEXT NOT NULL DEFAULT ''",
        ] {
            if let Err(e) = db.execute(stmt, []) {
                if !e.to_string().contains("duplicate column") {
                    return Err(e);
                }
            }
        }
        Ok(())
    }

    pub fn open() -> anyhow::Result<Self> {
        let path = std::env::var("AQUA_DIR_DB").unwrap_or_else(|_| "aqua-directory.sqlite".into());
        let db = Connection::open(&path)?;
        Self::init_schema(&db)?;
        // SMTP: env vars win, else the admin panel's aqua-smtp.json.
        let (smtp, source) = match SmtpConfig::from_env() {
            Some(c) => (Some(c), "env"),
            None => match SmtpConfig::from_file() {
                Some(c) => (Some(c), "file"),
                None => (None, "none"),
            },
        };
        // Dev mode (code returned in API + logged) is on unless real
        // SMTP is configured. AQUA_DEV_CODES=1 forces it on even then.
        let dev_forced = std::env::var("AQUA_DEV_CODES").ok().map(|v| v != "0");
        let dev_codes = dev_forced.unwrap_or(smtp.is_none());
        tracing::info!(%path, dev_codes, smtp = smtp.is_some(), "user directory opened");
        Ok(Self {
            db: Mutex::new(db),
            dev_forced,
            smtp: std::sync::RwLock::new(smtp),
            smtp_source: std::sync::RwLock::new(source),
            recent: Mutex::new(Default::default()),
        })
    }

    // ---- email verification ----

    /// Generate + store a 6-digit code for `email`. Returns the code
    /// only when running in dev mode (so the UI can show it); otherwise
    /// returns None and the code is delivered out of band.
    pub fn request_code(&self, email: &str) -> Result<Option<String>, DirError> {
        let email = email.trim().to_lowercase();
        if !email.contains('@') || email.len() < 5 {
            return Err(DirError::BadCode);
        }
        let mut n = [0u8; 4];
        OsRng.fill_bytes(&mut n);
        let code = format!("{:06}", u32::from_be_bytes(n) % 1_000_000);
        {
            let db = self.db.lock().unwrap();
            db.execute(
                "INSERT INTO email_codes (email, code, ticket, issued_at, verified_at)
                 VALUES (?1, ?2, NULL, ?3, NULL)
                 ON CONFLICT(email) DO UPDATE SET code=?2, ticket=NULL, issued_at=?3, verified_at=NULL",
                rusqlite::params![email, code, now_secs() as i64],
            )?;
        }
        self.send_email_code(&email, &code);
        Ok(if self.dev_codes() { Some(code) } else { None })
    }

    /// Check a code; on success mint + store a one-time ticket.
    pub fn verify_code(&self, email: &str, code: &str) -> Result<String, DirError> {
        let email = email.trim().to_lowercase();
        let db = self.db.lock().unwrap();
        let row = db
            .query_row(
                "SELECT code, issued_at FROM email_codes WHERE email=?1",
                rusqlite::params![email],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
            )
            .map_err(|_| DirError::NoCode)?;
        let (stored, issued_at) = row;
        if now_secs().saturating_sub(issued_at as u64) > CODE_TTL_SECS {
            return Err(DirError::BadCode);
        }
        if stored != code.trim() {
            return Err(DirError::BadCode);
        }
        let mut t = [0u8; 18];
        OsRng.fill_bytes(&mut t);
        let ticket = hex::encode(t);
        db.execute(
            "UPDATE email_codes SET ticket=?1, verified_at=?2 WHERE email=?3",
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
            if let Err(e) = send_smtp(&cfg, &email, &code).await {
                tracing::error!(%email, error = %e, "SMTP send failed");
            } else {
                tracing::info!(%email, "verification code emailed");
            }
        });
    }

    /// Recent dev-mode codes, newest first (admin panel).
    pub fn recent_codes(&self) -> Vec<(String, String, u64)> {
        self.recent.lock().map(|r| r.iter().cloned().collect()).unwrap_or_default()
    }

    /// True when codes are shown instead of mailed.
    pub fn dev_codes(&self) -> bool {
        let has_smtp = self.smtp.read().map(|c| c.is_some()).unwrap_or(false);
        self.dev_forced.unwrap_or(!has_smtp)
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
        send_smtp(&cfg, to.trim(), "123456").await
    }

    // ---- registration ----

    /// Validate a username slug.
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

    fn check_owner(db: &Connection, username: &str, identity_dh: &[u8; 32]) -> Result<(), DirError> {
        let owner: Vec<u8> = db
            .query_row(
                "SELECT identity_dh FROM users WHERE username=?1",
                rusqlite::params![username],
                |r| r.get(0),
            )
            .map_err(|_| DirError::NotOwner)?;
        if owner.as_slice() != identity_dh.as_slice() {
            return Err(DirError::NotOwner);
        }
        Ok(())
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
        if !matches!(mime.as_str(), "image/png" | "image/jpeg" | "image/webp" | "image/gif") {
            return Err(DirError::BadProfile(format!("unsupported image type {mime:?}")));
        }
        if data.is_empty() || data.len() > PROFILE_MEDIA_LIMIT {
            return Err(DirError::BadProfile("image is empty or larger than 8 MB".into()));
        }
        let db = self.db.lock().unwrap();
        Self::check_owner(&db, &username, identity_dh)?;
        db.execute(
            "INSERT INTO profile_media (username, kind, mime, data) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(username, kind) DO UPDATE SET mime=?3, data=?4",
            rusqlite::params![username, kind.as_str(), mime, data],
        )?;
        let old: i64 = db
            .query_row(&format!("SELECT {} FROM users WHERE username=?1", kind.ver_col()), rusqlite::params![username], |r| r.get(0))
            .unwrap_or(0);
        let ver = (now_secs() as i64).max(old + 1);
        db.execute(&format!("UPDATE users SET {}=?2 WHERE username=?1", kind.ver_col()), rusqlite::params![username, ver])?;
        Ok(ver)
    }

    /// Remove the owner's avatar or banner.
    pub fn clear_media(&self, username: &str, identity_dh: &[u8; 32], kind: MediaKind) -> Result<(), DirError> {
        let username = username.trim().to_lowercase();
        let db = self.db.lock().unwrap();
        Self::check_owner(&db, &username, identity_dh)?;
        db.execute("DELETE FROM profile_media WHERE username=?1 AND kind=?2", rusqlite::params![username, kind.as_str()])?;
        db.execute(&format!("UPDATE users SET {}=0 WHERE username=?1", kind.ver_col()), rusqlite::params![username])?;
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
    pub fn login(&self, ticket: &str, password: &str, identity_dh: &[u8; 32]) -> Result<String, DirError> {
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
        db.execute("UPDATE users SET identity_dh=?2 WHERE username=?1", rusqlite::params![username, identity_dh.as_slice()])?;
        // One-time ticket.
        db.execute("UPDATE email_codes SET ticket=NULL WHERE ticket=?1", rusqlite::params![ticket])?;
        Ok(username)
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
        db.query_row(
            "SELECT username FROM users WHERE identity_dh=?1 AND is_bot=0 LIMIT 1",
            rusqlite::params![identity_dh.as_slice()],
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
        let q = query.trim().to_lowercase();
        if q.is_empty() { return Vec::new(); }
        let like = format!("%{}%", q.replace('%', "").replace('_', ""));
        let db = self.db.lock().unwrap();
        let sql = format!(
            "SELECT {PROFILE_COLS} FROM users
             WHERE username LIKE ?1 OR lower(display_name) LIKE ?1
             ORDER BY username LIMIT ?2"
        );
        let mut stmt = match db.prepare(&sql) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let rows = stmt.query_map(rusqlite::params![like, limit as i64], profile_from_row);
        match rows {
            Ok(it) => it.filter_map(|r| r.ok()).collect(),
            Err(_) => Vec::new(),
        }
    }

    // ---- bots ----

    /// Register a bot account owned by `owner_dh`. The bot is a normal
    /// directory user with `is_bot = 1`; a random token is minted so
    /// the bot program can authenticate over the HTTP bot API. Returns
    /// the token. The username must be free (or already a bot owned by
    /// the same identity, in which case the token is rotated).
    pub fn register_bot(
        &self,
        username: &str,
        owner_dh: &[u8; 32],
        display_name: &str,
    ) -> Result<String, DirError> {
        let username = username.trim().to_lowercase();
        if !Self::valid_username(&username) {
            return Err(DirError::BadUsername);
        }
        let db = self.db.lock().unwrap();
        // If the username exists and isn't a bot owned by us, refuse.
        if let Ok((owner, is_bot)) = db.query_row(
            "SELECT identity_dh, is_bot FROM users WHERE username=?1",
            rusqlite::params![username],
            |r| Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, i64>(1)?)),
        ) {
            if is_bot == 0 || owner.as_slice() != owner_dh.as_slice() {
                return Err(DirError::UsernameTaken);
            }
        }
        // Upsert the bot as a directory user.
        db.execute(
            "INSERT INTO users (username, identity_dh, email, display_name, about, phone, is_bot, created_at)
             VALUES (?1, ?2, NULL, ?3, '', '', 1, ?4)
             ON CONFLICT(username) DO UPDATE SET display_name=?3, is_bot=1",
            rusqlite::params![username, owner_dh.as_slice(), display_name.trim(), now_secs() as i64],
        )?;
        // Rotate any old token for this bot, mint a fresh one.
        db.execute("DELETE FROM bot_tokens WHERE username=?1", rusqlite::params![username])?;
        let mut t = [0u8; 24];
        OsRng.fill_bytes(&mut t);
        let token = format!("aqbot_{}", hex::encode(t));
        db.execute(
            "INSERT INTO bot_tokens (token, username, owner_dh, created_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![token, username, owner_dh.as_slice(), now_secs() as i64],
        )?;
        tracing::info!(%username, "bot registered");
        Ok(token)
    }

    /// Resolve a bot token → its username.
    pub fn bot_by_token(&self, token: &str) -> Option<String> {
        let db = self.db.lock().unwrap();
        db.query_row(
            "SELECT username FROM bot_tokens WHERE token=?1",
            rusqlite::params![token],
            |r| r.get::<_, String>(0),
        ).ok()
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

    /// List bots owned by an identity (for the "my bots" management UI).
    pub fn list_bots_for_owner(&self, owner_dh: &[u8; 32]) -> Vec<(String, String)> {
        let db = self.db.lock().unwrap();
        let mut stmt = match db.prepare(
            "SELECT u.username, u.display_name, bt.token
             FROM bot_tokens bt JOIN users u ON u.username = bt.username
             WHERE bt.owner_dh = ?1 ORDER BY u.username",
        ) { Ok(s) => s, Err(_) => return Vec::new() };
        let rows = stmt.query_map(rusqlite::params![owner_dh.as_slice()], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(2)?))
        });
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
fn build_code_email(cfg: &SmtpConfig, to: &str, code: &str) -> anyhow::Result<lettre::Message> {
    use lettre::message::{Message, MultiPart};
    let text = format!(
        "Aquamarine\n\n\
         Ваш код подтверждения: {code}\n\
         Your verification code: {code}\n\n\
         Код действует 10 минут. Если вы его не запрашивали, просто проигнорируйте это письмо.\n\
         It expires in 10 minutes. If you didn't request it, ignore this email."
    );
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

async fn send_smtp(cfg: &SmtpConfig, to: &str, code: &str) -> anyhow::Result<()> {
    use lettre::{
        transport::smtp::authentication::Credentials,
        AsyncSmtpTransport, AsyncTransport, Tokio1Executor,
    };
    let email = build_code_email(cfg, to, code)?;
    let creds = Credentials::new(cfg.user.clone(), cfg.pass.clone());
    // 465 = implicit TLS (SSL); anything else (587) = STARTTLS.
    let builder = if cfg.port == 465 {
        AsyncSmtpTransport::<Tokio1Executor>::relay(&cfg.host)?
    } else {
        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&cfg.host)?
    };
    let mailer: AsyncSmtpTransport<Tokio1Executor> = builder
        .port(cfg.port)
        .credentials(creds)
        .timeout(Some(std::time::Duration::from_secs(20)))
        .build();
    mailer.send(email).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> Directory { Directory::open_memory() }

    #[test]
    fn code_email_builds() {
        let cfg = SmtpConfig { host: "smtp.example.com".into(), user: "me@example.com".into(),
            pass: "x".into(), from: "me@example.com".into(), port: 587 };
        let m = build_code_email(&cfg, "you@example.com", "042517").unwrap();
        let raw = String::from_utf8(m.formatted()).unwrap();
        assert!(raw.contains("From: Aquamarine <me@example.com>"));
        assert!(raw.contains("multipart/alternative"));
        assert!(raw.contains("042517"));
        if let Ok(p) = std::env::var("AQUA_DUMP_EMAIL") { std::fs::write(p, raw).unwrap(); }
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
        assert_eq!(d.identity_for("media"), Some(new_dev));
        assert!(d.login(&t2, "correct horse", &new_dev).is_err()); // ticket is single-use
        // back to the old identity for the rest of the test
        let code = d.request_code("m@x.com").unwrap().unwrap();
        let t3 = d.verify_code("m@x.com", &code).unwrap();
        d.login(&t3, "correct horse", &id).unwrap();

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
    fn bad_username_rejected() {
        let d = dir();
        let code = d.request_code("e@x.com").unwrap().unwrap();
        let t = d.verify_code("e@x.com", &code).unwrap();
        assert!(matches!(
            d.register("ab", &[1u8; 32], &t, "", "", ""),
            Err(DirError::BadUsername)
        ));
    }
}
