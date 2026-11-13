//! Stars (aqua-server `stars.rs`): marks only the project owner hands out.
//!
//! A star is a statement signed with the owner's Ed25519 key — "the account
//! with ID <uid> has star <kind> until <exp>". This app believes it only if
//! the signature checks out against the public key below, the ID is the
//! card's own, and it hasn't run out; so no server, plugin or person can
//! give themselves one. Kinds: 0 — the official star, 1–8 — Boosty levels.

use std::sync::Mutex as StdMutex;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::state::AppState;

/// The owner's public star key (hex; made on the star relay's panel → Stars,
/// 2026-11-13). Empty: no star shows. The repository variable
/// `AQUA_STAR_PUBLIC` (release.yml) wins when set — a public key, nothing secret.
pub const STAR_PUBLIC_BUILTIN: &str = "3abdfac4ef46672c05a21e9024073208655c0deae3f645441dc1776217a92431";

fn star_public() -> &'static str {
    match option_env!("AQUA_STAR_PUBLIC") {
        Some(k) if k.trim().len() == 64 => k.trim(),
        _ => STAR_PUBLIC_BUILTIN,
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Statement {
    pub uid: String,
    pub kind: u8,
    pub exp: u64,
    pub sig: String,
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// The star kind, if the statement is real, for this ID, and still holds.
pub fn check(uid: &str, st: &Statement) -> Option<u8> {
    let pk: [u8; 32] = hex::decode(star_public()).ok()?.try_into().ok()?;
    let sig: [u8; 64] = hex::decode(&st.sig).ok()?.try_into().ok()?;
    if st.uid != uid || uid.is_empty() || st.exp < now() || st.kind > 8 {
        return None;
    }
    let msg = format!("aqua-star-v1|{}|{}|{}", st.uid, st.kind, st.exp);
    aqua_core::crypto::SignPublic::from_bytes(pk).verify(msg.as_bytes(), &sig).ok()?;
    Some(st.kind)
}

/// A check mark statement (aqua-server `stars::check_bytes`): real, for
/// this ID, not run out.
pub fn check_mark(uid: &str, st: &Statement) -> bool {
    let Some(pk) = hex::decode(star_public()).ok().and_then(|v| <[u8; 32]>::try_from(v).ok()) else { return false };
    let Some(sig) = hex::decode(&st.sig).ok().and_then(|v| <[u8; 64]>::try_from(v).ok()) else { return false };
    if st.uid != uid || uid.is_empty() || st.exp < now() {
        return false;
    }
    let msg = format!("aqua-check-v1|{}|{}", st.uid, st.exp);
    aqua_core::crypto::SignPublic::from_bytes(pk).verify(msg.as_bytes(), &sig).is_ok()
}

/// Someone's card carries a statement (`extra.st`): its kind if it holds.
#[tauri::command]
pub fn star_verify(uid: String, star: serde_json::Value) -> Option<u8> {
    let st: Statement = serde_json::from_value(star).ok()?;
    check(&uid, &st)
}

/// The statement for `uid`, asked of my relay (which asks the star relay).
/// `Ok(None)`: the star relay says there's no star (404). `Err`: it couldn't
/// be asked — "don't know", not "no star".
async fn fetch(state: &State<'_, StdMutex<AppState>>, uid: Option<String>) -> Result<Option<Statement>, String> {
    if star_public().is_empty() {
        return Ok(None);
    }
    let (server, me) = {
        let s = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        match &s.transport {
            Some(t) => (t.server_url().to_string(), t.uid().to_string()),
            None => return Err("offline".into()),
        }
    };
    let uid = uid.unwrap_or(me);
    if uid.is_empty() || !uid.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Ok(None);
    }
    let http = aqua_client_core::transport::http_client().map_err(|e| e.to_string())?;
    let r = http
        .get(format!("{}/v1/stars/{uid}", server.trim_end_matches('/')))
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| e.without_url().to_string())?;
    if r.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !r.status().is_success() {
        return Err(format!("stars: HTTP {}", r.status()));
    }
    let st: Statement = r.json().await.map_err(|e| e.without_url().to_string())?;
    Ok(check(&uid, &st).map(|_| st))
}

/// My own star: the statement to put on my card, or None.
#[tauri::command]
pub async fn my_star(state: State<'_, StdMutex<AppState>>) -> Result<Option<Statement>, String> {
    fetch(&state, None).await
}


#[derive(Serialize)]
pub struct LiveMarks {
    /// The star's kind, or None.
    pub star: Option<u8>,
    /// The star relay's check mark; None: this relay can't tell (older).
    pub check: Option<bool>,
}

#[derive(Deserialize)]
struct MarksResp {
    #[serde(default)]
    star: Option<Statement>,
    #[serde(default)]
    check: Option<Statement>,
}

/// Someone's star and check mark as the star relay has them now — both
/// signed by the owner's key, so they show whichever server they're on.
/// `Err`: couldn't ask — the caller keeps what the card says.
#[tauri::command]
pub async fn marks_live(state: State<'_, StdMutex<AppState>>, uid: String) -> Result<LiveMarks, String> {
    if star_public().is_empty() || uid.is_empty() || !uid.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Ok(LiveMarks { star: None, check: None });
    }
    let server = {
        let s = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        match &s.transport {
            Some(t) => t.server_url().to_string(),
            None => return Err("offline".into()),
        }
    };
    let http = aqua_client_core::transport::http_client().map_err(|e| e.to_string())?;
    let r = http
        .get(format!("{}/v1/marks/{uid}", server.trim_end_matches('/')))
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| e.without_url().to_string())?;
    // An older relay: only the star, the old way.
    if r.status() == reqwest::StatusCode::NOT_FOUND || r.status() == reqwest::StatusCode::METHOD_NOT_ALLOWED {
        let star = fetch(&state, Some(uid)).await?.map(|st| st.kind);
        return Ok(LiveMarks { star, check: None });
    }
    if !r.status().is_success() {
        return Err(format!("marks: HTTP {}", r.status()));
    }
    let m: MarksResp = r.json().await.map_err(|e| e.without_url().to_string())?;
    let star = m.star.as_ref().and_then(|st| check(&uid, st));
    let check = m.check.as_ref().is_some_and(|st| check_mark(&uid, st));
    Ok(LiveMarks { star, check: Some(check) })
}
