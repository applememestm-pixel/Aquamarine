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
}

static STATUS: OnceLock<GpuStatus> = OnceLock::new();

fn mode_file() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .map(|d| d.join("aquamarine").join("gpu-mode"))
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
    let st = GpuStatus { mode, software, reason };
    let _ = STATUS.set(st.clone());
    st
}

#[tauri::command]
pub fn gpu_status() -> GpuStatus {
    STATUS.get().cloned().unwrap_or(GpuStatus { mode: saved_mode(), software: false, reason: String::new() })
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
