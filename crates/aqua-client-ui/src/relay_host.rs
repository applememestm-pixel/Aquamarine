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

    /// Start the app over in a fresh process (another account was picked).
    pub fn relaunch() {
        let Some(h) = HANDLE.get() else { return };
        if let Err(e) = h.run_mobile_plugin::<serde_json::Value>("relaunch", serde_json::json!({})) {
            tracing::warn!(%e, "relaunch");
        }
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
