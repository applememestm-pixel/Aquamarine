//! `@verify` — the relay's bot for check marks.
//!
//! Only the project owner may use it: the account whose ID is built in
//! (`OWNER_UIDS` / `AQUA_OWNER_UID`), or — until it is — the account that
//! signed up on this relay with the owner's e-mail (compared by its SHA-256,
//! so the address itself isn't in the code). Every relay runs the bot and keeps
//! its own list (`verified` table, by account ID, so a rename keeps the
//! mark). The mark shows in profile cards (`Profile.verified`).
//!
//! Commands: `/verify @name`, `/unverify @name`, `/list`, `/id`.

use crate::AppState;

/// SHA-256 of the owner's sign-up e-mail (lower-case).
const OWNER_EMAIL_SHA256: &str = "0d1d1d7fff5e851104b3268d6527fa9074f04671c544fe3101841868284e83d6";

/// The bot's @name: `verify` unless `AQUA_VERIFY_BOT` names another.
pub fn bot_name() -> String {
    std::env::var("AQUA_VERIFY_BOT").ok().map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()).unwrap_or_else(|| "verify".into())
}

/// The owner's account ID(s). An ID comes from the identity key, so nobody
/// can take it without the owner's private key — unlike an e-mail, which a
/// relay without mail (codes shown on screen) or a federated peer can put on
/// any account. `AQUA_OWNER_UID` (comma-separated) overrides. While neither
/// is set, the e-mail decides (and `/id` to this bot shows the ID to build in).
const OWNER_UIDS: &[&str] = &[];

fn pinned() -> Vec<String> {
    match std::env::var("AQUA_OWNER_UID") {
        Ok(v) if !v.trim().is_empty() => v.split(',').map(|x| x.trim().to_lowercase()).filter(|x| !x.is_empty()).collect(),
        _ => OWNER_UIDS.iter().map(|x| x.to_lowercase()).collect(),
    }
}

/// The owner's ID is built in (or set): the e-mail no longer counts.
pub fn owner_pinned() -> bool {
    !pinned().is_empty()
}

fn email_is_owners(e: &str) -> bool {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(e.as_bytes())) == OWNER_EMAIL_SHA256
}

/// Is this local account the owner?
pub fn is_owner(state: &AppState, username: &str) -> bool {
    if username.contains('@') || username.contains('~') {
        return false;
    }
    let ids = pinned();
    if !ids.is_empty() {
        return state.directory.uid_of(username).is_some_and(|u| ids.contains(&u.to_lowercase()));
    }
    state.directory.email_of(username).is_some_and(|e| email_is_owners(&e))
}

/// The owner's accounts on this relay (for the bots that ask them things).
pub fn owner_names(state: &AppState) -> Vec<String> {
    let ids = pinned();
    if !ids.is_empty() {
        return ids.iter().filter_map(|id| state.directory.username_by_uid(id)).collect();
    }
    state.directory.accounts_with_email().into_iter().filter(|(_, e)| email_is_owners(e)).map(|(n, _)| n).collect()
}

/// `/id` to @verify or @stars (the owner only): the account ID to build into
/// `OWNER_UIDS`.
pub fn id_text(state: &AppState, username: &str) -> String {
    let id = state.directory.uid_of(username).unwrap_or_default();
    if !is_owner(state, username) {
        return format!("Your account ID: {id}");
    }
    if owner_pinned() {
        format!("Your account ID: {id}\nThe owner is recognised by this ID.")
    } else {
        format!("Your account ID: {id}\nRight now the owner is recognised by the e-mail only. Build this ID in (verify.rs OWNER_UIDS) or set AQUA_OWNER_UID={id} on the server — then nobody else can pass for you.")
    }
}

pub(crate) fn ensure_bot(state: &AppState) {
    let name = bot_name();
    if state.directory.is_bot(&name) {
        return;
    }
    match state.directory.register_bot(&name, &crate::music::relay_bot_owner(), "Verification") {
        Ok(_) => tracing::info!(bot = %name, "verification bot created"),
        Err(e) => tracing::warn!(bot = %name, "can't create the verification bot: {e}"),
    }
}

/// Started with the relay.
pub fn spawn_bot(state: AppState) {
    tokio::spawn(async move {
        ensure_bot(&state);
        loop {
            let q = state.bot_queue(&bot_name());
            let batch: Vec<crate::BotUpdate> = {
                let notify = q.lock().await.notify.clone();
                let wait = notify.notified();
                let mut g = q.lock().await;
                if g.updates.is_empty() {
                    drop(g);
                    let _ = tokio::time::timeout(std::time::Duration::from_secs(30), wait).await;
                    q.lock().await.updates.drain(..).collect()
                } else {
                    g.updates.drain(..).collect()
                }
            };
            for u in batch {
                if u.from_bot {
                    continue;
                }
                answer(&state, &u).await;
            }
        }
    });
}

async fn say(state: &AppState, to: &str, text: &str) {
    crate::bot_deliver_text(state, &bot_name(), to, text).await;
}

/// `@name` / `name` from a command's argument.
fn target(arg: &str) -> Option<String> {
    let n = arg.split_whitespace().next()?.trim_start_matches('@').to_lowercase();
    (!n.is_empty() && n.len() <= 64).then_some(n)
}

async fn answer(state: &AppState, u: &crate::BotUpdate) {
    let text = u.text.trim();
    if text.is_empty() {
        return;
    }
    if text.split_whitespace().next() == Some("/id") {
        return say(state, &u.from, &id_text(state, &u.from)).await;
    }
    if !is_owner(state, &u.from) {
        say(state, &u.from, "✔️ This bot gives the check mark on this server. Only the Aquamarine team can use it.").await;
        return;
    }
    let (cmd, arg) = text.split_once(char::is_whitespace).unwrap_or((text, ""));
    let cmd = cmd.split('@').next().unwrap_or(cmd);
    match cmd {
        "/id" => say(state, &u.from, &id_text(state, &u.from)).await,
        "/verify" | "/unverify" => {
            let Some(name) = target(arg) else {
                say(state, &u.from, &format!("Who? {cmd} @name (or the account ID)")).await;
                return;
            };
            let on = cmd == "/verify";
            // The star relay's list holds on every server (stars::get_marks):
            // anyone, by @name from the name registry or by ID.
            if crate::stars::has_key() {
                let Some((uid, who)) = crate::stars::resolve(state, &name) else {
                    say(state, &u.from, &format!("I don't know @{name}: not on this server or in the name registry of the servers it knows. The account ID works too: {cmd} 1234567890")).await;
                    return;
                };
                state.directory.set_verified_uid(&uid, on);
                let msg = if on { format!("✔️ {who} is verified on every server (others see it within about 15 minutes).") } else { format!("{who} no longer has the check mark (gone everywhere within about 15 minutes).") };
                say(state, &u.from, &msg).await;
                tracing::info!(%uid, on, "check mark changed (every server)");
                return;
            }
            if state.directory.set_verified(&name, on) {
                let msg = if on { format!("✔️ @{name} is verified on this server.") } else { format!("@{name} no longer has the check mark.") };
                say(state, &u.from, &msg).await;
                tracing::info!(target_user = %name, on, "check mark changed");
            } else {
                say(state, &u.from, &format!("There's no @{name} on this server.")).await;
            }
        }
        "/list" => {
            let names = state.directory.verified_names();
            let msg = if names.is_empty() { "Nobody has a check mark here yet.".to_string() } else { format!("Verified here ({}):\n{}", names.len(), names.iter().map(|n| format!("✔️ @{n}")).collect::<Vec<_>>().join("\n")) };
            say(state, &u.from, &msg).await;
        }
        _ => {
            say(state, &u.from, "✔️ Check marks on this server:\n/verify @name — give\n/unverify @name — take away\n/list — who has one\n/id — your account ID").await;
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn target_takes_the_name() {
        assert_eq!(super::target("@Ivan rest").as_deref(), Some("ivan"));
        assert_eq!(super::target("  "), None);
    }
}
