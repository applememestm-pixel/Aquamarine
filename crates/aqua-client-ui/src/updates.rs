//! Updates from GitHub Releases (Settings → Updates; the banner on start).
//!
//! Two channels, the same as release.yml builds them: "nightly" (every
//! build: the newest published release, numbered pre-releases included) — the
//! pre-release rebuilt from every push to main — and "stable", the latest
//! v* release. The UI knows which build it is (vite.config.ts bakes in the
//! commit and version) and compares; this side finds the release, picks the
//! file for this platform, downloads it, checks it against SHA256SUMS.txt
//! and starts the install:
//!   * Android — the system installer (AquaUpdatePlugin in android/AquaRelay.kt);
//!     Android only installs it over the current app when both are signed
//!     with the same key (release.yml: ANDROID_KEYSTORE_B64);
//!   * Windows — the NSIS installer runs silently (`/S`) and the new version starts;
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
///
/// GitHub's API allows 60 requests an hour per address without a login, and
/// phones on mobile networks share one address with thousands of others: the
/// API answers "403 Forbidden" there for hours — and so do github.com's own
/// pages (the releases feed). So next comes `updates.json` on the project
/// site: GitHub Pages behind a CDN, no such limit; pages.yml writes it after
/// every release run, with the same release objects the API gives plus the
/// checksums already read. github.com pages are the last resort.
#[tauri::command]
pub async fn update_check(channel: String) -> Result<UpdateInfo, String> {
    let http = aqua_client_core::transport::http_client().map_err(|e| e.to_string())?;
    let api_err = match api_check(&http, &channel).await {
        Ok(i) => return Ok(i),
        Err(e) => e,
    };
    let feed_err = match feed_check(&http, &channel).await {
        Ok(i) => return Ok(i),
        Err(e) => e,
    };
    web_check(&http, &channel).await.map_err(|e| format!("{api_err}; {feed_err}; {e}"))
}

/// Where pages.yml puts the feed: the site's own domain, then the
/// github.io address (in case the domain is down or blocked).
const FEEDS: &[&str] = &["https://aquamarinemessenger.pw/updates.json", "https://applememestm-pixel.github.io/Aquamarine/updates.json"];

async fn feed_check(http: &reqwest::Client, channel: &str) -> Result<UpdateInfo, String> {
    let mut last = String::from("site: no feed");
    for url in FEEDS {
        let r = match http.get(*url).header("Cache-Control", "no-cache").timeout(std::time::Duration::from_secs(20)).send().await {
            Ok(r) => r,
            Err(e) => {
                last = format!("site: {}", e.without_url());
                continue;
            }
        };
        if !r.status().is_success() {
            last = format!("site: HTTP {}", r.status());
            continue;
        }
        let feed: serde_json::Value = match r.json().await {
            Ok(v) => v,
            Err(e) => {
                last = format!("site: {}", e.without_url());
                continue;
            }
        };
        let rel = &feed[if channel == "stable" { "stable" } else { "latest" }];
        if !rel.is_object() {
            return Err("no releases yet".into());
        }
        return release_info(http, rel, channel).await;
    }
    Err(last)
}

/// Text of the first `<tag>…</tag>` in `s`.
fn xml_tag<'a>(s: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}");
    let i = s.find(&open)?;
    let j = i + s[i..].find('>')? + 1;
    let k = j + s[j..].find(&format!("</{tag}>"))?;
    Some(&s[j..k])
}

fn xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&amp;", "&")
}

/// The same answer as `api_check`, from github.com pages instead of the API.
async fn web_check(http: &reqwest::Client, channel: &str) -> Result<UpdateInfo, String> {
    let base = format!("https://github.com/{REPO}");
    let get = |u: String| {
        let http = http.clone();
        async move {
            let r = http.get(&u).timeout(std::time::Duration::from_secs(20)).send().await.map_err(|e| e.without_url().to_string())?;
            if !r.status().is_success() {
                return Err(format!("github.com: HTTP {}", r.status()));
            }
            Ok::<reqwest::Response, String>(r)
        }
    };
    let (tag, name, published_at) = if channel == "stable" {
        // …/releases/latest redirects to …/releases/tag/<tag>.
        let r = get(format!("{base}/releases/latest")).await?;
        let tag = r.url().path().rsplit_once("/tag/").map(|(_, t)| t.to_string()).ok_or("no releases yet")?;
        (tag.clone(), tag, String::new())
    } else {
        // The feed lists every release, pre-releases too, newest first.
        let feed = get(format!("{base}/releases.atom")).await?.text().await.map_err(|e| e.to_string())?;
        let entry = xml_tag(&feed, "entry").ok_or("no releases yet")?;
        let link = entry.split("href=\"").nth(1).and_then(|r| r.split('"').next()).unwrap_or_default();
        let tag = link.rsplit_once("/tag/").map(|(_, t)| t.to_string()).ok_or("no releases yet")?;
        let name = xml_unescape(xml_tag(entry, "title").unwrap_or(&tag));
        (tag, name, xml_tag(entry, "updated").unwrap_or_default().to_string())
    };
    let dl = |f: &str| format!("{base}/releases/download/{tag}/{f}");
    let sums = match get(dl("SHA256SUMS.txt")).await {
        Ok(r) => r.text().await.unwrap_or_default(),
        Err(_) => String::new(),
    };
    let builds: Option<serde_json::Value> = match get(dl("BUILDS.json")).await {
        Ok(r) => r.json().await.ok(),
        Err(_) => None,
    };
    let mut asset = None;
    let mut built_from = None;
    for want in wanted_assets() {
        // Without the API there's no file list: a file is there when the
        // checksum list names it.
        let Some(sha) = parse_sums(&sums, want) else { continue };
        // The size: one byte asked for, the total from Content-Range.
        let size = match http.get(dl(want)).header(reqwest::header::RANGE, "bytes=0-0").timeout(std::time::Duration::from_secs(20)).send().await {
            Ok(r) => r.headers().get(reqwest::header::CONTENT_RANGE).and_then(|v| v.to_str().ok())
                .and_then(|v| v.rsplit('/').next()).and_then(|n| n.parse().ok())
                .or_else(|| r.content_length().filter(|&n| n > 1)).unwrap_or(0),
            Err(_) => 0,
        };
        built_from = builds.as_ref().and_then(|b| b[*want].as_str()).map(|c| c.to_ascii_lowercase());
        asset = Some(UpdateAsset { name: want.to_string(), url: dl(want), size, sha256: Some(sha) });
        break;
    }
    Ok(UpdateInfo {
        commit: if channel == "stable" { None } else { built_from.or_else(|| short_commit(&name)) },
        version: tag.starts_with('v').then(|| tag.trim_start_matches('v').to_string()),
        published_at,
        notes: String::new(),
        html_url: format!("{base}/releases/tag/{tag}"),
        tag,
        name,
        asset,
    })
}

async fn api_check(http: &reqwest::Client, channel: &str) -> Result<UpdateInfo, String> {
    let http = http.clone();
    let rel = if channel == "stable" {
        get_json(&http, &format!("https://api.github.com/repos/{REPO}/releases/latest")).await?
    } else {
        // Every build: the most recently published release — a numbered
        // pre-release (v1.0.61…), a stable one, or the old moving "nightly".
        let list = get_json(&http, &format!("https://api.github.com/repos/{REPO}/releases?per_page=15")).await?;
        list.as_array()
            .into_iter()
            .flatten()
            .filter(|r| !r["draft"].as_bool().unwrap_or(false))
            .max_by(|a, b| a["published_at"].as_str().unwrap_or_default().cmp(b["published_at"].as_str().unwrap_or_default()))
            .cloned()
            .ok_or("no releases yet")?
    };
    release_info(&http, &rel, channel).await
}

/// UpdateInfo from a release object (the API's, or the feed's — which also
/// carries `sums`, the text of SHA256SUMS.txt, and `builds`, BUILDS.json, so
/// nothing more is fetched).
async fn release_info(http: &reqwest::Client, rel: &serde_json::Value, channel: &str) -> Result<UpdateInfo, String> {
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
                    if let Some(t) = rel["sums"].as_str() {
                        sha256 = parse_sums(t, want);
                    } else if let Some(u) = sums["browser_download_url"].as_str() {
                        if let Ok(r) = http.get(u).timeout(std::time::Duration::from_secs(20)).send().await {
                            if let Ok(t) = r.text().await {
                                sha256 = parse_sums(&t, want);
                            }
                        }
                    }
                }
            }
            if rel["builds"].is_object() {
                built_from = rel["builds"][*want].as_str().map(|c| c.to_ascii_lowercase());
            } else if let Some(u) = find("BUILDS.json").and_then(|b| b["browser_download_url"].as_str().map(str::to_string)) {
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
        version: tag.starts_with('v').then(|| tag.trim_start_matches('v').to_string()),
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
    let path = dir.join(&asset.name);
    let part = dir.join(format!("{}.part", asset.name));
    let tag = dir.join(format!("{}.part.sha256", asset.name));
    // Already downloaded and intact: nothing to fetch.
    if let Some(h) = file_sha256(&path) {
        if h == expected {
            return Ok(path.display().to_string());
        }
    }
    // Only this download is kept: older builds and other leftovers go. A
    // part of *this* build stays, and the download resumes where it stopped.
    let resumable = std::fs::read_to_string(&tag).map(|t| t.trim() == expected).unwrap_or(false);
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let p = e.path();
            if resumable && (p == part || p == tag) {
                continue;
            }
            let _ = std::fs::remove_file(p);
        }
    }
    let _ = std::fs::write(&tag, &expected);
    let have = if resumable { std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0) } else { 0 };
    let http = aqua_client_core::transport::http_client().map_err(|e| e.to_string())?;
    let mut req = http.get(&asset.url);
    if have > 0 {
        req = req.header("Range", format!("bytes={have}-"));
    }
    let mut r = req.send().await.map_err(|e| e.to_string())?;
    if !r.status().is_success() {
        return Err(format!("download: HTTP {}", r.status()));
    }
    let resumed = have > 0 && r.status().as_u16() == 206;
    let mut hasher = Sha256::new();
    let mut done = 0u64;
    let mut f = if resumed {
        // Hash what's already there, then append.
        let mut old = std::fs::File::open(&part).map_err(|e| e.to_string())?;
        std::io::copy(&mut old, &mut HashWriter(&mut hasher)).map_err(|e| e.to_string())?;
        done = have;
        std::fs::OpenOptions::new().append(true).open(&part).map_err(|e| e.to_string())?
    } else {
        std::fs::File::create(&part).map_err(|e| e.to_string())?
    };
    let total = r.content_length().map(|n| n + done).unwrap_or(asset.size);
    let mut last = 0u64;
    let _ = app.emit("aqua://update-progress", Progress { done, total });
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
        let _ = std::fs::remove_file(&tag);
        return Err(format!("the download is damaged (SHA-256 {}… instead of {}…) — try again", &got[..12], &expected[..12]));
    }
    let _ = std::fs::remove_file(&tag);
    std::fs::rename(&part, &path).map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

/// SHA-256 of a file, if it can be read.
fn file_sha256(p: &std::path::Path) -> Option<String> {
    let mut f = std::fs::File::open(p).ok()?;
    let mut h = Sha256::new();
    std::io::copy(&mut f, &mut HashWriter(&mut h)).ok()?;
    Some(hex::encode(h.finalize()))
}

/// `io::Write` into a hasher (for `io::copy`).
struct HashWriter<'a>(&'a mut Sha256);
impl std::io::Write for HashWriter<'_> {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.update(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
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
        if r["signatureMismatch"].as_bool().unwrap_or(false) {
            return Ok("signature".into());
        }
        return Ok(if r["needsPermission"].as_bool().unwrap_or(false) { "permission".into() } else { "started".into() });
    }
    #[cfg(target_os = "windows")]
    {
        // Like Telegram: no installer window. The NSIS installer runs silently
        // over this copy (`/S`) and the new version starts. A detached cmd
        // does it — this process has to be gone for the files to change.
        use std::os::windows::process::CommandExt;
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let line = format!("/C \"ping -n 3 127.0.0.1 >nul & \"{}\" /S & start \"\" \"{}\"\"", p.display(), exe.display());
        std::process::Command::new("cmd")
            .raw_arg(line)
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .spawn()
            .map_err(|e| e.to_string())?;
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
mod web_tests {
    use super::*;

    #[test]
    fn feed_entry() {
        let feed = r#"<feed><title>Release notes</title><entry><id>x</id><updated>2026-11-07T10:00:00Z</updated>
<link rel="alternate" type="text/html" href="https://github.com/applememestm-pixel/Aquamarine/releases/tag/v1.2.11"/>
<title>Aquamarine 1.2.11 &amp; more</title></entry><entry><title>old</title></entry></feed>"#;
        let e = xml_tag(feed, "entry").unwrap();
        assert_eq!(xml_unescape(xml_tag(e, "title").unwrap()), "Aquamarine 1.2.11 & more");
        assert_eq!(xml_tag(e, "updated").unwrap(), "2026-11-07T10:00:00Z");
        let link = e.split("href=\"").nth(1).and_then(|r| r.split('"').next()).unwrap();
        assert!(link.ends_with("/tag/v1.2.11"));
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
