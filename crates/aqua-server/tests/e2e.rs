//! End-to-end integration test.
//!
//! Boots the real server on an ephemeral port, then drives two clients
//! (Alice and Bob) through the full flow:
//!
//! * `POST /v1/auth/challenge` → receive nonce
//! * sign the nonce with the identity Ed25519 key
//! * `POST /v1/auth/verify`    → receive bearer token
//! * `POST /v1/bundle/:user`   (authenticated) → publish prekey bundle
//! * open WebSocket with `?token=...`
//! * fetch peer's bundle over HTTP
//! * exchange the X3DH initial message
//! * decrypt on the responder
//! * swap several subsequent Double-Ratchet-encrypted messages
//!
//! This is the acceptance test for "the crypto layer plus the auth
//! layer plus the wire format plus the server all work together."

use std::{collections::HashMap, net::SocketAddr, time::Duration};

use aqua_core::{
    keys::{
        IdentityKeyPair, OneTimePreKey, OneTimePreKeyPublic, PreKeyBundle, PreKeyId,
        SignedPreKey,
    },
    session::Session,
};
use aqua_server::{
    app,
    auth::{ChallengeRequest, ChallengeResponse, VerifyRequest, VerifyResponse},
    AppState, BundleUpload, PayloadKind, RelayMessage,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

async fn spawn_server() -> SocketAddr {
    // A fresh directory DB per test run (not the repo's aqua-directory.sqlite,
    // which kept "owner" from the last run and failed the register step).
    static DB: std::sync::Once = std::sync::Once::new();
    DB.call_once(|| {
        let p = std::env::temp_dir().join(format!("aqua-e2e-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&p);
        std::env::set_var("AQUA_DIR_DB", p);
    });
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    // Open signup: the e2e test exercises the raw relay with ad-hoc
    // usernames that are never email-registered.
    let state = AppState::new_open();
    tokio::spawn(async move {
        axum::serve(listener, app(state)).await.unwrap();
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    addr
}

struct Client {
    _id: String,
    identity: IdentityKeyPair,
    spks: HashMap<PreKeyId, SignedPreKey>,
    opks: HashMap<PreKeyId, OneTimePreKey>,
    token: String,
}

impl Client {
    fn new(id: &str) -> Self {
        Self {
            _id: id.into(),
            identity: IdentityKeyPair::generate(),
            spks: HashMap::new(),
            opks: HashMap::new(),
            token: String::new(),
        }
    }

    fn fresh_bundle(&mut self, n_opks: u32) -> BundleUpload {
        let spk = SignedPreKey::generate(1, &self.identity);
        let spk_pub = spk.public();
        self.spks.insert(spk.id, spk);
        let mut opk_pubs: Vec<OneTimePreKeyPublic> = Vec::with_capacity(n_opks as usize);
        for i in 0..n_opks {
            let opk = OneTimePreKey::generate(i + 1);
            opk_pubs.push(opk.public());
            self.opks.insert(opk.id, opk);
        }
        BundleUpload {
            identity: self.identity.public(),
            signed_prekey: spk_pub,
            one_time_prekeys: opk_pubs,
        }
    }

    /// Run the challenge/verify flow and stash the resulting token.
    async fn login(&mut self, http: &reqwest::Client, base: &str) {
        // 1. Challenge.
        let chal_req = ChallengeRequest {
            identity_dh_pub: URL_SAFE_NO_PAD.encode(self.identity.public().dh_pub.as_bytes()),
        };
        let chal: ChallengeResponse = http
            .post(format!("{base}/v1/auth/challenge"))
            .json(&chal_req)
            .send().await.unwrap()
            .json().await.unwrap();

        // 2. Sign the raw nonce bytes.
        let nonce_bytes = URL_SAFE_NO_PAD.decode(&chal.nonce).unwrap();
        let sig = self.identity.sign.sign(&nonce_bytes);

        // 3. Verify.
        let verify_req = VerifyRequest {
            identity: self.identity.public(),
            nonce: chal.nonce.clone(),
            signature: URL_SAFE_NO_PAD.encode(sig),
        };
        let resp = http
            .post(format!("{base}/v1/auth/verify"))
            .json(&verify_req)
            .send().await.unwrap();
        assert!(resp.status().is_success(), "verify failed: {}", resp.status());
        let vr: VerifyResponse = resp.json().await.unwrap();
        assert!(!vr.token.is_empty());
        self.token = vr.token;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn full_authenticated_e2e_conversation() {
    let addr = spawn_server().await;
    let base = format!("http://{addr}");
    let http = reqwest::Client::new();

    // ------- 1. Both parties log in and get tokens -------
    let mut alice = Client::new("alice");
    let mut bob = Client::new("bob");
    alice.login(&http, &base).await;
    bob.login(&http, &base).await;
    assert_ne!(alice.token, bob.token);

    // ------- 2. Publish bundles (authenticated) -------
    let a_bundle = alice.fresh_bundle(3);
    let b_bundle = bob.fresh_bundle(3);
    let r = http
        .post(format!("{base}/v1/bundle/alice"))
        .bearer_auth(&alice.token)
        .json(&a_bundle)
        .send().await.unwrap();
    assert!(r.status().is_success(), "alice bundle upload: {}", r.status());
    let r = http
        .post(format!("{base}/v1/bundle/bob"))
        .bearer_auth(&bob.token)
        .json(&b_bundle)
        .send().await.unwrap();
    assert!(r.status().is_success(), "bob bundle upload: {}", r.status());

    // ------- 2b. Username squat: an attacker with their own token tries
    // ------- to claim "alice" — must be refused. Use a throwaway
    // ------- identity so Bob's real prekey secrets are left alone.
    {
        let mut squatter = Client::new("mallory");
        squatter.login(&http, &base).await;
        let bundle = squatter.fresh_bundle(0);
        let r = http
            .post(format!("{base}/v1/bundle/alice"))
            .bearer_auth(&squatter.token)
            .json(&bundle)
            .send().await.unwrap();
        assert_eq!(r.status(), reqwest::StatusCode::FORBIDDEN);
    }

    // ------- 3. Open WebSockets with tokens -------
    let (mut a_ws, _) = tokio_tungstenite::connect_async(format!(
        "ws://{addr}/v1/ws?user=alice&token={}",
        alice.token
    )).await.unwrap();
    let (mut b_ws, _) = tokio_tungstenite::connect_async(format!(
        "ws://{addr}/v1/ws?user=bob&token={}",
        bob.token
    )).await.unwrap();

    // WS with no token → refused.
    let bad = tokio_tungstenite::connect_async(format!("ws://{addr}/v1/ws?user=alice&token=")).await;
    assert!(bad.is_err(), "empty token must be rejected");

    // ------- 4. Alice fetches Bob's bundle (public, no token) -------
    let bob_bundle: PreKeyBundle = http
        .get(format!("{base}/v1/bundle/bob"))
        .send().await.unwrap()
        .json().await.unwrap();
    assert!(bob_bundle.verify().is_ok());
    let bob_used_opk_id = bob_bundle.one_time_prekey.as_ref().map(|o| o.id).unwrap();

    // ------- 5. Alice starts the session and sends the initial message -------
    let (mut alice_sess, initial) =
        Session::start_as_initiator(&alice.identity, &bob_bundle, b"hi bob, this is alice").unwrap();
    let payload = bincode::serialize(&initial).unwrap();
    let relay = RelayMessage {
        from: String::new(),
        to: "bob".into(),
        payload,
        kind: PayloadKind::Initial,
    };
    a_ws.send(Message::Binary(serde_json::to_vec(&relay).unwrap().into())).await.unwrap();

    // ------- 6. Bob decrypts -------
    let incoming = recv_relay(&mut b_ws).await;
    assert_eq!(incoming.from, "alice");
    assert_eq!(incoming.kind, PayloadKind::Initial);
    let incoming_initial: aqua_core::wire::InitialMessage =
        bincode::deserialize(&incoming.payload).unwrap();
    let spk = bob.spks.remove(&1).unwrap();
    let opk = incoming_initial.used_opk_id.and_then(|id| bob.opks.remove(&id));
    assert_eq!(incoming_initial.used_opk_id, Some(bob_used_opk_id));
    let (mut bob_sess, first_plain) =
        Session::accept_initial(&bob.identity, spk, opk, &incoming_initial).unwrap();
    assert_eq!(first_plain, b"hi bob, this is alice");

    // ------- 7. Ping-pong -------
    for i in 0..5 {
        let m = format!("bob->alice #{i}");
        let env = bob_sess.encrypt(m.as_bytes()).unwrap();
        b_ws.send(Message::Binary(serde_json::to_vec(&RelayMessage {
            from: String::new(),
            to: "alice".into(),
            payload: bincode::serialize(&env).unwrap(),
            kind: PayloadKind::Envelope,
        }).unwrap().into())).await.unwrap();
        let got = recv_relay(&mut a_ws).await;
        assert_eq!(got.from, "bob");
        let env: aqua_core::wire::Envelope = bincode::deserialize(&got.payload).unwrap();
        assert_eq!(alice_sess.decrypt(&env).unwrap(), m.as_bytes());

        let m = format!("alice->bob #{i}");
        let env = alice_sess.encrypt(m.as_bytes()).unwrap();
        a_ws.send(Message::Binary(serde_json::to_vec(&RelayMessage {
            from: String::new(),
            to: "bob".into(),
            payload: bincode::serialize(&env).unwrap(),
            kind: PayloadKind::Envelope,
        }).unwrap().into())).await.unwrap();
        let got = recv_relay(&mut b_ws).await;
        assert_eq!(got.from, "alice");
        let env: aqua_core::wire::Envelope = bincode::deserialize(&got.payload).unwrap();
        assert_eq!(bob_sess.decrypt(&env).unwrap(), m.as_bytes());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bundle_upload_without_token_is_refused() {
    let addr = spawn_server().await;
    let base = format!("http://{addr}");
    let http = reqwest::Client::new();
    let mut alice = Client::new("alice");
    let bundle = alice.fresh_bundle(1);
    let r = http
        .post(format!("{base}/v1/bundle/alice"))
        .json(&bundle)
        .send().await.unwrap();
    assert_eq!(r.status(), reqwest::StatusCode::UNAUTHORIZED);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn verify_rejects_wrong_signature() {
    let addr = spawn_server().await;
    let base = format!("http://{addr}");
    let http = reqwest::Client::new();
    let alice = IdentityKeyPair::generate();
    let attacker = IdentityKeyPair::generate();

    // Get a nonce as Alice.
    let chal: ChallengeResponse = http
        .post(format!("{base}/v1/auth/challenge"))
        .json(&ChallengeRequest {
            identity_dh_pub: URL_SAFE_NO_PAD.encode(alice.public().dh_pub.as_bytes()),
        })
        .send().await.unwrap()
        .json().await.unwrap();

    // Attacker signs with a different key but claims Alice's identity.
    let nonce_bytes = URL_SAFE_NO_PAD.decode(&chal.nonce).unwrap();
    let sig = attacker.sign.sign(&nonce_bytes);
    let r = http
        .post(format!("{base}/v1/auth/verify"))
        .json(&VerifyRequest {
            identity: alice.public(),
            nonce: chal.nonce,
            signature: URL_SAFE_NO_PAD.encode(sig),
        })
        .send().await.unwrap();
    assert!(r.status().is_client_error(), "must reject wrong signature");
}

async fn recv_relay(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> RelayMessage {
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("timed out waiting for relay message")
            .expect("stream ended")
            .expect("ws error");
        match msg {
            Message::Binary(bytes) => return serde_json::from_slice(&bytes).unwrap(),
            Message::Text(text) => return serde_json::from_str(&text).unwrap(),
            Message::Ping(_) | Message::Pong(_) => continue,
            other => panic!("unexpected ws frame: {other:?}"),
        }
    }
}

/// Public avatar / banner: owner uploads over HTTP, anyone fetches by
/// username, the version shows up on the profile card, strangers are
/// refused, extras round-trip.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn profile_media_is_public_and_owner_only() {
    let addr = spawn_server().await;
    let base = format!("http://{addr}");
    let http = reqwest::Client::new();

    let mut owner = Client::new("owner");
    let mut other = Client::new("other");
    owner.login(&http, &base).await;
    other.login(&http, &base).await;

    // Register "owner" via the email ticket flow (dev codes are on in tests).
    let code: serde_json::Value = http.post(format!("{base}/v1/auth/email/request"))
        .json(&serde_json::json!({ "email": "o@x.com" })).send().await.unwrap().json().await.unwrap();
    let code = code["dev_code"].as_str().unwrap().to_string();
    let t: serde_json::Value = http.post(format!("{base}/v1/auth/email/verify"))
        .json(&serde_json::json!({ "email": "o@x.com", "code": code })).send().await.unwrap().json().await.unwrap();
    let r = http.post(format!("{base}/v1/users/register")).bearer_auth(&owner.token)
        .json(&serde_json::json!({ "ticket": t["ticket"], "username": "owner", "display_name": "Owner", "password": "correct horse battery" }))
        .send().await.unwrap();
    let st = r.status();
    assert!(st.is_success(), "register: {} {}", st, r.text().await.unwrap_or_default());

    // Upload an avatar.
    let png = b"\x89PNG fake".to_vec();
    let r = http.post(format!("{base}/v1/users/owner/media/avatar")).bearer_auth(&owner.token)
        .header("content-type", "image/png").body(png.clone()).send().await.unwrap();
    assert!(r.status().is_success(), "upload: {}", r.status());
    let ver = r.json::<serde_json::Value>().await.unwrap()["ver"].as_i64().unwrap();
    assert!(ver > 0);

    // Anyone (no token) can fetch it; the card carries the version.
    let r = http.get(format!("{base}/v1/users/owner/media/avatar?v={ver}")).send().await.unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()["content-type"], "image/png");
    assert_eq!(r.bytes().await.unwrap().to_vec(), png);
    let card: serde_json::Value = http.get(format!("{base}/v1/users/owner/profile")).send().await.unwrap().json().await.unwrap();
    assert_eq!(card["avatar_ver"].as_i64().unwrap(), ver);
    assert_eq!(card["banner_ver"].as_i64().unwrap(), 0);

    // A stranger can't overwrite it, and bad kinds / types are refused.
    let r = http.post(format!("{base}/v1/users/owner/media/avatar")).bearer_auth(&other.token)
        .header("content-type", "image/png").body("x").send().await.unwrap();
    assert_eq!(r.status(), 403);
    let r = http.post(format!("{base}/v1/users/owner/media/banner")).bearer_auth(&owner.token)
        .header("content-type", "text/html").body("<b>").send().await.unwrap();
    assert_eq!(r.status(), 400);
    let r = http.get(format!("{base}/v1/users/owner/media/bogus")).send().await.unwrap();
    assert_eq!(r.status(), 404);

    // Favourite track (audio) shows up on the card; audio isn't an avatar.
    let r = http.post(format!("{base}/v1/users/owner/media/music")).bearer_auth(&owner.token)
        .header("content-type", "audio/mpeg").body(vec![0xFFu8; 4096]).send().await.unwrap();
    assert!(r.status().is_success(), "music: {}", r.status());
    let card: serde_json::Value = http.get(format!("{base}/v1/users/owner/profile")).send().await.unwrap().json().await.unwrap();
    assert!(card["music_ver"].as_i64().unwrap() > 0);
    let r = http.post(format!("{base}/v1/users/owner/media/avatar")).bearer_auth(&owner.token)
        .header("content-type", "audio/mpeg").body(vec![0u8; 10]).send().await.unwrap();
    assert_eq!(r.status(), 400);
    // Animated banners must be light: a 4 MB GIF is refused, a small video is fine.
    let r = http.post(format!("{base}/v1/users/owner/media/banner")).bearer_auth(&owner.token)
        .header("content-type", "image/gif").body(vec![0u8; 4 * 1024 * 1024]).send().await.unwrap();
    assert_eq!(r.status(), 400);
    let r = http.post(format!("{base}/v1/users/owner/media/banner")).bearer_auth(&owner.token)
        .header("content-type", "video/webm").body(vec![0u8; 200 * 1024]).send().await.unwrap();
    assert!(r.status().is_success(), "video banner: {}", r.status());

    // Extras round-trip through the profile endpoint.
    let r = http.post(format!("{base}/v1/users/owner/profile")).bearer_auth(&owner.token)
        .json(&serde_json::json!({ "display_name": "Owner", "about": "hi", "extra": { "status": "online", "banner_a": "#123456" } }))
        .send().await.unwrap();
    assert!(r.status().is_success());
    let card: serde_json::Value = http.get(format!("{base}/v1/users/owner/profile")).send().await.unwrap().json().await.unwrap();
    assert_eq!(card["extra"]["banner_a"], "#123456");

    // Delete.
    let r = http.delete(format!("{base}/v1/users/owner/media/avatar")).bearer_auth(&owner.token).send().await.unwrap();
    assert_eq!(r.status(), 204);
    let r = http.get(format!("{base}/v1/users/owner/media/avatar")).send().await.unwrap();
    assert_eq!(r.status(), 404);
}
