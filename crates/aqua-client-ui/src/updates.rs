//! Updates from GitHub Releases (Settings → Updates; the banner on start).
//!
//! Two channels, the same as release.yml builds them: "nightly" — the
//! pre-release rebuilt from every push to main — and "stable", the latest
//! v* release. The UI knows which build it is (vite.config.ts bakes in the
//! commit and version) and compares; this side finds the release, picks the
//! file for this platform, downloads it, checks it against SHA256SUMS.txt
//! and starts the install:
//!   * Android — the system installer (AquaUpdatePlugin in android/AquaRelay.kt);
//!     Android only installs it over the current app when both are signed
//!     with the same key (release.yml: ANDROID_KEYSTORE_B64);
//!   * Windows — runs the NSIS installer and closes the app;
//!   * Linux AppImage — swaps the AppImage file and starts the new one;
//!   * macOS and other Linux packages — opens the downloaded file.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};

const REPO: &str = "applememestm-pixel/Aquamarine";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateAsset {
    pub name: String,
    pub url: String,
    pub size: u64,
    /// From SHA256SUMS.txt — only when that file is newer than this one
    /// (a nightly build still uploading has old sums).
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateInfo {
    pub tag: String,
    pub name: String,
    /// Short commit of a nightly ("Aquamarine nightly (def5b3d)").
    pub commit: Option<String>,
    /// Version of a stable release ("v1.0.2" → "1.0.2").
    pub version: Option<String>,
    pub published_at: String,
    pub notes: String,
    pub html_url: String,
    /// The file for this platform; None while it isn't built yet.
    pub asset: Option<UpdateAsset>,
}

/// The release file this platform installs.
fn wanted_assets() -> &'static [&'static str] {
    if cfg!(target_os = "android") {
        &["Aquamarine.apk"]
    } else if cfg!(target_os = "windows") {
        &["Aquamarine-Setup.exe"]
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        &["Aquamarine.dmg"]
    } else if cfg!(target_os = "macos") {
        &["Aquamarine-Intel.dmg"]
    } else if cfg!(target_os = "linux") {
        &["Aquamarine.AppImage"]
    } else {
        &[]
    }
}

async fn get_json(http: &reqwest::Client, url: &str) -> Result<serde_json::Value, String> {
    let r = http
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = r.status();
    let body = r.bytes().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("GitHub: HTTP {status}"));
    }
    serde_json::from_slice(&body).map_err(|e| e.to_string())
}

fn short_commit(name: &str) -> Option<String> {
    let open = name.rfind('(')?;
    let inner = name[open + 1..].trim_end().strip_suffix(')')?;
    (inner.len() >= 7 && inner.chars().all(|c| c.is_ascii_hexdigit())).then(|| inner.to_ascii_lowercase())
}

fn parse_sums(text: &str, file: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let mut it = l.split_whitespace();
        let hash = it.next()?;
        let name = it.next()?.trim_start_matches('*');
        (name == file && hash.len() == 64).then(|| hash.to_ascii_lowercase())
    })
}

/// The newest release of `channel` ("nightly" | "stable") and its file for
/// this platform.
#[tauri::command]
pub async fn update_check(channel: String) -> Result<UpdateInfo, String> {
    let http = aqua_client_core::transport::http_client().map_err(|e| e.to_string())?;
    let url = if channel == "stable" {
        format!("https://api.github.com/repos/{REPO}/releases/latest")
    } else {
        format!("https://api.github.com/repos/{REPO}/releases/tags/nightly")
    };
    let rel = get_json(&http, &url).await?;
    let s = |k: &str| rel[k].as_str().unwrap_or_default().to_string();
    let (tag, name) = (s("tag_name"), s("name"));
    let assets = rel["assets"].as_array().cloned().unwrap_or_default();
    let find = |n: &str| assets.iter().find(|a| a["name"].as_str() == Some(n)).cloned();
    let mut asset = None;
    // Which commit each file really is (release.yml writes BUILDS.json): a
    // nightly whose build for this platform failed keeps the older file
    // under the new title.
    let mut built_from: Option<String> = None;
    for want in wanted_assets() {
        if let Some(a) = find(want) {
            let updated = a["updated_at"].as_str().unwrap_or_default().to_string();
            let mut sha256 = None;
            if let Some(sums) = find("SHA256SUMS.txt") {
                // ISO 8601 in UTC: string order is time order.
                if sums["updated_at"].as_str().unwrap_or_default() >= updated.as_str() {
                    if let Some(u) = sums["browser_download_url"].as_str() {
                        if let Ok(r) = http.get(u).timeout(std::time::Duration::from_secs(20)).send().await {
                            if let Ok(t) = r.text().await {
                                sha256 = parse_sums(&t, want);
                            }
                        }
                    }
                }
            }
            if let Some(u) = find("BUILDS.json").and_then(|b| b["browser_download_url"].as_str().map(str::to_string)) {
                if let Ok(r) = http.get(u).timeout(std::time::Duration::from_secs(20)).send().await {
                    if let Ok(v) = r.json::<serde_json::Value>().await {
                        built_from = v[*want].as_str().map(|c| c.to_ascii_lowercase());
                    }
                }
            }
            asset = Some(UpdateAsset {
                name: want.to_string(),
                url: a["browser_download_url"].as_str().unwrap_or_default().to_string(),
                size: a["size"].as_u64().unwrap_or(0),
                sha256,
            });
            break;
        }
    }
    Ok(UpdateInfo {
        commit: if channel == "stable" { None } else { built_from.or_else(|| short_commit(&name)) },
        version: (channel == "stable").then(|| tag.trim_start_matches('v').to_string()),
        published_at: s("published_at"),
        notes: s("body"),
        html_url: s("html_url"),
        tag,
        name,
        asset,
    })
}

fn updates_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let d = app.path().app_cache_dir().map_err(|e| e.to_string())?.join("updates");
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    Ok(d)
}

#[derive(Clone, Serialize)]
struct Progress {
    done: u64,
    total: u64,
}

/// Download the update and check its SHA-256 (a broken or cut-off download
/// never reaches the installer). Progress: `aqua://update-progress`.
#[tauri::command]
pub async fn update_download(app: AppHandle, asset: UpdateAsset) -> Result<String, String> {
    use std::io::Write;
    let expected = asset.sha256.clone().ok_or("this build is still being uploaded — try again in a few minutes")?;
    if !asset.url.starts_with("https://github.com/") || asset.name.contains(['/', '\\']) {
        return Err("unexpected update address".into());
    }
    let dir = updates_dir(&app)?;
    // Only the newest download is kept.
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let _ = std::fs::remove_file(e.path());
        }
    }
    let path = dir.join(&asset.name);
    let part = dir.join(format!("{}.part", asset.name));
    let http = aqua_client_core::transport::http_client().map_err(|e| e.to_string())?;
    let mut r = http.get(&asset.url).send().await.map_err(|e| e.to_string())?;
    if !r.status().is_success() {
        return Err(format!("download: HTTP {}", r.status()));
    }
    let total = r.content_length().unwrap_or(asset.size);
    let mut f = std::fs::File::create(&part).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let (mut done, mut last) = (0u64, 0u64);
    while let Some(chunk) = r.chunk().await.map_err(|e| e.to_string())? {
        f.write_all(&chunk).map_err(|e| e.to_string())?;
        hasher.update(&chunk);
        done += chunk.len() as u64;
        if done - last >= 512 * 1024 || done == total {
            last = done;
            let _ = app.emit("aqua://update-progress", Progress { done, total });
        }
    }
    f.flush().map_err(|e| e.to_string())?;
    drop(f);
    let got = hex::encode(hasher.finalize());
    if got != expected {
        let _ = std::fs::remove_file(&part);
        return Err(format!("the download is damaged (SHA-256 {}… instead of {}…) — try again", &got[..12], &expected[..12]));
    }
    std::fs::rename(&part, &path).map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

/// Install a file `update_download` fetched.
#[tauri::command]
pub async fn update_install(app: AppHandle, path: String) -> Result<String, String> {
    let p = PathBuf::from(&path);
    let dir = updates_dir(&app)?;
    if p.parent() != Some(dir.as_path()) || !p.is_file() {
        return Err("not a downloaded update".into());
    }
    #[cfg(target_os = "android")]
    {
        let r = tauri::async_runtime::spawn_blocking(move || android::call("install", serde_json::json!({ "path": path })))
            .await
            .map_err(|e| e.to_string())??;
        return Ok(if r["needsPermission"].as_bool().unwrap_or(false) { "permission".into() } else { "started".into() });
    }
    #[cfg(target_os = "windows")]
    {
        // The NSIS installer replaces the app in place: let it, and step aside.
        std::process::Command::new(&p).spawn().map_err(|e| e.to_string())?;
        app.exit(0);
        return Ok("started".into());
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(img) = std::env::var_os("APPIMAGE").map(PathBuf::from) {
            // Replacing the file is safe while the old one runs (it's mounted).
            let tmp = img.with_extension("AppImage.new");
            std::fs::copy(&p, &tmp).map_err(|e| e.to_string())?;
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
            std::fs::rename(&tmp, &img).map_err(|e| e.to_string())?;
            std::process::Command::new(&img).spawn().map_err(|e| e.to_string())?;
            app.exit(0);
            return Ok("started".into());
        }
        std::process::Command::new("xdg-open").arg(&p).spawn().map_err(|e| e.to_string())?;
        return Ok("opened".into());
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(&p).spawn().map_err(|e| e.to_string())?;
        return Ok("opened".into());
    }
    #[allow(unreachable_code)]
    {
        let _ = (app, p);
        Err("updates can't be installed on this platform".into())
    }
}

#[cfg(target_os = "android")]
pub mod android {
    use std::sync::OnceLock;
    use tauri::plugin::{Builder, PluginHandle, TauriPlugin};
    use tauri::Wry;

    static HANDLE: OnceLock<PluginHandle<Wry>> = OnceLock::new();

    pub fn plugin() -> TauriPlugin<Wry> {
        Builder::new("aquaupdate")
            .setup(|_app, api| {
                match api.register_android_plugin("app.aquamarine.messenger", "AquaUpdatePlugin") {
                    Ok(h) => { let _ = HANDLE.set(h); }
                    Err(e) => tracing::warn!(?e, "updates: Android plugin missing"),
                }
                Ok(())
            })
            .build()
    }

    pub fn call(cmd: &str, args: serde_json::Value) -> Result<serde_json::Value, String> {
        HANDLE.get().ok_or("update plugin not loaded")?.run_mobile_plugin::<serde_json::Value>(cmd, args).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_from_name() {
        assert_eq!(short_commit("Aquamarine nightly (def5b3d)").as_deref(), Some("def5b3d"));
        assert_eq!(short_commit("Aquamarine 1.0.1"), None);
    }

    #[test]
    fn sums() {
        let t = "6068967c7bc0a169be43cfd4561080b57b2fe59189710f1ae73d47f777e0b4d9  Aquamarine.apk\nabc  x\n";
        assert_eq!(parse_sums(t, "Aquamarine.apk").as_deref(), Some("6068967c7bc0a169be43cfd4561080b57b2fe59189710f1ae73d47f777e0b4d9"));
        assert_eq!(parse_sums(t, "Aquamarine.AppImage"), None);
    }
}
