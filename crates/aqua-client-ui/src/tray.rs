//! The tray icon (Settings → Advanced → System integration): click opens
//! the window, the menu has "Open" and "Quit". While it's on, closing the
//! window hides it to the tray instead of quitting, like Telegram.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Manager};

static ON: AtomicBool = AtomicBool::new(false);

/// Closing the main window only hides it.
pub fn keeps_running() -> bool {
    ON.load(Ordering::Relaxed)
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        // Phones have no minimized windows (and no such method).
        #[cfg(desktop)]
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

#[cfg(desktop)]
fn build(app: &AppHandle, mono: bool, unread: u32) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
    let _ = app.remove_tray_by_id("main");
    let open = MenuItem::with_id(app, "open", "Open Aquamarine", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let bytes: &[u8] = if mono { include_bytes!("../icons/tray-mono.png") } else { include_bytes!("../icons/tray.png") };
    let icon = tauri::image::Image::from_bytes(bytes)?;
    let tip = if unread > 0 { format!("Aquamarine — {unread} unread") } else { "Aquamarine".into() };
    TrayIconBuilder::with_id("main")
        .icon(icon)
        .tooltip(tip)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, e| match e.id.as_ref() {
            "open" => show_main(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|t, e| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                show_main(t.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/// Turn the tray icon on / off (and pick the monochrome one).
#[tauri::command]
pub fn tray_set(app: AppHandle, on: bool, mono: bool, unread: Option<u32>) -> Result<(), String> {
    #[cfg(desktop)]
    {
        if on {
            build(&app, mono, unread.unwrap_or(0)).map_err(|e| e.to_string())?;
        } else {
            let _ = app.remove_tray_by_id("main");
        }
        ON.store(on, Ordering::Relaxed);
    }
    #[cfg(mobile)]
    {
        let _ = (app, on, mono, unread);
    }
    Ok(())
}
