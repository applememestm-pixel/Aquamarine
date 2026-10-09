//! Local media library — stickers and GIFs that live as plain files
//! in the user's data directory. The whole philosophy is: no cloud,
//! no DRM, no vendor lock-in — just drop files into folders and they
//! show up in the picker.
//!
//! Layout under `<data_dir>/`:
//!
//! ```text
//! stickers/
//!     <pack-name-1>/
//!         😀_grin.png          ← optional "emoji_" prefix sets the sticker's emoji
//!         sad.webp
//!         wave.gif             ← animated stickers are just GIFs
//!     <pack-name-2>/
//!         ...
//! gifs/
//!     reaction.gif
//!     cat.gif
//! ```
//!
//! README files are written on first launch to explain this layout
//! inline.

use std::{fs, path::Path};

use serde::Serialize;

const README_STICKERS: &str = r#"# Aquamarine stickers

Drop PNG, WEBP or GIF files into subfolders here. Each subfolder
becomes a sticker pack whose name is the folder name.

Example:
    stickers/pepe/happy.png
    stickers/pepe/sad.png
    stickers/kappa/wave.webp

Files can optionally be named `<emoji>_name.ext` — the emoji before the
first underscore becomes the sticker's emoji tag. Example:
    stickers/mypack/😀_grin.png

The app picks up new files on next launch (or when you reload the
media picker). Nothing you drop here leaves your device until you
actually send a sticker — then only that one file is encrypted and
uploaded, same way as any other attachment.
"#;

const README_GIFS: &str = r#"# Aquamarine GIFs

Drop .gif files here. They appear in the GIF picker when you click
the sticker icon in the composer and switch to the GIFs tab.

Same privacy as everything else in this app: files never leave your
device until you send one, and sent ones go through the same
end-to-end-encrypted path as text and other attachments.
"#;

// ---------- Public API ----------------------------------------------------

#[derive(Debug, Serialize, Clone)]
pub struct StickerItem {
    pub id: String,
    pub path: String,
    pub emoji: Option<String>,
    pub mime: String,
    pub size: u64,
}

#[derive(Debug, Serialize, Clone)]
pub struct StickerPack {
    pub id: String,
    pub title: String,
    /// "sticker" or "emoji" (Telegram custom-emoji packs). From the pack's
    /// `pack.json`, written by the Telegram importer; defaults to "sticker".
    pub kind: String,
    /// Telegram short name (`t.me/addemoji/<name>`), when imported.
    pub source: Option<String>,
    pub items: Vec<StickerItem>,
}

#[derive(Debug, Serialize, Clone)]
pub struct GifItem {
    pub id: String,
    pub path: String,
    pub name: String,
    pub mime: String,
    pub size: u64,
}

/// Optional `<pack>/pack.json`.
#[derive(Debug, serde::Deserialize, Default)]
struct PackMeta {
    #[serde(default)]
    name: Option<String>,
    /// Telegram `sticker_type`: "regular" | "mask" | "custom_emoji".
    #[serde(default, rename = "type")]
    kind: Option<String>,
}

fn read_pack_meta(pack_dir: &Path) -> PackMeta {
    fs::read_to_string(pack_dir.join("pack.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Largest file `add_files` will copy into the library.
pub const ADD_LIMIT: u64 = 25 * 1024 * 1024;

/// Copy user files into the library: `target` = "gifs" or "stickers"
/// (then into `pack` — created if missing, default "My stickers"), or
/// "emoji" (pack default "My emoji", marked as an emoji pack). Returns how
/// many files were added. Unsupported / oversized files are skipped.
pub fn add_files(data_dir: &Path, target: &str, pack: Option<&str>, paths: &[String]) -> Result<usize, String> {
    let dest = match target {
        "gifs" => data_dir.join("gifs"),
        "stickers" | "emoji" => {
            let default = if target == "emoji" { "My emoji" } else { "My stickers" };
            let title = sanitize_name(pack.unwrap_or(default));
            let dir = data_dir.join("stickers").join(&title);
            fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {e}"))?;
            if target == "emoji" && !dir.join("pack.json").exists() {
                let _ = fs::write(dir.join("pack.json"), r#"{"type":"custom_emoji"}"#);
            }
            dir
        }
        _ => return Err(format!("unknown target {target:?}")),
    };
    fs::create_dir_all(&dest).map_err(|e| format!("mkdir: {e}"))?;
    let mut added = 0;
    for p in paths {
        let src = Path::new(p);
        let Some(name) = src.file_name().and_then(|n| n.to_str()) else { continue };
        let ok = if target == "gifs" { is_gif_like(name) } else { classify(name).1 };
        if !ok || !src.is_file() {
            continue;
        }
        if fs::metadata(src).map(|m| m.len() > ADD_LIMIT).unwrap_or(true) {
            continue;
        }
        let out = unique_path(&dest, &sanitize_name(name));
        if fs::copy(src, &out).is_ok() {
            added += 1;
        }
    }
    Ok(added)
}

fn is_gif_like(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    l.ends_with(".gif") || l.ends_with(".mp4") || l.ends_with(".webm")
}

fn sanitize_name(s: &str) -> String {
    let c: String = s
        .chars()
        .map(|c| match c { '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_', _ => c })
        .collect();
    let t = c.trim().trim_matches('.').trim();
    if t.is_empty() { "untitled".into() } else { t.to_string() }
}

fn unique_path(dir: &Path, name: &str) -> std::path::PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) => (&name[..i], &name[i..]),
        None => (name, ""),
    };
    for n in 2..10_000 {
        let p = dir.join(format!("{stem} ({n}){ext}"));
        if !p.exists() {
            return p;
        }
    }
    dir.join(format!("{stem}-{}{ext}", std::process::id()))
}

#[derive(Debug, Serialize, Clone)]
pub struct LocalMediaList {
    pub packs: Vec<StickerPack>,
    pub gifs: Vec<GifItem>,
    pub stickers_dir: String,
    pub gifs_dir: String,
}

/// Create the stickers/ and gifs/ subdirs under data_dir with README
/// files. Idempotent — safe to call on every boot.
pub fn ensure_dirs(data_dir: &Path) {
    let stickers = data_dir.join("stickers");
    let gifs = data_dir.join("gifs");
    let _ = fs::create_dir_all(&stickers);
    let _ = fs::create_dir_all(&gifs);
    let s_readme = stickers.join("README.md");
    if !s_readme.exists() {
        let _ = fs::write(&s_readme, README_STICKERS);
    }
    let g_readme = gifs.join("README.md");
    if !g_readme.exists() {
        let _ = fs::write(&g_readme, README_GIFS);
    }
}

/// Scan the media directories and build the picker catalogue.
pub fn scan(data_dir: &Path) -> LocalMediaList {
    let stickers_dir = data_dir.join("stickers");
    let gifs_dir = data_dir.join("gifs");

    let packs = scan_sticker_packs(&stickers_dir);
    let gifs = scan_gifs(&gifs_dir);

    LocalMediaList {
        packs,
        gifs,
        stickers_dir: stickers_dir.display().to_string(),
        gifs_dir: gifs_dir.display().to_string(),
    }
}

/// Verify that `candidate` is a file inside `allowed_root`. Follows
/// symlinks and refuses `..` traversal — defends the
/// `read_media_bytes` command against a hostile frontend asking us to
/// serve `C:\Windows\System32\...`.
pub fn path_is_within(allowed_root: &Path, candidate: &Path) -> bool {
    let root = match allowed_root.canonicalize() {
        Ok(p) => p,
        Err(_) => return false,
    };
    let cand = match candidate.canonicalize() {
        Ok(p) => p,
        Err(_) => return false,
    };
    cand.starts_with(root)
}

// ---------- Scanning ------------------------------------------------------

fn scan_sticker_packs(dir: &Path) -> Vec<StickerPack> {
    let mut packs = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else { return packs };
    for entry in entries.flatten() {
        let pack_dir = entry.path();
        if !pack_dir.is_dir() {
            continue;
        }
        let title = pack_dir
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        if title.is_empty() || title.starts_with('.') {
            continue;
        }
        let items = scan_sticker_items(&pack_dir, &title);
        if items.is_empty() {
            continue;
        }
        let meta = read_pack_meta(&pack_dir);
        let kind = if meta.kind.as_deref() == Some("custom_emoji") { "emoji" } else { "sticker" };
        packs.push(StickerPack {
            id: format!("pack:{}", slugify(&title)),
            title,
            kind: kind.into(),
            source: meta.name,
            items,
        });
    }
    packs.sort_by(|a, b| a.title.cmp(&b.title));
    packs
}

fn scan_sticker_items(pack_dir: &Path, pack_title: &str) -> Vec<StickerItem> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(pack_dir) else { return out };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let filename = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        let (mime, is_media) = classify(filename);
        if !is_media {
            continue;
        }
        let (emoji, _display) = parse_emoji_prefix(filename);
        let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        out.push(StickerItem {
            id: format!("pack:{}:{}", slugify(pack_title), filename),
            path: path.display().to_string(),
            emoji,
            mime,
            size,
        });
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

fn scan_gifs(dir: &Path) -> Vec<GifItem> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else { return out };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let filename = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if !is_gif_like(filename) {
            continue;
        }
        let lower = filename.to_ascii_lowercase();
        let mime = if lower.ends_with(".mp4") { "video/mp4" } else if lower.ends_with(".webm") { "video/webm" } else { "image/gif" };
        let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        out.push(GifItem {
            id: format!("gif:{filename}"),
            path: path.display().to_string(),
            name: filename.to_string(),
            mime: mime.into(),
            size,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

// ---------- Helpers -------------------------------------------------------

fn classify(filename: &str) -> (String, bool) {
    let lower = filename.to_ascii_lowercase();
    let (mime, is_media) = if lower.ends_with(".png") {
        ("image/png", true)
    } else if lower.ends_with(".webp") {
        ("image/webp", true)
    } else if lower.ends_with(".gif") {
        ("image/gif", true)
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        ("image/jpeg", true)
    } else if lower.ends_with(".svg") {
        ("image/svg+xml", true)
    } else if lower.ends_with(".tgs") {
        // Telegram animated sticker / emoji: gzipped Lottie JSON.
        ("application/x-tgsticker", true)
    } else if lower.ends_with(".webm") {
        ("video/webm", true)
    } else {
        ("application/octet-stream", false)
    };
    (mime.to_string(), is_media)
}

/// Split off an optional leading emoji prefix from a filename. Emoji
/// prefix is anything before the first `_`, if the prefix is 1–8
/// characters and contains at least one non-ASCII byte (a crude but
/// serviceable test for an emoji).
fn parse_emoji_prefix(filename: &str) -> (Option<String>, String) {
    if let Some(underscore_idx) = filename.find('_') {
        let prefix = &filename[..underscore_idx];
        if !prefix.is_empty()
            && prefix.chars().count() <= 8
            && prefix.bytes().any(|b| b >= 0x80)
        {
            return (Some(prefix.to_string()), filename[underscore_idx + 1..].to_string());
        }
    }
    (None, filename.to_string())
}

fn slugify(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

// ---------- Tests ---------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn ensure_dirs_creates_expected_tree() {
        let td = tempdir().unwrap();
        ensure_dirs(td.path());
        assert!(td.path().join("stickers").is_dir());
        assert!(td.path().join("stickers/README.md").is_file());
        assert!(td.path().join("gifs").is_dir());
        assert!(td.path().join("gifs/README.md").is_file());
    }

    #[test]
    fn scan_picks_up_dropped_files() {
        let td = tempdir().unwrap();
        ensure_dirs(td.path());
        let pack = td.path().join("stickers/pepe");
        fs::create_dir_all(&pack).unwrap();
        fs::write(pack.join("😀_happy.png"), b"\x89PNG\r\n").unwrap();
        fs::write(pack.join("sad.png"), b"\x89PNG\r\n").unwrap();
        fs::write(td.path().join("gifs/dance.gif"), b"GIF89a").unwrap();
        fs::write(td.path().join("gifs/notes.txt"), b"ignored").unwrap();

        let list = scan(td.path());
        assert_eq!(list.packs.len(), 1);
        assert_eq!(list.packs[0].title, "pepe");
        assert_eq!(list.packs[0].items.len(), 2);
        let with_emoji = list.packs[0].items.iter().find(|i| i.emoji.is_some()).unwrap();
        assert_eq!(with_emoji.emoji.as_deref(), Some("😀"));
        assert_eq!(list.gifs.len(), 1);
        assert_eq!(list.gifs[0].name, "dance.gif");
        assert_eq!(list.packs[0].kind, "sticker");
    }

    #[test]
    fn emoji_packs_animated_files_and_add_files() {
        let td = tempdir().unwrap();
        ensure_dirs(td.path());
        // An imported Telegram custom-emoji pack with animated + video items.
        let pack = td.path().join("stickers/El Taller");
        fs::create_dir_all(&pack).unwrap();
        fs::write(pack.join("pack.json"), r#"{"name":"eltaller47","title":"El Taller","type":"custom_emoji"}"#).unwrap();
        fs::write(pack.join("👍_001.tgs"), b"\x1f\x8b").unwrap();
        fs::write(pack.join("❤_002.webm"), b"webm").unwrap();
        let list = scan(td.path());
        let p = &list.packs[0];
        assert_eq!(p.kind, "emoji");
        assert_eq!(p.source.as_deref(), Some("eltaller47"));
        assert_eq!(p.items.len(), 2, "pack.json is not an item; tgs + webm are");
        assert!(p.items.iter().any(|i| i.mime == "application/x-tgsticker" && i.emoji.as_deref() == Some("👍")));

        // add_files: gif-like into gifs/, images into a new emoji pack, junk skipped.
        let src = td.path().join("src");
        fs::create_dir_all(&src).unwrap();
        for n in ["a.gif", "b.mp4", "c.txt", "d.png"] { fs::write(src.join(n), b"x").unwrap(); }
        let paths: Vec<String> = ["a.gif", "b.mp4", "c.txt"].iter().map(|n| src.join(n).display().to_string()).collect();
        assert_eq!(add_files(td.path(), "gifs", None, &paths).unwrap(), 2);
        assert_eq!(add_files(td.path(), "gifs", None, &paths[..1]).unwrap(), 1, "duplicate name gets a suffix");
        let gifs = scan(td.path()).gifs;
        assert_eq!(gifs.len(), 3);
        assert!(gifs.iter().any(|g| g.mime == "video/mp4"));
        let png = vec![src.join("d.png").display().to_string()];
        assert_eq!(add_files(td.path(), "emoji", Some("Mine"), &png).unwrap(), 1);
        let packs = scan(td.path()).packs;
        assert_eq!(packs.iter().find(|p| p.title == "Mine").unwrap().kind, "emoji");
        assert!(add_files(td.path(), "nope", None, &png).is_err());
    }

    #[test]
    fn path_within_rejects_traversal() {
        let td = tempdir().unwrap();
        let inside = td.path().join("ok.txt");
        fs::write(&inside, b"x").unwrap();
        assert!(path_is_within(td.path(), &inside));

        let outside = std::env::temp_dir().join("..").join("outside.txt");
        assert!(!path_is_within(td.path(), &outside));
    }
}
