//! Encrypted file bodies (blobs) on disk.
//!
//! Content-addressed by the SHA-256 of the ciphertext, one file per blob
//! under `aqua-blobs/` (`AQUA_BLOB_DIR`). They are a delivery cache, not an
//! archive: anything older than `AQUA_BLOB_TTL_HOURS` (72 by default) is
//! swept hourly. Survives restarts, unlike the in-memory map it replaces.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

#[derive(Clone)]
pub struct BlobStore {
    dir: PathBuf,
    ttl: Duration,
    /// Long-lived blobs (group photos and banners): `<dir>/keep`, expiring
    /// only after `KEEP_TTL` without a single download.
    keep: PathBuf,
}

/// Kept blobs live this long past their last download.
pub const KEEP_TTL: Duration = Duration::from_secs(90 * 24 * 3600);
/// Only small files may be kept.
pub const KEEP_LIMIT: usize = 4 * 1024 * 1024;
/// File kinds the admin panel tells apart.
pub const KINDS: [&str; 8] = ["photo", "video", "sticker", "gif", "emoji", "voice", "audio", "file"];

fn valid_cid(cid: &str) -> bool {
    cid.len() == 64 && cid.bytes().all(|b| b.is_ascii_hexdigit())
}

impl BlobStore {
    pub fn from_env() -> Self {
        let dir = std::env::var("AQUA_BLOB_DIR").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("aqua-blobs"));
        let hours = std::env::var("AQUA_BLOB_TTL_HOURS").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(72);
        Self::new(dir, Duration::from_secs(hours.max(1) * 3600))
    }

    pub fn new(dir: PathBuf, ttl: Duration) -> Self {
        let keep = dir.join("keep");
        let _ = std::fs::create_dir_all(&keep);
        Self { dir, ttl, keep }
    }

    fn path(&self, cid: &str) -> Option<PathBuf> {
        valid_cid(cid).then(|| self.dir.join(cid.to_ascii_lowercase()))
    }

    pub fn put(&self, cid: &str, data: &[u8]) -> std::io::Result<()> {
        self.put_in(&self.dir, cid, data)
    }

    /// Remember what kind of file a blob is (for the admin panel): a
    /// `.k.<cid>` file next to it, swept together with it.
    pub fn set_kind(&self, cid: &str, keep: bool, kind: &str) {
        if !valid_cid(cid) || !KINDS.contains(&kind) { return; }
        let dir = if keep { &self.keep } else { &self.dir };
        let _ = std::fs::write(dir.join(format!(".k.{}", cid.to_ascii_lowercase())), kind);
    }

    /// (kind, count, bytes) over both stores; unlabelled blobs (uploaded by
    /// older apps) count as "unknown".
    pub fn stats_by_kind(&self) -> Vec<(String, usize, u64)> {
        let mut m: std::collections::BTreeMap<String, (usize, u64)> = Default::default();
        for dir in [&self.dir, &self.keep] {
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
        if let Some(b) = self.path(cid).and_then(|p| std::fs::read(p).ok()) {
            return Some(b);
        }
        if !valid_cid(cid) {
            return None;
        }
        let p = self.keep.join(cid.to_ascii_lowercase());
        let b = std::fs::read(&p).ok()?;
        // Sliding expiry: every download keeps it alive.
        touch(&p);
        Some(b)
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

    /// Remove blobs older than the TTL (and stale partial uploads).
    pub fn sweep(&self) -> usize {
        sweep_dir(&self.dir, self.ttl) + sweep_dir(&self.keep, KEEP_TTL)
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
    fn put_get_sweep() {
        let dir = tempfile::tempdir().unwrap();
        let s = BlobStore::new(dir.path().to_path_buf(), Duration::from_secs(3600));
        let cid = "a".repeat(64);
        s.put(&cid, b"cipher").unwrap();
        assert_eq!(s.get(&cid).unwrap(), b"cipher");
        assert!(s.get("../etc/passwd").is_none());
        assert_eq!(s.stats(), (1, 6));
        s.set_kind(&cid, false, "video");
        s.set_kind(&cid, false, "../x");
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
