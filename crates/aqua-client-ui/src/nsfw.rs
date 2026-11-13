//! Discord's "sensitive media" filter, on the device: photos, GIFs and
//! stickers are looked at by the image detector (aqua-nsfw) after they are
//! decrypted here — nothing is sent anywhere, the E2EE stays whole. The
//! interface blurs what it calls explicit (lib/nsfw.ts decides for whom).
//!
//! Cheap by design: one picture at a time (a phone keeps its other cores
//! for the interface), each file looked at once — the verdict is kept by
//! file name + size (media files are named by their content hash), in memory
//! and in `nsfw-cache.json`, so scrolling back or restarting costs nothing.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::state::AppState;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Verdict {
    /// Blur it.
    pub explicit: bool,
    /// 0..1 (porn + hentai + half of "sexy").
    pub level: f32,
}

const CACHE_FILE: &str = "nsfw-cache.json";
const CACHE_MAX: usize = 20_000;

struct Cache {
    /// "<file name>:<size>" → verdict (None: not a picture it reads).
    map: Option<HashMap<String, Option<Verdict>>>,
    loaded: bool,
    dirty: usize,
}

static CACHE: Mutex<Cache> = Mutex::new(Cache { map: None, loaded: false, dirty: 0 });

fn gate() -> &'static tokio::sync::Semaphore {
    static G: std::sync::OnceLock<tokio::sync::Semaphore> = std::sync::OnceLock::new();
    G.get_or_init(|| tokio::sync::Semaphore::new(1))
}

fn key(path: &Path) -> Option<String> {
    let len = std::fs::metadata(path).ok().filter(|m| m.is_file())?.len();
    Some(format!("{}:{len}", path.file_name()?.to_string_lossy()))
}

fn cached(data_dir: &Path, k: &str) -> Option<Option<Verdict>> {
    let mut c = CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if !c.loaded {
        c.loaded = true;
        c.map = std::fs::read(data_dir.join(CACHE_FILE)).ok().and_then(|b| serde_json::from_slice(&b).ok());
    }
    c.map.get_or_insert_with(HashMap::new).get(k).copied()
}

fn remember(data_dir: &Path, k: String, v: Option<Verdict>) {
    let snapshot = {
        let mut c = CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let map = c.map.get_or_insert_with(HashMap::new);
        if map.len() >= CACHE_MAX {
            map.clear(); // rare; cheaper than tracking age
        }
        map.insert(k, v);
        c.dirty += 1;
        // Written every few new verdicts, not on each.
        if c.dirty < 8 {
            return;
        }
        c.dirty = 0;
        c.map.as_ref().and_then(|m| serde_json::to_vec(m).ok())
    };
    if let Some(b) = snapshot {
        let tmp = data_dir.join(format!("{CACHE_FILE}.part"));
        if std::fs::write(&tmp, b).is_ok() {
            let _ = std::fs::rename(tmp, data_dir.join(CACHE_FILE));
        }
    }
}

const PICTURES: [&str; 6] = ["png", "jpg", "jpeg", "webp", "gif", "bmp"];

/// Is this local picture explicit? `None`: not something it can look at
/// (video, animated .tgs, too small). `path` is a file the app already
/// shows (decrypted media, a sticker).
#[tauri::command]
pub async fn nsfw_check(state: State<'_, Mutex<AppState>>, path: String) -> Result<Option<Verdict>, String> {
    let data_dir = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).data_dir.clone();
    let p = PathBuf::from(&path);
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if !PICTURES.contains(&ext.as_str()) {
        return Ok(None);
    }
    let Some(k) = key(&p) else { return Ok(None) };
    if let Some(v) = cached(&data_dir, &k) {
        return Ok(v);
    }
    let _permit = gate().acquire().await.map_err(|e| e.to_string())?;
    // Someone may have looked while we waited.
    if let Some(v) = cached(&data_dir, &k) {
        return Ok(v);
    }
    let v = tokio::task::spawn_blocking(move || {
        aqua_nsfw::classify_file(&p).map(|s| Verdict { explicit: s.explicit(), level: s.level() })
    })
    .await
    .map_err(|e| e.to_string())?;
    remember(&data_dir, k, v);
    Ok(v)
}
