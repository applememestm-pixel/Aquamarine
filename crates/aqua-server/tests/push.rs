//! Waking an absent device (push.rs): only a fixed signal leaves the relay.

use std::time::Duration;

use aqua_core::keys::IdentityKeyPair;
use aqua_server::{
    app,
    auth::{ChallengeRequest, ChallengeResponse, VerifyRequest, VerifyResponse},
    AppState, PayloadKind, RelayMessage,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

const SECRET: &str = "a-long-shared-federation-secret";

async fn token(http: &reqwest::Client, base: &str, id: &IdentityKeyPair) -> String {
    let chal: ChallengeResponse = http.post(format!("{base}/v1/auth/challenge"))
        .json(&ChallengeRequest { identity_dh_pub: URL_SAFE_NO_PAD.encode(id.public().dh_pub.as_bytes()) })
        .send().await.unwrap().json().await.unwrap();
    let sig = id.sign.sign(&URL_SAFE_NO_PAD.decode(&chal.nonce).unwrap());
    let v: VerifyResponse = http.post(format!("{base}/v1/auth/verify"))
        .json(&VerifyRequest {
            identity: id.public(),
            dh_proof: aqua_client_core::transport::sign_in_proof(id, &URL_SAFE_NO_PAD.decode(&chal.nonce).unwrap(), chal.server_eph.as_deref()).unwrap(),
            nonce: chal.nonce,
            signature: URL_SAFE_NO_PAD.encode(sig),
        })
        .send().await.unwrap().json().await.unwrap();
    v.token
}

async fn register(http: &reqwest::Client, base: &str, id: &IdentityKeyPair, user: &str) -> String {
    let tok = token(http, base, id).await;
    let email = format!("{user}@x.com");
    let c: serde_json::Value = http.post(format!("{base}/v1/auth/email/request")).json(&serde_json::json!({ "email": email })).send().await.unwrap().json().await.unwrap();
    let t: serde_json::Value = http.post(format!("{base}/v1/auth/email/verify")).json(&serde_json::json!({ "email": email, "code": c["dev_code"] })).send().await.unwrap().json().await.unwrap();
    let r = http.post(format!("{base}/v1/users/register")).bearer_auth(&tok)
        .json(&serde_json::json!({ "ticket": t["ticket"], "username": user, "display_name": user, "password": "" }))
        .send().await.unwrap();
    assert!(r.status().is_success(), "register {user}: {}", r.text().await.unwrap());
    tok
}

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn ws(addr: &str, user: &str, tok: &str) -> Ws {
    tokio_tungstenite::connect_async(format!("ws://{addr}/v1/ws?user={user}&token={tok}&v=1.21.0-23")).await.unwrap().0
}

async fn recv(ws: &mut Ws) -> RelayMessage {
    loop {
        let m = tokio::time::timeout(Duration::from_secs(8), ws.next()).await.expect("timed out").unwrap().unwrap();
        match m {
            Message::Binary(b) => return serde_json::from_slice(&b).unwrap(),
            Message::Text(t) => return serde_json::from_str(&t).unwrap(),
            _ => continue,
        }
    }
}

fn msg(from: &str, to: &str, body: &[u8]) -> Message {
    Message::Binary(serde_json::to_vec(&RelayMessage { from: from.into(), to: to.into(), payload: body.to_vec(), kind: PayloadKind::Envelope, ts: None, id: None }).unwrap().into())
}

/// A copy on the relay is kept only for accounts with two-step verification

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_absent_device_gets_only_a_wake_signal() {
    use std::sync::{Arc, Mutex};
    aqua_server::push::allow_local_for_tests();
    // A UnifiedPush distributor and a gateway that record what they get.
    let got: Arc<Mutex<Vec<(String, Vec<u8>)>>> = Default::default();
    let gone = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let lp = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let up = format!("http://{}", lp.local_addr().unwrap());
    {
        let (got, gone) = (got.clone(), gone.clone());
        let r = axum::Router::new().fallback(move |req: axum::extract::Request| {
            let (got, gone) = (got.clone(), gone.clone());
            async move {
                let path = req.uri().path().to_string();
                let body = axum::body::to_bytes(req.into_body(), 1 << 20).await.unwrap().to_vec();
                got.lock().unwrap().push((path, body));
                if gone.load(std::sync::atomic::Ordering::SeqCst) { axum::http::StatusCode::GONE } else { axum::http::StatusCode::CREATED }
            }
        });
        tokio::spawn(async move { axum::serve(lp, r).await.unwrap() });
    }
    std::env::set_var("AQUA_PUSH_GATEWAY", &up);
    let la = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let a = la.local_addr().unwrap().to_string();
    let ua = format!("http://{a}");
    let tmp = tempfile::tempdir().unwrap();
    let sa = AppState::isolated(tmp.path().join("a"), None);
    { let s = sa.clone(); tokio::spawn(async move { axum::serve(la, app(s)).await.unwrap() }); }
    let http = reqwest::Client::builder().user_agent(concat!("aqua-client-core/", env!("CARGO_PKG_VERSION"))).build().unwrap();
    let (ia, ib, ic) = (IdentityKeyPair::generate(), IdentityKeyPair::generate(), IdentityKeyPair::generate());
    let ta = register(&http, &ua, &ia, "pushsender").await;
    let tb = register(&http, &ua, &ib, "pushsleeper").await;
    let tc = register(&http, &ua, &ic, "pushgw").await;
    // Bad registrations.
    let st = |r: reqwest::Response| r.status().as_u16();
    assert_eq!(st(http.post(format!("{ua}/v1/me/push")).json(&serde_json::json!({ "kind": "unifiedpush", "endpoint": "https://up.example.org/x" })).send().await.unwrap()), 401, "signed in only");
    assert_eq!(st(http.post(format!("{ua}/v1/me/push")).bearer_auth(&tb).json(&serde_json::json!({ "kind": "carrier-pigeon", "endpoint": "x" })).send().await.unwrap()), 400);
    // bob: UnifiedPush; carol: the gateway.
    assert_eq!(st(http.post(format!("{ua}/v1/me/push")).bearer_auth(&tb).json(&serde_json::json!({ "kind": "unifiedpush", "endpoint": format!("{up}/up/bob") })).send().await.unwrap()), 204);
    assert_eq!(st(http.post(format!("{ua}/v1/me/push")).bearer_auth(&tc).json(&serde_json::json!({ "kind": "gateway", "endpoint": "fcm-token-carol" })).send().await.unwrap()), 204);
    // alice writes to both while they're away — twice to bob.
    let mut wa = ws(&a, "pushsender", &ta).await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    let secret = b"secret text that must never leave";
    wa.send(msg("pushsender", "pushsleeper", secret)).await.unwrap();
    wa.send(msg("pushsender", "pushsleeper", secret)).await.unwrap();
    wa.send(msg("pushsender", "pushgw", secret)).await.unwrap();
    for _ in 0..40 { if got.lock().unwrap().len() >= 2 { break } tokio::time::sleep(Duration::from_millis(100)).await; }
    tokio::time::sleep(Duration::from_millis(300)).await;
    let seen = got.lock().unwrap().clone();
    assert_eq!(seen.len(), 2, "one wake per device (the second message within the debounce adds none): {seen:?}");
    let to_bob = seen.iter().find(|(p, _)| p == "/up/bob").expect("bob woken");
    assert_eq!(to_bob.1, aqua_server::push::WAKE_BODY, "a fixed body");
    let to_gw = seen.iter().find(|(p, _)| p == "/v1/wake").expect("gateway asked");
    let v: serde_json::Value = serde_json::from_slice(&to_gw.1).unwrap();
    assert_eq!(v, serde_json::json!({ "token": "fcm-token-carol", "wake": true }));
    for (_, body) in &seen {
        let text = String::from_utf8_lossy(body);
        for leak in ["secret text", "pushsender", "pushsleeper", "pushgw"] { assert!(!text.contains(leak), "{leak} in {text}"); }
    }
    // The messages are waiting as always; bob gets them when he connects.
    let mut wb = ws(&a, "pushsleeper", &tb).await;
    assert_eq!(recv(&mut wb).await.payload, secret);
    drop(wb);
    // An endpoint the distributor says is gone (410) is forgotten.
    gone.store(true, std::sync::atomic::Ordering::SeqCst);
    tokio::time::sleep(Duration::from_secs(21)).await;
    wa.send(msg("pushsender", "pushsleeper", b"again")).await.unwrap();
    for _ in 0..40 { if sa.directory.push_endpoints("pushsleeper").is_empty() { break } tokio::time::sleep(Duration::from_millis(100)).await; }
    assert!(sa.directory.push_endpoints("pushsleeper").is_empty());
}

/// Review of #113: a wake suppressed by the 20-second limit was lost — the
/// device woken by the first message may be asleep again by the second.
/// Now one more wake follows when the limit ends. Rows of devices that are
/// no longer the account's are dropped, not woken. Redirects aren't followed.
/// The gateway gets this relay's key when one is set.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_suppressed_wake_follows_later_and_stale_devices_are_dropped() {
    use std::sync::{Arc, Mutex};
    aqua_server::push::allow_local_for_tests();
    aqua_server::push::set_debounce_for_tests(Duration::from_millis(800));
    let got: Arc<Mutex<Vec<(String, Option<String>)>>> = Default::default();
    let lp = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let up = format!("http://{}", lp.local_addr().unwrap());
    {
        let got = got.clone();
        let to = up.clone();
        let r = axum::Router::new().fallback(move |req: axum::extract::Request| {
            let (got, to) = (got.clone(), to.clone());
            async move {
                let path = req.uri().path().to_string();
                let auth = req.headers().get("authorization").and_then(|v| v.to_str().ok()).map(str::to_string);
                got.lock().unwrap().push((path.clone(), auth));
                if path == "/redirect" {
                    return axum::response::Response::builder().status(307).header("location", format!("{to}/followed")).body(axum::body::Body::empty()).unwrap();
                }
                axum::response::Response::builder().status(201).body(axum::body::Body::empty()).unwrap()
            }
        });
        tokio::spawn(async move { axum::serve(lp, r).await.unwrap() });
    }
    let la = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let a = la.local_addr().unwrap().to_string();
    let ua = format!("http://{a}");
    let tmp = tempfile::tempdir().unwrap();
    let sa = AppState::isolated(tmp.path().join("a"), None);
    { let s = sa.clone(); tokio::spawn(async move { axum::serve(la, app(s)).await.unwrap() }); }
    let http = reqwest::Client::builder().user_agent(concat!("aqua-client-core/", env!("CARGO_PKG_VERSION"))).build().unwrap();
    let (ia, ib, ic) = (IdentityKeyPair::generate(), IdentityKeyPair::generate(), IdentityKeyPair::generate());
    let ta = register(&http, &ua, &ia, "trailsender").await;
    let tb = register(&http, &ua, &ib, "trailsleeper").await;
    let tc = register(&http, &ua, &ic, "trailredir").await;
    assert_eq!(http.post(format!("{ua}/v1/me/push")).bearer_auth(&tb).json(&serde_json::json!({ "kind": "unifiedpush", "endpoint": format!("{up}/up/bob") })).send().await.unwrap().status().as_u16(), 204);
    assert_eq!(http.post(format!("{ua}/v1/me/push")).bearer_auth(&tc).json(&serde_json::json!({ "kind": "unifiedpush", "endpoint": format!("{up}/redirect") })).send().await.unwrap().status().as_u16(), 204);
    // A device that isn't bob's (signed out, or never his): not woken, dropped.
    let stranger = hex::encode([7u8; 32]);
    sa.directory.push_register("trailsleeper", &stranger, "unifiedpush", &format!("{up}/up/stranger")).unwrap();
    let mut wa = ws(&a, "trailsender", &ta).await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    wa.send(msg("trailsender", "trailsleeper", b"one")).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    wa.send(msg("trailsender", "trailsleeper", b"two")).await.unwrap();
    wa.send(msg("trailsender", "trailredir", b"x")).await.unwrap();
    tokio::time::sleep(Duration::from_millis(1800)).await;
    let seen = got.lock().unwrap().clone();
    let to_bob = seen.iter().filter(|(p, _)| p == "/up/bob").count();
    assert_eq!(to_bob, 2, "the first at once, one more when the limit ends: {seen:?}");
    assert!(!seen.iter().any(|(p, _)| p == "/up/stranger"), "a device not of the account isn't woken");
    assert!(sa.directory.push_endpoints("trailsleeper").iter().all(|(d, _, _)| *d != stranger), "and its row is gone");
    assert!(seen.iter().any(|(p, _)| p == "/redirect") && !seen.iter().any(|(p, _)| p == "/followed"), "a redirect isn't followed");
    // The gateway: this relay's key with every wake.
    std::env::set_var("AQUA_PUSH_GATEWAY", &up);
    std::env::set_var("AQUA_PUSH_GATEWAY_KEY", "relay-key-for-gateway");
    assert_eq!(http.post(format!("{ua}/v1/me/push")).bearer_auth(&tc).json(&serde_json::json!({ "kind": "gateway", "endpoint": "tok" })).send().await.unwrap().status().as_u16(), 204);
    tokio::time::sleep(Duration::from_millis(900)).await;
    wa.send(msg("trailsender", "trailredir", b"y")).await.unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    let seen = got.lock().unwrap().clone();
    let gw = seen.iter().find(|(p, _)| p == "/v1/wake").expect("gateway asked");
    assert_eq!(gw.1.as_deref(), Some("Bearer relay-key-for-gateway"));
}
