//! The Android plugin of the messenger (AquaRelay.kt): opening links, the
//! home-screen widget, relaunching after an account switch.
//!
//! Until 1.2.0-5 this file also ran "Server on this device" — the relay
//! inside the messenger. The server is its own app now (crates/aqua-server-app,
//! "Aquamarine Server"); the messenger only chats.

#[cfg(target_os = "android")]
pub mod android {
    //! The foreground service lives in Kotlin (AquaRelay.kt).
    use std::sync::OnceLock;
    use tauri::plugin::{Builder, PluginHandle, TauriPlugin};
    use tauri::Wry;

    static HANDLE: OnceLock<PluginHandle<Wry>> = OnceLock::new();

    pub fn plugin() -> TauriPlugin<Wry> {
        Builder::new("aquarelay")
            .setup(|_app, api| {
                match api.register_android_plugin("app.aquamarine.messenger", "AquaRelayPlugin") {
                    Ok(h) => { let _ = HANDLE.set(h); }
                    Err(e) => tracing::warn!(?e, "relay host: Android plugin missing"),
                }
                Ok(())
            })
            .build()
    }

    pub fn keep_alive(on: bool, port: u16) {
        let Some(h) = HANDLE.get() else { return };
        let cmd = if on { "start" } else { "stop" };
        if let Err(e) = h.run_mobile_plugin::<serde_json::Value>(cmd, serde_json::json!({ "port": port })) {
            tracing::warn!(%e, "relay host: foreground service {cmd}");
        }
    }

    /// A link in the browser or mail app (AquaRelay.kt → openUrl).
    pub fn open_url(url: &str) -> Result<(), String> {
        let h = HANDLE.get().ok_or("opening links is not available")?;
        h.run_mobile_plugin::<serde_json::Value>("openUrl", serde_json::json!({ "url": url }))
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// What the home-screen widget shows (AquaWidget.kt).
    pub fn update_widget(data: &str) -> Result<(), String> {
        let h = HANDLE.get().ok_or("widget is not available")?;
        h.run_mobile_plugin::<serde_json::Value>("updateWidget", serde_json::json!({ "data": data }))
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// Shrink the app into a picture-in-picture window (a video plays on).
    pub fn enter_pip(w: u32, h: u32) -> Result<(), String> {
        let hd = HANDLE.get().ok_or("picture-in-picture is not available")?;
        hd.run_mobile_plugin::<serde_json::Value>("enterPip", serde_json::json!({ "w": w, "h": h }))
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// FLAG_SECURE on / off (AquaRelay.kt → secure).
    pub fn secure(on: bool) -> Result<(), String> {
        let h = HANDLE.get().ok_or("screen protection is not available")?;
        h.run_mobile_plugin::<serde_json::Value>("secure", serde_json::json!({ "on": on }))
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// Start the app over in a fresh process (another account was picked).
    pub fn relaunch() {
        let Some(h) = HANDLE.get() else { return };
        if let Err(e) = h.run_mobile_plugin::<serde_json::Value>("relaunch", serde_json::json!({})) {
            tracing::warn!(%e, "relaunch");
        }
    }
}

/// Phones: the app goes into a small floating window, like a video in
/// Telegram (MediaPlayer's picture-in-picture button; the video fills the
/// app first). Elsewhere the webview's own picture-in-picture is used.
#[tauri::command]
pub fn pip_enter(w: u32, h: u32) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        return android::enter_pip(w.max(1), h.max(1));
    }
    #[allow(unreachable_code)]
    {
        let _ = (w, h);
        Err("picture-in-picture is only on Android here".into())
    }
}

/// Keep the screen out of screenshots and recordings (view-once media, chats
/// with sharing off): FLAG_SECURE on Android, content protection on Windows
/// and macOS. Linux can't, so it says so.
#[tauri::command]
pub fn screen_secure(window: tauri::WebviewWindow, on: bool) -> Result<bool, String> {
    #[cfg(target_os = "android")]
    {
        let _ = window;
        return android::secure(on).map(|_| true);
    }
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        return window.set_content_protected(on).map(|_| true).map_err(|e| e.to_string());
    }
    #[allow(unreachable_code)]
    {
        let _ = (window, on);
        Ok(false)
    }
}

/// The home-screen widget's content: `{title, unread, empty, rows: [{n, p, u}]}`
/// (Android; elsewhere there's no widget and this does nothing).
#[tauri::command]
pub fn widget_update(data: String) -> Result<(), String> {
    if data.len() > 16 * 1024 {
        return Err("too much for a widget".into());
    }
    #[cfg(target_os = "android")]
    {
        return android::update_widget(&data);
    }
    #[allow(unreachable_code)]
    {
        let _ = data;
        Ok(())
    }
}
