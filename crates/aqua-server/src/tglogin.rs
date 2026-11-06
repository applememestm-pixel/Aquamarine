//! Signing in with Telegram (1.2.0-4).
//!
//! The relay's bot (the token from the admin panel, `aqua-telegram.json` /
//! `AQUA_TG_BOT_TOKEN` — the same one sticker packs use) confirms that a
//! person owns a Telegram account:
//!
//! 1. the app asks `POST /v1/auth/tg/start` → `{code, link, bot}`;
//! 2. the person opens `https://t.me/<bot>?start=aq<code>` and taps Start;
//! 3. this module reads the bot's messages (`getUpdates`, only while
//!    someone is waiting), finds `/start aq<code>`, answers in the chat;
//! 4. the app asks `POST /v1/auth/tg/poll {code}` → `{done: true, ticket}`.
//!
//! The ticket (10 minutes, once) is what sign-up, sign-in and linking take:
//! a linked Telegram account is a second factor like the authenticator app
//! (see `Directory::sign_in_plain`), so such an account can move between
//! devices. The Telegram ID lives in `users.security` (`Security.tg`) and
//! travels with the account through the federation.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::{http::StatusCode, Json};
use serde::Deserialize;

/// A code waits this long for the Start tap; a ticket lives this long.
const CODE_TTL: Duration = Duration::from_secs(10 * 60);

#[derive(Clone)]
struct Pending {
    at: Instant,
    /// Set when the person tapped Start: (Telegram user id, @username or name).
    who: Option<(String, String)>,
    ticket: Option<String>,
}

#[derive(Clone)]
struct Ticket {
    at: Instant,
    tg_id: String,
    tg_name: String,
}

#[derive(Default)]
struct Book {
    codes: HashMap<String, Pending>,
    tickets: HashMap<String, Ticket>,
    polling: bool,
    /// (token, @username): asked again when the token changes.
    bot: Option<(String, String)>,
    /// Why reading the bot's messages fails right now (shown to whoever waits).
    error: Option<String>,
}

/// Telegram's refusal in words a person can act on.
fn explain(e: &str) -> String {
    let l = e.to_lowercase();
    if l.contains("webhook") {
        "the server's Telegram bot has a webhook set, so the server can't read its messages — remove the webhook (another program uses this bot) or give the server its own bot".into()
    } else if l.contains("terminated by other getupdates") || l.contains("conflict") {
        "another program reads this Telegram bot's messages (a second server or script with the same token) — stop it or give this server its own bot".into()
    } else if l.contains("unauthorized") || l.contains("not found") {
        "the server's Telegram bot token is invalid (revoked?) — the server's admin has to set a new one".into()
    } else {
        format!("the server can't reach Telegram: {e}")
    }
}

fn rand_hex(n: usize) -> String {
    let mut b = vec![0u8; n];
    rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut b);
    hex::encode(b)
}

fn http() -> reqwest::Client {
    reqwest::Client::builder().timeout(Duration::from_secs(40)).build().unwrap_or_default()
}

async fn call(token: &str, method: &str, body: serde_json::Value) -> Result<serde_json::Value, String> {
    let r = http().post(format!("https://api.telegram.org/bot{token}/{method}")).json(&body).send().await.map_err(|e| e.without_url().to_string())?;
    let v: serde_json::Value = r.json().await.map_err(|e| e.to_string())?;
    if v["ok"].as_bool() != Some(true) {
        return Err(v["description"].as_str().unwrap_or("Telegram said no").to_string());
    }
    Ok(v["result"].clone())
}

/// The bot's @username (asked once).
async fn bot_name(token: &str) -> Result<String, String> {
    if let Some((t, b)) = state().bot.clone() {
        if t == token {
            return Ok(b);
        }
    }
    let me = call(token, "getMe", serde_json::json!({})).await?;
    let name = me["username"].as_str().ok_or("the bot has no username")?.to_string();
    state().bot = Some((token.to_string(), name.clone()));
    Ok(name)
}

/// Read the bot's messages while codes are waiting, then stop.
fn ensure_polling(token: String) {
    {
        let mut b = state();
        if b.polling {
            return;
        }
        b.polling = true;
    }
    tokio::spawn(async move {
        let mut offset: i64 = 0;
        loop {
            let waiting = {
                let mut b = state();
                b.codes.retain(|_, p| p.at.elapsed() < CODE_TTL);
                b.tickets.retain(|_, t| t.at.elapsed() < CODE_TTL);
                b.codes.values().any(|p| p.who.is_none())
            };
            if !waiting {
                state().polling = false;
                return;
            }
            let body = serde_json::json!({ "offset": offset, "timeout": 25, "allowed_updates": ["message"] });
            let updates = match call(&token, "getUpdates", body).await {
                Ok(v) => {
                    state().error = None;
                    v
                }
                Err(e) => {
                    state().error = Some(explain(&e));
                    tracing::warn!(error = %e, "telegram sign-in: getUpdates failed (is a webhook set on this bot?)");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    continue;
                }
            };
            for u in updates.as_array().into_iter().flatten() {
                offset = offset.max(u["update_id"].as_i64().unwrap_or(0) + 1);
                let m = &u["message"];
                let Some(text) = m["text"].as_str() else { continue };
                let Some(code) = text.trim().strip_prefix("/start").map(str::trim).and_then(|c| c.strip_prefix("aq")) else { continue };
                let Some(tg_id) = m["from"]["id"].as_i64().map(|i| i.to_string()) else { continue };
                if m["from"]["is_bot"].as_bool() == Some(true) {
                    continue;
                }
                let name = m["from"]["username"].as_str().map(|u| format!("@{u}"))
                    .or_else(|| m["from"]["first_name"].as_str().map(str::to_string))
                    .unwrap_or_default();
                let ru = m["from"]["language_code"].as_str().is_some_and(|l| l.starts_with("ru") || l.starts_with("uk") || l.starts_with("be"));
                let ok = {
                    let mut b = state();
                    match b.codes.get_mut(code) {
                        Some(p) if p.who.is_none() && p.at.elapsed() < CODE_TTL => {
                            p.who = Some((tg_id.clone(), name.clone()));
                            true
                        }
                        _ => false,
                    }
                };
                let reply = match (ok, ru) {
                    (true, true) => "✅ Вход в Aquamarine подтверждён. Вернитесь в приложение.",
                    (true, false) => "✅ Aquamarine sign-in confirmed. Go back to the app.",
                    (false, true) => "Код не найден или устарел. Начните вход в приложении заново.",
                    (false, false) => "This code is unknown or has expired. Start again in the app.",
                };
                let chat = m["chat"]["id"].clone();
                let _ = call(&token, "sendMessage", serde_json::json!({ "chat_id": chat, "text": reply })).await;
                if ok {
                    tracing::info!(tg = %tg_id, "telegram sign-in confirmed");
                }
            }
        }
    });
}

fn no_bot() -> (StatusCode, String) {
    (StatusCode::NOT_FOUND, "this server has no Telegram bot — its admin adds one in the panel (Stickers → Telegram)".into())
}

/// Telegram sign-in is set up on this relay.
pub fn available() -> bool {
    crate::packs::bot_token().is_some()
}

/// `POST /v1/auth/tg/start` → `{code, link, bot}`.
pub async fn start() -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let token = crate::packs::bot_token().ok_or_else(no_bot)?;
    let bot = bot_name(&token).await.map_err(|e| (StatusCode::BAD_GATEWAY, format!("Telegram: {e}")))?;
    let code = rand_hex(10);
    {
        let mut b = state();
        b.codes.retain(|_, p| p.at.elapsed() < CODE_TTL);
        if b.codes.len() > 2000 {
            return Err((StatusCode::TOO_MANY_REQUESTS, "too many sign-ins waiting — try again in a minute".into()));
        }
        b.codes.insert(code.clone(), Pending { at: Instant::now(), who: None, ticket: None });
        if !b.polling {
            b.error = None;
        }
    }
    ensure_polling(token);
    Ok(Json(serde_json::json!({ "code": code, "bot": bot, "link": format!("https://t.me/{bot}?start=aq{code}") })))
}

#[derive(Deserialize)]
pub struct PollBody {
    code: String,
}

/// `POST /v1/auth/tg/poll {code}` → `{done: false}` or `{done: true, ticket, name}`.
pub async fn poll(Json(b): Json<PollBody>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let mut book = state();
    let Some(p) = book.codes.get(&b.code).cloned() else {
        return Err((StatusCode::NOT_FOUND, "this Telegram sign-in has expired — start again".into()));
    };
    let Some((tg_id, tg_name)) = p.who else { return Ok(Json(serde_json::json!({ "done": false, "error": book.error }))) };
    let ticket = match p.ticket {
        Some(t) => t,
        None => {
            let t = rand_hex(16);
            book.tickets.insert(t.clone(), Ticket { at: Instant::now(), tg_id, tg_name: tg_name.clone() });
            if let Some(p) = book.codes.get_mut(&b.code) { p.ticket = Some(t.clone()); }
            t
        }
    };
    Ok(Json(serde_json::json!({ "done": true, "ticket": ticket, "name": tg_name })))
}

/// Use up a ticket → (Telegram user id, its @username or name).
pub fn take(ticket: &str) -> Option<(String, String)> {
    let mut b = state();
    let t = b.tickets.remove(ticket.trim())?;
    b.codes.retain(|_, p| p.ticket.as_deref() != Some(ticket.trim()));
    (t.at.elapsed() < CODE_TTL).then_some((t.tg_id, t.tg_name))
}

/// Tests: a confirmed ticket for this Telegram user, without Telegram.
#[doc(hidden)]
pub fn test_ticket(tg_id: &str, tg_name: &str) -> String {
    let t = rand_hex(16);
    state().tickets.insert(t.clone(), Ticket { at: Instant::now(), tg_id: tg_id.into(), tg_name: tg_name.into() });
    t
}

fn state() -> std::sync::MutexGuard<'static, Book> {
    static B: std::sync::OnceLock<Mutex<Book>> = std::sync::OnceLock::new();
    B.get_or_init(|| Mutex::new(Book::default())).lock().unwrap_or_else(|e| e.into_inner())
}
