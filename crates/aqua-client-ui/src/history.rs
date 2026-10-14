//! Chat history on disk.
//!
//! Every chat — its sidebar summary and all messages — is one record in
//! the encrypted vault (`hist:<chat id>`), JSON compressed with DEFLATE
//! and sealed with ChaCha20-Poly1305 like the rest of the vault. Groups
//! are one more record (`groups`). A background flush writes only the
//! chats whose content changed since the last write.
//!
//! Before this, 1:1 and group history lived only in memory, and "Saved
//! Messages" / `groups.json` sat on disk as plain JSON; both legacy files
//! are migrated into the vault and removed.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::io::{Read, Write};

use aqua_client_core::Vault;
use flate2::{read::DeflateDecoder, write::DeflateEncoder, Compression};
use serde::{Deserialize, Serialize};

use crate::state::{AppState, ChatMessage, ChatSummary, Group};

pub const PREFIX: &str = "hist:";
const GROUPS_KEY: &str = "groups";
/// Chats that are generated on the fly and never stored.
const EPHEMERAL: &[&str] = &["welcome"];

#[derive(Deserialize)]
struct Stored {
    summary: ChatSummary,
    messages: Vec<ChatMessage>,
}

#[derive(Serialize)]
struct StoredRef<'a> {
    summary: &'a ChatSummary,
    messages: &'a [ChatMessage],
}

pub fn compress(data: &[u8]) -> Vec<u8> {
    let mut enc = DeflateEncoder::new(Vec::with_capacity(data.len() / 3), Compression::best());
    let _ = enc.write_all(data);
    enc.finish().unwrap_or_default()
}

pub fn decompress(data: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(data.len() * 3);
    DeflateDecoder::new(data).read_to_end(&mut out).ok()?;
    Some(out)
}

fn hash_of(bytes: &[u8]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

/// Every stored chat, most recently written first.
pub fn load_all(vault: &Vault) -> Vec<(String, ChatSummary, Vec<ChatMessage>)> {
    let mut out = Vec::new();
    for key in vault.record_keys(PREFIX).unwrap_or_default() {
        let Some(id) = key.strip_prefix(PREFIX) else { continue };
        let Ok(Some(raw)) = vault.get_record(&key) else { continue };
        let Some(json) = decompress(&raw) else { continue };
        match serde_json::from_slice::<Stored>(&json) {
            Ok(st) => out.push((id.to_string(), st.summary, st.messages)),
            Err(e) => tracing::warn!(chat = id, error = %e, "stored chat could not be read"),
        }
    }
    out
}

pub fn load_groups(vault: &Vault) -> Option<HashMap<String, Group>> {
    let raw = vault.get_record(GROUPS_KEY).ok().flatten()?;
    let json = decompress(&raw)?;
    let list: Vec<Group> = serde_json::from_slice(&json).ok()?;
    Some(list.into_iter().map(|g| (g.id.clone(), g)).collect())
}

/// Write whatever changed. Cheap when nothing did (one serialisation +
/// hash per chat); called every few seconds and on exit.
pub fn flush(s: &mut AppState) {
    let mut written = 0usize;
    let ids: Vec<String> = s.chats.keys().cloned().collect();
    for id in &ids {
        if EPHEMERAL.contains(&id.as_str()) {
            continue;
        }
        let Some(book) = s.chats.get(id) else { continue };
        let Ok(json) = serde_json::to_vec(&StoredRef { summary: &book.summary, messages: &book.messages }) else { continue };
        let h = hash_of(&json);
        if s.hist_hashes.get(id) == Some(&h) {
            continue;
        }
        match s.vault.put_record(&format!("{PREFIX}{id}"), &compress(&json)) {
            Ok(()) => {
                s.hist_hashes.insert(id.clone(), h);
                written += 1;
            }
            Err(e) => tracing::warn!(chat = %id, error = %e, "saving chat history failed"),
        }
    }
    // Chats that were deleted.
    let gone: Vec<String> = s.hist_hashes.keys().filter(|k| !k.starts_with('\u{0}') && !s.chats.contains_key(*k)).cloned().collect();
    for id in gone {
        let _ = s.vault.delete_record(&format!("{PREFIX}{id}"));
        s.hist_hashes.remove(&id);
    }
    // Groups, in a stable order so the hash only moves on real changes.
    let mut groups: Vec<&Group> = s.groups.values().collect();
    groups.sort_by(|a, b| a.id.cmp(&b.id));
    if let Ok(json) = serde_json::to_vec(&groups) {
        let h = hash_of(&json);
        let key = "\u{0}groups".to_string();
        if s.hist_hashes.get(&key) != Some(&h) && s.vault.put_record(GROUPS_KEY, &compress(&json)).is_ok() {
            s.hist_hashes.insert(key, h);
            written += 1;
        }
    }
    if written > 0 {
        // The plain-JSON files of older versions are now in the vault.
        for legacy in [crate::state::SAVED_MESSAGES_FILE, crate::state::GROUPS_FILE] {
            let p = s.data_dir.join(legacy);
            if p.exists() {
                let _ = std::fs::remove_file(p);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_survives_a_restart_and_legacy_files_move_in() {
        let dir = tempfile::tempdir().unwrap();
        // An old install: Saved Messages as plain JSON.
        std::fs::write(
            dir.path().join(crate::state::SAVED_MESSAGES_FILE),
            r#"[{"id":"m-old","from":"me","time":"10:00","kind":{"type":"text","content":{"text":"legacy note"}}}]"#,
        ).unwrap();
        {
            let st = crate::state::AppState::boot(dir.path().to_path_buf()).unwrap();
            let mut s = st.lock().unwrap();
            assert_eq!(s.chats[crate::state::SAVED_MESSAGES_ID].messages.len(), 1);
            let mut book = s.chats[crate::state::SAVED_MESSAGES_ID].summary.clone();
            book.id = "alice".into();
            book.name = "Alice".into();
            s.chats.insert("alice".into(), crate::state::ChatBook {
                summary: book,
                messages: vec![crate::state::text_msg_from_me("hi alice")],
            });
            flush(&mut s);
        }
        assert!(!dir.path().join(crate::state::SAVED_MESSAGES_FILE).exists(), "plain JSON should be gone");
        let st = crate::state::AppState::boot(dir.path().to_path_buf()).unwrap();
        let s = st.lock().unwrap();
        assert_eq!(s.chats["alice"].messages.len(), 1);
        assert_eq!(s.chats["alice"].summary.name, "Alice");
        assert_eq!(s.chats[crate::state::SAVED_MESSAGES_ID].messages[0].id, "m-old");
        assert_eq!(s.chat_order[0], crate::state::SAVED_MESSAGES_ID);
        // Nothing readable on disk.
        let raw = std::fs::read(dir.path().join("vault.sqlite")).unwrap();
        assert!(!raw.windows(8).any(|w| w == b"hi alice"));
    }
    #[test]
    fn deflate_roundtrip_and_shrinks_chat_json() {
        let msg = r#"{"id":"m-1","from":"me","time":"12:00","kind":{"type":"text","content":{"text":"hello there"}}},"#;
        let json = format!("[{}]", msg.repeat(500).trim_end_matches(','));
        let c = compress(json.as_bytes());
        assert!(c.len() * 10 < json.len(), "{} vs {}", c.len(), json.len());
        assert_eq!(decompress(&c).unwrap(), json.as_bytes());
    }
}
