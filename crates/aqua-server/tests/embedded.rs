//! The relay inside another program (the app's "Server on this device").

#[tokio::test]
async fn embedded_relay_serves_and_stops() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_current_dir(dir.path()).unwrap();
    std::env::set_var("AQUA_PUBLIC_URL", "off");
    std::env::set_var("AQUA_DEFAULT_PACKS", "");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let task = tokio::spawn(aqua_server::serve_embedded(listener, async { let _ = rx.await; }));
    let body: serde_json::Value = reqwest::get(format!("http://127.0.0.1:{port}/v1/stats")).await.unwrap().json().await.unwrap();
    assert_eq!(body["ok"], true);
    assert!(dir.path().join("aqua-directory.sqlite").exists(), "files live in the working directory");
    tx.send(()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), task).await.unwrap().unwrap().unwrap();
}
