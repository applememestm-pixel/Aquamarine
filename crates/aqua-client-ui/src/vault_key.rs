//! The key of an account's local vault on Android (AQUA-SEC-013). It
//! protects local storage only: identity keys and sessions are untouched.
//!
//! Files in the account's folder:
//! * `master.key`    — the key as it is, kept until the Keystore has proven itself;
//! * `master.key.ks` — the key wrapped by a key that never leaves the Android
//!   Keystore (the format of 1.21.0-23);
//! * `master.key.rk` — the key wrapped by the account's 24 recovery words;
//! * `keystore.json` — how far the move into the Keystore has got.
//!
//! The rules: at every step one file that opens the vault stays on disk;
//! `master.key` goes only after the Keystore opened its copy after the phone
//! was restarted; a new key is never made next to an existing vault; nothing
//! the person has is deleted without being asked.

#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
use aqua_client_core::vault::{MasterKey, Vault};
use aqua_client_core::MASTER_KEY_LEN;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::fsx::write_atomic;

pub type Key = [u8; MASTER_KEY_LEN];

const PLAIN: &str = "master.key";
const WRAPPED: &str = "master.key.ks";
const RECOVERY: &str = "master.key.rk";
const RECORD: &str = "keystore.json";
/// The vault record with the 24 recovery words (backups.rs).
const PHRASE: &str = "backup:phrase";
/// This many failures and the Keystore of this phone isn't tried again.
const GIVE_UP_AFTER: u32 = 2;
/// Where the phone doesn't count its restarts: this many starts over this
/// long instead.
const STARTS_WITHOUT_BOOT_COUNT: u32 = 3;
const SECS_WITHOUT_BOOT_COUNT: u64 = 72 * 3600;

/// The system keystore.
pub trait Wrapper {
    fn wrap(&self, key: &Key) -> Result<String, String>;
    fn unwrap(&self, blob: &str) -> Result<Key, String>;
    /// How many times the phone has been started (`None`: it doesn't say).
    fn boot_count(&self) -> Option<i64>;
    /// A line in the system log (no key material): the release check reads it.
    fn note(&self, _stage: &str) {}
}

/// The vault's key can't be had: the app shows the recovery screen.
#[derive(Debug)]
pub struct Locked(pub String);

impl std::fmt::Display for Locked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "this account's data can't be opened: {}", self.0)
    }
}

impl std::error::Error for Locked {}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct Record {
    /// The phone's start count when the key was wrapped.
    wrapped_boot: Option<i64>,
    wrapped_at: u64,
    /// Starts on which the wrapped copy opened since.
    starts: u32,
    failures: u32,
}

fn load(dir: &Path) -> Record {
    std::fs::read(dir.join(RECORD)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save(dir: &Path, r: &Record) {
    if let Err(e) = serde_json::to_vec(r).map_err(std::io::Error::other).and_then(|b| write_atomic(&dir.join(RECORD), &b)) {
        tracing::warn!(%e, "keystore state not saved");
    }
}

/// The key of the vault in `dir`; `opens` says whether a key opens it.
pub fn resolve(dir: &Path, ks: &dyn Wrapper, opens: &dyn Fn(&Key) -> bool, now: u64) -> Result<Key> {
    finish_set_aside(dir)?;
    let plain_path = dir.join(PLAIN);
    let wrapped_path = dir.join(WRAPPED);
    // `Some(None)`: the file is there but isn't a key.
    let plain: Option<Option<Key>> = match std::fs::read(&plain_path) {
        Ok(b) => Some(Key::try_from(b.as_slice()).ok()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    let usable = plain.flatten().filter(|k| opens(k));
    let mut rec = load(dir);

    if wrapped_path.exists() {
        let got = std::fs::read_to_string(&wrapped_path).map_err(|e| e.to_string()).and_then(|b| ks.unwrap(b.trim()));
        let why = match got {
            Ok(k) if opens(&k) => {
                confirm(dir, ks, &k, plain, rec, now);
                return Ok(k);
            }
            Ok(_) => "the key in the keystore doesn't open this data".to_string(),
            Err(e) => e,
        };
        // The plain key is still here: use it, and wrap it anew another time.
        let Some(k) = usable else {
            ks.note("locked");
            return Err(Locked(why).into());
        };
        tracing::warn!(%why, "the keystore let us down; the plain key file is used");
        std::fs::rename(&wrapped_path, dir.join("master.key.ks.bad"))?;
        rec.failures += 1;
        rec.starts = 0;
        save(dir, &rec);
        ks.note("fallback");
        return Ok(k);
    }

    if plain.is_some() {
        let Some(k) = usable else {
            ks.note("locked");
            return Err(Locked("the key file doesn't open this data".into()).into());
        };
        protect(dir, ks, &k, rec, now);
        return Ok(k);
    }

    if dir.join("vault.sqlite").metadata().map(|m| m.len() > 0).unwrap_or(false) {
        ks.note("locked");
        return Err(Locked("the key of this data is missing".into()).into());
    }
    std::fs::create_dir_all(dir)?;
    let key = MasterKey::generate().0;
    // On disk first: the Keystore is an addition, never the only copy yet.
    write_atomic(&plain_path, &key)?;
    protect(dir, ks, &key, rec, now);
    Ok(key)
}

/// [`resolve`] once a start of the app: a state rebuilt later (after a
/// restore from a backup) takes the same key without going to the Keystore
/// again, under the app's lock.
#[cfg(target_os = "android")]
pub fn resolve_once(dir: &Path, opens: &dyn Fn(&Key) -> bool) -> Result<Key> {
    static OPENED: std::sync::Mutex<Option<(PathBuf, Key)>> = std::sync::Mutex::new(None);
    let mut opened = OPENED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((d, k)) = opened.as_ref() {
        if d == dir && opens(k) {
            return Ok(*k);
        }
    }
    let key = resolve(dir, &AndroidKeystore, opens, now_secs())?;
    *opened = Some((dir.to_path_buf(), key));
    Ok(key)
}

/// Put a copy of `key` under the Keystore; the plain file stays.
fn protect(dir: &Path, ks: &dyn Wrapper, key: &Key, mut rec: Record, now: u64) {
    if rec.failures >= GIVE_UP_AFTER {
        return;
    }
    let blob = match ks.wrap(key) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(%e, "keystore unavailable; the plain key file stays");
            return;
        }
    };
    if ks.unwrap(&blob).ok().as_ref() != Some(key) {
        rec.failures += 1;
        save(dir, &rec);
        return;
    }
    if let Err(e) = write_atomic(&dir.join(WRAPPED), blob.as_bytes()) {
        tracing::warn!(%e, "the wrapped key couldn't be written");
        return;
    }
    rec.wrapped_boot = ks.boot_count();
    rec.wrapped_at = now;
    rec.starts = 0;
    save(dir, &rec);
    ks.note("pending");
}

/// The wrapped copy opened. Once it has done so after a restart of the phone
/// (some keystores lose their keys exactly then), the plain file goes.
fn confirm(dir: &Path, ks: &dyn Wrapper, key: &Key, plain: Option<Option<Key>>, mut rec: Record, now: u64) {
    let Some(plain) = plain else {
        ks.note("confirmed");
        return;
    };
    let plain_path = dir.join(PLAIN);
    if plain != Some(*key) {
        // Another key (an older build made it): kept, out of the way.
        let _ = std::fs::rename(&plain_path, dir.join("master.key.unused"));
        return;
    }
    if rec.wrapped_at == 0 {
        rec.wrapped_boot = ks.boot_count();
        rec.wrapped_at = now;
    }
    rec.starts += 1;
    let proven = match (rec.wrapped_boot, ks.boot_count()) {
        (Some(then), Some(this)) => then != this,
        _ => rec.starts >= STARTS_WITHOUT_BOOT_COUNT && now.saturating_sub(rec.wrapped_at) >= SECS_WITHOUT_BOOT_COUNT,
    };
    save(dir, &rec);
    if !proven {
        ks.note("pending");
        return;
    }
    match std::fs::remove_file(&plain_path) {
        Ok(()) => ks.note("confirmed"),
        Err(e) => tracing::warn!(%e, "the plain key file couldn't be removed"),
    }
}

/// Android: keep `master.key.rk` in step with the account's recovery words,
/// so they can open the vault here if the Keystore fails.
pub fn keep_recovery(dir: &Path, vault: &Vault) {
    if cfg!(any(target_os = "android", test)) {
        write_recovery(dir, vault);
    }
}

fn write_recovery(dir: &Path, vault: &Vault) {
    let Some(phrase) = vault.get_record(PHRASE).ok().flatten() else { return };
    let phrase = String::from_utf8_lossy(&phrase).trim().to_string();
    let path = dir.join(RECOVERY);
    if std::fs::read(&path).is_ok_and(|b| vault.recovery_matches(&phrase, &b)) {
        return;
    }
    match vault.recovery_wrap(&phrase) {
        Ok(b) => {
            if let Err(e) = write_atomic(&path, &b) {
                tracing::warn!(%e, "the recovery copy of the vault key wasn't written");
            }
        }
        Err(e) => tracing::warn!(%e, "the recovery copy of the vault key wasn't made"),
    }
}

/// Open the vault key with the recovery words and give it back to this phone
/// (the next start wraps it anew).
pub fn restore_with_phrase(dir: &Path, phrase: &str, opens: &dyn Fn(&Key) -> bool) -> Result<(), String> {
    let blob = std::fs::read(dir.join(RECOVERY)).map_err(|_| "there is no copy of the key for the recovery words on this phone".to_string())?;
    let key = aqua_client_core::backup::unwrap_local(phrase, &blob).map_err(|e| e.to_string())?;
    if !opens(&key) {
        return Err("this key doesn't open the data on this phone".into());
    }
    // A key file that doesn't open the data is kept all the same.
    let plain = dir.join(PLAIN);
    if std::fs::read(&plain).is_ok_and(|b| b != key) {
        std::fs::rename(&plain, dir.join(format!("master.key.unused-{}", now_secs()))).map_err(|e| e.to_string())?;
    }
    write_atomic(&plain, &key).map_err(|e| e.to_string())?;
    if dir.join(WRAPPED).exists() {
        std::fs::rename(dir.join(WRAPPED), dir.join("master.key.ks.bad")).map_err(|e| e.to_string())?;
    }
    // A fresh start for the Keystore too.
    let _ = std::fs::remove_file(dir.join(RECORD));
    Ok(())
}

/// What belongs to the account's vault and sign-in. Media, downloads, other
/// accounts and the app's settings stay where they are.
const ACCOUNT_FILES: &[&str] = &[
    "vault.sqlite", "vault.sqlite-wal", "vault.sqlite-shm", "vault.sqlite-journal",
    PLAIN, WRAPPED, "master.key.ks.bad", RECOVERY, "master.key.unused", RECORD,
    "account.json", "last-account.json", "connection.json", "dev-trust.json", "dev-lists.json",
    "hist-sync.json", "saved-messages.json", "groups.json", "mesh.json", "outbox",
];
const MOVING: &str = "set-aside.pending";

/// Move the account that can't be opened into `locked-<time>/`, so a fresh
/// one can sign in. Nothing is deleted.
pub fn set_aside(dir: &Path, now: u64) -> std::io::Result<PathBuf> {
    // A move cut short goes on into the same folder.
    let name = std::fs::read_to_string(dir.join(MOVING))
        .ok()
        .filter(|n| n.starts_with("locked-") && !n.contains(['/', '\\']))
        .unwrap_or_else(|| format!("locked-{now}"));
    write_atomic(&dir.join(MOVING), name.as_bytes())?;
    let to = dir.join(name);
    std::fs::create_dir_all(&to)?;
    for f in ACCOUNT_FILES {
        let from = dir.join(f);
        if from.exists() {
            std::fs::rename(&from, to.join(f))?;
        }
    }
    std::fs::remove_file(dir.join(MOVING))?;
    Ok(to)
}

fn finish_set_aside(dir: &Path) -> std::io::Result<()> {
    if dir.join(MOVING).exists() {
        set_aside(dir, 0)?;
    }
    Ok(())
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[cfg(target_os = "android")]
pub struct AndroidKeystore;

#[cfg(target_os = "android")]
impl Wrapper for AndroidKeystore {
    fn wrap(&self, key: &Key) -> Result<String, String> {
        crate::relay_host::android::ks_wrap(&hex::encode(key))
    }
    fn unwrap(&self, blob: &str) -> Result<Key, String> {
        let hex_key = crate::relay_host::android::ks_unwrap(blob)?;
        hex::decode(hex_key.trim()).ok().and_then(|b| Key::try_from(b.as_slice()).ok()).ok_or_else(|| "the keystore gave a malformed key".into())
    }
    fn boot_count(&self) -> Option<i64> {
        crate::relay_host::android::ks_boot_count()
    }
    fn note(&self, stage: &str) {
        crate::relay_host::android::ks_note(stage);
    }
}

// ---------- The recovery screen ------------------------------------------

/// Managed instead of the app's state when the vault's key can't be had.
pub struct BootFailure {
    pub dir: PathBuf,
    pub reason: String,
}

static FAILED: AtomicBool = AtomicBool::new(false);

pub fn enter_recovery(app: &tauri::App, dir: PathBuf, reason: String) {
    tracing::error!(%reason, "the vault can't be opened; showing the recovery screen");
    FAILED.store(true, Ordering::SeqCst);
    app.manage(BootFailure { dir, reason });
}

pub fn failed() -> bool {
    FAILED.load(Ordering::SeqCst)
}

/// The commands that work without the app's state.
pub fn allowed(cmd: &str) -> bool {
    cmd.starts_with("vault_")
        || matches!(cmd, "accounts_list" | "account_switch" | "account_boot" | "app_relaunch" | "report_error" | "report_config")
}

#[derive(Serialize)]
pub struct BootInfo {
    pub reason: String,
    /// The 24 recovery words can open the data on this phone.
    pub has_recovery: bool,
}

/// `None`: the app started normally.
#[tauri::command]
pub fn vault_boot_state(app: AppHandle) -> Option<BootInfo> {
    let f = app.try_state::<BootFailure>()?;
    Some(BootInfo { reason: f.reason.clone(), has_recovery: f.dir.join(RECOVERY).exists() })
}

/// Recovery screen: open the data with the 24 words, then start over.
#[tauri::command]
pub fn vault_unlock_phrase(app: AppHandle, phrase: String) -> Result<(), String> {
    let dir = app.try_state::<BootFailure>().ok_or("the data is already open")?.dir.clone();
    restore_with_phrase(&dir, &phrase, &|k| crate::state::key_opens(&dir, k))?;
    crate::accounts::app_relaunch(app);
    Ok(())
}

/// Recovery screen, confirmed by the person: put the unreadable data aside
/// and start as a new device.
#[tauri::command]
pub fn vault_set_aside(app: AppHandle) -> Result<(), String> {
    let dir = app.try_state::<BootFailure>().ok_or("the data is already open")?.dir.clone();
    set_aside(&dir, now_secs()).map_err(|e| e.to_string())?;
    crate::accounts::app_relaunch(app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    /// A keystore that wraps by reversing the hex, and can be broken.
    struct Fake {
        boot: Cell<Option<i64>>,
        broken: Cell<bool>,
        notes: RefCell<Vec<String>>,
    }

    impl Fake {
        fn new() -> Self {
            Self { boot: Cell::new(Some(1)), broken: Cell::new(false), notes: RefCell::new(Vec::new()) }
        }
    }

    impl Wrapper for Fake {
        fn wrap(&self, key: &Key) -> Result<String, String> {
            if self.broken.get() {
                return Err("ks:nokey".into());
            }
            Ok(hex::encode(key).chars().rev().collect())
        }
        fn unwrap(&self, blob: &str) -> Result<Key, String> {
            if self.broken.get() {
                return Err("ks:nokey".into());
            }
            let h: String = blob.chars().rev().collect();
            hex::decode(h).ok().and_then(|b| Key::try_from(b.as_slice()).ok()).ok_or_else(|| "ks:badtag".into())
        }
        fn boot_count(&self) -> Option<i64> {
            self.boot.get()
        }
        fn note(&self, stage: &str) {
            self.notes.borrow_mut().push(stage.into());
        }
    }

    fn any(_: &Key) -> bool {
        true
    }

    fn has(d: &Path, f: &str) -> bool {
        d.join(f).exists()
    }

    #[test]
    fn plain_key_stays_until_the_phone_was_restarted() {
        let d = tempfile::tempdir().unwrap();
        let d = d.path();
        let ks = Fake::new();
        let k = resolve(d, &ks, &any, 10).unwrap();
        assert!(has(d, PLAIN) && has(d, WRAPPED));
        // Any number of starts of the app, the same start of the phone.
        for now in 11..20 {
            assert_eq!(resolve(d, &ks, &any, now).unwrap(), k);
        }
        assert!(has(d, PLAIN));
        ks.boot.set(Some(2));
        assert_eq!(resolve(d, &ks, &any, 30).unwrap(), k);
        assert!(!has(d, PLAIN) && has(d, WRAPPED));
        assert_eq!(resolve(d, &ks, &any, 31).unwrap(), k);
        assert_eq!(ks.notes.borrow().last().unwrap(), "confirmed");
    }

    #[test]
    fn an_existing_key_moves_in_unchanged() {
        let d = tempfile::tempdir().unwrap();
        let d = d.path();
        std::fs::write(d.join(PLAIN), [7u8; MASTER_KEY_LEN]).unwrap();
        let ks = Fake::new();
        assert_eq!(resolve(d, &ks, &any, 1).unwrap(), [7u8; MASTER_KEY_LEN]);
        ks.boot.set(Some(5));
        assert_eq!(resolve(d, &ks, &any, 2).unwrap(), [7u8; MASTER_KEY_LEN]);
        assert!(!has(d, PLAIN));
    }

    #[test]
    fn without_a_start_count_it_takes_starts_and_days() {
        let d = tempfile::tempdir().unwrap();
        let d = d.path();
        let ks = Fake::new();
        ks.boot.set(None);
        resolve(d, &ks, &any, 100).unwrap();
        resolve(d, &ks, &any, 101).unwrap();
        resolve(d, &ks, &any, 102).unwrap();
        resolve(d, &ks, &any, 103).unwrap();
        assert!(has(d, PLAIN), "starts without time");
        resolve(d, &ks, &any, 100 + SECS_WITHOUT_BOOT_COUNT).unwrap();
        assert!(!has(d, PLAIN));
    }

    #[test]
    fn a_keystore_that_fails_after_a_restart_costs_nothing_and_is_given_up() {
        let d = tempfile::tempdir().unwrap();
        let d = d.path();
        let ks = Fake::new();
        let k = resolve(d, &ks, &any, 1).unwrap();
        for round in 0..GIVE_UP_AFTER {
            ks.broken.set(true);
            ks.boot.set(Some(2 + i64::from(round)));
            assert_eq!(resolve(d, &ks, &any, 2).unwrap(), k);
            assert!(has(d, PLAIN) && !has(d, WRAPPED));
            ks.broken.set(false);
            assert_eq!(resolve(d, &ks, &any, 3).unwrap(), k);
        }
        assert!(!has(d, WRAPPED), "not tried again");
        assert!(has(d, PLAIN));
    }

    #[test]
    fn never_a_new_key_next_to_a_vault() {
        let d = tempfile::tempdir().unwrap();
        let d = d.path();
        std::fs::write(d.join("vault.sqlite"), b"data").unwrap();
        let ks = Fake::new();
        let e = resolve(d, &ks, &any, 1).unwrap_err();
        assert!(e.downcast_ref::<Locked>().is_some());
        assert!(!has(d, PLAIN) && !has(d, WRAPPED));
        // A wrapped copy the keystore no longer opens, and nothing else.
        std::fs::write(d.join(WRAPPED), b"abc").unwrap();
        ks.broken.set(true);
        let e = resolve(d, &ks, &any, 2).unwrap_err();
        assert!(e.context("loading master key").downcast_ref::<Locked>().is_some(), "found through context");
        assert!(has(d, WRAPPED), "left as it is");
        assert_eq!(std::fs::read(d.join("vault.sqlite")).unwrap(), b"data");
    }

    #[test]
    fn of_two_keys_the_one_that_opens_the_data_wins() {
        let d = tempfile::tempdir().unwrap();
        let d = d.path();
        let ks = Fake::new();
        let real = [1u8; MASTER_KEY_LEN];
        let opens = |k: &Key| *k == real;
        std::fs::write(d.join(WRAPPED), ks.wrap(&real).unwrap()).unwrap();
        std::fs::write(d.join(PLAIN), [2u8; MASTER_KEY_LEN]).unwrap();
        assert_eq!(resolve(d, &ks, &opens, 1).unwrap(), real);
        assert!(has(d, "master.key.unused") && !has(d, PLAIN));
        // And the other way round.
        std::fs::write(d.join(WRAPPED), ks.wrap(&[3u8; MASTER_KEY_LEN]).unwrap()).unwrap();
        std::fs::write(d.join(PLAIN), real).unwrap();
        assert_eq!(resolve(d, &ks, &opens, 2).unwrap(), real);
        assert!(has(d, PLAIN));
    }

    /// Whatever step the app was stopped after, the key is still to be had.
    #[test]
    fn stopped_halfway() {
        let k = [9u8; MASTER_KEY_LEN];
        let ks = Fake::new();
        for (plain, wrapped, record) in [(true, false, false), (true, true, false), (true, true, true), (false, true, false)] {
            let d = tempfile::tempdir().unwrap();
            let d = d.path();
            std::fs::write(d.join("vault.sqlite"), b"data").unwrap();
            if plain {
                std::fs::write(d.join(PLAIN), k).unwrap();
            }
            if wrapped {
                std::fs::write(d.join(WRAPPED), ks.wrap(&k).unwrap()).unwrap();
            }
            if record {
                save(d, &Record { wrapped_boot: Some(1), wrapped_at: 1, starts: 1, failures: 0 });
            }
            assert_eq!(resolve(d, &ks, &any, 2).unwrap(), k);
            assert!(has(d, PLAIN) || has(d, WRAPPED));
        }
    }

    #[test]
    fn set_aside_moves_the_account_and_only_it() {
        let d = tempfile::tempdir().unwrap();
        let d = d.path();
        std::fs::create_dir_all(d.join("accounts/a1")).unwrap();
        std::fs::create_dir_all(d.join("stickers")).unwrap();
        for f in ["vault.sqlite", WRAPPED, "account.json", "webstore.json", "accounts/a1/vault.sqlite"] {
            std::fs::write(d.join(f), f).unwrap();
        }
        let to = set_aside(d, 42).unwrap();
        assert_eq!(to, d.join("locked-42"));
        assert!(!has(d, "vault.sqlite") && !has(d, WRAPPED) && !has(d, "account.json"));
        assert_eq!(std::fs::read(to.join("vault.sqlite")).unwrap(), b"vault.sqlite");
        assert!(has(d, "webstore.json") && has(d, "accounts/a1/vault.sqlite") && has(d, "stickers"));
        // A fresh account starts there.
        let ks = Fake::new();
        resolve(d, &ks, &any, 43).unwrap();
    }

    #[test]
    fn a_move_cut_short_is_finished_at_the_next_start() {
        let d = tempfile::tempdir().unwrap();
        let d = d.path();
        std::fs::create_dir_all(d.join("locked-7")).unwrap();
        std::fs::write(d.join("locked-7/vault.sqlite"), b"old").unwrap();
        std::fs::write(d.join(WRAPPED), b"blob").unwrap();
        std::fs::write(d.join(MOVING), "locked-7").unwrap();
        let ks = Fake::new();
        resolve(d, &ks, &any, 9).unwrap();
        assert_eq!(std::fs::read(d.join("locked-7").join(WRAPPED)).unwrap(), b"blob");
        assert!(!has(d, MOVING));
    }

    #[test]
    fn the_recovery_words_bring_the_key_back() {
        let d = tempfile::tempdir().unwrap();
        let d = d.path();
        let ks = Fake::new();
        let key = resolve(d, &ks, &any, 1).unwrap();
        let vault = Vault::open(d.join("vault.sqlite"), MasterKey::from_bytes(key)).unwrap();
        let phrase = aqua_client_core::backup::new_phrase();
        vault.put_record(PHRASE, phrase.as_bytes()).unwrap();
        keep_recovery(d, &vault);
        drop(vault);
        // The Keystore forgets everything.
        let _ = std::fs::remove_file(d.join(PLAIN));
        ks.broken.set(true);
        assert!(resolve(d, &ks, &any, 2).is_err());
        let other = aqua_client_core::backup::new_phrase();
        assert!(restore_with_phrase(d, &other, &any).is_err());
        assert!(!has(d, PLAIN));
        std::fs::write(d.join(PLAIN), b"something else").unwrap();
        restore_with_phrase(d, &phrase, &any).unwrap();
        assert!(std::fs::read_dir(d).unwrap().any(|f| f.unwrap().file_name().to_string_lossy().starts_with("master.key.unused-")));
        ks.broken.set(false);
        assert_eq!(resolve(d, &ks, &any, 3).unwrap(), key);
    }
}
