//! Telegram Bot API compatibility layer.
//!
//! Lets bots written for Telegram run on Aquamarine unchanged: point the
//! library at this relay instead of `https://api.telegram.org`.
//!
//! ```text
//! python-telegram-bot  Application.builder().token(T).base_url("http://relay:7777/bot")
//! aiogram 3            Bot(T, session=AiohttpSession(api=TelegramAPIServer.from_base("http://relay:7777")))
//! telegraf / grammY    new Telegraf(T, { telegram: { apiRoot: "http://relay:7777" } })
//! pyTelegramBotAPI     telebot.apihelper.API_URL = "http://relay:7777/bot{0}/{1}"
//! curl                 curl http://relay:7777/bot$T/getMe
//! ```
//!
//! `T` is the bot's Aquamarine token. Telegram libraries that insist on the
//! `<digits>:<secret>` shape get the same token with an id in front
//! (`/v1/bot/me` and the Bots screen show it); the numeric part is ignored.
//!
//! What maps how:
//!
//! * Private chats only. A user's `id` (= `chat.id`) is a stable number
//!   derived from the @username; `chat_id` may also be `"@username"`.
//! * `getUpdates` (long polling, `offset`, `limit`, `timeout`) and webhooks
//!   (`setWebhook` with `secret_token`, `deleteWebhook`, `getWebhookInfo`).
//! * `sendMessage` with `parse_mode` HTML / MarkdownV2 / Markdown, turned
//!   into Aquamarine's markup (`**bold**`, `__italic__`, `` `code` ``…).
//! * Keyboards: the message carries the keyboard as a `[kb:v1]{…}` first
//!   line, and the app draws real buttons (inline under the message, reply
//!   keyboards over the message box). Pressing a callback button sends
//!   `[cb:v1]{"m":id,"d":data}` and the bot gets a real `callback_query`.
//!   Older apps still see the numbered list under the text and may answer
//!   with the number or the label.
//! * Mini apps (`web_app` buttons, the menu button): the app opens the page
//!   in Telegram Web's iframe mode, so the official `telegram-web-app.js`
//!   works unchanged; `initData` is signed by this relay with the bot token
//!   exactly like Telegram does (`POST /v1/webapp/init`). `sendData` arrives
//!   as `message.web_app_data`; `answerWebAppQuery` sends the result.
//! * `editMessageText` / `editMessageReplyMarkup` send the new version as a
//!   new message (Aquamarine has no bot-side edits yet); `deleteMessage`,
//!   `sendChatAction`, `setMyCommands`… succeed.
//! * Media methods (`sendPhoto`, `sendDocument`…) deliver the caption and,
//!   for a URL, the link; uploaded files are not relayed yet.
//!
//! Bot ↔ user chats are not end-to-end encrypted (same as the native bot
//! API); the client marks them so.

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicI64, Ordering},
        Arc, RwLock,
    },
    time::Duration,
};

use axum::{
    body::Bytes,
    extract::{FromRequest, Multipart, Path, Query, Request, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use dashmap::DashMap;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::{AppState, BotUpdate, BOT_TEXT_LIMIT};

const WEBHOOK_FILE: &str = "aqua-tg-webhooks.json";
/// A keyboard stays answerable this long.
const KEYBOARD_TTL: Duration = Duration::from_secs(24 * 3600);

/// One answerable button of an inline keyboard.
#[derive(Clone, Debug)]
struct Button {
    label: String,
    data: String,
}

#[derive(Clone, Debug)]
struct Keyboard {
    buttons: Vec<Button>,
    /// The message the keyboard was attached to (for callback_query.message).
    message_id: i64,
    text: String,
    at: std::time::Instant,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct Webhook {
    url: String,
    #[serde(default)]
    secret: String,
    #[serde(skip)]
    last_error: Option<(i64, String)>,
}

/// State of the compatibility layer (kept in [`AppState::tg`]).
#[derive(Default)]
pub struct TgState {
    /// (bot, user) → the last inline keyboard the bot sent the user.
    keyboards: DashMap<(String, String), Keyboard>,
    /// Numeric id → username (users and bots).
    names: DashMap<i64, String>,
    /// Per-bot outgoing message counters.
    counters: DashMap<String, Arc<AtomicI64>>,
    /// Bot → commands set with setMyCommands.
    commands: DashMap<String, Value>,
    /// Bot → webhook.
    webhooks: RwLock<HashMap<String, Webhook>>,
    /// Bot → running webhook pusher.
    pushers: DashMap<String, tokio::task::AbortHandle>,
    /// Bot → HMAC-SHA256("WebAppData", token as the bot uses it): signs
    /// mini app initData. Learned when the bot calls the API.
    webapp_keys: DashMap<String, Vec<u8>>,
    /// Bot → menu button (setChatMenuButton).
    menus: DashMap<String, Value>,
    /// Mini app query_id → (bot, user), for answerWebAppQuery.
    queries: DashMap<String, (String, String, std::time::Instant)>,
}

impl TgState {
    pub fn load() -> Self {
        let s = Self::default();
        if let Some(map) = std::fs::read(WEBHOOK_FILE)
            .ok()
            .and_then(|b| serde_json::from_slice::<HashMap<String, Webhook>>(&b).ok())
        {
            *s.webhooks.write().unwrap() = map;
        }
        s
    }

    fn save_webhooks(&self) {
        let map = self.webhooks.read().unwrap().clone();
        if let Ok(b) = serde_json::to_vec_pretty(&map) {
            let _ = std::fs::write(WEBHOOK_FILE, b);
        }
    }

    fn next_message_id(&self, bot: &str) -> i64 {
        self.counters
            .entry(bot.to_string())
            .or_insert_with(|| Arc::new(AtomicI64::new(1 << 20)))
            .fetch_add(1, Ordering::Relaxed)
    }
}

/// Stable positive numeric id for a username (48 bits: safe in JavaScript).
pub fn tg_id(name: &str) -> i64 {
    let h = Sha256::digest(name.as_bytes());
    let mut b = [0u8; 8];
    b[2..].copy_from_slice(&h[..6]);
    i64::from_be_bytes(b).max(1)
}

fn remember(state: &AppState, name: &str) -> i64 {
    let id = tg_id(name);
    state.tg.names.insert(id, name.to_string());
    id
}

/// `chat_id` → username: `"@name"`, a numeric id we handed out, or a bare name.
fn resolve_chat(state: &AppState, bot: &str, v: Option<&Value>) -> Option<String> {
    let v = v?;
    let raw = match v {
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.trim().to_string(),
        _ => return None,
    };
    if let Some(name) = raw.strip_prefix('@') {
        return Some(name.to_lowercase());
    }
    if let Ok(id) = raw.parse::<i64>() {
        if let Some(n) = state.tg.names.get(&id) {
            return Some(n.clone());
        }
        // Not seen since the restart: one of the bot's users?
        for u in state.directory.bot_peers_of(bot) {
            if tg_id(&u) == id {
                state.tg.names.insert(id, u.clone());
                return Some(u);
            }
        }
        if let Some(owner) = state.directory.bot_owner_name(bot) {
            if tg_id(&owner) == id {
                return Some(owner);
            }
        }
        return None;
    }
    Some(raw.to_lowercase())
}

fn user_json(state: &AppState, name: &str, is_bot: bool) -> Value {
    let id = remember(state, name);
    let display = state.directory.display_name_of(name);
    let first = if display.trim().is_empty() { name.to_string() } else { display };
    json!({ "id": id, "is_bot": is_bot, "first_name": first, "username": name })
}

fn chat_json(state: &AppState, name: &str) -> Value {
    let id = remember(state, name);
    let display = state.directory.display_name_of(name);
    let first = if display.trim().is_empty() { name.to_string() } else { display };
    json!({ "id": id, "type": "private", "first_name": first, "username": name })
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// `/start@bot args` → entities for the leading command.
fn entities(text: &str) -> Option<Value> {
    if !text.starts_with('/') {
        return None;
    }
    let len = text.find(char::is_whitespace).unwrap_or(text.len());
    let utf16 = text[..len].encode_utf16().count();
    Some(json!([{ "type": "bot_command", "offset": 0, "length": utf16 }]))
}

// ------------------------------------------------------------------- incoming

/// A user wrote to a bot. If the bot has an inline keyboard open for this
/// user and the text picks one of its buttons, the update becomes a
/// callback query.
pub fn classify(state: &AppState, bot: &str, user: &str, text: &str) -> Option<(String, i64)> {
    // A button pressed in the app: `[cb:v1]{"m":message_id,"d":callback_data}`.
    if let Some(body) = text.strip_prefix(CB_PREFIX) {
        let v: Value = serde_json::from_str(body).ok()?;
        let data = v.get("d")?.as_str()?.chars().take(64).collect::<String>();
        return Some((data, v.get("m").and_then(|m| m.as_i64()).unwrap_or(0)));
    }
    let key = (bot.to_string(), user.to_string());
    let kb = state.tg.keyboards.get(&key)?.clone();
    if kb.at.elapsed() > KEYBOARD_TTL {
        state.tg.keyboards.remove(&key);
        return None;
    }
    let t = text.trim();
    let pick = t
        .trim_end_matches(['.', ')'])
        .parse::<usize>()
        .ok()
        .and_then(|n| n.checked_sub(1))
        .and_then(|i| kb.buttons.get(i))
        .or_else(|| kb.buttons.iter().find(|b| b.label.trim().eq_ignore_ascii_case(t)))?;
    Some((pick.data.clone(), kb.message_id))
}

pub const CB_PREFIX: &str = "[cb:v1]";
pub const WAD_PREFIX: &str = "[wad:v1]";
pub const KB_PREFIX: &str = "[kb:v1]";

/// `[wad:v1]{"t":button_text,"d":data}` — Telegram.WebApp.sendData().
pub fn web_app_data(text: &str) -> Option<(String, String)> {
    let v: Value = serde_json::from_str(text.strip_prefix(WAD_PREFIX)?).ok()?;
    let data: String = v.get("d")?.as_str()?.chars().take(4096).collect();
    let button: String = v.get("t").and_then(|t| t.as_str()).unwrap_or("").chars().take(64).collect();
    Some((data, button))
}

/// A queued update in Telegram's shape.
fn update_json(state: &AppState, bot: &str, u: &BotUpdate) -> Value {
    let update_id = u.id as i64 + 1;
    let from = user_json(state, &u.from, false);
    let chat = chat_json(state, &u.from);
    if let Some(data) = &u.callback {
        let kb_text = state
            .tg
            .keyboards
            .get(&(bot.to_string(), u.from.clone()))
            .map(|k| k.text.clone())
            .unwrap_or_default();
        let msg = json!({
            "message_id": u.callback_msg.unwrap_or(0),
            "from": user_json(state, bot, true),
            "chat": chat,
            "date": u.ts,
            "text": kb_text,
        });
        return json!({
            "update_id": update_id,
            "callback_query": {
                "id": format!("{}-{}", tg_id(&u.from), update_id),
                "from": from,
                "message": msg,
                "chat_instance": tg_id(&format!("{bot}/{}", u.from)).to_string(),
                "data": data,
            }
        });
    }
    if let Some((data, button_text)) = web_app_data(&u.text) {
        return json!({ "update_id": update_id, "message": {
            "message_id": update_id, "from": from, "chat": chat, "date": u.ts,
            "web_app_data": { "data": data, "button_text": button_text },
        }});
    }
    let mut message = json!({
        "message_id": update_id,
        "from": from,
        "chat": chat,
        "date": u.ts,
        "text": u.text,
    });
    if let Some(e) = entities(&u.text) {
        message["entities"] = e;
    }
    json!({ "update_id": update_id, "message": message })
}

// ------------------------------------------------------------------- formatting

/// Telegram HTML → Aquamarine markup.
fn from_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    let b = s.as_bytes();
    let mut links: Vec<String> = Vec::new();
    while i < s.len() {
        if b[i] == b'<' {
            if let Some(end) = s[i..].find('>') {
                let tag = &s[i + 1..i + end];
                let closing = tag.starts_with('/');
                let name = tag.trim_start_matches('/').split(|c: char| c.is_whitespace()).next().unwrap_or("").to_lowercase();
                let mark = match name.as_str() {
                    "b" | "strong" => "**",
                    "i" | "em" => "__",
                    "u" | "ins" => "++",
                    "s" | "strike" | "del" => "~~",
                    "tg-spoiler" => "||",
                    "span" if tag.contains("tg-spoiler") || closing => "||",
                    "code" => "`",
                    "pre" => "```",
                    "br" => "\n",
                    _ => "",
                };
                if name == "a" {
                    if closing {
                        let href = links.pop().unwrap_or_default();
                        out.push_str(&format!("]({href})"));
                    } else {
                        let href = tag
                            .split("href=")
                            .nth(1)
                            .map(|h| h.trim_start_matches(['"', '\'']).split(['"', '\'']).next().unwrap_or("").to_string())
                            .unwrap_or_default();
                        links.push(html_unescape(&href));
                        out.push('[');
                    }
                } else if name == "blockquote" {
                    if !closing {
                        out.push_str("> ");
                    }
                } else if name == "pre" {
                    out.push_str(if closing { "\n```" } else { "```\n" });
                } else {
                    out.push_str(mark);
                }
                i += end + 1;
                continue;
            }
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    html_unescape(&out).replace("```\n\n", "```\n")
}

fn html_unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&amp;", "&")
}

/// Telegram MarkdownV2 / legacy Markdown → Aquamarine markup.
fn from_markdown(s: &str, v2: bool) -> String {
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\\' if v2 && i + 1 < chars.len() => {
                out.push(chars[i + 1]);
                i += 2;
                continue;
            }
            '`' => {
                // Code spans / blocks are copied verbatim.
                let fence = chars[i..].iter().take_while(|&&x| x == '`').count();
                let open: String = "`".repeat(fence);
                let rest: String = chars[i + fence..].iter().collect();
                if let Some(end) = rest.find(&open) {
                    out.push_str(&open);
                    out.push_str(&rest[..end]);
                    out.push_str(&open);
                    i += fence + rest[..end].chars().count() + fence;
                    continue;
                }
                out.push_str(&open);
                i += fence;
                continue;
            }
            '*' => out.push_str("**"),
            '_' if v2 && chars.get(i + 1) == Some(&'_') => {
                out.push_str("++");
                i += 2;
                continue;
            }
            '_' => out.push_str("__"),
            '~' if v2 => out.push_str("~~"),
            _ => out.push(c),
        }
        i += 1;
    }
    out
}

fn format_text(text: &str, parse_mode: Option<&str>) -> String {
    match parse_mode.map(|m| m.to_ascii_lowercase()) {
        Some(m) if m == "html" => from_html(text),
        Some(m) if m == "markdownv2" => from_markdown(text, true),
        Some(m) if m == "markdown" => from_markdown(text, false),
        _ => text.to_string(),
    }
}

fn keycap(n: usize) -> String {
    const CAPS: [&str; 10] = ["0️⃣", "1️⃣", "2️⃣", "3️⃣", "4️⃣", "5️⃣", "6️⃣", "7️⃣", "8️⃣", "9️⃣"];
    if n < 10 { CAPS[n].to_string() } else { format!("[{n}]") }
}

/// The keyboard as the app draws it (first line of the message):
/// `{"kind":"inline"|"reply"|"remove","rows":[[{"t":label,"cb"|"url"|"wa":…}]],…}`.
fn app_markup(markup: Option<&Value>, message_id: i64) -> Option<Value> {
    let m = match markup? {
        Value::String(s) => serde_json::from_str::<Value>(s).ok()?,
        v => v.clone(),
    };
    let cell = |b: &Value| -> Option<Value> {
        if let Some(t) = b.as_str() {
            return Some(json!({ "t": t }));
        }
        let t = b.get("text")?.as_str()?.chars().take(64).collect::<String>();
        let mut c = json!({ "t": t });
        if let Some(d) = b.get("callback_data").and_then(|d| d.as_str()) {
            c["cb"] = json!(d);
        } else if let Some(u) = b.get("url").or_else(|| b.get("login_url").and_then(|l| l.get("url"))).and_then(|u| u.as_str()) {
            c["url"] = json!(u);
        } else if let Some(u) = b.get("web_app").and_then(|w| w.get("url")).and_then(|u| u.as_str()) {
            c["wa"] = json!(u);
        } else if let Some(q) = b.get("switch_inline_query").or_else(|| b.get("switch_inline_query_current_chat")).and_then(|q| q.as_str()) {
            c["q"] = json!(q);
        } else if b.get("request_contact").and_then(|x| x.as_bool()) == Some(true) {
            c["contact"] = json!(true);
        } else if b.get("request_location").and_then(|x| x.as_bool()) == Some(true) {
            c["location"] = json!(true);
        }
        Some(c)
    };
    let rows = |key: &str| -> Option<Vec<Value>> {
        Some(
            m.get(key)?
                .as_array()?
                .iter()
                .take(16)
                .map(|row| Value::Array(row.as_array().into_iter().flatten().take(8).filter_map(cell).collect()))
                .filter(|r| r.as_array().is_some_and(|a| !a.is_empty()))
                .collect(),
        )
    };
    if let Some(r) = rows("inline_keyboard") {
        return Some(json!({ "kind": "inline", "m": message_id, "rows": r }));
    }
    if let Some(r) = rows("keyboard") {
        return Some(json!({
            "kind": "reply", "m": message_id, "rows": r,
            "once": m.get("one_time_keyboard").and_then(|x| x.as_bool()).unwrap_or(false),
            "ph": m.get("input_field_placeholder").and_then(|x| x.as_str()).unwrap_or(""),
        }));
    }
    if m.get("remove_keyboard").and_then(|x| x.as_bool()) == Some(true) {
        return Some(json!({ "kind": "remove", "m": message_id }));
    }
    None
}

/// Lists a reply_markup under the text; returns the answerable buttons.
fn render_markup(markup: Option<&Value>) -> (String, Vec<Button>) {
    let Some(m) = markup else { return (String::new(), Vec::new()) };
    let m = match m {
        Value::String(s) => serde_json::from_str::<Value>(s).unwrap_or(Value::Null),
        v => v.clone(),
    };
    let mut lines: Vec<String> = Vec::new();
    let mut buttons: Vec<Button> = Vec::new();
    if let Some(rows) = m.get("inline_keyboard").and_then(|r| r.as_array()) {
        for row in rows {
            let mut cells: Vec<String> = Vec::new();
            for b in row.as_array().into_iter().flatten() {
                let label = b.get("text").and_then(|t| t.as_str()).unwrap_or("").to_string();
                if let Some(url) = b.get("url").or_else(|| b.get("login_url").and_then(|l| l.get("url"))).and_then(|u| u.as_str()) {
                    cells.push(format!("🔗 [{label}]({url})"));
                } else if let Some(data) = b.get("callback_data").and_then(|d| d.as_str()) {
                    buttons.push(Button { label: label.clone(), data: data.to_string() });
                    cells.push(format!("{} {label}", keycap(buttons.len())));
                } else if let Some(url) = b.get("web_app").and_then(|w| w.get("url")).and_then(|u| u.as_str()) {
                    cells.push(format!("🌐 [{label}]({url})"));
                }
            }
            if !cells.is_empty() {
                lines.push(cells.join("   "));
            }
        }
        if !buttons.is_empty() {
            lines.push(if buttons.len() == 1 { "↩️ 1".to_string() } else { format!("↩️ 1–{}", buttons.len()) });
        }
    } else if let Some(rows) = m.get("keyboard").and_then(|r| r.as_array()) {
        for row in rows {
            let cells: Vec<String> = row
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|b| b.as_str().map(str::to_string).or_else(|| b.get("text").and_then(|t| t.as_str()).map(str::to_string)))
                .map(|l| format!("▫️ {l}"))
                .collect();
            if !cells.is_empty() {
                lines.push(cells.join("   "));
            }
        }
    }
    if lines.is_empty() {
        return (String::new(), buttons);
    }
    (format!("\n\n{}", lines.join("\n")), buttons)
}

// ------------------------------------------------------------------- HTTP glue

fn ok(result: Value) -> Response {
    Json(json!({ "ok": true, "result": result })).into_response()
}

fn fail(code: u16, description: &str) -> Response {
    let status = StatusCode::from_u16(code).unwrap_or(StatusCode::BAD_REQUEST);
    (status, Json(json!({ "ok": false, "error_code": code, "description": description }))).into_response()
}

fn fail_retry(after: u64) -> Response {
    (
        StatusCode::TOO_MANY_REQUESTS,
        Json(json!({
            "ok": false, "error_code": 429,
            "description": format!("Too Many Requests: retry after {after}"),
            "parameters": { "retry_after": after },
        })),
    )
        .into_response()
}

/// Query string + JSON / form / multipart body, as one parameter map.
async fn params(query: HashMap<String, String>, req: Request) -> Map<String, Value> {
    let mut p: Map<String, Value> = query.into_iter().map(|(k, v)| (k, Value::String(v))).collect();
    let ctype = req
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ctype.starts_with("multipart/form-data") {
        if let Ok(mut mp) = Multipart::from_request(req, &()).await {
            while let Ok(Some(field)) = mp.next_field().await {
                let name = field.name().unwrap_or("").to_string();
                let is_file = field.file_name().is_some();
                if let Ok(bytes) = field.bytes().await {
                    if is_file {
                        p.insert(name, Value::String("<uploaded file>".into()));
                    } else {
                        p.insert(name, Value::String(String::from_utf8_lossy(&bytes).into_owned()));
                    }
                }
            }
        }
        return p;
    }
    let Ok(body) = Bytes::from_request(req, &()).await else { return p };
    if body.is_empty() {
        return p;
    }
    if ctype.contains("json") || body.first() == Some(&b'{') {
        if let Ok(Value::Object(m)) = serde_json::from_slice::<Value>(&body) {
            p.extend(m);
        }
    } else {
        for (k, v) in form_urlencoded::parse(&body) {
            p.insert(k.into_owned(), Value::String(v.into_owned()));
        }
    }
    p
}

fn s<'a>(p: &'a Map<String, Value>, k: &str) -> Option<&'a str> {
    p.get(k).and_then(|v| v.as_str())
}

fn num(p: &Map<String, Value>, k: &str) -> Option<i64> {
    match p.get(k)? {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn flag(p: &Map<String, Value>, k: &str) -> bool {
    match p.get(k) {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => matches!(s.as_str(), "true" | "1" | "True"),
        _ => false,
    }
}

/// `bot<token>` path segment → the bot's username.
async fn auth(state: &AppState, seg: &str) -> Option<String> {
    let token = seg.strip_prefix("bot")?;
    // Telegram-shaped "<digits>:<token>": the number is decoration.
    let token = match token.split_once(':') {
        Some((n, t)) if n.chars().all(|c| c.is_ascii_digit()) => t,
        _ => token,
    };
    let raw = seg.strip_prefix("bot").unwrap_or("").to_string();
    let token = if token.starts_with("aqbot_") { token.to_string() } else { format!("aqbot_{token}") };
    match state.directory.bot_by_token(&token) {
        Some(b) => {
            // Mini apps check initData with the token exactly as the bot has it.
            if !state.tg.webapp_keys.contains_key(&b) {
                state.tg.webapp_keys.insert(b.clone(), webapp_key(&raw));
            }
            Some(b)
        }
        None => {
            tokio::time::sleep(Duration::from_millis(300)).await;
            None
        }
    }
}

fn hmac(key: &[u8], msg: &[u8]) -> Vec<u8> {
    use hmac::{Hmac, Mac};
    let mut m = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("hmac key");
    m.update(msg);
    m.finalize().into_bytes().to_vec()
}

/// Telegram: secret_key = HMAC_SHA256(key = "WebAppData", msg = bot_token).
fn webapp_key(token: &str) -> Vec<u8> {
    hmac(b"WebAppData", token.as_bytes())
}

/// Signed `initData` for a mini app of `bot` opened by `user` (Telegram's
/// format: url-encoded fields + `hash` over the sorted "k=v" lines).
pub fn init_data(state: &AppState, bot: &str, user: &str, start_param: Option<&str>) -> (String, bool) {
    let mut q = [0u8; 12];
    rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut q);
    let query_id = format!("AA{}", hex::encode(q));
    state.tg.queries.retain(|_, v| v.2.elapsed() < Duration::from_secs(3600));
    state.tg.queries.insert(query_id.clone(), (bot.to_string(), user.to_string(), std::time::Instant::now()));
    let display = state.directory.display_name_of(user);
    let u = json!({
        "id": remember(state, user),
        "first_name": if display.trim().is_empty() { user.to_string() } else { display },
        "username": user,
        "language_code": "ru",
        "allows_write_to_pm": true,
    });
    let mut fields: Vec<(String, String)> = vec![
        ("auth_date".into(), now().to_string()),
        ("query_id".into(), query_id),
        ("user".into(), u.to_string()),
    ];
    if let Some(sp) = start_param.filter(|s| !s.is_empty()) {
        fields.push(("start_param".into(), sp.chars().take(512).collect()));
    }
    fields.sort();
    let check: String = fields.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("\n");
    let key = state.tg.webapp_keys.get(bot).map(|k| k.clone());
    let signed = key.is_some();
    if let Some(key) = key {
        fields.push(("hash".into(), hex::encode(hmac(&key, check.as_bytes()))));
    }
    let enc: String = form_urlencoded::Serializer::new(String::new()).extend_pairs(fields.iter()).finish();
    // The user opened the bot's app: the bot may write to them (Telegram's
    // allows_write_to_pm).
    state.directory.bot_peer_add(bot, user);
    (enc, signed)
}

/// The bot's menu button, if it set a mini app one.
pub fn menu_button(state: &AppState, bot: &str) -> Value {
    state.tg.menus.get(bot).map(|m| m.clone()).unwrap_or(json!({ "type": "default" }))
}

/// `ANY /:bot_token/:method` — the Telegram Bot API entry point.
pub async fn handle(
    State(state): State<AppState>,
    Path((seg, method)): Path<(String, String)>,
    Query(query): Query<HashMap<String, String>>,
    req: Request,
) -> Response {
    if !seg.starts_with("bot") {
        return fail(404, "Not Found");
    }
    let Some(bot) = auth(&state, &seg).await else {
        return fail(401, "Unauthorized");
    };
    let p = params(query, req).await;
    let m = method.to_ascii_lowercase();
    match m.as_str() {
        "getme" => {
            let mut me = user_json(&state, &bot, true);
            me["can_join_groups"] = json!(false);
            me["can_read_all_group_messages"] = json!(false);
            me["supports_inline_queries"] = json!(false);
            me["can_connect_to_business"] = json!(false);
            me["has_main_web_app"] = json!(false);
            ok(me)
        }
        "getupdates" => get_updates(&state, &bot, &p).await,
        "sendmessage" => {
            let text = s(&p, "text").unwrap_or("").to_string();
            send(&state, &bot, &p, &text).await
        }
        "editmessagetext" => {
            let text = s(&p, "text").unwrap_or("").to_string();
            send(&state, &bot, &p, &text).await
        }
        "editmessagecaption" => {
            let text = s(&p, "caption").unwrap_or("").to_string();
            send(&state, &bot, &p, &text).await
        }
        "editmessagereplymarkup" => {
            if p.get("reply_markup").is_some() {
                send(&state, &bot, &p, "").await
            } else {
                ok(json!(true))
            }
        }
        "sendphoto" | "senddocument" | "sendvideo" | "sendaudio" | "sendvoice" | "sendanimation" | "sendvideonote"
        | "sendsticker" | "sendmediagroup" => {
            let kind = m.trim_start_matches("send");
            let icon = match kind {
                "photo" | "mediagroup" => "🖼",
                "video" | "animation" | "videonote" => "🎬",
                "audio" | "voice" => "🎵",
                "sticker" => "🌟",
                _ => "📎",
            };
            let field = match kind { "videonote" => "video_note", "mediagroup" => "media", k => k };
            let link = s(&p, field).filter(|v| v.starts_with("http")).map(|u| format!("\n{u}")).unwrap_or_default();
            let caption = s(&p, "caption").unwrap_or("");
            let text = format!("{icon} {caption}{link}");
            send(&state, &bot, &p, text.trim()).await
        }
        "sendlocation" | "sendvenue" => {
            let (lat, lon) = (s_or_num(&p, "latitude"), s_or_num(&p, "longitude"));
            let title = s(&p, "title").map(|t| format!("{t}\n")).unwrap_or_default();
            let text = format!("📍 {title}https://www.openstreetmap.org/?mlat={lat}&mlon={lon}");
            send(&state, &bot, &p, &text).await
        }
        "sendcontact" => {
            let text = format!("👤 {} {}", s(&p, "first_name").unwrap_or(""), s(&p, "phone_number").unwrap_or(""));
            send(&state, &bot, &p, text.trim()).await
        }
        "senddice" => {
            let n = (now() % 6) + 1;
            send(&state, &bot, &p, &format!("{} {n}", s(&p, "emoji").unwrap_or("🎲"))).await
        }
        "sendpoll" => {
            let q = s(&p, "question").unwrap_or("");
            let opts = match p.get("options") {
                Some(Value::String(js)) => serde_json::from_str::<Value>(js).unwrap_or(Value::Null),
                Some(v) => v.clone(),
                None => Value::Null,
            };
            let list: Vec<String> = opts
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
                .map(|(i, o)| format!("{} {}", keycap(i + 1), o.as_str().or_else(|| o.get("text").and_then(|t| t.as_str())).unwrap_or("")))
                .collect();
            send(&state, &bot, &p, &format!("📊 **{q}**\n{}", list.join("\n"))).await
        }
        "answercallbackquery" => {
            if let (Some(text), Some(id)) = (s(&p, "text").filter(|t| !t.trim().is_empty()), s(&p, "callback_query_id")) {
                let chat = id.split('-').next().and_then(|n| n.parse::<i64>().ok());
                if let Some(user) = chat.and_then(|c| state.tg.names.get(&c).map(|n| n.clone())) {
                    let prefix = if flag(&p, "show_alert") { "⚠️ " } else { "ℹ️ " };
                    crate::bot_deliver_text(&state, &bot, &user, &format!("{prefix}{text}")).await;
                }
            }
            ok(json!(true))
        }
        "sendchataction" | "deletemessage" | "deletemessages" | "setmydescription" | "setmyshortdescription"
        | "setmyname" | "setmydefaultadministratorrights" | "setmessagereaction" | "close"
        | "logout" | "pinchatmessage" | "unpinchatmessage" | "unpinallchatmessages" => ok(json!(true)),
        "setmycommands" => {
            let cmds = match p.get("commands") {
                Some(Value::String(js)) => serde_json::from_str::<Value>(js).unwrap_or(json!([])),
                Some(v) => v.clone(),
                None => json!([]),
            };
            state.tg.commands.insert(bot.clone(), cmds);
            ok(json!(true))
        }
        "getmycommands" => ok(state.tg.commands.get(&bot).map(|c| c.clone()).unwrap_or(json!([]))),
        "deletemycommands" => {
            state.tg.commands.remove(&bot);
            ok(json!(true))
        }
        "getmydescription" => ok(json!({ "description": "" })),
        "getmyshortdescription" => ok(json!({ "short_description": "" })),
        "getmyname" => ok(json!({ "name": state.directory.display_name_of(&bot) })),
        "getchatmenubutton" => ok(menu_button(&state, &bot)),
        "setchatmenubutton" => {
            let mb = match p.get("menu_button") {
                Some(Value::String(js)) => serde_json::from_str::<Value>(js).unwrap_or(Value::Null),
                Some(v) => v.clone(),
                None => json!({ "type": "default" }),
            };
            if mb.get("type").and_then(|t| t.as_str()) == Some("web_app") {
                state.tg.menus.insert(bot.clone(), mb);
            } else {
                state.tg.menus.remove(&bot);
            }
            ok(json!(true))
        }
        "answerwebappquery" => {
            let Some(qid) = s(&p, "web_app_query_id") else { return fail(400, "Bad Request: web_app_query_id is required") };
            let Some((qbot, user, _)) = state.tg.queries.get(qid).map(|q| q.clone()) else {
                return fail(400, "Bad Request: query is too old or query ID is invalid");
            };
            if qbot != bot {
                return fail(400, "Bad Request: query ID is invalid");
            }
            let result = match p.get("result") {
                Some(Value::String(js)) => serde_json::from_str::<Value>(js).unwrap_or(Value::Null),
                Some(v) => v.clone(),
                None => Value::Null,
            };
            let content = result.get("input_message_content");
            let text = content.and_then(|c| c.get("message_text")).and_then(|t| t.as_str())
                .or_else(|| result.get("title").and_then(|t| t.as_str()))
                .unwrap_or("");
            let body = format_text(text, content.and_then(|c| c.get("parse_mode")).and_then(|x| x.as_str()));
            if !body.trim().is_empty() {
                crate::bot_deliver_text(&state, &bot, &user, &body).await;
            }
            ok(json!({ "inline_message_id": qid }))
        }
        "getchat" => match resolve_chat(&state, &bot, p.get("chat_id")) {
            Some(u) => ok(chat_json(&state, &u)),
            None => fail(400, "Bad Request: chat not found"),
        },
        "getchatmember" => match resolve_chat(&state, &bot, p.get("chat_id")) {
            Some(u) => ok(json!({ "status": "member", "user": user_json(&state, &u, false) })),
            None => fail(400, "Bad Request: chat not found"),
        },
        "getuserprofilephotos" => ok(json!({ "total_count": 0, "photos": [] })),
        "setwebhook" => set_webhook(&state, &bot, &p).await,
        "deletewebhook" => {
            stop_pusher(&state, &bot);
            {
                let mut w = state.tg.webhooks.write().unwrap();
                w.remove(&bot);
            }
            state.tg.save_webhooks();
            if flag(&p, "drop_pending_updates") {
                drop_pending(&state, &bot).await;
            }
            ok(json!(true))
        }
        "getwebhookinfo" => {
            let w = state.tg.webhooks.read().unwrap().get(&bot).cloned();
            let pending = state.bot_queue(&bot).lock().await.updates.len();
            let mut info = json!({
                "url": w.as_ref().map(|w| w.url.clone()).unwrap_or_default(),
                "has_custom_certificate": false,
                "pending_update_count": pending,
                "max_connections": 1,
            });
            if let Some((at, msg)) = w.and_then(|w| w.last_error) {
                info["last_error_date"] = json!(at);
                info["last_error_message"] = json!(msg);
            }
            ok(info)
        }
        "forwardmessage" | "copymessage" | "forwardmessages" | "copymessages" => {
            fail(400, "Bad Request: message can't be forwarded (not supported by Aquamarine yet)")
        }
        "getfile" => fail(400, "Bad Request: file is not available (not supported by Aquamarine yet)"),
        _ => fail(404, "Not Found: method not supported by Aquamarine"),
    }
}

fn s_or_num(p: &Map<String, Value>, k: &str) -> String {
    match p.get(k) {
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::String(s)) => s.clone(),
        _ => "0".into(),
    }
}

/// sendMessage & co.: format, list the keyboard, deliver, answer a Message.
async fn send(state: &AppState, bot: &str, p: &Map<String, Value>, text: &str) -> Response {
    let Some(to) = resolve_chat(state, bot, p.get("chat_id")) else {
        return fail(400, "Bad Request: chat not found");
    };
    let body = format_text(text, s(p, "parse_mode"));
    let (kb_text, buttons) = render_markup(p.get("reply_markup"));
    let full = format!("{body}{kb_text}");
    if full.trim().is_empty() {
        return fail(400, "Bad Request: message text is empty");
    }
    if full.chars().count() > BOT_TEXT_LIMIT + 512 {
        return fail(400, "Bad Request: message is too long");
    }
    if to != bot && !state.directory.bot_may_message(bot, &to) {
        return fail(403, "Forbidden: bot can't initiate conversation with a user");
    }
    if !state.bot_limits.take(&format!("send:{bot}"), 30.0, 30.0) {
        return fail_retry(1);
    }
    if !state.bot_limits.take(&format!("send:{bot}>{to}"), 5.0, 1.0) {
        return fail_retry(1);
    }
    let message_id = state.tg.next_message_id(bot);
    let app_kb = app_markup(p.get("reply_markup"), message_id);
    if !buttons.is_empty() {
        state.tg.keyboards.insert(
            (bot.to_string(), to.clone()),
            Keyboard { buttons, message_id, text: body.clone(), at: std::time::Instant::now() },
        );
    }
    let mut text: String = full.chars().take(BOT_TEXT_LIMIT + 512).collect();
    if let Some(kb) = app_kb {
        // The app draws the buttons from the first line and hides the
        // numbered list after the invisible separator.
        let listing = text.get(body.len()..).unwrap_or("").to_string();
        let head: String = body.chars().take(BOT_TEXT_LIMIT).collect();
        text = format!("{KB_PREFIX}{kb}\n{head}\u{2063}{listing}");
    }
    crate::bot_deliver_text(state, bot, &to, &text).await;
    ok(json!({
        "message_id": message_id,
        "from": user_json(state, bot, true),
        "chat": chat_json(state, &to),
        "date": now(),
        "text": body,
    }))
}

async fn get_updates(state: &AppState, bot: &str, p: &Map<String, Value>) -> Response {
    if state.tg.webhooks.read().unwrap().contains_key(bot) {
        return fail(409, "Conflict: can't use getUpdates method while webhook is active; use deleteWebhook to delete the webhook first");
    }
    if !state.bot_limits.take(&format!("poll:{bot}"), 20.0, 5.0) {
        return fail_retry(1);
    }
    let offset = num(p, "offset").unwrap_or(0);
    let limit = num(p, "limit").unwrap_or(100).clamp(1, 100) as usize;
    let timeout = num(p, "timeout").unwrap_or(0).clamp(0, 50) as u64;
    let queue = state.bot_queue(bot);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout);
    loop {
        let notified = {
            let mut q = queue.lock().await;
            if offset > 0 {
                // update_id = queue id + 1; asking for `offset` confirms the ones before.
                let after = (offset - 1) as u64;
                while q.updates.front().is_some_and(|u| u.id < after) {
                    q.updates.pop_front();
                }
            } else if offset < 0 {
                let keep = (-offset) as usize;
                while q.updates.len() > keep {
                    q.updates.pop_front();
                }
            }
            if !q.updates.is_empty() || tokio::time::Instant::now() >= deadline {
                let list: Vec<Value> = q.updates.iter().take(limit).map(|u| update_json(state, bot, u)).collect();
                return ok(Value::Array(list));
            }
            q.notify.clone()
        };
        let _ = tokio::time::timeout_at(deadline, notified.notified()).await;
    }
}

async fn drop_pending(state: &AppState, bot: &str) {
    let q = state.bot_queue(bot);
    q.lock().await.updates.clear();
}

// ------------------------------------------------------------------- webhooks

fn webhook_url_ok(url: &str) -> bool {
    let Ok(u) = reqwest::Url::parse(url) else { return false };
    if !matches!(u.scheme(), "http" | "https") {
        return false;
    }
    // Never the cloud metadata service.
    !matches!(u.host_str(), Some(h) if h.starts_with("169.254.") || h == "metadata.google.internal")
}

async fn set_webhook(state: &AppState, bot: &str, p: &Map<String, Value>) -> Response {
    let url = s(p, "url").unwrap_or("").trim().to_string();
    if url.is_empty() {
        stop_pusher(state, bot);
        state.tg.webhooks.write().unwrap().remove(bot);
        state.tg.save_webhooks();
        return ok(json!(true));
    }
    if !webhook_url_ok(&url) {
        return fail(400, "Bad Request: bad webhook: an HTTP(S) URL is required");
    }
    let secret = s(p, "secret_token").unwrap_or("").to_string();
    if flag(p, "drop_pending_updates") {
        drop_pending(state, bot).await;
    }
    state.tg.webhooks.write().unwrap().insert(bot.to_string(), Webhook { url, secret, last_error: None });
    state.tg.save_webhooks();
    start_pusher(state.clone(), bot.to_string());
    ok(json!(true))
}

fn stop_pusher(state: &AppState, bot: &str) {
    if let Some((_, h)) = state.tg.pushers.remove(bot) {
        h.abort();
    }
}

/// Deliver the bot's updates to its webhook, one POST per update, in order.
fn start_pusher(state: AppState, bot: String) {
    stop_pusher(&state, &bot);
    let st = state.clone();
    let b = bot.clone();
    let task = tokio::spawn(async move {
        let http = reqwest::Client::builder().timeout(Duration::from_secs(30)).build().unwrap_or_default();
        let mut backoff = 1u64;
        loop {
            let Some(hook) = st.tg.webhooks.read().unwrap().get(&b).cloned() else { return };
            let queue = st.bot_queue(&b);
            let (next, notify) = {
                let q = queue.lock().await;
                (q.updates.front().cloned(), q.notify.clone())
            };
            let Some(u) = next else {
                let _ = tokio::time::timeout(Duration::from_secs(25), notify.notified()).await;
                continue;
            };
            let body = update_json(&st, &b, &u);
            let mut req = http.post(&hook.url).json(&body);
            if !hook.secret.is_empty() {
                req = req.header("X-Telegram-Bot-Api-Secret-Token", &hook.secret);
            }
            match req.send().await {
                Ok(r) if r.status().is_success() => {
                    backoff = 1;
                    let mut q = queue.lock().await;
                    if q.updates.front().is_some_and(|x| x.id == u.id) {
                        q.updates.pop_front();
                    }
                }
                other => {
                    let msg = match other {
                        Ok(r) => format!("Wrong response from the webhook: {}", r.status()),
                        Err(e) => format!("Connection failed: {e}"),
                    };
                    tracing::warn!(bot = %b, %msg, "webhook delivery failed");
                    if let Some(w) = st.tg.webhooks.write().unwrap().get_mut(&b) {
                        w.last_error = Some((now(), msg));
                    }
                    tokio::time::sleep(Duration::from_secs(backoff)).await;
                    backoff = (backoff * 2).min(60);
                }
            }
        }
    });
    state.tg.pushers.insert(bot, task.abort_handle());
}

/// Restart the webhook pushers saved before a restart.
pub fn resume_webhooks(state: AppState) {
    let bots: Vec<String> = state.tg.webhooks.read().unwrap().keys().cloned().collect();
    for b in bots {
        start_pusher(state.clone(), b);
    }
}

/// The Telegram-style token shown to bot owners: `<id>:<token>`.
pub fn telegram_token(bot: &str, token: &str) -> String {
    format!("{}:{}", tg_id(bot), token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_and_markdown_become_aquamarine_markup() {
        assert_eq!(from_html("<b>Hi</b> <i>there</i> &lt;3"), "**Hi** __there__ <3");
        assert_eq!(from_html(r#"<a href="https://x.y/?a=1&amp;b=2">link</a>"#), "[link](https://x.y/?a=1&b=2)");
        assert_eq!(from_html("<code>x</code><br><s>no</s>"), "`x`\n~~no~~");
        assert_eq!(from_markdown(r"*bold* _it_ __u__ ~s~ \. `a_b*c`", true), "**bold** __it__ ++u++ ~~s~~ . `a_b*c`");
        assert_eq!(from_markdown("*b* _i_", false), "**b** __i__");
    }

    #[test]
    fn inline_keyboard_is_listed_and_answerable() {
        let markup = json!({ "inline_keyboard": [
            [{ "text": "Yes", "callback_data": "y" }, { "text": "No", "callback_data": "n" }],
            [{ "text": "Site", "url": "https://example.com" }],
        ]});
        let (text, buttons) = render_markup(Some(&markup));
        assert!(text.contains("1️⃣ Yes") && text.contains("2️⃣ No") && text.contains("[Site](https://example.com)"));
        assert_eq!(buttons.len(), 2);
        // Form-encoded bodies carry reply_markup as a JSON string.
        let (_, b2) = render_markup(Some(&Value::String(markup.to_string())));
        assert_eq!(b2.len(), 2);
    }

    #[test]
    fn app_keyboard_and_callbacks() {
        let markup = json!({ "inline_keyboard": [[{ "text": "Yes", "callback_data": "y" }, { "text": "App", "web_app": { "url": "https://a.b" } }]] });
        let kb = app_markup(Some(&markup), 7).unwrap();
        assert_eq!(kb["kind"], "inline");
        assert_eq!(kb["rows"][0][0]["cb"], "y");
        assert_eq!(kb["rows"][0][1]["wa"], "https://a.b");
        let reply = app_markup(Some(&json!({ "keyboard": [["A", { "text": "B" }]], "one_time_keyboard": true })), 8).unwrap();
        assert_eq!(reply["kind"], "reply");
        assert_eq!(reply["once"], true);
        assert_eq!(reply["rows"][0][1]["t"], "B");
        assert_eq!(app_markup(Some(&json!({ "remove_keyboard": true })), 9).unwrap()["kind"], "remove");
        assert_eq!(web_app_data(r#"[wad:v1]{"t":"Go","d":"42"}"#), Some(("42".into(), "Go".into())));
    }

    #[test]
    fn webapp_signature_matches_telegram() {
        // Telegram's check: hex(HMAC(HMAC("WebAppData", token), data_check_string)).
        let key = webapp_key("123:ABC");
        let sig = hex::encode(hmac(&key, b"auth_date=1\nquery_id=AA\nuser={}"));
        assert_eq!(sig.len(), 64);
        assert_ne!(webapp_key("123:ABC"), webapp_key("aqbot_ABC"));
    }

    #[test]
    fn command_entities_and_ids() {
        assert_eq!(entities("/start abc").unwrap()[0]["length"], 6);
        assert!(entities("hello").is_none());
        let id = tg_id("alice");
        assert!(id > 0 && id < (1i64 << 48));
        assert_eq!(id, tg_id("alice"));
    }
}
