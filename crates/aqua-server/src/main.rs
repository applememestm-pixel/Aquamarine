//! Aquamarine signaling / relay server — thin binary entry point.
//! All of the work lives in the [`aqua_server`] library.

use std::net::SocketAddr;

use aqua_server::{app, AppState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,aqua_server=debug")),
        )
        .init();

    let addr: SocketAddr = std::env::var("AQUA_LISTEN")
        .unwrap_or_else(|_| "127.0.0.1:7777".into())
        .parse()?;

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "aqua-server listening");
    axum::serve(listener, app(AppState::new())).await?;
    Ok(())
}
