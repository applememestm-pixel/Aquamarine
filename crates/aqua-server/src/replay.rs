//! A signed federation request is taken once (AQUA-SEC-009). The signatures
//! of the clock window are kept in memory and appended to a file, so neither
//! a flood nor a restart lets one be sent again. Whenever that can't be
//! promised, the request is refused.
//!
//! 1.21.0-25 (A5):
//! * a quota per sender (a node id, or "secret" for everyone with only the
//!   shared secret): one flooding sender fills its own share, not the table
//!   the others need;
//! * expired signatures leave by time order (amortised), not by a scan of
//!   the whole table on every request once it held a tenth of its size;
//! * the file is synced to disk once a second and at a clean stop, which
//!   leaves a mark; a start without that mark (a crash, a power cut, an
//!   older build) takes no signature made before it — those may be among
//!   the last ones not yet on disk;
//! * (review of #113, 27.11) the floor only rises: every entry dropped as
//!   expired raises it past its stamp, and it is kept in the file — a clock
//!   that ran ahead, dropped an entry and came back (with or without a
//!   restart meanwhile) can't make that signature good again;
//! * after a crash the floor is the start plus a clock window: a signature
//!   stamped ahead of our clock may have been taken and lost. Entries
//!   stamped further ahead than that (passes, kept until they expire) are
//!   synced before they are taken. A damaged file: the start plus `AHEAD`;
//! * a clean mark that can't be taken away (it would say "clean" after a
//!   crash) closes the table until it is gone;
//! * one process at a time: a lock next to the file. A second process (the
//!   new one while Windows replaces the old during an update) refuses until
//!   the first is gone, then reads what it recorded.

use std::collections::{BTreeSet, HashMap};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// How many signatures one clock window may hold in all.
const MAX: usize = 400_000;
/// …and from one sender.
const MAX_PER: usize = 50_000;
/// How often appended lines are synced to disk.
const SYNC_MS: i64 = 1000;
/// How far ahead of our clock an entry may be stamped: a pass is kept until
/// it expires (`PASS_SECS` + a minute, lib.rs), a signature ±`SKEW_MS`.
pub(crate) const AHEAD: i64 = 15 * 60 * 1000;
/// The line holding the floor (`Inner::floor`) in the file.
pub(crate) const FLOOR_SIG: &str = "floor:";

pub struct Replay {
    inner: Mutex<Inner>,
}

struct Inner {
    /// Signature → (the time it was signed with, its sender).
    seen: HashMap<String, (i64, String)>,
    /// The same, by time: expired ones are taken from the front.
    by_time: BTreeSet<(i64, String)>,
    /// Signatures held per sender.
    per: HashMap<String, usize>,
    path: Option<PathBuf>,
    file: Option<std::fs::File>,
    /// Lines in the file (expired ones too): when to write it anew.
    lines: usize,
    window: i64,
    max: usize,
    max_per: usize,
    /// Everything is refused until then: the file was there and unreadable,
    /// so we don't know what has been taken.
    closed_until: i64,
    /// Signatures made before this (ms) are refused: the last stop wasn't
    /// clean, and what was taken just before it may not be on disk.
    floor: i64,
    /// A clean mark is on disk while we take signatures (it couldn't be
    /// removed, or this table was stopped cleanly and is used again):
    /// nothing is taken until it is gone.
    mark_left: bool,
    /// The lock file held while this process owns the table.
    lock: Option<std::fs::File>,
    /// Another process held the lock at start: read the file again once ours.
    reload: bool,
    last_sync: i64,
    unsynced: bool,
    /// Entries looked at while dropping expired ones (tests: amortised).
    work: u64,
}

fn all() -> &'static Mutex<HashMap<Option<PathBuf>, Arc<Replay>>> {
    static ALL: std::sync::OnceLock<Mutex<HashMap<Option<PathBuf>, Arc<Replay>>>> = std::sync::OnceLock::new();
    ALL.get_or_init(Default::default)
}

fn mark_of(p: &std::path::Path) -> PathBuf {
    p.with_extension("clean")
}
fn lock_of(p: &std::path::Path) -> PathBuf {
    p.with_extension("lock")
}

impl Replay {
    /// The table kept in `path` (`None`: memory only). One per file for the
    /// whole process, so reloading the config keeps it.
    pub fn shared(path: Option<PathBuf>, window: i64, now: i64) -> Arc<Self> {
        let mut all = all().lock().unwrap_or_else(|e| e.into_inner());
        all.entry(path.clone())
            .or_insert_with(|| Arc::new(Self::open(path, window, MAX, MAX_PER, now)))
            .clone()
    }

    /// At a clean stop of the relay: every table synced, and marked clean.
    pub fn shutdown_all() {
        let all = all().lock().unwrap_or_else(|e| e.into_inner());
        for r in all.values() {
            r.close_clean();
        }
    }

    /// Sync and leave the clean mark (and the lock).
    pub fn close_clean(&self) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let Some(path) = g.path.clone() else { return };
        if g.lock.is_none() {
            return;
        }
        let synced = match &g.file {
            Some(f) => f.sync_all().is_ok(),
            None => true,
        };
        if synced && crate::fed::write_private(&mark_of(&path), "clean\n").is_ok() {
            g.file = None;
            g.mark_left = true;
            if let Some(l) = g.lock.take() {
                let _ = l.unlock();
            }
        }
    }

    /// A table of its own with what `path` still holds.
    fn open(path: Option<PathBuf>, window: i64, max: usize, max_per: usize, now: i64) -> Self {
        let mut inner = Inner {
            seen: HashMap::new(), by_time: BTreeSet::new(), per: HashMap::new(), path, file: None, lines: 0, window, max, max_per,
            closed_until: 0, floor: 0, mark_left: false, lock: None, reload: false, last_sync: now, unsynced: false, work: 0,
        };
        if inner.path.is_some() {
            if inner.acquire() {
                inner.load(now);
            } else {
                tracing::warn!("federation: another process holds the list of used signatures — refusing signed requests until it stops");
                inner.reload = true;
            }
        }
        Self { inner: Mutex::new(inner) }
    }

    /// `true` the first time `sig` (from `sender`) is seen, and only once it
    /// is on record.
    pub fn take(&self, sender: &str, sig: &str, ts: i64, now: i64) -> bool {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if g.path.is_some() && g.lock.is_none() {
            // Another process had it: ours now? Then read what it recorded.
            if !g.acquire() {
                return false;
            }
            if std::mem::take(&mut g.reload) {
                g.load(now);
            }
        }
        if g.mark_left {
            match std::fs::remove_file(mark_of(g.path.as_deref().unwrap_or(std::path::Path::new("")))) {
                Ok(()) => g.mark_left = false,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => g.mark_left = false,
                Err(e) => {
                    tracing::error!(error = %e, "federation: the clean-stop mark of the used signatures can't be removed — signed requests refused until it is");
                    return false;
                }
            }
        }
        if sig == FLOOR_SIG || ts > now + AHEAD {
            return false;
        }
        if now < g.closed_until || ts < g.floor || !g.remember(sender, sig, ts, now) {
            return false;
        }
        if let Err(e) = g.record(sender, sig, ts, now) {
            tracing::error!(error = %e, "federation: a used signature can't be recorded — request refused");
            g.file = None;
            g.forget(sig);
            return false;
        }
        true
    }
}

impl Inner {
    /// Take the lock next to the file: true when this process holds it.
    fn acquire(&mut self) -> bool {
        if self.lock.is_some() {
            return true;
        }
        let Some(path) = self.path.clone() else { return true };
        let Ok(f) = append(&lock_of(&path)) else { return false };
        match f.try_lock() {
            Ok(()) => { self.lock = Some(f); true }
            Err(_) => false,
        }
    }

    /// Read the file: what's still in the window, and whether the last stop
    /// was clean.
    fn load(&mut self, now: i64) {
        let Some(path) = self.path.clone() else { return };
        let mark = mark_of(&path);
        let clean = mark.exists();
        let mut found = false;
        match std::fs::read(&path) {
            Ok(bytes) => {
                found = true;
                let text = String::from_utf8_lossy(&bytes);
                let lines: Vec<&str> = text.lines().collect();
                let mut damaged = 0usize;
                for (i, line) in lines.iter().enumerate() {
                    if line.is_empty() { continue; }
                    // A line that doesn't read is damage (a disk error, a
                    // half-written block) — except the very last one, which a
                    // crash may have cut short. Whatever it held may be lost.
                    let ok_sig = |s: &str| !s.is_empty() && s.len() <= 300 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b':' || b == b'-' || b == b'_');
                    let mut it = line.splitn(3, ' ');
                    let parsed = match (it.next(), it.next()) {
                        (Some(ts), Some(sig)) if ok_sig(sig) => ts.parse::<i64>().ok().map(|ts| (ts, sig)),
                        _ => None,
                    };
                    let Some((ts, sig)) = parsed else {
                        if i + 1 < lines.len() { damaged += 1; }
                        continue;
                    };
                    if sig == FLOOR_SIG {
                        self.floor = self.floor.max(ts);
                        continue;
                    }
                    // Lines of 1.21.0-24 name no sender.
                    let sender = it.next().unwrap_or("?").to_string();
                    // Not expired yet. Entries stamped ahead (a pass's nonce
                    // is kept until the pass expires) are kept too. One that
                    // expired raises the floor: it is never good again.
                    if now - ts <= self.window {
                        self.insert(sender, sig.to_string(), ts);
                    } else {
                        self.floor = self.floor.max(ts + 1);
                    }
                }
                if damaged > 0 {
                    tracing::error!(damaged, "federation: the list of used signatures is damaged — signed requests are refused for a quarter of an hour");
                    // Whatever the line held was stamped at most `AHEAD` on.
                    self.floor = self.floor.max(now + AHEAD + 1);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                tracing::error!(error = %e, "federation: the list of used signatures can't be read — refusing requests for ten minutes");
                // A signature stays good from `window` before its time to `window` after.
                self.closed_until = now + 2 * self.window;
            }
        }
        if found && !clean {
            tracing::warn!("federation: the last stop wasn't clean — signed requests are refused for one clock window");
            // Lost may be what wasn't synced: stamped at most a window ahead
            // of the clock then (further ahead is synced before it's taken).
            self.floor = self.floor.max(now + self.window + 1);
        }
        if self.floor > now + AHEAD + self.window + 1 {
            tracing::error!(floor = self.floor, now, "federation: the clock is far behind what the list of used signatures saw — signed requests are refused until it catches up (or remove the list after checking the clock)");
        }
        // Running now: the mark is for the next start to find, after a clean
        // stop. Can't take it away: it would say "clean" after a crash —
        // nothing is taken until it is gone (`take`).
        if clean && std::fs::remove_file(&mark).is_err() {
            self.mark_left = true;
        }
        // Drops what expired.
        if found {
            if let Err(e) = self.rewrite() {
                tracing::warn!(error = %e, "federation: the list of used signatures wasn't compacted");
            }
        }
    }

    fn insert(&mut self, sender: String, sig: String, ts: i64) -> bool {
        if self.seen.contains_key(&sig) {
            return false;
        }
        *self.per.entry(sender.clone()).or_default() += 1;
        self.by_time.insert((ts, sig.clone()));
        self.seen.insert(sig, (ts, sender));
        true
    }

    fn forget(&mut self, sig: &str) {
        if let Some((ts, sender)) = self.seen.remove(sig) {
            self.by_time.remove(&(ts, sig.to_string()));
            if let Some(n) = self.per.get_mut(&sender) {
                *n -= 1;
                if *n == 0 { self.per.remove(&sender); }
            }
        }
    }

    /// Drop signatures older than the window, oldest first: each one is
    /// looked at once, when it leaves.
    fn prune(&mut self, now: i64) {
        while let Some((ts, sig)) = self.by_time.first().cloned() {
            if now - ts <= self.window {
                break;
            }
            self.work += 1;
            // Never good again, even if the clock comes back.
            self.floor = self.floor.max(ts + 1);
            self.forget(&sig);
        }
    }

    fn remember(&mut self, sender: &str, sig: &str, ts: i64, now: i64) -> bool {
        self.prune(now);
        if self.seen.contains_key(sig) {
            return false;
        }
        // Full (only holders of a key or the secret can make that many):
        // refuse new ones until old ones expire. Forgetting them, as before
        // 1.21.0-24, let every one of them be sent again. One sender fills
        // only its own share.
        if self.per.get(sender).copied().unwrap_or(0) >= self.max_per {
            tracing::warn!(%sender, "federation: too many signed requests from one sender in one clock window — refusing");
            return false;
        }
        if self.seen.len() >= self.max {
            tracing::warn!("federation: too many signed requests in one clock window — refusing");
            return false;
        }
        self.insert(sender.to_string(), sig.to_string(), ts)
    }

    fn record(&mut self, sender: &str, sig: &str, ts: i64, now: i64) -> std::io::Result<()> {
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
        let sender = if sender.is_empty() || sender.contains(char::is_whitespace) { "?" } else { sender };
        if let Some(f) = &mut self.file {
            f.write_all(format!("{ts} {sig} {sender}\n").as_bytes())?;
            self.lines += 1;
            self.unsynced = true;
            // Stamped further ahead than the floor after a crash reaches:
            // on disk before it's taken.
            if now - self.last_sync >= SYNC_MS || ts > now + self.window {
                f.sync_data()?;
                self.last_sync = now;
                self.unsynced = false;
            }
        }
        Ok(())
    }

    /// Replace the file with the signatures still held.
    fn rewrite(&mut self) -> std::io::Result<()> {
        let Some(path) = self.path.clone() else {
            return Ok(());
        };
        self.file = None;
        let mut text: String = if self.floor > 0 { format!("{} {FLOOR_SIG} -\n", self.floor) } else { String::new() };
        text.extend(self.seen.iter().map(|(sig, (ts, sender))| format!("{ts} {sig} {sender}\n")));
        let tmp = path.with_extension("tmp");
        crate::fed::write_private(&tmp, &text)?;
        std::fs::File::open(&tmp)?.sync_all()?;
        std::fs::rename(&tmp, &path)?;
        self.lines = self.seen.len() + 1;
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

    /// An entry kept until a later time (a pass, until it expires) survives
    /// a restart however far ahead it is (review: it was dropped on reload).
    #[test]
    fn a_future_stamped_entry_survives_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("seen.log");
        let first = Replay::open(Some(file.clone()), WINDOW, 100, 100, NOW);
        assert!(first.take("pass", "pass:n1", NOW + 600_000, NOW));
        first.close_clean();
        drop(first);
        let again = Replay::open(Some(file), WINDOW, 100, 100, NOW + 10);
        assert!(!again.take("pass", "pass:n1", NOW + 600_000, NOW + 10));
    }

    /// A damaged line in the middle (not only a cut last one) may have held
    /// a used signature: everything signed before the start is refused, even
    /// after a clean stop.
    #[test]
    fn a_damaged_log_refuses_older_signatures() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("seen.log");
        let first = Replay::open(Some(file.clone()), WINDOW, 100, 100, NOW);
        assert!(first.take("s", "aa", NOW, NOW));
        assert!(first.take("s", "bb", NOW, NOW));
        first.close_clean();
        drop(first);
        // A block of the file went bad: "bb"'s line is garbage now.
        let text = std::fs::read_to_string(&file).unwrap().replace(&format!("{NOW} bb"), "\u{fffd}\u{fffd}x b?");
        std::fs::write(&file, text + &format!("{NOW} cc s\n")).unwrap();
        let again = Replay::open(Some(file), WINDOW, 100, 100, NOW + 10);
        assert!(!again.take("s", "bb", NOW, NOW + 10), "the lost entry can't be replayed");
        assert!(!again.take("s", "zz", NOW + 5, NOW + 10), "nothing signed before the start");
        let after = NOW + 11 + super::AHEAD;
        assert!(again.take("s", "fresh", after, after), "after the floor");
    }

    /// The clock went back (NTP, a dead RTC battery): entries stamped "in the
    /// future" stay until they really expire; a jump forward only makes old
    /// signatures too old to pass the clock check before this table.
    #[test]
    fn a_clock_jump_back_keeps_entries() {
        let r = Replay::open(None, WINDOW, 100, 100, NOW);
        assert!(r.take("s", "a", NOW, NOW));
        let back = NOW - 3_600_000;
        assert!(!r.take("s", "a", NOW, back), "still remembered an hour \"earlier\"");
        assert!(r.take("s", "b", back, back));
    }

    /// Review of #113 (27.11): the clock runs ahead past the window, the
    /// table drops the entry as expired, then the clock comes back — the
    /// signature was good again. Also across a restart made meanwhile.
    #[test]
    fn a_clock_jump_forward_then_back_does_not_reopen_a_signature() {
        let r = Replay::open(None, WINDOW, 100, 100, NOW);
        assert!(r.take("s", "a", NOW, NOW));
        let ahead = NOW + 3 * WINDOW;
        assert!(r.take("s", "b", ahead, ahead), "the jump forward prunes \"a\"");
        assert!(!r.take("s", "a", NOW, NOW + 1), "back again: still used");
        assert!(r.take("s", "c", NOW + 2, NOW + 2), "newer than anything dropped: fine");

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("seen.log");
        let first = Replay::open(Some(file.clone()), WINDOW, 100, 100, NOW);
        assert!(first.take("s", "a", NOW, NOW));
        first.close_clean();
        drop(first);
        // Restarted while the clock was ahead: "a" left the file as expired.
        let ahead = Replay::open(Some(file.clone()), WINDOW, 100, 100, NOW + 3 * WINDOW);
        ahead.close_clean();
        drop(ahead);
        let back = Replay::open(Some(file), WINDOW, 100, 100, NOW + 1);
        assert!(!back.take("s", "a", NOW, NOW + 1), "back after a restart: still used");
    }

    /// Review of #113: the clean mark that can't be taken away would say
    /// "clean" after a crash — nothing is taken until it is gone.
    #[test]
    fn a_clean_mark_that_stays_closes_the_table() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("seen.log");
        let first = Replay::open(Some(file.clone()), WINDOW, 100, 100, NOW);
        assert!(first.take("s", "a", NOW, NOW));
        first.close_clean();
        drop(first);
        // The mark can't be removed (here: a folder with a file in it).
        let mark = super::mark_of(&file);
        std::fs::remove_file(&mark).unwrap();
        std::fs::create_dir(&mark).unwrap();
        std::fs::write(mark.join("x"), "").unwrap();
        let again = Replay::open(Some(file.clone()), WINDOW, 100, 100, NOW + 10);
        assert!(!again.take("s", "fresh", NOW + 10, NOW + 10), "refused while the mark stays");
        std::fs::remove_dir_all(&mark).unwrap();
        assert!(again.take("s", "fresh", NOW + 20, NOW + 20), "taken once it is gone");
        // A table stopped cleanly and used again in the same process takes
        // its mark away again first.
        again.close_clean();
        assert!(mark.exists());
        assert!(again.take("s", "more", NOW + 30, NOW + 30));
        assert!(!mark.exists(), "running again: no clean mark left behind");
    }

    /// Review of #113: a crash loses what wasn't synced. A signature stamped
    /// a little ahead of our clock, or a pass (stamped with its expiry, up
    /// to 11 minutes ahead), was past the floor "now" of the next start.
    #[test]
    fn a_crash_loses_nothing_stamped_ahead() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("seen.log");
        let first = Replay::open(Some(file.clone()), WINDOW, 100, 100, NOW);
        assert!(first.take("s", "ahead", NOW + WINDOW - 1, NOW));
        assert!(first.take("pass", "pass:n1", NOW + 660_000, NOW));
        drop(first); // a crash...
        // ...and what wasn't synced is gone: only what is synced remains.
        let kept: String = std::fs::read_to_string(&file).unwrap().lines().filter(|l| !l.contains(" ahead ")).map(|l| format!("{l}\n")).collect();
        std::fs::write(&file, kept).unwrap();
        let later = NOW + 1000;
        let again = Replay::open(Some(file.clone()), WINDOW, 100, 100, later);
        assert!(!again.take("s", "ahead", NOW + WINDOW - 1, later), "a signature stamped ahead isn't taken again");
        assert!(!again.take("pass", "pass:n1", NOW + 660_000, later), "a pass taken before the crash was on disk");
        let after = later + WINDOW + 1;
        assert!(again.take("s", "fresh", after, after), "signed after the floor: fine");
    }

    /// A damaged line may have held anything up to the furthest stamp
    /// allowed: nothing stamped up to there is taken.
    #[test]
    fn a_damaged_log_refuses_passes_too() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("seen.log");
        std::fs::write(&file, format!("{NOW} ok s\n\u{fffd}garbage\n{NOW} ok2 s\n")).unwrap();
        std::fs::write(super::mark_of(&file), "clean\n").unwrap();
        let r = Replay::open(Some(file), WINDOW, 100, 100, NOW);
        assert!(!r.take("pass", "pass:lost", NOW + 660_000, NOW), "a pass of the damaged line");
        assert!(!r.take("s", "ahead", NOW + WINDOW, NOW));
    }

    /// Nothing is taken stamped further ahead than a pass can be: such an
    /// entry would hold its place (and the floor) for no reason.
    #[test]
    fn too_far_ahead_is_refused() {
        let r = Replay::open(None, WINDOW, 100, 100, NOW);
        assert!(!r.take("pass", "pass:far", NOW + super::AHEAD + 1, NOW));
        assert!(r.take("pass", "pass:near", NOW + super::AHEAD, NOW));
        assert!(!r.take("s", super::FLOOR_SIG, NOW, NOW), "the floor line's name is reserved");
    }

    /// Load: how fast the table takes signatures with its file (printed, run
    /// with --ignored --nocapture).
    #[test]
    #[ignore = "measurement"]
    fn replay_throughput() {
        let dir = tempfile::tempdir().unwrap();
        let r = Replay::open(Some(dir.path().join("seen.log")), WINDOW, super::MAX, super::MAX_PER, NOW);
        let n = 200_000i64;
        let t = std::time::Instant::now();
        for i in 0..n {
            let now = NOW + i * 6; // 200 000 requests over 20 minutes (~170/s): the window turns over
            assert!(r.take(if i % 2 == 0 { "secret" } else { "node" }, &format!("{i:064x}"), now, now));
        }
        let el = t.elapsed();
        let size = std::fs::metadata(dir.path().join("seen.log")).unwrap().len();
        println!("replay: {n} takes in {el:?} ({:.0}/s, {:.1} µs each), file {} KiB, held {}", n as f64 / el.as_secs_f64(), el.as_micros() as f64 / n as f64, size / 1024, r.inner.lock().unwrap().seen.len());
    }

    /// A5: one sender filling its share doesn't keep the others out.
    #[test]
    fn per_sender_quota_isolates_a_flood() {
        let r = Replay::open(None, WINDOW, 1000, 100, NOW);
        for i in 0..100 {
            assert!(r.take("secret", &format!("f{i}"), NOW, NOW));
        }
        assert!(!r.take("secret", "f-more", NOW, NOW), "the flooding sender is at its quota");
        assert!(r.take("v2-node", "v2:node:n1", NOW, NOW), "another sender still gets through");
        assert!(!r.take("secret", "f0", NOW, NOW), "and nothing of the flood is forgotten");
        // Its share frees up as its signatures expire.
        let later = NOW + WINDOW + 1;
        assert!(r.take("secret", "f-more", later, later));
    }

    /// A5: expired signatures leave in time order — each looked at once —
    /// instead of a scan of the whole table on every request.
    #[test]
    fn pruning_is_amortized() {
        let r = Replay::open(None, WINDOW, 1_000_000, 1_000_000, NOW);
        let n = 30_000i64;
        // Signatures spread over three windows, asked in time order.
        for i in 0..n {
            let t = NOW + i * (3 * WINDOW / n);
            assert!(r.take("s", &format!("s{i}"), t, t));
        }
        let g = r.inner.lock().unwrap();
        assert!(g.work <= n as u64, "looked at {} entries for {n} requests", g.work);
        assert!(g.seen.len() as i64 <= n / 3 + 2, "what's held is about one window: {}", g.seen.len());
        assert_eq!(g.seen.len(), g.by_time.len());
        assert_eq!(g.per.values().sum::<usize>(), g.seen.len());
    }

    /// A5: a start without the clean mark (crash, power cut) refuses every
    /// signature made before it — the last ones taken may not be on disk.
    #[test]
    fn crash_without_clean_mark_refuses_older_signatures() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("seen.log");
        let first = Replay::open(Some(file.clone()), WINDOW, 100, 100, NOW);
        assert!(first.take("s", "a", NOW, NOW));
        drop(first); // no close_clean: a crash
        // Lost from the file (not synced before the crash).
        std::fs::write(&file, "").unwrap();
        let later = NOW + 1000;
        let again = Replay::open(Some(file.clone()), WINDOW, 100, 100, later);
        assert!(!again.take("s", "lost", NOW + 500, later), "signed before this start");
        assert!(!again.take("s", "a", NOW, later), "the lost one isn't taken again either");
        assert!(!again.take("s", "fresh", later, later), "nor anything within a window of the start");
        let after = later + WINDOW + 1;
        assert!(again.take("s", "fresh", after, after), "signed after that: fine");
    }

    /// A5: after a clean stop, signatures from before it still work (a peer
    /// doesn't have to sign again), and what was taken stays taken.
    #[test]
    fn clean_restart_keeps_working() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("seen.log");
        let first = Replay::open(Some(file.clone()), WINDOW, 100, 100, NOW);
        assert!(first.take("s", "a", NOW, NOW));
        first.close_clean();
        assert!(super::mark_of(&file).exists());
        drop(first);
        let again = Replay::open(Some(file.clone()), WINDOW, 100, 100, NOW + 10);
        assert!(!super::mark_of(&file).exists(), "the mark is taken away while running");
        assert!(!again.take("s", "a", NOW, NOW + 10), "still taken");
        assert!(again.take("s", "b", NOW + 5, NOW + 10), "signed before the restart, never used: fine");
    }

    /// A5: two processes on one file (the new one while Windows replaces the
    /// old): the second refuses until the first is gone, then reads what the
    /// first recorded.
    #[test]
    fn a_second_process_waits_for_the_first() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("seen.log");
        let old = Replay::open(Some(file.clone()), WINDOW, 100, 100, NOW);
        let new = Replay::open(Some(file.clone()), WINDOW, 100, 100, NOW);
        assert!(!new.take("s", "x", NOW, NOW), "refused while the other one runs");
        assert!(old.take("s", "a", NOW, NOW));
        old.close_clean();
        drop(old);
        assert!(!new.take("s", "a", NOW, NOW + 1), "it read what the first one took");
        assert!(new.take("s", "x", NOW, NOW + 1));
    }

    #[test]
    fn a_signature_is_taken_once() {
        let r = Replay::open(None, WINDOW, 100, 100, NOW);
        assert!(r.take("s", "a", NOW, NOW));
        assert!(!r.take("s", "a", NOW, NOW), "replayed");
        assert!(r.take("s", "b", NOW, NOW));
    }

    /// A full table refuses new signatures and keeps the old ones, so none
    /// of them can be replayed; expired ones make room again.
    #[test]
    fn a_full_table_stays_closed() {
        let r = Replay::open(None, WINDOW, 20, 20, NOW);
        for i in 0..20 {
            assert!(r.take("s", &format!("s{i}"), NOW, NOW));
        }
        assert!(!r.take("s", "one-more", NOW, NOW));
        assert!(!r.take("s", "s0", NOW, NOW), "still remembered");
        let later = NOW + WINDOW + 1;
        assert!(r.take("s", "one-more", later, later));
    }

    #[test]
    fn a_restart_does_not_forget() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("seen.log");
        let first = Replay::open(Some(file.clone()), WINDOW, 100, 100, NOW);
        assert!(first.take("s", "v2:node:nonce", NOW, NOW));
        assert!(first.take("s", "old", NOW - WINDOW, NOW));
        drop(first);
        let again = Replay::open(Some(file.clone()), WINDOW, 100, 100, NOW + 1);
        assert!(!again.take("s", "v2:node:nonce", NOW, NOW + 1), "replayed after a restart");
        let after = NOW + 2 + WINDOW;
        assert!(again.take("s", "new", after, after));
        // What expired isn't carried on.
        assert!(!std::fs::read_to_string(&file).unwrap().contains("old"));
    }

    #[test]
    fn a_line_cut_short_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("seen.log");
        std::fs::write(&file, format!("{NOW} whole\n{NOW}")).unwrap();
        std::fs::write(super::mark_of(&file), "clean\n").unwrap();
        let r = Replay::open(Some(file.clone()), WINDOW, 100, 100, NOW);
        assert!(!r.take("s", "whole", NOW, NOW));
        assert!(r.take("s", "next", NOW, NOW));
        drop(r);
        assert!(!Replay::open(Some(file), WINDOW, 100, 100, NOW).take("s", "next", NOW, NOW));
    }

    #[test]
    fn one_of_many_at_once_gets_through() {
        let dir = tempfile::tempdir().unwrap();
        let r = std::sync::Arc::new(Replay::open(Some(dir.path().join("seen.log")), WINDOW, 1000, 1000, NOW));
        let threads: Vec<_> = (0..32)
            .map(|_| {
                let r = r.clone();
                std::thread::spawn(move || (0..50).filter(|i| r.take("s", &format!("s{i}"), NOW, NOW)).count())
            })
            .collect();
        let taken: usize = threads.into_iter().map(|t| t.join().unwrap()).sum();
        assert_eq!(taken, 50);
    }

    #[test]
    fn nothing_is_accepted_that_cannot_be_recorded() {
        let dir = tempfile::tempdir().unwrap();
        let nowhere = Replay::open(Some(dir.path().join("missing").join("seen.log")), WINDOW, 100, 100, NOW);
        assert!(!nowhere.take("s", "a", NOW, NOW));
        // There, but unreadable (a folder in its place): closed for a while.
        let unreadable = Replay::open(Some(dir.path().to_path_buf()), WINDOW, 100, 100, NOW);
        assert!(!unreadable.take("s", "a", NOW, NOW));
    }
}
