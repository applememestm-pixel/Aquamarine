//! Sync older Saved Messages attachments from the device that still has the file.

use std::{
    collections::HashMap,
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use aqua_client_core::Transport;
use aqua_core::{encrypt_file_streaming, encrypt_file_with_category, MAX_FILE_SIZE};
use tauri::{AppHandle, Emitter, Manager};

use crate::state::{AppState, ChatMessage, MessageKind, SAVED_MESSAGES_ID};

const RETRY_AFTER: Duration = Duration::from_secs(300);
const INTERVAL: Duration = Duration::from_secs(15);

fn legacy_cid(cid: &str) -> bool {
    cid.strip_prefix("local-").is_some_and(|suffix| {
        !suffix.is_empty() && suffix.len() <= 56 && suffix.bytes().all(|b| b.is_ascii_hexdigit())
    })
}

pub(super) fn promoted(msg: &ChatMessage) -> Option<(&str, &str)> {
    let stable = msg
        .meta
        .as_ref()?
        .saved_local_cid
        .as_deref()
        .filter(|cid| legacy_cid(cid))?;
    match &msg.kind {
        MessageKind::File { cid, .. }
            if cid.len() == 64 && cid.bytes().all(|b| b.is_ascii_hexdigit()) =>
        {
            Some((stable, cid))
        }
        _ => None,
    }
}

pub(super) fn apply_upgrade(local: &mut ChatMessage, remote: &ChatMessage) -> bool {
    let Some((old_cid, _)) = promoted(remote) else {
        return false;
    };
    let MessageKind::File { cid, saved_at, .. } = &local.kind else {
        return false;
    };
    if old_cid != cid {
        return false;
    }
    let mut kind = remote.kind.clone();
    // The synced path belongs to the other device. Keep this device's own copy.
    if let MessageKind::File { saved_at: dest, .. } = &mut kind {
        dest.clone_from(saved_at);
    }
    local.kind = kind;
    local
        .meta
        .get_or_insert_with(Default::default)
        .saved_local_cid = Some(old_cid.to_owned());
    true
}

struct Pending {
    data_dir: PathBuf,
    transport: Arc<Transport>,
    message: ChatMessage,
}

fn pending(app: &AppHandle, attempted: &HashMap<(PathBuf, String), Instant>) -> Option<Pending> {
    let state = app.state::<Mutex<AppState>>();
    let s = state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if crate::applock::is_locked() {
        return None;
    }
    let transport = s.transport.clone()?;
    let me = s.username.as_deref()?;
    let book = s.chats.get(SAVED_MESSAGES_ID)?;
    let data_dir = s.data_dir.clone();
    let message = book.messages.iter().rev().find(|m| {
        (m.from == "me" || m.from == me)
            && !m.meta.as_ref().is_some_and(|meta| meta.deleted)
            && matches!(&m.kind, MessageKind::File { cid, saved_at: Some(_), .. } if legacy_cid(cid))
            && !attempted.contains_key(&(data_dir.clone(), m.id.clone()))
    })?.clone();
    Some(Pending {
        data_dir,
        transport,
        message,
    })
}

fn prepare(root: &Path, mut original: ChatMessage) -> Option<(ChatMessage, Vec<u8>)> {
    let MessageKind::File {
        cid,
        name,
        mime,
        category,
        pack,
        caption,
        saved_at: Some(path),
        ..
    } = &original.kind
    else {
        return None;
    };
    let path = PathBuf::from(path).canonicalize().ok()?;
    // Only migrate files already copied into this account's Saved Messages storage.
    if !crate::media::path_is_within(&root.join("saved-media"), &path) {
        return None;
    }
    let file = std::fs::File::open(&path).ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > MAX_FILE_SIZE {
        return None;
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_FILE_SIZE + 1).read_to_end(&mut bytes).ok()?;
    let stream =
        bytes.len() > 1024 * 1024 && mime.as_deref().is_some_and(|m| m.starts_with("video/"));
    let (mut envelope, ciphertext) = if stream {
        encrypt_file_streaming(&bytes, name, mime.clone(), category.clone())
    } else {
        encrypt_file_with_category(&bytes, name, mime.clone(), category.clone())
    }
    .ok()?;
    envelope.pack.clone_from(pack);
    envelope.caption.clone_from(caption);
    let stable = cid.clone();
    original.kind = crate::state::classify_full(&envelope.to_message_text(), None).0;
    original
        .meta
        .get_or_insert_with(Default::default)
        .saved_local_cid = Some(stable);
    Some((original, ciphertext))
}

async fn publish(app: &AppHandle, work: Pending) {
    let id = work.message.id.clone();
    let root = work.data_dir.clone();
    let Ok(Some((message, ciphertext))) =
        tokio::task::spawn_blocking(move || prepare(&root, work.message)).await
    else {
        return;
    };
    let MessageKind::File {
        cid,
        name,
        mime,
        category,
        ..
    } = &message.kind
    else {
        return;
    };
    let kind = crate::commands::blob_kind(category.as_deref(), mime.as_deref(), name);
    if work
        .transport
        .upload_blob_kind(cid, ciphertext, false, Some(kind))
        .await
        .is_err()
    {
        return;
    }
    let state = app.state::<Mutex<AppState>>();
    let mut s = state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if s.data_dir != work.data_dir
        || !s
            .transport
            .as_ref()
            .is_some_and(|t| Arc::ptr_eq(t, &work.transport))
    {
        return;
    }
    let Some(local) = s
        .chats
        .get_mut(SAVED_MESSAGES_ID)
        .and_then(|book| book.messages.iter_mut().find(|m| m.id == id))
    else {
        return;
    };
    if local.meta.as_ref().is_some_and(|m| m.deleted) || !apply_upgrade(local, &message) {
        return;
    }
    crate::history::mark_dirty(&mut s, SAVED_MESSAGES_ID);
    drop(s);
    let _ = app.emit("aqua://chats_updated", ());
}

pub(super) fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut attempted = HashMap::new();
        loop {
            attempted.retain(|_, at: &mut Instant| at.elapsed() < RETRY_AFTER);
            if let Some(work) = pending(&app, &attempted) {
                attempted.insert(
                    (work.data_dir.clone(), work.message.id.clone()),
                    Instant::now(),
                );
                publish(&app, work).await;
            }
            tokio::time::sleep(INTERVAL).await;
        }
    });
}
