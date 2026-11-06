//! Error reports (1.2.0-10).
//!
//! * The server's own failures — `tracing` ERROR events and panics — go to
//!   the developer's report service (`tools/report-worker`, its address is
//!   baked in at build time from `AQUA_REPORT_URL`). Always on: a bug on a
//!   server is everyone's problem on it.
//! * Apps also send their failures here (`POST /v1/report`), so whoever runs
//!   this server sees them too: Aquamarine Server → "Errors"
//!   (`GET /v1/admin/reports`). Those aren't forwarded again — the apps
//!   send them to the report service themselves, so nothing arrives twice
//!   and reports keep coming even when this server is down.
//!
//! No message text is ever part of a report: the failure, version, device.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};

/// The developer's report service (`…/report`), if the build has one.
pub fn report_url() -> Option<&'static str> {
    option_env!("AQUA_REPORT_URL").filter(|u| u.starts_with("https://"))
}

#[derive(Clone, Serialize, Deserialize, Default, Debug)]
pub struct Report {
    #[serde(default)]
    pub app: String,
    #[serde(default)]
    pub ver: String,
    #[serde(default)]
    pub os: String,
    /// server | panic | error | js
    #[serde(default)]
    pub kind: String,
    /// A code from docs/errors.md, when the failure is a known one.
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub msg: String,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub server: String,
    /// A random id of the installation (not the account).
    #[serde(default)]
    pub id: String,
    /// Unix seconds; set on arrival.
    #[serde(default)]
    pub at: u64,
}

impl Report {
    fn clipped(mut self) -> Self {
        let cut = |s: &mut String, n: usize| {
            if s.len() > n {
                let mut i = n;
                while !s.is_char_boundary(i) {
                    i -= 1;
                }
                s.truncate(i);
            }
        };
        cut(&mut self.app, 40);
        cut(&mut self.ver, 24);
        cut(&mut self.os, 60);
        cut(&mut self.kind, 16);
        cut(&mut self.code, 40);
        cut(&mut self.msg, 1000);
        cut(&mut self.detail, 6000);
        cut(&mut self.server, 120);
        cut(&mut self.id, 16);
        self
    }
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

// ---------- sending to the report service ----------------------------------

struct Gate {
    seen: HashMap<String, Instant>,
    sent: VecDeque<Instant>,
}
static GATE: Mutex<Option<Gate>> = Mutex::new(None);

/// Same failure once per 10 minutes, at most 60 an hour from this process.
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
    if g.sent.len() >= 60 {
        return false;
    }
    if g.seen.len() > 2000 {
        g.seen.clear();
    }
    g.seen.insert(key.to_string(), t);
    g.sent.push_back(t);
    true
}

/// Send to the report service (in the background; never blocks, never fails).
pub fn send(r: Report) {
    let Some(url) = report_url() else { return };
    let mut r = r.clipped();
    if r.at == 0 {
        r.at = now();
    }
    if !admit(&format!("{}|{}|{}|{}", r.app, r.ver, r.code, r.msg.chars().take(200).collect::<String>())) {
        return;
    }
    let go = async move {
        let c = reqwest::Client::builder().timeout(Duration::from_secs(10)).build();
        if let Ok(c) = c {
            let _ = c.post(url).json(&r).send().await;
        }
    };
    match tokio::runtime::Handle::try_current() {
        Ok(h) => {
            h.spawn(go);
        }
        // A panic hook or a thread without a runtime: a short one of its own.
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

fn base(app: &str, kind: &str) -> Report {
    Report {
        app: app.into(),
        ver: env!("CARGO_PKG_VERSION").into(),
        os: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        kind: kind.into(),
        server: public_name(),
        ..Default::default()
    }
}

static NAME: Mutex<String> = Mutex::new(String::new());
/// How this server shows up in reports (its public address, if any).
pub fn set_public_name(s: &str) {
    if let Ok(mut g) = NAME.lock() {
        *g = s.to_string();
    }
}
fn public_name() -> String {
    NAME.lock().map(|g| g.clone()).unwrap_or_default()
}

/// Panics of this process → a report (then the usual message on stderr).
pub fn install_panic_hook(app: &'static str) {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let mut r = base(app, "panic");
        r.msg = info.to_string();
        r.detail = std::backtrace::Backtrace::force_capture().to_string();
        send(r);
        prev(info);
    }));
}

/// A `tracing` layer: every ERROR event of the server becomes a report.
pub struct ErrorLayer {
    pub app: &'static str,
}

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for ErrorLayer {
    fn on_event(&self, ev: &tracing::Event<'_>, _cx: tracing_subscriber::layer::Context<'_, S>) {
        if *ev.metadata().level() != tracing::Level::ERROR {
            return;
        }
        struct V(String);
        impl tracing::field::Visit for V {
            fn record_debug(&mut self, f: &tracing::field::Field, v: &dyn std::fmt::Debug) {
                if !self.0.is_empty() {
                    self.0.push(' ');
                }
                if f.name() == "message" {
                    self.0.push_str(&format!("{v:?}"));
                } else {
                    self.0.push_str(&format!("{}={v:?}", f.name()));
                }
            }
        }
        let mut v = V(String::new());
        ev.record(&mut v);
        let mut r = base(self.app, "server");
        r.code = format!("SRV:{}", ev.metadata().target());
        r.msg = v.0;
        r.detail = format!("{}:{}", ev.metadata().file().unwrap_or("?"), ev.metadata().line().unwrap_or(0));
        send(r);
    }
}

// ---------- reports from apps, shown to this server's admin ----------------

struct Inbox {
    list: VecDeque<Report>,
    per_ip: HashMap<String, VecDeque<Instant>>,
}
static INBOX: Mutex<Option<Inbox>> = Mutex::new(None);
const KEEP: usize = 300;
const LOG: &str = "aqua-reports.log";

/// `POST /v1/report` — an app's failure (≤ 16 KB; ≤ 20 a minute per address
/// behind a proxy that names it, ≤ 120 a minute in all otherwise).
pub async fn receive(headers: axum::http::HeaderMap, body: axum::body::Bytes) -> StatusCode {
    if body.len() > 16 * 1024 {
        return StatusCode::PAYLOAD_TOO_LARGE;
    }
    let Ok(r) = serde_json::from_slice::<Report>(&body) else { return StatusCode::BAD_REQUEST };
    let mut r = r.clipped();
    r.at = now();
    {
        let Ok(mut g) = INBOX.lock() else { return StatusCode::INTERNAL_SERVER_ERROR };
        let g = g.get_or_insert_with(|| Inbox { list: VecDeque::new(), per_ip: HashMap::new() });
        let t = Instant::now();
        let who = headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(',').next())
            .map(|v| v.trim().to_string());
        let limit = if who.is_some() { 20 } else { 120 };
        let w = g.per_ip.entry(who.unwrap_or_else(|| "*".into())).or_default();
        while w.front().is_some_and(|p| t.duration_since(*p) > Duration::from_secs(60)) {
            w.pop_front();
        }
        if w.len() >= limit {
            return StatusCode::TOO_MANY_REQUESTS;
        }
        w.push_back(t);
        if g.per_ip.len() > 5000 {
            g.per_ip.clear();
        }
        g.list.push_back(r.clone());
        while g.list.len() > KEEP {
            g.list.pop_front();
        }
    }
    if let Ok(line) = serde_json::to_string(&r) {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(LOG) {
            let _ = writeln!(f, "{line}");
        }
    }
    StatusCode::NO_CONTENT
}

/// The newest reports first (for Aquamarine Server → "Errors").
pub fn recent() -> Vec<Report> {
    let mut v: Vec<Report> = INBOX.lock().ok().and_then(|g| g.as_ref().map(|i| i.list.iter().cloned().collect())).unwrap_or_default();
    if v.is_empty() {
        // After a restart: the tail of the log file.
        if let Ok(t) = std::fs::read_to_string(LOG) {
            v = t.lines().rev().take(KEEP).filter_map(|l| serde_json::from_str(l).ok()).collect();
            v.reverse();
        }
    }
    v.reverse();
    v
}

pub async fn admin_list(State(state): State<crate::AppState>, headers: axum::http::HeaderMap) -> Result<Json<serde_json::Value>, StatusCode> {
    crate::admin_authed(&state, &headers)?;
    Ok(Json(serde_json::json!({ "reports": recent(), "forwarding": report_url().is_some() })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_failure_is_sent_once_per_ten_minutes() {
        assert!(admit("a|1|x|boom"));
        assert!(!admit("a|1|x|boom"));
        assert!(admit("a|1|x|other"));
    }

    #[test]
    fn long_fields_are_cut_on_char_boundaries() {
        let r = Report { msg: "я".repeat(900), ..Default::default() }.clipped();
        assert!(r.msg.len() <= 1000);
        assert!(r.msg.chars().all(|c| c == 'я'));
    }
}
