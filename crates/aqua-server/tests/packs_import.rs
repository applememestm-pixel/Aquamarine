//! Pack import against a fake Telegram Bot API: files come in parallel,
//! and two requests for the same pack share one download.
//! (Own test binary: it changes the working directory.)

use std::time::{Duration, Instant};

use axum::{extract::Query, routing::get, Json, Router};

const N: usize = 40;
const DELAY_MS: u64 = 250;
// 1×1 webp
const WEBP: &[u8] = &[
    0x52, 0x49, 0x46, 0x46, 0x22, 0, 0, 0, 0x57, 0x45, 0x42, 0x50, 0x56, 0x50, 0x38, 0x20, 0x16, 0, 0, 0, 0x30, 0x01, 0, 0x9d,
    0x01, 0x2a, 0x01, 0, 0x01, 0, 0x0e, 0xc0, 0xfe, 0x25, 0xa4, 0, 0x03, 0x70, 0, 0, 0, 0, 0,
];

async fn sticker_set() -> Json<serde_json::Value> {
    let st: Vec<_> = (0..N).map(|i| serde_json::json!({ "file_id": format!("f{i}"), "emoji": "👍" })).collect();
    Json(serde_json::json!({ "ok": true, "result": { "name": "slowpack", "title": "Slow pack", "sticker_type": "regular", "stickers": st } }))
}

async fn get_file(Query(q): Query<std::collections::HashMap<String, String>>) -> Json<serde_json::Value> {
    tokio::time::sleep(Duration::from_millis(DELAY_MS)).await;
    Json(serde_json::json!({ "ok": true, "result": { "file_path": format!("stickers/{}.webp", q["file_id"]) } }))
}

async fn file() -> &'static [u8] {
    tokio::time::sleep(Duration::from_millis(DELAY_MS)).await;
    WEBP
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn import_is_parallel_and_deduplicated() {
    let dir = std::env::temp_dir().join(format!("aqua-packs-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_current_dir(&dir).unwrap();

    let app = Router::new()
        .route("/bot123:abc/getStickerSet", get(sticker_set))
        .route("/bot123:abc/getFile", get(get_file))
        .route("/file/bot123:abc/stickers/:f", get(file));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    std::env::set_var("AQUA_TG_API", format!("http://{addr}"));
    std::env::set_var("AQUA_TG_BOT_TOKEN", "123:abc");

    let t0 = Instant::now();
    let (a, b) = tokio::join!(
        aqua_server::packs::import("https://t.me/addstickers/slowpack"),
        aqua_server::packs::import("slowpack"),
    );
    let took = t0.elapsed();
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_eq!(a.files.len(), N);
    assert_eq!(b.files.len(), N);
    // In order, like the set.
    assert!(a.files.windows(2).all(|w| w[0].file < w[1].file), "{:?}", a.files.iter().map(|f| &f.file).collect::<Vec<_>>());
    // One at a time this is N × 2 × 250 ms = 20 s.
    assert!(took < Duration::from_secs(6), "took {took:?}");
    assert!(dir.join("aqua-packs/slowpack/manifest.json").is_file());
    let _ = std::fs::remove_dir_all(&dir);
}
