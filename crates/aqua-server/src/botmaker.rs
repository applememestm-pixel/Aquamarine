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
    extract::{Path, State},
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Choice {
    label: String,
    next: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Step {
    Message { text: String, next: String },
    Buttons { text: String, choices: Vec<Choice> },
    Wait { next: String },
    Branch { equals: String, yes: String, no: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Scenario {
    entry: String,
    steps: std::collections::BTreeMap<String, Step>,
}

impl Scenario {
    fn validate(&self) -> bool {
        let target = |s: &str| s.is_empty() || self.steps.contains_key(s);
        let text = |s: &str| s.len() <= TEXT_MAX && !s.trim().is_empty();
        self.steps.len() <= 50 && self.steps.contains_key(&self.entry) && self.steps.iter().all(|(id, step)| {
            if !valid_command(id) { return false; }
            match step {
                Step::Message { text: message, next } => text(message) && target(next),
                Step::Buttons { text: message, choices } => text(message) && (1..=8).contains(&choices.len()) &&
                    choices.iter().all(|b| !b.label.trim().is_empty() && b.label.len() <= 64 && target(&b.next)),
                Step::Wait { next } => target(next),
                Step::Branch { equals, yes, no } => equals.len() <= 200 && target(yes) && target(no),
            }
        })
    }

    fn advance(&self, cursor: &mut String, update: &BotUpdate, bot: &str) -> Vec<String> {
        let reset = update.text.split_whitespace().next().is_some_and(|w|
            w == "/start" || w.strip_prefix("/start@").is_some_and(|name| name.eq_ignore_ascii_case(bot))
        );
        let mut reply = Vec::new();
        if reset || cursor.is_empty() {
            if !reset && cursor.is_empty() && update.callback.is_some() { return reply; }
            *cursor = self.entry.clone();
        } else {
            match self.steps.get(cursor.as_str()) {
                Some(Step::Wait { next }) if update.callback.is_none() => cursor.clone_from(next),
                Some(Step::Buttons { choices, .. }) => {
                    let prefix = format!("pro:{cursor}:");
                    let index = update.callback.as_deref().and_then(|v| v.strip_prefix(&prefix))
                        .and_then(|v| v.parse::<usize>().ok());
                    let Some(choice) = index.and_then(|i| choices.get(i)) else { return reply; };
                    cursor.clone_from(&choice.next);
                }
                _ => return reply,
            }
        }
        for _ in 0..16 {
            match self.steps.get(cursor.as_str()) {
                Some(Step::Message { text, next }) => {
                    reply.push(text.clone());
                    cursor.clone_from(next);
                }
                Some(Step::Buttons { text, choices }) => {
                    let rows: Vec<_> = choices.iter().enumerate().map(|(i, b)|
                        [json!({ "t": b.label, "cb": format!("pro:{cursor}:{i}") })]
                    ).collect();
                    let keyboard = json!({ "kind": "inline", "m": 0, "rows": rows });
                    reply.push(format!("{}{keyboard}\\n{text}", crate::tgapi::KB_PREFIX));
                    return reply;
                }
                Some(Step::Wait { .. }) => return reply,
                Some(Step::Branch { equals, yes, no }) => {
                    cursor.clone_from(if update.text.trim().to_lowercase() == equals.trim().to_lowercase() { yes } else { no });
                }
                None => { cursor.clear(); return reply; }
            }
        }
        cursor.clear();
        reply
    }
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scenario: Option<Scenario>,
}

fn specs() -> &'static RwLock<HashMap<String, Spec>> {
    static S: std::sync::OnceLock<RwLock<HashMap<String, Spec>>> = std::sync::OnceLock::new();
    S.get_or_init(|| {
        let map = match std::fs::read(FILE) {
            Err(_) => HashMap::new(),
            Ok(b) => serde_json::from_slice(&b).unwrap_or_else(|e| {
                // A broken file isn't overwritten by the next save: it's kept aside.
                let aside = format!("{FILE}.broken-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0));
                let _ = std::fs::rename(FILE, &aside);
                tracing::warn!(%e, file = %aside, "bot constructor: the specs file is broken; kept aside, starting empty");
                HashMap::new()
            }),
        };
        RwLock::new(map)
    })
}

/// One save at a time (they share the .tmp file); a failure is logged.
fn save() {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let map = specs().read().unwrap_or_else(|e| e.into_inner()).clone();
    let res = serde_json::to_vec_pretty(&map).map_err(|e| e.to_string()).and_then(|b| {
        let tmp = format!("{FILE}.tmp");
        std::fs::write(&tmp, b).and_then(|_| std::fs::rename(&tmp, FILE)).map_err(|e| e.to_string())
    });
    if let Err(e) = res {
        tracing::warn!(%e, "bot constructor: can't save the specs");
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
    if s.scenario.as_ref().is_some_and(|scenario| !scenario.validate()) {
        return Err("invalid scenario steps or links".into());
    }
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
    // Whole words: "hi" answers "hi there", not "this".
    let norm = |x: &str| format!(" {} ", x.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" "));
    let low = norm(t);
    s.keywords
        .iter()
        .find(|k| k.words.split(',').map(norm).any(|w| !w.trim().is_empty() && low.contains(&w)))
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
    /// Off the list however the loop ends (a panic included), so it can start again.
    struct Done(String);
    impl Drop for Done {
        fn drop(&mut self) { running().remove(&self.0); }
    }
    tokio::spawn(async move {
        let _done = Done(bot.clone());
        let mut current_scenario: Option<Scenario> = None;
        let mut cursors: HashMap<String, String> = HashMap::new();
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
            if batch.is_empty() { continue; }
            if current_scenario != spec.scenario {
                current_scenario.clone_from(&spec.scenario);
                cursors.clear();
            }
            let pro = if spec.scenario.is_some() { pro_enabled(&state, &bot).await } else { false };
            for u in batch {
                // Bot to bot: no answer (two made bots would talk forever).
                if u.from_bot {
                    continue;
                }
                if let Some(scenario) = &spec.scenario {
                    if pro {
                        if cursors.len() >= 10_000 && !cursors.contains_key(&u.from) { cursors.clear(); }
                        let cursor = cursors.entry(u.from.clone()).or_default();
                        for text in scenario.advance(cursor, &u, &bot) {
                            crate::bot_deliver_text(&state, &bot, &u.from, &text).await;
                        }
                    }
                    continue;
                }
                if let Some(r) = pick(&spec, &bot, &u) {
                    crate::bot_deliver_text(&state, &bot, &u.from, &render(r)).await;
                }
            }
        }
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
    if s.scenario.is_none() && spec_of(&bot).is_some_and(|spec| spec.scenario.is_some()) {
        return Err((StatusCode::CONFLICT, "edit the Pro scenario from your account".into()));
    }
    put_spec(state, bot, s).await
}

async fn pro_enabled(state: &AppState, bot: &str) -> bool {
    let Some(owner) = state.directory.bot_owner_name(bot) else { return false; };
    crate::quota::paid(crate::quota::tier_of_user(state, &owner).await)
}

async fn put_spec(state: AppState, bot: String, s: Spec) -> Result<StatusCode, Err> {
    if s.scenario.is_some() && !pro_enabled(&state, &bot).await {
        return Err((StatusCode::FORBIDDEN, "Stars subscription required".into()));
    }
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

fn owned_bot(state: &AppState, dh: &[u8; 32], name: String) -> Result<String, Err> {
    let name = name.to_ascii_lowercase();
    let owner = state.directory.bot_account_identity(dh);
    if state.directory.bot_owner(&name).as_deref() != Some(owner.as_slice()) {
        return Err((StatusCode::NOT_FOUND, "bot not found".into()));
    }
    Ok(name)
}

pub async fn owner_get(State(state): State<AppState>, crate::Caller(dh): crate::Caller, Path(name): Path<String>) -> Result<Json<Value>, Err> {
    let bot = owned_bot(&state, &dh, name)?;
    Ok(Json(spec_of(&bot).map(|s| json!(s)).unwrap_or(Value::Null)))
}

pub async fn owner_put(State(state): State<AppState>, crate::Caller(dh): crate::Caller, Path(name): Path<String>, Json(spec): Json<Spec>) -> Result<StatusCode, Err> {
    let bot = owned_bot(&state, &dh, name)?;
    put_spec(state, bot, spec).await
}

pub async fn owner_delete(State(state): State<AppState>, crate::Caller(dh): crate::Caller, Path(name): Path<String>) -> Result<StatusCode, Err> {
    let bot = owned_bot(&state, &dh, name)?;
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
            scenario: None,
        }
    }

    #[test]
    fn scenario_waits_branches_and_resets_independently() {
        let scenario: Scenario = serde_json::from_value(json!({
            "entry": "ask", "steps": {
                "ask": { "type": "message", "text": "Ready?", "next": "wait" },
                "wait": { "type": "wait", "next": "branch" },
                "branch": { "type": "branch", "equals": "yes", "yes": "done", "no": "ask" },
                "done": { "type": "message", "text": "Done", "next": "" }
            }
        })).unwrap();
        assert!(scenario.validate());
        let (mut alice, mut bob) = (String::new(), String::new());
        assert_eq!(scenario.advance(&mut alice, &upd("/start", None), "bot"), ["Ready?"]);
        assert_eq!(scenario.advance(&mut bob, &upd("hello", None), "bot"), ["Ready?"]);
        assert_eq!(scenario.advance(&mut alice, &upd(" YES ", None), "bot"), ["Done"]);
        assert_eq!(scenario.advance(&mut bob, &upd("/start", None), "bot"), ["Ready?"]);
        assert_eq!(bob, "wait");
    }

    #[test]
    fn answers() {
        let s = clean(spec()).unwrap();
        assert_eq!(pick(&s, "shop", &upd("/start", None)).unwrap().text, "Hi");
        assert_eq!(pick(&s, "shop", &upd("/prices@shop", None)).unwrap().text, "100");
        assert_eq!(pick(&s, "shop", &upd("x", Some("go:prices"))).unwrap().text, "100");
        assert_eq!(pick(&s, "shop", &upd("Ну ПРИВЕТ!", None)).unwrap().text, "Hey");
        assert_eq!(pick(&s, "shop", &upd("shellohello", None)).unwrap().text, "?", "whole words only");
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
