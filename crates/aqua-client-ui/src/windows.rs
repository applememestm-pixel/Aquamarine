//! Extra windows: a chat opened on its own (Ctrl+click in the chat list).
//!
//! The window loads the same UI with `#popout=<chat>` and shows only that
//! chat; it shares the app state, so messages, keys and the connection are
//! the main window's.

use tauri::AppHandle;

/// Open (or focus) a window with just one chat.
#[tauri::command]
pub async fn open_chat_window(app: AppHandle, peer_id: String, title: String) -> Result<(), String> {
    #[cfg(desktop)]
    {
        use tauri::Manager;
        if peer_id.is_empty() || peer_id.len() > 80 {
            return Err("bad chat id".into());
        }
        let safe: String = peer_id
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' })
            .collect();
        let label = format!("chat-{safe}");
        if let Some(w) = app.get_webview_window(&label) {
            let _ = w.unminimize();
            let _ = w.show();
            let _ = w.set_focus();
            return Ok(());
        }
        let enc: String = peer_id
            .bytes()
            .map(|b| if b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || b == b'-' { (b as char).to_string() } else { format!("%{b:02X}") })
            .collect();
        let url = tauri::WebviewUrl::App(format!("index.html#popout={enc}").into());
        let title: String = title.chars().take(60).collect();
        let win = tauri::WebviewWindowBuilder::new(&app, &label, url)
            .title(if title.is_empty() { "Aquamarine".to_string() } else { format!("{title} — Aquamarine") })
            .inner_size(520.0, 760.0)
            .min_inner_size(360.0, 420.0)
            .decorations(false)
            .shadow(true)
            .theme(Some(tauri::Theme::Dark))
            .background_color(tauri::window::Color(6, 7, 12, 255))
            .build()
            .map_err(|e| format!("window: {e}"))?;
        #[cfg(target_os = "linux")]
        crate::linux_webview::configure_window(&win);
        #[cfg(not(target_os = "linux"))]
        let _ = win;
        Ok(())
    }
    #[cfg(not(desktop))]
    {
        let _ = (app, peer_id, title);
        Err("separate windows are a desktop feature".into())
    }
}
