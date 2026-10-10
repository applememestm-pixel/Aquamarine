//! Aquamarine signaling / relay server — thin binary entry point.
//! All of the work lives in the [`aqua_server`] library.

use std::net::SocketAddr;

use aqua_server::{app, lan_ip, AppState};
use nu_ansi_term::{Color, Style};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Old Windows consoles print ANSI colour codes as garbage ("←[2m…");
    // turn on VT processing, or fall back to plain logs.
    #[cfg(windows)]
    let ansi = nu_ansi_term::enable_ansi_support().is_ok();
    #[cfg(not(windows))]
    let ansi = true;

    tracing_subscriber::fmt()
        .with_ansi(ansi)
        .with_target(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,aqua_server=info")),
        )
        .init();

    let listen = std::env::var("AQUA_LISTEN").unwrap_or_else(|_| "127.0.0.1:7777".into());
    let addr: SocketAddr = listen.parse()?;

    // Admin panel: on by default with a fresh one-off token, unless the
    // operator set their own AQUA_ADMIN_TOKEN or turned it off (AQUA_ADMIN=off).
    let admin_off = std::env::var("AQUA_ADMIN").map(|v| v == "off" || v == "0").unwrap_or(false);
    let token = match std::env::var("AQUA_ADMIN_TOKEN") {
        Ok(t) if !t.is_empty() => Some(t),
        _ if admin_off => None,
        _ => {
            use rand_core::{OsRng, RngCore};
            let mut b = [0u8; 12];
            OsRng.fill_bytes(&mut b);
            let t = hex::encode(b);
            // Still single-threaded here (before the server starts).
            std::env::set_var("AQUA_ADMIN_TOKEN", &t);
            Some(t)
        }
    };

    let listener = tokio::net::TcpListener::bind(addr).await?;
    let local = format!("http://127.0.0.1:{}", addr.port());
    let panel = token.as_ref().map(|t| format!("{local}/admin#token={t}"));
    banner(ansi, &addr, &local, panel.as_deref());

    if let (Some(url), true) = (panel.as_ref(), std::env::var("AQUA_OPEN_PANEL").map(|v| v == "1").unwrap_or(false)) {
        open_in_browser(url);
    }

    tracing::info!(%addr, "aqua-server listening");
    axum::serve(listener, app(AppState::new())).await?;
    Ok(())
}

fn banner(ansi: bool, addr: &SocketAddr, local: &str, panel: Option<&str>) {
    let paint = |s: &str, st: Style| if ansi { st.paint(s).to_string() } else { s.to_string() };
    let teal = Color::Rgb(0, 229, 199).bold();
    let violet = Color::Rgb(123, 92, 255).normal();
    let dim = Color::Rgb(140, 150, 165).normal();
    let fg = Style::new().bold();
    let line = paint(&"─".repeat(60), violet);

    println!();
    println!("  {}  {}", paint("◆ Aquamarine", teal), paint("relay server", dim));
    println!("  {line}");
    println!("  {}  {}", paint("This PC     ", dim), paint(local, fg));
    if addr.ip().is_unspecified() {
        match lan_ip() {
            Some(ip) => println!("  {}  {}", paint("Phones/LAN  ", dim), paint(&format!("http://{ip}:{}", addr.port()), fg)),
            None => println!("  {}  {}", paint("Phones/LAN  ", dim), paint("no LAN address found", dim)),
        }
    } else {
        println!("  {}  {}", paint("Phones/LAN  ", dim), paint("off — set AQUA_LISTEN=0.0.0.0:7777", dim));
    }
    println!("  {}  {}", paint("Emulator    ", dim), paint(&format!("http://10.0.2.2:{}", addr.port()), fg));
    match panel {
        Some(p) => println!("  {}  {}", paint("Admin panel ", dim), paint(p, teal)),
        None => println!("  {}  {}", paint("Admin panel ", dim), paint("off", dim)),
    }
    println!("  {line}");
    println!("  {}", paint("Email codes (dev mode) appear below and in the admin panel.", dim));
    println!("  {}", paint("Close this window to stop the server.", dim));
    println!();
}

fn open_in_browser(url: &str) {
    #[cfg(windows)]
    let r = std::process::Command::new("cmd").args(["/C", "start", "", url]).spawn();
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(url).spawn();
    if let Err(e) = r {
        tracing::warn!(error = %e, "could not open the admin panel in a browser");
    }
}
