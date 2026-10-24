//! Aquamarine Tauri desktop shell.
//!
//! Entry point for the mobile / desktop Tauri app. Registers the
//! command handlers from [`commands`] and wraps the shared
//! [`state::AppState`] in a `Mutex` for cross-command access.

pub mod state;
pub mod commands;
pub mod media;
pub mod activity;
pub mod web;
pub mod media_server;
pub mod personal;
pub mod history;
pub mod backups;
pub mod gpu;
pub mod stickers;
pub mod multidev;
pub mod autodelete;
pub mod windows;
pub mod bluetooth;
pub mod servers;
pub mod mesh;
pub mod relay_host;
pub mod gallery;
pub mod accounts;
pub mod updates;

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
    // Before any thread or webview exists: the web process inherits it.
    #[cfg(target_os = "linux")]
    media_server::prefer_software_video_decoding();
    let _ = gpu::init();
    let builder = tauri::Builder::default();
    // Android: the Bluetooth sockets of nearby delivery live in Kotlin.
    #[cfg(target_os = "android")]
    let builder = builder.plugin(mesh::android_plugin()).plugin(relay_host::android::plugin()).plugin(gallery::android::plugin()).plugin(updates::android::plugin());
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
        // Info by default (the plugin's default is Trace: every WebSocket
        // read and keyring call). AQUA_LOG=debug|trace for more.
        .plugin(
            tauri_plugin_log::Builder::default()
                .level(match std::env::var("AQUA_LOG").as_deref() {
                    Ok("trace") => log::LevelFilter::Trace,
                    Ok("debug") => log::LevelFilter::Debug,
                    Ok("warn") => log::LevelFilter::Warn,
                    _ => log::LevelFilter::Info,
                })
                .build(),
        )
        .setup(|app| {
            // Priority: AQUA_DATA_DIR env var > OS app-data dir > temp dir.
            // The env var is how you run two independent clients on the
            // same machine (each with its own identity + vault): set
            // AQUA_DATA_DIR to a distinct path per launch.
            let base_dir = std::env::var_os("AQUA_DATA_DIR")
                .map(std::path::PathBuf::from)
                .or_else(|| app.path().app_data_dir().ok())
                .unwrap_or_else(|| std::env::temp_dir().join("aquamarine-dev"));
            // Several accounts on this device: each has its own directory
            // (accounts.rs); the first one is the base directory itself.
            let data_dir = accounts::resolve(&base_dir);
            app.manage(accounts::Base(base_dir));
            tracing::info!(path = %data_dir.display(), "aquamarine data directory");
            // Cached media (web previews, article videos / music) is played
            // through the asset protocol — also when AQUA_DATA_DIR points
            // outside the default scope (test clients on another drive).
            let _ = app.asset_protocol_scope().allow_directory(&data_dir, true);
            let state = state::AppState::boot(data_dir)?;
            app.manage(state);
            // Write changed chat history to the encrypted vault every few seconds.
            {
                let handle = app.handle().clone();
                std::thread::spawn(move || loop {
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    let st = handle.state::<std::sync::Mutex<state::AppState>>();
                    let guard = st.lock();
                    if let Ok(mut s) = guard {
                        history::flush(&mut s);
                    }
                });
            }
            commands::spawn_activity_watcher(app.handle().clone());
            autodelete::spawn(app.handle().clone());
            // Nearby delivery (Bluetooth, like Briar): listen for contacts.
            {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move { mesh::start(handle).await });
            }
            #[cfg(target_os = "linux")]
            linux_webview::configure(app);
            Ok(())
        })
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            mesh::mesh_status,
            mesh::mesh_set_enabled,
            mesh::mesh_sync_now,
            bluetooth::bt_status,
            bluetooth::bt_devices,
            bluetooth::bt_power,
            bluetooth::bt_send,
            bluetooth::bt_answer,
            bluetooth::bt_receive,
            bluetooth::bt_show_inbox,
            web::link_preview,
            commands::upload_attachment,
            commands::prepare_banner_video,
            gpu::gpu_status,
            stickers::create_sticker_pack,
            stickers::pack_add_files,
            stickers::pack_set_order,
            stickers::pack_set_icon,
            stickers::sticker_set_emoji,
            stickers::sticker_delete,
            stickers::sync_telegram_pack,
            stickers::check_pack_updates,
            stickers::pack_preview,
            media_server::transcode_media,
            gpu::set_gpu_mode,
            gpu::set_dmabuf,
            gpu::confirm_dmabuf,
            gpu::restart_app,
            commands::fetch_attachment,
            commands::allow_media_preview,
            media_server::media_stream_url,
            media_server::probe_media,
            media_server::media_support,
            personal::save_sound,
            personal::list_sounds,
            personal::delete_sound,
            personal::write_theme_file,
            personal::read_theme_file,
            personal::save_console_bot,
            personal::save_temp_file,
            personal::set_window_icon,
            personal::save_wallpaper,
            personal::games_search,
            personal::game_icon,
            personal::game_cover,
            personal::load_keybindings,
            personal::save_keybindings,
            personal::open_keybindings_file,
            personal::list_wallpapers,
            personal::delete_wallpaper,
            commands::fed_peers,
            relay_host::relay_host_status,
            relay_host::relay_host_start,
            relay_host::relay_host_stop,
            gallery::gallery_recent,
            gallery::gallery_ask,
            gallery::gallery_albums,
            servers::server_ping,
            accounts::accounts_list,
            accounts::account_switch,
            accounts::account_forget,
            accounts::app_relaunch,
            updates::update_check,
            updates::update_download,
            updates::update_install,
            servers::probe_servers,
            servers::scan_lan_relays,
            backups::backup_status,
            backups::backup_enable,
            backups::backup_phrase,
            backups::recovery_status,
            autodelete::get_chat_ttl,
            commands::bot_send_raw,
            commands::capsule_create,
            commands::capsules_mine,
            commands::capsule_stop,
            commands::capsule_open,
            multidev::handoff,
            commands::group_control_admins,
            commands::forward_file,
            personal::write_text_file,
            windows::open_chat_window,
            commands::webapp_init,
            commands::webapp_menu,
            autodelete::set_chat_ttl,
            backups::recovery_create,
            backups::password_reset,
            backups::backup_now,
            backups::backup_disable,
            backups::backup_restore,
            web::cache_remote_media,
            web::telegraph_page,
            web::open_external,
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
            commands::set_profile_media_file,
            commands::clear_profile_image,
            commands::set_photo_privacy,
            commands::group_control,
            commands::group_control_to,
            commands::set_local_prefs,
            commands::login_account,
            multidev::list_devices,
            multidev::remove_device,
            multidev::rename_device,
            commands::delete_group_message_of,
            commands::profile_media_path,
            commands::add_media_files,
            commands::delete_media_item,
            commands::delete_sticker_pack,
            commands::rename_sticker_pack,
            commands::set_activity_detection,
            commands::current_activity,
            commands::start_bot_chat,
            commands::create_bot,
            commands::list_bots,
            commands::delete_bot,
            commands::start_chat_with,
            commands::list_groups,
            commands::create_group,
            commands::update_group,
            commands::leave_group,
            commands::group_set_my_tag,
            commands::group_request_join,
            commands::group_send,
            commands::set_group_room,
            commands::edit_message,
            commands::delete_message,
            commands::delete_messages_local,
            commands::attach_file,
            commands::attach_file_as,
            commands::attach_bytes,
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
            commands::server_packs,
            commands::import_pack_via_server,
            commands::pack_file_path,
            commands::list_local_media,
            commands::read_media_bytes,
            commands::read_user_image,
            commands::read_user_font,
            commands::copy_file_to,
        ])
        .build(tauri::generate_context!())
        .map(|app| {
            app.run(|handle, ev| {
                if let tauri::RunEvent::Exit = ev {
                    let st = handle.state::<std::sync::Mutex<state::AppState>>();
                    let guard = st.lock();
                    if let Ok(mut s) = guard {
                        history::flush(&mut s);
                    }
                }
            })
        })
        .map_err(anyhow::Error::from)?;
    Ok(())
}

/// WebKitGTK ships with WebRTC switched off; calls need it (plus media
/// streams for getUserMedia and MSE for in-app media playback).
#[cfg(target_os = "linux")]
pub mod linux_webview {
    use tauri::Manager;

    pub fn configure(app: &tauri::App) {
        let Some(win) = app.get_webview_window("main") else { return };
        configure_window(&win);
    }

    /// WebKitGTK settings every window of ours needs (calls, media, logs).
    pub fn configure_window(win: &tauri::WebviewWindow) {
        let res = win.with_webview(|wv| {
            use webkit2gtk::{SettingsExt, WebViewExt};
            let view = wv.inner();
            if let Some(s) = WebViewExt::settings(&view) {
                s.set_enable_webrtc(true);
                s.set_enable_media_stream(true);
                s.set_enable_mediasource(true);
                s.set_media_playback_requires_user_gesture(false);
                // JS console → stdout, so tools/aquamarine.sh's log has it.
                s.set_enable_write_console_messages_to_stdout(true);
                // No usable GPU (see gpu.rs): paint on the CPU directly
                // instead of compositing through emulated OpenGL.
                if crate::gpu::gpu_status().software {
                    s.set_hardware_acceleration_policy(webkit2gtk::HardwareAccelerationPolicy::Never);
                }
            }
        });
        if let Err(e) = res {
            tracing::warn!("could not configure the WebKitGTK webview: {e}");
        }
    }
}
