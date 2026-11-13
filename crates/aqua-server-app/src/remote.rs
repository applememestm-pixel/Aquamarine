//! Remote servers (1.20.1-6): the panel of an Aquamarine server on another
//! machine, reached over SSH — the bar at the bottom of the window switches
//! between "This device" and these.
//!
//! * Connect: SSH (password or private key; the host key is remembered on
//!   the first connection and checked after that), then the panel's port and
//!   token are read from `panel.json` on that machine (the console server and
//!   this app write it: aqua_server::host::write_panel_file), unless they are
//!   given by hand (Docker keeps it inside the container).
//! * The panel's requests go through a local port forwarded over the same
//!   SSH connection to 127.0.0.1:<port> there — the relay's admin API never
//!   has to be open to the internet.
//! * Saved in `remotes.json` next to `server-app.json` (mode 600). The
//!   password / key text only with "Remember on this device"; otherwise it
//!   lives in memory until the app quits.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use russh::client;
use russh::keys::{decode_secret_key, HashAlg, PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const FILE: &str = "remotes.json";

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct Remote {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub host: String,
    #[serde(default = "ssh_port")]
    pub ssh_port: u16,
    pub user: String,
    /// "password" | "key"
    #[serde(default)]
    pub auth: String,
    /// The password or the private key (OpenSSH / PEM text) — only when remembered.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub secret: String,
    /// The key's passphrase, if it has one (remembered with it).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub passphrase: String,
    /// The relay's port there; 0 = from panel.json.
    #[serde(default)]
    pub server_port: u16,
    /// The admin token; "" = from panel.json.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub token: String,
    /// SHA256 fingerprint of the host key, from the first connection.
    #[serde(default)]
    pub fingerprint: String,
}

fn ssh_port() -> u16 {
    22
}

/// What the window sees: no secrets.
#[derive(Serialize)]
pub struct RemoteView {
    id: String,
    name: String,
    host: String,
    ssh_port: u16,
    user: String,
    auth: String,
    server_port: u16,
    has_secret: bool,
    has_token: bool,
    remembered: bool,
    fingerprint: String,
    /// connected | connecting | off | error
    state: String,
    error: Option<String>,
    /// The forwarded panel: http://127.0.0.1:<local port> (pictures of packs).
    local_url: Option<String>,
    /// The relay's port on that machine.
    port: u16,
    version: Option<String>,
}

struct Conn {
    handle: Arc<client::Handle<Hk>>,
    token: String,
    port: u16,
    local: u16,
    version: Option<String>,
    accept: tauri::async_runtime::JoinHandle<()>,
}

#[derive(Default)]
struct Live {
    conns: HashMap<String, Conn>,
    /// Secrets typed without "Remember": for this run only.
    secrets: HashMap<String, (String, String)>,
    errors: HashMap<String, String>,
    connecting: std::collections::HashSet<String>,
}

static LIVE: Mutex<Option<Live>> = Mutex::new(None);

fn live<R>(f: impl FnOnce(&mut Live) -> R) -> R {
    let mut g = LIVE.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(Live::default))
}

fn path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join(FILE))
}

fn load(app: &AppHandle) -> Vec<Remote> {
    path(app).and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save(app: &AppHandle, list: &[Remote]) -> Result<(), String> {
    let p = path(app).ok_or("no config folder")?;
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let body = serde_json::to_vec_pretty(list).map_err(|e| e.to_string())?;
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, body).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
    }
    std::fs::rename(&tmp, &p).map_err(|e| e.to_string())
}

fn view(r: &Remote) -> RemoteView {
    live(|l| {
        let c = l.conns.get(&r.id).filter(|c| !c.handle.is_closed());
        let mem = l.secrets.contains_key(&r.id);
        let state = if c.is_some() {
            "connected"
        } else if l.connecting.contains(&r.id) {
            "connecting"
        } else if l.errors.contains_key(&r.id) {
            "error"
        } else {
            "off"
        };
        RemoteView {
            id: r.id.clone(),
            name: if r.name.trim().is_empty() { format!("{}@{}", r.user, r.host) } else { r.name.clone() },
            host: r.host.clone(),
            ssh_port: r.ssh_port,
            user: r.user.clone(),
            auth: if r.auth.is_empty() { "password".into() } else { r.auth.clone() },
            server_port: r.server_port,
            has_secret: !r.secret.is_empty() || mem,
            has_token: !r.token.is_empty(),
            remembered: !r.secret.is_empty(),
            fingerprint: r.fingerprint.clone(),
            state: state.into(),
            error: if c.is_some() { None } else { l.errors.get(&r.id).cloned() },
            local_url: c.map(|c| format!("http://127.0.0.1:{}", c.local)),
            port: c.map(|c| c.port).unwrap_or(r.server_port),
            version: c.and_then(|c| c.version.clone()),
        }
    })
}

/// The SSH side: remembers the host key it was shown.
struct Hk {
    expected: String,
    seen: Arc<Mutex<String>>,
}

impl client::Handler for Hk {
    type Error = russh::Error;

    async fn check_server_key(&mut self, key: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        let fp = match key {
            PublicKeyOrCertificate::PublicKey { key, .. } => key.fingerprint(HashAlg::Sha256).to_string(),
            PublicKeyOrCertificate::Certificate(c) => c.public_key().fingerprint(HashAlg::Sha256).to_string(),
        };
        *self.seen.lock().unwrap_or_else(|e| e.into_inner()) = fp.clone();
        Ok(self.expected.is_empty() || self.expected == fp)
    }
}

/// A command on that machine; its output (stdout) and exit code.
async fn exec(h: &client::Handle<Hk>, cmd: &str) -> Result<(String, Option<u32>), String> {
    let mut ch = h.channel_open_session().await.map_err(|e| e.to_string())?;
    ch.exec(true, cmd).await.map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    let mut code = None;
    let read = async {
        while let Some(m) = ch.wait().await {
            match m {
                russh::ChannelMsg::Data { data } => out.extend_from_slice(&data),
                russh::ChannelMsg::ExitStatus { exit_status } => code = Some(exit_status),
                _ => {}
            }
            if out.len() > 1 << 20 {
                break;
            }
        }
    };
    let _ = tokio::time::timeout(std::time::Duration::from_secs(20), read).await;
    Ok((String::from_utf8_lossy(&out).into_owned(), code))
}

/// `panel.json` of the server there (Linux / macOS, then Windows' cmd).
async fn read_panel(h: &client::Handle<Hk>) -> Option<serde_json::Value> {
    const SH: &str = r#"for f in "${XDG_CONFIG_HOME:-$HOME/.config}/aquamarine/panel.json" "$HOME/.config/aquamarine/panel.json" "$HOME/Library/Application Support/aquamarine/panel.json"; do [ -r "$f" ] && cat "$f" && exit 0; done; exit 3"#;
    const CMD: &str = r#"type "%APPDATA%\aquamarine\panel.json""#;
    for c in [SH, CMD] {
        if let Ok((out, _)) = exec(h, c).await {
            if let Some(i) = out.find('{') {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(out[i..].trim()) {
                    if v["token"].is_string() {
                        return Some(v);
                    }
                }
            }
        }
    }
    None
}

fn friendly(e: &russh::Error) -> String {
    match e {
        russh::Error::UnknownKey => "the machine's SSH key has changed since the last connection — if that's expected, remove the server and add it again".into(),
        russh::Error::ConnectionTimeout => "no answer from the machine (timeout)".into(),
        _ => e.to_string(),
    }
}

async fn connect_inner(app: &AppHandle, id: &str) -> Result<(), String> {
    let list = load(app);
    let r = list.iter().find(|r| r.id == id).cloned().ok_or("no such server")?;
    let (secret, passphrase) = if !r.secret.is_empty() {
        (r.secret.clone(), r.passphrase.clone())
    } else {
        live(|l| l.secrets.get(id).cloned()).ok_or("enter the password or the key first")?
    };
    let seen = Arc::new(Mutex::new(String::new()));
    let cfg = Arc::new(client::Config {
        inactivity_timeout: None,
        keepalive_interval: Some(std::time::Duration::from_secs(20)),
        keepalive_max: 4,
        nodelay: true,
        ..Default::default()
    });
    let hk = Hk { expected: r.fingerprint.clone(), seen: seen.clone() };
    let addr = (r.host.trim().to_string(), r.ssh_port);
    let mut h = tokio::time::timeout(std::time::Duration::from_secs(20), client::connect(cfg, addr, hk))
        .await
        .map_err(|_| "no answer from the machine (timeout)".to_string())?
        .map_err(|e| friendly(&e))?;
    let user = r.user.trim().to_string();
    let ok = if r.auth == "key" {
        let key = decode_secret_key(secret.trim(), (!passphrase.is_empty()).then_some(passphrase.as_str())).map_err(|e| format!("the key: {e}"))?;
        let hash = h.best_supported_rsa_hash().await.map_err(|e| e.to_string())?.flatten();
        h.authenticate_publickey(user, PrivateKeyWithHashAlg::new(Arc::new(key), hash)).await.map_err(|e| e.to_string())?.success()
    } else {
        h.authenticate_password(user, secret).await.map_err(|e| e.to_string())?.success()
    };
    if !ok {
        return Err("SSH: wrong login, password or key".into());
    }
    // First connection: remember the host key.
    let fp = seen.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if r.fingerprint.is_empty() && !fp.is_empty() {
        let mut list = load(app);
        if let Some(x) = list.iter_mut().find(|x| x.id == id) {
            x.fingerprint = fp;
        }
        let _ = save(app, &list);
    }
    // Port and token: given, or from panel.json.
    let panel = if r.token.is_empty() || r.server_port == 0 { read_panel(&h).await } else { None };
    let token = if !r.token.is_empty() { r.token.clone() } else { panel.as_ref().and_then(|p| p["token"].as_str()).map(str::to_string).unwrap_or_default() };
    let port = if r.server_port != 0 { r.server_port } else { panel.as_ref().and_then(|p| p["port"].as_u64()).map(|p| p as u16).unwrap_or(7777) };
    if token.is_empty() {
        return Err("connected, but no Aquamarine server is running there (no panel.json) — start it, or enter its admin token by hand".into());
    }
    let version = panel.as_ref().and_then(|p| p["version"].as_str()).map(str::to_string);
    let handle = Arc::new(h);
    // The local end of the tunnel.
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.map_err(|e| e.to_string())?;
    let local = listener.local_addr().map_err(|e| e.to_string())?.port();
    let h2 = handle.clone();
    let accept = tauri::async_runtime::spawn(async move {
        loop {
            let Ok((mut sock, from)) = listener.accept().await else { break };
            let h = h2.clone();
            tokio::spawn(async move {
                match h.channel_open_direct_tcpip("127.0.0.1", port as u32, from.ip().to_string(), from.port() as u32).await {
                    Ok(ch) => {
                        let mut st = ch.into_stream();
                        let _ = tokio::io::copy_bidirectional(&mut sock, &mut st).await;
                    }
                    Err(e) => tracing::warn!(%e, "remote panel: tunnel"),
                }
            });
            if h2.is_closed() {
                break;
            }
        }
    });
    let old = live(|l| {
        l.errors.remove(id);
        l.conns.insert(id.to_string(), Conn { handle, token, port, local, version, accept })
    });
    if let Some(o) = old {
        o.accept.abort();
        let _ = o.handle.disconnect(russh::Disconnect::ByApplication, "", "en").await;
    }
    Ok(())
}

pub async fn connect(app: &AppHandle, id: &str) -> Result<RemoteView, String> {
    let first = live(|l| l.connecting.insert(id.to_string()));
    if !first {
        return Err("already connecting".into());
    }
    let res = connect_inner(app, id).await;
    live(|l| {
        l.connecting.remove(id);
        if let Err(e) = &res {
            l.errors.insert(id.to_string(), e.clone());
        }
    });
    let r = load(app).into_iter().find(|r| r.id == id).ok_or("no such server")?;
    res.map(|_| view(&r))
}

pub async fn disconnect(id: &str) {
    if let Some(c) = live(|l| {
        l.errors.remove(id);
        l.conns.remove(id)
    }) {
        c.accept.abort();
        let _ = c.handle.disconnect(russh::Disconnect::ByApplication, "", "en").await;
    }
}

/// Port and token of a connected server: the tunnel's local port.
fn target(id: &str) -> Option<(u16, String)> {
    live(|l| l.conns.get(id).filter(|c| !c.handle.is_closed()).map(|c| (c.local, c.token.clone())))
}

/// The panel's `/v1/admin/…` call to a remote server (lib.rs `admin`).
/// A dropped SSH connection is made again once.
pub async fn admin_target(app: &AppHandle, id: &str) -> Result<(u16, String), String> {
    if let Some(t) = target(id) {
        return Ok(t);
    }
    connect(app, id).await?;
    target(id).ok_or_else(|| "not connected".to_string())
}

// ---------------------------------------------------------------- commands

#[tauri::command]
pub fn remote_list(app: AppHandle) -> Vec<RemoteView> {
    load(&app).iter().map(view).collect()
}

#[derive(Deserialize)]
pub struct RemoteIn {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    host: String,
    #[serde(default)]
    ssh_port: u16,
    user: String,
    #[serde(default)]
    auth: String,
    /// New password / key text; empty = keep the one there is.
    #[serde(default)]
    secret: String,
    #[serde(default)]
    passphrase: String,
    #[serde(default)]
    remember: bool,
    #[serde(default)]
    server_port: u16,
    /// New token; empty = keep (or from panel.json).
    #[serde(default)]
    token: String,
    #[serde(default)]
    clear_token: bool,
}

/// Add or change a server; returns its id.
#[tauri::command]
pub fn remote_save(app: AppHandle, r: RemoteIn) -> Result<String, String> {
    let host = r.host.trim().trim_start_matches("ssh://").to_string();
    if host.is_empty() || host.contains(char::is_whitespace) || host.contains('/') {
        return Err("enter the machine's address (name or IP)".into());
    }
    if r.user.trim().is_empty() {
        return Err("enter the SSH login".into());
    }
    let mut list = load(&app);
    let id = if r.id.is_empty() {
        use rand::RngCore;
        let mut b = [0u8; 6];
        rand::thread_rng().fill_bytes(&mut b);
        hex::encode(b)
    } else {
        r.id.clone()
    };
    let i = match list.iter().position(|x| x.id == id) {
        Some(i) => i,
        None => {
            list.push(Remote { id: id.clone(), ..Default::default() });
            list.len() - 1
        }
    };
    let port = if r.ssh_port == 0 { 22 } else { r.ssh_port };
    let x = &mut list[i];
    // Another machine: its host key is new too.
    if x.host != host || x.ssh_port != port {
        x.fingerprint.clear();
    }
    x.name = r.name.trim().to_string();
    x.host = host;
    x.ssh_port = port;
    x.user = r.user.trim().to_string();
    x.auth = if r.auth == "key" { "key".into() } else { "password".into() };
    x.server_port = r.server_port;
    if r.clear_token {
        x.token.clear();
    } else if !r.token.trim().is_empty() {
        x.token = r.token.trim().to_string();
    }
    if !r.secret.is_empty() {
        if r.remember {
            x.secret = r.secret.clone();
            x.passphrase = r.passphrase.clone();
            live(|l| l.secrets.remove(&id));
        } else {
            x.secret.clear();
            x.passphrase.clear();
            live(|l| l.secrets.insert(id.clone(), (r.secret.clone(), r.passphrase.clone())));
        }
    } else if !r.remember && !x.secret.is_empty() {
        // "Remember" turned off: keep it for this run only.
        let s = (std::mem::take(&mut x.secret), std::mem::take(&mut x.passphrase));
        live(|l| l.secrets.insert(id.clone(), s));
    }
    save(&app, &list)?;
    Ok(id)
}

#[tauri::command]
pub async fn remote_delete(app: AppHandle, id: String) -> Result<(), String> {
    disconnect(&id).await;
    live(|l| l.secrets.remove(&id));
    let mut list = load(&app);
    list.retain(|r| r.id != id);
    save(&app, &list)
}

#[tauri::command]
pub async fn remote_connect(app: AppHandle, id: String) -> Result<RemoteView, String> {
    connect(&app, &id).await
}

#[tauri::command]
pub async fn remote_disconnect(app: AppHandle, id: String) -> Vec<RemoteView> {
    disconnect(&id).await;
    remote_list(app)
}
