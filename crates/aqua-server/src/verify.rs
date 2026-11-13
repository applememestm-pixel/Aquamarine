//! `@verify` — the relay's bot for check marks.
//!
//! Only the project owner may use it: the account must have signed up on
//! this relay with the owner's e-mail (compared by its SHA-256, so the
//! address itself isn't in the code). Every relay runs the bot and keeps
//! its own list (`verified` table, by account ID, so a rename keeps the
//! mark). The mark shows in profile cards (`Profile.verified`).
//!
//! Commands: `/verify @name`, `/unverify @name`, `/list`.

use crate::AppState;

/// SHA-256 of the owner's sign-up e-mail (lower-case).
const OWNER_EMAIL_SHA256: &str = "0d1d1d7fff5e851104b3268d6527fa9074f04671c544fe3101841868284e83d6";

/// The bot's @name: `verify` unless `AQUA_VERIFY_BOT` names another.
pub fn bot_name() -> String {
    std::env::var("AQUA_VERIFY_BOT").ok().map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()).unwrap_or_else(|| "verify".into())
}

/// Is this local account the owner?
pub fn is_owner(state: &AppState, username: &str) -> bool {
    use sha2::{Digest, Sha256};
    if username.contains('@') || username.contains('~') {
        return false;
    }
    state
        .directory
        .email_of(username)
        .is_some_and(|e| hex::encode(Sha256::digest(e.as_bytes())) == OWNER_EMAIL_SHA256)
}

/// The owner's accounts on this relay (for the bots that ask them things).
pub fn owner_names(state: &AppState) -> Vec<String> {
    use sha2::{Digest, Sha256};
    state
        .directory
        .accounts_with_email()
        .into_iter()
        .filter(|(_, e)| hex::encode(Sha256::digest(e.as_bytes())) == OWNER_EMAIL_SHA256)
        .map(|(n, _)| n)
        .collect()
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
    if !is_owner(state, &u.from) {
        say(state, &u.from, "✔️ This bot gives the check mark on this server. Only the Aquamarine team can use it.").await;
        return;
    }
    let (cmd, arg) = text.split_once(char::is_whitespace).unwrap_or((text, ""));
    let cmd = cmd.split('@').next().unwrap_or(cmd);
    match cmd {
        "/verify" | "/unverify" => {
            let Some(name) = target(arg) else {
                say(state, &u.from, &format!("Who? {cmd} @name")).await;
                return;
            };
            let on = cmd == "/verify";
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
            say(state, &u.from, "✔️ Check marks on this server:\n/verify @name — give\n/unverify @name — take away\n/list — who has one").await;
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
