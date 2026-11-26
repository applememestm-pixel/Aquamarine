//! The open chat is asked for again on every event. Only what changed since
//! the caller's last answer goes over to the interface: a long history was
//! sent, parsed and compared whole for each new message (slow on a phone).

use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{DefaultHasher, Hasher};
use std::sync::Mutex;

use crate::state::ChatMessage;

#[derive(Debug, serde::Serialize)]
pub struct MessagesDelta {
    /// Name this answer: the next request says it has it.
    pub rev: u64,
    /// `changed` is the whole chat; the caller's copy isn't used.
    pub full: bool,
    /// How many of the caller's messages, from the start, stay in place.
    pub keep: usize,
    /// The ids that follow them, in order.
    pub tail: Vec<String>,
    /// Messages that are new or differ from the caller's.
    pub changed: Vec<ChatMessage>,
}

/// What a caller holds: ids in order, each with a hash of its content.
struct Snapshot {
    rev: u64,
    chat: String,
    items: Vec<(String, u64)>,
}

/// Enough for a few open chats and windows; a forgotten one costs one full read.
const KEPT: usize = 12;

#[derive(Default)]
pub struct Snapshots {
    last_rev: u64,
    held: VecDeque<Snapshot>,
}

struct HashWriter(DefaultHasher);

impl std::io::Write for HashWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.write(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn digest(msg: &ChatMessage) -> u64 {
    let mut w = HashWriter(DefaultHasher::new());
    // Writing into a hasher can't fail.
    let _ = serde_json::to_writer(&mut w, msg);
    w.0.finish()
}

impl Snapshots {
    pub fn delta(&mut self, chat: &str, since: Option<u64>, msgs: Vec<ChatMessage>) -> MessagesDelta {
        let items: Vec<(String, u64)> = msgs.iter().map(|m| (m.id.clone(), digest(m))).collect();
        let base = since.and_then(|rev| self.held.iter().position(|s| s.rev == rev && s.chat == chat));
        // Two messages under one id can't be told apart by id: send it all.
        let unique = items.iter().map(|(id, _)| id.as_str()).collect::<HashSet<_>>().len() == items.len();
        if let Some(i) = base.filter(|_| unique) {
            let old = &self.held[i].items;
            if *old == items {
                return MessagesDelta { rev: self.held[i].rev, full: false, keep: items.len(), tail: Vec::new(), changed: Vec::new() };
            }
            let keep = old.iter().zip(&items).take_while(|(a, b)| a.0 == b.0).count();
            let tail = items[keep..].iter().map(|(id, _)| id.clone()).collect();
            let had: HashMap<&str, u64> = old.iter().map(|(id, h)| (id.as_str(), *h)).collect();
            let changed = msgs.into_iter().zip(&items)
                .filter(|(_, (id, h))| had.get(id.as_str()) != Some(h))
                .map(|(m, _)| m)
                .collect();
            self.held.remove(i);
            let rev = self.hold(chat, items);
            return MessagesDelta { rev, full: false, keep, tail, changed };
        }
        if let Some(i) = base {
            self.held.remove(i);
        }
        let rev = self.hold(chat, items);
        MessagesDelta { rev, full: true, keep: 0, tail: Vec::new(), changed: msgs }
    }

    fn hold(&mut self, chat: &str, items: Vec<(String, u64)>) -> u64 {
        self.last_rev += 1;
        self.held.push_back(Snapshot { rev: self.last_rev, chat: chat.to_string(), items });
        while self.held.len() > KEPT {
            self.held.pop_front();
        }
        self.last_rev
    }
}

pub fn delta(chat: &str, since: Option<u64>, msgs: Vec<ChatMessage>) -> MessagesDelta {
    static HELD: std::sync::LazyLock<Mutex<Snapshots>> = std::sync::LazyLock::new(Default::default);
    HELD.lock().unwrap_or_else(std::sync::PoisonError::into_inner).delta(chat, since, msgs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::MessageKind;

    fn msg(id: &str, text: &str) -> ChatMessage {
        ChatMessage {
            id: id.into(),
            from: "me".into(),
            time: "12:00:00".into(),
            at: None,
            kind: MessageKind::Text { text: text.into() },
            meta: None,
        }
    }

    fn ids(m: &[ChatMessage]) -> Vec<&str> {
        m.iter().map(|m| m.id.as_str()).collect()
    }

    #[test]
    fn only_changes_cross_over() {
        let mut s = Snapshots::default();
        let a = vec![msg("1", "a"), msg("2", "b"), msg("3", "c")];
        let first = s.delta("bob", None, a.clone());
        assert!(first.full);
        assert_eq!(ids(&first.changed), ["1", "2", "3"]);

        let same = s.delta("bob", Some(first.rev), a.clone());
        assert!(!same.full && same.changed.is_empty() && same.tail.is_empty());
        assert_eq!((same.rev, same.keep), (first.rev, 3));

        let mut b = a.clone();
        b.push(msg("4", "d"));
        let added = s.delta("bob", Some(first.rev), b.clone());
        assert_eq!(added.keep, 3);
        assert_eq!(added.tail, ["4"]);
        assert_eq!(ids(&added.changed), ["4"]);

        b[1] = msg("2", "edited");
        let edited = s.delta("bob", Some(added.rev), b.clone());
        assert_eq!((edited.keep, edited.tail.len()), (4, 0));
        assert_eq!(ids(&edited.changed), ["2"]);

        b.remove(1);
        let removed = s.delta("bob", Some(edited.rev), b);
        assert_eq!(removed.keep, 1);
        assert_eq!(removed.tail, ["3", "4"]);
        assert!(removed.changed.is_empty());
    }

    #[test]
    fn unknown_copy_gets_everything() {
        let mut s = Snapshots::default();
        let a = vec![msg("1", "a")];
        let bob = s.delta("bob", None, a.clone());
        // Another chat's revision, one already replaced, or made up.
        assert!(s.delta("eve", Some(bob.rev), a.clone()).full);
        assert!(s.delta("bob", Some(999), a.clone()).full);
        for _ in 0..KEPT {
            s.delta("eve", None, a.clone());
        }
        assert!(s.delta("bob", Some(bob.rev), a).full);
        let twins = vec![msg("1", "a"), msg("1", "b")];
        let t = s.delta("t", None, twins.clone());
        assert!(s.delta("t", Some(t.rev), twins).full);
    }
}
