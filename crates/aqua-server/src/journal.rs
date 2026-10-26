//! The admin panel's journal: who came online / went offline, signed in or
//! registered, and which relays and people of the federation reached us.
//!
//! The last `CAP` events are kept in memory; every event is also appended
//! to `AQUA_JOURNAL` (`aqua-journal.log` by default, one tab-separated line
//! per event) so they can be counted later. `AQUA_JOURNAL=off` keeps it in
//! memory only.

use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::sync::Mutex;

const CAP: usize = 2000;

#[derive(Clone, serde::Serialize)]
pub struct Event {
    /// Unix seconds.
    pub at: u64,
    pub user: String,
    /// "online" | "offline" | "login" | "register"
    pub ev: &'static str,
    /// Device, how long they were online…
    pub note: String,
}

#[derive(Clone, serde::Serialize)]
pub struct Visitor {
    /// A relay's address, or a person's @name.
    pub who: String,
    /// "relay" | "user"
    pub kind: &'static str,
    pub first: u64,
    pub last: u64,
    pub count: u64,
}

static LOG: Mutex<VecDeque<Event>> = Mutex::new(VecDeque::new());
static VISITORS: Mutex<Option<HashMap<String, Visitor>>> = Mutex::new(None);
/// When each person came online (for "was online 12 min").
static SINCE: Mutex<Option<HashMap<String, u64>>> = Mutex::new(None);

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn log(user: &str, ev: &'static str, note: impl Into<String>) {
    let e = Event { at: now(), user: user.to_string(), ev, note: note.into() };
    let path = std::env::var("AQUA_JOURNAL").unwrap_or_else(|_| "aqua-journal.log".into());
    if path != "off" {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
            let clean = |s: &str| s.replace(['\t', '\n'], " ");
            let _ = writeln!(f, "{}\t{}\t{}\t{}", e.at, e.ev, clean(&e.user), clean(&e.note));
        }
    }
    if let Ok(mut l) = LOG.lock() {
        if l.len() >= CAP { l.pop_front(); }
        l.push_back(e);
    }
}

/// A person's first device connected.
pub fn online(user: &str, device: &str) {
    if let Ok(mut s) = SINCE.lock() { s.get_or_insert_with(HashMap::new).insert(user.to_string(), now()); }
    log(user, "online", device);
}

/// Their last device went away.
pub fn offline(user: &str) {
    let since = SINCE.lock().ok().and_then(|mut s| s.get_or_insert_with(HashMap::new).remove(user));
    let note = since.map(|t| dur(now().saturating_sub(t))).unwrap_or_default();
    log(user, "offline", note);
}

fn dur(s: u64) -> String {
    if s < 60 { format!("{s} s") } else if s < 3600 { format!("{} min", s / 60) } else { format!("{} h {} min", s / 3600, s % 3600 / 60) }
}

/// Newest first.
pub fn recent(limit: usize) -> Vec<Event> {
    LOG.lock().map(|l| l.iter().rev().take(limit).cloned().collect()).unwrap_or_default()
}

/// A relay of the network or a person from it reached us.
pub fn visitor(who: &str, kind: &'static str) {
    if who.is_empty() { return; }
    let t = now();
    if let Ok(mut v) = VISITORS.lock() {
        let m = v.get_or_insert_with(HashMap::new);
        let e = m.entry(format!("{kind}:{who}")).or_insert(Visitor { who: who.to_string(), kind, first: t, last: t, count: 0 });
        e.last = t;
        e.count += 1;
    }
}

/// Since this start, most recent first.
pub fn visitors() -> Vec<Visitor> {
    let mut v: Vec<Visitor> = VISITORS.lock().map(|v| v.as_ref().map(|m| m.values().cloned().collect()).unwrap_or_default()).unwrap_or_default();
    v.sort_by(|a, b| b.last.cmp(&a.last));
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn journal_keeps_events() {
        std::env::set_var("AQUA_JOURNAL", "off");
        online("jt_alice", "Pixel");
        offline("jt_alice");
        let r = recent(10);
        assert!(r.iter().any(|e| e.user == "jt_alice" && e.ev == "offline"));
        visitor("http://peer", "relay");
        visitor("http://peer", "relay");
        assert!(visitors().iter().any(|v| v.who == "http://peer" && v.count == 2));
    }
}
