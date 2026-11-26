//! Federation: several relays act as one network.
//!
//! Peers share a secret (`AQUA_FED_SECRET` / admin panel) and list each
//! other (`AQUA_FED_PEERS`). Every relay-to-relay request is signed:
//!
//! ```text
//! x-aqua-fed: <unix ms>.<hex HMAC-SHA256(secret, METHOD \n PATH \n ms \n SHA256(body))>
//! ```
//!
//! Since 1.21.0-23 (AQUA-SEC-009) each relay also has its own Ed25519 node
//! key. Relays learn each other's keys from signed hellos and keep them
//! (`FedConfig::pins`); a request to a relay whose key we know carries
//!
//! ```text
//! x-aqua-fed2: <unix ms>.<nonce hex>.<sender id>.<recipient id>.<hex Ed25519 sig>
//!   over "aqua-fed-v2\nMETHOD\nPATH\nms\nnonce\nsender\nrecipient\nSHA256(body)"
//! ```
//!
//! — bound to sender and recipient, taken once. Once every peer's key is
//! known ("strict"), a request signed only with the shared secret is
//! refused except for hello / ping / the name registry: knowing the secret
//! no longer lets anyone speak as a relay of the network.
//!
//! What they do together:
//! * accounts, public profiles, profile media and encrypted backups
//!   replicate between all relays: a relay that changes something pokes
//!   its peers (`POST /v1/fed/poke`, within ~2 s) and they pull "everything
//!   since T" right away (plus a pull every 30 s as a safety net; newest
//!   row wins), so any relay can sign a user in and show their profile;
//! * an envelope for someone connected elsewhere is forwarded there; if
//!   nobody has them online it waits where it was sent, and the relay they
//!   connect to next collects it from its peers;
//! * prekey bundles, encrypted files and profile media are fetched from a
//!   peer when the local relay doesn't have them;
//! * `GET /v1/fed/peers` lists the public addresses, so clients can switch
//!   to a live relay when theirs goes down.

use std::sync::Arc;
use std::time::Duration;

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{AppState, RelayMessage};

const FILE: &str = "aqua-federation.json";
/// This relay's node key when the config file doesn't hold it.
const NODE_KEY_FILE: &str = "aqua-fed-node.key";

/// Node ids whose private key became public (REV2-001: a test run wrote one
/// into the source tree and it was committed in 1.21.0-23's branch). Never
/// accepted from anyone; a relay that has one makes itself a new key.
pub const LEAKED_NODE_IDS: &[&str] = &["1271400a10f36e27afb42bf3277bc1b1"];

/// Write a file only its owner can read (the node key, the federation
/// config with its secret).
pub(crate) fn write_private(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
        // `mode` applies only to a new file: an older one (0644) is fixed too.
        f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        f.write_all(text.as_bytes())
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, text)
    }
}
const SKEW_MS: i64 = 5 * 60 * 1000;
/// Signatures already taken (replay.rs).
const SEEN_FILE: &str = "aqua-fed-seen.log";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FedConfig {
    #[serde(default)]
    pub secret: String,
    #[serde(default)]
    pub peers: Vec<String>,
    /// How clients reach THIS relay (shown to them as a fallback).
    #[serde(default)]
    pub public_url: String,
    /// "solo" — this relay works alone; "network" — with the others; ""
    /// — not chosen yet (the admin panel asks on first open; a config with
    /// a secret and peers from before counts as "network").
    #[serde(default)]
    pub mode: String,
    /// Solo relays that share the name registry with us (they said hello
    /// at `/v1/fed/names/hello`): only their (uid, @name) list is pulled.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub solo_peers: Vec<String>,
    /// A lasting id of this relay: how a relay without a reachable address
    /// is known to its peers (`tunnel:<node>`, see fedtunnel.rs).
    #[serde(default)]
    pub node: String,
    /// Kept in memory only (tests): peers learnt later aren't written to disk.
    #[serde(skip)]
    pub ephemeral: bool,
    /// This relay's Ed25519 node key (secret, hex). Kept ONLY in its own
    /// file (`NODE_KEY_FILE`, 0600), never written into this config
    /// (REV2-001); read here once to move keys of an older build out.
    #[serde(default, skip_serializing)]
    pub node_key: String,
    /// The folder the config and key files are in ("" = the current one).
    #[serde(skip)]
    pub dir: std::path::PathBuf,
    /// Keys of peer relays, learnt from their signed hellos.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pins: Vec<Pin>,
    /// Node ids the admin revoked: never accepted again.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub revoked: Vec<String>,
    /// After a key change: the new public key signed by the old one, sent
    /// with hellos so peers move their pin.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub rotated: String,
    /// One-time admission tokens this relay handed out in invitations
    /// (REV2-004): SHA-256 hex and expiry (unix seconds).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub admit_tokens: Vec<(String, u64)>,
    /// Relays that said hello without being admitted, while every peer's key
    /// is known: they wait for the admin (`/v1/admin/fed/admit`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_nodes: Vec<Pin>,
    /// The admission token of the invitation this relay joined with, sent
    /// with hellos until a relay of the network vouches for us.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub join_token: String,
    /// Statements by relays of the network that they admitted us (shown to
    /// the others, so they admit us too).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vouches: Vec<Vouch>,
    /// Node keys required (owner, 26.11): "" (a config from before — set to
    /// "transition" at the first start), "transition" (requests signed only
    /// with the shared secret are still taken from relays without a pinned
    /// key) or "enforced" (never again, see [`Fed::enforced`]). Only ever
    /// moves forward: transition → enforced when every peer's key is known,
    /// at `auth_deadline`, or by the admin.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub auth_mode: String,
    /// Unix seconds: the end of the transition.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub auth_deadline: u64,
    /// Unix seconds: when keys became required here.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub enforced_at: u64,
    /// Unix seconds: the admin let relays without keys in again until then
    /// (`/v1/admin/fed/auth`, at most [`LEGACY_MAX_DAYS`]); ends by itself.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub legacy_until: u64,
    /// This relay's last key changes, each signed by the key before it
    /// (newest last, at most [`MAX_ROTATIONS`]): a peer that was off for
    /// two changes follows the chain.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rotations: Vec<RotStep>,
    /// Node ids of peers we followed away from (their keys changed): never
    /// taken again, like revoked ones.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub retired: Vec<String>,
    /// Keys of those retired ids and when we followed away from them
    /// (`[id, key hex, unix secs]`): deletions a relay signed with its key
    /// before it changed are still taken — none signed after that.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub retired_keys: Vec<(String, String, u64)>,
}

/// One key change: `old` (public key, hex) signed
/// `aqua-fed-rotate-v2\n{url}\n{old_id}\n{new}\n{seq}`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RotStep {
    pub old: String,
    pub new: String,
    pub seq: u64,
    pub sig: String,
}

/// Key changes a relay keeps and sends in its hellos.
pub const MAX_ROTATIONS: usize = 8;

fn rot_message(url: &str, old_id: &str, new_pub: &str, seq: u64) -> String {
    format!("aqua-fed-rotate-v2\n{url}\n{old_id}\n{new_pub}\n{seq}")
}

fn is_zero(v: &u64) -> bool { *v == 0 }

/// How long a network keeps taking relays without keys after this version
/// first starts (the owner named no term: our choice, told to the owner).
pub const AUTH_TRANSITION_DAYS: u64 = 14;
/// The panel and the log warn this long before the transition ends.
pub const AUTH_WARN_DAYS: u64 = 3;
/// Longest window the admin can open for relays without keys.
pub const LEGACY_MAX_DAYS: u64 = 7;
const DAY: u64 = 24 * 3600;

/// `AQUA_FED_AUTH=enforced`: keys required whatever the config says (it can
/// only make it stricter; there is no value that makes it laxer).
fn env_enforced() -> bool {
    std::env::var("AQUA_FED_AUTH").is_ok_and(|v| v.trim().eq_ignore_ascii_case("enforced"))
}

/// "Relay `by` admitted node `id` at `url`": Ed25519 by `by`'s node key over
/// `aqua-fed-admit\n{id}\n{url}`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Vouch {
    pub by: String,
    pub sig: String,
}

/// How long an admission token in an invitation is good for.
pub const ADMIT_TTL_SECS: u64 = 7 * 24 * 3600;
/// Most relays waiting for admission.
const MAX_PENDING_NODES: usize = 64;

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// A peer relay's node key as we keep it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pin {
    /// How we reach it (its address or `tunnel:<node>`).
    pub url: String,
    /// Its node id (`node_id_of(key)`).
    pub id: String,
    /// Its Ed25519 public key, hex.
    pub key: String,
    /// "network" (a relay of the network; "" in configs from before) or
    /// "names" (a solo relay sharing only the name registry: it may only
    /// read and announce names).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub role: String,
    /// Unix seconds: when it was pinned.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub since: u64,
    /// The last key change of it we followed (`rotations`, 1.21.0-25).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rot_seq: u64,
    /// Waiting for the admin: it came with another key for an address we
    /// keep a key for, unsigned by that key (`pending_nodes` only).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub conflict: bool,
}

impl Pin {
    pub fn names_only(&self) -> bool {
        self.role == "names"
    }
    /// The same relay under the same key (when and how it was pinned aside).
    fn same(&self, o: &Pin) -> bool {
        self.url == o.url && self.id == o.id && self.key == o.key && self.names_only() == o.names_only()
    }
}

fn valid_key_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Is this secret key (hex) one of `leaked` (node ids)?
fn key_is_leaked_in(hex_key: &str, leaked: &[&str]) -> bool {
    hex::decode(hex_key).ok().and_then(|b| <[u8; 32]>::try_from(b).ok())
        .is_some_and(|b| leaked.contains(&node_id_of(aqua_core::crypto::SignSecret::from_bytes(b).public().as_bytes()).as_str()))
}

/// A node id: the first 16 bytes of SHA-256 of the public key, hex.
pub fn node_id_of(pubkey: &[u8; 32]) -> String {
    hex::encode(&Sha256::digest(pubkey)[..16])
}

impl FedConfig {
    pub fn load() -> Self {
        Self::load_in(std::path::Path::new("."))
    }

    /// [`Self::load`] from a given folder (the data folder; tests).
    pub fn load_in(dir: &std::path::Path) -> Self {
        Self::load_in_with(dir, LEAKED_NODE_IDS)
    }

    /// [`Self::load_in`] with a given list of published node ids (tests).
    pub fn load_in_with(dir: &std::path::Path, leaked: &[&str]) -> Self {
        let file = dir.join(FILE);
        let key_file = dir.join(NODE_KEY_FILE);
        let mut c: FedConfig = std::fs::read(&file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        c.dir = dir.to_path_buf();
        if let Ok(s) = std::env::var("AQUA_FED_SECRET") {
            if !s.trim().is_empty() { c.secret = s.trim().to_string(); }
        }
        if let Ok(p) = std::env::var("AQUA_FED_PEERS") {
            let list: Vec<String> = p.split([',', ' ']).map(|x| x.trim().trim_end_matches('/').to_string()).filter(|x| !x.is_empty()).collect();
            if !list.is_empty() { c.peers = list; }
        }
        if let Ok(u) = std::env::var("AQUA_PUBLIC_URL") {
            if !u.trim().is_empty() { c.public_url = u.trim().trim_end_matches('/').to_string(); }
        }
        c.peers.retain(|p| !p.is_empty() && p.trim_end_matches('/') != c.public_url);
        // The node key lives in its own file and must outlive restarts (also
        // when the network is set by environment variables: a new key at
        // every start would look like an impostor to peers that pinned the
        // old one). Only for a relay that is in a network or name registry:
        // an unconfigured relay (tests, a fresh install) writes no key.
        let from_json = std::mem::take(&mut c.node_key);
        if c.active() || c.registry() {
            let from_file = std::fs::read_to_string(&key_file).ok().map(|s| s.trim().to_string()).filter(|s| valid_key_hex(s));
            let mut key = from_file.clone().or_else(|| Some(from_json.clone()).filter(|s| valid_key_hex(s)));
            if key.as_deref().is_some_and(|k| key_is_leaked_in(k, leaked)) {
                tracing::error!("federation: this relay's node key is a published one — making a new key (peers re-admit it)");
                key = None;
            }
            let key = key.unwrap_or_else(|| hex::encode(aqua_core::crypto::SignSecret::generate().to_bytes()));
            if from_file.as_deref() != Some(key.as_str()) {
                if let Err(e) = write_private(&key_file, &key) {
                    tracing::error!(error = %e, "federation: the node key can't be saved — it will change at the next start");
                }
            }
            c.node_key = key;
        }
        // An older build kept the key in this file: drop it from there.
        if !from_json.is_empty() && file.exists() {
            let _ = c.save();
        }
        if c.node.is_empty() {
            let mut b = [0u8; 8];
            rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut b);
            c.node = hex::encode(b);
            if file.exists() {
                let _ = c.save();
            }
        }
        c
    }
    pub fn save(&self) -> std::io::Result<()> {
        // The node key (after a rotation) is kept where `load` reads it.
        if !self.node_key.is_empty() {
            write_private(&self.dir.join(NODE_KEY_FILE), &self.node_key)?;
        }
        // The secret is in here: owner-only, like the key.
        write_private(&self.dir.join(FILE), &String::from_utf8_lossy(&serde_json::to_vec_pretty(self).unwrap_or_default()))
    }
    /// In a network: a secret is set (peers may still be on their way —
    /// they say hello and get added by themselves).
    pub fn active(&self) -> bool {
        self.mode != "solo" && self.secret.len() >= 16 && (self.mode == "network" || !self.peers.is_empty())
    }
    /// A solo relay that shares only the name registry with a network
    /// (joined with an invitation, but stays solo).
    pub fn registry(&self) -> bool {
        self.mode == "solo" && self.secret.len() >= 16 && !self.peers.is_empty()
    }
    /// Every peer's key is known (and at least one relay of the network is
    /// keyed, or one was revoked).
    pub fn all_keys_known(&self) -> bool {
        (self.pins.iter().any(|p| !p.names_only()) || !self.revoked.is_empty())
            && self.peers.iter().all(|u| self.pins.iter().any(|p| p.url == *u && !p.names_only()))
    }

    /// Move the key requirement forward when it's time (never back). True
    /// when something changed (to be saved).
    pub fn settle(&mut self, now: u64) -> bool {
        if !(self.active() || self.registry()) {
            return false;
        }
        let before = (self.auth_mode.clone(), self.auth_deadline, self.enforced_at, self.legacy_until);
        if self.auth_mode.is_empty() {
            self.auth_mode = "transition".into();
            self.auth_deadline = now + AUTH_TRANSITION_DAYS * DAY;
        }
        if self.auth_mode != "enforced" && (env_enforced() || self.all_keys_known() || now >= self.auth_deadline) {
            if self.auth_mode != "transition" {
                tracing::error!(mode = %self.auth_mode, "federation: unknown key requirement in the config — keys are required now");
            }
            self.auth_mode = "enforced".into();
            self.enforced_at = now;
            tracing::warn!("federation: node keys are required from now on (requests signed only with the shared secret are refused)");
        }
        if self.legacy_until != 0 && (now >= self.legacy_until || env_enforced()) {
            self.legacy_until = 0;
            tracing::warn!("federation: the window for relays without keys is over");
        }
        before != (self.auth_mode.clone(), self.auth_deadline, self.enforced_at, self.legacy_until)
    }

    /// "solo" / "network" / "" — what the panel shows.
    pub fn effective_mode(&self) -> &str {
        if self.mode.is_empty() && self.secret.len() >= 16 && !self.peers.is_empty() { "network" } else { &self.mode }
    }
}

/// An invitation into this network: another relay's admin pastes it and
/// both relays add each other. `aquafed:` + base64url(JSON {url, secret}).
pub fn invite(cfg: &FedConfig) -> Option<String> {
    use base64::Engine;
    let url = if cfg.public_url.is_empty() { crate::public::url() } else { cfg.public_url.clone() };
    if cfg.secret.len() < 16 || url.is_empty() {
        return None;
    }
    let json = serde_json::json!({ "url": url, "secret": cfg.secret }).to_string();
    Some(format!("aquafed:{}", base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json)))
}

/// An invitation with a one-time admission token `t` (REV2-004). Older
/// relays read only `url` and `secret`.
pub fn invite_with(cfg: &FedConfig, token: Option<&str>) -> Option<String> {
    use base64::Engine;
    let url = if cfg.public_url.is_empty() { crate::public::url() } else { cfg.public_url.clone() };
    if cfg.secret.len() < 16 || url.is_empty() {
        return None;
    }
    let mut v = serde_json::json!({ "url": url, "secret": cfg.secret });
    if let Some(t) = token { v["t"] = serde_json::Value::String(t.to_string()); }
    Some(format!("aquafed:{}", base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v.to_string())))
}

/// The admission token of an invitation, if it has one.
pub fn invite_token(s: &str) -> Option<String> {
    use base64::Engine;
    let b = s.trim().strip_prefix("aquafed:")?;
    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(b.trim_end_matches('=')).ok()?;
    let v: serde_json::Value = serde_json::from_slice(&raw).ok()?;
    v["t"].as_str().map(str::to_string)
}

/// (relay address, secret) from an invitation.
pub fn parse_invite(s: &str) -> Option<(String, String)> {
    use base64::Engine;
    let b = s.trim().strip_prefix("aquafed:")?;
    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(b.trim_end_matches('=')).ok()?;
    let v: serde_json::Value = serde_json::from_slice(&raw).ok()?;
    let url = v["url"].as_str()?.trim().trim_end_matches('/').to_string();
    let secret = v["secret"].as_str()?.trim().to_string();
    (url.starts_with("http") && secret.len() >= 16).then_some((url, secret))
}

/// Change the federation config: read, change, save and put in place under
/// one lock. Each change used to read the config, change a copy and write it
/// back on its own — two at once (a hello pinning a key while vouches were
/// kept) and the later one wrote back a copy without the other's change.
/// `f` says whether it changed anything; it must not read `state.fed`.
/// Returns the new network when it changed and was saved.
pub(crate) fn update_cfg(state: &AppState, f: impl FnOnce(&Fed, &mut FedConfig) -> bool) -> Option<Arc<Fed>> {
    let mut g = state.fed.write().unwrap_or_else(|e| e.into_inner());
    let cur = g.clone()?;
    let mut c = cur.cfg.clone();
    if !f(&cur, &mut c) {
        return None;
    }
    if !c.ephemeral {
        if let Err(e) = c.save() {
            tracing::error!(error = %e, "federation: the config can't be saved — the change is dropped");
            return None;
        }
    }
    let next = Fed::new_after(c, &cur);
    *g = next.clone();
    next
}

/// The admin's change of the federation settings (panel, invitation): like
/// [`update_cfg`] under the same lock, but also when no network runs yet
/// (the config is then read from disk). Returns what runs after it.
pub fn admin_update(state: &AppState, f: impl FnOnce(&mut FedConfig)) -> std::io::Result<Option<Arc<Fed>>> {
    let mut g = state.fed.write().unwrap_or_else(|e| e.into_inner());
    let cur = g.clone();
    let mut c = cur.as_ref().map(|x| x.cfg.clone()).unwrap_or_else(FedConfig::load);
    f(&mut c);
    if !c.ephemeral {
        c.save()?;
    }
    let next = match &cur { Some(old) => Fed::new_after(c, old), None => Fed::new(c) };
    *g = next.clone();
    Ok(next)
}

/// Add relays to the peer list (saved, and the live config replaced).
/// Returns the ones that were new.
pub fn add_peers(state: &AppState, urls: &[String]) -> Vec<String> {
    if current(state).is_none() {
        return Vec::new();
    }
    let mut added = Vec::new();
    update_cfg(state, |f, c| {
    // REV2-004: once keys are required, only relays whose key we keep
    // (admitted) join the list.
    let strict = f.enforced();
    let me = my_id(c);
    for u in urls {
        let u = u.trim().trim_end_matches('/').to_string();
        if !(u.starts_with("http") || u.starts_with("tunnel:")) || u == me || c.peers.contains(&u) || c.peers.len() >= 64 {
            continue;
        }
        if strict && !c.pins.iter().any(|p| p.url == u) {
            continue;
        }
        c.peers.push(u.clone());
        added.push(u);
    }
    !added.is_empty()
    });
    if !added.is_empty() {
        tracing::info!(?added, "federation: new peers");
    }
    added
}

/// How this relay is known to its peers: its address, or `tunnel:<node>`
/// when it has none that answers (behind a router; fedtunnel.rs).
pub fn my_id(cfg: &FedConfig) -> String {
    if !cfg.public_url.is_empty() {
        return cfg.public_url.clone();
    }
    let u = crate::public::url();
    if !u.is_empty() { u } else { format!("tunnel:{}", cfg.node) }
}

/// A peer polling us over a reverse link (fedtunnel.rs): keep it in the
/// list under `id`, dropping `addr` — the address of it that never answered.
/// The address is dropped only on the word of the relay whose key is pinned
/// for it (`sender`): anyone else with the secret named addresses of other
/// relays and took them out of the list.
pub fn adopt_peer(state: &AppState, id: &str, addr: &str, sender: &Sender) {
    if current(state).is_none() { return }
    let changed = update_cfg(state, |f, c| {
        let has = c.peers.iter().any(|p| p == id);
        let owns_addr = f.pin_for(addr).is_some_and(|p| matches!(sender, Sender::Node(n) if *n == p.id));
        let stale = !addr.is_empty() && addr != id && owns_addr && c.peers.iter().any(|p| p == addr);
        if has && !stale {
            return false;
        }
        if stale {
            c.peers.retain(|p| p != addr);
        }
        if !has && c.peers.len() < 64 && id != my_id(c) {
            c.peers.push(id.to_string());
        }
        true
    });
    if changed.is_some() {
        tracing::info!(peer = %id, "federation: a peer behind a router linked back to us");
        state.fed_kick.notify_one();
    }
}

/// Direct requests to these peers failed lately: use their reverse link.
fn down() -> &'static std::sync::Mutex<std::collections::HashMap<String, std::time::Instant>> {
    static D: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, std::time::Instant>>> = std::sync::OnceLock::new();
    D.get_or_init(Default::default)
}
fn mark_down(peer: &str) {
    if let Ok(mut d) = down().lock() { d.insert(peer.to_string(), std::time::Instant::now()); }
}
/// The direct way to `peer` failed lately (the panel shows its reverse link).
pub fn tunnel_preferred(peer: &str) -> bool {
    is_down(peer)
}
fn is_down(peer: &str) -> bool {
    down().lock().ok().and_then(|d| d.get(peer).map(|t| t.elapsed() < Duration::from_secs(300))).unwrap_or(false)
}

/// Who signed a federation request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sender {
    /// A relay by its pinned node key (`x-aqua-fed2`): its node id.
    Node(String),
    /// Only the network's shared secret (`x-aqua-fed`): any holder of it.
    Secret,
}

pub struct Fed {
    pub cfg: FedConfig,
    http: reqwest::Client,
    key: aqua_core::crypto::SignSecret,
    /// Vouches peers sent back to our hellos, to be kept (`absorb_vouches`).
    got_vouches: std::sync::Mutex<Vec<Vouch>>,
    /// Keys of peers read from their hello answers, to be pinned
    /// (`absorb_vouches`).
    got_pins: std::sync::Mutex<Vec<Pin>>,
    /// Which signed requests we have already taken.
    replay: Arc<crate::replay::Replay>,
}

pub type FedSlot = Arc<std::sync::RwLock<Option<Arc<Fed>>>>;

pub fn slot_from_config() -> FedSlot {
    Arc::new(std::sync::RwLock::new(Fed::new(FedConfig::load())))
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// What a relay signs on its answer to a v2 request.
fn resp_message(req_v2: &str, status: u16, body: &[u8]) -> String {
    format!("aqua-fed-resp\n{req_v2}\n{status}\n{}", hex::encode(Sha256::digest(body)))
}

/// Middleware on `/v1/fed/*`: an answer to a v2-signed request is signed
/// with this relay's node key (blobs aren't: they are named by their hash).
pub async fn sign_answers(State(state): State<AppState>, req: axum::extract::Request, next: axum::middleware::Next) -> axum::response::Response {
    let h2 = req.headers().get("x-aqua-fed2").and_then(|v| v.to_str().ok()).map(str::to_string);
    let blob = req.uri().path().starts_with("/v1/fed/blob/");
    let resp = next.run(req).await;
    let (Some(h2), false, Some(fed)) = (h2, blob, registry(&state)) else { return resp };
    let (mut parts, body) = resp.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, 256 * 1024 * 1024).await else {
        return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let sig = fed.key.sign(resp_message(&h2, parts.status.as_u16(), &bytes).as_bytes());
    if let Ok(v) = axum::http::HeaderValue::from_str(&hex::encode(sig)) {
        parts.headers.insert("x-aqua-fed2-resp", v);
    }
    axum::response::Response::from_parts(parts, axum::body::Body::from(bytes))
}

fn v2_message(method: &str, path: &str, ts: i64, nonce: &str, from: &str, to: &str, body: &[u8]) -> String {
    format!("aqua-fed-v2\n{method}\n{path}\n{ts}\n{nonce}\n{from}\n{to}\n{}", hex::encode(Sha256::digest(body)))
}

fn mac(secret: &str, method: &str, path: &str, ts: i64, body: &[u8]) -> String {
    let msg = format!("{method}\n{path}\n{ts}\n{}", hex::encode(Sha256::digest(body)));
    hex::encode(aqua_core::crypto::hmac_sha256(secret.as_bytes(), msg.as_bytes()))
}

impl Fed {
    pub fn new(mut cfg: FedConfig) -> Option<Arc<Self>> {
        if !cfg.active() && !cfg.registry() {
            return None;
        }
        // Kept on disk right away: after a restart it can't step back.
        if cfg.settle(now_secs()) && !cfg.ephemeral && !cfg.dir.as_os_str().is_empty() {
            if let Err(e) = cfg.save() {
                tracing::error!(error = %e, "federation: the key requirement can't be saved");
            }
        }
        // A config made in memory (tests, the panel before saving): a key now.
        let key = hex::decode(&cfg.node_key).ok().and_then(|b| <[u8; 32]>::try_from(b).ok()).map(aqua_core::crypto::SignSecret::from_bytes);
        let key = match key {
            Some(k) => k,
            None => {
                let k = aqua_core::crypto::SignSecret::generate();
                cfg.node_key = hex::encode(k.to_bytes());
                k
            }
        };
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .user_agent(concat!("aqua-server/", env!("CARGO_PKG_VERSION")))
            .build()
            .ok()?;
        // An absolute path: the same table whatever the working folder later is.
        let seen = (!cfg.ephemeral && !cfg.dir.as_os_str().is_empty())
            .then(|| std::env::current_dir().unwrap_or_default().join(&cfg.dir).join(SEEN_FILE));
        let replay = crate::replay::Replay::shared(seen, SKEW_MS, now_ms());
        Some(Arc::new(Self { cfg, http, key, got_vouches: std::sync::Mutex::new(Vec::new()), got_pins: std::sync::Mutex::new(Vec::new()), replay }))
    }

    /// [`Self::new`] replacing `old`: what its hellos collected and wasn't
    /// kept yet (vouches, keys from answers) carries over.
    fn new_after(cfg: FedConfig, old: &Fed) -> Option<Arc<Self>> {
        let next = Self::new(cfg)?;
        for (from, to) in [(&old.got_vouches, &next.got_vouches)] {
            if let (Ok(mut a), Ok(mut b)) = (from.lock(), to.lock()) { b.append(&mut a); }
        }
        if let (Ok(mut a), Ok(mut b)) = (old.got_pins.lock(), next.got_pins.lock()) { b.append(&mut a); }
        Some(next)
    }

    /// The `x-aqua-fed` header for a request (fedtunnel.rs signs its polls).
    pub fn sign(&self, method: &str, path: &str, body: &[u8]) -> String {
        self.header(method, path, body)
    }

    /// This relay's node id and public key (hex).
    pub fn node_id(&self) -> String {
        node_id_of(self.key.public().as_bytes())
    }
    pub fn node_pub(&self) -> String {
        hex::encode(self.key.public().as_bytes())
    }

    /// An account pass (`aqpass` v2): the node key's signature over
    /// `aqpass-v2\n{u}\n{i}\n{h}\n{x}\n{n}` — (node id, signature hex).
    pub fn sign_pass(&self, msg: &str) -> (String, String) {
        (self.node_id(), hex::encode(self.key.sign(msg.as_bytes())))
    }

    /// A pass's v2 signature: by `k`, a relay of the network we keep a key
    /// for at the pass's home host `h`. None: no v2 fields, or `k` isn't a
    /// relay we keep a key for.
    pub fn pass_signed_by(&self, k: &str, sig: &str, msg: &str, host: &str) -> Option<bool> {
        if k.is_empty() && sig.is_empty() {
            return None;
        }
        if self.is_revoked(k) {
            return Some(false);
        }
        // A relay whose key we don't keep (yet): can't be checked — the
        // caller decides (taken on the HMAC only during the transition).
        let pin = self.cfg.pins.iter().find(|p| p.id == k && !p.names_only())?;
        if crate::host_of(&pin.url) != host {
            return Some(false);
        }
        let (Some(key), Some(sig)) = (hex::decode(&pin.key).ok().and_then(|b| <[u8; 32]>::try_from(b).ok()), hex::decode(sig).ok().and_then(|b| <[u8; 64]>::try_from(b).ok())) else { return Some(false) };
        Some(aqua_core::crypto::SignPublic::from_bytes(key).verify(msg.as_bytes(), &sig).is_ok())
    }

    /// Use up `key` (a pass's nonce) for good: in the same table as signed
    /// requests (on disk, survives a restart), kept until `until_ms` and a
    /// clock window after.
    pub fn take_once(&self, sender: &str, key: &str, until_ms: i64) -> bool {
        self.replay.take(sender, key, until_ms, now_ms())
    }

    /// A tombstone for an account deleted here, signed by our node key (A7).
    pub fn sign_tomb(&self, uid: &str, username: &str, created_at: i64, deleted_at: i64) -> crate::directory::Tomb {
        let mut t = crate::directory::Tomb { uid: uid.into(), created_at, username: username.into(), deleted_at, origin: self.node_id(), ..Default::default() };
        t.sig = hex::encode(self.key.sign(t.message().as_bytes()));
        t
    }

    /// Signed by the relay it names (`origin`), whose key we keep and which
    /// isn't revoked.
    pub fn verify_tomb(&self, t: &crate::directory::Tomb) -> bool {
        if self.cfg.revoked.iter().any(|r| *r == t.origin) || LEAKED_NODE_IDS.contains(&t.origin.as_str()) {
            return false;
        }
        // The relay's current key; or a key it changed away from, for
        // deletions made before we followed the change.
        let key = match self.cfg.pins.iter().find(|p| p.id == t.origin && !p.names_only()) {
            Some(pin) => pin.key.clone(),
            None => match self.cfg.retired_keys.iter().find(|x| x.0 == t.origin) {
                Some((_, k, at)) if t.deleted_at >= 0 && (t.deleted_at as u64) <= *at => k.clone(),
                _ => return false,
            },
        };
        let (Some(k), Some(sig)) = (hex::decode(&key).ok().and_then(|b| <[u8; 32]>::try_from(b).ok()), hex::decode(&t.sig).ok().and_then(|b| <[u8; 64]>::try_from(b).ok())) else { return false };
        aqua_core::crypto::SignPublic::from_bytes(k).verify(t.message().as_bytes(), &sig).is_ok()
    }

    /// Revoked here, or a published key (`LEAKED_NODE_IDS`).
    pub fn is_revoked(&self, id: &str) -> bool {
        self.cfg.revoked.iter().any(|r| r == id) || self.cfg.retired.iter().any(|r| r == id) || LEAKED_NODE_IDS.contains(&id)
    }

    /// The key we keep for `peer` (by address), if any.
    pub fn pin_for(&self, peer: &str) -> Option<&Pin> {
        let peer = peer.trim_end_matches('/');
        self.cfg.pins.iter().find(|p| p.url == peer)
    }

    /// Node keys are required: requests signed only with the shared secret
    /// are refused (except hello / ping / names-hello, which only queue a
    /// relay for the admin). Unlike `strict()` before it, this isn't worked
    /// out from the peer list on each request (any relay without a key put
    /// on the list switched it off): it's a state kept in the config that
    /// only moves forward ([`FedConfig::settle`]). Only the admin's window
    /// for relays without keys lifts it, until that window ends.
    pub fn enforced(&self) -> bool {
        if env_enforced() {
            return true;
        }
        let now = now_secs();
        let on = self.cfg.auth_mode == "enforced" || self.cfg.all_keys_known() || (self.cfg.auth_deadline != 0 && now >= self.cfg.auth_deadline);
        on && self.cfg.legacy_until <= now
    }

    /// The `x-aqua-fed2` header for a request to `peer`, when we know its key.
    pub fn sign2(&self, peer: &str, method: &str, path: &str, body: &[u8]) -> Option<String> {
        let to = self.pin_for(peer)?.id.clone();
        let mut n = [0u8; 12];
        rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut n);
        let nonce = hex::encode(n);
        let ts = now_ms();
        let from = self.node_id();
        let sig = self.key.sign(v2_message(method, path, ts, &nonce, &from, &to, body).as_bytes());
        Some(format!("{ts}.{nonce}.{from}.{to}.{}", hex::encode(sig)))
    }

    /// Check an `x-aqua-fed2` header: `Some(sender id)` when it's good.
    fn verify2(&self, h: &str, method: &str, path: &str, body: &[u8]) -> Option<String> {
        let mut it = h.splitn(5, '.');
        let (ts, nonce, from, to, sig) = (it.next()?, it.next()?, it.next()?, it.next()?, it.next()?);
        let ts: i64 = ts.parse().ok()?;
        if (now_ms() - ts).abs() > SKEW_MS || to != self.node_id() || self.is_revoked(from) {
            return None;
        }
        let pin = self.cfg.pins.iter().find(|p| p.id == from)?;
        let key: [u8; 32] = hex::decode(&pin.key).ok()?.try_into().ok()?;
        let sig: [u8; 64] = hex::decode(sig).ok()?.try_into().ok()?;
        aqua_core::crypto::SignPublic::from_bytes(key)
            .verify(v2_message(method, path, ts, nonce, from, to, body).as_bytes(), &sig)
            .ok()?;
        self.replay.take(from, &format!("v2:{from}:{nonce}"), ts, now_ms()).then(|| from.to_string())
    }

    /// Add both signatures to a request to `peer`.
    fn signed(&self, rb: reqwest::RequestBuilder, peer: &str, method: &str, path: &str, body: &[u8]) -> reqwest::RequestBuilder {
        self.signed2(rb, peer, method, path, body).0
    }

    /// [`Self::signed`], also giving the v2 header sent (its answer must be
    /// signed back over it, see [`Self::checked`]).
    fn signed2(&self, rb: reqwest::RequestBuilder, peer: &str, method: &str, path: &str, body: &[u8]) -> (reqwest::RequestBuilder, Option<String>) {
        let rb = rb.header("x-aqua-fed", self.header(method, path, body));
        match self.sign2(peer, method, path, body) {
            Some(s) => (rb.header("x-aqua-fed2", s.clone()), Some(s)),
            None => (rb, None),
        }
    }

    /// The answer of a relay whose key we keep must be signed by that key
    /// over our request's v2 header, its status and its body (1.21.0-23):
    /// a proxy or a man in the middle can't change what it pulls (accounts,
    /// names, history). Answers of relays without keys pass as before.
    async fn checked(&self, peer: &str, sent2: Option<String>, r: reqwest::Response) -> Option<reqwest::Response> {
        let Some(h2) = sent2 else { return Some(r) };
        let pin = self.pin_for(peer)?;
        let key: [u8; 32] = hex::decode(&pin.key).ok()?.try_into().ok()?;
        let status = r.status();
        let ct = r.headers().get("content-type").cloned();
        let sig = r.headers().get("x-aqua-fed2-resp").and_then(|v| v.to_str().ok()).and_then(|s| hex::decode(s).ok()).and_then(|b| <[u8; 64]>::try_from(b).ok());
        let body = r.bytes().await.ok()?;
        let ok = sig.is_some_and(|s| aqua_core::crypto::SignPublic::from_bytes(key).verify(resp_message(&h2, status.as_u16(), &body).as_bytes(), &s).is_ok());
        if !ok {
            tracing::warn!(%peer, "federation: an answer without a valid signature of the peer's node key — dropped");
            return None;
        }
        let mut b = axum::http::Response::builder().status(status);
        if let Some(ct) = ct { b = b.header("content-type", ct); }
        b.body(body.to_vec()).ok().map(reqwest::Response::from)
    }

    /// What a hello carries about this relay's key: its public key, a
    /// signature by it over our address and key (so nobody passes off a key
    /// they don't hold), and — after a key change — the new key signed by
    /// the old one.
    pub fn hello_key_fields(&self) -> serde_json::Value {
        self.hello_key_fields_at(&my_id(&self.cfg))
    }

    /// [`Self::hello_key_fields`] for the address `url` the hello names.
    pub fn hello_key_fields_at(&self, url: &str) -> serde_json::Value {
        let key = self.node_pub();
        let sig = hex::encode(self.key.sign(format!("aqua-fed-hello\n{url}\n{key}").as_bytes()));
        serde_json::json!({ "node_key": key, "node_sig": sig, "rotated": self.cfg.rotated, "rotations": self.cfg.rotations, "admit": self.cfg.join_token, "vouches": self.cfg.vouches })
    }

    /// A vouch by this relay for node `id` at `url`.
    fn vouch_for(&self, id: &str, url: &str) -> Vouch {
        Vouch { by: self.node_id(), sig: hex::encode(self.key.sign(format!("aqua-fed-admit\n{id}\n{url}").as_bytes())) }
    }

    /// A good vouch for `pin`: signed by a relay whose key we keep and that
    /// isn't revoked.
    fn vouched(&self, pin: &Pin, vouches: &[Vouch]) -> bool {
        vouches.iter().any(|v| {
            if self.is_revoked(&v.by) { return false; }
            let Some(by) = self.cfg.pins.iter().find(|p| p.id == v.by) else { return false };
            let (Some(k), Some(s)) = (hex::decode(&by.key).ok().and_then(|b| <[u8; 32]>::try_from(b).ok()), hex::decode(&v.sig).ok().and_then(|b| <[u8; 64]>::try_from(b).ok())) else { return false };
            aqua_core::crypto::SignPublic::from_bytes(k).verify(format!("aqua-fed-admit\n{}\n{}", pin.id, pin.url).as_bytes(), &s).is_ok()
        })
    }

    /// Over the peer's reverse link, when that's the way to it: it has no
    /// address, or its address stopped answering and it polls us.
    async fn via_tunnel(&self, peer: &str, method: &str, path: &str, body: &[u8], wait: Duration) -> Option<Option<reqwest::Response>> {
        let tunnel_only = peer.starts_with("tunnel:");
        if !tunnel_only && !(is_down(peer) && crate::fedtunnel::active(peer)) {
            return None;
        }
        let sig = self.header(method, path, body);
        let sig2 = self.sign2(peer, method, path, body);
        let r = crate::fedtunnel::request(peer, method, path, body.to_vec(), sig, sig2.clone().unwrap_or_default(), wait).await;
        match r.filter(|r| r.status().is_success()) {
            Some(r) => Some(self.checked(peer, sig2, r).await),
            None => Some(None),
        }
    }

    fn header(&self, method: &str, path: &str, body: &[u8]) -> String {
        // Strictly increasing: two equal requests in one millisecond got the
        // same signature, and receivers now take each signature once.
        static LAST: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);
        let now = now_ms();
        let ts = match LAST.fetch_update(std::sync::atomic::Ordering::SeqCst, std::sync::atomic::Ordering::SeqCst, |l| Some(now.max(l + 1))) {
            Ok(prev) => now.max(prev + 1),
            Err(_) => now,
        };
        format!("{ts}.{}", mac(&self.cfg.secret, method, path, ts, body))
    }

    pub fn verify(&self, headers: &HeaderMap, method: &str, path: &str, body: &[u8]) -> bool {
        self.verify_from(headers, method, path, body).is_some()
    }

    /// [`Self::verify`], saying who signed: a relay by its pinned node key,
    /// or anyone holding the shared secret.
    pub fn verify_from(&self, headers: &HeaderMap, method: &str, path: &str, body: &[u8]) -> Option<Sender> {
        // A relay whose key we know signs with it (AQUA-SEC-009).
        // A sender we don't know yet (it learnt our key first) falls back to
        // the shared secret below, like a relay without keys; a known
        // sender with a bad v2 signature is refused.
        if let Some(h2) = headers.get("x-aqua-fed2").and_then(|v| v.to_str().ok()) {
            let from = h2.split('.').nth(2).unwrap_or("");
            if self.is_revoked(from) {
                return None;
            }
            if self.cfg.pins.iter().any(|p| p.id == from) {
                let from = self.verify2(h2, method, path, body)?;
                // A solo relay (names only) reads and announces names, nothing else.
                let base = path.split('?').next().unwrap_or(path);
                if self.cfg.pins.iter().find(|p| p.id == from).is_some_and(Pin::names_only)
                    && !matches!(base, "/v1/fed/names" | "/v1/fed/names/hello" | "/v1/fed/ping")
                {
                    tracing::warn!(%path, "federation: a relay of the name registry asked for more than names — refused");
                    return None;
                }
                // The `x-aqua-fed` sent next to it is used up too: otherwise
                // the same request, with `x-aqua-fed2` cut off, passed again
                // on the shared secret wherever that is still taken.
                if let Some((ts, sig)) = self.secret_sig(headers, method, path, body) {
                    if !self.replay.take(&from, &sig, ts, now_ms()) {
                        return None;
                    }
                }
                return Some(Sender::Node(from));
            }
        }
        // Only the shared secret: fine while we don't know every peer's key
        // (older relays), and always for introducing oneself.
        let base = path.split('?').next().unwrap_or(path);
        // Introductions only: a hello queues an unknown relay for the admin.
        let intro = matches!(base, "/v1/fed/hello" | "/v1/fed/ping" | "/v1/fed/names/hello");
        if self.enforced() && !intro {
            tracing::warn!(%path, "federation: a request signed only with the shared secret — refused (node keys are required)");
            return None;
        }
        let (ts, sig) = self.secret_sig(headers, method, path, body)?;
        // AQUA-SEC-009: a signed request was good for its whole ±5-minute
        // window, as many times as anyone sent it again. Each signature is
        // now taken once (signatures cover the time, so a retry signs anew).
        self.replay.take("secret", &sig, ts, now_ms()).then_some(Sender::Secret)
    }

    /// A good `x-aqua-fed` (shared-secret) signature of this request, fresh:
    /// its time and signature. Not used up here.
    fn secret_sig(&self, headers: &HeaderMap, method: &str, path: &str, body: &[u8]) -> Option<(i64, String)> {
        let h = headers.get("x-aqua-fed").and_then(|v| v.to_str().ok())?;
        let (ts, sig) = h.split_once('.')?;
        let ts = ts.parse::<i64>().ok()?;
        if (now_ms() - ts).abs() > SKEW_MS {
            return None;
        }
        let want = mac(&self.cfg.secret, method, path, ts, body);
        bool::from(subtle::ConstantTimeEq::ct_eq(want.as_bytes(), sig.as_bytes())).then(|| (ts, sig.to_string()))
    }

    /// May `sender` speak for the relay known as `id` (its reverse link,
    /// dropping its old address)? Only the relay whose key is pinned for
    /// `id`; anyone with the secret only while no key is pinned there and
    /// the network isn't on keys yet.
    pub fn speaks_for(&self, sender: &Sender, id: &str) -> bool {
        match self.pin_for(id) {
            Some(p) => matches!(sender, Sender::Node(n) if *n == p.id),
            None => !self.enforced(),
        }
    }

    /// Is `peer` a relay we still talk to (1.2.0 or newer)? Asked once in a
    /// while (`/v1/stats` says its version; older relays don't say).
    async fn modern(&self, peer: &str) -> bool {
        self.peer_version(peer).await.is_some()
    }

    /// The version `peer` says it runs, when it is one we still talk to
    /// (1.2.0 or newer). Asked once in a while (`/v1/stats`; older relays
    /// don't say). A relay known only by its reverse link: None.
    pub(crate) async fn peer_version(&self, peer: &str) -> Option<String> {
        if peer.starts_with("tunnel:") {
            return None;
        }
        type Seen = std::collections::HashMap<String, (std::time::Instant, Option<String>)>;
        static SEEN: std::sync::Mutex<Option<Seen>> = std::sync::Mutex::new(None);
        if let Some((at, v)) = SEEN.lock().ok().and_then(|m| m.as_ref().and_then(|m| m.get(peer).cloned())) {
            if at.elapsed() < Duration::from_secs(if v.is_some() { 600 } else { 120 }) {
                return v;
            }
        }
        let v = match self.http.get(format!("{peer}/v1/stats")).timeout(Duration::from_secs(5)).send().await {
            Ok(r) => r.json::<serde_json::Value>().await.ok()
                .and_then(|v| v["version"].as_str().map(str::to_string))
                .filter(|v| crate::relay_version_ok(v)),
            // Unreachable now: don't decide for 10 minutes on that.
            Err(_) => { mark_down(peer); return None }
        };
        if v.is_none() {
            tracing::warn!(%peer, "federation: this relay is older than {} — not talking to it", crate::MIN_RELAY);
        }
        if let Ok(mut m) = SEEN.lock() {
            m.get_or_insert_with(std::collections::HashMap::new).insert(peer.to_string(), (std::time::Instant::now(), v.clone()));
        }
        v
    }

    async fn get(&self, peer: &str, path: &str) -> Option<reqwest::Response> {
        if let Some(r) = self.via_tunnel(peer, "GET", path, b"", Duration::from_secs(30)).await {
            return r;
        }
        if !self.modern(peer).await {
            return self.tunnel_fallback(peer, "GET", path, b"").await;
        }
        let (rb, h2) = self.signed2(self.http.get(format!("{peer}{path}")), peer, "GET", path, b"");
        match rb.send().await {
            Ok(r) if r.status().is_success() => self.checked(peer, h2, r).await,
            Ok(_) => None,
            Err(_) => { mark_down(peer); self.tunnel_fallback(peer, "GET", path, b"").await }
        }
    }

    async fn post(&self, peer: &str, path: &str, body: Vec<u8>) -> Option<reqwest::Response> {
        if let Some(r) = self.via_tunnel(peer, "POST", path, &body, Duration::from_secs(8)).await {
            return r;
        }
        if !self.modern(peer).await {
            return self.tunnel_fallback(peer, "POST", path, &body).await;
        }
        let (rb, h2) = self.signed2(self.http.post(format!("{peer}{path}")), peer, "POST", path, &body);
        let sent = rb
            .header("content-type", "application/json")
            .timeout(Duration::from_secs(4))
            .body(body.clone()).send().await;
        match sent {
            Ok(r) if r.status().is_success() => self.checked(peer, h2, r).await,
            Ok(_) => None,
            Err(_) => { mark_down(peer); self.tunnel_fallback(peer, "POST", path, &body).await }
        }
    }

    /// A signed POST to one peer with its answer as is — status and text,
    /// errors included (stars.rs passes the star relay's own message on).
    pub async fn post_answer(&self, peer: &str, path: &str, body: Vec<u8>) -> Option<(u16, String)> {
        let sent = self.signed(self.http.post(format!("{peer}{path}")), peer, "POST", path, &body)
            .header("content-type", "application/json")
            .timeout(Duration::from_secs(10))
            .body(body.clone()).send().await;
        match sent {
            Ok(r) => { let st = r.status().as_u16(); Some((st, r.text().await.unwrap_or_default())) }
            Err(_) => {
                let r = self.tunnel_fallback(peer, "POST", path, &body).await?;
                let st = r.status().as_u16();
                Some((st, r.text().await.unwrap_or_default()))
            }
        }
    }

    /// The direct way failed: the reverse link, if the peer keeps one.
    async fn tunnel_fallback(&self, peer: &str, method: &str, path: &str, body: &[u8]) -> Option<reqwest::Response> {
        if !crate::fedtunnel::active(peer) {
            return None;
        }
        mark_down(peer);
        let sig = self.header(method, path, body);
        let sig2 = self.sign2(peer, method, path, body);
        let r = crate::fedtunnel::request(peer, method, path, body.to_vec(), sig, sig2.clone().unwrap_or_default(), Duration::from_secs(30)).await.filter(|r| r.status().is_success())?;
        self.checked(peer, sig2, r).await
    }

    /// Try to hand an envelope to whichever peer has the recipient online.
    pub async fn forward(&self, msg: &RelayMessage) -> bool {
        let Ok(body) = serde_json::to_vec(msg) else { return false };
        let tries = self.cfg.peers.iter().map(|p| {
            let body = body.clone();
            async move {
                match self.post(p, "/v1/fed/deliver", body).await {
                    Some(r) => r.json::<serde_json::Value>().await.ok().and_then(|v| v["delivered"].as_bool()).unwrap_or(false),
                    None => false,
                }
            }
        });
        futures_util::future::join_all(tries).await.into_iter().any(|d| d)
    }

    /// Say hello to every peer: they add us if they didn't know us, and we
    /// learn whom they know (same secret — the reply is signed by it).
    /// Returns relays that were new to us.
    pub async fn hello_all(&self) -> Vec<String> {
        let me = my_id(&self.cfg);
        let mut hello = serde_json::json!({ "url": me, "peers": self.cfg.peers });
        if let (Some(o), serde_json::Value::Object(k)) = (hello.as_object_mut(), self.hello_key_fields()) { o.extend(k); }
        let body = serde_json::to_vec(&hello).unwrap_or_default();
        let tries = self.cfg.peers.iter().map(|p| {
            let body = body.clone();
            async move {
                let r = self.post(p, "/v1/fed/hello", body).await?;
                let v: serde_json::Value = r.json().await.ok()?;
                self.note_vouch(&v);
                self.note_answer_key(p, &v);
                let mut out: Vec<String> = v["peers"].as_array()?.iter().filter_map(|x| x.as_str().map(str::to_string)).collect();
                if let Some(s) = v["self"].as_str() { out.push(s.to_string()); }
                Some(out)
            }
        });
        let mut known: Vec<String> = futures_util::future::join_all(tries).await.into_iter().flatten().flatten().collect();
        known.sort();
        known.dedup();
        known.retain(|u| u.trim_end_matches('/') != me && !self.cfg.peers.iter().any(|p| p == u.trim_end_matches('/')));
        known
    }

    /// Say hello to one relay (after learning its key: so it learns ours).
    pub async fn hello_to(&self, peer: &str) {
        let mut hello = serde_json::json!({ "url": my_id(&self.cfg), "peers": self.cfg.peers });
        if let (Some(o), serde_json::Value::Object(k)) = (hello.as_object_mut(), self.hello_key_fields()) { o.extend(k); }
        if let Some(r) = self.post(peer, "/v1/fed/hello", serde_json::to_vec(&hello).unwrap_or_default()).await {
            if let Ok(v) = r.json::<serde_json::Value>().await { self.note_vouch(&v); self.note_answer_key(peer, &v); }
        }
    }

    /// A hello answer carries the answering relay's key, signed by it over
    /// its address. Kept when that address is the one we asked (we reached
    /// it there ourselves) and the key may be pinned (`pin_change`): a
    /// relay behind a router learns the key of the public one it polls
    /// without waiting for a hello back over its reverse link.
    fn note_answer_key(&self, peer: &str, v: &serde_json::Value) {
        let peer = peer.trim_end_matches('/');
        if v["self"].as_str().map(|s| s.trim_end_matches('/')) != Some(peer) || !peer.starts_with("http") {
            return;
        }
        let h = Hello {
            url: peer.to_string(),
            admit: String::new(),
            vouches: Vec::new(),
            peers: Vec::new(),
            node_key: v["node_key"].as_str().unwrap_or("").to_string(),
            node_sig: v["node_sig"].as_str().unwrap_or("").to_string(),
            rotated: v["rotated"].as_str().unwrap_or("").to_string(),
            rotations: serde_json::from_value(v["rotations"].clone()).unwrap_or_default(),
        };
        // Only a peer on our own list, under a key we take: a first key or
        // a signed change. Another key without a signature isn't taken here.
        let pin = match pin_change(self, &h) {
            Some(PinChange::New(p)) if self.cfg.peers.iter().any(|u| u == peer) => p,
            Some(PinChange::Rotated(p, ..)) => p,
            _ => return,
        };
        if let Ok(mut g) = self.got_pins.lock() { g.push(pin); }
    }

    /// A hello answer may carry a vouch for us (we were admitted).
    fn note_vouch(&self, v: &serde_json::Value) {
        if let Ok(vouch) = serde_json::from_value::<Vouch>(v["vouch"].clone()) {
            if !vouch.by.is_empty() {
                if let Ok(mut g) = self.got_vouches.lock() { g.push(vouch); }
            }
        }
    }

    /// Is `url` a relay of this network (does it accept our signature)?
    pub async fn same_network(&self, url: &str) -> bool {
        self.get(url.trim_end_matches('/'), "/v1/fed/ping").await.is_some()
    }

    /// Have a peer with mail set up send a sign-up code (this relay has no
    /// mail server): the SMTP password never leaves the relay that has it.
    pub async fn send_mail(&self, to: &str, code: &str, link: Option<&str>) -> bool {
        let body = serde_json::to_vec(&serde_json::json!({ "to": to, "code": code, "link": link })).unwrap_or_default();
        for p in &self.cfg.peers {
            let r = self.signed(self.http.post(format!("{p}/v1/fed/mail")), p, "POST", "/v1/fed/mail", &body)
                .header("content-type", "application/json")
                .timeout(Duration::from_secs(50))
                .body(body.clone()).send().await;
            if r.is_ok_and(|r| r.status().is_success()) {
                tracing::info!(peer = %p, "sign-up code e-mailed by a peer");
                return true;
            }
        }
        false
    }

    /// Tell every peer to pull from us now.
    pub async fn poke_all(&self) {
        let body = serde_json::to_vec(&serde_json::json!({ "from": self.cfg.public_url })).unwrap_or_default();
        let tries = self.cfg.peers.iter().map(|p| {
            let body = body.clone();
            async move { self.post(p, "/v1/fed/poke", body).await.is_some() }
        });
        let ok = futures_util::future::join_all(tries).await.into_iter().filter(|x| *x).count();
        tracing::debug!(ok, peers = self.cfg.peers.len(), "federation: poked peers");
    }

    /// Tell every peer: something is queued here — pull it for whoever is
    /// connected to you (only that, not a full sync).
    pub async fn poke_inbox_all(&self) {
        let body = serde_json::to_vec(&serde_json::json!({ "from": self.cfg.public_url, "inbox": true })).unwrap_or_default();
        let tries = self.cfg.peers.iter().map(|p| {
            let body = body.clone();
            async move { self.post(p, "/v1/fed/poke", body).await.is_some() }
        });
        futures_util::future::join_all(tries).await;
    }

    /// Envelopes that peers kept for `user` (they drop them once sent).
    pub async fn collect_inbox(&self, user: &str) -> Vec<RelayMessage> {
        let path = format!("/v1/fed/inbox/{user}");
        // All peers at once (one by one, every slow one added its wait).
        let users = [user.to_string()];
        let tries = self.cfg.peers.iter().map(|p| {
            let (path, users) = (path.clone(), &users);
            async move {
                if let Some(got) = self.take_kept(p, users).await { return Some(got); }
                // A peer from before 1.20.1-8: it hands them over and forgets them.
                self.get(p, &path).await?.json::<Vec<RelayMessage>>().await.ok()
            }
        });
        futures_util::future::join_all(tries).await.into_iter().flatten().flatten().collect()
    }

    /// What `peer` keeps for `users`, without it forgetting them before we
    /// have them: it lists them (`/v1/fed/inboxes2`), we confirm what came
    /// (`/v1/fed/inbox_ack`) and only then it deletes. The old endpoints
    /// deleted while answering — a reply lost on the way (a timeout, a
    /// tunnel that dropped) lost the messages with it. None: not supported
    /// there (or not reachable).
    async fn take_kept(&self, peer: &str, users: &[String]) -> Option<Vec<RelayMessage>> {
        let body = serde_json::to_vec(&serde_json::json!({ "users": users })).ok()?;
        let r = self.post(peer, "/v1/fed/inboxes2", body).await?;
        let rows: Vec<serde_json::Value> = r.json().await.ok()?;
        let mut ids = Vec::new();
        let mut out = Vec::new();
        for row in rows {
            if let Some(id) = row["id"].as_i64() { ids.push(id); }
            if let Ok(m) = serde_json::from_value::<RelayMessage>(row["m"].clone()) { out.push(m); }
        }
        if !ids.is_empty() {
            let body = serde_json::to_vec(&serde_json::json!({ "ids": ids })).unwrap_or_default();
            // Not confirmed: they come again next time (the device drops copies by id).
            let _ = self.post(peer, "/v1/fed/inbox_ack", body).await;
        }
        Some(out)
    }

    /// Envelopes peers keep for any of `users` (connected here) — one call per
    /// peer. A push (`forward`) that failed once no longer waits for the
    /// recipient's next reconnect. Peers older than this ignore the call.
    pub async fn collect_inboxes(&self, users: &[String]) -> Vec<RelayMessage> {
        if users.is_empty() { return Vec::new(); }
        let body = serde_json::to_vec(&serde_json::json!({ "users": users })).unwrap_or_default();
        let tries = self.cfg.peers.iter().map(|p| {
            let body = body.clone();
            async move {
                if let Some(got) = self.take_kept(p, users).await { return Some(got); }
                let r = self.post(p, "/v1/fed/inboxes", body).await?;
                r.json::<Vec<RelayMessage>>().await.ok()
            }
        });
        futures_util::future::join_all(tries).await.into_iter().flatten().flatten().collect()
    }

    pub(crate) async fn blob_response(&self, peer: &str, cid: &str) -> Option<reqwest::Response> {
        let path = format!("/v1/fed/blob/{cid}");
        if self.modern(peer).await {
            if let Ok(response) = self.signed(self.http.get(format!("{peer}{path}")), peer, "GET", &path, b"")
                .timeout(Duration::from_secs(1200)).send().await {
                return response.status().is_success().then_some(response);
            }
        }
        self.get(peer, &path).await
    }

    pub async fn fetch_bytes(&self, path: &str) -> Option<(String, Vec<u8>)> {
        for p in &self.cfg.peers {
            if let Some(r) = self.get(p, path).await {
                let mime = r.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("application/octet-stream").to_string();
                if let Ok(b) = r.bytes().await {
                    return Some((mime, b.to_vec()));
                }
            }
        }
        None
    }

    pub async fn fetch_bundle(&self, user: &str) -> Option<serde_json::Value> {
        let path = format!("/v1/fed/bundle/{user}");
        for p in &self.cfg.peers {
            if let Some(r) = self.get(p, &path).await {
                if let Ok(v) = r.json().await {
                    return Some(v);
                }
            }
        }
        None
    }
}

/// The network this relay is part of (not a solo relay's name registry).
pub fn current(state: &AppState) -> Option<Arc<Fed>> {
    state.fed.read().ok().and_then(|g| g.clone()).filter(|f| f.cfg.active())
}

/// Whoever we share the name registry with: the network, or — for a solo
/// relay — the network it reserves names in.
pub fn registry(state: &AppState) -> Option<Arc<Fed>> {
    state.fed.read().ok().and_then(|g| g.clone())
}

/// Signed by our secret, from a relay new enough — for the name registry,
/// which solo relays use too.
fn check_registry(state: &AppState, headers: &HeaderMap, method: &str, path: &str, body: &[u8]) -> Result<Arc<Fed>, StatusCode> {
    let fed = registry(state).ok_or(StatusCode::NOT_FOUND)?;
    if !fed.verify(headers, method, path, body) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    if !crate::caller_version(headers, None).as_deref().is_some_and(crate::relay_version_ok) {
        return Err(StatusCode::UPGRADE_REQUIRED);
    }
    Ok(fed)
}

fn my_url(cfg: &FedConfig) -> String {
    if cfg.public_url.is_empty() { crate::public::url() } else { cfg.public_url.clone() }
}

/// `GET /v1/fed/names?since=T` — the name registry (see
/// [`crate::directory::Directory::names_since`]).
pub async fn h_names(State(state): State<AppState>, headers: HeaderMap, uri: axum::http::Uri, Query(q): Query<Since>) -> Result<impl IntoResponse, StatusCode> {
    // Only the path as signed: the other spellings tried before (no query,
    // `since` alone) let one signature stand for any page.
    let fed = check_registry(&state, &headers, "GET", signed_path(&uri), b"")?;
    let dir = state.directory.clone();
    let names = tokio::task::spawn_blocking(move || dir.names_after(q.since, q.after.as_deref(), 1000)).await.unwrap_or_default();
    Ok(Json(serde_json::json!({ "home": my_url(&fed.cfg), "mode": if fed.cfg.active() { "network" } else { "solo" }, "names": names })))
}

/// `POST /v1/fed/names/hello {url}` — a solo relay shares the name registry
/// with us; it learns our peers (to reserve its names with all of them).
pub async fn h_names_hello(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Result<Json<serde_json::Value>, StatusCode> {
    let fed = check_registry(&state, &headers, "POST", "/v1/fed/names/hello", &body)?;
    let v: serde_json::Value = serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;
    let url = v["url"].as_str().unwrap_or("").trim().trim_end_matches('/').to_string();
    let k = fed.hello_key_fields();
    let answer = |fed: &Fed| Json(serde_json::json!({ "self": my_url(&fed.cfg), "peers": fed.cfg.peers,
        "node_key": k["node_key"], "node_sig": k["node_sig"], "rotated": k["rotated"], "rotations": k["rotations"] }));
    // A solo relay brings its node key (1.21.0-25): pinned as "names", like a
    // hello pins a relay of the network. Once keys are required, a solo
    // relay we don't know joins only with an invitation token or a vouch —
    // otherwise it waits for the admin; without a key it isn't taken at all.
    if fed.cfg.active() && url.starts_with("http") && url != my_url(&fed.cfg) {
        let h: Option<Hello> = serde_json::from_value(v.clone()).ok();
        let admitted = match &h {
            Some(h) => decide_pin(&state, &fed, h, true).await.0,
            None => !fed.enforced() || fed.pin_for(&url).is_some(),
        };
        if !admitted {
            return Ok(Json(serde_json::json!({ "pending": true, "self": my_url(&fed.cfg) })));
        }
    }
    let joined = update_cfg(&state, |_, c| {
        if !(c.active() && url.starts_with("http") && url != my_url(c) && !c.solo_peers.contains(&url) && c.solo_peers.len() < 256) {
            return false;
        }
        c.solo_peers.push(url.clone());
        true
    });
    if joined.is_some() {
        tracing::info!(%url, "name registry: a solo relay joined");
    }
    let fed = registry(&state).ok_or(StatusCode::NOT_FOUND)?;
    Ok(answer(&fed))
}

/// A solo relay: say hello to the network for the name registry. Returns
/// the relays it told us about that we didn't know.
pub async fn names_hello(fed: &Fed) -> Vec<String> {
    let me = my_url(&fed.cfg);
    // Its key too (1.21.0-25): the network pins it as a relay of the names
    // registry, and it pins theirs from the answers.
    let mut hello = serde_json::json!({ "url": me });
    if let (Some(o), serde_json::Value::Object(k)) = (hello.as_object_mut(), fed.hello_key_fields_at(&me)) { o.extend(k); }
    let body = serde_json::to_vec(&hello).unwrap_or_default();
    let mut known = Vec::new();
    for p in &fed.cfg.peers {
        if let Some(r) = fed.post(p, "/v1/fed/names/hello", body.clone()).await {
            if let Ok(v) = r.json::<serde_json::Value>().await {
                fed.note_answer_key(p, &v);
                for x in v["peers"].as_array().into_iter().flatten().chain(std::iter::once(&v["self"])) {
                    if let Some(u) = x.as_str() { known.push(u.trim_end_matches('/').to_string()); }
                }
            }
        }
    }
    known.sort();
    known.dedup();
    known.retain(|u| *u != me && !fed.cfg.peers.contains(u));
    known
}

/// Where we are in a peer's rows: `(updated_at, key)` of the last row read
/// (REV-001). The key breaks ties between equal timestamps, so a page that
/// ends inside a run of them goes on right after its last row — no row is
/// skipped or read twice. Relays before 1.21.0-23 ignore `after` and answer
/// `updated_at > since`: with them it works as before.
pub type Cursor = (i64, String);

/// `?since=T&after=KEY` (the key percent-encoded; empty key: no `after`).
fn cursor_query(c: &Cursor) -> String {
    if c.1.is_empty() {
        return format!("since={}", c.0);
    }
    let mut key = String::new();
    for b in c.1.bytes() {
        if b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-' { key.push(b as char) } else { key.push_str(&format!("%{b:02X}")) }
    }
    format!("since={}&after={key}", c.0)
}

/// The signed path of a cursor request as it arrived (with its query).
fn signed_path(uri: &axum::http::Uri) -> &str {
    uri.path_and_query().map(|p| p.as_str()).unwrap_or(uri.path())
}

/// Pull the name registry from one relay (paged).
async fn sync_names(state: &AppState, fed: &Fed, peer: &str) {
    for _ in 0..50 {
        let from = cursor(state, peer, "names");
        let Some(r) = fed.get(peer, &format!("/v1/fed/names?{}", cursor_query(&from))).await else { return };
        let Ok(v) = r.json::<serde_json::Value>().await else { return };
        let home = v["home"].as_str().unwrap_or(peer).trim_end_matches('/').to_string();
        let Ok(rows) = serde_json::from_value::<Vec<crate::directory::NameRow>>(v["names"].clone()) else { return };
        if rows.is_empty() { return; }
        let n = rows.len();
        let last = rows.last().map(|r| (r.updated_at, r.username.clone())).unwrap_or(from);
        let dir = state.directory.clone();
        let Ok(moved) = tokio::task::spawn_blocking(move || rows.iter().filter_map(|r| dir.fed_apply_name(r, &home)).collect::<Vec<_>>()).await else { return };
        for (old, new) in &moved {
            crate::move_live(state, old, new);
        }
        if !keep_cursor(state, peer, "names", &last) { return; }
        if n < 1000 { return; }
    }
}

/// `check` for other modules (fedtunnel.rs), with who signed.
pub fn check_pub(state: &AppState, headers: &HeaderMap, method: &str, path: &str, body: &[u8]) -> Result<(Arc<Fed>, Sender), StatusCode> {
    check_from(state, headers, method, path, body)
}

fn check(state: &AppState, headers: &HeaderMap, method: &str, path: &str, body: &[u8]) -> Result<Arc<Fed>, StatusCode> {
    check_from(state, headers, method, path, body).map(|(f, _)| f)
}

/// A request of a relay of this network: the network and who signed it.
fn check_from(state: &AppState, headers: &HeaderMap, method: &str, path: &str, body: &[u8]) -> Result<(Arc<Fed>, Sender), StatusCode> {
    let fed = current(state).ok_or(StatusCode::NOT_FOUND)?;
    let Some(sender) = fed.verify_from(headers, method, path, body) else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    // Relays older than 1.2.0 don't know two-step codes: an account copied
    // there could be signed into without one. They're not served.
    if !crate::caller_version(headers, None).as_deref().is_some_and(crate::relay_version_ok) {
        return Err(StatusCode::UPGRADE_REQUIRED);
    }
    Ok((fed, sender))
}

// ------------------------------------------------------------------ handlers

/// A peer asks: is this user connected here? Deliver if so (never queue).
pub async fn h_deliver(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Result<Json<serde_json::Value>, StatusCode> {
    let (_, sender) = check_from(&state, &headers, "POST", "/v1/fed/deliver", &body)?;
    let msg: RelayMessage = serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;
    // A8: the sender must be an account of the network we know (not banned,
    // not deleted, not a bot of another relay posing as a person), and a
    // relay can hand over only so many a minute. This narrows what a relay
    // can make up; it doesn't prove who wrote the envelope (that's the
    // end-to-end encryption's job on the devices).
    if !per_node_ok(&sender, "deliver", 6000) {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }
    let from = crate::base_user(&msg.from).to_string();
    let known = { let d = state.directory.clone(); tokio::task::spawn_blocking(move || d.may_send_from_peer(&from)).await.unwrap_or(false) };
    if !known {
        tracing::warn!(from = %msg.from, "federation: an envelope from someone not known here — refused");
        return Err(StatusCode::FORBIDDEN);
    }
    crate::journal::visitor(crate::base_user(&msg.from), "user");
    // A bot of this relay: its queue (a user of another relay wrote to it).
    let delivered = crate::deliver_to_bot(&state, &msg).await || crate::deliver_local(&state, &msg).await;
    Ok(Json(serde_json::json!({ "delivered": delivered })))
}

/// At most `per_min` calls of `what` a minute from one sender (a node id;
/// everyone with the secret only shares one count).
fn per_node_ok(sender: &Sender, what: &str, per_min: u32) -> bool {
    type Counts = std::collections::HashMap<String, (std::time::Instant, u32)>;
    static C: std::sync::OnceLock<std::sync::Mutex<Counts>> = std::sync::OnceLock::new();
    let key = match sender { Sender::Node(n) => format!("{what} {n}"), Sender::Secret => format!("{what} secret") };
    let mut g = C.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    if g.len() > 4096 { g.retain(|_, (t, _)| t.elapsed() < Duration::from_secs(60)); }
    let e = g.entry(key).or_insert((std::time::Instant::now(), 0));
    if e.0.elapsed() >= Duration::from_secs(60) { *e = (std::time::Instant::now(), 0); }
    e.1 += 1;
    e.1 <= per_min
}

/// A peer changed something: pull from the peers now instead of in ≤ 30 s.
pub async fn h_poke(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Result<StatusCode, StatusCode> {
    check(&state, &headers, "POST", "/v1/fed/poke", &body)?;
    let inbox = serde_json::from_slice::<serde_json::Value>(&body).ok().is_some_and(|v| v["inbox"].as_bool() == Some(true));
    if !inbox {
        state.fed_kick.notify_one();
        return Ok(StatusCode::NO_CONTENT);
    }
    // Only queued envelopes: pull them for the people connected here (one pull at a time).
    use std::sync::atomic::{AtomicBool, Ordering};
    static PULLING: AtomicBool = AtomicBool::new(false);
    if !PULLING.swap(true, Ordering::SeqCst) {
        if let Some(fed) = current(&state) {
            tokio::spawn(async move {
                pull_for_connected(&state, &fed).await;
                PULLING.store(false, Ordering::SeqCst);
            });
        } else {
            PULLING.store(false, Ordering::SeqCst);
        }
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn h_inbox(State(state): State<AppState>, headers: HeaderMap, Path(user): Path<String>) -> Result<Json<Vec<RelayMessage>>, StatusCode> {
    check(&state, &headers, "GET", &format!("/v1/fed/inbox/{user}"), b"")?;
    let mut out = Vec::new();
    for (id, bytes) in state.directory.inbox_take(&user) {
        if let Ok(m) = serde_json::from_slice::<RelayMessage>(&bytes) {
            out.push(m);
        }
        state.directory.inbox_ack(id);
    }
    Ok(Json(out))
}

/// Batch of `h_inbox`: what waits here for these users (connected to the caller).
pub async fn h_inboxes(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Result<Json<Vec<RelayMessage>>, StatusCode> {
    check(&state, &headers, "POST", "/v1/fed/inboxes", &body)?;
    let v: serde_json::Value = serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;
    let users = v["users"].as_array().ok_or(StatusCode::BAD_REQUEST)?;
    let mut out = Vec::new();
    for u in users.iter().filter_map(|x| x.as_str()).take(5000) {
        for (id, bytes) in state.directory.inbox_take(u) {
            if let Ok(m) = serde_json::from_slice::<RelayMessage>(&bytes) {
                out.push(m);
            }
            state.directory.inbox_ack(id);
        }
    }
    Ok(Json(out))
}

/// `h_inboxes` that forgets nothing yet: `[{id, m}]`, deleted by `h_inbox_ack`
/// once the caller has them (see `Fed::take_kept`).
pub async fn h_inboxes2(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Result<Json<Vec<serde_json::Value>>, StatusCode> {
    check(&state, &headers, "POST", "/v1/fed/inboxes2", &body)?;
    let v: serde_json::Value = serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;
    let users = v["users"].as_array().ok_or(StatusCode::BAD_REQUEST)?;
    let mut out = Vec::new();
    for u in users.iter().filter_map(|x| x.as_str()).take(5000) {
        for (id, bytes) in state.directory.inbox_take(u) {
            match serde_json::from_slice::<serde_json::Value>(&bytes) {
                Ok(m) => out.push(serde_json::json!({ "id": id, "m": m })),
                Err(_) => state.directory.inbox_ack(id),
            }
        }
    }
    Ok(Json(out))
}

/// The caller has these (`h_inboxes2`): forget them.
pub async fn h_inbox_ack(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Result<StatusCode, StatusCode> {
    check(&state, &headers, "POST", "/v1/fed/inbox_ack", &body)?;
    let v: serde_json::Value = serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;
    for id in v["ids"].as_array().ok_or(StatusCode::BAD_REQUEST)?.iter().filter_map(|x| x.as_i64()).take(100_000) {
        state.directory.inbox_ack(id);
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Something was queued here for a user who isn't connected here: ask the
/// peers to pull now (they take it for whoever is connected to them).
/// Bursts collapse into one poke a second.
pub fn queued_for_absent(state: &AppState) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static BUSY: AtomicBool = AtomicBool::new(false);
    let Some(fed) = current(state) else { return };
    if BUSY.swap(true, Ordering::SeqCst) { return; }
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(700)).await;
        BUSY.store(false, Ordering::SeqCst);
        fed.poke_inbox_all().await;
    });
}

/// Pull what peers queued for the people connected here, and hand it over.
async fn pull_for_connected(state: &AppState, fed: &Fed) {
    let users: Vec<String> = state.users.iter()
        .filter(|e| e.value().try_lock().map(|u| u.tx.iter().any(|t| !t.is_closed())).unwrap_or(true))
        .map(|e| e.key().clone())
        .collect();
    let got = fed.collect_inboxes(&users).await;
    if got.is_empty() { return; }
    tracing::info!(n = got.len(), "federation: pulled queued envelopes for people connected here");
    for m in got {
        if crate::deliver_local(state, &m).await { continue; }
        // Gone meanwhile: keep it here until they're back.
        if let Ok(bytes) = serde_json::to_vec(&m) { let _ = state.directory.inbox_push(&m.to, &bytes); }
    }
}

pub async fn h_bundle(State(state): State<AppState>, headers: HeaderMap, Path(user): Path<String>) -> Result<impl IntoResponse, StatusCode> {
    check(&state, &headers, "GET", &format!("/v1/fed/bundle/{user}"), b"")?;
    crate::local_bundle(&state, &user).await.map(Json).ok_or(StatusCode::NOT_FOUND)
}

pub async fn h_blob(State(state): State<AppState>, headers: HeaderMap, Path(cid): Path<String>) -> Result<impl IntoResponse, StatusCode> {
    check(&state, &headers, "GET", &format!("/v1/fed/blob/{cid}"), b"")?;
    let store = state.blobs.clone();
    let (file, len) = tokio::task::spawn_blocking(move || store.open_blob(&cid))
        .await.ok().flatten().ok_or(StatusCode::NOT_FOUND)?;
    Ok(crate::blob_upload::response(file, len))
}

#[derive(Deserialize)]
pub struct Since {
    #[serde(default)] since: i64,
    /// The key of the last row read at `since` (REV-001); older relays send none.
    #[serde(default)] after: Option<String>,
}

pub async fn h_users(State(state): State<AppState>, headers: HeaderMap, uri: axum::http::Uri, Query(q): Query<Since>) -> Result<impl IntoResponse, StatusCode> {
    check(&state, &headers, "GET", signed_path(&uri), b"")?;
    Ok(Json(state.directory.fed_users_after(q.since, q.after.as_deref())))
}

pub async fn h_backups(State(state): State<AppState>, headers: HeaderMap, uri: axum::http::Uri, Query(q): Query<Since>) -> Result<impl IntoResponse, StatusCode> {
    check(&state, &headers, "GET", signed_path(&uri), b"")?;
    Ok(Json(state.directory.fed_backups_after(q.since, q.after.as_deref())))
}

pub async fn h_backup(State(state): State<AppState>, headers: HeaderMap, Path(user): Path<String>) -> Result<impl IntoResponse, StatusCode> {
    check(&state, &headers, "GET", &format!("/v1/fed/backup/{user}"), b"")?;
    state.directory.get_backup(&user).ok_or(StatusCode::NOT_FOUND)
}

pub async fn h_media(State(state): State<AppState>, headers: HeaderMap, Path((user, kind)): Path<(String, String)>) -> Result<impl IntoResponse, StatusCode> {
    check(&state, &headers, "GET", &format!("/v1/fed/media/{user}/{kind}"), b"")?;
    let k = crate::directory::MediaKind::parse(&kind).ok_or(StatusCode::NOT_FOUND)?;
    let (mime, data) = state.directory.get_media(&user, k).ok_or(StatusCode::NOT_FOUND)?;
    Ok(([(axum::http::header::CONTENT_TYPE, mime)], data))
}

/// Navidrome of this relay for a relay of the network that has none: what
/// works on one server works on all (the owner). Read-only methods only.
pub async fn h_music_rest(State(state): State<AppState>, headers: HeaderMap, Path(method): Path<String>,
    axum::extract::RawQuery(q): axum::extract::RawQuery) -> Result<impl IntoResponse, StatusCode> {
    let path = match &q { Some(q) => format!("/v1/fed/music/rest/{method}?{q}"), None => format!("/v1/fed/music/rest/{method}") };
    check(&state, &headers, "GET", &path, b"")?;
    let v = crate::music::local_rest(&method, q.as_deref()).await.map_err(|_| StatusCode::NOT_FOUND)?;
    Ok(Json(v))
}

/// A cover / track of this relay's Navidrome for another relay (whole file).
pub async fn h_music_file(State(state): State<AppState>, headers: HeaderMap, Path((what, id)): Path<(String, String)>) -> Result<impl IntoResponse, StatusCode> {
    check(&state, &headers, "GET", &format!("/v1/fed/music/file/{what}/{id}"), b"")?;
    let (mime, data) = crate::music::local_file(&what, &id).await.ok_or(StatusCode::NOT_FOUND)?;
    Ok(([(axum::http::header::CONTENT_TYPE, mime)], data))
}

/// A public group / channel @name of this relay, for another relay of the
/// network (names are one namespace across it; lib.rs group_by_name).
pub async fn h_group_by_name(State(state): State<AppState>, headers: HeaderMap, Path(name): Path<String>) -> Result<impl IntoResponse, StatusCode> {
    check(&state, &headers, "GET", &format!("/v1/fed/groups/by-name/{name}"), b"")?;
    state.directory.group_by_name(&name).map(Json).ok_or(StatusCode::NOT_FOUND)
}

/// This relay's public groups / channels matching `q`, for the network's search.
pub async fn h_group_search(State(state): State<AppState>, headers: HeaderMap, axum::extract::RawQuery(q): axum::extract::RawQuery) -> Result<impl IntoResponse, StatusCode> {
    let path = match &q { Some(q) => format!("/v1/fed/groups/search?{q}"), None => "/v1/fed/groups/search".to_string() };
    check(&state, &headers, "GET", &path, b"")?;
    let query = q.as_deref().unwrap_or("").split('&').find_map(|kv| kv.strip_prefix("q=")).map(crate::music::pct).unwrap_or_default();
    Ok(Json(state.directory.search_groups(&query, 20)))
}

/// Ask every relay of the network (in parallel, a few seconds each) a JSON path.
impl Fed {
    pub async fn ask_all(&self, path: &str) -> Vec<serde_json::Value> {
        let asks = self.cfg.peers.iter().map(|p| async move {
            tokio::time::timeout(Duration::from_secs(5), self.music_json_at(p, path)).await.ok().flatten()
        });
        futures_util::future::join_all(asks).await.into_iter().flatten().collect()
    }
}

/// What this relay's Navidrome is called in the network (music.rs merges
/// every relay's into one): its tag and a name to show next to a track.
pub async fn h_music_info(State(state): State<AppState>, headers: HeaderMap) -> Result<impl IntoResponse, StatusCode> {
    check(&state, &headers, "GET", "/v1/fed/music/info", b"")?;
    crate::music::local_info().map(Json).ok_or(StatusCode::NOT_FOUND)
}

impl Fed {
    /// The relays of the network (their addresses), to ask each of them.
    pub fn peer_list(&self) -> Vec<String> {
        self.cfg.peers.clone()
    }

    /// One relay's answer to a music path.
    pub async fn music_json_at(&self, peer: &str, path: &str) -> Option<serde_json::Value> {
        self.get(peer, path).await?.json::<serde_json::Value>().await.ok()
    }

    /// A file from one relay (its Navidrome's track or cover).
    pub async fn fetch_bytes_at(&self, peer: &str, path: &str) -> Option<(String, Vec<u8>)> {
        let r = self.get(peer, path).await?;
        let mime = r.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("application/octet-stream").to_string();
        Some((mime, r.bytes().await.ok()?.to_vec()))
    }

}

#[derive(Deserialize)]
pub struct Hello {
    pub(crate) url: String,
    /// An invitation's admission token (REV2-004).
    #[serde(default)] admit: String,
    /// Relays of the network that admitted the sender.
    #[serde(default)] vouches: Vec<Vouch>,
    #[serde(default)] peers: Vec<String>,
    /// Its node key (hex), a signature by it over url and key, and after a
    /// key change the new key signed by the old one (1.21.0-23).
    #[serde(default)] node_key: String,
    #[serde(default)] node_sig: String,
    #[serde(default)] rotated: String,
    /// Its last key changes (1.21.0-25).
    #[serde(default)] rotations: Vec<RotStep>,
}

/// A relay of the network says hello: it's added to our peers if new, and
/// gets the list of relays we know (that's how they find each other).
/// History records changed after `since` (ms): `[[user, key, updated_at]]`.
pub async fn h_hist_list(State(state): State<AppState>, headers: HeaderMap, uri: axum::http::Uri, Query(q): Query<Since>) -> Result<impl IntoResponse, StatusCode> {
    check(&state, &headers, "GET", signed_path(&uri), b"")?;
    Ok(Json(state.directory.fed_hist_after(q.since, q.after.as_deref())))
}

/// `GET /v1/fed/tombs?since&after` — accounts deleted (A7): ours and those
/// handed on to us, each signed by the relay that deleted it.
pub async fn h_tombs(State(state): State<AppState>, headers: HeaderMap, uri: axum::http::Uri, Query(q): Query<Since>) -> Result<impl IntoResponse, StatusCode> {
    check(&state, &headers, "GET", signed_path(&uri), b"")?;
    Ok(Json(state.directory.tombs_after(q.since, q.after.as_deref(), 500)))
}

pub async fn h_hist_get(State(state): State<AppState>, headers: HeaderMap, Path((user, k)): Path<(String, String)>) -> Result<impl IntoResponse, StatusCode> {
    check(&state, &headers, "GET", &format!("/v1/fed/hist/{user}/{k}"), b"")?;
    state.directory.hist_get(&user, &k).ok_or(StatusCode::NOT_FOUND)
}

pub async fn h_hello(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Result<Json<serde_json::Value>, StatusCode> {
    let fed = check(&state, &headers, "POST", "/v1/fed/hello", &body)?;
    let h: Hello = serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;
    crate::journal::visitor(h.url.trim_end_matches('/'), "relay");
    // REV2-004: once every peer's key is known, a relay we don't know yet
    // joins only with an invitation's admission token, or vouched for by a
    // relay of the network — otherwise it waits for the admin.
    let strict = fed.enforced();
    let mut vouch = None;
    let known = fed.pin_for(&h.url).is_some();
    let (admitted, pinned) = decide_pin(&state, &fed, &h, false).await;
    if let Some(pin) = &pinned {
        if strict && !known { vouch = Some(fed.vouch_for(&pin.id, &pin.url)); }
        // It now knows... not yet our key: tell it, so it can sign too.
        if let Some(f) = current(&state) {
            let url = pin.url.clone();
            tokio::spawn(async move { f.hello_to(&url).await; });
        }
    }
    if !admitted {
        return Ok(Json(serde_json::json!({ "pending": true, "self": my_id(&fed.cfg) })));
    }
    let mut urls = vec![h.url];
    // Relays it names join only through their own hello once we're strict.
    if !strict { urls.extend(h.peers); }
    let st = state.clone();
    // Peers it knows join too — but only those that answer our signature.
    tokio::spawn(async move {
        let mut ok = Vec::new();
        for u in urls {
            if let Some(f) = current(&st) {
                if f.cfg.peers.iter().any(|p| p == u.trim_end_matches('/')) { continue; }
                if f.same_network(&u).await { ok.push(u); }
            }
        }
        if !add_peers(&st, &ok).is_empty() { st.fed_kick.notify_one(); }
    });
    let k = fed.hello_key_fields();
    Ok(Json(serde_json::json!({ "self": my_id(&fed.cfg), "peers": fed.cfg.peers, "vouch": vouch,
        "node_key": k["node_key"], "node_sig": k["node_sig"], "rotated": k["rotated"], "rotations": k["rotations"] })))
}

/// What to do with the key a hello (or names hello) brings. Returns whether
/// the sender may go on (`false`: it waits for the admin) and the pin kept.
///
/// * a first key: an invitation token or a vouch admits it; without them,
///   during the transition, only an address on our own list
///   (`names`: always, as before keys) — `tunnel:` ones never by themselves;
///   an `http` address must prove it holds the key (`proves_address`);
/// * a signed key change is followed; another key without one waits for the
///   admin as a conflict, unless it comes with a fresh invitation token (a
///   relay that lost its key, re-invited);
/// * a pinned key at a new address moves there once it proves it.
async fn decide_pin(state: &AppState, fed: &Fed, h: &Hello, names: bool) -> (bool, Option<Pin>) {
    let enforced = fed.enforced();
    let known = fed.pin_for(&h.url).is_some();
    let Some(ch) = pin_change(fed, h) else { return (!enforced || known, None) };
    let ch = match ch {
        PinChange::New(p) if names => PinChange::New(Pin { role: "names".into(), ..p }),
        other => other,
    };
    let token_ok = |st: &AppState| take_admit_token(st, &h.admit);
    match &ch {
        PinChange::Rotated(p, ..) => {
            let p = p.clone();
            let changed = apply_pin_change(state, &ch);
            (true, changed.then_some(p))
        }
        PinChange::Moved { pin, .. } => {
            if fed.proves_address(&pin.url, pin).await && apply_pin_change(state, &ch) {
                (true, Some(pin.clone()))
            } else {
                (!enforced, None)
            }
        }
        PinChange::Conflict(pin) => {
            // A fresh invitation token stands for the admin: the relay lost
            // its key and was invited again.
            if (!pin.url.starts_with("http") || fed.proves_address(&pin.url, pin).await) && token_ok(state) {
                tracing::warn!(url = %pin.url, "federation: a relay's key replaced with an invitation token (it lost the old one)");
                apply_pin_change(state, &ch);
                (true, Some(pin.clone()))
            } else {
                add_pending(state, pin.clone());
                (false, None)
            }
        }
        PinChange::New(pin) => {
            let on_list = if names { fed.cfg.solo_peers.contains(&pin.url) || !enforced } else { fed.cfg.peers.contains(&pin.url) };
            let ok = if pin.url.starts_with("http") {
                let by_us = !enforced && on_list;
                // The address first: a relay that can't prove it doesn't use up the token.
                fed.proves_address(&pin.url, pin).await && (by_us || fed.vouched(pin, &h.vouches) || token_ok(state))
            } else {
                token_ok(state) || fed.vouched(pin, &h.vouches)
            };
            if ok && apply_pin_change(state, &ch) {
                tracing::info!(url = %pin.url, names, "federation: a relay's key pinned");
                (true, Some(pin.clone()))
            } else if enforced {
                tracing::warn!(url = %pin.url, id = %pin.id, "federation: a relay that isn't admitted said hello — waiting for the admin");
                add_pending(state, pin.clone());
                (false, None)
            } else {
                (true, None)
            }
        }
    }
}

/// Keep vouches our hellos got back, and drop the invitation token once a
/// relay of the network has admitted us (REV2-004).
pub fn absorb_vouches(state: &AppState) {
    let Some(cur) = registry(state) else { return };
    // Both taken from this config before it is replaced below.
    let pins: Vec<Pin> = cur.got_pins.lock().map(|mut g| std::mem::take(&mut *g)).unwrap_or_default();
    let got: Vec<Vouch> = cur.got_vouches.lock().map(|mut g| std::mem::take(&mut *g)).unwrap_or_default();
    for pin in pins {
        if set_pin(state, pin) {
            tracing::info!("federation: a peer's key pinned from its hello answer");
        }
    }
    if got.is_empty() { return; }
    update_cfg(state, |_, c| {
        for v in got {
            if !c.vouches.iter().any(|x| x.by == v.by) && c.vouches.len() < 64 { c.vouches.push(v); }
        }
        c.join_token.clear();
        true
    });
}

/// Use up an admission token (REV2-004). True when it was one of ours and
/// still good.
fn take_admit_token(state: &AppState, token: &str) -> bool {
    let token = token.trim();
    if token.len() < 32 { return false; }
    let h = hex::encode(Sha256::digest(token.as_bytes()));
    let now = now_secs();
    let mut hit = false;
    // Under the lock: two hellos with the same token can't both use it.
    let saved = update_cfg(state, |_, c| {
        let before = c.admit_tokens.len();
        hit = c.admit_tokens.iter().any(|(t, exp)| subtle::ConstantTimeEq::ct_eq(t.as_bytes(), h.as_bytes()).into() && *exp > now);
        c.admit_tokens.retain(|(t, exp)| *t != h && *exp > now);
        hit || c.admit_tokens.len() != before
    });
    // Not saved: not used up either, so it isn't taken.
    hit && saved.is_some()
}

/// A new one-time admission token for an invitation (kept as a hash).
pub fn new_admit_token(state: &AppState) -> Option<String> {
    let mut b = [0u8; 32];
    rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut b);
    let token = hex::encode(b);
    let now = now_secs();
    update_cfg(state, |_, c| {
        c.admit_tokens.retain(|(_, exp)| *exp > now);
        c.admit_tokens.push((hex::encode(Sha256::digest(token.as_bytes())), now + ADMIT_TTL_SECS));
        true
    })?;
    Some(token)
}

fn add_pending(state: &AppState, pin: Pin) {
    update_cfg(state, |_, c| {
        if c.pending_nodes.iter().any(|p| p.id == pin.id && p.url == pin.url && p.conflict == pin.conflict) { return false; }
        // Full: the oldest waiting one goes (one flood of hellos kept the
        // list full and real relays out of it).
        c.pending_nodes.retain(|p| p.id != pin.id);
        while c.pending_nodes.len() >= MAX_PENDING_NODES { c.pending_nodes.remove(0); }
        c.pending_nodes.push(pin);
        true
    });
}

/// Admin: admit a waiting relay by its node id. True when it was waiting.
pub fn admit(state: &AppState, id: &str) -> bool {
    let mut admitted: Option<(String, bool)> = None;
    let next = update_cfg(state, |_, c| {
        let Some(pos) = c.pending_nodes.iter().position(|p| p.id == id) else { return false };
        let pin = c.pending_nodes.remove(pos);
        c.revoked.retain(|r| r != id);
        c.pins.retain(|p| p.url != pin.url);
        if pin.names_only() {
            if !c.solo_peers.contains(&pin.url) && c.solo_peers.len() < 256 { c.solo_peers.push(pin.url.clone()); }
        } else if !c.peers.contains(&pin.url) && c.peers.len() < 64 {
            c.peers.push(pin.url.clone());
        }
        admitted = Some((pin.url.clone(), pin.names_only()));
        c.pins.push(Pin { since: now_secs(), conflict: false, ..pin });
        true
    });
    match (next, admitted) {
        (Some(f), Some((url, false))) => { tokio::spawn(async move { f.hello_to(&url).await; }); true }
        (Some(_), Some(_)) => true,
        _ => false,
    }
}

/// What a hello's key means for our pins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PinChange {
    /// A relay whose key (and address) we don't keep yet.
    New(Pin),
    /// A pinned relay changed its key, each step signed by the one before;
    /// the ids it moved away from (to be retired) and their keys.
    Rotated(Pin, Vec<String>, Vec<String>),
    /// A pinned key at a new address (to be checked there before moving).
    Moved { from: String, pin: Pin },
    /// Another key for an address we keep a key for, not signed by it: for
    /// the admin (or a fresh invitation token) to decide.
    Conflict(Pin),
}

/// What the key a hello brings would change:
/// * it must be signed by itself over the sender's address (possession);
/// * a relay we know under another key moves only along key changes each
///   signed by the key before (`rotations`; `rotated` of 1.21.0-23/24 for
///   one step) — otherwise it's a conflict;
/// * revoked and retired node ids never come back.
fn pin_change(fed: &Fed, h: &Hello) -> Option<PinChange> {
    let url = h.url.trim().trim_end_matches('/').to_string();
    let key: [u8; 32] = hex::decode(h.node_key.trim()).ok()?.try_into().ok()?;
    let sig: [u8; 64] = hex::decode(h.node_sig.trim()).ok()?.try_into().ok()?;
    let pubk = aqua_core::crypto::SignPublic::from_bytes(key);
    pubk.verify(format!("aqua-fed-hello\n{url}\n{}", hex::encode(key)).as_bytes(), &sig).ok()?;
    let id = node_id_of(&key);
    if fed.is_revoked(&id) {
        tracing::warn!(%url, %id, "federation: hello from a revoked or retired node key — ignored");
        return None;
    }
    let pin = Pin { url: url.clone(), id: id.clone(), key: hex::encode(key), since: now_secs(), ..Default::default() };
    match fed.pin_for(&url) {
        Some(old) if old.key == pin.key => None,
        Some(old) => match follow_rotations(&url, old, &pin.key, &h.rotations, &h.rotated) {
            Some((seq, retired, keys)) => {
                tracing::info!(%url, seq, "federation: a peer changed its node key (signed by the old one)");
                Some(PinChange::Rotated(Pin { role: old.role.clone(), rot_seq: seq, ..pin }, retired, keys))
            }
            None => {
                tracing::warn!(%url, "federation: a hello with ANOTHER node key and no signature by the known one — for the admin");
                Some(PinChange::Conflict(Pin { role: old.role.clone(), conflict: true, ..pin }))
            }
        },
        None => match fed.cfg.pins.iter().find(|p| p.id == id && p.url != url) {
            Some(other) => Some(PinChange::Moved { from: other.url.clone(), pin: Pin { role: other.role.clone(), rot_seq: other.rot_seq, ..pin } }),
            None => Some(PinChange::New(pin)),
        },
    }
}

/// From the key we keep (`old`) to `new` along signed key changes: the
/// last step's number and the ids passed through. Steps must go up from the
/// number we followed last; a chain may be at most [`MAX_ROTATIONS`] long.
fn follow_rotations(url: &str, old: &Pin, new: &str, steps: &[RotStep], legacy: &str) -> Option<(u64, Vec<String>, Vec<String>)> {
    let verify = |by: &str, msg: &str, sig: &str| -> bool {
        let (Some(k), Some(s)) = (hex::decode(by).ok().and_then(|b| <[u8; 32]>::try_from(b).ok()), hex::decode(sig.trim()).ok().and_then(|b| <[u8; 64]>::try_from(b).ok())) else { return false };
        aqua_core::crypto::SignPublic::from_bytes(k).verify(msg.as_bytes(), &s).is_ok()
    };
    let mut at = old.key.clone();
    let mut seq = old.rot_seq;
    let mut passed = Vec::new();
    let mut keys = Vec::new();
    for st in steps.iter().take(MAX_ROTATIONS) {
        if st.old != at || st.seq <= seq {
            continue;
        }
        let Some(old_id) = hex::decode(&at).ok().and_then(|b| <[u8; 32]>::try_from(b).ok()).map(|b| node_id_of(&b)) else { return None };
        if !verify(&at, &rot_message(url, &old_id, &st.new, st.seq), &st.sig) {
            return None;
        }
        passed.push(old_id);
        keys.push(at.clone());
        at = st.new.clone();
        seq = st.seq;
        if at == new {
            return Some((seq, passed, keys));
        }
    }
    // A relay of 1.21.0-23/24: one step, `aqua-fed-rotate\n{new}` by the old key.
    if old.rot_seq == 0 && steps.is_empty() && verify(&old.key, &format!("aqua-fed-rotate\n{new}"), legacy) {
        return Some((0, vec![old.id.clone()], vec![old.key.clone()]));
    }
    None
}

impl Fed {
    /// Does the relay at `url` hold `pin`'s key? A ping there, signed for
    /// that node id; the answer must be signed by that key over our request
    /// (`sign_answers`). Whoever only has the secret can't answer for an
    /// address that isn't theirs.
    pub(crate) async fn proves_address(&self, url: &str, pin: &Pin) -> bool {
        if !url.starts_with("http") {
            return false;
        }
        let Some(key) = hex::decode(&pin.key).ok().and_then(|b| <[u8; 32]>::try_from(b).ok()) else { return false };
        // A solo relay serves only the name registry.
        let path = if pin.names_only() { "/v1/fed/names?since=0" } else { "/v1/fed/ping" };
        let mut n = [0u8; 12];
        rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut n);
        let (nonce, ts, from) = (hex::encode(n), now_ms(), self.node_id());
        let sig = self.key.sign(v2_message("GET", path, ts, &nonce, &from, &pin.id, b"").as_bytes());
        let h2 = format!("{ts}.{nonce}.{from}.{}.{}", pin.id, hex::encode(sig));
        let Ok(r) = self.http.get(format!("{url}{path}"))
            .header("x-aqua-fed", self.header("GET", path, b""))
            .header("x-aqua-fed2", h2.clone())
            .timeout(Duration::from_secs(6))
            .send().await else { return false };
        let status = r.status();
        let sig = r.headers().get("x-aqua-fed2-resp").and_then(|v| v.to_str().ok()).and_then(|s| hex::decode(s).ok()).and_then(|b| <[u8; 64]>::try_from(b).ok());
        let Ok(body) = r.bytes().await else { return false };
        status.is_success() && sig.is_some_and(|s| aqua_core::crypto::SignPublic::from_bytes(key).verify(resp_message(&h2, status.as_u16(), &body).as_bytes(), &s).is_ok())
    }
}

/// Apply what a hello's key changes, once decided it may (`admit` paths in
/// `h_hello` / `h_names_hello`). True when the pins changed.
fn apply_pin_change(state: &AppState, ch: &PinChange) -> bool {
    match ch {
        PinChange::New(p) | PinChange::Conflict(p) => set_pin(state, Pin { conflict: false, ..p.clone() }),
        PinChange::Rotated(p, retired, keys) => update_cfg(state, |_, c| {
            c.pins.retain(|x| x.url != p.url);
            c.pins.push(p.clone());
            let now = now_secs();
            for (r, k) in retired.iter().zip(keys) {
                if !c.retired.contains(r) { c.retired.push(r.clone()); }
                if !c.retired_keys.iter().any(|x| x.0 == *r) { c.retired_keys.push((r.clone(), k.clone(), now)); }
            }
            let extra = c.retired.len().saturating_sub(256);
            c.retired.drain(..extra);
            let extra = c.retired_keys.len().saturating_sub(256);
            c.retired_keys.drain(..extra);
            true
        }).is_some(),
        PinChange::Moved { from, pin } => {
            let moved = update_cfg(state, |_, c| {
                c.pins.retain(|x| x.url != *from && x.url != pin.url);
                c.pins.push(pin.clone());
                for list in [&mut c.peers, &mut c.solo_peers] {
                    if let Some(i) = list.iter().position(|u| u == from) {
                        if list.contains(&pin.url) { list.remove(i); } else { list[i] = pin.url.clone(); }
                    }
                }
                true
            }).is_some();
            if moved {
                tracing::info!(%from, to = %pin.url, "federation: a peer moved to a new address (same key, checked there)");
                state.directory.fed_cursors_move(from, &pin.url);
            }
            moved
        }
    }
}

/// Keep `pin` (replacing the one for its address). True when it changed.
pub fn set_pin(state: &AppState, pin: Pin) -> bool {
    update_cfg(state, |_, c| {
        if c.pins.iter().any(|p| p.same(&pin)) { return false; }
        c.pins.retain(|p| p.url != pin.url);
        c.pins.push(pin);
        true
    }).is_some()
}

/// Put `new` in as the node key of `c` (now `cur`'s): the old key signs the
/// change — for peers of 1.21.0-23/24 (`rotated`, one step) and as the next
/// step of the chain for the rest.
fn rotate_into(cur: &Fed, c: &mut FedConfig, new: &aqua_core::crypto::SignSecret) {
    let new_pub = hex::encode(new.public().as_bytes());
    c.rotated = hex::encode(cur.key.sign(format!("aqua-fed-rotate\n{new_pub}").as_bytes()));
    let seq = c.rotations.last().map(|r| r.seq).unwrap_or(0) + 1;
    let url = my_id(c);
    let sig = hex::encode(cur.key.sign(rot_message(&url, &cur.node_id(), &new_pub, seq).as_bytes()));
    c.rotations.push(RotStep { old: cur.node_pub(), new: new_pub, seq, sig });
    let extra = c.rotations.len().saturating_sub(MAX_ROTATIONS);
    c.rotations.drain(..extra);
    c.node_key = hex::encode(new.to_bytes());
}

/// Admin: a new node key. The old one signs the new one; hellos carry that,
/// so peers move their pins. Returns the new node id.
pub async fn rotate_key(state: &AppState) -> Option<String> {
    let new = aqua_core::crypto::SignSecret::generate();
    let next = update_cfg(state, |cur, c| { rotate_into(cur, c, &new); true })?;
    let id = next.node_id();
    for p in next.cfg.peers.clone() { next.hello_to(&p).await; }
    Some(id)
}

/// What the admin panel shows about the key requirement.
pub fn auth_status(fed: &Fed) -> serde_json::Value {
    let c = &fed.cfg;
    let now = now_secs();
    let unpinned: Vec<&String> = c.peers.iter().filter(|u| fed.pin_for(u).is_none()).collect();
    let warn = c.auth_mode == "transition" && c.auth_deadline > now && c.auth_deadline - now <= AUTH_WARN_DAYS * DAY && !unpinned.is_empty();
    serde_json::json!({
        "mode": if fed.enforced() { "enforced" } else if c.auth_mode == "enforced" { "legacy" } else { "transition" },
        "deadline": c.auth_deadline,
        "enforced_at": c.enforced_at,
        "legacy_until": c.legacy_until,
        "env_enforced": env_enforced(),
        "unpinned": unpinned,
        "warn": warn,
        "conflicts": c.pending_nodes.iter().filter(|p| p.conflict).map(|p| serde_json::json!({ "url": p.url, "id": p.id })).collect::<Vec<_>>(),
        "legacy_max_days": LEGACY_MAX_DAYS,
    })
}

/// Admin (`POST /v1/admin/fed/auth`): `enforce` — keys required now;
/// `postpone {days ≤ 14}` — a later end of the transition (only during it);
/// `legacy {days ≤ 7, confirm: true}` — let relays without keys in again for
/// a while (ends by itself); `legacy_end`. Never anything that stays lax.
pub fn set_auth(state: &AppState, action: &str, days: u64, confirm: bool) -> Result<serde_json::Value, &'static str> {
    registry(state).ok_or("not in a network")?;
    let now = now_secs();
    let mut err = None;
    let next = update_cfg(state, |_, c| {
    c.settle(now);
    let r: Result<(), &'static str> = (|| { match action {
        "enforce" => {
            if c.auth_mode != "enforced" {
                c.auth_mode = "enforced".into();
                c.enforced_at = now;
            }
        }
        "postpone" => {
            if c.auth_mode != "transition" || env_enforced() { return Err("keys are already required"); }
            if !(1..=AUTH_TRANSITION_DAYS).contains(&days) { return Err("1 to 14 days"); }
            // At most 14 days from now, whatever was set before.
            c.auth_deadline = (c.auth_deadline.max(now) + days * DAY).min(now + AUTH_TRANSITION_DAYS * DAY);
        }
        "legacy" => {
            if !confirm { return Err("confirm: true is required"); }
            if env_enforced() { return Err("AQUA_FED_AUTH=enforced is set"); }
            if c.auth_mode != "enforced" { return Err("keys aren't required yet"); }
            if !(1..=LEGACY_MAX_DAYS).contains(&days) { return Err("1 to 7 days"); }
            c.legacy_until = now + days * DAY;
        }
        "legacy_end" => c.legacy_until = 0,
        _ => return Err("unknown action"),
    } Ok(()) })();
    match r { Ok(()) => true, Err(e) => { err = Some(e); false } }
    });
    if let Some(e) = err { return Err(e); }
    let next = next.ok_or("can't save")?;
    Ok(auth_status(&next))
}

/// Admin: never accept this node id again (and drop its pin and address).
pub fn revoke(state: &AppState, id: &str) -> bool {
    let mut urls: Vec<String> = Vec::new();
    let done = update_cfg(state, |_, c| {
        urls = c.pins.iter().filter(|p| p.id == id).map(|p| p.url.clone()).collect();
        c.pins.retain(|p| p.id != id);
        c.peers.retain(|p| !urls.contains(p));
        c.solo_peers.retain(|p| !urls.contains(p));
        c.pending_nodes.retain(|p| p.id != id);
        if !c.revoked.iter().any(|r| r == id) { c.revoked.push(id.to_string()); }
        true
    }).is_some();
    // Where sync was with it is of no use any more.
    for u in &urls {
        state.directory.fed_cursors_drop(Some(u), None);
    }
    done
}

/// "Do you accept our signature?" — 204 for relays of the same network.
pub async fn h_ping(State(state): State<AppState>, headers: HeaderMap) -> Result<StatusCode, StatusCode> {
    check(&state, &headers, "GET", "/v1/fed/ping", b"")?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct MailReq { to: String, code: String, #[serde(default)] link: Option<String> }

/// A relay of the network without mail asks us to send a sign-up code.
/// Only to relays that share the secret; a few per address per 10 minutes.
pub async fn h_mail(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Result<StatusCode, StatusCode> {
    check(&state, &headers, "POST", "/v1/fed/mail", &body)?;
    let m: MailReq = serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;
    if m.code.len() > 12 || !m.to.contains('@') || m.link.as_ref().is_some_and(|l| !l.starts_with("http") || l.len() > 300) {
        return Err(StatusCode::BAD_REQUEST);
    }
    {
        static RECENT: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, Vec<i64>>>> = std::sync::OnceLock::new();
        let mut g = RECENT.get_or_init(Default::default).lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let now = now_ms();
        let list = g.entry(m.to.to_lowercase()).or_default();
        list.retain(|t| now - t < 10 * 60 * 1000);
        if list.len() >= 5 {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
        list.push(now);
    }
    match state.directory.send_code_mail(&m.to, &m.code, m.link.as_deref()).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            tracing::warn!(error = %e, "relaying a sign-up e-mail for a peer failed");
            Err(StatusCode::SERVICE_UNAVAILABLE)
        }
    }
}

/// Public: where else clients can connect.
pub async fn h_peers(State(state): State<AppState>) -> Json<serde_json::Value> {
    let cfg = current(&state).map(|f| f.cfg.clone());
    Json(serde_json::json!({
        "federated": cfg.is_some(),
        "self": cfg.as_ref().map(|c| c.public_url.clone()).filter(|u| !u.is_empty()).unwrap_or_else(crate::public::url),
        // Clients connect by address: relays known only by a reverse link aren't listed.
        "servers": cfg.map(|c| c.peers.into_iter().filter(|p| p.starts_with("http")).collect::<Vec<_>>()).unwrap_or_default(),
    }))
}

// ------------------------------------------------------------------ sync loop

/// Pull accounts, profiles and backups from every peer — right away when a
/// peer pokes us, and every 30 seconds anyway. A second task watches the
/// local database and pokes the peers whenever something here changed.
pub fn spawn_sync(state: AppState) {
    let st = state.clone();
    tokio::spawn(async move {
        loop {
            sync_once(&st).await;
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(30)) => {}
                _ = st.fed_kick.notified() => {
                    // Let a burst of changes on the peer settle.
                    tokio::time::sleep(Duration::from_millis(300)).await;
                }
            }
        }
    });
    crate::fedtunnel::spawn(state.clone());
    spawn_announcer(state);
}

/// Poke the peers whenever an account, profile or backup changed here.
pub fn spawn_announcer(state: AppState) {
    // Every few minutes: hello to the peers, so relays of the network find
    // each other (and a relay that was down learns who joined meanwhile).
    let st = state.clone();
    tokio::spawn(async move {
        loop {
            if let Some(fed) = current(&st) {
                let mut new = Vec::new();
                for u in fed.hello_all().await {
                    if fed.same_network(&u).await { new.push(u); }
                }
                absorb_vouches(&st);
                if !add_peers(&st, &new).is_empty() { st.fed_kick.notify_one(); }
            } else if let Some(fed) = registry(&st) {
                let new = names_hello(&fed).await;
                absorb_vouches(&st);
                if !add_peers(&st, &new).is_empty() { st.fed_kick.notify_one(); }
            }
            tokio::time::sleep(Duration::from_secs(180)).await;
        }
    });
    tokio::spawn(async move {
        let mut last: Option<(i64, i64)> = None;
        loop {
            let dir = state.directory.clone();
            let mark = tokio::task::spawn_blocking(move || dir.change_mark()).await.unwrap_or_default();
            if last != Some(mark) {
                // First round too: peers learn what changed while we were down.
                if let Some(fed) = current(&state) {
                    fed.poke_all().await;
                }
                last = Some(mark);
            }
            tokio::time::sleep(Duration::from_millis(1500)).await;
        }
    });
}

/// Where sync is in each peer's streams: kept in the database
/// (`fed_cursors`, A6), so a restart goes on where it was instead of pulling
/// everything again. A cursor moves only past rows that were taken in full
/// (written, their media fetched); on any error it stays, and the row comes
/// again next time.
fn cursor(state: &AppState, peer: &str, stream: &str) -> Cursor {
    state.directory.fed_cursor(peer, stream).unwrap_or_default()
}

/// Keep a cursor. False when it couldn't be written: the caller stops (it
/// would go on from a place not on record).
fn keep_cursor(state: &AppState, peer: &str, stream: &str, c: &Cursor) -> bool {
    let ok = state.directory.set_fed_cursor(peer, stream, c.0, &c.1);
    if !ok {
        tracing::error!(%peer, %stream, "federation: where sync is can't be saved — stopping this round");
    }
    ok
}

/// Admin (`POST /v1/admin/fed/resync`): read a peer's streams (or all) from
/// the start again — after restoring the database from a backup, say.
pub fn resync(state: &AppState, peer: Option<&str>, stream: Option<&str>) {
    state.directory.fed_cursors_drop(peer, stream);
    state.fed_kick.notify_one();
}

/// Media of one account that failed to come this many times in a row is
/// skipped (it may be gone on the peer), so one row can't stop sync for good.
const MEDIA_TRIES: u32 = 3;

fn media_failed(key: &str) -> u32 {
    static F: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, u32>>> = std::sync::OnceLock::new();
    let mut g = F.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    if g.len() > 10_000 { g.clear(); }
    let n = g.entry(key.to_string()).or_default();
    *n += 1;
    *n
}

pub async fn sync_once(state: &AppState) {
    let Some(fed) = registry(state) else { return };
    if fed.cfg.active() {
        // Messages first: they're what people wait for.
        pull_for_connected(state, &fed).await;
        for peer in fed.cfg.peers.clone() {
            // Deletions first: an account deleted there isn't taken again below.
            sync_tombs(state, &fed, &peer).await;
            sync_peer(state, &fed, &peer).await;
            sync_hist(state, &fed, &peer).await;
        }
        // Solo relays: only their names.
        for peer in fed.cfg.solo_peers.clone() {
            sync_names(state, &fed, &peer).await;
        }
    } else {
        // A solo relay in the name registry: the network's names, nothing else.
        for peer in fed.cfg.peers.clone() {
            sync_names(state, &fed, &peer).await;
        }
    }
}

/// Encrypted chat history of accounts (client history_sync.rs): a device may
/// talk to any relay of the network and still find it.
async fn sync_hist(state: &AppState, fed: &Fed, peer: &str) {
    loop {
        let from = cursor(state, peer, "hist");
        let Some(r) = fed.get(peer, &format!("/v1/fed/hist?{}", cursor_query(&from))).await else { return };
        let Ok(rows) = r.json::<Vec<(String, String, i64)>>().await else { return };
        if rows.is_empty() { return; }
        let n = rows.len();
        // The cursor moves row by row, and stops before a record we couldn't
        // fetch or write, so the next sync tries it again (AQUA-SEC-010).
        let mut cur = from.clone();
        for (user, k, at) in &rows {
            if crate::directory::Directory::valid_name(user) && k.len() <= 64 {
                let got = match fed.get(peer, &format!("/v1/fed/hist/{user}/{k}")).await {
                    Some(b) => match b.bytes().await {
                        Ok(bytes) => state.directory.fed_apply_hist(user, k, &bytes, *at).is_ok(),
                        Err(_) => false,
                    },
                    None => false,
                };
                if !got {
                    if cur != from { keep_cursor(state, peer, "hist", &cur); }
                    return;
                }
            }
            cur = (*at, format!("{user}/{k}"));
        }
        if !keep_cursor(state, peer, "hist", &cur) { return; }
        if n < 1000 { return; }
    }
}

/// Most deletions signed by one relay taken in an hour by themselves; more
/// wait (an admin deleting a lot at once is delayed, a relay that turned
/// bad can't empty the network in one go).
pub const TOMBS_PER_HOUR: usize = 20;

/// Deleted accounts (A7), from `peer`: each signed by the relay that
/// deleted it, whose key we keep.
async fn sync_tombs(state: &AppState, fed: &Fed, peer: &str) {
    for _ in 0..20 {
        let from = cursor(state, peer, "tombs");
        let Some(r) = fed.get(peer, &format!("/v1/fed/tombs?{}", cursor_query(&from))).await else { return };
        // Relays before 1.21.0-25 don't have the route: nothing to read.
        let Ok(rows) = r.json::<Vec<crate::directory::Tomb>>().await else { return };
        if rows.is_empty() { return; }
        let n = rows.len();
        let mut cur = from.clone();
        for t in &rows {
            let key = (t.got_at, format!("{}:{}", t.uid, t.created_at));
            let mine = t.origin == fed.node_id();
            if !mine && fed.verify_tomb(t) {
                if state.directory.tombs_from_since(&t.origin, now_ms() - 3_600_000) >= TOMBS_PER_HOUR {
                    tracing::warn!(origin = %t.origin, "federation: many account deletions from one relay — the rest wait");
                    if cur != from { keep_cursor(state, peer, "tombs", &cur); }
                    return;
                }
                match state.directory.apply_tomb(t) {
                    Ok(Some(name)) => {
                        tracing::warn!(user = %name, origin = %t.origin, "federation: an account deleted on its relay — deleted here too");
                        crate::journal::log(&name, "deleted", format!("by relay {}", t.origin));
                        crate::drop_connections_pub(state, &name).await;
                    }
                    Ok(None) => {}
                    Err(_) => {
                        if cur != from { keep_cursor(state, peer, "tombs", &cur); }
                        return;
                    }
                }
            } else if !mine {
                tracing::warn!(origin = %t.origin, "federation: a deletion not signed by a relay we keep a key for — not taken");
            }
            cur = key;
        }
        if !keep_cursor(state, peer, "tombs", &cur) { return; }
        if n < 500 { return; }
    }
}

async fn sync_peer(state: &AppState, fed: &Fed, peer: &str) {
    // A8: accounts only from relays of the network (never a names-only pin).
    if fed.pin_for(peer).is_some_and(Pin::names_only) {
        return;
    }
    // Accounts (paged: 500 per call).
    'pages: loop {
        let since = cursor(state, peer, "users");
        let Some(r) = fed.get(peer, &format!("/v1/fed/users?{}", cursor_query(&since))).await else { return };
        let Ok(rows) = r.json::<Vec<crate::directory::FedUser>>().await else { return };
        if rows.is_empty() {
            break;
        }
        let mut applied = 0;
        let mut cur = since.clone();
        let mut stopped = false;
        for u in &rows {
            // Profile media travels with the account, so it is here even if
            // the other relay goes away. Fetched before the account is
            // written: written first, a failed fetch would never be retried
            // (the row would be ours already, the versions equal).
            let before = state.directory.media_versions(&u.username).unwrap_or((0, 0, 0));
            let mut media = Vec::new();
            let mut media_ok = true;
            for (kind, old, new) in [("avatar", before.0, u.avatar_ver), ("banner", before.1, u.banner_ver), ("music", before.2, u.music_ver)] {
                if new == old || new == 0 {
                    continue;
                }
                let Some(k) = crate::directory::MediaKind::parse(kind) else { continue };
                let got = match fed.get(peer, &format!("/v1/fed/media/{}/{kind}", u.username)).await {
                    Some(r) => {
                        let mime = r.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("application/octet-stream").to_string();
                        r.bytes().await.ok().map(|b| (k, mime, b))
                    }
                    None => None,
                };
                match got {
                    Some(m) => media.push(m),
                    // Gone on the peer, maybe: after a few tries, without it.
                    None if media_failed(&format!("{peer} {} {kind} {new}", u.username)) >= MEDIA_TRIES => {}
                    None => media_ok = false,
                }
            }
            if !media_ok {
                stopped = true;
                break;
            }
            let res = state.directory.fed_apply_user_from(u, fed.pin_for(peer).map(|p| p.id.as_str()).unwrap_or(""));
            if res.failed {
                // Not written here: the cursor stays before it.
                tracing::warn!(%peer, user = %u.username, "federation: an account couldn't be written — will try again");
                stopped = true;
                break;
            }
            if let Some((old, new)) = &res.displaced {
                crate::move_live(state, old, new);
            }
            if res.applied {
                applied += 1;
                if !media.iter().all(|(k, mime, b)| state.directory.fed_store_media(&res.name, *k, mime, b)) {
                    // The row is in, its media not: read the row again next
                    // round with our media versions put back.
                    state.directory.fed_media_unset(&res.name, before);
                    stopped = true;
                    break;
                }
            }
            cur = (u.updated_at, u.username.clone());
        }
        if cur != since && !keep_cursor(state, peer, "users", &cur) {
            return;
        }
        if applied > 0 {
            tracing::info!(peer, applied, "federation: accounts updated");
        }
        if stopped || rows.len() < 500 {
            break 'pages;
        }
    }
    // Encrypted backups.
    // Paged like the rest (200 per call: only the first page was read), and
    // the cursor moves only past backups we have (AQUA-SEC-010).
    for _ in 0..50 {
        let since = cursor(state, peer, "backups");
        let Some(r) = fed.get(peer, &format!("/v1/fed/backups?{}", cursor_query(&since))).await else { return };
        let Ok(list) = r.json::<Vec<(String, i64)>>().await else { return };
        let n = list.len();
        let mut last = since.clone();
        for (user, at) in list {
            let have = state.directory.backup_meta(&user).is_some_and(|(_, ours)| ours >= at);
            let ok = have || match fed.get(peer, &format!("/v1/fed/backup/{user}")).await {
                Some(b) => match b.bytes().await {
                    Ok(bytes) => state.directory.fed_apply_backup(&user, &bytes, at).is_ok(),
                    Err(_) => false,
                },
                None => false,
            };
            if !ok {
                if last != since { keep_cursor(state, peer, "backups", &last); }
                return;
            }
            last = (at, user);
        }
        let moved = last != since;
        if moved && !keep_cursor(state, peer, "backups", &last) { return; }
        if n < 200 || !moved { return; }
    }
}

/// All relay-to-relay routes: mounted in the app, and run in-process for
/// requests that come over a reverse link (fedtunnel.rs).
pub fn routes() -> axum::Router<AppState> {
    use axum::routing::{get, post};
    axum::Router::new()
        .route("/v1/fed/names", get(h_names))
        .route("/v1/fed/names/hello", post(h_names_hello))
        .route("/v1/fed/hist", get(h_hist_list))
        .route("/v1/fed/hist/:user/:k", get(h_hist_get))
        .route("/v1/fed/tombs", get(h_tombs))
        .route("/v1/fed/peers", get(h_peers))
        .route("/v1/fed/hello", post(h_hello))
        .route("/v1/fed/ping", get(h_ping))
        .route("/v1/fed/mail", post(h_mail))
        .route("/v1/fed/deliver", post(h_deliver))
        .route("/v1/fed/poke", post(h_poke))
        .route("/v1/fed/inbox/:user", get(h_inbox))
        .route("/v1/fed/inboxes", post(h_inboxes))
        .route("/v1/fed/inboxes2", post(h_inboxes2))
        .route("/v1/fed/inbox_ack", post(h_inbox_ack))
        .route("/v1/fed/bundle/:user", get(h_bundle))
        .route("/v1/fed/blob/:cid", get(h_blob))
        .route("/v1/fed/users", get(h_users))
        .route("/v1/fed/backups", get(h_backups))
        .route("/v1/fed/backup/:user", get(h_backup))
        .route("/v1/fed/media/:user/:kind", get(h_media))
        .route("/v1/fed/music/info", get(h_music_info))
        .route("/v1/fed/groups/by-name/:name", get(h_group_by_name))
        .route("/v1/fed/groups/search", get(h_group_search))
        .route("/v1/fed/music/rest/:method", get(h_music_rest))
        .route("/v1/fed/music/file/:what/:id", get(h_music_file))
        .route("/v1/fed/tunnel", post(crate::fedtunnel::h_poll))
        .route("/v1/fed/tunnel/reply", post(crate::fedtunnel::h_reply)
            .layer(axum::extract::DefaultBodyLimit::max(96 * 1024 * 1024)))
}

// ---------- bots of other relays (owner: every bot works on every server) ----

/// `GET /v1/fed/bot/:name` — a bot of this relay, for the network: its
/// profile, "/" commands and menu button. 404 if it isn't one here.
pub async fn h_bot(State(state): State<AppState>, headers: HeaderMap, Path(name): Path<String>) -> Result<Json<serde_json::Value>, StatusCode> {
    let name = name.trim().trim_start_matches('@').to_lowercase();
    check(&state, &headers, "GET", &format!("/v1/fed/bot/{name}"), b"")?;
    if !state.directory.is_bot(&name) {
        return Err(StatusCode::NOT_FOUND);
    }
    let profile = state.directory.get_profile(&name).ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(serde_json::json!({
        "profile": profile,
        "commands": state.tg.commands.get(&name).map(|c| c.clone()).unwrap_or(serde_json::json!([])),
        "menu": crate::tgapi::menu_button(&state, &name),
    })))
}

/// `POST /v1/fed/webapp/init {bot, user, start_param}` — initData for one of
/// this relay's bots, for a user of another relay of the network (that relay
/// checked who they are; the request is signed for the network).
pub async fn h_webapp_init(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Result<Json<serde_json::Value>, StatusCode> {
    let (_, sender) = check_from(&state, &headers, "POST", "/v1/fed/webapp/init", &body)?;
    let v: serde_json::Value = serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;
    let bot = v["bot"].as_str().unwrap_or("").trim().trim_start_matches('@').to_lowercase();
    let user = v["user"].as_str().unwrap_or("").trim().to_lowercase();
    if bot.is_empty() || user.is_empty() || !state.directory.is_bot(&bot) {
        return Err(StatusCode::NOT_FOUND);
    }
    // A8: initData only for an account of the network we know, a few a
    // minute per relay (a relay named any user and got data signed for them).
    if !per_node_ok(&sender, "webapp", 120) {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }
    if !state.directory.may_send_from_peer(crate::base_user(&user)) {
        return Err(StatusCode::FORBIDDEN);
    }
    let sp = v["start_param"].as_str().unwrap_or("");
    let (init_data, signed) = crate::tgapi::init_data(&state, &bot, &user, Some(sp));
    Ok(Json(serde_json::json!({ "init_data": init_data, "signed": signed })))
}

type BotCache = std::collections::HashMap<String, (std::time::Instant, Option<(String, serde_json::Value)>)>;
static BOTS: std::sync::Mutex<Option<BotCache>> = std::sync::Mutex::new(None);

/// A bot that runs on another relay of the network: (that relay, its info).
/// Asked of every peer at once; remembered 5 minutes (a miss — 1 minute).
async fn find_bot(state: &AppState, name: &str) -> Option<(String, serde_json::Value)> {
    let name = name.trim().trim_start_matches('@').to_lowercase();
    if name.is_empty() || name.len() > 64 || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return None;
    }
    if let Some((at, hit)) = BOTS.lock().ok().and_then(|g| g.as_ref().and_then(|m| m.get(&name).cloned())) {
        let ttl = if hit.is_some() { 300 } else { 60 };
        if at.elapsed() < Duration::from_secs(ttl) {
            return hit;
        }
    }
    let f = current(state)?;
    let path = format!("/v1/fed/bot/{name}");
    let asks = f.peer_list().into_iter().map(|p| {
        let f = f.clone();
        let path = path.clone();
        async move {
            let v = tokio::time::timeout(Duration::from_secs(6), f.music_json_at(&p, &path)).await.ok().flatten()?;
            v.get("profile").is_some_and(|x| x.is_object()).then_some((p, v))
        }
    });
    let hit = futures_util::future::join_all(asks).await.into_iter().flatten().next();
    if let Ok(mut g) = BOTS.lock() {
        let m = g.get_or_insert_with(std::collections::HashMap::new);
        if m.len() > 2000 { m.clear(); }
        m.insert(name, (std::time::Instant::now(), hit.clone()));
    }
    hit
}

/// The info of a bot of another relay (`{profile, commands, menu}`).
pub async fn remote_bot(state: &AppState, name: &str) -> Option<serde_json::Value> {
    find_bot(state, name).await.map(|(_, v)| v)
}

/// initData for a bot of another relay, signed there, for `user` of this one.
pub async fn remote_webapp_init(state: &AppState, bot: &str, user: &str, start_param: &str) -> Option<serde_json::Value> {
    let (peer, _) = find_bot(state, bot).await?;
    let f = current(state)?;
    let body = serde_json::json!({ "bot": bot, "user": user, "start_param": start_param }).to_string().into_bytes();
    let (st, text) = f.post_answer(&peer, "/v1/fed/webapp/init", body).await?;
    if !(200..300).contains(&st) {
        return None;
    }
    serde_json::from_str(&text).ok()
}

#[cfg(test)]
mod cursor_tests {

    fn fed(url: &str) -> std::sync::Arc<super::Fed> {
        super::Fed::new(super::FedConfig { secret: "a-long-shared-federation-secret".into(), peers: vec![], public_url: url.into(), mode: "network".into(), ephemeral: true, ..Default::default() }).unwrap()
    }
    fn with_pins(f: &super::Fed, pins: Vec<super::Pin>, peers: Vec<String>) -> std::sync::Arc<super::Fed> {
        let mut c = f.cfg.clone();
        c.pins = pins;
        c.peers = peers;
        super::Fed::new(c).unwrap()
    }
    fn pin_of(f: &super::Fed, url: &str) -> super::Pin {
        super::Pin { url: url.into(), id: f.node_id(), key: f.node_pub(), ..Default::default() }
    }

    /// AQUA-SEC-009: a v2 signature is bound to its recipient and path and
    /// is taken once; a relay that knows every peer's key refuses requests
    /// signed only with the shared secret (except introductions).
    #[test]
    fn node_signatures_bind_recipient_and_refuse_secret_only() {
        let (a, b, c) = (fed("http://a"), fed("http://b"), fed("http://c"));
        let a = with_pins(&a, vec![pin_of(&b, "http://b"), pin_of(&c, "http://c")], vec!["http://b".into(), "http://c".into()]);
        let b = with_pins(&b, vec![pin_of(&a, "http://a")], vec!["http://a".into()]);
        let c = with_pins(&c, vec![pin_of(&a, "http://a")], vec!["http://a".into()]);
        let mut h = axum::http::HeaderMap::new();
        h.insert("x-aqua-fed2", a.sign2("http://b", "GET", "/v1/fed/users?since=0", b"").unwrap().parse().unwrap());
        // The same signature at C (another recipient): refused.
        assert!(!c.verify(&h, "GET", "/v1/fed/users?since=0", b""), "signed for B, not for C");
        // At B, for another path: refused; for its path: once.
        assert!(!b.verify(&h, "GET", "/v1/fed/backups?since=0", b""));
        assert!(b.verify(&h, "GET", "/v1/fed/users?since=0", b""));
        assert!(!b.verify(&h, "GET", "/v1/fed/users?since=0", b""), "replayed");
        // Only the shared secret: B knows all its peers' keys → refused,
        // except for hello / ping.
        assert!(b.enforced());
        let mut s = axum::http::HeaderMap::new();
        s.insert("x-aqua-fed", a.header("GET", "/v1/fed/users?since=0", b"").parse().unwrap());
        assert!(!b.verify(&s, "GET", "/v1/fed/users?since=0", b""));
        let mut p = axum::http::HeaderMap::new();
        p.insert("x-aqua-fed", a.header("GET", "/v1/fed/ping", b"").parse().unwrap());
        assert!(b.verify(&p, "GET", "/v1/fed/ping", b""));
        // A known sender id with a forged signature: refused.
        let forged = fed("http://evil");
        let evil = with_pins(&forged, vec![super::Pin { url: "http://b".into(), id: b.node_id(), key: b.node_pub(), ..Default::default() }], vec![]);
        let fake = evil.sign2("http://b", "GET", "/v1/fed/users?since=0", b"").unwrap().replacen(&evil.node_id(), &a.node_id(), 1);
        let mut f2 = axum::http::HeaderMap::new();
        f2.insert("x-aqua-fed2", fake.parse().unwrap());
        assert!(!b.verify(&f2, "GET", "/v1/fed/users?since=0", b""), "a known id with another key's signature");
    }

    /// A hello pins a self-signed key; another key for the same address
    /// moves the pin only with the old key's signature; revoked ids stay out.
    #[test]
    fn hello_pins_rotate_only_when_signed_by_the_old_key() {
        let (a, b) = (fed("http://a"), fed("http://b"));
        let hello = |f: &super::Fed| -> super::Hello {
            let mut v = serde_json::json!({ "url": super::my_id(&f.cfg), "peers": [] });
            if let (Some(o), serde_json::Value::Object(k)) = (v.as_object_mut(), f.hello_key_fields()) { o.extend(k); }
            serde_json::from_value(v).unwrap()
        };
        let Some(super::PinChange::New(pin)) = super::pin_change(&b, &hello(&a)) else { panic!("first key is new") };
        assert_eq!(pin.id, a.node_id());
        let b = with_pins(&b, vec![pin.clone()], vec!["http://a".into()]);
        assert!(super::pin_change(&b, &hello(&a)).is_none(), "same key: nothing to change");
        // An impostor at A's address with its own key: refused.
        let imp = fed("http://a");
        assert!(matches!(super::pin_change(&b, &hello(&imp)), Some(super::PinChange::Conflict(p)) if p.conflict), "another key without a signature: a conflict for the admin");
        // A real rotation: the old key signs the new one.
        let new = aqua_core::crypto::SignSecret::generate();
        let mut c = a.cfg.clone();
        c.rotated = hex::encode(a.key.sign(format!("aqua-fed-rotate\n{}", hex::encode(new.public().as_bytes())).as_bytes()));
        c.node_key = hex::encode(new.to_bytes());
        let a2 = super::Fed::new(c).unwrap();
        let Some(super::PinChange::Rotated(moved, retired, _)) = super::pin_change(&b, &hello(&a2)) else { panic!("signed rotation is accepted") };
        assert_eq!(retired, vec![a.node_id()]);
        assert_eq!(moved.id, a2.node_id());
        // Revoked: never again.
        let mut rc = b.cfg.clone();
        rc.revoked.push(a2.node_id());
        let b2 = super::Fed::new(rc).unwrap();
        assert!(super::pin_change(&b2, &hello(&a2)).is_none());
    }

    fn net_json(extra: &str) -> String {
        format!(r#"{{"secret":"a-long-shared-federation-secret","peers":["http://p"],"mode":"network"{extra}}}"#)
    }

    /// REV2-001: the node key lives only in its own owner-only file. A key
    /// an older build wrote into aqua-federation.json moves there and leaves
    /// the config; each relay makes its own key; a published key is replaced.
    #[test]
    fn node_key_is_private_unique_and_migrated() {
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        // An older config with the key inside.
        let old = hex::encode(aqua_core::crypto::SignSecret::generate().to_bytes());
        std::fs::write(a.path().join(super::FILE), net_json(&format!(r#","node_key":"{old}""#))).unwrap();
        let c = super::FedConfig::load_in(a.path());
        assert_eq!(c.node_key, old, "the existing key is kept");
        let cfg_text = std::fs::read_to_string(a.path().join(super::FILE)).unwrap();
        assert!(!cfg_text.contains(&old) && !cfg_text.contains("node_key"), "no key in the config any more");
        assert_eq!(std::fs::read_to_string(a.path().join(super::NODE_KEY_FILE)).unwrap().trim(), old);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for f in [super::NODE_KEY_FILE, super::FILE] {
                assert_eq!(std::fs::metadata(a.path().join(f)).unwrap().permissions().mode() & 0o777, 0o600, "{f}");
            }
        }
        assert_eq!(super::FedConfig::load_in(a.path()).node_key, old, "stable across starts");
        // Another relay: its own key.
        std::fs::write(b.path().join(super::FILE), net_json("")).unwrap();
        let other = super::FedConfig::load_in(b.path());
        assert!(!other.node_key.is_empty());
        assert_ne!(other.node_key, old);
        // An unconfigured relay writes no key at all.
        let none = tempfile::tempdir().unwrap();
        assert!(super::FedConfig::load_in(none.path()).node_key.is_empty());
        assert!(!none.path().join(super::NODE_KEY_FILE).exists());
    }

    #[test]
    fn a_published_node_key_is_replaced_and_refused() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(super::FILE), net_json("")).unwrap();
        // A stand-in for a published key: put its id on the list for this test
        // is not possible (a const), so check the real leaked id instead.
        let f = fed("http://x");
        assert!(f.is_revoked(super::LEAKED_NODE_IDS[0]));
        let leaked_pin = super::Pin { url: "http://l".into(), id: super::LEAKED_NODE_IDS[0].into(), key: "00".repeat(32), ..Default::default() };
        let g = with_pins(&f, vec![leaked_pin], vec!["http://l".into()]);
        let mut h = axum::http::HeaderMap::new();
        h.insert("x-aqua-fed2", format!("{}.n.{}.{}.00", super::now_ms(), super::LEAKED_NODE_IDS[0], g.node_id()).parse().unwrap());
        assert!(!g.verify(&h, "GET", "/v1/fed/users?since=0", b""), "a leaked id is never accepted");
        // A relay whose key file holds a published key makes a new one at
        // start (a fresh key stands in for the published one here).
        let k = aqua_core::crypto::SignSecret::generate();
        let id = super::node_id_of(k.public().as_bytes());
        let hexk = hex::encode(k.to_bytes());
        std::fs::write(dir.path().join(super::NODE_KEY_FILE), &hexk).unwrap();
        let c = super::FedConfig::load_in_with(dir.path(), &[id.as_str()]);
        assert_ne!(c.node_key, hexk, "replaced");
        assert!(!super::key_is_leaked_in(&c.node_key, &[id.as_str()]));
        assert_eq!(std::fs::read_to_string(dir.path().join(super::NODE_KEY_FILE)).unwrap().trim(), c.node_key);
        // Not on the list: kept.
        assert_eq!(super::FedConfig::load_in(dir.path()).node_key, c.node_key);
    }

    /// A2: a request carrying both signatures uses up both. The same
    /// request with `x-aqua-fed2` cut off doesn't pass again on the secret
    /// (where the secret is still taken).
    #[test]
    fn a_v2_request_uses_up_its_secret_signature_too() {
        let (a, b) = (fed("http://a"), fed("http://b"));
        // B knows A's key, but not every peer's: the secret alone still works there.
        let b = with_pins(&b, vec![pin_of(&a, "http://a")], vec!["http://a".into(), "http://old".into()]);
        let a = with_pins(&a, vec![pin_of(&b, "http://b")], vec!["http://b".into()]);
        assert!(!b.enforced());
        let path = "/v1/fed/users?since=0";
        let mut h = axum::http::HeaderMap::new();
        h.insert("x-aqua-fed", a.header("GET", path, b"").parse().unwrap());
        h.insert("x-aqua-fed2", a.sign2("http://b", "GET", path, b"").unwrap().parse().unwrap());
        assert_eq!(b.verify_from(&h, "GET", path, b""), Some(super::Sender::Node(a.node_id())));
        let mut stripped = h.clone();
        stripped.remove("x-aqua-fed2");
        assert!(!b.verify(&stripped, "GET", path, b""), "replayed without x-aqua-fed2");
        // A fresh secret-only request is still fine here.
        let mut s = axum::http::HeaderMap::new();
        s.insert("x-aqua-fed", a.header("GET", path, b"").parse().unwrap());
        assert_eq!(b.verify_from(&s, "GET", path, b""), Some(super::Sender::Secret));
        // speaks_for: the pinned relay only, for its own address.
        assert!(b.speaks_for(&super::Sender::Node(a.node_id()), "http://a"));
        assert!(!b.speaks_for(&super::Sender::Secret, "http://a"));
        assert!(!b.speaks_for(&super::Sender::Node("someone".into()), "http://a"));
        assert!(b.speaks_for(&super::Sender::Secret, "http://old"), "unkeyed address, network not on keys");
    }

    /// A3: once every peer's key is known, keys are required — and a peer
    /// without a key put on the list afterwards (by the admin, AQUA_FED_PEERS,
    /// a hello, a made-up `tunnel:` id) no longer switches that off. Kept on
    /// disk: a restart doesn't step back either.
    #[test]
    fn enforcement_is_a_ratchet() {
        let (a, b) = (fed("http://a"), fed("http://b"));
        assert!(!b.enforced(), "a new network is in transition");
        assert_eq!(b.cfg.auth_mode, "transition");
        let b = with_pins(&b, vec![pin_of(&a, "http://a")], vec!["http://a".into()]);
        assert!(b.enforced());
        assert_eq!(b.cfg.auth_mode, "enforced");
        let b = with_pins(&b, b.cfg.pins.clone(), vec!["http://a".into(), "tunnel:made-up".into(), "http://old".into()]);
        assert!(b.enforced(), "an unkeyed peer doesn't switch it off");
        let mut s = axum::http::HeaderMap::new();
        s.insert("x-aqua-fed", a.header("GET", "/v1/fed/users?since=0", b"").parse().unwrap());
        assert!(!b.verify(&s, "GET", "/v1/fed/users?since=0", b""));
        // On disk.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(super::FILE), net_json("")).unwrap();
        let mut c = super::FedConfig::load_in(dir.path());
        c.pins = vec![super::Pin { url: "http://p".into(), ..pin_of(&a, "http://p") }];
        let f = super::Fed::new(c).unwrap();
        assert!(f.enforced());
        let mut back = super::FedConfig::load_in(dir.path());
        assert_eq!(back.auth_mode, "enforced", "saved");
        back.pins.clear();
        back.peers.push("http://new".into());
        assert!(super::Fed::new(back).unwrap().enforced(), "after a restart, without the pins: still required");
    }

    #[test]
    fn deadline_enforces_and_never_reverts() {
        let b = fed("http://b");
        let mut c = b.cfg.clone();
        c.peers = vec!["http://old".into()];
        assert!(!super::Fed::new(c.clone()).unwrap().enforced());
        assert!(c.auth_deadline > super::now_secs() + 13 * super::DAY, "14 days by default");
        c.auth_deadline = super::now_secs() - 1;
        let f = super::Fed::new(c).unwrap();
        assert!(f.enforced(), "the end of the transition");
        assert_eq!(f.cfg.auth_mode, "enforced");
        let mut c = f.cfg.clone();
        c.auth_deadline = super::now_secs() + 100 * super::DAY;
        assert!(super::Fed::new(c).unwrap().enforced(), "a later deadline doesn't bring it back");
        // A config that says something else: required.
        let mut c = f.cfg.clone();
        c.auth_mode = "off".into();
        assert!(super::Fed::new(c).unwrap().enforced());
    }

    #[test]
    fn legacy_window_expires_by_itself() {
        let a = fed("http://a");
        let b = with_pins(&fed("http://b"), vec![pin_of(&a, "http://a")], vec!["http://a".into()]);
        assert!(b.enforced());
        let mut c = b.cfg.clone();
        c.legacy_until = super::now_secs() + 1;
        let open = super::Fed::new(c).unwrap();
        assert!(!open.enforced(), "the admin's window");
        let mut s = axum::http::HeaderMap::new();
        s.insert("x-aqua-fed", a.header("GET", "/v1/fed/users?since=0", b"").parse().unwrap());
        assert!(open.verify(&s, "GET", "/v1/fed/users?since=0", b""));
        assert_eq!(open.cfg.auth_mode, "enforced", "still on record");
        std::thread::sleep(std::time::Duration::from_millis(2100));
        assert!(open.enforced(), "closed by itself, with no restart");
        let mut s = axum::http::HeaderMap::new();
        s.insert("x-aqua-fed", a.header("GET", "/v1/fed/users?since=0", b"").parse().unwrap());
        assert!(!open.verify(&s, "GET", "/v1/fed/users?since=0", b""));
        assert_eq!(super::Fed::new(open.cfg.clone()).unwrap().cfg.legacy_until, 0, "and cleared from the config");
    }

    /// A solo relay pinned for the name registry reads names only.
    #[test]
    fn names_role_cannot_read_users() {
        let (s, b) = (fed("http://s"), fed("http://b"));
        let solo = super::Pin { role: "names".into(), ..pin_of(&s, "http://s") };
        let mut c = b.cfg.clone();
        c.pins = vec![solo];
        c.solo_peers = vec!["http://s".into()];
        let b = super::Fed::new(c).unwrap();
        let s = with_pins(&s, vec![pin_of(&b, "http://b")], vec!["http://b".into()]);
        for (path, ok) in [("/v1/fed/users?since=0", false), ("/v1/fed/backups?since=0", false), ("/v1/fed/inboxes2", false), ("/v1/fed/names?since=0", true), ("/v1/fed/ping", true)] {
            let mut h = axum::http::HeaderMap::new();
            h.insert("x-aqua-fed2", s.sign2("http://b", "GET", path, b"").unwrap().parse().unwrap());
            assert_eq!(b.verify(&h, "GET", path, b""), ok, "{path}");
        }
        // A names pin doesn't make the network "all keys known".
        let mut c = b.cfg.clone();
        c.peers = vec![];
        c.auth_mode.clear();
        assert!(!c.all_keys_known());
    }

    fn hello_of(f: &super::Fed) -> super::Hello {
        let mut v = serde_json::json!({ "url": super::my_id(&f.cfg), "peers": [] });
        if let (Some(o), serde_json::Value::Object(k)) = (v.as_object_mut(), f.hello_key_fields()) { o.extend(k); }
        serde_json::from_value(v).unwrap()
    }
    fn rotated(f: &super::Fed) -> std::sync::Arc<super::Fed> {
        let mut c = f.cfg.clone();
        super::rotate_into(f, &mut c, &aqua_core::crypto::SignSecret::generate());
        super::Fed::new(c).unwrap()
    }

    /// A4: a peer that was off while a relay changed its key twice follows
    /// the whole chain; the ids it passed are retired.
    #[test]
    fn rotation_chain_two_steps_offline_peer() {
        let (a0, b) = (fed("http://a"), fed("http://b"));
        let b = with_pins(&b, vec![pin_of(&a0, "http://a")], vec!["http://a".into()]);
        let a1 = rotated(&a0);
        let a2 = rotated(&a1);
        assert_eq!(a2.cfg.rotations.len(), 2);
        let Some(super::PinChange::Rotated(p, retired, keys)) = super::pin_change(&b, &hello_of(&a2)) else { panic!("two steps followed") };
        assert_eq!((p.id.as_str(), p.rot_seq), (a2.node_id().as_str(), 2));
        assert_eq!(retired, vec![a0.node_id(), a1.node_id()]);
        assert_eq!(keys, vec![a0.node_pub(), a1.node_pub()]);
        // The one-step field alone (a relay of 1.21.0-24) isn't enough for two.
        let mut h = hello_of(&a2);
        h.rotations.clear();
        assert!(matches!(super::pin_change(&b, &h), Some(super::PinChange::Conflict(_))));
        // A forged step (signed by another key) breaks the chain.
        let mut h = hello_of(&a2);
        h.rotations[0].sig = hex::encode(fed("http://x").key.sign(b"x"));
        assert!(matches!(super::pin_change(&b, &h), Some(super::PinChange::Conflict(_))));
        // At most MAX_ROTATIONS kept.
        let mut f = a2;
        for _ in 0..10 { f = rotated(&f); }
        assert_eq!(f.cfg.rotations.len(), super::MAX_ROTATIONS);
    }

    /// A4: once followed away from, an old key (or one of the chain) never
    /// comes back — neither by a hello of its own nor by a new chain from it.
    #[test]
    fn retired_id_cannot_rotate_again() {
        let (a0, b) = (fed("http://a"), fed("http://b"));
        let b = with_pins(&b, vec![pin_of(&a0, "http://a")], vec!["http://a".into()]);
        let a1 = rotated(&a0);
        let Some(super::PinChange::Rotated(p, retired, _)) = super::pin_change(&b, &hello_of(&a1)) else { panic!() };
        let mut c = b.cfg.clone();
        c.pins = vec![p];
        c.retired = retired;
        let b = super::Fed::new(c).unwrap();
        assert!(super::pin_change(&b, &hello_of(&a0)).is_none(), "the old key's own hello");
        // Someone with the old key makes a chain from it to a key of theirs.
        let evil = rotated(&a0);
        assert!(!matches!(super::pin_change(&b, &hello_of(&evil)), Some(super::PinChange::Rotated(..))), "a chain from a retired key");
        // And the old key can't sign requests.
        let mut h = axum::http::HeaderMap::new();
        let a0b = with_pins(&a0, vec![pin_of(&b, "http://b")], vec!["http://b".into()]);
        h.insert("x-aqua-fed2", a0b.sign2("http://b", "GET", "/v1/fed/users?since=0", b"").unwrap().parse().unwrap());
        assert!(!b.verify(&h, "GET", "/v1/fed/users?since=0", b""));
    }

    /// Review: deletions a relay signed before it changed its key are still
    /// taken by a peer that followed the change; none signed after it.
    #[test]
    fn tombs_signed_with_a_retired_key_before_the_change() {
        let (a0, b) = (fed("http://a"), fed("http://b"));
        let b = with_pins(&b, vec![pin_of(&a0, "http://a")], vec!["http://a".into()]);
        let before = a0.sign_tomb("1000000001", "gone", 1, super::now_secs() as i64 - 10);
        let a1 = rotated(&a0);
        let Some(super::PinChange::Rotated(p, retired, keys)) = super::pin_change(&b, &hello_of(&a1)) else { panic!() };
        let mut c = b.cfg.clone();
        c.pins = vec![p];
        let now = super::now_secs();
        c.retired_keys = retired.iter().zip(&keys).map(|(r, k)| (r.clone(), k.clone(), now)).collect();
        c.retired = retired;
        let b = super::Fed::new(c).unwrap();
        assert!(b.verify_tomb(&before), "signed before the change");
        let after = a0.sign_tomb("1000000002", "other", 1, now as i64 + 100);
        assert!(!b.verify_tomb(&after), "the old key after the change: no");
        assert!(b.verify_tomb(&a1.sign_tomb("1000000003", "x", 1, now as i64 + 100)), "the new key: yes");
    }

    #[test]
    fn cursor_queries_encode_the_key() {
        assert_eq!(super::cursor_query(&(5, String::new())), "since=5");
        assert_eq!(super::cursor_query(&(5, "bob".into())), "since=5&after=bob");
        assert_eq!(super::cursor_query(&(5, "bob/k 1".into())), "since=5&after=bob%2Fk%201");
    }
}
