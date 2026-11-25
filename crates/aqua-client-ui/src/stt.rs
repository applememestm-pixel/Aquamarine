//! Voice-to-text (aqua-stt) for the app: the one-time download of the model
//! and ONNX Runtime, and transcripts of voice messages, circles and audio.
//!
//! Everything happens on this device — the server never gets the sound.
//! The files are the device's, not an account's: `<shared root>/stt/<tag>/`.
//! The WebView decodes the audio (any format it can play) to 16 kHz mono
//! and sends the samples here as the raw request body (s16le).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex as StdMutex;

use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, State};

use crate::state::AppState;

const REPO: &str = "applememestm-pixel/Aquamarine";
/// Written after every file passed its SHA-256: the files are ready.
const MARKER: &str = "verified";

fn stt_root(state: &State<'_, StdMutex<AppState>>) -> PathBuf {
    let dir = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).data_dir.clone();
    crate::accounts::shared_root(&dir).join("stt")
}

fn files_dir(root: &Path) -> PathBuf {
    root.join(aqua_stt::RELEASE_TAG)
}

fn ready(dir: &Path) -> bool {
    !aqua_stt::needed().is_empty()
        && dir.join(MARKER).is_file()
        && aqua_stt::needed().iter().all(|f| std::fs::metadata(dir.join(f.name)).map(|m| m.len() == f.size).unwrap_or(false))
}

#[derive(Serialize)]
pub struct SttStatus {
    /// Voice-to-text can run on this platform.
    supported: bool,
    /// Downloaded and checked: transcripts work (offline too).
    ready: bool,
    /// Bytes to download in all (shown before asking).
    size: u64,
    /// A download is running.
    downloading: bool,
}

static DOWNLOADING: AtomicBool = AtomicBool::new(false);

#[tauri::command]
pub fn stt_status(state: State<'_, StdMutex<AppState>>) -> SttStatus {
    let dir = files_dir(&stt_root(&state));
    let supported = aqua_stt::supported() && !aqua_stt::needed().is_empty();
    SttStatus {
        supported,
        ready: supported && ready(&dir),
        size: aqua_stt::needed().iter().map(|f| f.size).sum(),
        downloading: DOWNLOADING.load(Ordering::Relaxed),
    }
}

#[derive(Clone, Serialize)]
struct Progress {
    done: u64,
    total: u64,
}

/// Downloads what's missing (resuming a cut-off file), checks every file's
/// SHA-256 against the one built into the app. Progress: `aqua://stt-progress`.
#[tauri::command]
pub async fn stt_download(app: AppHandle, state: State<'_, StdMutex<AppState>>) -> Result<(), String> {
    if !aqua_stt::supported() || aqua_stt::needed().is_empty() {
        return Err("voice-to-text isn't available on this device".into());
    }
    if DOWNLOADING.swap(true, Ordering::SeqCst) {
        return Err("already downloading".into());
    }
    struct Done;
    impl Drop for Done {
        fn drop(&mut self) {
            DOWNLOADING.store(false, Ordering::SeqCst);
        }
    }
    let _done = Done;
    let root = stt_root(&state);
    let dir = files_dir(&root);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    // Files of an older model (another tag) go.
    if let Ok(rd) = std::fs::read_dir(&root) {
        for e in rd.flatten() {
            if e.path() != dir {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
    let _ = std::fs::remove_file(dir.join(MARKER));
    let needed = aqua_stt::needed();
    let total: u64 = needed.iter().map(|f| f.size).sum();
    let have = |f: &aqua_stt::FileSpec| std::fs::metadata(dir.join(f.name)).map(|m| m.len() == f.size).unwrap_or(false);
    let mut done: u64 = needed.iter().filter(|f| have(f)).map(|f| f.size).sum();
    let _ = app.emit("aqua://stt-progress", Progress { done, total });
    let http = aqua_client_core::transport::http_client().map_err(|e| e.to_string())?;
    for f in &needed {
        let path = dir.join(f.name);
        if have(f) && file_sha256(&path).as_deref() == Some(f.sha256) {
            continue;
        }
        let _ = std::fs::remove_file(&path);
        let part = dir.join(format!("{}.part", f.name));
        let mut got = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        if got >= f.size {
            let _ = std::fs::remove_file(&part);
            got = 0;
        }
        let url = format!("https://github.com/{REPO}/releases/download/{}/{}", aqua_stt::RELEASE_TAG, f.name);
        let mut req = http.get(&url);
        if got > 0 {
            req = req.header("Range", format!("bytes={got}-"));
        }
        let mut r = req.send().await.map_err(|e| format!("download: {e}"))?;
        if !r.status().is_success() {
            return Err(format!("download: HTTP {}", r.status()));
        }
        let mut out = if got > 0 && r.status().as_u16() == 206 {
            std::fs::OpenOptions::new().append(true).open(&part).map_err(|e| e.to_string())?
        } else {
            got = 0;
            std::fs::File::create(&part).map_err(|e| e.to_string())?
        };
        let base = done;
        let mut last = 0u64;
        while let Some(chunk) = r.chunk().await.map_err(|e| format!("download: {e}"))? {
            out.write_all(&chunk).map_err(|e| e.to_string())?;
            got += chunk.len() as u64;
            if got > f.size {
                break;
            }
            if got - last >= 512 * 1024 {
                last = got;
                let _ = app.emit("aqua://stt-progress", Progress { done: base + got, total });
            }
        }
        out.flush().map_err(|e| e.to_string())?;
        drop(out);
        let sum = file_sha256(&part);
        if sum.as_deref() != Some(f.sha256) {
            let _ = std::fs::remove_file(&part);
            return Err("the download is damaged — try again".into());
        }
        std::fs::rename(&part, &path).map_err(|e| e.to_string())?;
        done = base + f.size;
        let _ = app.emit("aqua://stt-progress", Progress { done, total });
    }
    std::fs::write(dir.join(MARKER), aqua_stt::RELEASE_TAG).map_err(|e| e.to_string())?;
    Ok(())
}

/// Deletes the model and ONNX Runtime (Settings → Storage).
#[tauri::command]
pub fn stt_delete(state: State<'_, StdMutex<AppState>>) -> Result<(), String> {
    if DOWNLOADING.load(Ordering::SeqCst) {
        return Err("a download is running".into());
    }
    #[cfg(not(target_os = "ios"))]
    {
        *RECOGNIZER.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }
    let root = stt_root(&state);
    match std::fs::remove_dir_all(&root) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(not(target_os = "ios"))]
static RECOGNIZER: StdMutex<Option<aqua_stt::Recognizer>> = StdMutex::new(None);

/// Text of a recording: 16 kHz mono s16le samples.
#[tauri::command]
pub async fn stt_transcribe(state: State<'_, StdMutex<AppState>>, pcm: Vec<u8>) -> Result<String, String> {
    if pcm.len() % 2 != 0 {
        return Err("invalid recording length".into());
    }
    let pcm: Vec<f32> = pcm.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect();
    let dir = files_dir(&stt_root(&state));
    if !ready(&dir) {
        return Err("not downloaded".into());
    }
    #[cfg(target_os = "ios")]
    {
        let _ = pcm;
        Err("voice-to-text isn't available on this device".into())
    }
    #[cfg(not(target_os = "ios"))]
    tokio::task::spawn_blocking(move || {
        let mut g = RECOGNIZER.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if g.is_none() {
            // Leave a core or two for the interface (and the phone's heat).
            let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2);
            let threads = if cfg!(target_os = "android") { cores.clamp(1, 4) / 2 + 1 } else { (cores / 2).clamp(1, 6) };
            *g = Some(aqua_stt::Recognizer::load(&dir, threads)?);
        }
        g.as_mut().map(|r| r.transcribe(&pcm)).unwrap_or_else(|| Err("not loaded".into()))
    })
    .await
    .map_err(|e| e.to_string())?
}

fn file_sha256(p: &Path) -> Option<String> {
    let mut f = std::fs::File::open(p).ok()?;
    let mut h = Sha256::new();
    std::io::copy(&mut f, &mut HashW(&mut h)).ok()?;
    Some(hex::encode(h.finalize()))
}

struct HashW<'a>(&'a mut Sha256);
impl Write for HashW<'_> {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.update(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}


#[tauri::command]
pub async fn server_processing(
    state: State<'_, StdMutex<AppState>>,
    operation: String,
    text: Option<String>,
    target: Option<String>,
    pcm: Option<String>,
) -> Result<serde_json::Value, String> {
    let transport = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
        .transport.clone().ok_or("not connected")?;
    let base = transport.server_url().trim_end_matches('/');
    let request = match operation.as_str() {
        "capabilities" => transport.http().get(format!("{base}/v1/me/processing")),
        "translate" => transport.http().post(format!("{base}/v1/me/translate")).json(
            &serde_json::json!({ "text": text.ok_or("missing text")?, "target": target.ok_or("missing language")? }),
        ),
        "speech" => {
            use base64::Engine;
            let encoded = pcm.ok_or("missing audio")?;
            if encoded.len() > 25_600_000 { return Err("recording exceeds ten minutes".into()); }
            let pcm = base64::engine::general_purpose::STANDARD.decode(encoded).map_err(|e| e.to_string())?;
            if pcm.len() > 16_000 * 2 * 600 { return Err("recording exceeds ten minutes".into()); }
            transport.http().post(format!("{base}/v1/me/speech"))
                .header("Content-Type", "application/octet-stream").body(pcm)
                .timeout(std::time::Duration::from_secs(200))
        }
        _ => return Err("unsupported operation".into()),
    };
    let response = request.bearer_auth(transport.bearer().await.map_err(|e| e.to_string())?)
        .send().await.map_err(|e| e.to_string())?;
    if !response.status().is_success() { return Err(format!("HTTP {}", response.status())); }
    response.json().await.map_err(|e| e.to_string())
}
