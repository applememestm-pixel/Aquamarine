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

/// The unread count on the app's icon: a red badge in the corner of the
/// taskbar button (Windows — an overlay icon drawn here), the dock badge
/// (macOS), the launcher count (Linux, where the desktop supports it).
/// 0 takes it off.
#[tauri::command]
pub fn badge_set(app: AppHandle, unread: u32) -> Result<(), String> {
    #[cfg(desktop)]
    {
        let Some(w) = app.get_webview_window("main") else { return Ok(()) };
        #[cfg(windows)]
        {
            let icon = (unread > 0).then(|| {
                let (w_, h, rgba) = badge_rgba(unread);
                tauri::image::Image::new_owned(rgba, w_, h)
            });
            w.set_overlay_icon(icon).map_err(|e| e.to_string())?;
        }
        #[cfg(not(windows))]
        {
            let _ = w.set_badge_count((unread > 0).then_some(unread as i64));
        }
    }
    #[cfg(mobile)]
    {
        let _ = (app, unread);
    }
    Ok(())
}

/// 3×5 pixel digits (bit 2 = left column), "+" last.
const GLYPHS: [[u8; 5]; 11] = [
    [7, 5, 5, 5, 7], [2, 6, 2, 2, 7], [7, 1, 7, 4, 7], [7, 1, 7, 1, 7], [5, 5, 7, 1, 1],
    [7, 4, 7, 1, 7], [7, 4, 7, 5, 7], [7, 1, 2, 2, 2], [7, 5, 7, 5, 7], [7, 5, 7, 1, 7],
    [0, 2, 7, 2, 0],
];

/// A 32×32 red disc with the count in white ("99+" past 99), RGBA.
#[cfg_attr(not(windows), allow(dead_code))]
fn badge_rgba(n: u32) -> (u32, u32, Vec<u8>) {
    const S: usize = 32;
    let mut px = vec![0u8; S * S * 4];
    // Disc, edge smoothed by coverage.
    let c = (S as f32 - 1.0) / 2.0;
    for y in 0..S {
        for x in 0..S {
            let d = ((x as f32 - c).powi(2) + (y as f32 - c).powi(2)).sqrt();
            let a = (c + 0.5 - d).clamp(0.0, 1.0);
            if a > 0.0 {
                let i = (y * S + x) * 4;
                px[i..i + 4].copy_from_slice(&[0xE5, 0x39, 0x35, (a * 255.0) as u8]);
            }
        }
    }
    let text: Vec<usize> = if n > 99 { vec![9, 9, 10] } else { n.to_string().bytes().map(|b| (b - b'0') as usize).collect() };
    // Scale 3 for one or two digits, 2 for "99+".
    let k = if text.len() > 2 { 2 } else { 3 };
    let gw = 3 * k;
    let w = text.len() * gw + (text.len() - 1) * k;
    let (x0, y0) = ((S - w) / 2, (S - 5 * k) / 2);
    for (ci, &g) in text.iter().enumerate() {
        for (row, bits) in GLYPHS[g].iter().enumerate() {
            for col in 0..3 {
                if bits & (4 >> col) == 0 {
                    continue;
                }
                for dy in 0..k {
                    for dx in 0..k {
                        let (x, y) = (x0 + ci * (gw + k) + col * k + dx, y0 + row * k + dy);
                        let i = (y * S + x) * 4;
                        px[i..i + 4].copy_from_slice(&[255, 255, 255, 255]);
                    }
                }
            }
        }
    }
    (S as u32, S as u32, px)
}

#[cfg(test)]
mod tests {
    #[test]
    fn badge_fits() {
        for n in [1, 7, 42, 99, 100, 12345] {
            let (w, h, px) = super::badge_rgba(n);
            assert_eq!(px.len(), (w * h * 4) as usize);
            assert!(px.chunks(4).any(|p| p == [255, 255, 255, 255]));
        }
    }
}
