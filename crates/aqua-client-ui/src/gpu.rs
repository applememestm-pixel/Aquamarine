//! Graphics mode of the web view (Linux).
//!
//! When the GPU driver can't be used — most often an NVIDIA driver updated
//! without a reboot ("Driver/library version mismatch") — WebKitGTK still
//! composites through OpenGL, which Mesa then emulates on the CPU
//! (llvmpipe): every scroll step and animation costs a full software GL
//! frame and the whole app lags. Plain CPU painting without compositing is
//! several times cheaper, so in that case compositing is switched off
//! before the web view starts. The user can force either mode
//! (Settings → Behaviour → Graphics), stored in `~/.config/aquamarine/gpu-mode`.

use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct GpuStatus {
    /// "auto" | "on" | "off"
    pub mode: String,
    /// Compositing was switched off for this run.
    pub software: bool,
    /// Why ("nvidia-mismatch", "forced", "env", "").
    pub reason: String,
    /// WebKit's DMA-BUF renderer is on for this run (Linux: frames go to
    /// the compositor straight from the GPU instead of being copied
    /// through the CPU — much smoother at full HD, but on some NVIDIA +
    /// Wayland setups the window stays blank).
    #[serde(default)]
    pub dmabuf: bool,
    /// Just switched on and not confirmed yet: the UI asks "does it look
    /// right?" and switches back by itself if nobody answers.
    #[serde(default)]
    pub dmabuf_pending: bool,
}

static STATUS: OnceLock<GpuStatus> = OnceLock::new();

fn config_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .map(|d| d.join("aquamarine"))
}

fn mode_file() -> Option<PathBuf> {
    config_dir().map(|d| d.join("gpu-mode"))
}

fn dmabuf_file() -> Option<PathBuf> {
    config_dir().map(|d| d.join("dmabuf"))
}

/// Written when DMA-BUF is switched on, removed once the UI confirms the
/// window looks right. Still there at the next start = it didn't work.
fn dmabuf_pending_file() -> Option<PathBuf> {
    config_dir().map(|d| d.join("dmabuf-pending"))
}

fn dmabuf_saved() -> bool {
    dmabuf_file().and_then(|p| std::fs::read_to_string(p).ok()).is_some_and(|v| v.trim() == "on")
}

pub fn saved_mode() -> String {
    let m = mode_file().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
    match m.trim() {
        "on" | "off" => m.trim().to_string(),
        _ => "auto".into(),
    }
}

/// `nvidia-smi` says the kernel module and the libraries differ.
fn nvidia_mismatch() -> bool {
    let Ok(mut child) = std::process::Command::new("nvidia-smi")
        .arg("-L")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .stdin(std::process::Stdio::null())
        .spawn()
    else {
        return false;
    };
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() < Duration::from_millis(2500) => std::thread::sleep(Duration::from_millis(30)),
            _ => {
                let _ = child.kill();
                return false;
            }
        }
    }
    let Ok(out) = child.wait_with_output() else { return false };
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)).to_lowercase();
    text.contains("version mismatch") || text.contains("couldn't communicate with the nvidia driver")
}

/// Decide the mode and set WebKit's environment. Call before any thread or
/// web view exists.
pub fn init() -> GpuStatus {
    let mode = saved_mode();
    let (software, reason) = if std::env::var_os("WEBKIT_DISABLE_COMPOSITING_MODE").is_some_and(|v| v == "1") {
        (true, "env".to_string())
    } else {
        match mode.as_str() {
            "off" => (true, "forced".to_string()),
            "on" => (false, String::new()),
            _ if cfg!(target_os = "linux") && nvidia_mismatch() => (true, "nvidia-mismatch".to_string()),
            _ => (false, String::new()),
        }
    };
    if software && cfg!(target_os = "linux") {
        std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
        eprintln!("[aqua-gpu] drawing without GPU compositing ({reason})");
    }
    // DMA-BUF renderer: the launcher turns it off (blank windows on some
    // NVIDIA + Wayland setups); the user may turn it back on.
    let mut dmabuf = false;
    let mut dmabuf_pending = false;
    if cfg!(target_os = "linux") && !software && dmabuf_saved() {
        let pending = dmabuf_pending_file().is_some_and(|p| p.exists());
        if pending {
            // Switched on last time and never confirmed (blank window,
            // crash): back to the safe mode.
            eprintln!("[aqua-gpu] DMA-BUF renderer wasn't confirmed last time — switching it off");
            if let Some(p) = dmabuf_file() { let _ = std::fs::write(p, "off"); }
            if let Some(p) = dmabuf_pending_file() { let _ = std::fs::remove_file(p); }
        } else {
            std::env::remove_var("WEBKIT_DISABLE_DMABUF_RENDERER");
            dmabuf = true;
            eprintln!("[aqua-gpu] DMA-BUF renderer on");
        }
    }
    if dmabuf {
        // Confirmed already? (No pending marker = a confirmed earlier run.)
        dmabuf_pending = std::fs::read_to_string(config_dir().map(|d| d.join("dmabuf-confirmed")).unwrap_or_default())
            .is_err();
        if dmabuf_pending {
            if let Some(p) = dmabuf_pending_file() { let _ = std::fs::write(p, "1"); }
        }
    }
    let st = GpuStatus { mode, software, reason, dmabuf, dmabuf_pending };
    let _ = STATUS.set(st.clone());
    st
}

#[tauri::command]
pub fn gpu_status() -> GpuStatus {
    STATUS.get().cloned().unwrap_or(GpuStatus { mode: saved_mode(), software: false, reason: String::new(), dmabuf: false, dmabuf_pending: false })
}

/// Switch the DMA-BUF renderer on / off (after a restart).
#[tauri::command]
pub fn set_dmabuf(on: bool) -> Result<(), String> {
    let d = config_dir().ok_or("no config directory")?;
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    std::fs::write(d.join("dmabuf"), if on { "on" } else { "off" }).map_err(|e| e.to_string())?;
    // A fresh switch-on must be confirmed again.
    let _ = std::fs::remove_file(d.join("dmabuf-confirmed"));
    let _ = std::fs::remove_file(d.join("dmabuf-pending"));
    Ok(())
}

/// The window looks right with DMA-BUF: keep it.
#[tauri::command]
pub fn confirm_dmabuf() -> Result<(), String> {
    let d = config_dir().ok_or("no config directory")?;
    let _ = std::fs::remove_file(d.join("dmabuf-pending"));
    std::fs::write(d.join("dmabuf-confirmed"), "1").map_err(|e| e.to_string())
}

/// Restart the app (to apply a graphics change).
#[tauri::command]
pub fn restart_app(app: tauri::AppHandle) {
    app.restart();
}

/// Takes effect after a restart.
#[tauri::command]
pub fn set_gpu_mode(mode: String) -> Result<(), String> {
    if !matches!(mode.as_str(), "auto" | "on" | "off") {
        return Err("mode must be auto, on or off".into());
    }
    let p = mode_file().ok_or("no config directory")?;
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(&p, &mode).map_err(|e| e.to_string())
}
