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

    /// The messenger's background connection (AquaSyncService) on / off.
    pub fn background_sync(on: bool, reminder: Option<String>) -> Result<(), String> {
        let h = HANDLE.get().ok_or("background connection is not available")?;
        h.run_mobile_plugin::<serde_json::Value>(if on { "syncStart" } else { "syncStop" }, serde_json::json!({ "reminder": reminder }))
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// The call's foreground service (AquaCallService) on / off.
    pub fn call_service(on: bool, text: Option<String>) -> Result<(), String> {
        let h = HANDLE.get().ok_or("the call service is not available")?;
        h.run_mobile_plugin::<serde_json::Value>("callService", serde_json::json!({ "on": on, "text": text }))
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// The system's "don't optimise battery for this app" dialog; true if already off.
    pub fn ask_battery() -> Result<bool, String> {
        let h = HANDLE.get().ok_or("not available")?;
        h.run_mobile_plugin::<serde_json::Value>("askBattery", serde_json::json!({}))
            .map(|v| v["ok"].as_bool().unwrap_or(false))
            .map_err(|e| e.to_string())
    }

    /// AQUA-SEC-013: wrap the vault master key with a key that lives in the
    /// Android Keystore (AquaRelay.kt → ksWrap; AES-GCM, the key never leaves
    /// the Keystore). Returns the wrapped blob (base64).
    pub fn ks_wrap(key_hex: &str) -> Result<String, String> {
        let h = HANDLE.get().ok_or("keystore is not available")?;
        h.run_mobile_plugin::<serde_json::Value>("ksWrap", serde_json::json!({ "key": key_hex }))
            .map_err(|e| e.to_string())?
            .get("blob").and_then(|v| v.as_str()).map(str::to_string)
            .ok_or_else(|| "keystore gave no blob".to_string())
    }

    /// The master key (hex) out of a blob made by [`ks_wrap`].
    pub fn ks_unwrap(blob: &str) -> Result<String, String> {
        let h = HANDLE.get().ok_or("keystore is not available")?;
        h.run_mobile_plugin::<serde_json::Value>("ksUnwrap", serde_json::json!({ "blob": blob }))
            .map_err(|e| e.to_string())?
            .get("key").and_then(|v| v.as_str()).map(str::to_string)
            .ok_or_else(|| "keystore gave no key".to_string())
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

/// Stay connected in the background (Android: a quiet foreground service).
#[tauri::command]
pub fn bg_sync_set(on: bool, reminder: Option<String>) -> Result<(), String> {
    #[cfg(target_os = "android")]
    { android::background_sync(on, reminder) }
    #[cfg(not(target_os = "android"))]
    { let _ = (on, reminder); Ok(()) }
}

/// A call is on or over (Android: keeps the microphone and camera working in
/// the background with a foreground service).
#[tauri::command]
pub fn call_service_set(on: bool, text: Option<String>) -> Result<(), String> {
    #[cfg(target_os = "android")]
    { android::call_service(on, text) }
    #[cfg(not(target_os = "android"))]
    { let _ = (on, text); Ok(()) }
}

/// Android: ask to take the app off battery optimisation. True if it already is.
#[tauri::command]
pub fn battery_exempt() -> Result<bool, String> {
    #[cfg(target_os = "android")]
    { android::ask_battery() }
    #[cfg(not(target_os = "android"))]
    { Ok(true) }
}
