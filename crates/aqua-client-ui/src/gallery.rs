//! Recent photos and videos for the attach sheet (like Telegram's grid).
//! Android only: the MediaStore lives in Kotlin (android/AquaRelay.kt,
//! AquaGalleryPlugin). Other platforms answer "unsupported" and the sheet
//! shows the system picker instead.

#[tauri::command]
pub async fn gallery_recent(limit: Option<u32>) -> Result<serde_json::Value, String> {
    #[cfg(target_os = "android")]
    {
        let limit = limit.unwrap_or(60);
        return tauri::async_runtime::spawn_blocking(move || android::call("recent", serde_json::json!({ "limit": limit })))
            .await
            .map_err(|e| e.to_string())?;
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = limit;
        Err("unsupported".into())
    }
}

#[tauri::command]
pub async fn gallery_ask() -> Result<serde_json::Value, String> {
    #[cfg(target_os = "android")]
    {
        return tauri::async_runtime::spawn_blocking(|| android::call("ask", serde_json::json!({})))
            .await
            .map_err(|e| e.to_string())?;
    }
    #[cfg(not(target_os = "android"))]
    Err("unsupported".into())
}

#[cfg(target_os = "android")]
pub mod android {
    use std::sync::OnceLock;
    use tauri::plugin::{Builder, PluginHandle, TauriPlugin};
    use tauri::Wry;

    static HANDLE: OnceLock<PluginHandle<Wry>> = OnceLock::new();

    pub fn plugin() -> TauriPlugin<Wry> {
        Builder::new("aquagallery")
            .setup(|_app, api| {
                match api.register_android_plugin("app.aquamarine.messenger", "AquaGalleryPlugin") {
                    Ok(h) => { let _ = HANDLE.set(h); }
                    Err(e) => tracing::warn!(?e, "gallery: Android plugin missing"),
                }
                Ok(())
            })
            .build()
    }

    pub fn call(cmd: &str, args: serde_json::Value) -> Result<serde_json::Value, String> {
        HANDLE.get().ok_or("gallery plugin not loaded")?.run_mobile_plugin::<serde_json::Value>(cmd, args).map_err(|e| e.to_string())
    }
}
