//! Review of #115 (27.11): wakes had no limit of their own — a burst for many
//! accounts started as many requests at once, and waited without bound. Own
//! file: the limits are for the whole process.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use aqua_server::AppState;
use tokio::net::TcpListener;

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn wakes_are_limited_at_once_and_in_all() {
    aqua_server::push::allow_local_for_tests();
    aqua_server::push::set_debounce_for_tests(Duration::from_millis(10));
    aqua_server::push::set_limits_for_tests(3, 6);
    // A slow distributor that counts what runs at once.
    let (now, max, total) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let up = format!("http://{}", l.local_addr().unwrap());
    {
        let (now, max, total) = (now.clone(), max.clone(), total.clone());
        let r = axum::Router::new().fallback(move || {
            let (now, max, total) = (now.clone(), max.clone(), total.clone());
            async move {
                let n = now.fetch_add(1, Ordering::SeqCst) + 1;
                max.fetch_max(n, Ordering::SeqCst);
                total.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(400)).await;
                now.fetch_sub(1, Ordering::SeqCst);
                axum::http::StatusCode::CREATED
            }
        });
        tokio::spawn(async move { axum::serve(l, r).await.unwrap() });
    }
    let tmp = tempfile::tempdir().unwrap();
    let sa = AppState::isolated(tmp.path().join("a"), None);
    // Twenty accounts, one device each (straight into the directory).
    for i in 0..20 {
        let id = aqua_core::keys::IdentityKeyPair::generate();
        let user = format!("burst{i}");
        sa.directory.register_plain(&user, id.public().dh_pub.as_bytes(), "long-enough-password", &user).unwrap();
        sa.directory.push_register(&user, &hex::encode(id.public().dh_pub.as_bytes()), "unifiedpush", &format!("{up}/u{i}")).unwrap();
    }
    for i in 0..20 {
        aqua_server::push::wake(&sa, &format!("burst{i}"));
    }
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert!(max.load(Ordering::SeqCst) <= 3, "at most 3 at once: {}", max.load(Ordering::SeqCst));
    assert_eq!(total.load(Ordering::SeqCst), 6, "at most 6 waiting in all, the rest dropped");
}
