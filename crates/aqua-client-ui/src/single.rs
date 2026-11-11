//! One window per data folder (desktop). Starting Aquamarine again — a
//! second click on the icon, autostart plus a manual start — used to open
//! a second copy on the same vault: two windows, two connections, and
//! messages shown in one of them only. Now the first copy holds a loopback
//! port derived from its data folder; a second start finds it taken, asks
//! the first copy to come to the front and quits. (AQUA_DATA_DIR still
//! gives a separate client: another folder, another port.)

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};
use std::path::Path;
use std::time::Duration;

use tauri::{AppHandle, Manager};

const HELLO: &[u8] = b"aquamarine:show\n";

fn port_of(dir: &Path) -> u16 {
    // FNV-1a of the path → 40000..=59999.
    let mut h: u64 = 0xcbf29ce484222325;
    for b in dir.to_string_lossy().to_lowercase().bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    40000 + (h % 20000) as u16
}

/// `true`: this is the only copy (keep going). `false`: another copy was
/// asked to show itself — quit.
pub fn claim(app: &AppHandle, dir: &Path) -> bool {
    let addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port_of(dir));
    match TcpListener::bind(addr) {
        Ok(l) => {
            let app = app.clone();
            std::thread::spawn(move || {
                for c in l.incoming() {
                    let Ok(mut c) = c else { continue };
                    let _ = c.set_read_timeout(Some(Duration::from_secs(2)));
                    let mut buf = [0u8; 32];
                    let n = c.read(&mut buf).unwrap_or(0);
                    if &buf[..n] != HELLO {
                        continue;
                    }
                    let _ = c.write_all(b"ok\n");
                    let app2 = app.clone();
                    let _ = app.run_on_main_thread(move || {
                        if let Some(w) = app2.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.unminimize();
                            let _ = w.set_focus();
                        }
                    });
                }
            });
            true
        }
        Err(_) => {
            // Taken: by us (answers "ok") or by some other program (then
            // just start as before).
            let Ok(mut c) = TcpStream::connect_timeout(&addr.into(), Duration::from_millis(800)) else { return true };
            let _ = c.set_read_timeout(Some(Duration::from_secs(2)));
            if c.write_all(HELLO).is_err() {
                return true;
            }
            let mut buf = [0u8; 8];
            let n = c.read(&mut buf).unwrap_or(0);
            if &buf[..n] == b"ok\n" {
                tracing::info!("Aquamarine is already running — brought it to the front");
                false
            } else {
                true
            }
        }
    }
}
