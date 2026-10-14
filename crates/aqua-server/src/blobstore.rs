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
}

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
        let _ = std::fs::create_dir_all(&dir);
        Self { dir, ttl }
    }

    fn path(&self, cid: &str) -> Option<PathBuf> {
        valid_cid(cid).then(|| self.dir.join(cid.to_ascii_lowercase()))
    }

    pub fn put(&self, cid: &str, data: &[u8]) -> std::io::Result<()> {
        let Some(p) = self.path(cid) else { return Err(std::io::Error::other("bad cid")) };
        if p.exists() {
            // Same content; just refresh its age.
            let _ = std::fs::File::options().write(true).open(&p).and_then(|f| f.set_modified(SystemTime::now()));
            return Ok(());
        }
        let tmp = self.dir.join(format!(".{cid}.part"));
        std::fs::write(&tmp, data)?;
        std::fs::rename(tmp, p)
    }

    pub fn get(&self, cid: &str) -> Option<Vec<u8>> {
        std::fs::read(self.path(cid)?).ok()
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
                if let Ok(m) = e.metadata() {
                    n += 1;
                    bytes += m.len();
                }
            }
        }
        (n, bytes)
    }

    /// Remove blobs older than the TTL (and stale partial uploads).
    pub fn sweep(&self) -> usize {
        sweep_dir(&self.dir, self.ttl)
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

fn sweep_dir(dir: &Path, ttl: Duration) -> usize {
    let now = SystemTime::now();
    let mut removed = 0;
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let old = e.metadata().and_then(|m| m.modified()).ok()
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
        // Age it past the TTL.
        let f = std::fs::File::options().write(true).open(dir.path().join(&cid)).unwrap();
        f.set_modified(SystemTime::now() - Duration::from_secs(7200)).unwrap();
        assert_eq!(s.sweep(), 1);
        assert!(s.get(&cid).is_none());
    }
}
