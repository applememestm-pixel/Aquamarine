//! Messages without the internet, the way Briar does it: when the relay
//! can't be reached, encrypted messages for a contact wait in a local
//! outbox, and the moment that contact's phone or PC is within Bluetooth
//! range the two devices connect directly and swap what they hold for each
//! other. When the relay is back, whatever is still waiting goes through it.
//!
//! * Only existing contacts: both sides prove they own the identity key
//!   their E2EE session already knows (Ed25519 signature over fresh
//!   nonces), so a stranger can't connect, read the queue or pretend to be
//!   someone. Everything carried is the same Double Ratchet envelope the
//!   relay would have carried — Bluetooth adds no new way to read it.
//! * A copy that comes twice (Bluetooth and later the relay) is harmless:
//!   ratchet decryption is all-or-nothing, the second copy is dropped.
//! * Transports: Linux — BlueZ RFCOMM profile (`org.bluez.Profile1`);
//!   Android — `AquaMesh.kt` (insecure RFCOMM, no pairing needed).
//!
//! Wire: frames of `u32` big-endian length + JSON ([`Frame`]); the side that
//! dialled speaks first at every step, so large batches never collide.

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use aqua_core::keys::IdentityPublic;
use aqua_core::wire::Envelope;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};

use crate::state::AppState;

/// RFCOMM service of Aquamarine (SDP record on both platforms).
pub const SERVICE_UUID: &str = "a9a1e5c0-7b3d-4f2e-9c61-5e2d8b0f4a17";
const MAGIC: &[u8] = b"aqua-mesh-v1";
const MAX_FRAME: usize = 8 << 20;
const MAX_ITEMS: usize = 3000;
const KEEP_SECS: u64 = 14 * 24 * 3600;

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

// ---------- the book: outbox, known peers, last results ----------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Item {
    id: String,
    /// Device address the envelope is for (`bob` or `bob~a1b2c3`).
    to: String,
    /// bincode(Envelope), base64.
    payload: String,
    at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Peer {
    /// Their Aquamarine device address.
    pub addr: String,
    pub name: String,
    pub last: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncReport {
    pub at: u64,
    pub device: String,
    pub peer: String,
    pub sent: usize,
    pub received: usize,
    pub error: Option<String>,
}

#[derive(Default, Serialize, Deserialize)]
struct Book {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    items: Vec<Item>,
    /// Bluetooth address → who answered there last time.
    #[serde(default)]
    peers: HashMap<String, Peer>,
    #[serde(default)]
    last: Vec<SyncReport>,
    #[serde(skip)]
    seen: VecDeque<String>,
    #[serde(skip)]
    path: Option<PathBuf>,
}

static BOOK: OnceLock<Mutex<Book>> = OnceLock::new();

fn book(app: &AppHandle) -> std::sync::MutexGuard<'static, Book> {
    let m = BOOK.get_or_init(|| {
        let path = app
            .try_state::<std::sync::Mutex<AppState>>()
            .and_then(|s| s.lock().ok().map(|s| s.data_dir.join("mesh.json")));
        let mut b: Book = path
            .as_ref()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|raw| serde_json::from_slice(&raw).ok())
            .unwrap_or_default();
        b.path = path;
        Mutex::new(b)
    });
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Book {
    fn save(&mut self) {
        let cut = now().saturating_sub(KEEP_SECS);
        self.items.retain(|i| i.at >= cut);
        if self.items.len() > MAX_ITEMS {
            let drop = self.items.len() - MAX_ITEMS;
            self.items.drain(..drop);
        }
        self.last.truncate(12);
        if let Some(p) = &self.path {
            if let Ok(raw) = serde_json::to_vec(self) {
                let tmp = p.with_extension("json.tmp");
                if std::fs::write(&tmp, raw).is_ok() {
                    let _ = std::fs::rename(&tmp, p);
                }
            }
        }
    }
    fn mark_seen(&mut self, id: &str) -> bool {
        if self.seen.iter().any(|x| x == id) {
            return false;
        }
        self.seen.push_back(id.to_string());
        if self.seen.len() > 4096 {
            self.seen.pop_front();
        }
        true
    }
}

fn item_id(payload: &[u8]) -> String {
    hex::encode(&Sha256::digest(payload)[..16])
}

/// Nearby delivery is on unless the user switched it off.
pub fn enabled(app: &AppHandle) -> bool {
    book(app).enabled.unwrap_or(true)
}

fn emit_status(app: &AppHandle) {
    let st = status_of(app);
    let _ = app.emit("aqua://mesh", st);
}

/// The relay couldn't take `env` for `to`: keep it for Bluetooth (and for
/// the relay once it's back).
pub fn queue(app: &AppHandle, to: &str, env: &Envelope) {
    let Ok(bytes) = bincode::serialize(env) else { return };
    let id = item_id(&bytes);
    {
        let mut b = book(app);
        if b.items.iter().any(|i| i.id == id) {
            return;
        }
        b.items.push(Item { id, to: to.to_string(), payload: B64.encode(bytes), at: now() });
        b.save();
    }
    emit_status(app);
    if enabled(app) {
        kick(app.clone());
    }
}

/// Connected to a relay again: hand it everything still waiting.
pub async fn flush_to_relay(app: &AppHandle) {
    let transport = {
        let st = app.state::<std::sync::Mutex<AppState>>();
        let Ok(s) = st.lock() else { return };
        s.transport.clone()
    };
    let Some(t) = transport else { return };
    let items: Vec<Item> = book(app).items.clone();
    if items.is_empty() {
        return;
    }
    let mut sent = Vec::new();
    for it in &items {
        let Ok(raw) = B64.decode(&it.payload) else { sent.push(it.id.clone()); continue };
        let Ok(env) = bincode::deserialize::<Envelope>(&raw) else { sent.push(it.id.clone()); continue };
        if t.send_envelope(&it.to, &env).is_ok() {
            sent.push(it.id.clone());
        } else {
            break;
        }
    }
    {
        let mut b = book(app);
        b.items.retain(|i| !sent.contains(&i.id));
        b.save();
    }
    tracing::info!(n = sent.len(), "mesh: queued messages handed to the relay");
    emit_status(app);
}

// ---------- status / commands -------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct MeshStatus {
    pub supported: bool,
    pub enabled: bool,
    /// Listening on at least one way (local network or Bluetooth).
    pub ready: bool,
    pub lan_ready: bool,
    /// This platform can do Bluetooth delivery.
    pub bt_supported: bool,
    pub bt_ready: bool,
    pub queued: usize,
    /// Queued messages per chat (account name).
    pub per_chat: HashMap<String, usize>,
    pub peers: Vec<Peer>,
    pub last: Vec<SyncReport>,
    pub syncing: bool,
    pub note: Option<String>,
}

static SYNCING: Mutex<bool> = Mutex::new(false);
static NOTE: Mutex<Option<String>> = Mutex::new(None);

fn status_of(app: &AppHandle) -> MeshStatus {
    let b = book(app);
    let mut per_chat: HashMap<String, usize> = HashMap::new();
    for i in &b.items {
        *per_chat.entry(crate::multidev::base_user(&i.to).to_string()).or_default() += 1;
    }
    let mut peers: Vec<Peer> = b.peers.values().cloned().collect();
    peers.sort_by(|a, b| b.last.cmp(&a.last));
    MeshStatus {
        supported: platform::SUPPORTED,
        enabled: b.enabled.unwrap_or(true),
        ready: platform::ready(),
        lan_ready: lan::ready(),
        bt_supported: bt::SUPPORTED,
        bt_ready: bt::ready(),
        queued: b.items.len(),
        per_chat,
        peers,
        last: b.last.clone(),
        syncing: *SYNCING.lock().unwrap_or_else(|e| e.into_inner()),
        note: NOTE.lock().unwrap_or_else(|e| e.into_inner()).clone(),
    }
}

#[tauri::command]
pub fn mesh_status(app: AppHandle) -> MeshStatus {
    status_of(&app)
}

#[tauri::command]
pub async fn mesh_set_enabled(app: AppHandle, on: bool) -> Result<MeshStatus, String> {
    {
        let mut b = book(&app);
        b.enabled = Some(on);
        b.save();
    }
    if on {
        start(app.clone()).await;
    } else {
        platform::stop(&app).await;
    }
    Ok(status_of(&app))
}

/// Look for contacts nearby right now and swap messages with them.
#[tauri::command]
pub async fn mesh_sync_now(app: AppHandle) -> Result<MeshStatus, String> {
    if !platform::SUPPORTED {
        return Err("Bluetooth delivery isn't available on this platform yet".into());
    }
    {
        let mut g = SYNCING.lock().unwrap_or_else(|e| e.into_inner());
        if *g {
            return Ok(status_of(&app));
        }
        *g = true;
    }
    emit_status(&app);
    let r = platform::sync_round(&app, true).await;
    *SYNCING.lock().unwrap_or_else(|e| e.into_inner()) = false;
    *NOTE.lock().unwrap_or_else(|e| e.into_inner()) = r.err();
    emit_status(&app);
    Ok(status_of(&app))
}

/// Start listening for nearby contacts (app start, or switched on).
pub async fn start(app: AppHandle) {
    if !enabled(&app) || !platform::SUPPORTED {
        return;
    }
    if let Err(e) = platform::listen(&app).await {
        tracing::info!(%e, "mesh: not listening");
        *NOTE.lock().unwrap_or_else(|e| e.into_inner()) = Some(e);
    }
    emit_status(&app);
    // Background rounds: while something waits and the relay is away, try
    // the devices we've met before every couple of minutes.
    static LOOP: OnceLock<()> = OnceLock::new();
    if LOOP.set(()).is_ok() {
        let app2 = app.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(120)).await;
                if !enabled(&app2) || book(&app2).items.is_empty() || relay_up(&app2) {
                    continue;
                }
                let busy = { let mut g = SYNCING.lock().unwrap_or_else(|e| e.into_inner()); let b = *g; *g = true; b };
                if busy {
                    continue;
                }
                let _ = platform::sync_round(&app2, false).await;
                *SYNCING.lock().unwrap_or_else(|e| e.into_inner()) = false;
                emit_status(&app2);
            }
        });
    }
}

static RELAY_LOST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// The relay connection dropped (until the next successful connect).
pub fn relay_lost() {
    RELAY_LOST.store(true, std::sync::atomic::Ordering::Relaxed);
}
pub fn relay_back() {
    RELAY_LOST.store(false, std::sync::atomic::Ordering::Relaxed);
}
fn relay_up(app: &AppHandle) -> bool {
    !RELAY_LOST.load(std::sync::atomic::Ordering::Relaxed)
        && app.state::<std::sync::Mutex<AppState>>().lock().map(|s| s.transport.is_some()).unwrap_or(false)
}

/// A message was just queued: try known peers soon (debounced).
fn kick(app: AppHandle) {
    static PENDING: Mutex<bool> = Mutex::new(false);
    {
        let mut p = PENDING.lock().unwrap_or_else(|e| e.into_inner());
        if *p {
            return;
        }
        *p = true;
    }
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = false;
        let busy = { let mut g = SYNCING.lock().unwrap_or_else(|e| e.into_inner()); let b = *g; *g = true; b };
        if busy {
            return;
        }
        let _ = platform::sync_round(&app, false).await;
        *SYNCING.lock().unwrap_or_else(|e| e.into_inner()) = false;
        emit_status(&app);
    });
}

// ---------- the session protocol ---------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
enum Frame {
    Hello { v: u32, addrs: Vec<String>, nonce: String, sign_pub: String, dh_pub: String },
    Auth { addr: String, sig: String },
    Batch { items: Vec<WireItem> },
    Ack { ids: Vec<String> },
    Bye { reason: String },
}

#[derive(Debug, Serialize, Deserialize)]
struct WireItem {
    id: String,
    to: String,
    payload: String,
}

fn send_frame(w: &mut impl Write, f: &Frame) -> Result<(), String> {
    let raw = serde_json::to_vec(f).map_err(|e| e.to_string())?;
    w.write_all(&(raw.len() as u32).to_be_bytes()).map_err(|e| format!("write: {e}"))?;
    w.write_all(&raw).map_err(|e| format!("write: {e}"))?;
    w.flush().map_err(|e| format!("write: {e}"))
}

fn read_frame(r: &mut impl Read) -> Result<Frame, String> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len).map_err(|e| format!("read: {e}"))?;
    let n = u32::from_be_bytes(len) as usize;
    if n > MAX_FRAME {
        return Err("frame too large".into());
    }
    let mut buf = vec![0u8; n];
    r.read_exact(&mut buf).map_err(|e| format!("read: {e}"))?;
    let f: Frame = serde_json::from_slice(&buf).map_err(|e| format!("bad frame: {e}"))?;
    if let Frame::Bye { reason } = &f {
        return Err(format!("the other side ended: {reason}"));
    }
    Ok(f)
}

fn transcript(a_nonce: &[u8], b_nonce: &[u8], addr: &str) -> Vec<u8> {
    let mut m = Vec::with_capacity(MAGIC.len() + 80 + addr.len());
    m.extend_from_slice(MAGIC);
    m.extend_from_slice(a_nonce);
    m.extend_from_slice(b_nonce);
    m.extend_from_slice(addr.as_bytes());
    m
}

struct Me {
    addrs: Vec<String>,
    identity: aqua_core::keys::IdentityKeyPair,
    /// addr → identity of every device we have a session with.
    contacts: HashMap<String, IdentityPublic>,
}

fn me(app: &AppHandle) -> Result<Me, String> {
    let st = app.state::<std::sync::Mutex<AppState>>();
    let s = st.lock().map_err(|e| e.to_string())?;
    let user = s.username.clone().ok_or("not signed in")?;
    let dev = crate::multidev::device_id(s.identity.public().dh_pub.as_bytes());
    Ok(Me {
        addrs: vec![format!("{user}~{dev}"), user],
        identity: s.identity.clone(),
        contacts: s.sessions.iter().map(|(a, sess)| (a.clone(), sess.peer)).collect(),
    })
}

/// One exchange with a nearby device over any byte stream. `dialled`: we
/// opened the connection (and so speak first). Blocking — run it on a
/// blocking thread.
pub fn run_session<S: Read + Write>(app: &AppHandle, link: &mut S, dialled: bool, device: &str) -> SyncReport {
    let mut rep = SyncReport { at: now(), device: device.to_string(), ..Default::default() };
    match exchange(&AppCtx(app), link, dialled, &mut rep) {
        Ok(()) => {}
        Err(e) => {
            tracing::info!(%device, %e, "mesh session ended");
            let _ = send_frame(link, &Frame::Bye { reason: "error".into() });
            rep.error = Some(e);
        }
    }
    {
        let mut b = book(app);
        if !rep.peer.is_empty() {
            let name = crate::multidev::base_user(&rep.peer).to_string();
            b.peers.insert(device.to_string(), Peer { addr: rep.peer.clone(), name, last: rep.at });
        }
        if !rep.peer.is_empty() || dialled {
            b.last.insert(0, rep.clone());
        }
        b.save();
    }
    emit_status(app);
    rep
}

/// What a session needs from the app (a trait so tests can run two ends).
trait Ctx {
    fn me(&self) -> Result<Me, String>;
    /// Queued items for this device address.
    fn outbox_for(&self, addr: &str) -> Vec<WireItem>;
    /// The other side has these now.
    fn acked(&self, ids: &[String]);
    /// First time this item arrives here?
    fn first_time(&self, id: &str) -> bool;
    /// Hand an envelope to the normal receive path.
    fn deliver(&self, from: String, envelope: Envelope);
}

struct AppCtx<'a>(&'a AppHandle);

impl Ctx for AppCtx<'_> {
    fn me(&self) -> Result<Me, String> {
        me(self.0)
    }
    fn outbox_for(&self, addr: &str) -> Vec<WireItem> {
        book(self.0).items.iter().filter(|i| i.to == addr)
            .map(|i| WireItem { id: i.id.clone(), to: i.to.clone(), payload: i.payload.clone() }).collect()
    }
    fn acked(&self, ids: &[String]) {
        let mut b = book(self.0);
        b.items.retain(|i| !ids.contains(&i.id));
        b.save();
    }
    fn first_time(&self, id: &str) -> bool {
        book(self.0).mark_seen(id)
    }
    fn deliver(&self, from: String, envelope: Envelope) {
        let app = self.0.clone();
        tauri::async_runtime::block_on(async move {
            crate::commands::handle_incoming(&app, aqua_client_core::Incoming::Envelope { from, envelope, ts: None }).await;
        });
    }
}

fn exchange<S: Read + Write, C: Ctx>(ctx: &C, link: &mut S, dialled: bool, rep: &mut SyncReport) -> Result<(), String> {
    let me = ctx.me()?;
    let my_nonce: [u8; 32] = rand::random();
    let pubk = me.identity.public();
    let hello = Frame::Hello {
        v: 1,
        addrs: me.addrs.clone(),
        nonce: B64.encode(my_nonce),
        sign_pub: B64.encode(pubk.sign_pub.as_bytes()),
        dh_pub: B64.encode(pubk.dh_pub.as_bytes()),
    };
    // 1. Hello both ways. The dialled side checks the caller is a contact
    // before it says who it is, so a stranger learns nothing.
    let theirs = if dialled {
        send_frame(link, &hello)?;
        read_frame(link)?
    } else {
        read_frame(link)?
    };
    let Frame::Hello { v, addrs, nonce, sign_pub, dh_pub } = theirs else { return Err("expected hello".into()) };
    if v != 1 {
        return Err(format!("protocol v{v}"));
    }
    let their_nonce = B64.decode(nonce).map_err(|_| "bad nonce")?;
    let sp: [u8; 32] = B64.decode(sign_pub).ok().and_then(|b| b.try_into().ok()).ok_or("bad key")?;
    let dp: [u8; 32] = B64.decode(dh_pub).ok().and_then(|b| b.try_into().ok()).ok_or("bad key")?;
    // Which of their addresses do we know, with exactly this identity?
    let peer_addr = addrs
        .iter()
        .take(4)
        .find(|a| me.contacts.get(*a).map(|id| id.sign_pub.as_bytes() == &sp && id.dh_pub.as_bytes() == &dp).unwrap_or(false))
        .cloned();
    let Some(peer_addr) = peer_addr else {
        let _ = send_frame(link, &Frame::Bye { reason: "not a contact".into() });
        return Err("not one of your contacts".into());
    };
    let their_id = me.contacts[&peer_addr];
    if !dialled {
        send_frame(link, &hello)?;
    }
    // My address as they know it: the first one of mine, the device address.
    let my_addr = me.addrs[0].clone();

    // 2. Prove the keys: sign their nonce + mine + my address.
    let my_auth = Frame::Auth { addr: my_addr.clone(), sig: B64.encode(me.identity.sign.sign(&transcript(&their_nonce, &my_nonce, &my_addr))) };
    let check = |f: Frame| -> Result<(), String> {
        let Frame::Auth { addr, sig } = f else { return Err("expected auth".into()) };
        if addr != peer_addr && !addrs.contains(&addr) {
            return Err("auth for another address".into());
        }
        let sig: [u8; 64] = B64.decode(sig).ok().and_then(|b| b.try_into().ok()).ok_or("bad signature")?;
        their_id.sign_pub.verify(&transcript(&my_nonce, &their_nonce, &addr), &sig).map_err(|_| "signature doesn't match".to_string())
    };
    if dialled {
        send_frame(link, &my_auth)?;
        check(read_frame(link)?)?;
    } else {
        check(read_frame(link)?)?;
        send_frame(link, &my_auth)?;
    }
    rep.peer = peer_addr.clone();

    // 3. Batches: what I hold for any of their addresses, then theirs for me.
    let give = |link: &mut S| -> Result<usize, String> {
        let mine = ctx.outbox_for(&peer_addr);
        let n = mine.len();
        send_frame(link, &Frame::Batch { items: mine })?;
        let Frame::Ack { ids } = read_frame(link)? else { return Err("expected ack".into()) };
        ctx.acked(&ids);
        Ok(n.min(ids.len()))
    };
    let take = |link: &mut S| -> Result<usize, String> {
        let Frame::Batch { items } = read_frame(link)? else { return Err("expected batch".into()) };
        let mut ids = Vec::new();
        let mut got = 0;
        for it in items.into_iter().take(MAX_ITEMS) {
            if !me.addrs.contains(&it.to) {
                continue;
            }
            ids.push(it.id.clone());
            if !ctx.first_time(&it.id) {
                continue;
            }
            let Ok(raw) = B64.decode(&it.payload) else { continue };
            if item_id(&raw) != it.id {
                continue;
            }
            let Ok(envelope) = bincode::deserialize::<Envelope>(&raw) else { continue };
            // Same path as a relay delivery; `from` is the device that just
            // proved its key, never what the item claims.
            ctx.deliver(peer_addr.clone(), envelope);
            got += 1;
        }
        send_frame(link, &Frame::Ack { ids })?;
        Ok(got)
    };
    if dialled {
        rep.sent = give(link)?;
        rep.received = take(link)?;
    } else {
        rep.received = take(link)?;
        rep.sent = give(link)?;
    }
    let _ = send_frame(link, &Frame::Bye { reason: "done".into() });
    tracing::info!(peer = %rep.peer, sent = rep.sent, received = rep.received, "mesh: exchanged");
    Ok(())
}

// ---------- transports ---------------------------------------------------------

#[path = "mesh_lan.rs"]
mod lan;

#[cfg(target_os = "linux")]
#[path = "mesh_linux.rs"]
mod bt;

#[cfg(target_os = "android")]
#[path = "mesh_android.rs"]
mod bt;

#[cfg(any(windows, aqua_wincheck))]
#[path = "mesh_windows.rs"]
mod bt;

#[cfg(target_os = "android")]
pub use bt::plugin as android_plugin;

#[cfg(not(any(target_os = "linux", target_os = "android", windows, aqua_wincheck)))]
mod bt {
    use tauri::AppHandle;
    pub const SUPPORTED: bool = false;
    pub fn ready() -> bool { false }
    pub async fn listen(_app: &AppHandle) -> Result<(), String> { Err("Bluetooth delivery isn't available on this platform yet".into()) }
    pub async fn stop(_app: &AppHandle) {}
    pub async fn sync_round(_app: &AppHandle, _scan: bool) -> Result<(), String> { Err("Bluetooth delivery isn't available on this platform yet".into()) }
}

/// Both transports together: the local network everywhere, Bluetooth where
/// the platform has it.
mod platform {
    use super::{bt, lan};
    use tauri::AppHandle;

    pub const SUPPORTED: bool = true;

    pub fn ready() -> bool {
        lan::ready() || bt::ready()
    }

    pub async fn listen(app: &AppHandle) -> Result<(), String> {
        let l = lan::listen(app).await;
        let b = if bt::SUPPORTED { bt::listen(app).await } else { Ok(()) };
        match (l, b) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(e), Err(f)) => Err(format!("{e}; {f}")),
            // One way works: note what the other said.
            (Ok(()), Err(e)) | (Err(e), Ok(())) => Err(e),
        }
    }

    pub async fn stop(app: &AppHandle) {
        lan::stop();
        bt::stop(app).await;
    }

    pub async fn sync_round(app: &AppHandle, scan: bool) -> Result<(), String> {
        let l = lan::sync_round(app, scan).await;
        let b = if bt::SUPPORTED { bt::sync_round(app, scan).await } else { Err(String::new()) };
        match (l, b) {
            (Ok(n), _) if n > 0 => Ok(()),
            (_, Ok(())) => Ok(()),
            _ if !scan => Ok(()),
            _ => Err("no contacts with Aquamarine nearby — same Wi-Fi, or Bluetooth on".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_roundtrip() {
        let mut buf = Vec::new();
        send_frame(&mut buf, &Frame::Ack { ids: vec!["a".into(), "b".into()] }).unwrap();
        let mut r = std::io::Cursor::new(buf);
        match read_frame(&mut r).unwrap() {
            Frame::Ack { ids } => assert_eq!(ids, vec!["a", "b"]),
            _ => panic!(),
        }
        let mut bye = Vec::new();
        send_frame(&mut bye, &Frame::Bye { reason: "x".into() }).unwrap();
        assert!(read_frame(&mut std::io::Cursor::new(bye)).is_err());
        let mut big = (MAX_FRAME as u32 + 1).to_be_bytes().to_vec();
        big.extend_from_slice(b"{}");
        assert!(read_frame(&mut std::io::Cursor::new(big)).is_err());
    }

    use aqua_core::keys::{IdentityKeyPair, OneTimePreKey, PreKeyBundle, SignedPreKey};
    use aqua_core::session::Session;
    use std::cell::RefCell;

    /// One end of a test exchange: its identity, sessions and outbox.
    struct T {
        user: &'static str,
        id: IdentityKeyPair,
        sessions: RefCell<HashMap<String, Session>>,
        outbox: RefCell<Vec<WireItem>>,
        seen: RefCell<Vec<String>>,
        got: RefCell<Vec<(String, Vec<u8>)>>,
    }
    unsafe impl Sync for T {}
    impl T {
        fn addr(&self) -> String {
            format!("{}~{}", self.user, crate::multidev::device_id(self.id.public().dh_pub.as_bytes()))
        }
        fn queue(&self, to: &str, text: &str) {
            let env = self.sessions.borrow_mut().get_mut(to).unwrap().encrypt(text.as_bytes()).unwrap();
            let raw = bincode::serialize(&env).unwrap();
            self.outbox.borrow_mut().push(WireItem { id: item_id(&raw), to: to.into(), payload: B64.encode(raw) });
        }
    }
    impl Ctx for T {
        fn me(&self) -> Result<Me, String> {
            Ok(Me {
                addrs: vec![self.addr(), self.user.to_string()],
                identity: self.id.clone(),
                contacts: self.sessions.borrow().iter().map(|(a, s)| (a.clone(), s.peer)).collect(),
            })
        }
        fn outbox_for(&self, addr: &str) -> Vec<WireItem> {
            self.outbox.borrow().iter().filter(|i| i.to == addr).map(|i| WireItem { id: i.id.clone(), to: i.to.clone(), payload: i.payload.clone() }).collect()
        }
        fn acked(&self, ids: &[String]) {
            self.outbox.borrow_mut().retain(|i| !ids.contains(&i.id));
        }
        fn first_time(&self, id: &str) -> bool {
            if self.seen.borrow().iter().any(|x| x == id) { return false; }
            self.seen.borrow_mut().push(id.into());
            true
        }
        fn deliver(&self, from: String, envelope: Envelope) {
            let plain = self.sessions.borrow_mut().get_mut(&from).unwrap().decrypt(&envelope).unwrap();
            self.got.borrow_mut().push((from, plain));
        }
    }

    fn pair() -> (T, T) {
        let mk = |user| T { user, id: IdentityKeyPair::generate(), sessions: Default::default(), outbox: Default::default(), seen: Default::default(), got: Default::default() };
        let (a, b) = (mk("alice"), mk("bob"));
        let spk = SignedPreKey::generate(1, &b.id);
        let opk = OneTimePreKey::generate(1);
        let bundle = PreKeyBundle { identity: b.id.public(), signed_prekey: spk.public(), one_time_prekey: Some(opk.public()) };
        let (sa, init) = Session::start_as_initiator(&a.id, &bundle, b"hi").unwrap();
        let (sb, _) = Session::accept_initial(&b.id, spk, Some(opk), &init).unwrap();
        // Alice knows Bob by his device address, Bob knows Alice as plain "alice".
        a.sessions.borrow_mut().insert(b.addr(), sa);
        b.sessions.borrow_mut().insert("alice".into(), sb);
        (a, b)
    }

    fn run(a: &T, b: &T) -> (Result<SyncReport, String>, Result<SyncReport, String>) {
        let (mut la, mut lb) = std::os::unix::net::UnixStream::pair().unwrap();
        std::thread::scope(|sc| {
            let hb = sc.spawn(|| { let mut r = SyncReport::default(); exchange(b, &mut lb, false, &mut r).map(|_| r) });
            let mut r = SyncReport::default();
            let ra = exchange(a, &mut la, true, &mut r).map(|_| r);
            (ra, hb.join().unwrap())
        })
    }

    #[test]
    fn contacts_swap_their_queues() {
        let (a, b) = pair();
        a.queue(&b.addr(), "one");
        a.queue(&b.addr(), "two");
        b.queue("alice", "hey");
        let (ra, rb) = run(&a, &b);
        let (ra, rb) = (ra.unwrap(), rb.unwrap());
        assert_eq!((ra.sent, ra.received), (2, 1));
        assert_eq!((rb.sent, rb.received), (1, 2));
        assert_eq!(ra.peer, b.addr());
        assert_eq!(rb.peer, "alice");
        assert_eq!(b.got.borrow().iter().map(|(f, p)| (f.as_str(), p.as_slice())).collect::<Vec<_>>(), vec![("alice", &b"one"[..]), ("alice", &b"two"[..])]);
        assert_eq!(a.got.borrow()[0].1, b"hey");
        assert!(a.outbox.borrow().is_empty() && b.outbox.borrow().is_empty());
        // A second meeting has nothing new to swap.
        let (ra, _) = run(&a, &b);
        assert_eq!(ra.unwrap().sent, 0);
    }

    #[test]
    fn a_stranger_is_turned_away() {
        let (a, _b) = pair();
        let (eve, _) = pair(); // has a session with someone else, not with Alice
        a.queue(&_b.addr(), "secret");
        let (ra, re) = run(&eve, &a);
        assert!(ra.is_err() && re.is_err());
        assert_eq!(a.outbox.borrow().len(), 1, "nothing leaves for a stranger");
    }

    #[test]
    fn a_copied_key_without_the_secret_fails_auth() {
        // Mallory claims Bob's public identity but can't sign with it.
        let (a, b) = pair();
        a.queue(&b.addr(), "for bob");
        let mallory = T { user: "bob", id: IdentityKeyPair::generate(), sessions: Default::default(), outbox: Default::default(), seen: Default::default(), got: Default::default() };
        struct Liar<'a>(&'a T, &'a T);
        impl Ctx for Liar<'_> {
            fn me(&self) -> Result<Me, String> {
                let mut m = self.0.me()?;
                m.addrs = vec![self.1.addr()];
                Ok(m)
            }
            fn outbox_for(&self, a: &str) -> Vec<WireItem> { self.0.outbox_for(a) }
            fn acked(&self, ids: &[String]) { self.0.acked(ids) }
            fn first_time(&self, id: &str) -> bool { self.0.first_time(id) }
            fn deliver(&self, f: String, e: Envelope) { self.0.deliver(f, e) }
        }
        // Mallory's hello carries her own key → Alice doesn't know it.
        let liar = Liar(&mallory, &b);
        let (mut la, mut lm) = std::os::unix::net::UnixStream::pair().unwrap();
        let (r1, r2) = std::thread::scope(|sc| {
            let h = sc.spawn(|| { let mut r = SyncReport::default(); exchange(&liar, &mut lm, true, &mut r) });
            let mut r = SyncReport::default();
            (exchange(&a, &mut la, false, &mut r), h.join().unwrap())
        });
        assert!(r1.is_err() && r2.is_err());
        assert_eq!(a.outbox.borrow().len(), 1);
    }

    #[test]
    fn transcript_binds_order_and_address() {
        let a = transcript(b"n1", b"n2", "bob");
        assert_ne!(a, transcript(b"n2", b"n1", "bob"));
        assert_ne!(a, transcript(b"n1", b"n2", "eve"));
    }
}
