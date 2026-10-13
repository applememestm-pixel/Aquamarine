//! Telegram sticker-pack importer.
//!
//! Ported from the old PipoHuy messenger's `TelegramStickerClient`. The
//! whole point of Aquamarine stickers is that a pack is just a folder of
//! image files under `<data_dir>/stickers/<pack>/` — so "importing" a
//! Telegram pack means: ask the Telegram Bot API for the set, download
//! each sticker file, and drop it into such a folder. Nothing about this
//! touches our own relay server; it is a pure client-side convenience.
//!
//! Flow (Telegram Bot API, needs any bot token from @BotFather):
//!   1. `GET /bot<token>/getStickerSet?name=<short_name>` → the set and
//!      its `stickers[]` (each has `file_id`, `emoji`, `is_animated`,
//!      `is_video`).
//!   2. For each sticker: `GET /bot<token>/getFile?file_id=<id>` →
//!      `result.file_path`.
//!   3. Download `https://api.telegram.org/file/bot<token>/<file_path>`.
//!   4. Write it to `<stickers_root>/<pack_title>/<emoji>_<nn>.<ext>`.
//!
//! Telegram packs come in three flavours:
//!   * static  → `.webp`  (renders everywhere in our picker)
//!   * video   → `.webm`  (saved; shown as a video sticker)
//!   * animated→ `.tgs`   (gzipped Lottie — saved but not rendered yet)
//!
//! We download all three so nothing is lost, and report how many are
//! directly renderable.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// Hard cap so a single import can't spawn thousands of HTTP requests /
/// fill the disk. Telegram packs top out at 120 stickers anyway.
const MAX_STICKERS: usize = 120;

#[derive(Debug, Clone, Serialize)]
pub struct ImportReport {
    /// Telegram short name (the `addstickers/<name>` slug).
    pub pack_name: String,
    /// Human-readable pack title.
    pub pack_title: String,
    /// Folder the stickers were written into.
    pub dir: String,
    /// Number of files actually written.
    pub imported: usize,
    /// Number that render as static images right now (`.webp`/`.png`).
    pub renderable: usize,
    /// Number of animated `.tgs` stickers saved but not yet renderable.
    pub animated: usize,
    /// Number of `.webm` video stickers saved.
    pub video: usize,
    /// Total stickers advertised by the set.
    pub total: usize,
}

/// Import a Telegram sticker pack into `stickers_root/<pack_title>/`.
///
/// `pack_ref` may be a full `t.me/addstickers/<name>` URL, a bare
/// `@name`, or just the short name. `bot_token` is any Telegram bot
/// token (from @BotFather) — it is used only to talk to Telegram and is
/// never sent to our own server.
pub async fn import_pack(
    bot_token: &str,
    pack_ref: &str,
    stickers_root: &Path,
) -> Result<ImportReport, String> {
    let token = bot_token.trim();
    if token.is_empty() {
        return Err("Telegram bot token is empty".into());
    }
    let short_name = extract_short_name(pack_ref);
    if short_name.is_empty() {
        return Err("Sticker pack name/link is empty".into());
    }

    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("http client: {e}"))?;

    // 1. getStickerSet
    let set_url = format!(
        "https://api.telegram.org/bot{token}/getStickerSet?name={}",
        urlencode(&short_name)
    );
    let set: serde_json::Value = http
        .get(&set_url)
        .send()
        .await
        .map_err(|e| format!("getStickerSet: {e}"))?
        .json()
        .await
        .map_err(|e| format!("getStickerSet decode: {e}"))?;

    if !set["ok"].as_bool().unwrap_or(false) {
        let desc = set["description"].as_str().unwrap_or("Telegram API error");
        return Err(format!("Telegram: {desc}"));
    }
    let result = &set["result"];
    let pack_name = result["name"].as_str().unwrap_or(&short_name).to_string();
    let pack_title = result["title"].as_str().unwrap_or(&short_name).to_string();
    let empty = Vec::new();
    let stickers = result["stickers"].as_array().unwrap_or(&empty);
    let total = stickers.len();

    // Destination folder: <stickers_root>/<safe title>/
    let folder = sanitize_folder(&pack_title);
    let dest = stickers_root.join(&folder);
    std::fs::create_dir_all(&dest).map_err(|e| format!("mkdir pack dir: {e}"))?;

    let mut imported = 0usize;
    let mut renderable = 0usize;
    let mut animated = 0usize;
    let mut video = 0usize;

    for (idx, st) in stickers.iter().take(MAX_STICKERS).enumerate() {
        let file_id = st["file_id"].as_str().unwrap_or("");
        if file_id.is_empty() {
            continue;
        }
        let emoji = st["emoji"].as_str().unwrap_or("").trim().to_string();
        let is_animated = st["is_animated"].as_bool().unwrap_or(false);
        let is_video = st["is_video"].as_bool().unwrap_or(false);

        // 2. getFile → file_path
        let file_url = format!(
            "https://api.telegram.org/bot{token}/getFile?file_id={}",
            urlencode(file_id)
        );
        let file_meta: serde_json::Value = match http.get(&file_url).send().await {
            Ok(r) => match r.json().await {
                Ok(v) => v,
                Err(_) => continue,
            },
            Err(_) => continue,
        };
        if !file_meta["ok"].as_bool().unwrap_or(false) {
            continue;
        }
        let file_path = file_meta["result"]["file_path"].as_str().unwrap_or("");
        if file_path.is_empty() {
            continue;
        }

        // 3. download bytes
        let dl = format!("https://api.telegram.org/file/bot{token}/{file_path}");
        let bytes = match http.get(&dl).send().await {
            Ok(r) => match r.bytes().await {
                Ok(b) => b.to_vec(),
                Err(_) => continue,
            },
            Err(_) => continue,
        };
        if bytes.is_empty() {
            continue;
        }

        // 4. write <emoji>_<nn>.<ext>
        let ext = ext_from_path(file_path, is_animated, is_video);
        let out = sticker_filename(&dest, &emoji, idx, &ext);
        if std::fs::write(&out, &bytes).is_ok() {
            imported += 1;
            match ext.as_str() {
                "tgs" => animated += 1,
                "webm" => video += 1,
                _ => renderable += 1,
            }
        }
    }

    if imported == 0 {
        // Clean up the empty folder we created.
        let _ = std::fs::remove_dir(&dest);
        return Err(format!(
            "No stickers could be downloaded from \"{pack_title}\" \
             (pack has {total}). Check the bot token and link."
        ));
    }

    // Remember what this pack is (sticker vs custom-emoji) so the picker can
    // put emoji packs in the Emoji tab and reactions can find a pack by name.
    let meta = serde_json::json!({
        "name": pack_name,
        "title": pack_title,
        "type": result["sticker_type"].as_str().unwrap_or("regular"),
    });
    let _ = std::fs::write(dest.join("pack.json"), meta.to_string());

    Ok(ImportReport {
        pack_name,
        pack_title,
        dir: dest.display().to_string(),
        imported,
        renderable,
        animated,
        video,
        total,
    })
}

// ---------- helpers -------------------------------------------------------

/// Accept `t.me/addstickers/<name>`, `telegram.me/addstickers/<name>`,
/// `@name`, or a bare short name.
pub fn extract_short_name(input: &str) -> String {
    let v = input.trim();
    if v.is_empty() {
        return String::new();
    }
    for marker in ["addstickers/", "addemoji/"] {
        if let Some(pos) = v.find(marker) {
            let rest = &v[pos + marker.len()..];
            let name = rest.split(['/', '?', '#', ' ']).next().unwrap_or("").trim();
            return name.to_string();
        }
    }
    if let Some(stripped) = v.strip_prefix('@') {
        return stripped.trim().to_string();
    }
    v.to_string()
}

/// Pick a file extension. Trust the Telegram `file_path` extension when
/// present, else fall back to the animated/video flags.
pub fn ext_from_path(file_path: &str, is_animated: bool, is_video: bool) -> String {
    if let Some(dot) = file_path.rfind('.') {
        let ext = file_path[dot + 1..].to_ascii_lowercase();
        if !ext.is_empty() && ext.len() <= 5 && ext.chars().all(|c| c.is_ascii_alphanumeric()) {
            return ext;
        }
    }
    if is_animated {
        "tgs".into()
    } else if is_video {
        "webm".into()
    } else {
        "webp".into()
    }
}

/// Build a collision-free sticker filename. If we have an emoji we use
/// the `<emoji>_<nn>.<ext>` form the scanner understands; otherwise just
/// `<nn>.<ext>`.
fn sticker_filename(dest: &Path, emoji: &str, idx: usize, ext: &str) -> PathBuf {
    let nn = format!("{:03}", idx + 1);
    let safe_emoji: String = emoji
        .chars()
        .filter(|c| !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '_'))
        .collect();
    let name = if safe_emoji.is_empty() {
        format!("{nn}.{ext}")
    } else {
        format!("{safe_emoji}_{nn}.{ext}")
    };
    dest.join(name)
}

/// Sanitize a pack title into a folder name.
pub fn sanitize_folder(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').trim();
    if trimmed.is_empty() {
        "telegram-pack".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Minimal percent-encoding for query values (names and file ids are
/// alphanumeric + `_` in practice, but be safe).
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

// ---------- tests ---------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_name_from_variants() {
        assert_eq!(extract_short_name("https://t.me/addstickers/Pepe"), "Pepe");
        assert_eq!(
            extract_short_name("t.me/addstickers/cats_by_bot?x=1"),
            "cats_by_bot"
        );
        assert_eq!(
            extract_short_name("https://telegram.me/addstickers/Foo/"),
            "Foo"
        );
        assert_eq!(extract_short_name("@MyPack"), "MyPack");
        assert_eq!(extract_short_name("  raw_name  "), "raw_name");
        assert_eq!(extract_short_name(""), "");
    }

    #[test]
    fn ext_detection() {
        assert_eq!(ext_from_path("stickers/file_1.webp", false, false), "webp");
        assert_eq!(ext_from_path("stickers/file_2.WEBM", false, true), "webm");
        assert_eq!(ext_from_path("stickers/file_3.tgs", true, false), "tgs");
        // no extension → fall back to flags
        assert_eq!(ext_from_path("stickers/file_4", true, false), "tgs");
        assert_eq!(ext_from_path("stickers/file_5", false, true), "webm");
        assert_eq!(ext_from_path("stickers/file_6", false, false), "webp");
    }

    #[test]
    fn folder_sanitization() {
        assert_eq!(sanitize_folder("My Pack"), "My Pack");
        assert_eq!(sanitize_folder("a/b:c"), "a_b_c");
        assert_eq!(sanitize_folder("   "), "telegram-pack");
    }

    #[test]
    fn filename_with_and_without_emoji() {
        let d = Path::new("/tmp/x");
        assert_eq!(
            sticker_filename(d, "😀", 0, "webp").file_name().unwrap(),
            "😀_001.webp"
        );
        assert_eq!(
            sticker_filename(d, "", 4, "tgs").file_name().unwrap(),
            "005.tgs"
        );
    }

    #[test]
    fn urlencode_basic() {
        assert_eq!(urlencode("abc_123"), "abc_123");
        assert_eq!(urlencode("a b"), "a%20b");
    }
}
