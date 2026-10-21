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

/// Save a text export the user picked a place for (calendar .ics, a chat
/// document as .md / .txt / .html). Only those extensions, up to 4 MB.
#[tauri::command]
pub fn write_text_file(path: String, text: String) -> Result<(), String> {
    let p = std::path::PathBuf::from(&path);
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if !matches!(ext.as_str(), "ics" | "md" | "txt" | "html" | "csv") {
        return Err("only .ics, .md, .txt, .html and .csv files".into());
    }
    if text.len() > 4 * 1024 * 1024 {
        return Err("too large".into());
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

/// Write bytes the UI made (a cropped photo…) to a temp file and return
/// its path, so commands that take a file can use it.
#[tauri::command]
pub fn save_temp_file(state: State<'_, StdMutex<AppState>>, name: String, bytes: Vec<u8>) -> Result<String, String> {
    if bytes.len() > 40 * 1024 * 1024 {
        return Err("file too large".into());
    }
    let dir = { state.lock().map_err(|e| e.to_string())?.data_dir.join("tmp") };
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let p = dir.join(format!("{stamp:x}-{}", safe_name(&name)));
    std::fs::write(&p, bytes).map_err(|e| e.to_string())?;
    Ok(p.display().to_string())
}

// ---- game catalogue (via the relay) --------------------------------------------

#[derive(serde::Serialize)]
pub struct GameHit { pub id: String, pub name: String, pub has_icon: bool }

#[tauri::command]
pub async fn games_search(state: State<'_, StdMutex<AppState>>, q: String) -> Result<Vec<GameHit>, String> {
    let t = { state.lock().map_err(|e| e.to_string())?.transport.clone().ok_or("not connected")? };
    let v = t.games_search(q.trim()).await.map_err(|e| e.to_string())?;
    Ok(v.into_iter().map(|(id, name, has_icon)| GameHit { id, name, has_icon }).collect())
}

/// Local path of a game's icon (downloaded from the relay once).
#[tauri::command]
pub async fn game_icon(state: State<'_, StdMutex<AppState>>, id: String) -> Result<Option<String>, String> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) || id.len() > 24 {
        return Ok(None);
    }
    let (dir, t) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        (s.data_dir.join("game-icons"), s.transport.clone())
    };
    let file = dir.join(format!("{id}.png"));
    if file.exists() {
        return Ok(Some(file.display().to_string()));
    }
    let Some(t) = t else { return Ok(None) };
    match t.game_icon(&id).await.map_err(|e| e.to_string())? {
        Some(b) => {
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            std::fs::write(&file, b).map_err(|e| e.to_string())?;
            Ok(Some(file.display().to_string()))
        }
        None => Ok(None),
    }
}

/// Portrait cover of a game, cached in `<data>/game-covers/`. `None` when the
/// relay has none (then the UI draws a card from the icon).
#[tauri::command]
pub async fn game_cover(state: State<'_, StdMutex<AppState>>, id: String) -> Result<Option<String>, String> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) || id.len() > 24 {
        return Ok(None);
    }
    let (dir, t) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        (s.data_dir.join("game-covers"), s.transport.clone())
    };
    let file = dir.join(format!("{id}.jpg"));
    if file.exists() {
        return Ok(Some(file.display().to_string()));
    }
    // Asked recently, nothing there: don't ask again for a day.
    let none = dir.join(format!("{id}.none"));
    if std::fs::metadata(&none).and_then(|m| m.modified()).ok()
        .and_then(|t| std::time::SystemTime::now().duration_since(t).ok())
        .is_some_and(|age| age < std::time::Duration::from_secs(24 * 3600)) {
        return Ok(None);
    }
    let Some(t) = t else { return Ok(None) };
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    match t.game_cover(&id).await.map_err(|e| e.to_string())? {
        Some(b) => {
            std::fs::write(&file, b).map_err(|e| e.to_string())?;
            Ok(Some(file.display().to_string()))
        }
        None => {
            let _ = std::fs::write(&none, b"");
            Ok(None)
        }
    }
}

// ---- wallpapers -------------------------------------------------------------

fn wallpapers_dir(state: &State<'_, StdMutex<AppState>>) -> Result<std::path::PathBuf, String> {
    let dir = state.lock().map_err(|e| e.to_string())?.data_dir.join("wallpapers");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// Keep a wallpaper picture in the library (`<data>/wallpapers/`).
#[tauri::command]
pub fn save_wallpaper(state: State<'_, StdMutex<AppState>>, name: String, bytes: Vec<u8>) -> Result<String, String> {
    if bytes.is_empty() || bytes.len() > 40 * 1024 * 1024 {
        return Err("pick an image up to 40 MB".into());
    }
    let dir = wallpapers_dir(&state)?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    // Android's gallery hands over "content://…/1000012345" — no extension.
    // Name the file after what it really is, so the library lists it.
    let kind = infer::get(&bytes).filter(|k| k.mime_type().starts_with("image/")).ok_or("not a picture")?;
    let mut base = safe_name(&name);
    if !is_wall_ext(&base) {
        base = format!("{base}.{}", kind.extension());
    }
    let p = dir.join(format!("{stamp}-{base}"));
    std::fs::write(&p, bytes).map_err(|e| e.to_string())?;
    Ok(p.display().to_string())
}

fn is_wall_ext(name: &str) -> bool {
    matches!(ext_of(name).as_str(), "png" | "jpg" | "jpeg" | "webp" | "gif" | "avif" | "bmp")
}

/// Older saves without an extension: add one from the content.
fn fix_wallpaper_names(dir: &std::path::Path) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if !p.is_file() || is_wall_ext(&p.display().to_string()) {
            continue;
        }
        let head = std::fs::File::open(&p).ok().and_then(|mut f| { use std::io::Read; let mut b = vec![0u8; 64]; f.read(&mut b).ok().map(|n| { b.truncate(n); b }) });
        if let Some(kind) = head.and_then(|b| infer::get(&b)).filter(|k| k.mime_type().starts_with("image/")) {
            let _ = std::fs::rename(&p, p.with_file_name(format!("{}.{}", p.file_name().and_then(|n| n.to_str()).unwrap_or("wallpaper"), kind.extension())));
        }
    }
}

/// Wallpapers in the library, newest first.
#[tauri::command]
pub fn list_wallpapers(state: State<'_, StdMutex<AppState>>) -> Result<Vec<String>, String> {
    let dir = wallpapers_dir(&state)?;
    fix_wallpaper_names(&dir);
    let mut v: Vec<(std::time::SystemTime, String)> = std::fs::read_dir(&dir).map_err(|e| e.to_string())?
        .flatten()
        .filter(|e| e.path().is_file())
        .filter(|e| is_wall_ext(&e.path().display().to_string()))
        .map(|e| (e.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH), e.path().display().to_string()))
        .collect();
    v.sort_by(|a, b| b.0.cmp(&a.0));
    Ok(v.into_iter().map(|x| x.1).collect())
}

#[tauri::command]
pub fn delete_wallpaper(state: State<'_, StdMutex<AppState>>, path: String) -> Result<(), String> {
    let dir = wallpapers_dir(&state)?;
    let p = std::path::PathBuf::from(&path);
    if p.parent() != Some(dir.as_path()) {
        return Err("not a library wallpaper".into());
    }
    std::fs::remove_file(p).map_err(|e| e.to_string())
}

/// Swap the window / taskbar icon for the one the UI drew (the logo in the
/// current theme colours, or the user's own picture). RGBA, square, ≤ 512.
/// Wayland compositors take the icon from the .desktop file instead, so
/// there this is a no-op.
#[tauri::command]
pub fn set_window_icon(window: tauri::WebviewWindow, rgba: Vec<u8>, size: u32) -> Result<(), String> {
    if size == 0 || size > 512 || rgba.len() != (size * size * 4) as usize {
        return Err("bad icon".into());
    }
    // Phones have no window icon (Android / iOS: the launcher icon is fixed).
    #[cfg(desktop)]
    {
        let img = tauri::image::Image::new_owned(rgba, size, size);
        window.set_icon(img).map_err(|e| e.to_string())
    }
    #[cfg(mobile)]
    {
        let _ = (window, rgba);
        Ok(())
    }
}

/// The console-chat bot script (tools/aqua-console.py), bundled so the
/// Bots tab can save a copy next to the user's other files.
const CONSOLE_BOT: &str = include_str!("../../../tools/aqua-console.py");

/// Save the bundled console bot to a path from a save dialog (.py only).
#[tauri::command]
pub fn save_console_bot(path: String) -> Result<(), String> {
    if ext_of(&path) != "py" {
        return Err("the script must end in .py".into());
    }
    std::fs::write(&path, CONSOLE_BOT).map_err(|e| format!("write: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755));
    }
    Ok(())
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

// ---- keyboard shortcuts (keybindings.json) -------------------------------------

fn keybindings_file() -> Option<std::path::PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("APPDATA").map(std::path::PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
        .map(|d| d.join("aquamarine").join("keybindings.json"))
}

/// The user's shortcut overrides (`{"settings": ["Ctrl+Comma"], …}`), if any.
#[tauri::command]
pub fn load_keybindings() -> Option<String> {
    let s = std::fs::read_to_string(keybindings_file()?).ok()?;
    (s.len() < 64 * 1024).then_some(s)
}

#[tauri::command]
pub fn save_keybindings(json: String) -> Result<(), String> {
    if json.len() > 64 * 1024 || serde_json::from_str::<serde_json::Value>(&json).is_err() {
        return Err("bad keybindings".into());
    }
    let p = keybindings_file().ok_or("no config directory")?;
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(&p, json).map_err(|e| e.to_string())
}

/// Open keybindings.json in the system's text editor (created if missing).
#[tauri::command]
pub fn open_keybindings_file() -> Result<String, String> {
    let p = keybindings_file().ok_or("no config directory")?;
    if !p.exists() {
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        std::fs::write(&p, "{\n}\n").map_err(|e| e.to_string())?;
    }
    let s = p.display().to_string();
    #[cfg(target_os = "linux")]
    let r = std::process::Command::new("xdg-open").arg(&s).spawn();
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").args(["-t", &s]).spawn();
    #[cfg(target_os = "windows")]
    let r = std::process::Command::new("notepad").arg(&s).spawn();
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    let r: std::io::Result<std::process::Child> = Err(std::io::Error::other("not supported here"));
    r.map(|_| s).map_err(|e| e.to_string())
}
