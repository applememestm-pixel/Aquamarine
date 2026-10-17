//! Sticker packs: the @Stickers bot, built into the picker.
//!
//! Packs made here (a folder under `<data>/stickers/` whose `pack.json`
//! has no Telegram `name`) can be created as sticker or emoji packs, get
//! stickers added and removed, reordered, re-tagged with another emoji
//! (the emoji is the file-name prefix, "😀_name.webp"), and a pack icon.
//! Copies of Telegram packs are read-only: every command here refuses
//! them. Instead they are kept in sync with the relay's copy, which in
//! turn follows the pack on Telegram (`sync_telegram_pack`,
//! `check_pack_updates`).

use std::path::{Path, PathBuf};
use std::sync::Mutex as StdMutex;

use futures_util::StreamExt;
use serde::Serialize;
use tauri::State;

use crate::media::{self, is_telegram_pack, read_pack_json, write_pack_json};
use crate::state::AppState;

fn root(state: &State<'_, StdMutex<AppState>>) -> Result<PathBuf, String> {
    Ok(state.lock().map_err(|e| e.to_string())?.data_dir.join("stickers"))
}

fn bad_title(t: &str) -> bool {
    t.is_empty() || t.len() > 64 || t.starts_with('.') || t.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|'])
}

/// The folder of one of our own packs.
fn own_pack(state: &State<'_, StdMutex<AppState>>, title: &str) -> Result<PathBuf, String> {
    let root = root(state)?;
    let t = title.trim();
    if bad_title(t) {
        return Err("bad pack name".into());
    }
    let dir = root.join(t);
    if !dir.is_dir() || !media::path_is_within(&root, &dir) {
        return Err("no such pack".into());
    }
    if is_telegram_pack(&dir) {
        return Err("packs from Telegram can't be changed".into());
    }
    Ok(dir)
}

/// Pack folder + file name of a sticker in one of our own packs.
fn own_sticker(state: &State<'_, StdMutex<AppState>>, path: &str) -> Result<(PathBuf, String), String> {
    let root = root(state)?;
    let p = PathBuf::from(path);
    if !p.is_file() || !media::path_is_within(&root, &p) {
        return Err("no such sticker".into());
    }
    let dir = p.parent().ok_or("no such sticker")?.to_path_buf();
    if dir.parent().map(|d| d.canonicalize().ok()) != Some(root.canonicalize().ok()) {
        return Err("no such sticker".into());
    }
    if is_telegram_pack(&dir) {
        return Err("packs from Telegram can't be changed".into());
    }
    let file = p.file_name().and_then(|n| n.to_str()).ok_or("bad name")?.to_string();
    Ok((dir, file))
}

/// Media files of a pack in display order.
fn ordered_files(dir: &Path) -> Vec<String> {
    let title = dir.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
    media::scan(dir.parent().and_then(|p| p.parent()).unwrap_or(dir))
        .packs
        .into_iter()
        .find(|p| p.title == title)
        .map(|p| p.items.iter().filter_map(|i| Path::new(&i.path).file_name().and_then(|n| n.to_str()).map(str::to_string)).collect())
        .unwrap_or_default()
}

fn set_order(dir: &Path, order: Vec<String>) -> Result<(), String> {
    let mut m = read_pack_json(dir);
    m.insert("order".into(), serde_json::json!(order));
    write_pack_json(dir, &m)
}

/// New pack: "sticker" or "emoji". Returns its title.
#[tauri::command]
pub fn create_sticker_pack(state: State<'_, StdMutex<AppState>>, title: String, kind: String) -> Result<String, String> {
    let t = title.trim().to_string();
    if bad_title(&t) {
        return Err("pack name: 1–64 characters, no / \\ : * ? \" < > |".into());
    }
    let dir = root(&state)?.join(&t);
    if dir.exists() {
        return Err("a pack with this name already exists".into());
    }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut m = serde_json::Map::new();
    m.insert("type".into(), serde_json::json!(if kind == "emoji" { "custom_emoji" } else { "regular" }));
    m.insert("own".into(), serde_json::json!(true));
    write_pack_json(&dir, &m)?;
    Ok(t)
}

/// Copy files into one of our packs (appended at the end). Returns paths.
#[tauri::command]
pub fn pack_add_files(state: State<'_, StdMutex<AppState>>, title: String, paths: Vec<String>) -> Result<Vec<String>, String> {
    let dir = own_pack(&state, &title)?;
    let mut order = ordered_files(&dir);
    let mut added = Vec::new();
    for p in paths {
        let src = Path::new(&p);
        let Some(name) = src.file_name().and_then(|n| n.to_str()) else { continue };
        if !media::classify(name).1 || !src.is_file() || std::fs::metadata(src).map(|m| m.len() > media::ADD_LIMIT).unwrap_or(true) {
            continue;
        }
        let out = media::unique_path(&dir, &media::sanitize_name(name));
        if std::fs::copy(src, &out).is_ok() {
            if let Some(f) = out.file_name().and_then(|n| n.to_str()) {
                order.push(f.to_string());
            }
            added.push(out.display().to_string());
        }
    }
    set_order(&dir, order)?;
    Ok(added)
}

/// Put a sticker (saved by the editor into a pack) at the end of the order.
pub fn note_added(dir: &Path, file: &str) {
    if is_telegram_pack(dir) {
        return;
    }
    let mut order = ordered_files(dir);
    order.retain(|f| f != file);
    order.push(file.to_string());
    let _ = set_order(dir, order);
}

#[tauri::command]
pub fn pack_set_order(state: State<'_, StdMutex<AppState>>, title: String, files: Vec<String>) -> Result<(), String> {
    let dir = own_pack(&state, &title)?;
    let have = ordered_files(&dir);
    // Only names that exist; anything missing keeps its place at the end.
    let mut order: Vec<String> = files.into_iter().filter(|f| have.contains(f)).collect();
    for f in have {
        if !order.contains(&f) {
            order.push(f);
        }
    }
    set_order(&dir, order)
}

#[tauri::command]
pub fn pack_set_icon(state: State<'_, StdMutex<AppState>>, title: String, file: Option<String>) -> Result<(), String> {
    let dir = own_pack(&state, &title)?;
    let mut m = read_pack_json(&dir);
    match file {
        Some(f) if dir.join(&f).is_file() && !f.contains(['/', '\\']) => { m.insert("icon".into(), serde_json::json!(f)); }
        _ => { m.remove("icon"); }
    }
    write_pack_json(&dir, &m)
}

/// Change a sticker's emoji (its file-name prefix). Returns the new path.
#[tauri::command]
pub fn sticker_set_emoji(state: State<'_, StdMutex<AppState>>, path: String, emoji: String) -> Result<String, String> {
    let (dir, file) = own_sticker(&state, &path)?;
    let e: String = emoji.trim().chars().filter(|c| !c.is_whitespace() && !matches!(c, '_' | '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')).take(8).collect();
    if e.is_empty() || !e.bytes().any(|b| b >= 0x80) {
        return Err("pick an emoji".into());
    }
    let (_, rest) = media::parse_emoji_prefix(&file);
    let new_name = format!("{e}_{rest}");
    if new_name == file {
        return Ok(path);
    }
    let dest = media::unique_path(&dir, &new_name);
    let new_file = dest.file_name().and_then(|n| n.to_str()).unwrap_or(&new_name).to_string();
    let order: Vec<String> = ordered_files(&dir).into_iter().map(|f| if f == file { new_file.clone() } else { f }).collect();
    std::fs::rename(dir.join(&file), &dest).map_err(|e| e.to_string())?;
    let mut m = read_pack_json(&dir);
    if m.get("icon").and_then(|v| v.as_str()) == Some(file.as_str()) {
        m.insert("icon".into(), serde_json::json!(new_file));
    }
    m.insert("order".into(), serde_json::json!(order));
    write_pack_json(&dir, &m)?;
    Ok(dest.display().to_string())
}

#[tauri::command]
pub fn sticker_delete(state: State<'_, StdMutex<AppState>>, path: String) -> Result<(), String> {
    let (dir, file) = own_sticker(&state, &path)?;
    std::fs::remove_file(dir.join(&file)).map_err(|e| e.to_string())?;
    let mut m = read_pack_json(&dir);
    if let Some(o) = m.get("order").and_then(|v| v.as_array()).cloned() {
        m.insert("order".into(), serde_json::Value::Array(o.into_iter().filter(|v| v.as_str() != Some(file.as_str())).collect()));
    }
    if m.get("icon").and_then(|v| v.as_str()) == Some(file.as_str()) {
        m.remove("icon");
    }
    write_pack_json(&dir, &m)
}

// ---------------------------------------------------------------- Telegram

#[derive(Serialize, Default)]
pub struct SyncReport {
    pub title: String,
    pub changed: bool,
    pub added: usize,
    pub removed: usize,
}

/// Bring a local copy in line with the relay's manifest: download what's
/// missing (16 at a time), drop what's gone, save Telegram's order.
pub async fn apply_manifest(t: &aqua_client_core::Transport, dir: &Path, m: &serde_json::Value) -> Result<SyncReport, String> {
    let name = m["name"].as_str().unwrap_or_default().to_string();
    let files: Vec<String> = m["files"].as_array().cloned().unwrap_or_default().iter()
        .filter_map(|f| f["file"].as_str())
        .filter(|f| !f.contains(['/', '\\']) && !f.starts_with('.') && f != &"pack.json")
        .map(str::to_string)
        .collect();
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let missing: Vec<String> = files.iter().filter(|f| !dir.join(f).exists()).cloned().collect();
    let added = futures_util::stream::iter(missing)
        .map(|file| {
            let (out, name) = (dir.join(&file), name.clone());
            async move {
                let bytes = t.pack_file(&name, &file).await.ok()?;
                // Write next to it first: a half-written sticker never shows.
                let tmp = out.with_extension("part");
                std::fs::write(&tmp, bytes).ok()?;
                std::fs::rename(&tmp, &out).ok()
            }
        })
        .buffer_unordered(16)
        .filter_map(|x| async move { x })
        .count()
        .await;
    let mut removed = 0;
    if !files.is_empty() {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().to_string();
                if media::classify(&n).1 && !files.contains(&n) && std::fs::remove_file(e.path()).is_ok() {
                    removed += 1;
                }
            }
        }
    }
    let mut meta = read_pack_json(dir);
    let old_rev = meta.get("rev").and_then(|v| v.as_u64());
    let rev = m["rev"].as_u64().unwrap_or(0);
    meta.insert("name".into(), serde_json::json!(name));
    meta.insert("title".into(), serde_json::json!(m["title"].as_str().unwrap_or(&name)));
    meta.insert("type".into(), serde_json::json!(m["type"].as_str().unwrap_or("regular")));
    meta.insert("order".into(), serde_json::json!(files));
    meta.insert("rev".into(), serde_json::json!(rev));
    write_pack_json(dir, &meta)?;
    Ok(SyncReport {
        title: dir.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string(),
        changed: added > 0 || removed > 0 || old_rev != Some(rev),
        added,
        removed,
    })
}

fn transport_and_root(state: &State<'_, StdMutex<AppState>>) -> Result<(std::sync::Arc<aqua_client_core::Transport>, PathBuf), String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    Ok((s.transport.clone().ok_or("not connected")?, s.data_dir.join("stickers")))
}

/// "Update" on a Telegram pack: the relay re-checks it on Telegram now.
#[tauri::command]
pub async fn sync_telegram_pack(state: State<'_, StdMutex<AppState>>, title: String) -> Result<SyncReport, String> {
    let (t, root) = transport_and_root(&state)?;
    let dir = root.join(title.trim());
    if bad_title(title.trim()) || !dir.is_dir() {
        return Err("no such pack".into());
    }
    let name = read_pack_json(&dir).get("name").and_then(|v| v.as_str()).map(str::to_string).ok_or("not a Telegram pack")?;
    let m = match t.refresh_pack(&name).await {
        Ok(m) => m,
        // Older relay without /refresh: take what it has.
        Err(_) => t.pack_manifest(&name).await.map_err(|e| e.to_string())?.ok_or("the server doesn't have this pack")?,
    };
    apply_manifest(&t, &dir, &m).await
}

/// Background check of every Telegram pack (the relay refreshes its copy
/// from Telegram when it's a few hours old). Returns the packs that changed.
#[tauri::command]
pub async fn check_pack_updates(state: State<'_, StdMutex<AppState>>) -> Result<Vec<SyncReport>, String> {
    let (t, root) = transport_and_root(&state)?;
    let dirs: Vec<(PathBuf, String, Option<u64>)> = std::fs::read_dir(&root)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|d| {
            let m = read_pack_json(&d);
            let name = m.get("name")?.as_str()?.to_string();
            let rev = m.get("rev").and_then(|v| v.as_u64());
            (!name.is_empty()).then_some((d, name, rev))
        })
        .collect();
    let t = &t;
    let out: Vec<SyncReport> = futures_util::stream::iter(dirs)
        .map(|(dir, name, rev)| async move {
            let m = t.pack_manifest(&name).await.ok().flatten()?;
            let new_rev = m["rev"].as_u64();
            if new_rev.is_some() && new_rev == rev {
                return None;
            }
            apply_manifest(t, &dir, &m).await.ok().filter(|r| r.changed)
        })
        .buffer_unordered(4)
        .filter_map(|x| async move { x })
        .collect()
        .await;
    Ok(out)
}
