//! Personalisation files: notification sounds the user adds (a "Sounds"
//! folder in the app) and theme config files (`.aquatheme`) to share.

use std::path::PathBuf;
use std::sync::Mutex as StdMutex;

use serde::Serialize;
use tauri::State;

use crate::state::AppState;

const SOUND_EXTS: &[&str] = &["mp3", "ogg", "oga", "opus", "wav", "m4a", "flac", "webm", "weba", "aac"];
const MAX_SOUND: usize = 3 * 1024 * 1024;
const MAX_THEME: u64 = 24 * 1024 * 1024;

#[derive(Serialize)]
pub struct SoundFile {
    /// File name inside the sounds folder (also the id).
    pub id: String,
    /// Name without extension, for display.
    pub name: String,
    pub path: String,
}

fn sounds_dir(state: &State<'_, StdMutex<AppState>>) -> Result<PathBuf, String> {
    let dir = state.lock().map_err(|e| e.to_string())?.data_dir.join("sounds");
    std::fs::create_dir_all(&dir).map_err(|e| format!("sounds folder: {e}"))?;
    Ok(dir)
}

fn safe_name(name: &str) -> String {
    let n: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || " ._-()".contains(c) { c } else { '_' })
        .collect();
    let n = n.trim().trim_start_matches('.').to_string();
    if n.is_empty() { "sound".into() } else { n.chars().take(80).collect() }
}

fn ext_of(name: &str) -> String {
    name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default()
}

fn entry(path: PathBuf) -> Option<SoundFile> {
    let id = path.file_name()?.to_str()?.to_string();
    if !SOUND_EXTS.contains(&ext_of(&id).as_str()) {
        return None;
    }
    let name = id.rsplit_once('.').map(|(n, _)| n.to_string()).unwrap_or_else(|| id.clone());
    Some(SoundFile { id, name, path: path.display().to_string() })
}

/// Add a sound (bytes from a file the user picked or a shared theme).
#[tauri::command]
pub fn save_sound(state: State<'_, StdMutex<AppState>>, name: String, bytes: Vec<u8>) -> Result<SoundFile, String> {
    if bytes.len() > MAX_SOUND {
        return Err(format!("sound too large (max {} MB)", MAX_SOUND / 1024 / 1024));
    }
    let name = safe_name(&name);
    if !SOUND_EXTS.contains(&ext_of(&name).as_str()) {
        return Err("not an audio file (mp3, ogg, opus, wav, m4a, flac)".into());
    }
    let dir = sounds_dir(&state)?;
    let path = dir.join(&name);
    std::fs::write(&path, &bytes).map_err(|e| format!("save sound: {e}"))?;
    entry(path).ok_or_else(|| "bad sound name".into())
}

#[tauri::command]
pub fn list_sounds(state: State<'_, StdMutex<AppState>>) -> Result<Vec<SoundFile>, String> {
    let dir = sounds_dir(&state)?;
    let mut out: Vec<SoundFile> = std::fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .filter_map(|e| entry(e.path()))
        .collect();
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(out)
}

#[tauri::command]
pub fn delete_sound(state: State<'_, StdMutex<AppState>>, id: String) -> Result<(), String> {
    let dir = sounds_dir(&state)?;
    let p = dir.join(safe_name(&id));
    if p.parent() != Some(dir.as_path()) {
        return Err("bad id".into());
    }
    std::fs::remove_file(p).map_err(|e| e.to_string())
}

fn theme_path_ok(path: &str) -> Result<PathBuf, String> {
    let p = PathBuf::from(path);
    match ext_of(path).as_str() {
        "aquatheme" | "json" => Ok(p),
        _ => Err("theme files end in .aquatheme".into()),
    }
}

/// Save a theme config the user exported (path from a save dialog).
#[tauri::command]
pub fn write_theme_file(path: String, text: String) -> Result<(), String> {
    let p = theme_path_ok(&path)?;
    if text.len() as u64 > MAX_THEME {
        return Err("theme too large".into());
    }
    std::fs::write(p, text).map_err(|e| format!("write: {e}"))
}

/// Read a theme config (picked by the user, or received in a chat).
#[tauri::command]
pub fn read_theme_file(path: String) -> Result<String, String> {
    let p = theme_path_ok(&path)?;
    let len = std::fs::metadata(&p).map_err(|e| e.to_string())?.len();
    if len > MAX_THEME {
        return Err("theme too large".into());
    }
    std::fs::read_to_string(p).map_err(|e| format!("read: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names() {
        assert_eq!(safe_name("../../etc/passwd"), "_.._etc_passwd");
        assert_eq!(safe_name("Ding (2).mp3"), "Ding (2).mp3");
        assert_eq!(safe_name(".."), "sound");
        assert!(theme_path_ok("/x/a.aquatheme").is_ok());
        assert!(theme_path_ok("/x/a.sh").is_err());
    }
}
