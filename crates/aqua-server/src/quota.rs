//! Each account's share of this server, by level (owner, 20.11: "у меня у
//! всех сервов сейчас в сумме порядка 100 гигов" — so every level has a
//! limit, the top one too).
//!
//! Levels: a guest (no account here), an account, an account with two-step
//! verification, and the CeZeOs star 1–2, 3–5 and 6 (stars.rs — a signed
//! statement, so no server can make one up; the star relay is asked, the
//! answer is kept 10 minutes, and while it can't be asked the last known
//! level stands). The admin sets the limits (`aqua-admin.json` → `quotas`).
//!
//! What's counted, per "folder":
//! - files of chats (the delivery cache): each blob is charged to whoever
//!   uploaded it first (blobstore `.o.<cid>`); guests share one pool, since
//!   a new key costs nothing;
//! - history kept for the account's devices, the backup, bots.
//!
//! A level going down (a star ending, the admin lowering a limit) is
//! checked at once and hourly: files over the limit stay for `GRACE`, then
//! the oldest of that account's go (recipients already have them — the apps
//! download on arrival). History, backup and bots are never deleted: over
//! the limit only new writes are refused.
//!
//! Separately, the disk guard (blobstore `fit`) is what keeps the server
//! alive: the limits together may promise more than the disk has.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::AppState;

const MB: u64 = 1024 * 1024;
const GB: u64 = 1024 * MB;
/// After a limit goes down, files over it stay this long.
pub const GRACE: u64 = 7 * 24 * 3600;
/// The key of the guests' shared pool.
pub const GUESTS: &str = "@guests";
/// Upper bound for a disk-backed resumable upload.
pub const FILE_MAX_CAP: u64 = aqua_core::file_transfer::MAX_RESUMABLE_FILE_SIZE;
/// The backup goes in one request too (lib.rs `BACKUP_LIMIT`).
pub const BACKUP_CAP: u64 = 64 * MB;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    Guest,
    Account,
    Secure,
    Star12,
    Star35,
    Star6,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limits {
    /// Files of chats, bytes.
    pub files: u64,
    /// One file, bytes (≤ `FILE_MAX_CAP`).
    pub file_max: u64,
    /// History for the account's devices, bytes.
    pub hist: u64,
    /// The encrypted backup, bytes (0: none; ≤ `BACKUP_CAP`).
    pub backup: u64,
    pub bots: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Quotas {
    /// All guests together (files).
    pub guest_pool: u64,
    pub guest: Limits,
    pub account: Limits,
    pub secure: Limits,
    pub star12: Limits,
    pub star35: Limits,
    pub star6: Limits,
}

impl Default for Quotas {
    fn default() -> Self {
        let l = |files, file_max, hist, backup, bots| Limits { files, file_max, hist, backup, bots };
        Quotas {
            guest_pool: GB,
            guest: Limits { files: 100 * MB, file_max: 50 * MB, hist: 0, backup: 0, bots: 0 },
            account: l(GB, 200 * MB, 64 * MB, 0, 3),
            secure: l(2 * GB, 512 * MB, GB, 64 * MB, 5),
            star12: l(5 * GB, GB, 2 * GB, 64 * MB, 10),
            star35: l(10 * GB, 1536 * MB, 4 * GB, 64 * MB, 20),
            star6: l(20 * GB, 2 * GB, 8 * GB, 64 * MB, 50),
        }
    }
}

impl Quotas {
    pub fn of(&self, t: Tier) -> Limits {
        match t {
            Tier::Guest => self.guest,
            Tier::Account => self.account,
            Tier::Secure => self.secure,
            Tier::Star12 => self.star12,
            Tier::Star35 => self.star35,
            Tier::Star6 => self.star6,
        }
    }

    /// What the admin sent, within what the relay can do (and never "no limit").
    pub fn clean(mut self) -> Self {
        let fix = |l: &mut Limits| {
            l.files = l.files.min(10 * 1024 * GB);
            l.file_max = l.file_max.clamp(MB, FILE_MAX_CAP);
            l.hist = l.hist.min(1024 * GB);
            l.backup = l.backup.min(BACKUP_CAP);
            l.bots = l.bots.min(1000);
        };
        for l in [&mut self.guest, &mut self.account, &mut self.secure, &mut self.star12, &mut self.star35, &mut self.star6] {
            fix(l);
        }
        self.guest_pool = self.guest_pool.min(10 * 1024 * GB);
        self
    }
}

/// The level from what's known of an account. `star`: its CeZeOs level.
pub fn tier_from(registered: bool, two_step: bool, star: Option<u8>) -> Tier {
    if !registered {
        return Tier::Guest;
    }
    match star {
        Some(6..) => Tier::Star6,
        Some(3..=5) => Tier::Star35,
        Some(1..=2) => Tier::Star12,
        _ if two_step => Tier::Secure,
        _ => Tier::Account,
    }
}

/// What to do with an owner's files against their limit.
#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Within it (a former "over" mark goes).
    Fine,
    /// Over it since `since`; the files stay until `since + GRACE`.
    Grace { since: u64 },
    /// The grace is over: free this many bytes, the oldest files first.
    Free(u64),
}

pub fn verdict(used: u64, limit: u64, over_since: Option<u64>, now: u64) -> Verdict {
    if used <= limit {
        return Verdict::Fine;
    }
    let since = over_since.unwrap_or(now);
    if now >= since + GRACE {
        Verdict::Free(used - limit)
    } else {
        Verdict::Grace { since }
    }
}

// ------------------------------------------------------------- live state

pub fn quotas(state: &AppState) -> Quotas {
    state.admin.read().ok().and_then(|c| c.quotas.clone()).unwrap_or_default()
}

/// Read the same signed entitlement everywhere: the star service owns freshness.
pub async fn tier_of_user(state: &AppState, username: &str) -> Tier {
    let two_step = state.directory.has_2fa(username);
    let star = match state.directory.uid_of(username) {
        Some(uid) => crate::stars::star_of(&uid).await.ok().flatten().map(|s| s.kind),
        None => None,
    };
    tier_from(true, two_step, star)
}

pub fn paid(tier: Tier) -> bool {
    matches!(tier, Tier::Star12 | Tier::Star35 | Tier::Star6)
}

/// Files went by another way (disk short, an age limit, "clear"): count again on next use.
pub fn invalidate() {
    let mut usage = USAGE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    usage.by_owner = None;
    usage.generation = usage.generation.wrapping_add(1);
}

/// The caller: (quota key, level). Guests share `GUESTS`.
pub async fn caller(state: &AppState, dh: &[u8; 32]) -> (String, Tier) {
    match state.directory.username_for_identity(dh) {
        Some(u) => {
            let t = tier_of_user(state, &u).await;
            (u, t)
        }
        None => (GUESTS.to_string(), Tier::Guest),
    }
}

/// The files limit of a quota key.
pub fn files_limit(q: &Quotas, key: &str, t: Tier) -> u64 {
    if key == GUESTS { q.guest_pool } else { q.of(t).files }
}

/// Bytes of chat files per owner, kept in step with uploads and rebuilt by
/// the hourly pass.
struct Usage {
    by_owner: Option<HashMap<String, u64>>,
    generation: u64,
}
static USAGE: Mutex<Usage> = Mutex::new(Usage { by_owner: None, generation: 0 });

pub fn used(state: &AppState, key: &str) -> u64 {
    usage_snapshot(state, key).0
}

pub fn usage_snapshot(state: &AppState, key: &str) -> (u64, u64) {
    let mut usage = USAGE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let used = usage.by_owner.get_or_insert_with(|| state.blobs.usage_by_owner()).get(key).copied().unwrap_or(0);
    (used, usage.generation)
}

pub fn add(key: &str, bytes: u64, generation: u64) {
    let mut usage = USAGE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if usage.generation == generation {
        if let Some(by_owner) = &mut usage.by_owner {
            let used = by_owner.entry(key.to_owned()).or_default();
            *used = used.saturating_add(bytes);
        }
    } else {
        // A rescan during the write may have already seen the new file.
        usage.by_owner = None;
        usage.generation = usage.generation.wrapping_add(1);
    }
}

pub fn lock_upload(owner: &str) -> std::sync::MutexGuard<'static, ()> {
    use std::hash::{DefaultHasher, Hash, Hasher};
    static LOCKS: [Mutex<()>; 64] = [const { Mutex::new(()) }; 64];
    let mut hash = DefaultHasher::new();
    owner.hash(&mut hash);
    LOCKS[hash.finish() as usize % LOCKS.len()].lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

const OVER_FILE: &str = "aqua-quota-over.json";

fn over_marks() -> HashMap<String, u64> {
    std::fs::read(OVER_FILE).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn over_since(key: &str) -> Option<u64> {
    over_marks().get(key).copied()
}

/// One pass over everyone's files: marks who went over, frees what's
/// past the grace. Run hourly and right after the admin saves limits.
pub async fn reconcile(state: &AppState) -> usize {
    let st = state.clone();
    let files = tokio::task::spawn_blocking(move || {
        crate::blob_upload::sweep(&st.blobs.upload_root());
        st.blobs.owned_files()
    }).await.unwrap_or_default();
    let mut by_owner: HashMap<String, Vec<(String, u64, std::time::SystemTime)>> = HashMap::new();
    for (owner, cid, len, at) in files {
        by_owner.entry(owner).or_default().push((cid, len, at));
    }
    let q = quotas(state);
    let now = (crate::now_ms() / 1000) as u64;
    let old = over_marks();
    let mut marks = HashMap::new();
    let mut removed = 0;
    for (owner, mut list) in by_owner {
        let tier = if owner == GUESTS { Tier::Guest } else { tier_of_user(state, &owner).await };
        let limit = files_limit(&q, &owner, tier);
        let total: u64 = list.iter().map(|f| f.1).sum();
        match verdict(total, limit, old.get(&owner).copied(), now) {
            Verdict::Fine => {}
            Verdict::Grace { since } => {
                marks.insert(owner.clone(), since);
            }
            Verdict::Free(mut need) => {
                list.sort_by_key(|f| f.2);
                for (cid, len, _) in &list {
                    if need == 0 {
                        break;
                    }
                    if state.blobs.remove_cached(cid) {
                        need = need.saturating_sub(*len);
                        removed += 1;
                    }
                }
                tracing::info!(owner = %owner, removed, "over the limit past the grace: the oldest files went");
            }
        }
    }
    // Counted afresh on next use (uploads during this pass aren't lost).
    invalidate();
    if marks != old {
        if let Ok(b) = serde_json::to_vec_pretty(&marks) {
            let _ = std::fs::write(OVER_FILE, b);
        }
    }
    removed
}

/// Started with the relay: the hourly pass.
pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(3600)).await;
            reconcile(&state).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_single_file_limits_match_tiers() {
        let q = Quotas::default();
        assert_eq!([q.guest.file_max, q.account.file_max, q.secure.file_max, q.star12.file_max, q.star35.file_max, q.star6.file_max],
            [50 * MB, 200 * MB, 512 * MB, GB, 1536 * MB, 2 * GB]);
    }

    #[test]
    fn levels() {
        assert_eq!(tier_from(false, true, Some(5)), Tier::Guest, "no account here: a guest, star or not");
        assert_eq!(tier_from(true, false, None), Tier::Account);
        assert_eq!(tier_from(true, true, None), Tier::Secure);
        assert_eq!(tier_from(true, false, Some(2)), Tier::Star12);
        assert_eq!(tier_from(true, false, Some(4)), Tier::Star35);
        assert_eq!(tier_from(true, false, Some(6)), Tier::Star6);
        assert_eq!(tier_from(true, true, Some(0)), Tier::Secure);
    }

    #[test]
    fn every_level_has_a_limit() {
        let q = Quotas::default();
        for t in [Tier::Guest, Tier::Account, Tier::Secure, Tier::Star12, Tier::Star35, Tier::Star6] {
            let l = q.of(t);
            assert!(l.files > 0 && l.files <= 20 * GB && l.file_max <= FILE_MAX_CAP && l.backup <= BACKUP_CAP);
        }
        // An admin can't take the limits off or past what the relay can take.
        let mut big = q.clone();
        big.star6.file_max = u64::MAX;
        big.star6.files = u64::MAX;
        let c = big.clean();
        assert_eq!(c.star6.file_max, FILE_MAX_CAP);
        assert!(c.star6.files < u64::MAX);
    }

    #[test]
    fn grace_then_free() {
        let t0 = 1_000_000;
        assert_eq!(verdict(10, 20, None, t0), Verdict::Fine);
        assert_eq!(verdict(10, 20, Some(t0), t0 + 99), Verdict::Fine, "back under: the mark goes");
        assert_eq!(verdict(30, 20, None, t0), Verdict::Grace { since: t0 });
        assert_eq!(verdict(30, 20, Some(t0), t0 + GRACE - 1), Verdict::Grace { since: t0 });
        assert_eq!(verdict(30, 20, Some(t0), t0 + GRACE), Verdict::Free(10));
    }
}
