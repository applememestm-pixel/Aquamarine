//! Error reports from the messenger (1.2.0-10): crashes of the Rust side and
//! failures the UI catches (ui/src/main.ts → `report_error`).
//!
//! Each goes to the developer's report service (`tools/report-worker`,
//! address from `AQUA_REPORT_URL` at build time) — instantly, even when
//! the user's server is down — and to the user's own server
//! (`POST <server>/v1/report`, Aquamarine Server → "Errors").
//! On by default, Settings → About → "Send error reports" turns it off.
//! No message text: the failure, version, device, a random install id.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};

static ON: AtomicBool = AtomicBool::new(true);
static SERVER: Mutex<String> = Mutex::new(String::new());
static ID: Mutex<String> = Mutex::new(String::new());

fn report_url() -> Option<&'static str> {
    option_env!("AQUA_REPORT_URL").filter(|u| u.starts_with("https://"))
}

#[derive(serde::Serialize, serde::Deserialize, Default, Clone)]
pub struct Report {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub msg: String,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub app: String,
    #[serde(default)]
    pub ver: String,
    #[serde(default)]
    pub os: String,
    #[serde(default)]
    pub server: String,
    #[serde(default)]
    pub id: String,
}

struct Gate {
    seen: HashMap<String, Instant>,
    sent: VecDeque<Instant>,
}
static GATE: Mutex<Option<Gate>> = Mutex::new(None);

/// Same failure once per 10 minutes, at most 40 an hour.
fn admit(key: &str) -> bool {
    let Ok(mut g) = GATE.lock() else { return false };
    let g = g.get_or_insert_with(|| Gate { seen: HashMap::new(), sent: VecDeque::new() });
    let t = Instant::now();
    if g.seen.get(key).is_some_and(|p| t.duration_since(*p) < Duration::from_secs(600)) {
        return false;
    }
    while g.sent.front().is_some_and(|p| t.duration_since(*p) > Duration::from_secs(3600)) {
        g.sent.pop_front();
    }
    if g.sent.len() >= 40 {
        return false;
    }
    if g.seen.len() > 1000 {
        g.seen.clear();
    }
    g.seen.insert(key.into(), t);
    g.sent.push_back(t);
    true
}

/// Home folders and user names out of paths: `C:\Users\vlad\…` → `~\…`.
fn scrub(s: &str) -> String {
    let mut out = s.to_string();
    for var in ["HOME", "USERPROFILE"] {
        if let Ok(h) = std::env::var(var) {
            if h.len() > 3 {
                out = out.replace(&h, "~");
            }
        }
    }
    out
}

fn clip(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

pub fn send(mut r: Report) {
    if !ON.load(Ordering::Relaxed) {
        return;
    }
    r.app = "aquamarine".into();
    r.ver = env!("CARGO_PKG_VERSION").into();
    r.os = format!("{} {}", std::env::consts::OS, std::env::consts::ARCH);
    r.server = SERVER.lock().map(|g| g.clone()).unwrap_or_default();
    r.id = ID.lock().map(|g| g.clone()).unwrap_or_default();
    r.msg = clip(&scrub(&r.msg), 1000);
    r.detail = clip(&scrub(&r.detail), 6000);
    r.code = clip(&r.code, 40);
    if !admit(&format!("{}|{}|{}", r.kind, r.code, clip(&r.msg, 200))) {
        return;
    }
    let Ok(body) = serde_json::to_string(&r) else { return };
    let mut targets: Vec<String> = report_url().map(|u| vec![u.to_string()]).unwrap_or_default();
    if r.server.starts_with("http") {
        targets.push(format!("{}/v1/report", r.server.trim_end_matches('/')));
    }
    let go = async move {
        let Ok(c) = reqwest::Client::builder().timeout(Duration::from_secs(10)).build() else { return };
        for t in targets {
            let _ = c.post(&t).header("content-type", "application/json").body(body.clone()).send().await;
        }
    };
    match tokio::runtime::Handle::try_current() {
        Ok(h) => {
            h.spawn(go);
        }
        Err(_) => {
            let _ = std::thread::spawn(move || {
                if let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() {
                    rt.block_on(go);
                }
            })
            .join();
        }
    }
}

/// Crashes of the Rust side → a report (the usual message still printed).
pub fn install(app: &AppHandle) {
    // A random id of this installation, so reports from one device group up.
    let id = app
        .path()
        .app_data_dir()
        .ok()
        .map(|d| {
            let f = d.join("report-id");
            std::fs::read_to_string(&f).ok().map(|s| s.trim().to_string()).filter(|s| s.len() == 12).unwrap_or_else(|| {
                let s: String = (0..12).map(|_| char::from(b"0123456789abcdef"[rand::random::<usize>() % 16])).collect();
                let _ = std::fs::create_dir_all(&d);
                let _ = std::fs::write(&f, &s);
                s
            })
        })
        .unwrap_or_default();
    if let Ok(mut g) = ID.lock() {
        *g = id;
    }
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        send(Report {
            kind: "panic".into(),
            msg: info.to_string(),
            detail: std::backtrace::Backtrace::force_capture().to_string(),
            ..Default::default()
        });
        prev(info);
    }));
}

/// The UI caught a failure (JS error, unhandled promise, failed command).
#[tauri::command]
pub fn report_error(report: Report) {
    send(report);
}

/// Settings → "Send error reports", and the server the app talks to.
#[tauri::command]
pub fn report_config(enabled: bool, server: Option<String>) {
    ON.store(enabled, Ordering::Relaxed);
    if let (Some(s), Ok(mut g)) = (server, SERVER.lock()) {
        *g = s;
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn repeats_are_held_back() {
        assert!(super::admit("js||boom"));
        assert!(!super::admit("js||boom"));
    }
}
