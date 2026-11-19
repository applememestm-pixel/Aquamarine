//! Bot constructor: a bot made in the app without code. Its owner saves a
//! spec — a greeting, /commands, answers to words, buttons that lead from
//! one answer to another or open a link — and this relay answers for the bot.
//!
//! `GET / PUT / DELETE /v1/botmaker/spec` with the bot's token (the app keeps
//! it in the owner's vault). Specs live in `aqua-bot-specs.json`; each bot
//! with a spec gets its own loop over its queue, like the built-in bots.
//! Messages reach the bot the usual way (`deliver_to_bot`), from this relay
//! and from the network.

use std::collections::HashMap;
use std::sync::RwLock;

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::{AppState, BotUpdate};

const FILE: &str = "aqua-bot-specs.json";
const TEXT_MAX: usize = 4000;
const COMMANDS_MAX: usize = 50;
const KEYWORDS_MAX: usize = 100;
const ROWS_MAX: usize = 8;
const ROW_MAX: usize = 4;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Button {
    pub text: String,
    /// Another answer of this bot: a command's name, or "start".
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub goto: String,
    /// …or a link (http / https).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub url: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Reply {
    #[serde(default)]
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub buttons: Vec<Vec<Button>>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Command {
    pub command: String,
    #[serde(default)]
    pub description: String,
    pub reply: Reply,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Keyword {
    /// Comma-separated; a message with any of them (any case) gets the reply.
    pub words: String,
    pub reply: Reply,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Spec {
    #[serde(default)]
    pub about: String,
    /// /start — and the first message of a new chat.
    #[serde(default)]
    pub start: Reply,
    #[serde(default)]
    pub commands: Vec<Command>,
    #[serde(default)]
    pub keywords: Vec<Keyword>,
    /// Anything else (none: the bot stays silent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<Reply>,
}

fn specs() -> &'static RwLock<HashMap<String, Spec>> {
    static S: std::sync::OnceLock<RwLock<HashMap<String, Spec>>> = std::sync::OnceLock::new();
    S.get_or_init(|| {
        let map = std::fs::read(FILE).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        RwLock::new(map)
    })
}

fn save() {
    let map = specs().read().unwrap_or_else(|e| e.into_inner()).clone();
    if let Ok(b) = serde_json::to_vec_pretty(&map) {
        let tmp = format!("{FILE}.tmp");
        if std::fs::write(&tmp, b).is_ok() {
            let _ = std::fs::rename(&tmp, FILE);
        }
    }
}

fn spec_of(bot: &str) -> Option<Spec> {
    specs().read().unwrap_or_else(|e| e.into_inner()).get(bot).cloned()
}

fn command_name(s: &str) -> String {
    s.trim().trim_start_matches('/').to_lowercase()
}

fn valid_command(c: &str) -> bool {
    !c.is_empty() && c.len() <= 32 && c.chars().all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
}

fn clean_text(t: &str) -> String {
    t.trim().chars().take(TEXT_MAX).collect()
}

/// Checked and trimmed: what a client sends is never stored as it came.
pub fn clean(mut s: Spec) -> Result<Spec, String> {
    s.about = s.about.trim().chars().take(512).collect();
    if s.commands.len() > COMMANDS_MAX {
        return Err(format!("at most {COMMANDS_MAX} commands"));
    }
    if s.keywords.len() > KEYWORDS_MAX {
        return Err(format!("at most {KEYWORDS_MAX} answers to words"));
    }
    let mut names = vec!["start".to_string()];
    for c in &mut s.commands {
        c.command = command_name(&c.command);
        if !valid_command(&c.command) {
            return Err(format!("/{}: a command is 1–32 latin letters, digits or _", c.command));
        }
        if names.contains(&c.command) {
            return Err(format!("/{} is there twice", c.command));
        }
        names.push(c.command.clone());
        c.description = c.description.trim().chars().take(256).collect();
    }
    for k in &mut s.keywords {
        k.words = k.words.split(',').map(str::trim).filter(|w| !w.is_empty()).map(|w| w.chars().take(64).collect::<String>()).collect::<Vec<_>>().join(", ");
        if k.words.is_empty() {
            return Err("an answer to words has no words".into());
        }
    }
    let fix = |r: &mut Reply| -> Result<(), String> {
        r.text = clean_text(&r.text);
        if r.buttons.len() > ROWS_MAX || r.buttons.iter().any(|row| row.len() > ROW_MAX) {
            return Err(format!("buttons: at most {ROWS_MAX} rows of {ROW_MAX}"));
        }
        r.buttons.retain(|row| !row.is_empty());
        for b in r.buttons.iter_mut().flatten() {
            b.text = b.text.trim().chars().take(64).collect();
            b.goto = command_name(&b.goto);
            b.url = b.url.trim().to_string();
            if b.text.is_empty() {
                return Err("a button has no name".into());
            }
            match (b.goto.is_empty(), b.url.is_empty()) {
                (false, true) if names.contains(&b.goto) => {}
                (false, true) => return Err(format!("«{}» leads to /{}, and there's no such command", b.text, b.goto)),
                (true, false) if (b.url.starts_with("https://") || b.url.starts_with("http://")) && b.url.len() <= 512 => {}
                (true, false) => return Err(format!("«{}»: the link must start with https://", b.text)),
                _ => return Err(format!("«{}»: a button leads to a command or opens a link", b.text)),
            }
        }
        if r.text.is_empty() {
            return Err("an answer has no text".into());
        }
        Ok(())
    };
    fix(&mut s.start)?;
    for c in &mut s.commands {
        fix(&mut c.reply).map_err(|e| format!("/{}: {e}", c.command))?;
    }
    for k in &mut s.keywords {
        fix(&mut k.reply).map_err(|e| format!("«{}»: {e}", k.words))?;
    }
    if let Some(f) = s.fallback.as_mut() {
        fix(f)?;
    }
    Ok(s)
}

/// What the bot says to this update, if anything.
pub fn pick<'a>(s: &'a Spec, bot: &str, u: &BotUpdate) -> Option<&'a Reply> {
    let by_name = |name: &str| -> Option<&'a Reply> {
        if name == "start" {
            return Some(&s.start);
        }
        s.commands.iter().find(|c| c.command == name).map(|c| &c.reply)
    };
    if let Some(cb) = u.callback.as_deref() {
        return cb.strip_prefix("go:").and_then(by_name);
    }
    let t = u.text.trim();
    if let Some(rest) = t.strip_prefix('/') {
        let word = rest.split_whitespace().next().unwrap_or("");
        let word = word.split_once('@').filter(|(_, at)| at.eq_ignore_ascii_case(bot)).map(|(w, _)| w).unwrap_or(word);
        return by_name(&word.to_lowercase()).or(s.fallback.as_ref());
    }
    let low = t.to_lowercase();
    s.keywords
        .iter()
        .find(|k| k.words.split(',').map(|w| w.trim().to_lowercase()).any(|w| !w.is_empty() && low.contains(&w)))
        .map(|k| &k.reply)
        .or(s.fallback.as_ref())
}

/// The message as the app draws it: `[kb:v1]{…}` + text when there are buttons.
pub fn render(r: &Reply) -> String {
    if r.buttons.is_empty() {
        return r.text.clone();
    }
    let rows: Vec<Vec<Value>> = r
        .buttons
        .iter()
        .map(|row| {
            row.iter()
                .map(|b| if b.url.is_empty() { json!({ "t": b.text, "cb": format!("go:{}", b.goto) }) } else { json!({ "t": b.text, "url": b.url }) })
                .collect()
        })
        .collect();
    let kb = json!({ "kind": "inline", "m": 0, "rows": rows });
    format!("{}{kb}\n{}", crate::tgapi::KB_PREFIX, r.text)
}

/// The "/" menu and the description, as the app asks for them.
async fn publish(state: &AppState, bot: &str, s: &Spec) {
    let cmds: Vec<Value> = s.commands.iter().map(|c| json!({ "command": c.command, "description": if c.description.is_empty() { c.command.clone() } else { c.description.clone() } })).collect();
    let mut p = Map::new();
    p.insert("commands".into(), Value::Array(cmds));
    let _ = crate::tgapi::dispatch(state, bot, "setMyCommands", p).await;
    let mut p = Map::new();
    p.insert("description".into(), json!(s.about));
    let _ = crate::tgapi::dispatch(state, bot, "setMyDescription", p).await;
}

fn running() -> &'static dashmap::DashSet<String> {
    static R: std::sync::OnceLock<dashmap::DashSet<String>> = std::sync::OnceLock::new();
    R.get_or_init(dashmap::DashSet::new)
}

/// The bot's loop: answers while it has a spec; ends when the spec goes.
fn run(state: AppState, bot: String) {
    if !running().insert(bot.clone()) {
        return;
    }
    tokio::spawn(async move {
        loop {
            let Some(spec) = spec_of(&bot) else { break };
            if !state.directory.is_bot(&bot) {
                break;
            }
            let q = state.bot_queue(&bot);
            let batch: Vec<BotUpdate> = {
                let notify = q.lock().await.notify.clone();
                let wait = notify.notified();
                let mut g = q.lock().await;
                if g.updates.is_empty() {
                    drop(g);
                    let _ = tokio::time::timeout(std::time::Duration::from_secs(10), wait).await;
                    q.lock().await.updates.drain(..).collect()
                } else {
                    g.updates.drain(..).collect()
                }
            };
            for u in batch {
                // Bot to bot: no answer (two made bots would talk forever).
                if u.from_bot {
                    continue;
                }
                if let Some(r) = pick(&spec, &bot, &u) {
                    crate::bot_deliver_text(&state, &bot, &u.from, &render(r)).await;
                }
            }
        }
        running().remove(&bot);
    });
}

/// Started with the relay: every made bot answers again (and its menu is back:
/// commands live in memory).
pub fn spawn(state: AppState) {
    let all: Vec<(String, Spec)> = specs().read().unwrap_or_else(|e| e.into_inner()).iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    tokio::spawn(async move {
        for (bot, s) in all {
            if state.directory.is_bot(&bot) {
                publish(&state, &bot, &s).await;
                run(state.clone(), bot);
            }
        }
    });
}

type Err = (StatusCode, String);

pub async fn spec_get(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<Value>, Err> {
    let bot = crate::bot_auth(&state, &headers, "").await?;
    Ok(Json(spec_of(&bot).map(|s| json!(s)).unwrap_or(Value::Null)))
}

pub async fn spec_put(State(state): State<AppState>, headers: HeaderMap, Json(s): Json<Spec>) -> Result<StatusCode, Err> {
    let bot = crate::bot_auth(&state, &headers, "").await?;
    let s = clean(s).map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    specs().write().unwrap_or_else(|e| e.into_inner()).insert(bot.clone(), s.clone());
    save();
    publish(&state, &bot, &s).await;
    run(state, bot);
    Ok(StatusCode::NO_CONTENT)
}

/// Back to a bot run by its own program (or none): the relay stops answering.
pub async fn spec_delete(State(state): State<AppState>, headers: HeaderMap) -> Result<StatusCode, Err> {
    let bot = crate::bot_auth(&state, &headers, "").await?;
    forget(&bot);
    state.tg.commands.remove(&bot);
    Ok(StatusCode::NO_CONTENT)
}

/// The bot was deleted: its spec goes too.
pub fn forget(bot: &str) {
    if specs().write().unwrap_or_else(|e| e.into_inner()).remove(bot).is_some() {
        save();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(t: &str) -> Reply {
        Reply { text: t.into(), buttons: vec![] }
    }
    fn upd(text: &str, cb: Option<&str>) -> BotUpdate {
        BotUpdate { id: 0, from: "u".into(), text: text.into(), ts: 0, callback: cb.map(str::to_string), callback_msg: None, from_bot: false }
    }
    fn spec() -> Spec {
        Spec {
            about: String::new(),
            start: Reply { text: "Hi".into(), buttons: vec![vec![Button { text: "Prices".into(), goto: "prices".into(), url: String::new() }]] },
            commands: vec![Command { command: "prices".into(), description: "Prices".into(), reply: r("100") }],
            keywords: vec![Keyword { words: "hello, привет".into(), reply: r("Hey") }],
            fallback: Some(r("?")),
        }
    }

    #[test]
    fn answers() {
        let s = clean(spec()).unwrap();
        assert_eq!(pick(&s, "shop", &upd("/start", None)).unwrap().text, "Hi");
        assert_eq!(pick(&s, "shop", &upd("/prices@shop", None)).unwrap().text, "100");
        assert_eq!(pick(&s, "shop", &upd("x", Some("go:prices"))).unwrap().text, "100");
        assert_eq!(pick(&s, "shop", &upd("Ну ПРИВЕТ", None)).unwrap().text, "Hey");
        assert_eq!(pick(&s, "shop", &upd("what", None)).unwrap().text, "?");
        assert_eq!(pick(&s, "shop", &upd("/nope", None)).unwrap().text, "?");
        assert!(pick(&s, "shop", &upd("x", Some("go:nope"))).is_none());
        let mut quiet = s.clone();
        quiet.fallback = None;
        assert!(pick(&quiet, "shop", &upd("what", None)).is_none());
    }

    #[test]
    fn renders_buttons() {
        let s = clean(spec()).unwrap();
        let out = render(&s.start);
        assert!(out.starts_with(crate::tgapi::KB_PREFIX));
        assert!(out.contains(r#""cb":"go:prices""#) && out.ends_with("\nHi"));
        assert_eq!(render(&r("plain")), "plain");
    }

    #[test]
    fn rejects_bad_specs() {
        let mut s = spec();
        s.start.buttons[0][0].goto = "missing".into();
        assert!(clean(s).is_err());
        let mut s = spec();
        s.start.buttons[0][0] = Button { text: "Site".into(), goto: String::new(), url: "javascript:alert(1)".into() };
        assert!(clean(s).is_err());
        let mut s = spec();
        s.commands.push(s.commands[0].clone());
        assert!(clean(s).is_err());
        let mut s = spec();
        s.commands[0].command = "Bad Name".into();
        assert!(clean(s).is_err());
        let mut s = spec();
        s.commands[0].command = "/Prices".into();
        assert_eq!(clean(s).unwrap().commands[0].command, "prices");
    }
}
