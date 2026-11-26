//! A signed federation request is taken once (AQUA-SEC-009). The signatures
//! of the clock window are kept in memory and appended to a file, so neither
//! a flood nor a restart lets one be sent again. Whenever that can't be
//! promised, the request is refused.

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// How many signatures one clock window may hold.
const MAX: usize = 200_000;

pub struct Replay {
    inner: Mutex<Inner>,
}

struct Inner {
    /// Signature → the time it was signed with.
    seen: HashMap<String, i64>,
    path: Option<PathBuf>,
    file: Option<std::fs::File>,
    /// Lines in the file (expired ones too): when to write it anew.
    lines: usize,
    window: i64,
    max: usize,
    /// Everything is refused until then: the file was there and unreadable,
    /// so we don't know what has been taken.
    closed_until: i64,
}

impl Replay {
    /// The table kept in `path` (`None`: memory only). One per file for the
    /// whole process, so reloading the config keeps it.
    pub fn shared(path: Option<PathBuf>, window: i64, now: i64) -> Arc<Self> {
        static ALL: Mutex<Option<HashMap<Option<PathBuf>, Arc<Replay>>>> = Mutex::new(None);
        let mut all = ALL.lock().unwrap_or_else(|e| e.into_inner());
        all.get_or_insert_with(HashMap::new)
            .entry(path.clone())
            .or_insert_with(|| Arc::new(Self::open(path, window, MAX, now)))
            .clone()
    }

    /// A table of its own with what `path` still holds.
    fn open(path: Option<PathBuf>, window: i64, max: usize, now: i64) -> Self {
        let mut seen = HashMap::new();
        let mut closed_until = 0;
        let mut found = false;
        if let Some(p) = &path {
            match std::fs::read(p) {
                Ok(bytes) => {
                    found = true;
                    for line in String::from_utf8_lossy(&bytes).lines() {
                        let Some((ts, sig)) = line.split_once(' ') else { continue };
                        let Ok(ts) = ts.parse::<i64>() else { continue };
                        if (now - ts).abs() <= window {
                            seen.insert(sig.to_string(), ts);
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    tracing::error!(error = %e, "federation: the list of used signatures can't be read — refusing requests for ten minutes");
                    // A signature stays good from `window` before its time to `window` after.
                    closed_until = now + 2 * window;
                }
            }
        }
        let mut inner = Inner { seen, path, file: None, lines: 0, window, max, closed_until };
        // Drops what expired. If it can't (Windows: the process being
        // replaced still holds the file), new lines are added to it.
        if found {
            if let Err(e) = inner.rewrite() {
                tracing::warn!(error = %e, "federation: the list of used signatures wasn't compacted");
            }
        }
        Self { inner: Mutex::new(inner) }
    }

    /// `true` the first time `sig` is seen, and only once it is on record.
    pub fn take(&self, sig: &str, ts: i64, now: i64) -> bool {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if now < g.closed_until || !g.remember(sig, ts, now) {
            return false;
        }
        if let Err(e) = g.record(sig, ts) {
            tracing::error!(error = %e, "federation: a used signature can't be recorded — request refused");
            g.file = None;
            return false;
        }
        true
    }
}

impl Inner {
    fn remember(&mut self, sig: &str, ts: i64, now: i64) -> bool {
        let window = self.window;
        if self.seen.len() > self.max / 10 {
            self.seen.retain(|_, t| (now - *t).abs() <= window);
        }
        // Full of live signatures (only a relay with a key or the secret can
        // make that many): refuse new ones until old ones expire. Forgetting
        // them, as before 1.21.0-24, let every one of them be sent again.
        if self.seen.len() >= self.max && !self.seen.contains_key(sig) {
            tracing::warn!("federation: too many signed requests in one clock window — refusing");
            return false;
        }
        self.seen.insert(sig.to_string(), ts).is_none()
    }

    fn record(&mut self, sig: &str, ts: i64) -> std::io::Result<()> {
        let Some(path) = self.path.clone() else {
            return Ok(());
        };
        if self.lines > 2 * self.seen.len() + 10_000 {
            // `sig` is in `seen` already, so it's written with the rest.
            if self.rewrite().is_ok() {
                return Ok(());
            }
            self.lines = self.seen.len(); // try again later
        }
        if self.file.is_none() {
            let mut f = append(&path)?;
            // The last line may have been cut short by a crash.
            f.write_all(b"\n")?;
            self.file = Some(f);
        }
        if let Some(f) = &mut self.file {
            f.write_all(format!("{ts} {sig}\n").as_bytes())?;
            self.lines += 1;
        }
        Ok(())
    }

    /// Replace the file with the signatures still held.
    fn rewrite(&mut self) -> std::io::Result<()> {
        let Some(path) = self.path.clone() else {
            return Ok(());
        };
        self.file = None;
        let text: String = self
            .seen
            .iter()
            .map(|(sig, ts)| format!("{ts} {sig}\n"))
            .collect();
        let tmp = path.with_extension("tmp");
        crate::fed::write_private(&tmp, &text)?;
        std::fs::rename(&tmp, &path)?;
        self.lines = self.seen.len();
        Ok(())
    }
}

fn append(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    let mut o = std::fs::OpenOptions::new();
    o.append(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o.open(path)
}

#[cfg(test)]
mod tests {
    use super::Replay;

    const WINDOW: i64 = 300_000;
    const NOW: i64 = 1_000_000_000;

    #[test]
    fn a_signature_is_taken_once() {
        let r = Replay::open(None, WINDOW, 100, NOW);
        assert!(r.take("a", NOW, NOW));
        assert!(!r.take("a", NOW, NOW), "replayed");
        assert!(r.take("b", NOW, NOW));
    }

    /// A full table refuses new signatures and keeps the old ones, so none
    /// of them can be replayed; expired ones make room again.
    #[test]
    fn a_full_table_stays_closed() {
        let r = Replay::open(None, WINDOW, 20, NOW);
        for i in 0..20 {
            assert!(r.take(&format!("s{i}"), NOW, NOW));
        }
        assert!(!r.take("one-more", NOW, NOW));
        assert!(!r.take("s0", NOW, NOW), "still remembered");
        let later = NOW + WINDOW + 1;
        assert!(r.take("one-more", later, later));
    }

    #[test]
    fn a_restart_does_not_forget() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("seen.log");
        let first = Replay::open(Some(file.clone()), WINDOW, 100, NOW);
        assert!(first.take("v2:node:nonce", NOW, NOW));
        assert!(first.take("old", NOW - WINDOW, NOW));
        drop(first);
        let again = Replay::open(Some(file.clone()), WINDOW, 100, NOW + 1);
        assert!(!again.take("v2:node:nonce", NOW, NOW + 1), "replayed after a restart");
        assert!(again.take("new", NOW + 1, NOW + 1));
        // What expired isn't carried on.
        assert!(!std::fs::read_to_string(&file).unwrap().contains("old"));
    }

    #[test]
    fn a_line_cut_short_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("seen.log");
        std::fs::write(&file, format!("{NOW} whole\n{NOW}")).unwrap();
        let r = Replay::open(Some(file.clone()), WINDOW, 100, NOW);
        assert!(!r.take("whole", NOW, NOW));
        assert!(r.take("next", NOW, NOW));
        drop(r);
        assert!(!Replay::open(Some(file), WINDOW, 100, NOW).take("next", NOW, NOW));
    }

    #[test]
    fn one_of_many_at_once_gets_through() {
        let dir = tempfile::tempdir().unwrap();
        let r = std::sync::Arc::new(Replay::open(Some(dir.path().join("seen.log")), WINDOW, 1000, NOW));
        let threads: Vec<_> = (0..32)
            .map(|_| {
                let r = r.clone();
                std::thread::spawn(move || (0..50).filter(|i| r.take(&format!("s{i}"), NOW, NOW)).count())
            })
            .collect();
        let taken: usize = threads.into_iter().map(|t| t.join().unwrap()).sum();
        assert_eq!(taken, 50);
    }

    #[test]
    fn nothing_is_accepted_that_cannot_be_recorded() {
        let dir = tempfile::tempdir().unwrap();
        let nowhere = Replay::open(Some(dir.path().join("missing").join("seen.log")), WINDOW, 100, NOW);
        assert!(!nowhere.take("a", NOW, NOW));
        // There, but unreadable (a folder in its place): closed for a while.
        let unreadable = Replay::open(Some(dir.path().to_path_buf()), WINDOW, 100, NOW);
        assert!(!unreadable.take("a", NOW, NOW));
    }
}
