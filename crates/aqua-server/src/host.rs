//! The machine the relay runs on (the panel's "Machine" card): what it is
//! (a server, a PC, a phone), its system, processor, memory, disk, network
//! and how long it's been up — and stop / restart from the panel.
//!
//! Read straight from the system, no extra libraries: `/proc` on Linux and
//! Android, `df` for the disk; on Windows and macOS only what Rust's
//! standard library knows (the server app on a PC shows the rest itself).

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{json, Value};

/// Set by the console server (`main.rs`): stop / restart from the panel
/// end this process. Inside the server app, the app does that.
pub static STANDALONE: AtomicBool = AtomicBool::new(false);

fn read(p: &str) -> Option<String> {
    std::fs::read_to_string(p).ok()
}

/// `/proc/meminfo`-style "Key:   123 kB" → bytes.
fn kb_field(text: &str, key: &str) -> Option<u64> {
    text.lines()
        .find(|l| l.starts_with(key) && l[key.len()..].starts_with(':'))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse::<u64>().ok())
        .map(|kb| kb * 1024)
}

/// (busy, total) jiffies of all CPUs.
fn cpu_ticks() -> Option<(u64, u64)> {
    let stat = read("/proc/stat")?;
    let line = stat.lines().next()?;
    let v: Vec<u64> = line.split_whitespace().skip(1).filter_map(|x| x.parse().ok()).collect();
    if v.len() < 4 {
        return None;
    }
    let idle = v[3] + v.get(4).copied().unwrap_or(0);
    let total: u64 = v.iter().sum();
    Some((total - idle, total))
}

/// Busy share of the processor over a quarter of a second, %.
fn cpu_percent() -> Option<f64> {
    let (b1, t1) = cpu_ticks()?;
    std::thread::sleep(std::time::Duration::from_millis(250));
    let (b2, t2) = cpu_ticks()?;
    let dt = t2.saturating_sub(t1);
    (dt > 0).then(|| (b2.saturating_sub(b1)) as f64 * 100.0 / dt as f64)
}

/// Total, used, free bytes of the disk holding `dir`.
fn disk(dir: &Path) -> Option<(u64, u64, u64)> {
    let out = std::process::Command::new("df").arg("-kP").arg(dir).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().nth(1)?;
    let f: Vec<u64> = line.split_whitespace().skip(1).take(3).filter_map(|x| x.parse().ok()).collect();
    (f.len() == 3).then(|| (f[0] * 1024, f[1] * 1024, f[2] * 1024))
}

/// Bytes received / sent by all network cards but the loopback.
fn net() -> Option<(u64, u64)> {
    let dev = read("/proc/net/dev")?;
    let (mut rx, mut tx) = (0u64, 0u64);
    for l in dev.lines().skip(2) {
        let Some((name, rest)) = l.split_once(':') else { continue };
        if name.trim() == "lo" {
            continue;
        }
        let v: Vec<u64> = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
        if v.len() >= 9 {
            rx += v[0];
            tx += v[8];
        }
    }
    Some((rx, tx))
}

/// "server", "pc" or "phone".
fn kind() -> &'static str {
    if cfg!(target_os = "android") || cfg!(target_os = "ios") {
        return "phone";
    }
    if cfg!(any(target_os = "windows", target_os = "macos")) {
        return "pc";
    }
    // Linux: a battery or a desktop / laptop case — a PC; otherwise (VPS,
    // rack, a virtual machine) — a server.
    let battery = std::fs::read_dir("/sys/class/power_supply")
        .map(|d| d.flatten().any(|e| e.file_name().to_string_lossy().starts_with("BAT")))
        .unwrap_or(false);
    let chassis: u32 = read("/sys/class/dmi/id/chassis_type").and_then(|s| s.trim().parse().ok()).unwrap_or(0);
    if battery || matches!(chassis, 3..=10 | 13..=16 | 30..=32) {
        "pc"
    } else {
        "server"
    }
}

fn os_name() -> String {
    if let Some(r) = read("/etc/os-release") {
        if let Some(n) = r.lines().find_map(|l| l.strip_prefix("PRETTY_NAME=")) {
            return n.trim_matches('"').to_string();
        }
    }
    match std::env::consts::OS {
        "android" => "Android".into(),
        "windows" => "Windows".into(),
        "macos" => "macOS".into(),
        o => o.into(),
    }
}

fn cpu_model() -> Option<String> {
    let info = read("/proc/cpuinfo")?;
    info.lines()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            matches!(k.trim(), "model name" | "Hardware" | "Processor" | "cpu model").then(|| v.trim().to_string())
        })
        .filter(|s| !s.is_empty())
}

/// The systemd unit this process runs in, if any ("aqua-server.service").
pub fn systemd_unit() -> Option<String> {
    std::env::var_os("INVOCATION_ID")?;
    let cg = read("/proc/self/cgroup")?;
    cg.lines()
        .flat_map(|l| l.rsplit('/').next())
        .find(|seg| seg.ends_with(".service"))
        .map(str::to_string)
}

/// Windows has no /proc: one PowerShell call (no window) for the system,
/// memory, processor, uptime and the data folder's disk.
#[cfg(target_os = "windows")]
fn windows_extra(dir: &Path) -> Option<Value> {
    use std::os::windows::process::CommandExt;
    let d = dir.display().to_string().replace('\'', "''");
    let script = format!(
        "[Console]::OutputEncoding=[Text.Encoding]::UTF8; $o=Get-CimInstance Win32_OperatingSystem; $c=Get-CimInstance Win32_Processor | Select-Object -First 1; \
         $q=Get-PSDrive -Name (Split-Path -Qualifier '{d}').TrimEnd(':'); \
         [pscustomobject]@{{os=$o.Caption; ver=$o.Version; host=$env:COMPUTERNAME; total=[uint64]$o.TotalVisibleMemorySize*1024; \
         free=[uint64]$o.FreePhysicalMemory*1024; up=[uint64]((Get-Date)-$o.LastBootUpTime).TotalSeconds; cpu=$c.Name; \
         load=$c.LoadPercentage; used=[uint64]$q.Used; dfree=[uint64]$q.Free}} | ConvertTo-Json -Compress"
    );
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .output()
        .ok()?;
    serde_json::from_str(String::from_utf8_lossy(&out.stdout).trim_start_matches('\u{feff}').trim()).ok()
}

/// Everything the "Machine" card shows. Blocks for ~¼ s (processor load).
pub fn snapshot(data_dir: &Path) -> Value {
    let mem = read("/proc/meminfo");
    let m = |k: &str| mem.as_deref().and_then(|t| kb_field(t, k));
    let me = read("/proc/self/status");
    let load: Option<Vec<f64>> = read("/proc/loadavg").map(|l| l.split_whitespace().take(3).filter_map(|x| x.parse().ok()).collect());
    let uptime = read("/proc/uptime").and_then(|u| u.split_whitespace().next()?.parse::<f64>().ok()).map(|s| s as u64);
    let d = disk(data_dir);
    let n = net();
    #[allow(unused_mut)]
    let mut v = json!({
        "kind": kind(),
        "hostname": read("/proc/sys/kernel/hostname").map(|s| s.trim().to_string())
            .or_else(|| std::env::var("COMPUTERNAME").ok()).or_else(|| std::env::var("HOSTNAME").ok()),
        "os": os_name(),
        "kernel": read("/proc/sys/kernel/osrelease").map(|s| s.trim().to_string()),
        "arch": std::env::consts::ARCH,
        "cpu": cpu_model(),
        "cores": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
        "cpu_pct": cpu_percent(),
        "load": load,
        "mem_total": m("MemTotal"),
        "mem_avail": m("MemAvailable"),
        "swap_total": m("SwapTotal"),
        "swap_free": m("SwapFree"),
        "disk_total": d.map(|x| x.0),
        "disk_used": d.map(|x| x.1),
        "disk_free": d.map(|x| x.2),
        "net_rx": n.map(|x| x.0),
        "net_tx": n.map(|x| x.1),
        "host_uptime": uptime,
        "proc_rss": me.as_deref().and_then(|t| kb_field(t, "VmRSS")),
        "proc_threads": me.as_deref().and_then(|t| t.lines().find_map(|l| l.strip_prefix("Threads:")).and_then(|v| v.trim().parse::<u64>().ok())),
        "pid": std::process::id(),
        "systemd": systemd_unit(),
        "power": STANDALONE.load(Ordering::Relaxed),
    });
    #[cfg(target_os = "windows")]
    if let Some(w) = windows_extra(data_dir) {
        let num = |k: &str| w[k].as_u64().map(Value::from).unwrap_or(Value::Null);
        let (total, free, used, dfree) = (w["total"].as_u64(), w["free"].as_u64(), w["used"].as_u64(), w["dfree"].as_u64());
        v["os"] = w["os"].clone();
        v["kernel"] = w["ver"].clone();
        v["hostname"] = w["host"].clone();
        v["cpu"] = w["cpu"].clone();
        v["cpu_pct"] = num("load");
        v["mem_total"] = num("total");
        v["mem_avail"] = num("free");
        v["host_uptime"] = num("up");
        if let (Some(u), Some(f)) = (used, dfree) {
            v["disk_total"] = Value::from(u + f);
            v["disk_used"] = Value::from(u);
            v["disk_free"] = Value::from(f);
        }
        let _ = (total, free);
    }
    v
}

/// Stop (`restart` = false) or restart this server, a moment after the
/// panel got its answer. Under systemd it asks systemd (so a stop stays
/// stopped whatever `Restart=` says); otherwise the process ends, and for a
/// restart starts itself again first.
pub fn power(restart: bool, before_exit: impl FnOnce() + Send + 'static) {
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(600));
        tracing::warn!(restart, "stopped from the admin panel");
        if let Some(unit) = systemd_unit() {
            let ok = std::process::Command::new("systemctl")
                .arg(if restart { "restart" } else { "stop" })
                .arg(&unit)
                .spawn()
                .is_ok();
            if ok {
                // systemd ends us; if it doesn't within a while, end anyway.
                std::thread::sleep(std::time::Duration::from_secs(20));
            }
        }
        before_exit();
        if restart {
            if let Ok(exe) = std::env::current_exe() {
                // The new one waits for the port (main.rs binds with retries).
                let _ = std::process::Command::new(exe).args(std::env::args_os().skip(1)).spawn();
            }
        }
        std::process::exit(if restart { 75 } else { 0 });
    });
}
