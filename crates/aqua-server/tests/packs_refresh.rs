//! Refreshing a stored pack downloads only what's new on Telegram, drops
//! what was removed and follows Telegram's order.
//! (Own test binary: it changes the working directory.)

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use axum::{extract::Query, routing::get, Json, Router};

const WEBP: &[u8] = &[
    0x52, 0x49, 0x46, 0x46, 0x22, 0, 0, 0, 0x57, 0x45, 0x42, 0x50, 0x56, 0x50, 0x38, 0x20, 0x16, 0, 0, 0, 0x30, 0x01, 0, 0x9d,
    0x01, 0x2a, 0x01, 0, 0x01, 0, 0x0e, 0xc0, 0xfe, 0x25, 0xa4, 0, 0x03, 0x70, 0, 0, 0, 0, 0,
];

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn refresh_downloads_only_new_stickers() {
    let dir = std::env::temp_dir().join(format!("aqua-packs-refresh-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_current_dir(&dir).unwrap();

    // The set: version 1 = a b c; version 2 = d a c (b removed, d new, first).
    let version = Arc::new(AtomicUsize::new(1));
    let downloads = Arc::new(AtomicUsize::new(0));
    let v = version.clone();
    let d = downloads.clone();
    let app = Router::new()
        .route("/bot1:x/getStickerSet", get(move || {
            let v = v.load(Ordering::SeqCst);
            async move {
                let ids: &[&str] = if v == 1 { &["a", "b", "c"] } else { &["d", "a", "c"] };
                let st: Vec<_> = ids.iter().map(|i| serde_json::json!({ "file_id": i, "file_unique_id": format!("u{i}"), "emoji": "🙂" })).collect();
                Json(serde_json::json!({ "ok": true, "result": { "name": "live", "title": "Live", "sticker_type": "regular", "stickers": st } }))
            }
        }))
        .route("/bot1:x/getFile", get(|Query(q): Query<std::collections::HashMap<String, String>>| async move {
            Json(serde_json::json!({ "ok": true, "result": { "file_path": format!("s/{}.webp", q["file_id"]) } }))
        }))
        .route("/file/bot1:x/s/:f", get(move || { d.fetch_add(1, Ordering::SeqCst); async { WEBP } }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    std::env::set_var("AQUA_TG_API", format!("http://{addr}"));
    std::env::set_var("AQUA_TG_BOT_TOKEN", "1:x");

    let m1 = aqua_server::packs::import("live").await.unwrap();
    assert_eq!(m1.files.len(), 3);
    assert_eq!(downloads.load(Ordering::SeqCst), 3);
    assert_eq!(m1.rev, 1);

    // Pretend the copy is older than a minute, change the set, refresh.
    let mut old = m1.clone();
    old.updated -= 120;
    std::fs::write("aqua-packs/live/manifest.json", serde_json::to_vec(&old).unwrap()).unwrap();
    version.store(2, Ordering::SeqCst);
    let m2 = aqua_server::packs::sync("live", true).await.unwrap();
    assert_eq!(downloads.load(Ordering::SeqCst), 4, "only the new sticker is downloaded");
    let ids: Vec<&str> = m2.files.iter().map(|f| f.id.as_str()).collect();
    assert_eq!(ids, ["ud", "ua", "uc"], "Telegram's order");
    assert_eq!(m2.rev, 2);
    for f in &m2.files {
        assert!(dir.join("aqua-packs/live").join(&f.file).is_file(), "{} on disk", f.file);
    }
    let b = m1.files.iter().find(|f| f.id == "ub").unwrap();
    assert!(!dir.join("aqua-packs/live").join(&b.file).exists(), "removed sticker is gone");

    // Right after a refresh another one is a no-op (rate limit).
    let m3 = aqua_server::packs::sync("live", true).await.unwrap();
    assert_eq!(m3.rev, 2);
    assert_eq!(downloads.load(Ordering::SeqCst), 4);
    let _ = std::fs::remove_dir_all(&dir);
}
