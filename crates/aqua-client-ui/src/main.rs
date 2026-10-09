// Prevents an extra console window from opening on Windows in release
// mode. This must appear before any use statements.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Err(e) = aqua_client_ui::run() {
        eprintln!("aquamarine: {e:?}");
        std::process::exit(1);
    }
}
