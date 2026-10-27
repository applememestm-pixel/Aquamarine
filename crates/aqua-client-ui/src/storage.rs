//! Where received files go and how much room the app may take
//! (Settings → Data and storage), like Telegram's "Download path" and
//! "Manage local storage".
//!
//! - Download folder: the system Downloads/Aquamarine (default), a
//!   temporary folder inside the app's data that is wiped on log-out, or a
//!   folder the user picked.
//! - Limits: total size of saved files, size of the media cache, and age —
//!   the oldest files the app saved itself go first. Only files the app
//!   saved (a message points at them) and its own cache are ever deleted.

use std::path::{Path, PathBuf};
use std::sync::Mutex as StdMutex;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::state::{AppState, MessageKind};

const FILE: &str = "storage.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct StorageConf {
    /// "default" | "temp" | "custom"
    pub mode: String,
    /// The folder for "custom".
    pub custom: String,
    /// Saved files may take this many MB in total (0 = no limit).
    pub total_mb: u64,
    /// The media cache (article videos, music) may take this many MB.
    pub cache_mb: u64,
    /// Files untouched for longer than this many days are removed (0 = never).
    pub max_days: u64,
}

impl Default for StorageConf {
    fn default() -> Self {
        StorageConf { mode: "default".into(), custom: String::new(), total_mb: 0, cache_mb: 1024, max_days: 0 }
    }
}

pub fn load(data_dir: &Path) -> StorageConf {
    std::fs::read(data_dir.join(FILE)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save(data_dir: &Path, c: &StorageConf) -> Result<(), String> {
    let b = serde_json::to_vec_pretty(c).map_err(|e| e.to_string())?;
    std::fs::write(data_dir.join(FILE), b).map_err(|e| e.to_string())
}

/// The system's own folder for downloads, with our subfolder.
pub fn default_downloads(data_dir: &Path) -> PathBuf {
    #[cfg(target_os = "android")]
    {
        return data_dir.join("downloads");
    }
    #[cfg(target_os = "ios")]
    {
        return data_dir.join("Downloads");
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let _ = data_dir;
        dirs::download_dir().unwrap_or_else(std::env::temp_dir).join("Aquamarine")
    }
}

pub fn temp_downloads(data_dir: &Path) -> PathBuf {
    data_dir.join("temp-downloads")
}

/// Where a received file is saved right now.
pub fn downloads_dir(data_dir: &Path) -> PathBuf {
    let c = load(data_dir);
    match c.mode.as_str() {
        "temp" => temp_downloads(data_dir),
        "custom" if !c.custom.trim().is_empty() => PathBuf::from(c.custom.trim()),
        _ => default_downloads(data_dir),
    }
}

pub fn media_cache_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("media-cache")
}

#[derive(Debug, Serialize)]
pub struct StorageInfo {
    pub conf: StorageConf,
    /// The folder files go to now, and the three choices resolved.
    pub downloads: String,
    pub default_dir: String,
    pub temp_dir: String,
}

#[tauri::command]
pub fn storage_get(state: State<'_, StdMutex<AppState>>) -> Result<StorageInfo, String> {
    let d = state.lock().map_err(|e| e.to_string())?.data_dir.clone();
    Ok(StorageInfo {
        conf: load(&d),
        downloads: downloads_dir(&d).display().to_string(),
        default_dir: default_downloads(&d).display().to_string(),
        temp_dir: temp_downloads(&d).display().to_string(),
    })
}

#[tauri::command]
pub fn storage_set(app: AppHandle, state: State<'_, StdMutex<AppState>>, conf: StorageConf) -> Result<StorageInfo, String> {
    let d = state.lock().map_err(|e| e.to_string())?.data_dir.clone();
    let mut c = conf;
    if !matches!(c.mode.as_str(), "default" | "temp" | "custom") {
        c.mode = "default".into();
    }
    if c.mode == "custom" {
        let p = PathBuf::from(c.custom.trim());
        if !p.is_absolute() {
            return Err("choose a folder".into());
        }
        std::fs::create_dir_all(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        // Writable?
        let probe = p.join(".aquamarine-write-test");
        std::fs::write(&probe, b"ok").map_err(|e| format!("can't write to {}: {e}", p.display()))?;
        let _ = std::fs::remove_file(probe);
        c.custom = p.display().to_string();
        // Pictures from there must show in the app (asset protocol).
        use tauri::Manager;
        let _ = app.asset_protocol_scope().allow_directory(&p, true);
    }
    save(&d, &c)?;
    storage_get(state)
}

/// Sizes by kind, the way Telegram's "Local storage" shows them.
#[derive(Debug, Default, Serialize)]
pub struct StorageStats {
    pub images: u64,
    pub videos: u64,
    pub audio: u64,
    pub stickers: u64,
    pub files: u64,
    pub cache: u64,
    pub count: u64,
}

pub fn kind_of(name: &str, mime: Option<&str>, category: Option<&str>) -> &'static str {
    let n = name.to_ascii_lowercase();
    let m = mime.unwrap_or("").to_ascii_lowercase();
    let c = category.unwrap_or("");
    if c == "sticker" || c == "gif" || n.ends_with(".tgs") || n.ends_with(".gif") { return "stickers"; }
    if c == "voice" || m.starts_with("audio/") || [".mp3", ".ogg", ".opus", ".m4a", ".flac", ".wav", ".aac", ".weba"].iter().any(|e| n.ends_with(e)) { return "audio"; }
    if m.starts_with("image/") || [".jpg", ".jpeg", ".png", ".webp", ".bmp", ".heic", ".avif"].iter().any(|e| n.ends_with(e)) { return "images"; }
    if c == "circle" || m.starts_with("video/") || [".mp4", ".webm", ".mov", ".mkv", ".m4v"].iter().any(|e| n.ends_with(e)) { return "videos"; }
    "files"
}

/// Every file the app saved for a message: (path, kind). Deduplicated.
fn saved_files(s: &AppState) -> Vec<(String, &'static str)> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for b in s.chats.values() {
        for m in &b.messages {
            if let MessageKind::File { saved_at: Some(p), name, mime, category, .. } = &m.kind {
                if seen.insert(p.clone()) {
                    out.push((p.clone(), kind_of(name, mime.as_deref(), category.as_deref())));
                }
            }
        }
    }
    out
}

fn dir_size(p: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(p) else { return 0 };
    rd.flatten().map(|e| {
        let md = e.metadata();
        match md {
            Ok(m) if m.is_dir() => dir_size(&e.path()),
            Ok(m) => m.len(),
            Err(_) => 0,
        }
    }).sum()
}

#[tauri::command]
pub fn storage_stats(state: State<'_, StdMutex<AppState>>) -> Result<StorageStats, String> {
    let (files, d) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        (saved_files(&s), s.data_dir.clone())
    };
    let mut st = StorageStats::default();
    for (p, k) in files {
        let Ok(md) = std::fs::metadata(&p) else { continue };
        st.count += 1;
        let n = md.len();
        match k {
            "images" => st.images += n,
            "videos" => st.videos += n,
            "audio" => st.audio += n,
            "stickers" => st.stickers += n,
            _ => st.files += n,
        }
    }
    st.cache = dir_size(&media_cache_dir(&d));
    Ok(st)
}

/// Forget the local copy of these files: delete them and clear the
/// message's `saved_at`, so they can be downloaded again (they stay on the
/// relay as long as it keeps them).
fn drop_files(s: &mut AppState, paths: &std::collections::HashSet<String>) -> u64 {
    let mut freed = 0;
    for p in paths {
        if let Ok(md) = std::fs::metadata(p) {
            if std::fs::remove_file(p).is_ok() { freed += md.len(); }
        }
    }
    for b in s.chats.values_mut() {
        for m in &mut b.messages {
            if let MessageKind::File { saved_at, .. } = &mut m.kind {
                if saved_at.as_ref().is_some_and(|x| paths.contains(x)) { *saved_at = None; }
            }
        }
    }
    freed
}

/// Clear the chosen kinds ("images", "videos", "audio", "stickers",
/// "files", "cache"). Returns the bytes freed.
#[tauri::command]
pub fn storage_clear(app: AppHandle, state: State<'_, StdMutex<AppState>>, kinds: Vec<String>) -> Result<u64, String> {
    let mut freed = 0;
    {
        let mut s = state.lock().map_err(|e| e.to_string())?;
        let paths: std::collections::HashSet<String> = saved_files(&s).into_iter()
            .filter(|(_, k)| kinds.iter().any(|x| x == k)).map(|(p, _)| p).collect();
        freed += drop_files(&mut s, &paths);
        if kinds.iter().any(|k| k == "cache") {
            let c = media_cache_dir(&s.data_dir);
            freed += dir_size(&c);
            let _ = std::fs::remove_dir_all(&c);
            let _ = std::fs::create_dir_all(&c);
        }
    }
    let _ = app.emit("aqua://chats_updated", ());
    Ok(freed)
}

fn last_used(md: &std::fs::Metadata) -> SystemTime {
    md.accessed().ok().max(md.modified().ok()).unwrap_or(SystemTime::UNIX_EPOCH)
}

/// Apply the limits: too old first, then the oldest until it fits.
pub fn trim(s: &mut AppState) -> u64 {
    let c = load(&s.data_dir);
    let now = SystemTime::now();
    let mut freed = 0;
    let mut files: Vec<(String, u64, SystemTime)> = saved_files(s).into_iter()
        .filter_map(|(p, _)| std::fs::metadata(&p).ok().map(|m| (p, m.len(), last_used(&m))))
        .collect();
    if c.max_days > 0 {
        let cutoff = now.checked_sub(Duration::from_secs(c.max_days * 86_400)).unwrap_or(SystemTime::UNIX_EPOCH);
        let old: std::collections::HashSet<String> = files.iter().filter(|f| f.2 < cutoff).map(|f| f.0.clone()).collect();
        freed += drop_files(s, &old);
        files.retain(|f| !old.contains(&f.0));
    }
    if c.total_mb > 0 {
        let limit = c.total_mb * 1024 * 1024;
        let mut total: u64 = files.iter().map(|f| f.1).sum();
        files.sort_by_key(|f| f.2);
        let mut gone = std::collections::HashSet::new();
        for f in &files {
            if total <= limit { break; }
            total = total.saturating_sub(f.1);
            gone.insert(f.0.clone());
        }
        freed += drop_files(s, &gone);
    }
    if c.cache_mb > 0 {
        let dir = media_cache_dir(&s.data_dir);
        let limit = c.cache_mb * 1024 * 1024;
        let mut entries: Vec<(PathBuf, u64, SystemTime)> = std::fs::read_dir(&dir).map(|rd| rd.flatten()
            .filter_map(|e| e.metadata().ok().filter(|m| m.is_file()).map(|m| (e.path(), m.len(), last_used(&m)))).collect()).unwrap_or_default();
        let mut total: u64 = entries.iter().map(|e| e.1).sum();
        entries.sort_by_key(|e| e.2);
        for e in entries {
            if total <= limit { break; }
            if std::fs::remove_file(&e.0).is_ok() { total = total.saturating_sub(e.1); freed += e.1; }
        }
    }
    freed
}

#[tauri::command]
pub fn storage_trim(app: AppHandle, state: State<'_, StdMutex<AppState>>) -> Result<u64, String> {
    let freed = { let mut s = state.lock().map_err(|e| e.to_string())?; trim(&mut s) };
    if freed > 0 { let _ = app.emit("aqua://chats_updated", ()); }
    Ok(freed)
}

/// "Temporary folder": wiped when you log out.
pub fn on_log_out(s: &mut AppState) {
    if load(&s.data_dir).mode == "temp" {
        let dir = temp_downloads(&s.data_dir);
        let paths: std::collections::HashSet<String> = saved_files(s).into_iter()
            .map(|(p, _)| p).filter(|p| Path::new(p).starts_with(&dir)).collect();
        drop_files(s, &paths);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Every chat as JSON (texts, times, who wrote; files by name) — "Export
/// Aquamarine data". Written where the user chose in the save dialog.
#[tauri::command]
pub fn export_chats(state: State<'_, StdMutex<AppState>>, dest: String) -> Result<u64, String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    let mut chats = Vec::new();
    for id in &s.chat_order {
        let Some(b) = s.chats.get(id) else { continue };
        let msgs: Vec<serde_json::Value> = b.messages.iter().map(|m| {
            let body = match &m.kind {
                MessageKind::Text { text } => serde_json::json!({ "text": text }),
                MessageKind::Code { code, language } => serde_json::json!({ "code": code, "language": language }),
                MessageKind::File { name, size, mime, .. } => serde_json::json!({ "file": name, "size": size, "mime": mime }),
                other => serde_json::json!({ "other": format!("{other:?}").chars().take(200).collect::<String>() }),
            };
            serde_json::json!({ "id": m.id, "from": m.from, "time": m.time, "body": body })
        }).collect();
        chats.push(serde_json::json!({ "id": id, "name": b.summary.name, "messages": msgs }));
    }
    let doc = serde_json::json!({
        "app": "Aquamarine", "version": env!("CARGO_PKG_VERSION"),
        "account": s.username, "exported_at": SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0), "chats": chats,
    });
    let bytes = serde_json::to_vec_pretty(&doc).map_err(|e| e.to_string())?;
    let p = PathBuf::from(&dest);
    if !p.is_absolute() { return Err("choose where to save".into()); }
    std::fs::write(&p, &bytes).map_err(|e| format!("{}: {e}", p.display()))?;
    Ok(bytes.len() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn kinds() {
        assert_eq!(kind_of("a.jpg", None, None), "images");
        assert_eq!(kind_of("a.bin", Some("video/mp4"), None), "videos");
        assert_eq!(kind_of("v.ogg", None, Some("voice")), "audio");
        assert_eq!(kind_of("s.webp", None, Some("sticker")), "stickers");
        assert_eq!(kind_of("doc.pdf", None, None), "files");
    }
    #[test]
    fn conf_round_trip() {
        let d = std::env::temp_dir().join(format!("aqua-st-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        assert_eq!(load(&d), StorageConf::default());
        let c = StorageConf { mode: "temp".into(), total_mb: 2048, ..Default::default() };
        save(&d, &c).unwrap();
        assert_eq!(load(&d).mode, "temp");
        assert!(downloads_dir(&d).ends_with("temp-downloads"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
