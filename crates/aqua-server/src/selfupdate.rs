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
    sha256_file(&mut std::fs::File::open(p).ok()?)
}

/// SHA-256 of what `f` holds, from its start.
fn sha256_file(f: &mut std::fs::File) -> Option<String> {
    use std::io::{Read, Seek};
    f.seek(std::io::SeekFrom::Start(0)).ok()?;
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
fn put_in_place(new: &Path, checked: &Checked, version: &str, proof: (&str, &str)) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let staged = stage_dir().join(bin_name());
    // Already running from the staged copy: that's the place.
    if (exe.canonicalize().ok() == staged.canonicalize().ok() || std::fs::read_to_string(stage_dir().join("version")).is_ok_and(|v| v.trim() == VERSION)) && new == staged {
        keep_staged(&stage_dir(), version, proof).map_err(|e| e.to_string())?;
        return Ok(staged);
    }
    // Over the running file: on Unix the old one stays open by inode; on
    // Windows a running .exe can be renamed, not overwritten.
    let tmp = exe.with_extension("new");
    let over = (|| -> std::io::Result<()> {
        // The checked bytes, not a second read of `new`.
        std::fs::write(&tmp, &checked.bytes)?;
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

/// A release binary as it was checked: its bytes, read once, hashed and
/// kept in memory — what is written in place and what is run are these
/// bytes, not whatever the file holds by then (review of F: a rename or a
/// write into the checked file after the check). On Windows the file also
/// stays open, no one may write, rename or delete it, until it is started.
pub(crate) struct Checked {
    bytes: Vec<u8>,
    #[cfg(windows)]
    held: Option<std::fs::File>,
}

/// `path` holds exactly the release file with SHA-256 `want`.
fn check_file(path: &Path, want: &str) -> Option<Checked> {
    use std::io::Read;
    let mut o = std::fs::OpenOptions::new();
    o.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        o.share_mode(1); // FILE_SHARE_READ: no writing, renaming or deleting meanwhile
    }
    let mut f = o.open(path).ok()?;
    let mut bytes = Vec::new();
    f.read_to_end(&mut bytes).ok()?;
    if !hex::encode(Sha256::digest(&bytes)).eq_ignore_ascii_case(want) {
        return None;
    }
    #[cfg(not(windows))]
    drop(f);
    Some(Checked {
        bytes,
        #[cfg(windows)]
        held: Some(f),
    })
}

/// The staged binary is the one release `version` lists in sums signed by
/// `key` — checked again each time, the data folder can be written by others.
#[cfg(test)]
fn staged_is_genuine(d: &Path, version: &str, key: Option<&str>) -> bool {
    staged_genuine(d, version, key).is_some()
}

fn staged_genuine(d: &Path, version: &str, key: Option<&str>) -> Option<Checked> {
    let read = |f: &str| std::fs::read_to_string(d.join(f)).ok();
    let (sums, sig, name) = (read("SHA256SUMS.txt")?, read(aqua_core::release::SIG_ASSET)?, wanted()?);
    if !(aqua_core::release::sums_trusted_with(key, &sums, Some(&sig)) && aqua_core::release::sums_name_version(&sums, version)) {
        return None;
    }
    check_file(&d.join(bin_name()), &sums_line(&sums, name)?)
}

/// Marks that the staged version has run here (`run_newest`).
const RAN: &str = "ran";

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
    // This is it (started from the staged bytes, maybe from memory — the
    // path of this process then isn't the staged file's).
    if v == VERSION {
        let _ = std::fs::write(d.join(RAN), VERSION);
        return;
    }
    if !aqua_core::release::version_ok(&v) || newer(VERSION, &v) {
        let _ = std::fs::remove_dir_all(&d);
        return;
    }
    // Pre-releases only for testers: switched off after the download, a
    // staged pre-release that never ran isn't started. One that already ran
    // here is (going back to the image's older binary over the same data
    // could break it); the next release replaces it.
    let ran = std::fs::read_to_string(d.join(RAN)).is_ok_and(|r| r.trim() == v);
    if !aqua_core::release::channel_allows(&v, tester()) && !ran {
        eprintln!("aqua-server {VERSION}: the staged {v} is a pre-release and this server isn't a tester — not started");
        return;
    }
    let Some(checked) = staged_genuine(&d, &v, aqua_core::release::UPDATE_PUBKEY) else {
        // Not run; the next update check downloads or confirms it again.
        eprintln!("aqua-server {VERSION}: the staged {v} isn't confirmed by a signed release list — not started");
        return;
    };
    eprintln!("aqua-server {VERSION}: starting the newer {v} from {}", staged.display());
    let e = launch(checked, &staged);
    // Not started: this version goes on (never the file by name instead).
    eprintln!("aqua-server {VERSION}: the checked {v} couldn't be started ({e}) — going on with {VERSION}");
}

/// Replace this process with the checked bytes. Returns only when that
/// couldn't be done — and then nothing else is run (fail-closed):
///   * Linux: a sealed memfd (no one can change it, not even us) run with
///     `execveat` — no file, no `/proc` needed;
///   * Windows: the file, still held open so no one could change it since
///     the check, is started, and this process leaves;
///   * other Unix (macOS): a copy in a folder of our own under the user's
///     temporary folder (0700), run from there.
fn launch(c: Checked, bin: &Path) -> std::io::Error {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    #[cfg(target_os = "linux")]
    {
        let fd = match sealed(&c.bytes) {
            Ok(fd) => fd,
            Err(e) => return e,
        };
        drop(c);
        crate::replay::Replay::shutdown_all();
        exec_fd(&fd, bin, &args)
    }
    #[cfg(windows)]
    {
        crate::replay::Replay::shutdown_all();
        match std::process::Command::new(bin).args(&args).spawn() {
            Ok(_) => {
                drop(c);
                std::process::exit(0);
            }
            Err(e) => e,
        }
    }
    #[cfg(all(unix, not(target_os = "linux")))]
    {
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
        use std::os::unix::process::CommandExt;
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("aqua-server-run-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let run = (|| -> std::io::Result<PathBuf> {
            std::fs::DirBuilder::new().mode(0o700).create(&dir)?;
            let file = dir.join(bin_name());
            let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o700).open(&file)?;
            f.write_all(&c.bytes)?;
            f.sync_all()?;
            Ok(file)
        })();
        let file = match run {
            Ok(f) => f,
            Err(e) => return e,
        };
        crate::replay::Replay::shutdown_all();
        std::process::Command::new(&file).arg0(bin).args(&args).exec()
    }
}

/// The bytes in a memfd sealed against any change (write, grow, shrink,
/// more seals). Fails where memfds can't be made or run.
#[cfg(target_os = "linux")]
fn sealed(bytes: &[u8]) -> std::io::Result<std::os::fd::OwnedFd> {
    use std::io::Write;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    // Byte strings, not c"…" literals: those need Rust 1.77 (the workspace says 1.75).
    let name: &[u8] = b"aqua-server\0";
    // SAFETY: a valid C string; the result is checked.
    let raw = unsafe { libc::memfd_create(name.as_ptr().cast(), libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING) };
    if raw < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `raw` is a new descriptor we own.
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    let mut f = std::fs::File::from(fd);
    f.write_all(bytes)?;
    let seals = libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL;
    // SAFETY: fcntl on our own descriptor.
    if unsafe { libc::fcntl(f.as_raw_fd(), libc::F_ADD_SEALS, seals) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(f.into())
}

/// Arguments and environment ready for `execveat` (built before, so the
/// call itself allocates nothing — tests run it after `fork`).
#[cfg(target_os = "linux")]
struct Exec {
    _argv: Vec<std::ffi::CString>,
    _env: Vec<std::ffi::CString>,
    argp: Vec<*const libc::c_char>,
    envp: Vec<*const libc::c_char>,
}

// SAFETY: the pointers point into the strings `Exec` owns and never change.
#[cfg(target_os = "linux")]
unsafe impl Send for Exec {}
#[cfg(target_os = "linux")]
unsafe impl Sync for Exec {}

#[cfg(target_os = "linux")]
impl Exec {
    fn new(bin: &Path, args: &[std::ffi::OsString]) -> std::io::Result<Self> {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let c = |s: &[u8]| CString::new(s).map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput));
        let mut argv = vec![c(bin.as_os_str().as_bytes())?];
        for a in args {
            argv.push(c(a.as_bytes())?);
        }
        let env: Vec<CString> = std::env::vars_os()
            .filter_map(|(k, v)| CString::new([k.as_bytes(), b"=", v.as_bytes()].concat()).ok())
            .collect();
        let mut argp: Vec<*const libc::c_char> = argv.iter().map(|a| a.as_ptr()).collect();
        argp.push(std::ptr::null());
        let mut envp: Vec<*const libc::c_char> = env.iter().map(|a| a.as_ptr()).collect();
        envp.push(std::ptr::null());
        Ok(Self { _argv: argv, _env: env, argp, envp })
    }

    /// `execveat(fd, "", …, AT_EMPTY_PATH)`: run what `fd` holds. Returns
    /// only on failure.
    fn run(&self, fd: libc::c_int) -> std::io::Error {
        // SAFETY: null-terminated arrays of C strings owned by `self`.
        unsafe { libc::syscall(libc::SYS_execveat, fd, b"\0".as_ptr().cast::<libc::c_char>(), self.argp.as_ptr(), self.envp.as_ptr(), libc::AT_EMPTY_PATH) };
        std::io::Error::last_os_error()
    }
}

/// Run what `fd` holds, named `bin`, with these arguments and this
/// process's environment. Returns only on failure.
#[cfg(target_os = "linux")]
fn exec_fd(fd: &std::os::fd::OwnedFd, bin: &Path, args: &[std::ffi::OsString]) -> std::io::Error {
    use std::os::fd::AsRawFd;
    match Exec::new(bin, args) {
        Ok(x) => x.run(fd.as_raw_fd()),
        Err(e) => e,
    }
}

/// One pass. `Some(bin)` when an update is in place and the server should
/// restart into it.
async fn pass() -> (std::time::Duration, Option<(PathBuf, Checked)>) {
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
    // Read once, checked once: what is put in place and run (review of F).
    let Some(checked) = check_file(&file, &sha) else {
        return fail("the downloaded update changed on disk — will try again".into());
    };
    // The channel again, right before installing: switched off "Tester"
    // since the check, a pre-release isn't installed.
    if !aqua_core::release::channel_allows(&f.version, tester()) {
        set(|s| s.state = "latest".into());
        return (EVERY, None);
    }
    let bin = match put_in_place(&file, &checked, &f.version, (&sums, &sig)) {
        Ok(b) => b,
        Err(e) => return fail(e),
    };
    set(|s| s.state = "installing".into());
    tracing::warn!(version = %f.version, bin = %bin.display(), "update: restarting into the new version");
    (EVERY, Some((bin, checked)))
}

/// Check now (the panel's button); restarts when there's a new version.
pub async fn run_once(before_exit: impl FnOnce() + Send + 'static) {
    if BUSY.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let (_, bin) = pass().await;
    BUSY.store(false, std::sync::atomic::Ordering::SeqCst);
    if let Some((bin, checked)) = bin {
        restart_into(bin, checked, before_exit);
    }
}

fn restart_into(bin: PathBuf, checked: Checked, before_exit: impl FnOnce() + Send + 'static) {
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(600));
        before_exit();
        // systemd: its restart runs the file in ExecStart — the checked bytes
        // written over it (put_in_place), or the old one, which checks and
        // runs the staged copy itself (run_newest).
        if let Some(unit) = crate::host::systemd_unit() {
            if std::process::Command::new("systemctl").arg("restart").arg(&unit).spawn().is_ok() {
                std::thread::sleep(std::time::Duration::from_secs(20));
            }
        }
        // The checked bytes, never the file by name (review of F).
        let e = launch(checked, &bin);
        tracing::error!(%e, "update: the checked binary couldn't be started — exiting; the next start checks it again");
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
            if let Some((bin, checked)) = bin {
                let b = before_exit.clone();
                restart_into(bin, checked, move || b());
                return;
            }
            tokio::time::sleep(wait).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{bin_name, keep_staged, newer, staged_is_genuine, wanted};

    #[test]
    fn a_pre_release_marked_stable_is_not_for_everyone() {
        // The feed says "not a pre-release"; its number says it is.
        let list = serde_json::json!([{ "tag_name": "v9.9.0-1", "prerelease": false }, { "tag_name": "v9.8.1", "prerelease": false }]);
        assert_eq!(super::best(&list, false).map(|b| b.0).as_deref(), Some("9.8.1"));
        assert_eq!(super::best(&list, true).map(|b| b.0).as_deref(), Some("9.9.0-1"));
    }

    /// A binary in the data folder runs only while a signed release list
    /// still names exactly it and its version.
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
        // Read once and checked: what is put in place and run are these
        // bytes — a write into the file, or another file renamed into its
        // place, after the check changes nothing of them.
        std::fs::write(d.join(bin_name()), b"new server").unwrap();
        let checked = super::staged_genuine(d, "9.0.1", Some(&pk)).expect("genuine");
        #[cfg(not(windows))]
        {
            std::fs::write(d.join(bin_name()), b"written into").unwrap();
            std::fs::write(d.join("evil"), b"swapped").unwrap();
            std::fs::rename(d.join("evil"), d.join(bin_name())).unwrap();
        }
        assert_eq!(checked.bytes, b"new server");
    }

    /// Linux: the checked bytes run from a memfd sealed against any change
    /// (no file, no /proc), and a change to it is refused.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_checked_bytes_run_sealed() {
        use std::io::Write;
        use std::os::fd::AsRawFd;
        use std::os::unix::process::CommandExt;
        let bytes = std::fs::read("/bin/echo").unwrap();
        let fd = super::sealed(&bytes).expect("a sealed memfd");
        let mut f = std::fs::File::from(fd.try_clone().unwrap());
        assert!(f.write_all(b"x").is_err(), "sealed against writing");
        assert!(f.set_len(1).is_err(), "and against shrinking");
        let x = super::Exec::new(std::path::Path::new("echo"), &["ran from the sealed memfd".into()]).unwrap();
        let raw = fd.as_raw_fd();
        // SAFETY: after fork only `run` (a syscall, no allocation) is called.
        let out = unsafe { std::process::Command::new("/bin/false").pre_exec(move || Err(x.run(raw))).output() }.unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout), "ran from the sealed memfd\n");
    }

    #[test]
    fn a_pre_release_with_a_bad_number_is_never_picked() {
        let list = serde_json::json!([{ "tag_name": "v9.9.0-rc1", "prerelease": false }, { "tag_name": "v9.9.0-1-2" }, { "tag_name": "v9.8.1" }]);
        assert_eq!(super::best(&list, true).map(|b| b.0).as_deref(), Some("9.8.1"));
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
