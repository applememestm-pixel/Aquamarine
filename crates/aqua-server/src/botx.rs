//! Native Bot API, the convenient parts (`Authorization: Bot aqbot_…`):
//!
//! | Method | Path | What |
//! |---|---|---|
//! | POST | /v1/bot/call/{method} | any Telegram Bot API method, JSON in, `{ok, result}` out |
//! | POST | /v1/bot/send | (lib.rs) + `parse_mode`, `buttons`, `keyboard`, `remove_keyboard` |
//! | POST | /v1/bot/edit | new version of a message (with buttons) |
//! | POST | /v1/bot/broadcast | one text to every contact, paced by the rate limits |
//! | GET  | /v1/bot/contacts | who may be written to |
//! | GET  | /v1/bot/user/{name} | a user's public card + whether they're a contact |
//! | GET/POST/DELETE | /v1/bot/commands | the "/" command menu |
//! | GET/POST/DELETE | /v1/bot/webhook | push updates to a URL instead of polling |
//! | GET/POST | /v1/bot/profile | description / short description |
//! | GET  | /v1/bot/stats | contacts, waiting updates, webhook, commands |

use axum::{
    body::to_bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::{bot_auth, tgapi, AppState};

type Err = (StatusCode, String);

/// Runs a Telegram method and unwraps `{ok, result}` into a plain result.
pub(crate) async fn tg(state: &AppState, bot: &str, method: &str, p: Map<String, Value>) -> Result<Value, Err> {
    let resp = tgapi::dispatch(state, bot, method, p).await;
    let status = resp.status();
    let body = to_bytes(resp.into_body(), 4 << 20).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let v: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    if v.get("ok").and_then(|o| o.as_bool()) == Some(true) {
        Ok(v.get("result").cloned().unwrap_or(Value::Null))
    } else {
        let msg = v.get("description").and_then(|d| d.as_str()).unwrap_or("error").to_string();
        Err((if status.is_success() { StatusCode::BAD_REQUEST } else { status }, msg))
    }
}

/// `buttons`: rows of `{"text", "data" | "url" | "app"}` (or plain strings,
/// which send their own text back) → Telegram `inline_keyboard`.
fn inline_keyboard(rows: &Value) -> Value {
    let row = |r: &Value| -> Vec<Value> {
        r.as_array()
            .into_iter()
            .flatten()
            .filter_map(|b| {
                if let Some(t) = b.as_str() {
                    return Some(json!({ "text": t, "callback_data": t }));
                }
                let text = b.get("text")?.as_str()?;
                if let Some(u) = b.get("url").and_then(|v| v.as_str()) {
                    Some(json!({ "text": text, "url": u }))
                } else if let Some(u) = b.get("app").and_then(|v| v.as_str()) {
                    Some(json!({ "text": text, "web_app": { "url": u } }))
                } else {
                    let d = b.get("data").and_then(|v| v.as_str()).unwrap_or(text);
                    Some(json!({ "text": text, "callback_data": d }))
                }
            })
            .collect()
    };
    // One flat row is accepted too: ["Yes", "No"].
    let rows: Vec<Value> = match rows.as_array() {
        Some(a) if a.iter().all(|x| !x.is_array()) => vec![Value::Array(a.clone())],
        Some(a) => a.clone(),
        None => vec![],
    };
    json!({ "inline_keyboard": rows.iter().map(|r| row(r)).filter(|r: &Vec<Value>| !r.is_empty()).collect::<Vec<_>>() })
}

/// Reply keyboard over the message box: rows of button texts.
fn reply_keyboard(rows: &Value, once: bool) -> Value {
    let rows: Vec<Value> = match rows.as_array() {
        Some(a) if a.iter().all(|x| !x.is_array()) => vec![Value::Array(a.clone())],
        Some(a) => a.clone(),
        None => vec![],
    };
    let rows: Vec<Vec<Value>> = rows
        .iter()
        .map(|r| r.as_array().into_iter().flatten().filter_map(|b| b.as_str().map(|t| json!({ "text": t }))).collect())
        .collect();
    json!({ "keyboard": rows, "resize_keyboard": true, "one_time_keyboard": once })
}

/// Fields of a rich message (send / edit / broadcast).
#[derive(Deserialize, Default, Clone)]
pub struct Rich {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub parse_mode: Option<String>,
    #[serde(default)]
    pub buttons: Option<Value>,
    #[serde(default)]
    pub keyboard: Option<Value>,
    #[serde(default)]
    pub one_time: bool,
    #[serde(default)]
    pub remove_keyboard: bool,
    /// Raw Telegram reply_markup, if you have one already.
    #[serde(default)]
    pub reply_markup: Option<Value>,
}

impl Rich {
    pub(crate) fn is_rich(&self) -> bool {
        self.parse_mode.is_some() || self.buttons.is_some() || self.keyboard.is_some() || self.remove_keyboard || self.reply_markup.is_some()
    }

    fn params(&self, to: &str) -> Map<String, Value> {
        let mut p = Map::new();
        p.insert("chat_id".into(), json!(format!("@{}", to.trim().trim_start_matches('@'))));
        p.insert("text".into(), json!(self.text));
        if let Some(m) = &self.parse_mode {
            p.insert("parse_mode".into(), json!(m));
        }
        let markup = if let Some(m) = &self.reply_markup {
            Some(m.clone())
        } else if let Some(b) = &self.buttons {
            Some(inline_keyboard(b))
        } else if let Some(k) = &self.keyboard {
            Some(reply_keyboard(k, self.one_time))
        } else if self.remove_keyboard {
            Some(json!({ "remove_keyboard": true }))
        } else {
            None
        };
        if let Some(m) = markup {
            p.insert("reply_markup".into(), m);
        }
        p
    }
}

/// `POST /v1/bot/send` with formatting or buttons → `{message_id}`.
pub(crate) async fn send_rich(state: &AppState, bot: &str, to: &str, rich: &Rich) -> Result<Value, Err> {
    let r = tg(state, bot, "sendMessage", rich.params(to)).await?;
    Ok(json!({ "message_id": r.get("message_id").cloned().unwrap_or(Value::Null) }))
}

/// `POST /v1/bot/call/{method}` — the whole Telegram Bot API through the
/// native auth header; the answer is Telegram's own `{ok, result}`.
pub async fn call(State(state): State<AppState>, headers: HeaderMap, Path(method): Path<String>, body: Option<Json<Value>>) -> Response {
    let bot = match bot_auth(&state, &headers, "").await {
        Ok(b) => b,
        Err(e) => return e.into_response(),
    };
    let p = match body.map(|b| b.0) {
        Some(Value::Object(m)) => m,
        _ => Map::new(),
    };
    tgapi::dispatch(&state, &bot, &method, p).await
}

#[derive(Deserialize)]
pub struct EditBody {
    to: String,
    #[serde(default)]
    message_id: Option<i64>,
    #[serde(flatten)]
    rich: Rich,
}

/// `POST /v1/bot/edit` — Aquamarine shows edits as a new version of the message.
pub async fn edit(State(state): State<AppState>, headers: HeaderMap, Json(b): Json<EditBody>) -> Result<Json<Value>, Err> {
    let bot = bot_auth(&state, &headers, "").await?;
    let mut p = b.rich.params(&b.to);
    if let Some(id) = b.message_id {
        p.insert("message_id".into(), json!(id));
    }
    let r = tg(&state, &bot, "editMessageText", p).await?;
    Ok(Json(json!({ "message_id": r.get("message_id").cloned().unwrap_or(Value::Null) })))
}

/// `POST /v1/bot/broadcast` — one message to every contact. Runs in the
/// background at the bot's rate limit; answers at once with how many.
pub async fn broadcast(State(state): State<AppState>, headers: HeaderMap, Json(rich): Json<Rich>) -> Result<(StatusCode, Json<Value>), Err> {
    let bot = bot_auth(&state, &headers, "").await?;
    if rich.text.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "empty text".into()));
    }
    if rich.text.chars().count() > crate::BOT_TEXT_LIMIT {
        return Err((StatusCode::PAYLOAD_TOO_LARGE, format!("text is longer than {} characters", crate::BOT_TEXT_LIMIT)));
    }
    let peers = state.directory.bot_peers_of(&bot);
    let n = peers.len();
    let st = state.clone();
    tokio::spawn(async move {
        for to in peers {
            for attempt in 0..5 {
                match tg(&st, &bot, "sendMessage", rich.params(&to)).await {
                    Err((StatusCode::TOO_MANY_REQUESTS, _)) if attempt < 4 => tokio::time::sleep(std::time::Duration::from_millis(1100)).await,
                    _ => break,
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        }
    });
    Ok((StatusCode::ACCEPTED, Json(json!({ "queued": n }))))
}

fn card(state: &AppState, name: &str) -> Value {
    json!({
        "username": name,
        "display_name": state.directory.display_name_of(name),
        "id": tgapi::tg_id(name),
    })
}

/// `GET /v1/bot/contacts` — everyone who wrote to the bot and didn't /stop.
pub async fn contacts(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<Value>, Err> {
    let bot = bot_auth(&state, &headers, "").await?;
    let list: Vec<Value> = state.directory.bot_peers_of(&bot).iter().map(|u| card(&state, u)).collect();
    Ok(Json(Value::Array(list)))
}

/// `GET /v1/bot/user/{name}`
pub async fn user(State(state): State<AppState>, headers: HeaderMap, Path(name): Path<String>) -> Result<Json<Value>, Err> {
    let bot = bot_auth(&state, &headers, "").await?;
    let name = name.trim().trim_start_matches('@').to_lowercase();
    let known = state.directory.user_exists(&name) || state.bindings.contains_key(&name) || state.directory.bot_may_message(&bot, &name);
    if !known {
        return Err((StatusCode::NOT_FOUND, "no such user".into()));
    }
    let mut c = card(&state, &name);
    c["is_bot"] = json!(state.directory.is_bot(&name));
    c["is_contact"] = json!(state.directory.bot_may_message(&bot, &name));
    Ok(Json(c))
}

/// `GET /v1/bot/commands` → `[{command, description}]`
pub async fn commands_get(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<Value>, Err> {
    let bot = bot_auth(&state, &headers, "").await?;
    Ok(Json(state.tg.commands.get(&bot).map(|c| c.clone()).unwrap_or(json!([]))))
}

/// `POST /v1/bot/commands` — `[{command, description}]` or `{"help": "…", …}`.
pub async fn commands_set(State(state): State<AppState>, headers: HeaderMap, Json(v): Json<Value>) -> Result<StatusCode, Err> {
    let bot = bot_auth(&state, &headers, "").await?;
    let list: Vec<Value> = match v {
        Value::Array(a) => a,
        Value::Object(m) => m.into_iter().map(|(k, d)| json!({ "command": k, "description": d.as_str().unwrap_or("") })).collect(),
        _ => return Err((StatusCode::BAD_REQUEST, "expected a list or an object".into())),
    };
    let clean: Vec<Value> = list
        .into_iter()
        .filter_map(|c| {
            let cmd = c.get("command")?.as_str()?.trim().trim_start_matches('/').to_lowercase();
            let ok = !cmd.is_empty() && cmd.len() <= 32 && cmd.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_');
            ok.then(|| json!({ "command": cmd, "description": c.get("description").and_then(|d| d.as_str()).unwrap_or("").chars().take(256).collect::<String>() }))
        })
        .take(100)
        .collect();
    state.tg.commands.insert(bot, Value::Array(clean));
    Ok(StatusCode::NO_CONTENT)
}

pub async fn commands_delete(State(state): State<AppState>, headers: HeaderMap) -> Result<StatusCode, Err> {
    let bot = bot_auth(&state, &headers, "").await?;
    state.tg.commands.remove(&bot);
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /v1/bot/webhook` → Telegram's getWebhookInfo.
pub async fn webhook_get(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<Value>, Err> {
    let bot = bot_auth(&state, &headers, "").await?;
    Ok(Json(tg(&state, &bot, "getWebhookInfo", Map::new()).await?))
}

#[derive(Deserialize)]
pub struct HookBody {
    url: String,
    #[serde(default)]
    secret: Option<String>,
    #[serde(default)]
    drop_pending: bool,
}

/// `POST /v1/bot/webhook` `{url, secret?, drop_pending?}`
pub async fn webhook_set(State(state): State<AppState>, headers: HeaderMap, Json(b): Json<HookBody>) -> Result<StatusCode, Err> {
    let bot = bot_auth(&state, &headers, "").await?;
    let mut p = Map::new();
    p.insert("url".into(), json!(b.url));
    if let Some(s) = b.secret {
        p.insert("secret_token".into(), json!(s));
    }
    p.insert("drop_pending_updates".into(), json!(b.drop_pending));
    tg(&state, &bot, "setWebhook", p).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn webhook_delete(State(state): State<AppState>, headers: HeaderMap) -> Result<StatusCode, Err> {
    let bot = bot_auth(&state, &headers, "").await?;
    tg(&state, &bot, "deleteWebhook", Map::new()).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /v1/bot/profile` → `{username, display_name, description, short_description}`
pub async fn profile_get(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<Value>, Err> {
    let bot = bot_auth(&state, &headers, "").await?;
    let (d, sd) = state.tg.about.get(&bot).map(|a| a.clone()).unwrap_or_default();
    Ok(Json(json!({
        "username": bot,
        "display_name": state.directory.display_name_of(&bot),
        "description": d,
        "short_description": sd,
    })))
}

#[derive(Deserialize)]
pub struct ProfileBody {
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    short_description: Option<String>,
}

/// `POST /v1/bot/profile` `{description?, short_description?}`
pub async fn profile_set(State(state): State<AppState>, headers: HeaderMap, Json(b): Json<ProfileBody>) -> Result<StatusCode, Err> {
    let bot = bot_auth(&state, &headers, "").await?;
    let mut e = state.tg.about.entry(bot).or_default();
    if let Some(d) = b.description {
        e.0 = d.chars().take(512).collect();
    }
    if let Some(d) = b.short_description {
        e.1 = d.chars().take(120).collect();
    }
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /v1/bot/stats`
pub async fn stats(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<Value>, Err> {
    let bot = bot_auth(&state, &headers, "").await?;
    let pending = state.bot_queue(&bot).lock().await.updates.len();
    let webhook = state.tg.webhooks.read().unwrap().get(&bot).map(|w| w.url.clone());
    let commands = state.tg.commands.get(&bot).and_then(|c| c.as_array().map(|a| a.len())).unwrap_or(0);
    Ok(Json(json!({
        "username": bot,
        "contacts": state.directory.bot_peers_of(&bot).len(),
        "pending_updates": pending,
        "webhook": webhook,
        "commands": commands,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboards() {
        let k = inline_keyboard(&json!(["Yes", {"text": "Site", "url": "https://a.b"}]));
        assert_eq!(k["inline_keyboard"][0][0]["callback_data"], "Yes");
        assert_eq!(k["inline_keyboard"][0][1]["url"], "https://a.b");
        let k = inline_keyboard(&json!([[{"text": "A", "data": "a"}], [{"text": "App", "app": "https://x.y"}]]));
        assert_eq!(k["inline_keyboard"][0][0]["callback_data"], "a");
        assert_eq!(k["inline_keyboard"][1][0]["web_app"]["url"], "https://x.y");
        let r = reply_keyboard(&json!([["1", "2"], ["3"]]), true);
        assert_eq!(r["keyboard"][1][0]["text"], "3");
        assert_eq!(r["one_time_keyboard"], true);
        let rich = Rich { text: "hi".into(), buttons: Some(json!(["ok"])), ..Default::default() };
        assert!(rich.is_rich());
        assert_eq!(rich.params("@Bob")["chat_id"], "@Bob");
    }
}
