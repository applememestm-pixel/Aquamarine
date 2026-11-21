//! Windows message toasts need an OS tag so they can be recalled on lock or read.
//! The notification plugin does not expose a Windows tag or support closing there.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use tauri::AppHandle;
use windows::{
    core::{Interface, HSTRING},
    Data::Xml::Dom::XmlDocument,
    Foundation::TypedEventHandler,
    UI::Notifications::{ToastActivatedEventArgs, ToastNotification, ToastNotificationManager},
};

use super::{perform_action, Prefs, READ, REPLY};

const GROUP: &str = "aqua-messages";
static SHOWN: LazyLock<Mutex<HashMap<i32, ToastNotification>>> = LazyLock::new(Default::default);

fn xml(value: &str) -> String {
    let mut out = String::new();
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if c.is_control() || matches!(c, '\u{fffe}' | '\u{ffff}') => {}
            c => out.push(c),
        }
    }
    out
}

pub(super) fn show(
    app: &AppHandle,
    id: i32,
    title: &str,
    body: &str,
    prefs: &Prefs,
    route: Option<(String, String)>,
    can_reply: bool,
    hidden: bool,
) -> windows::core::Result<()> {
    let mut actions = String::new();
    if route.is_some() {
        actions = format!(
            r#"<actions><action content="{}" arguments="{READ}" activationType="foreground"/>"#,
            xml(&prefs.read_text)
        );
        if can_reply {
            actions.push_str(&format!(
                r#"<action content="{}" arguments="{REPLY}" activationType="foreground"/>"#,
                xml(&prefs.reply_text)
            ));
        }
        actions.push_str("</actions>");
    }
    let doc = XmlDocument::new()?;
    doc.LoadXml(&HSTRING::from(format!(
        "<toast><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text></binding></visual>{actions}</toast>",
        xml(title), xml(body),
    )))?;
    let note = ToastNotification::CreateToastNotification(&doc)?;
    note.SetTag(&HSTRING::from(id.to_string()))?;
    note.SetGroup(&HSTRING::from(GROUP))?;
    let app_id = HSTRING::from(app.config().identifier.as_str());
    let notifier = ToastNotificationManager::CreateToastNotifierWithId(&app_id)?;
    let handle = app.clone();
    note.Activated(&TypedEventHandler::new(
        move |_, arg: windows::core::Ref<'_, windows::core::IInspectable>| {
            if let Some((account, peer)) = &route {
                if let Some(action) = arg
                    .as_ref()
                    .and_then(|a| a.cast::<ToastActivatedEventArgs>().ok())
                    .and_then(|a| a.Arguments().ok())
                    .map(|a| a.to_string())
                {
                    perform_action(handle.clone(), account.clone(), peer.clone(), action, None);
                }
            }
            Ok(())
        },
    ))?;
    let mut shown = SHOWN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // A password might have been enabled while the toast was being built.
    // Recheck under the same guard used by remove().
    if (!hidden && crate::applock::is_locked())
        || super::prefs()
            .is_none_or(|p| p.locked != prefs.locked || p.preview != prefs.preview || !p.enabled)
    {
        return Ok(());
    }
    notifier.Show(&note)?;
    shown.insert(id, note);
    Ok(())
}

pub(super) fn remove(app: &AppHandle, ids: Vec<i32>) {
    let mut shown = SHOWN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let app_id = HSTRING::from(app.config().identifier.as_str());
    let notifier = ToastNotificationManager::CreateToastNotifierWithId(&app_id).ok();
    let history = ToastNotificationManager::History().ok();
    for id in ids {
        if let Some(note) = shown.remove(&id) {
            if let Some(notifier) = &notifier {
                let _ = notifier.Hide(&note);
            }
        }
        if let Some(history) = &history {
            let _ = history.RemoveGroupedTagWithId(
                &HSTRING::from(id.to_string()),
                &HSTRING::from(GROUP),
                &app_id,
            );
        }
    }
}
