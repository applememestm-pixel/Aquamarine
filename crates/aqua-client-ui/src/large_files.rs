//! Disk-backed transfers. The staging file is encrypted and its envelope stays in the vault.

use std::{
    fs::{self, File},
    path::PathBuf,
    sync::Mutex,
    time::{Duration, UNIX_EPOCH},
};

use aqua_client_core::Transport;
use aqua_core::{
    file_transfer::{chunk_count, decrypt_file_to, encrypt_file_to, MAX_RESUMABLE_FILE_SIZE},
    FileEnvelope, CHUNK_TAG,
};

use crate::state::AppState;

pub(crate) static UPLOAD: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub(crate) struct Pending {
    pub envelope: FileEnvelope,
    path: PathBuf,
    record: String,
}

impl Pending {
    pub(crate) async fn upload(&self, transport: &Transport, kind: &str) -> Result<(), String> {
        transport
            .upload_blob_resumable(&self.envelope, &self.path, kind)
            .await
            .map_err(|e| e.to_string())
    }

    pub(crate) fn finish(self, state: &Mutex<AppState>) {
        let s = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = s.vault.delete_record(&self.record);
        let _ = fs::remove_file(&self.path);
    }
}

pub(crate) async fn prepare(
    app: &tauri::AppHandle,
    source: PathBuf,
    filename: String,
    mime: Option<String>,
    category: Option<String>,
) -> Result<Pending, String> {
    use tauri::Manager;
    let app = app.clone();
    tokio::task::spawn_blocking(move || {
        let state = app.state::<Mutex<AppState>>();
        let meta = fs::metadata(&source).map_err(|e| e.to_string())?;
        let modified = meta
            .modified()
            .map_err(|e| e.to_string())?
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let key = serde_json::to_vec(&(
            source.as_os_str(),
            meta.len(),
            modified,
            &filename,
            &mime,
            &category,
        ))
        .map_err(|e| e.to_string())?;
        let name = aqua_core::hex_sha256(&key);
        let record = format!("large-upload:{name}");
        let folder = {
            let s = state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            s.data_dir.join("large-upload")
        };
        fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        let path = folder.join(format!("{name}.sealed"));
        if let Ok(entries) = fs::read_dir(&folder) {
            let s = state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for entry in entries.flatten() {
                let entry_path = entry.path();
                if entry_path == path
                    || !entry
                        .metadata()
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.elapsed().ok())
                        .is_some_and(|age| age > Duration::from_secs(86_400))
                {
                    continue;
                }
                if let Some(stem) = entry_path.file_stem().and_then(|v| v.to_str()) {
                    let _ = s.vault.delete_record(&format!("large-upload:{stem}"));
                }
                let _ = fs::remove_file(entry_path);
            }
        }
        {
            let s = state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Ok(Some(value)) = s.vault.get_record(&record) {
                if let Ok(wire) = std::str::from_utf8(&value) {
                    if let Ok(envelope) = FileEnvelope::from_message_text(wire) {
                        if fs::metadata(&path)
                            .is_ok_and(|m| Some(m.len()) == sealed_size(&envelope))
                        {
                            return Ok(Pending {
                                envelope,
                                path,
                                record,
                            });
                        }
                    }
                }
            }
        }
        let temporary = Temporary(folder.join(format!("{name}.part")));
        let input = File::open(&source).map_err(|e| e.to_string())?;
        let mut out = File::create(&temporary.0).map_err(|e| e.to_string())?;
        let envelope = encrypt_file_to(input, &mut out, meta.len(), &filename, mime, category)
            .map_err(|e| e.to_string())?;
        out.sync_all().map_err(|e| e.to_string())?;
        drop(out);
        if fs::metadata(&source)
            .ok()
            .filter(|m| m.len() == meta.len())
            .and_then(|m| m.modified().ok())
            != meta.modified().ok()
        {
            return Err("file changed during encryption".into());
        }
        if path.exists() {
            fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
        fs::rename(&temporary.0, &path).map_err(|e| e.to_string())?;
        let s = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        s.vault
            .put_record(&record, envelope.to_message_text().as_bytes())
            .map_err(|e| e.to_string())?;
        Ok(Pending {
            envelope,
            path,
            record,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

fn sealed_size(envelope: &FileEnvelope) -> Option<u64> {
    let piece = envelope.chunk.filter(|&c| c > 0)?;
    if envelope.size > MAX_RESUMABLE_FILE_SIZE {
        return None;
    }
    Some(envelope.size + chunk_count(envelope.size, piece) * CHUNK_TAG as u64)
}

struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub(crate) async fn download(
    transport: &Transport,
    envelope: FileEnvelope,
    output: PathBuf,
) -> Result<(), String> {
    let suffix = crate::state::uuid_like();
    let dir = output.parent().ok_or("missing download directory")?;
    let sealed = Temporary(dir.join(format!(".{suffix}.sealed")));
    let mut file = tokio::fs::File::create(&sealed.0)
        .await
        .map_err(|e| e.to_string())?;
    let len = sealed_size(&envelope).ok_or("invalid encrypted file")?;
    transport
        .download_blob_to(&envelope.cid, &mut file, len)
        .await
        .map_err(|e| e.to_string())?;
    drop(file);
    let plain = Temporary(dir.join(format!(".{suffix}.part")));
    tokio::task::spawn_blocking(move || {
        let input = File::open(&sealed.0).map_err(|e| e.to_string())?;
        let mut out = File::create(&plain.0).map_err(|e| e.to_string())?;
        decrypt_file_to(&envelope, input, &mut out).map_err(|e| e.to_string())?;
        out.sync_all().map_err(|e| e.to_string())?;
        drop(out);
        if output.exists() {
            return Err("download destination already exists".into());
        }
        fs::rename(&plain.0, &output).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
