//! Encrypted file bodies (blobs) on disk.
//!
//! Content-addressed by the SHA-256 of the ciphertext, one file per blob
//! under `aqua-blobs/` (`AQUA_BLOB_DIR`). Files stay — across stops,
//! restarts and updates (owner, 20.11: photos went "unavailable" after a
//! restart) — and go only when there's no room: the least recently used
//! first, when the disk has less than `AQUA_BLOB_MIN_FREE_GB` free (2 by
//! default) or the files take more than `AQUA_BLOB_MAX_GB` (no cap by
//! default). `AQUA_BLOB_TTL_HOURS` brings back an age limit if set.
//! Kept blobs (`keep/`), live stories (`stories/`) and people's data stay.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

#[derive(Clone)]
pub struct BlobStore {
    dir: PathBuf,
    /// Age limit of the delivery cache: none unless `AQUA_BLOB_TTL_HOURS`.
    ttl: Option<Duration>,
    /// Size cap of the delivery cache (`AQUA_BLOB_MAX_GB`).
    max_bytes: Option<u64>,
    /// Keep at least this much of the disk free (`AQUA_BLOB_MIN_FREE_GB`).
    min_free: u64,
    /// Long-lived blobs (group photos and banners): `<dir>/keep`, expiring
    /// only after `KEEP_TTL` without a single download.
    keep: PathBuf,
    /// Story files: `<dir>/stories`. A story lives up to 48 h, and a
    /// restart (or a self-update) of the relay must not take it away
    /// while it's still up — so they're not part of the delivery cache.
    stories: PathBuf,
}

/// Where a blob goes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shelf {
    /// The delivery cache (kept until the disk runs short).
    Cache,
    /// Long-lived, ≤ `KEEP_LIMIT` (`KEEP_TTL` past the last download).
    Keep,
    /// A story's file (`STORY_TTL`, kept across restarts).
    Story,
}

/// Kept blobs live this long past their last download.
pub const KEEP_TTL: Duration = Duration::from_secs(90 * 24 * 3600);
/// Only small files may be kept.
pub const KEEP_LIMIT: usize = 4 * 1024 * 1024;
/// Story files live this long since the last upload (the longest story is
/// 48 h; the author's app uploads it again when it finds it gone).
pub const STORY_TTL: Duration = Duration::from_secs(50 * 3600);
/// File kinds the admin panel tells apart.
pub const KINDS: [&str; 8] = ["photo", "video", "sticker", "gif", "emoji", "voice", "audio", "file"];

fn valid_cid(cid: &str) -> bool {
    cid.len() == 64 && cid.bytes().all(|b| b.is_ascii_hexdigit())
}

impl BlobStore {
    pub fn from_env() -> Self {
        let dir = std::env::var("AQUA_BLOB_DIR").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("aqua-blobs"));
        let num = |k: &str| std::env::var(k).ok().and_then(|v| v.trim().parse::<f64>().ok()).filter(|v| *v >= 0.0);
        let mut s = Self::new_unlimited(dir);
        s.ttl = num("AQUA_BLOB_TTL_HOURS").filter(|h| *h > 0.0).map(|h| Duration::from_secs((h * 3600.0) as u64));
        s.max_bytes = num("AQUA_BLOB_MAX_GB").filter(|g| *g > 0.0).map(|g| (g * GB as f64) as u64);
        s.min_free = num("AQUA_BLOB_MIN_FREE_GB").map(|g| (g * GB as f64) as u64).unwrap_or(2 * GB);
        s
    }

    /// With an age limit on the delivery cache (tests).
    pub fn new(dir: PathBuf, ttl: Duration) -> Self {
        let mut s = Self::new_unlimited(dir);
        s.ttl = Some(ttl);
        s
    }

    fn new_unlimited(dir: PathBuf) -> Self {
        let keep = dir.join("keep");
        let _ = std::fs::create_dir_all(&keep);
        let stories = dir.join("stories");
        let _ = std::fs::create_dir_all(&stories);
        Self { dir, ttl: None, max_bytes: None, min_free: 0, keep, stories }
    }

    fn shelf_dir(&self, shelf: Shelf) -> &Path {
        match shelf {
            Shelf::Cache => &self.dir,
            Shelf::Keep => &self.keep,
            Shelf::Story => &self.stories,
        }
    }

    fn path(&self, cid: &str) -> Option<PathBuf> {
        valid_cid(cid).then(|| self.dir.join(cid.to_ascii_lowercase()))
    }

    pub fn put(&self, cid: &str, data: &[u8]) -> std::io::Result<()> {
        // Short of room: make some first (checked at most once a minute).
        static LAST: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        if now.saturating_sub(LAST.swap(now, std::sync::atomic::Ordering::Relaxed)) >= 60 {
            let n = self.fit(data.len() as u64);
            if n > 0 {
                tracing::info!(removed = n, "low on room: the oldest files went");
            }
        }
        self.put_in(&self.dir, cid, data)
    }

    /// Remember what kind of file a blob is (for the admin panel): a
    /// `.k.<cid>` file next to it, swept together with it.
    pub fn set_kind(&self, cid: &str, shelf: Shelf, kind: &str) {
        if !valid_cid(cid) || !KINDS.contains(&kind) { return; }
        let dir = self.shelf_dir(shelf);
        let _ = std::fs::write(dir.join(format!(".k.{}", cid.to_ascii_lowercase())), kind);
    }

    /// (kind, count, bytes) over both stores; unlabelled blobs (uploaded by
    /// older apps) count as "unknown".
    pub fn stats_by_kind(&self) -> Vec<(String, usize, u64)> {
        let mut m: std::collections::BTreeMap<String, (usize, u64)> = Default::default();
        for dir in [&self.dir, &self.keep, &self.stories] {
            let Ok(rd) = std::fs::read_dir(dir) else { continue };
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with('.') { continue; }
                let Some(meta) = e.metadata().ok().filter(|m| m.is_file()) else { continue };
                let kind = std::fs::read_to_string(dir.join(format!(".k.{name}"))).ok()
                    .filter(|k| KINDS.contains(&k.as_str())).unwrap_or_else(|| "unknown".into());
                let x = m.entry(kind).or_default();
                x.0 += 1;
                x.1 += meta.len();
            }
        }
        let mut v: Vec<_> = m.into_iter().map(|(k, (n, b))| (k, n, b)).collect();
        v.sort_by(|a, b| b.2.cmp(&a.2));
        v
    }

    /// Store a blob that should outlive the delivery cache (≤ `KEEP_LIMIT`).
    pub fn put_keep(&self, cid: &str, data: &[u8]) -> std::io::Result<()> {
        if data.len() > KEEP_LIMIT {
            return Err(std::io::Error::other("too large to keep"));
        }
        self.put_in(&self.keep, cid, data)
    }

    /// Store a story's file (see `stories`).
    pub fn put_story(&self, cid: &str, data: &[u8]) -> std::io::Result<()> {
        self.put_in(&self.stories, cid, data)
    }

    /// Is it here (any shelf)? Without reading it.
    pub fn has(&self, cid: &str) -> bool {
        valid_cid(cid) && [&self.dir, &self.keep, &self.stories].iter().any(|d| d.join(cid.to_ascii_lowercase()).is_file())
    }

    fn put_in(&self, dir: &Path, cid: &str, data: &[u8]) -> std::io::Result<()> {
        if !valid_cid(cid) {
            return Err(std::io::Error::other("bad cid"));
        }
        let p = dir.join(cid.to_ascii_lowercase());
        if p.exists() {
            // Same content; just refresh its age.
            touch(&p);
            touch(&dir.join(format!(".k.{}", cid.to_ascii_lowercase())));
            return Ok(());
        }
        let tmp = dir.join(format!(".{cid}.part"));
        std::fs::write(&tmp, data)?;
        std::fs::rename(tmp, p)
    }

    pub fn get(&self, cid: &str) -> Option<Vec<u8>> {
        if let Some(p) = self.path(cid) {
            if let Ok(b) = std::fs::read(&p) {
                // Last used: when room runs short, what nobody fetches lately goes first.
                touch(&p);
                return Some(b);
            }
        }
        if !valid_cid(cid) {
            return None;
        }
        let p = self.keep.join(cid.to_ascii_lowercase());
        if let Ok(b) = std::fs::read(&p) {
            // Sliding expiry: every download keeps it alive.
            touch(&p);
            return Some(b);
        }
        std::fs::read(self.stories.join(cid.to_ascii_lowercase())).ok()
    }

    /// (count, total bytes)
    pub fn stats(&self) -> (usize, u64) {
        let mut n = 0;
        let mut bytes = 0;
        if let Ok(rd) = std::fs::read_dir(&self.dir) {
            for e in rd.flatten() {
                if e.file_name().to_string_lossy().starts_with('.') {
                    continue;
                }
                if let Some(m) = e.metadata().ok().filter(|m| m.is_file()) {
                    n += 1;
                    bytes += m.len();
                }
            }
        }
        (n, bytes)
    }

    /// Remove what has run its time: the cache only with an age limit set,
    /// stale partial uploads always; then make room if the disk is short.
    pub fn sweep(&self) -> usize {
        let cache = match self.ttl {
            Some(ttl) => sweep_dir(&self.dir, ttl),
            None => sweep_parts(&self.dir),
        };
        cache + sweep_dir(&self.keep, KEEP_TTL) + sweep_dir(&self.stories, STORY_TTL) + self.fit(0)
    }

    /// Room for `incoming` more bytes: the least recently used files of the
    /// delivery cache go until it's under `AQUA_BLOB_MAX_GB` and the disk has
    /// `AQUA_BLOB_MIN_FREE_GB` free. Nothing goes while there's room.
    pub fn fit(&self, incoming: u64) -> usize {
        let free = if self.min_free > 0 { crate::host::disk_free(&self.dir) } else { None };
        self.fit_with(incoming, free)
    }

    fn fit_with(&self, incoming: u64, free: Option<u64>) -> usize {
        let Ok(rd) = std::fs::read_dir(&self.dir) else { return 0 };
        let mut files: Vec<(PathBuf, u64, SystemTime)> = rd
            .flatten()
            .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
            .filter_map(|e| {
                let m = e.metadata().ok().filter(|m| m.is_file())?;
                Some((e.path(), m.len(), m.modified().unwrap_or(SystemTime::UNIX_EPOCH)))
            })
            .collect();
        let mut total: u64 = files.iter().map(|f| f.1).sum::<u64>() + incoming;
        let mut freed = 0u64;
        let short = |total: u64, freed: u64| {
            self.max_bytes.is_some_and(|m| total > m)
                || free.is_some_and(|f| (f + freed).saturating_sub(incoming) < self.min_free)
        };
        if !short(total, freed) {
            return 0;
        }
        files.sort_by_key(|f| f.2);
        let mut removed = 0;
        for (p, len, _) in files {
            if !short(total, freed) {
                break;
            }
            if std::fs::remove_file(&p).is_ok() {
                if let Some(name) = p.file_name() {
                    let _ = std::fs::remove_file(self.dir.join(format!(".k.{}", name.to_string_lossy())));
                }
                total = total.saturating_sub(len);
                freed += len;
                removed += 1;
            }
        }
        removed
    }

    /// Every file of the delivery cache — only when asked ("Clear the
    /// server"). A stop, a restart or an update no longer does it (owner,
    /// 20.11: photos must not go "unavailable" after a restart).
    pub fn clear_cache(&self) -> usize {
        let mut removed = 0;
        if let Ok(rd) = std::fs::read_dir(&self.dir) {
            for e in rd.flatten() {
                if e.metadata().is_ok_and(|m| m.is_file()) && std::fs::remove_file(e.path()).is_ok() {
                    removed += 1;
                }
            }
        }
        removed
    }

    /// "Clear the server": every file, the long-lived ones too.
    pub fn clear_all(&self) -> usize {
        let mut removed = self.clear_cache();
        for dir in [&self.keep, &self.stories] {
            if let Ok(rd) = std::fs::read_dir(dir) {
                for e in rd.flatten() {
                    if e.metadata().is_ok_and(|m| m.is_file()) && std::fs::remove_file(e.path()).is_ok() {
                        removed += 1;
                    }
                }
            }
        }
        removed
    }

    /// Hourly sweep in the background.
    pub fn spawn_sweeper(&self) {
        let me = self.clone();
        tokio::spawn(async move {
            loop {
                let removed = tokio::task::spawn_blocking({ let me = me.clone(); move || me.sweep() }).await.unwrap_or(0);
                if removed > 0 {
                    tracing::info!(removed, "expired blobs swept");
                }
                tokio::time::sleep(Duration::from_secs(3600)).await;
            }
        });
    }
}

fn touch(p: &Path) {
    let _ = std::fs::File::options().write(true).open(p).and_then(|f| f.set_modified(SystemTime::now()));
}

const GB: u64 = 1024 * 1024 * 1024;

/// Partial uploads left by a broken connection (a day old).
fn sweep_parts(dir: &Path) -> usize {
    let now = SystemTime::now();
    let mut removed = 0;
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !(name.starts_with('.') && name.ends_with(".part")) {
                continue;
            }
            let old = e.metadata().ok().and_then(|m| m.modified().ok()).and_then(|t| now.duration_since(t).ok()).is_some_and(|a| a > Duration::from_secs(86_400));
            if old && std::fs::remove_file(e.path()).is_ok() {
                removed += 1;
            }
        }
    }
    removed
}

fn sweep_dir(dir: &Path, ttl: Duration) -> usize {
    let now = SystemTime::now();
    let mut removed = 0;
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let old = e.metadata().ok().filter(|m| m.is_file()).and_then(|m| m.modified().ok())
                .and_then(|t| now.duration_since(t).ok())
                .is_some_and(|age| age > ttl);
            if old && std::fs::remove_file(e.path()).is_ok() {
                removed += 1;
            }
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shutdown_clears_the_cache_but_keeps_kept_blobs() {
        let dir = tempfile::tempdir().unwrap();
        let s = BlobStore::new(dir.path().to_path_buf(), Duration::from_secs(3600));
        std::fs::write(dir.path().join("a".repeat(64)), b"x").unwrap();
        std::fs::write(dir.path().join("keep").join("b".repeat(64)), b"y").unwrap();
        s.put_story(&"c".repeat(64), b"z").unwrap();
        assert_eq!(s.clear_cache(), 1);
        assert!(dir.path().join("keep").join("b".repeat(64)).exists());
        // A live story survives a restart, and is still served.
        assert_eq!(s.get(&"c".repeat(64)).unwrap(), b"z");
        assert!(s.has(&"c".repeat(64)) && !s.has(&"a".repeat(64)));
        assert_eq!(s.clear_all(), 2);
    }
    #[test]
    fn files_stay_until_room_runs_short() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = BlobStore::new_unlimited(dir.path().to_path_buf());
        let (a, b, c) = ("a".repeat(64), "b".repeat(64), "c".repeat(64));
        for (cid, age) in [(&a, 300), (&b, 200), (&c, 100)] {
            s.put_in(&s.dir.clone(), cid, &[0u8; 100]).unwrap();
            let f = std::fs::File::options().write(true).open(dir.path().join(cid)).unwrap();
            f.set_modified(SystemTime::now() - Duration::from_secs(age * 3600)).unwrap();
        }
        // No age limit: a year-old file isn't swept.
        assert_eq!(s.sweep(), 0);
        // Plenty of room: nothing goes.
        assert_eq!(s.fit_with(0, Some(10 * GB)), 0);
        // A cap of 250 bytes: the oldest (a) goes, then the next — and only as many as needed.
        s.max_bytes = Some(250);
        assert_eq!(s.fit_with(0, None), 1);
        assert!(!s.has(&a) && s.has(&b) && s.has(&c));
        // A fetch makes a file "used": the other one goes first next time.
        s.get(&b).unwrap();
        s.max_bytes = Some(150);
        assert_eq!(s.fit_with(0, None), 1);
        assert!(s.has(&b) && !s.has(&c));
        // Disk short: below the free-space floor, files go until it's back.
        s.max_bytes = None;
        s.min_free = 1000;
        assert_eq!(s.fit_with(0, Some(950)), 1);
        assert!(!s.has(&b));
    }

    #[test]
    fn put_get_sweep() {
        let dir = tempfile::tempdir().unwrap();
        let s = BlobStore::new(dir.path().to_path_buf(), Duration::from_secs(3600));
        let cid = "a".repeat(64);
        s.put(&cid, b"cipher").unwrap();
        assert_eq!(s.get(&cid).unwrap(), b"cipher");
        assert!(s.get("../etc/passwd").is_none());
        assert_eq!(s.stats(), (1, 6));
        s.set_kind(&cid, Shelf::Cache, "video");
        s.set_kind(&cid, Shelf::Cache, "../x");
        assert_eq!(s.stats(), (1, 6));
        assert_eq!(s.stats_by_kind(), vec![("video".to_string(), 1, 6)]);
        // Age it past the TTL.
        let f = std::fs::File::options().write(true).open(dir.path().join(&cid)).unwrap();
        f.set_modified(SystemTime::now() - Duration::from_secs(7200)).unwrap();
        assert_eq!(s.sweep(), 1);
        assert!(s.get(&cid).is_none());
        // Kept blobs survive the cache TTL.
        s.put_keep(&cid, b"photo").unwrap();
        let f = std::fs::File::options().write(true).open(dir.path().join("keep").join(&cid)).unwrap();
        f.set_modified(SystemTime::now() - Duration::from_secs(7200)).unwrap();
        assert_eq!(s.sweep(), 0);
        assert_eq!(s.get(&cid).unwrap(), b"photo");
        assert!(s.put_keep(&"b".repeat(64), &vec![0; KEEP_LIMIT + 1]).is_err());
    }
}
