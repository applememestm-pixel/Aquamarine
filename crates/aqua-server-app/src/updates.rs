//! Updates of the server app itself (1.2.0-8), from GitHub Releases.
//!
//! A loop checks 20 s after start and then every 3 hours for the newest
//! numbered release (`v…`, pre-releases included — the project ships every
//! PR as one). Its file for this platform is downloaded and checked against
//! SHA256SUMS.txt; then, with "Update automatically" on (the default):
//!   * Windows — the NSIS installer runs silently and starts the new app
//!     (hidden again if it was in the tray);
//!   * Linux AppImage — the file is swapped and the new one started;
//!   * macOS, .deb, Android — can't replace themselves: the window shows
//!     "Update" and that opens the file / the download.
//! The server is down only for the seconds the restart takes.

use std::path::PathBuf;
use std::sync::Mutex;

use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

const REPO: &str = "applememestm-pixel/Aquamarine";
const EVERY: std::time::Duration = std::time::Duration::from_secs(3 * 3600);
/// The newest release has no file for us yet (still building): look again sooner.
const SOON: std::time::Duration = std::time::Duration::from_secs(15 * 60);

#[derive(Clone, Serialize, Default)]
pub struct UpdState {
    /// idle | checking | latest | waiting | downloading | ready | installing | error
    pub state: String,
    pub current: String,
    /// The newer version, when there is one.
    pub version: Option<String>,
    pub notes_url: Option<String>,
    pub done: u64,
    pub total: u64,
    pub error: Option<String>,
    /// This platform installs by itself (Windows, AppImage).
    pub can_self_install: bool,
    /// Last check, unix seconds.
    pub checked_at: u64,
}

static STATE: Mutex<Option<UpdState>> = Mutex::new(None);
static BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// What `install` will open / run.
static FILE: Mutex<Option<(PathBuf, String)>> = Mutex::new(None);

fn set(f: impl FnOnce(&mut UpdState)) {
    if let Ok(mut g) = STATE.lock() {
        let s = g.get_or_insert_with(fresh);
        f(s);
    }
}

fn fresh() -> UpdState {
    UpdState { state: "idle".into(), current: env!("CARGO_PKG_VERSION").into(), can_self_install: can_self_install(), ..Default::default() }
}

pub fn snapshot() -> UpdState {
    STATE.lock().ok().and_then(|g| g.clone()).unwrap_or_else(fresh)
}

fn can_self_install() -> bool {
    cfg!(target_os = "windows") || (cfg!(target_os = "linux") && !cfg!(target_os = "android") && std::env::var_os("APPIMAGE").is_some())
}

/// The release file of this platform.
fn wanted() -> Option<&'static str> {
    if cfg!(target_os = "android") {
        Some("AquamarineServer.apk")
    } else if cfg!(target_os = "windows") {
        Some("AquamarineServer-Setup.exe")
    } else if cfg!(target_os = "macos") {
        Some("AquamarineServer.dmg")
    } else if cfg!(target_os = "linux") {
        Some(if std::env::var_os("APPIMAGE").is_some() { "AquamarineServer.AppImage" } else { "aquamarine-server.deb" })
    } else {
        None
    }
}

/// The project's numbering (ui/src/lib/updates.ts::newer): 1.0.10 > 1.0.9,
/// 1.0.7 > 1.0.62 > 1.0.61 > 1.0.6, and 1.2.0-8 > 1.2.0-7 > 1.2.0.
pub fn newer(a: &str, b: &str) -> bool {
    let sa: Vec<&str> = a.split(['.', '-']).collect();
    let sb: Vec<&str> = b.split(['.', '-']).collect();
    let num = |s: &str| s.parse::<u64>().unwrap_or(0);
    for i in 0..sa.len().max(sb.len()) {
        let (x, y) = (sa.get(i).copied().unwrap_or("0"), sb.get(i).copied().unwrap_or("0"));
        if i == 2 && x.len() != y.len() {
            let short = x.len().min(y.len());
            let (hx, hy) = (num(&x[..short]), num(&y[..short]));
            if hx != hy {
                return hx > hy;
            }
            return x.len() > y.len();
        }
        if num(x) != num(y) {
            return num(x) > num(y);
        }
    }
    false
}

fn http() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(concat!("aqua-server-app/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())
}

struct Found {
    version: String,
    notes_url: String,
    asset: Option<(String, String, u64, Option<String>)>, // name, url, size, sha256
}

/// GitHub's API: 60 requests an hour per address without a login (shared
/// addresses run out of it — "403"). Then the plain website instead.
async fn find(c: &reqwest::Client) -> Result<Option<Found>, String> {
    match find_api(c).await {
        Ok(f) => Ok(f),
        Err(e) => find_web(c).await.map_err(|w| format!("{e}; {w}")),
    }
}

/// The releases feed (every release, newest first) and download links.
async fn find_web(c: &reqwest::Client) -> Result<Option<Found>, String> {
    let base = format!("https://github.com/{REPO}");
    let feed = c.get(format!("{base}/releases.atom")).timeout(std::time::Duration::from_secs(30)).send().await
        .map_err(|e| e.without_url().to_string())?;
    if !feed.status().is_success() {
        return Err(format!("github.com: HTTP {}", feed.status()));
    }
    let feed = feed.text().await.map_err(|e| e.to_string())?;
    let cur = env!("CARGO_PKG_VERSION");
    let best = feed
        .split("href=\"")
        .skip(1)
        .filter_map(|r| r.split('"').next())
        .filter_map(|l| l.rsplit_once("/releases/tag/v").map(|(_, v)| v.to_string()))
        .filter(|v| v.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .reduce(|a, b| if newer(&b, &a) { b } else { a });
    let Some(version) = best else { return Ok(None) };
    if !newer(&version, cur) {
        return Ok(None);
    }
    let dl = |f: &str| format!("{base}/releases/download/v{version}/{f}");
    let mut asset = None;
    if let Some(name) = wanted() {
        let sums = match c.get(dl("SHA256SUMS.txt")).timeout(std::time::Duration::from_secs(30)).send().await {
            Ok(r) if r.status().is_success() => r.text().await.unwrap_or_default(),
            _ => String::new(),
        };
        let sha = sums.lines().find_map(|l| {
            let mut it = l.split_whitespace();
            let h = it.next()?;
            (it.next()?.trim_start_matches('*') == name && h.len() == 64).then(|| h.to_ascii_lowercase())
        });
        if sha.is_some() {
            let size = match c.get(dl(name)).header(reqwest::header::RANGE, "bytes=0-0").timeout(std::time::Duration::from_secs(30)).send().await {
                Ok(r) => r.headers().get(reqwest::header::CONTENT_RANGE).and_then(|v| v.to_str().ok())
                    .and_then(|v| v.rsplit('/').next()).and_then(|n| n.parse().ok()).unwrap_or(0),
                Err(_) => 0,
            };
            asset = Some((name.to_string(), dl(name), size, sha));
        }
    }
    Ok(Some(Found { version: version.clone(), notes_url: format!("{base}/releases/tag/v{version}"), asset }))
}

async fn find_api(c: &reqwest::Client) -> Result<Option<Found>, String> {
    let r = c
        .get(format!("https://api.github.com/repos/{REPO}/releases?per_page=20"))
        .header("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !r.status().is_success() {
        return Err(format!("GitHub: HTTP {}", r.status()));
    }
    let list: serde_json::Value = r.json().await.map_err(|e| e.to_string())?;
    let cur = env!("CARGO_PKG_VERSION");
    let best = list
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| !r["draft"].as_bool().unwrap_or(false))
        .filter_map(|r| Some((r["tag_name"].as_str()?.strip_prefix('v')?.to_string(), r)))
        .filter(|(v, _)| v.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .reduce(|a, b| if newer(&b.0, &a.0) { b } else { a });
    let Some((version, rel)) = best else { return Ok(None) };
    if !newer(&version, cur) {
        return Ok(None);
    }
    let assets = rel["assets"].as_array().cloned().unwrap_or_default();
    let by = |n: &str| assets.iter().find(|a| a["name"].as_str() == Some(n)).cloned();
    let mut asset = None;
    if let Some(name) = wanted() {
        if let Some(a) = by(name) {
            let mut sha = None;
            if let Some(sums) = by("SHA256SUMS.txt") {
                // Sums older than the file belong to an earlier upload of it.
                if sums["updated_at"].as_str().unwrap_or_default() >= a["updated_at"].as_str().unwrap_or_default() {
                    if let Some(u) = sums["browser_download_url"].as_str() {
                        if let Ok(r) = c.get(u).timeout(std::time::Duration::from_secs(30)).send().await {
                            let t = r.text().await.unwrap_or_default();
                            sha = t.lines().find_map(|l| {
                                let mut it = l.split_whitespace();
                                let h = it.next()?;
                                (it.next()?.trim_start_matches('*') == name && h.len() == 64).then(|| h.to_ascii_lowercase())
                            });
                        }
                    }
                }
            }
            asset = Some((
                name.to_string(),
                a["browser_download_url"].as_str().unwrap_or_default().to_string(),
                a["size"].as_u64().unwrap_or(0),
                sha,
            ));
        }
    }
    Ok(Some(Found { version, notes_url: rel["html_url"].as_str().unwrap_or_default().to_string(), asset }))
}

fn dir(app: &AppHandle) -> Result<PathBuf, String> {
    let d = app.path().app_cache_dir().map_err(|e| e.to_string())?.join("updates");
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    Ok(d)
}

fn file_sha256(p: &std::path::Path) -> Option<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(p).ok()?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Some(hex::encode(h.finalize()))
}

async fn download(app: &AppHandle, c: &reqwest::Client, name: &str, url: &str, size: u64, sha: &str) -> Result<PathBuf, String> {
    use std::io::Write;
    if !url.starts_with("https://github.com/") || name.contains(['/', '\\']) {
        return Err("unexpected update address".into());
    }
    let d = dir(app)?;
    let path = d.join(name);
    if file_sha256(&path).as_deref() == Some(sha) {
        return Ok(path);
    }
    if let Ok(rd) = std::fs::read_dir(&d) {
        for e in rd.flatten() {
            let _ = std::fs::remove_file(e.path());
        }
    }
    let part = d.join(format!("{name}.part"));
    let mut r = c.get(url).send().await.map_err(|e| e.to_string())?;
    if !r.status().is_success() {
        return Err(format!("download: HTTP {}", r.status()));
    }
    let total = r.content_length().unwrap_or(size);
    set(|s| {
        s.state = "downloading".into();
        s.done = 0;
        s.total = total;
    });
    let mut f = std::fs::File::create(&part).map_err(|e| e.to_string())?;
    let mut h = Sha256::new();
    let mut done = 0u64;
    while let Some(chunk) = r.chunk().await.map_err(|e| e.to_string())? {
        f.write_all(&chunk).map_err(|e| e.to_string())?;
        h.update(&chunk);
        done += chunk.len() as u64;
        set(|s| s.done = done);
    }
    f.flush().map_err(|e| e.to_string())?;
    drop(f);
    let got = hex::encode(h.finalize());
    if got != sha {
        let _ = std::fs::remove_file(&part);
        return Err("the download is damaged (SHA-256 mismatch) — will try again".into());
    }
    std::fs::rename(&part, &path).map_err(|e| e.to_string())?;
    Ok(path)
}

/// One pass: check, download, and (`install`) put it in place.
pub async fn run_once(app: &AppHandle, install: bool) -> std::time::Duration {
    if BUSY.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return EVERY;
    }
    let next = pass(app, install).await;
    BUSY.store(false, std::sync::atomic::Ordering::SeqCst);
    next
}

async fn pass(app: &AppHandle, install: bool) -> std::time::Duration {
    set(|s| {
        s.state = "checking".into();
        s.error = None;
    });
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let fail = |e: String| {
        tracing::warn!(%e, "update check");
        set(|s| {
            s.state = "error".into();
            s.error = Some(e);
        });
        SOON
    };
    let c = match http() {
        Ok(c) => c,
        Err(e) => return fail(e),
    };
    let found = match find(&c).await {
        Ok(f) => f,
        Err(e) => return fail(e),
    };
    set(|s| s.checked_at = now);
    let Some(f) = found else {
        set(|s| {
            s.state = "latest".into();
            s.version = None;
        });
        return EVERY;
    };
    set(|s| {
        s.version = Some(f.version.clone());
        s.notes_url = Some(f.notes_url.clone());
    });
    let Some((name, url, size, Some(sha))) = f.asset else {
        // The release exists, its file (or its checksum) isn't up yet.
        set(|s| s.state = "waiting".into());
        return SOON;
    };
    if cfg!(target_os = "android") {
        // Android installs an APK only through the system: the window offers it.
        if let Ok(mut g) = FILE.lock() {
            *g = Some((PathBuf::new(), url));
        }
        set(|s| s.state = "ready".into());
        return EVERY;
    }
    let path = match download(app, &c, &name, &url, size, &sha).await {
        Ok(p) => p,
        Err(e) => return fail(e),
    };
    if let Ok(mut g) = FILE.lock() {
        *g = Some((path, url));
    }
    set(|s| s.state = "ready".into());
    if install && can_self_install() {
        if let Err(e) = install_now(app) {
            return fail(e);
        }
    }
    EVERY
}

/// Put the downloaded file in place (or open it where that's all we can do).
pub fn install_now(app: &AppHandle) -> Result<(), String> {
    let (path, url) = FILE.lock().ok().and_then(|g| g.clone()).ok_or("nothing downloaded yet")?;
    set(|s| s.state = "installing".into());
    #[cfg(desktop)]
    let hidden = app.get_webview_window("main").and_then(|w| w.is_visible().ok()).map(|v| !v).unwrap_or(false);
    #[cfg(mobile)]
    let hidden = false;
    #[cfg(target_os = "android")]
    {
        let _ = (&path, hidden);
        use tauri_plugin_opener::OpenerExt;
        return app.opener().open_url(url.as_str(), None::<&str>).map_err(|e| e.to_string());
    }
    #[cfg(target_os = "windows")]
    {
        let _ = &url;
        use std::os::windows::process::CommandExt;
        // Silent NSIS install over this copy, then start it again. Run by a
        // detached cmd: this process has to be gone for the files to change.
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let again = if hidden { " --hidden" } else { "" };
        let line = format!(
            "/C \"ping -n 3 127.0.0.1 >nul & \"{}\" /S & start \"\" \"{}\"{}\"",
            path.display(),
            exe.display(),
            again
        );
        std::process::Command::new("cmd")
            .raw_arg(line)
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .spawn()
            .map_err(|e| e.to_string())?;
        app.exit(0);
        return Ok(());
    }
    #[cfg(all(target_os = "linux", not(target_os = "android")))]
    {
        let _ = &url;
        if let Some(img) = std::env::var_os("APPIMAGE").map(PathBuf::from) {
            // Replacing the file is safe while the old one runs (it's mounted).
            let tmp = img.with_extension("AppImage.new");
            std::fs::copy(&path, &tmp).map_err(|e| e.to_string())?;
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
            std::fs::rename(&tmp, &img).map_err(|e| e.to_string())?;
            // The old copy must let go of the port (and of the one-copy lock)
            // first: the new one starts two seconds after this one exits.
            let mut cmd = std::process::Command::new("sh");
            cmd.arg("-c").arg(r#"sleep 2; exec "$0" "$@""#).arg(&img);
            if hidden {
                cmd.arg("--hidden");
            }
            cmd.spawn().map_err(|e| e.to_string())?;
            app.exit(0);
            return Ok(());
        }
        std::process::Command::new("xdg-open").arg(&path).spawn().map_err(|e| e.to_string())?;
        set(|s| s.state = "ready".into());
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    {
        let _ = (&url, hidden);
        std::process::Command::new("open").arg(&path).spawn().map_err(|e| e.to_string())?;
        set(|s| s.state = "ready".into());
        return Ok(());
    }
    #[allow(unreachable_code)]
    {
        let _ = (&path, &url, hidden);
        Err("updates can't be installed here".into())
    }
}

/// The background loop (started in `setup`).
pub fn spawn(app: AppHandle, auto: impl Fn(&AppHandle) -> bool + Send + 'static) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(20)).await;
        loop {
            let wait = run_once(&app, auto(&app)).await;
            tokio::time::sleep(wait).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::newer;

    #[test]
    fn numbering() {
        assert!(newer("1.2.0-8", "1.2.0-7"));
        assert!(newer("1.2.0-10", "1.2.0-9"));
        assert!(newer("1.2.0-1", "1.2.0"));
        assert!(newer("1.2.1", "1.2.0-12"));
        assert!(newer("1.0.7", "1.0.62"));
        assert!(newer("1.0.61", "1.0.6"));
        assert!(!newer("1.2.0-7", "1.2.0-7"));
        assert!(!newer("1.2.0-7", "1.2.0-8"));
    }
}
