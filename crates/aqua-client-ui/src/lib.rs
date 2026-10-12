//! Aquamarine Tauri desktop shell.
//!
//! Entry point for the mobile / desktop Tauri app. Registers the
//! command handlers from [`commands`] and wraps the shared
//! [`state::AppState`] in a `Mutex` for cross-command access.

pub mod state;
pub mod commands;
pub mod media;
pub mod activity;

use anyhow::Result;
use tauri::Manager;

/// Android / iOS entry point (desktop starts from `main.rs`).
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn mobile_main() {
    if let Err(e) = run() {
        eprintln!("aquamarine: {e:?}");
    }
}

pub fn run() -> Result<()> {
    let builder = tauri::Builder::default();
    // WebKitGTK denies camera / mic / screen capture unless the app answers
    // its permission request ("Mic test failed: NotAllowedError" on Linux).
    // The webview only ever shows our own UI, so grant what calls need.
    #[cfg(target_os = "linux")]
    let builder = builder.on_permission_request(|_, kind| {
        use tauri::webview::{PermissionKind as K, PermissionResponse as R};
        match kind {
            K::Microphone | K::Camera | K::DisplayCapture | K::Notifications | K::ClipboardRead => R::Allow,
            _ => R::Default,
        }
    });
    builder
        .plugin(tauri_plugin_log::Builder::default().build())
        .setup(|app| {
            // Priority: AQUA_DATA_DIR env var > OS app-data dir > temp dir.
            // The env var is how you run two independent clients on the
            // same machine (each with its own identity + vault): set
            // AQUA_DATA_DIR to a distinct path per launch.
            let data_dir = std::env::var_os("AQUA_DATA_DIR")
                .map(std::path::PathBuf::from)
                .or_else(|| app.path().app_data_dir().ok())
                .unwrap_or_else(|| std::env::temp_dir().join("aquamarine-dev"));
            tracing::info!(path = %data_dir.display(), "aquamarine data directory");
            let state = state::AppState::boot(data_dir)?;
            app.manage(state);
            commands::spawn_activity_watcher(app.handle().clone());
            #[cfg(target_os = "linux")]
            linux_webview::configure(app);
            Ok(())
        })
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::init_app,
            commands::complete_registration,
            commands::log_out,
            commands::list_chats,
            commands::get_messages,
            commands::send_message,
            commands::verify_fingerprint,
            commands::session_fingerprint,
            commands::connect_to_server,
            commands::set_network_proxy,
            commands::request_email_code,
            commands::verify_email_code,
            commands::register_account,
            commands::register_username,
            commands::search_directory,
            commands::fetch_directory_profile,
            commands::publish_profile,
            commands::publish_profile_full,
            commands::set_profile_image,
            commands::set_profile_image_bytes,
            commands::clear_profile_image,
            commands::set_photo_privacy,
            commands::group_control,
            commands::group_control_to,
            commands::set_local_prefs,
            commands::login_account,
            commands::delete_group_message_of,
            commands::profile_media_path,
            commands::add_media_files,
            commands::delete_media_item,
            commands::set_activity_detection,
            commands::current_activity,
            commands::start_bot_chat,
            commands::create_bot,
            commands::list_bots,
            commands::start_chat_with,
            commands::list_groups,
            commands::create_group,
            commands::update_group,
            commands::leave_group,
            commands::group_set_my_tag,
            commands::group_request_join,
            commands::group_send,
            commands::edit_message,
            commands::delete_message,
            commands::attach_file,
            commands::download_file,
            commands::send_call_signal,
            commands::send_control,
            commands::send_sticker,
            commands::send_gif,
            commands::send_voice_message,
            commands::send_circle_video,
            commands::send_edited_image,
            commands::save_sticker,
            commands::import_telegram_stickers,
            commands::list_local_media,
            commands::read_media_bytes,
            commands::read_user_image,
            commands::read_user_font,
            commands::copy_file_to,
        ])
        .run(tauri::generate_context!())
        .map_err(anyhow::Error::from)?;
    Ok(())
}

/// WebKitGTK ships with WebRTC switched off; calls need it (plus media
/// streams for getUserMedia and MSE for in-app media playback).
#[cfg(target_os = "linux")]
mod linux_webview {
    use tauri::Manager;

    pub fn configure(app: &tauri::App) {
        let Some(win) = app.get_webview_window("main") else { return };
        let res = win.with_webview(|wv| {
            use webkit2gtk::{SettingsExt, WebViewExt};
            let view = wv.inner();
            if let Some(s) = WebViewExt::settings(&view) {
                s.set_enable_webrtc(true);
                s.set_enable_media_stream(true);
                s.set_enable_mediasource(true);
                s.set_media_playback_requires_user_gesture(false);
            }
        });
        if let Err(e) = res {
            tracing::warn!("could not configure the WebKitGTK webview: {e}");
        }
    }
}
