//! Updates of the console server (`aqua-server`, also the Docker image) —
//! 1.20.1-6. The owner's rule: every server gets a new release, however it
//! is started. (Inside the Aquamarine Server app the app updates itself:
//! aqua-server-app/src/updates.rs.)
//!
//! 30 s after start and then every 3 hours: the newest numbered release on
//! GitHub (the site's `updates.json` when the API says 403). Pre-releases
//! only for testers (`aqua-updates.json` `{ "tester": true }`, the panel's
//! switch, or `AQUA_UPDATES=tester`). Its binary for this platform
//! (`aqua-server-<os>-<arch>`) is downloaded, checked against SHA256SUMS.txt
//! and put in place:
//!
//!   * the running file itself, when it can be written (a plain download,
//!     the server's own folder);
//!   * otherwise (`/usr/local/bin` of the Docker image, a root-owned
//!     install) `aqua-update/aqua-server` in the data folder — the data
//!     folder outlives the container, and every start runs the newest of
//!     the two (`run_newest`).
//!
//! Then the server restarts: under systemd through `systemctl restart`, on
//! Unix otherwise by `exec` in place (same PID — PID 1 of a container stays
//! PID 1), on Windows by starting the new file and leaving.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;
use sha2::{Digest, Sha256};

const REPO: &str = "applememestm-pixel/Aquamarine";
const EVERY: std::time::Duration = std::time::Duration::from_secs(3 * 3600);
const SOON: std::time::Duration = std::time::Duration::from_secs(15 * 60);
const STAGE_DIR: &str = "aqua-update";
const CONF: &str = "aqua-updates.json";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Serialize, Default)]
pub struct UpdState {
    /// off | idle | checking | latest | waiting | downloading | installing | error
    pub state: String,
    pub current: String,
    pub version: Option<String>,
    pub notes_url: Option<String>,
    pub done: u64,
    pub total: u64,
    pub error: Option<String>,
    pub checked_at: u64,
    pub tester: bool,
    /// This process updates itself (the console server; not inside the app).
    pub managed: bool,
}

static STATE: Mutex<Option<UpdState>> = Mutex::new(None);
static BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static MANAGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn set(f: impl FnOnce(&mut UpdState)) {
    let mut g = STATE.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(fresh));
}

fn fresh() -> UpdState {
    UpdState { state: "idle".into(), current: VERSION.into(), ..Default::default() }
}

pub fn snapshot() -> UpdState {
    let mut s = STATE.lock().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_else(fresh);
    s.tester = tester();
    s.managed = MANAGED.load(std::sync::atomic::Ordering::Relaxed);
    s
}

/// Tester: pre-releases too. `AQUA_UPDATES=tester` or the panel's switch.
pub fn tester() -> bool {
    if std::env::var("AQUA_UPDATES").map(|v| v.eq_ignore_ascii_case("tester")).unwrap_or(false) {
        return true;
    }
    std::fs::read(CONF)
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .and_then(|v| v["tester"].as_bool())
        .unwrap_or(false)
}

pub fn set_tester(on: bool) -> std::io::Result<()> {
    std::fs::write(CONF, serde_json::to_vec_pretty(&serde_json::json!({ "tester": on })).unwrap_or_default())
}

/// The release file of this platform.
fn wanted() -> Option<&'static str> {
    if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        Some("aqua-server-windows-x64.exe")
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("aqua-server-macos-arm64")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("aqua-server-linux-x64")
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        Some("aqua-server-linux-arm64")
    } else {
        None
    }
}

/// The project's numbering (= ui/src/lib/updates.ts::newer): 1.0.10 > 1.0.9,
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

fn sums_line(text: &str, name: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let mut it = l.split_whitespace();
        let h = it.next()?;
        (it.next()?.trim_start_matches('*') == name && h.len() == 64).then(|| h.to_ascii_lowercase())
    })
}

struct Found {
    version: String,
    notes_url: String,
    /// url, size, sha256
    asset: Option<(String, u64, Option<String>)>,
    /// The signed list the checksum came from: its text and signature.
    proof: Option<(String, String)>,
}

/// The highest release of the list this channel may install. Pre-release
/// or not is told by the version (signed), not by the list's flag.
fn best(list: &serde_json::Value, tester: bool) -> Option<(String, &serde_json::Value)> {
    list.as_array()
        .into_iter()
        .flatten()
        .filter(|r| !r["draft"].as_bool().unwrap_or(false))
        .filter(|r| tester || !r["prerelease"].as_bool().unwrap_or(false))
        .filter_map(|r| Some((r["tag_name"].as_str()?.strip_prefix('v')?.to_string(), r)))
        .filter(|(v, _)| v.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .filter(|(v, _)| aqua_core::release::channel_allows(v, tester))
        .reduce(|a, b| if newer(&b.0, &a.0) { b } else { a })
}

async fn pick(c: &reqwest::Client, list: &serde_json::Value, tester: bool) -> Option<Found> {
    let (version, rel) = best(list, tester)?;
    if !newer(&version, VERSION) {
        return Some(Found { version: String::new(), notes_url: String::new(), asset: None, proof: None });
    }
    let assets = rel["assets"].as_array().cloned().unwrap_or_default();
    let by = |n: &str| assets.iter().find(|a| a["name"].as_str() == Some(n)).cloned();
    let mut asset = None;
    let mut proof = None;
    if let Some(name) = wanted() {
        if let Some(a) = by(name) {
            let mut sha = None;
            if let Some(sums) = by("SHA256SUMS.txt") {
                // Sums older than the file belong to an earlier upload of it.
                if sums["updated_at"].as_str().unwrap_or_default() >= a["updated_at"].as_str().unwrap_or_default() {
                    let text = match (rel["sums"].as_str(), sums["browser_download_url"].as_str()) {
                        (Some(t), _) => Some(t.to_string()),
                        (None, Some(u)) => match c.get(u).timeout(std::time::Duration::from_secs(30)).send().await {
                            Ok(r) => r.text().await.ok(),
                            Err(_) => None,
                        },
                        _ => None,
                    };
                    // AQUA-SEC-012: signed by the built-in release key (once set).
                    let sig = match (rel["sums_sig"].as_str(), by(aqua_core::release::SIG_ASSET).and_then(|a| a["browser_download_url"].as_str().map(str::to_string))) {
                        (Some(t), _) => Some(t.to_string()),
                        (None, Some(u)) => match c.get(u).timeout(std::time::Duration::from_secs(30)).send().await {
                            Ok(r) => r.text().await.ok(),
                            Err(_) => None,
                        },
                        _ => None,
                    };
                    if let Some(t) = text.filter(|t| aqua_core::release::sums_trusted_for(t, sig.as_deref(), &version)) {
                        sha = sums_line(&t, name);
                        proof = sig.map(|s| (t, s));
                    }
                }
            }
            asset = Some((a["browser_download_url"].as_str().unwrap_or_default().to_string(), a["size"].as_u64().unwrap_or(0), sha));
        }
    }
    Some(Found { version, notes_url: rel["html_url"].as_str().unwrap_or_default().to_string(), asset, proof })
}

const FEEDS: &[&str] = &["https://aquamarinemessenger.pw/updates.json", "https://applememestm-pixel.github.io/Aquamarine/updates.json"];

/// None: nothing newer. GitHub's API first, then the site's feed.
async fn find(c: &reqwest::Client, tester: bool) -> Result<Option<Found>, String> {
    // REV2-003 (fail-closed): a build without the release key installs no update.
    if !aqua_core::release::signing_enabled() {
        return Err(aqua_core::release::NO_KEY.into());
    }
    let api = async {
        let r = c
            .get(format!("https://api.github.com/repos/{REPO}/releases?per_page=20"))
            .header("Accept", "application/vnd.github+json")
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| e.without_url().to_string())?;
        if !r.status().is_success() {
            return Err(format!("GitHub: HTTP {}", r.status()));
        }
        r.json::<serde_json::Value>().await.map_err(|e| e.to_string())
    };
    let list = match api.await {
        Ok(l) => l,
        Err(e) => {
            let mut got = None;
            let mut last = e;
            for url in FEEDS {
                match c.get(*url).header("Cache-Control", "no-cache").timeout(std::time::Duration::from_secs(30)).send().await {
                    Ok(r) if r.status().is_success() => match r.json::<serde_json::Value>().await {
                        Ok(f) => {
                            got = Some(f["releases"].clone());
                            break;
                        }
                        Err(e) => last = format!("{last}; site: {}", e.without_url()),
                    },
                    Ok(r) => last = format!("{last}; site: HTTP {}", r.status()),
                    Err(e) => last = format!("{last}; site: {}", e.without_url()),
                }
            }
            got.ok_or(last)?
        }
    };
    Ok(pick(c, &list, tester).await.filter(|f| !f.version.is_empty()))
}

fn sha256_of(p: &Path) -> Option<String> {
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

fn stage_dir() -> PathBuf {
    crate::datadir::current().join(STAGE_DIR)
}

/// Download into `<data>/aqua-update/aqua-server(.exe).part`, check, rename.
async fn download(c: &reqwest::Client, url: &str, size: u64, sha: &str) -> Result<PathBuf, String> {
    use std::io::Write;
    if !url.starts_with(aqua_core::release::DOWNLOAD_PREFIX) {
        return Err("unexpected update address".into());
    }
    let d = stage_dir();
    std::fs::create_dir_all(&d).map_err(|e| format!("{}: {e}", d.display()))?;
    let path = d.join(bin_name());
    let part = d.join(format!("{}.part", bin_name()));
    let mut r = c.get(url).send().await.map_err(|e| e.without_url().to_string())?;
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
    if hex::encode(h.finalize()) != sha {
        let _ = std::fs::remove_file(&part);
        return Err("the download is damaged (SHA-256 mismatch) — will try again".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&part, std::fs::Permissions::from_mode(0o755));
    }
    std::fs::rename(&part, &path).map_err(|e| e.to_string())?;
    Ok(path)
}

fn bin_name() -> &'static str {
    if cfg!(windows) {
        "aqua-server.exe"
    } else {
        "aqua-server"
    }
}

/// Put `new` (already checked) where the next start runs it. Returns the
/// file to run.
fn put_in_place(new: &Path, version: &str, proof: (&str, &str)) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let staged = stage_dir().join(bin_name());
    // Already running from the staged copy: that's the place.
    if exe.canonicalize().ok() == staged.canonicalize().ok() && new == staged {
        keep_staged(&stage_dir(), version, proof).map_err(|e| e.to_string())?;
        return Ok(staged);
    }
    // Over the running file: on Unix the old one stays open by inode; on
    // Windows a running .exe can be renamed, not overwritten.
    let tmp = exe.with_extension("new");
    let over = (|| -> std::io::Result<()> {
        std::fs::copy(new, &tmp)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
        }
        #[cfg(windows)]
        {
            let old = exe.with_extension("old.exe");
            let _ = std::fs::remove_file(&old);
            std::fs::rename(&exe, &old)?;
        }
        std::fs::rename(&tmp, &exe)?;
        Ok(())
    })();
    match over {
        Ok(()) => {
            // The staged copy isn't needed: the binary itself is new.
            let _ = std::fs::remove_dir_all(stage_dir());
            Ok(exe)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            tracing::info!(%e, exe = %exe.display(), "update: the binary can't be replaced — it runs from the data folder");
            keep_staged(&stage_dir(), version, proof).map_err(|e| e.to_string())?;
            Ok(staged)
        }
    }
}

/// What `run_newest` needs to run the staged binary later: its version and
/// the signed list that names it.
fn keep_staged(d: &Path, version: &str, (sums, sig): (&str, &str)) -> std::io::Result<()> {
    std::fs::write(d.join("SHA256SUMS.txt"), sums)?;
    std::fs::write(d.join(aqua_core::release::SIG_ASSET), sig)?;
    std::fs::write(d.join("version"), version)
}

/// The staged binary is the one release `version` lists in sums signed by
/// `key` — checked again each time, the data folder can be written by others.
fn staged_is_genuine(d: &Path, version: &str, key: Option<&str>) -> bool {
    let read = |f: &str| std::fs::read_to_string(d.join(f)).ok();
    let (Some(sums), Some(sig), Some(name)) = (read("SHA256SUMS.txt"), read(aqua_core::release::SIG_ASSET), wanted()) else { return false };
    aqua_core::release::sums_trusted_with(key, &sums, Some(&sig))
        && aqua_core::release::sums_name_version(&sums, version)
        && sums_line(&sums, name).is_some_and(|h| sha256_of(&d.join(bin_name())) == Some(h))
}

/// At start (main.rs, after entering the data folder): when the data folder
/// holds a newer staged binary than this one, run that instead. An older
/// staged one (the image itself was updated) is removed.
pub fn run_newest() {
    if cfg!(debug_assertions) {
        return;
    }
    #[cfg(windows)]
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(exe.with_extension("old.exe"));
    }
    let d = stage_dir();
    let staged = d.join(bin_name());
    let Ok(v) = std::fs::read_to_string(d.join("version")) else { return };
    let v = v.trim().to_string();
    if !staged.is_file() {
        return;
    }
    let exe = std::env::current_exe().ok();
    let is_staged = exe.as_ref().and_then(|e| e.canonicalize().ok()) == staged.canonicalize().ok();
    if is_staged {
        return;
    }
    if !newer(&v, VERSION) {
        let _ = std::fs::remove_dir_all(&d);
        return;
    }
    if !staged_is_genuine(&d, &v, aqua_core::release::UPDATE_PUBKEY) {
        // Not run; the next update check downloads or confirms it again.
        eprintln!("aqua-server {VERSION}: the staged {v} isn't confirmed by a signed release list — not started");
        return;
    }
    eprintln!("aqua-server {VERSION}: starting the newer {v} from {}", staged.display());
    restart_as(&staged);
}

/// Replace this process with `bin` (same arguments).
fn restart_as(bin: &Path) {
    // Used signatures on disk and marked clean before the new one starts.
    crate::replay::Replay::shutdown_all();
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let e = std::process::Command::new(bin).args(&args).exec();
        tracing::warn!(%e, "update: exec failed");
    }
    #[cfg(not(unix))]
    {
        if std::process::Command::new(bin).args(&args).spawn().is_ok() {
            std::process::exit(0);
        }
    }
}

/// One pass. `Some(bin)` when an update is in place and the server should
/// restart into it.
async fn pass() -> (std::time::Duration, Option<PathBuf>) {
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
        (SOON, None)
    };
    let c = match reqwest::Client::builder()
        .user_agent(concat!("aqua-server/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(std::time::Duration::from_secs(20))
        .build()
    {
        Ok(c) => c,
        Err(e) => return fail(e.to_string()),
    };
    let found = match find(&c, tester()).await {
        Ok(f) => f,
        Err(e) => return fail(e),
    };
    set(|s| s.checked_at = now);
    let Some(f) = found else {
        set(|s| {
            s.state = "latest".into();
            s.version = None;
        });
        return (EVERY, None);
    };
    set(|s| {
        s.version = Some(f.version.clone());
        s.notes_url = Some(f.notes_url.clone());
    });
    let (Some((url, size, Some(sha))), Some((sums, sig))) = (f.asset, f.proof) else {
        set(|s| s.state = "waiting".into());
        return (SOON, None);
    };
    let staged = stage_dir().join(bin_name());
    let file = if sha256_of(&staged).as_deref() == Some(sha.as_str()) {
        staged
    } else {
        match download(&c, &url, size, &sha).await {
            Ok(p) => p,
            Err(e) => return fail(e),
        }
    };
    let bin = match put_in_place(&file, &f.version, (&sums, &sig)) {
        Ok(b) => b,
        Err(e) => return fail(e),
    };
    set(|s| s.state = "installing".into());
    tracing::warn!(version = %f.version, bin = %bin.display(), "update: restarting into the new version");
    (EVERY, Some(bin))
}

/// Check now (the panel's button); restarts when there's a new version.
pub async fn run_once(before_exit: impl FnOnce() + Send + 'static) {
    if BUSY.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let (_, bin) = pass().await;
    BUSY.store(false, std::sync::atomic::Ordering::SeqCst);
    if let Some(bin) = bin {
        restart_into(bin, before_exit);
    }
}

fn restart_into(bin: PathBuf, before_exit: impl FnOnce() + Send + 'static) {
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(600));
        before_exit();
        // systemd: its restart runs the file in ExecStart (now the new one,
        // or the old one, which then runs the staged copy — run_newest).
        if let Some(unit) = crate::host::systemd_unit() {
            if std::process::Command::new("systemctl").arg("restart").arg(&unit).spawn().is_ok() {
                std::thread::sleep(std::time::Duration::from_secs(20));
            }
        }
        restart_as(&bin);
        crate::replay::Replay::shutdown_all();
        std::process::exit(75);
    });
}

/// The background loop of the console server (main.rs).
pub fn spawn(before_exit: impl Fn() + Send + Sync + 'static) {
    if cfg!(debug_assertions) || wanted().is_none() {
        set(|s| s.state = "off".into());
        return;
    }
    MANAGED.store(true, std::sync::atomic::Ordering::Relaxed);
    let before_exit = std::sync::Arc::new(before_exit);
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        loop {
            if BUSY.swap(true, std::sync::atomic::Ordering::SeqCst) {
                tokio::time::sleep(SOON).await;
                continue;
            }
            let (wait, bin) = pass().await;
            BUSY.store(false, std::sync::atomic::Ordering::SeqCst);
            if let Some(bin) = bin {
                let b = before_exit.clone();
                restart_into(bin, move || b());
                return;
            }
            tokio::time::sleep(wait).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{bin_name, keep_staged, newer, staged_is_genuine, wanted};

    /// A binary in the data folder runs only while a signed release list
    /// still names exactly it and its version.
    #[test]
    fn a_pre_release_marked_stable_is_not_for_everyone() {
        // The feed says "not a pre-release"; its number says it is.
        let list = serde_json::json!([{ "tag_name": "v9.9.0-1", "prerelease": false }, { "tag_name": "v9.8.1", "prerelease": false }]);
        assert_eq!(super::best(&list, false).map(|b| b.0).as_deref(), Some("9.8.1"));
        assert_eq!(super::best(&list, true).map(|b| b.0).as_deref(), Some("9.9.0-1"));
    }

    #[test]
    fn a_staged_binary_needs_its_signed_list() {
        let Some(name) = wanted() else { return };
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        let key = aqua_core::crypto::SignSecret::generate();
        let pk = hex::encode(key.public().as_bytes());
        std::fs::write(d.join(bin_name()), b"new server").unwrap();
        assert!(!staged_is_genuine(d, "9.0.1", Some(&pk)), "no list");
        let sums = format!("{}  {name}\n{}  VERSION\n", aqua_core::hex_sha256(b"new server"), aqua_core::hex_sha256(b"9.0.1"));
        let sig = hex::encode(key.sign(sums.as_bytes()));
        keep_staged(d, "9.0.1", (&sums, &sig)).unwrap();
        assert!(staged_is_genuine(d, "9.0.1", Some(&pk)));
        assert!(!staged_is_genuine(d, "9.0.2", Some(&pk)), "another version");
        assert!(!staged_is_genuine(d, "9.0.1", None), "no key built in");
        std::fs::write(d.join(bin_name()), b"swapped").unwrap();
        assert!(!staged_is_genuine(d, "9.0.1", Some(&pk)), "changed file");
    }

    #[test]
    fn numbering() {
        assert!(newer("1.2.0-8", "1.2.0-7"));
        assert!(newer("1.20.11", "1.20.1-6"));
        assert!(newer("1.20.1-6", "1.20.1"));
        assert!(newer("1.0.7", "1.0.62"));
        assert!(!newer("1.2.0-7", "1.2.0-7"));
    }
}
