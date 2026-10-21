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

/// Bots: register → /me → long poll → reset token → delete. Also drives
/// the stdlib console bot (tools/aqua-console.py) when python3 exists.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bot_lifecycle_and_console_script() {
    let addr = spawn_server().await;
    let base = format!("http://{addr}");
    let http = reqwest::Client::new();
    let mut owner = Client::new("botowner");
    let mut other = Client::new("botother");
    owner.login(&http, &base).await;
    other.login(&http, &base).await;

    let reg = |name: &'static str| {
        let http = http.clone();
        let base = base.clone();
        let tok = owner.token.clone();
        async move {
            let v: serde_json::Value = http.post(format!("{base}/v1/bot/register")).bearer_auth(tok)
                .json(&serde_json::json!({ "username": name, "display_name": "Console Bot" }))
                .send().await.unwrap().json().await.unwrap();
            v["token"].as_str().unwrap().to_string()
        }
    };
    let token = reg("e2econsole").await;

    let me: serde_json::Value = http.get(format!("{base}/v1/bot/me?token={token}")).send().await.unwrap().json().await.unwrap();
    assert_eq!(me["username"], "e2econsole");
    assert_eq!(me["display_name"], "Console Bot");
    let list: serde_json::Value = http.get(format!("{base}/v1/bot/list")).bearer_auth(&owner.token).send().await.unwrap().json().await.unwrap();
    assert_eq!(list[0]["display_name"], "Console Bot");
    // The relay keeps only a hash: the list shows the last 4 characters.
    assert_eq!(list[0]["token"], "");
    assert_eq!(list[0]["hint"].as_str().unwrap(), &token[token.len() - 4..]);
    // The header form works too.
    let me2: serde_json::Value = http.get(format!("{base}/v1/bot/me")).header("Authorization", format!("Bot {token}"))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(me2["username"], "e2econsole");
    // Bots can't cold-message people who never wrote to them.
    let r = http.post(format!("{base}/v1/bot/send")).header("Authorization", format!("Bot {token}"))
        .json(&serde_json::json!({ "to": "botother", "text": "spam" })).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 403);
    let r = http.post(format!("{base}/v1/bot/send")).header("Authorization", format!("Bot {token}"))
        .json(&serde_json::json!({ "to": "e2econsole", "text": "x".repeat(5000) })).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 413);
    // Per-chat rate limit: a burst of 5, then 429.
    let mut codes = Vec::new();
    for i in 0..8 {
        let r = http.post(format!("{base}/v1/bot/send")).header("Authorization", format!("Bot {token}"))
            .json(&serde_json::json!({ "to": "e2econsole", "text": format!("n{i}") })).send().await.unwrap();
        codes.push(r.status().as_u16());
    }
    assert!(codes.contains(&429), "{codes:?}");
    // Drain those, and let the per-chat bucket refill for the checks below.
    let ups: serde_json::Value = http.get(format!("{base}/v1/bot/updates?token={token}&after=0")).send().await.unwrap().json().await.unwrap();
    let next = ups.as_array().unwrap().last().map(|u| u["id"].as_u64().unwrap() + 1).unwrap_or(0);
    let _ = http.get(format!("{base}/v1/bot/updates?token={token}&after={next}")).send().await.unwrap();
    tokio::time::sleep(Duration::from_secs(5)).await;

    // Long poll: empty after the timeout…
    let t0 = std::time::Instant::now();
    let ups: serde_json::Value = http.get(format!("{base}/v1/bot/updates?token={token}&after={next}&timeout=1")).send().await.unwrap().json().await.unwrap();
    assert_eq!(ups.as_array().unwrap().len(), 0);
    assert!(t0.elapsed() >= Duration::from_millis(900));
    // …and wakes up early when a message arrives (the bot writes to itself).
    let poll = {
        let http = http.clone();
        let url = format!("{base}/v1/bot/updates?token={token}&after={next}&timeout=20");
        tokio::spawn(async move { http.get(url).send().await.unwrap().json::<serde_json::Value>().await.unwrap() })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    let t1 = std::time::Instant::now();
    let r = http.post(format!("{base}/v1/bot/send"))
        .json(&serde_json::json!({ "token": token, "to": "e2econsole", "text": "hello" })).send().await.unwrap();
    assert!(r.status().is_success());
    let got = poll.await.unwrap();
    assert!(t1.elapsed() < Duration::from_secs(3));
    assert_eq!(got[0]["text"], "hello");

    // The console script, if python3 is around.
    if std::process::Command::new("python3").arg("--version").output().is_ok() {
        use std::io::Write;
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/aqua-console.py");
        let mut child = std::process::Command::new("python3")
            .args([script, "--server", &base, "--chat", "e2econsole"])
            .env("AQUA_BOT_TOKEN", &token)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn().unwrap();
        let mut stdin = child.stdin.take().unwrap();
        tokio::time::sleep(Duration::from_millis(1500)).await;
        writeln!(stdin, "from the console").unwrap();
        tokio::time::sleep(Duration::from_millis(2000)).await;
        writeln!(stdin, "/quit").unwrap();
        drop(stdin);
        let out = tokio::task::spawn_blocking(move || child.wait_with_output().unwrap()).await.unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains("Connected as @e2econsole"), "{text}");
        assert!(text.contains("hello"), "backlog not shown: {text}");
        assert!(text.contains("@e2econsole\u{1b}[0m: from the console"), "own message not echoed back: {text}");
    }

    // Reset: the old token dies.
    let token2 = reg("e2econsole").await;
    assert_ne!(token, token2);
    let r = http.get(format!("{base}/v1/bot/me?token={token}")).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 401);

    // Only the owner can delete it.
    let r = http.delete(format!("{base}/v1/bot/e2econsole")).bearer_auth(&other.token).send().await.unwrap();
    assert!(!r.status().is_success());
    let r = http.delete(format!("{base}/v1/bot/e2econsole")).bearer_auth(&owner.token).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 204);
    let r = http.get(format!("{base}/v1/bot/me?token={token2}")).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 401);
}

/// Backups: sign up with a hashed password, upload a sealed backup, then a
/// "new device" fetches it with an e-mail ticket + the same password,
/// opens it, and signs in with the restored identity.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn encrypted_backup_moves_an_account_to_a_new_device() {
    use aqua_client_core::backup;
    let addr = spawn_server().await;
    let base = format!("http://{addr}");
    let http = reqwest::Client::new();
    let email = "move@x.com";
    let password = "correct horse battery";
    let secret = backup::auth_secret(password, email);

    // Old device: register.
    let mut old = Client::new("mover");
    old.login(&http, &base).await;
    let code: serde_json::Value = http.post(format!("{base}/v1/auth/email/request")).json(&serde_json::json!({ "email": email })).send().await.unwrap().json().await.unwrap();
    let t: serde_json::Value = http.post(format!("{base}/v1/auth/email/verify")).json(&serde_json::json!({ "email": email, "code": code["dev_code"] })).send().await.unwrap().json().await.unwrap();
    let r = http.post(format!("{base}/v1/users/register")).bearer_auth(&old.token)
        .json(&serde_json::json!({ "ticket": t["ticket"], "username": "mover", "display_name": "M", "password": secret }))
        .send().await.unwrap();
    assert!(r.status().is_success(), "{}", r.text().await.unwrap());

    // Upload a sealed backup.
    let (keys, phrase) = backup::new_keys(password);
    let blob = backup::seal(&keys, b"vault dump + chats");
    let r = http.put(format!("{base}/v1/backup")).bearer_auth(&old.token).body(b"plain text".to_vec()).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 400, "only sealed backups are accepted");
    let r = http.put(format!("{base}/v1/backup")).bearer_auth(&old.token).body(blob.clone()).send().await.unwrap();
    assert!(r.status().is_success());
    let meta: serde_json::Value = http.get(format!("{base}/v1/backup")).bearer_auth(&old.token).send().await.unwrap().json().await.unwrap();
    assert_eq!(meta["exists"], true);

    // New device: ticket + password → backup.
    let code: serde_json::Value = http.post(format!("{base}/v1/auth/email/request")).json(&serde_json::json!({ "email": email })).send().await.unwrap().json().await.unwrap();
    let t: serde_json::Value = http.post(format!("{base}/v1/auth/email/verify")).json(&serde_json::json!({ "email": email, "code": code["dev_code"] })).send().await.unwrap().json().await.unwrap();
    let ticket = t["ticket"].as_str().unwrap();
    assert!(aqua_client_core::Transport::fetch_backup(&base, ticket, "wrong").await.is_err());
    let got = aqua_client_core::Transport::fetch_backup(&base, ticket, &secret).await.unwrap().unwrap();
    assert_eq!(got, blob);
    assert_eq!(backup::open_with_password(&got, password).unwrap().0, b"vault dump + chats");
    assert_eq!(backup::open_with_recovery(&got, &phrase).unwrap().0, b"vault dump + chats");
    // The relay only ever had the hash: it can't open the backup with it.
    assert!(backup::open_with_password(&got, &secret).is_err());

    // The ticket still works for the sign-in, with the restored identity.
    let u = aqua_client_core::Transport::login_account(&base, &old.identity, ticket, &secret).await.unwrap();
    assert_eq!(u, "mover");
}

/// Multi-device: one account signed in on two devices. Each device has its
/// own identity and bundle, gets its own copy of every message, and the
/// sender of a reply is visible per device.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_account_on_two_devices() {
    let addr = spawn_server().await;
    let base = format!("http://{addr}");
    let http = reqwest::Client::new();
    let email = "multi@x.com";
    let secret = "multi-secret-hash";

    async fn ticket(http: &reqwest::Client, base: &str, email: &str) -> String {
        let code: serde_json::Value = http.post(format!("{base}/v1/auth/email/request")).json(&serde_json::json!({ "email": email })).send().await.unwrap().json().await.unwrap();
        let t: serde_json::Value = http.post(format!("{base}/v1/auth/email/verify")).json(&serde_json::json!({ "email": email, "code": code["dev_code"] })).send().await.unwrap().json().await.unwrap();
        t["ticket"].as_str().unwrap().to_string()
    }
    async fn recv(ws: &mut tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>) -> RelayMessage {
        loop {
            let m = tokio::time::timeout(Duration::from_secs(5), ws.next()).await.expect("timed out").unwrap().unwrap();
            if let Message::Binary(b) = m {
                return serde_json::from_slice(&b).unwrap();
            }
        }
    }

    // Device A registers the account.
    let mut a = Client::new("multi-a");
    a.login(&http, &base).await;
    let t = ticket(&http, &base, email).await;
    let r = http.post(format!("{base}/v1/users/register")).bearer_auth(&a.token)
        .json(&serde_json::json!({ "ticket": t, "username": "multi", "display_name": "M", "password": secret }))
        .send().await.unwrap();
    assert!(r.status().is_success(), "{}", r.text().await.unwrap());

    // Device B signs in: it is added, A stays.
    let mut b = Client::new("multi-b");
    b.login(&http, &base).await;
    let t = ticket(&http, &base, email).await;
    let r = http.post(format!("{base}/v1/users/login")).bearer_auth(&b.token)
        .json(&serde_json::json!({ "ticket": t, "password": secret, "device_name": "Phone" }))
        .send().await.unwrap();
    assert!(r.status().is_success(), "{}", r.text().await.unwrap());

    let devs: serde_json::Value = http.get(format!("{base}/v1/devices/multi")).send().await.unwrap().json().await.unwrap();
    let devs = devs.as_array().unwrap();
    assert_eq!(devs.len(), 2);
    assert_eq!(devs[0]["addr"], "multi");
    assert_eq!(devs[0]["primary"], true);
    let b_addr = devs[1]["addr"].as_str().unwrap().to_string();
    assert!(b_addr.starts_with("multi~"));
    assert_eq!(devs[1]["name"], "Phone");

    // Both publish bundles under the same username; the relay files them per device.
    for c in [&mut a, &mut b] {
        let bundle = c.fresh_bundle(2);
        let r = http.post(format!("{base}/v1/bundle/multi")).bearer_auth(&c.token).json(&bundle).send().await.unwrap();
        assert!(r.status().is_success(), "{}", r.status());
    }
    let ba: PreKeyBundle = http.get(format!("{base}/v1/bundle/multi")).send().await.unwrap().json().await.unwrap();
    let bb: PreKeyBundle = http.get(format!("{base}/v1/bundle/{b_addr}")).send().await.unwrap().json().await.unwrap();
    assert_eq!(ba.identity.dh_pub.as_bytes(), a.identity.public().dh_pub.as_bytes());
    assert_eq!(bb.identity.dh_pub.as_bytes(), b.identity.public().dh_pub.as_bytes());

    // A stranger's key can't use the account.
    let mut eve = Client::new("eve");
    eve.login(&http, &base).await;
    assert!(tokio_tungstenite::connect_async(format!("ws://{addr}/v1/ws?user=multi&token={}", eve.token)).await.is_err());

    let (mut a_ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/v1/ws?user=multi&token={}", a.token)).await.unwrap();
    let (mut b_ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/v1/ws?user=multi&token={}", b.token)).await.unwrap();
    let mut peer = Client::new("mpeer");
    peer.login(&http, &base).await;
    let (mut p_ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/v1/ws?user=mpeer&token={}", peer.token)).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    // The peer encrypts once per device.
    for (to, body) in [("multi".to_string(), b"for A".to_vec()), (b_addr.clone(), b"for B".to_vec())] {
        let m = RelayMessage { from: String::new(), to, payload: body, kind: PayloadKind::Envelope };
        p_ws.send(Message::Binary(serde_json::to_vec(&m).unwrap().into())).await.unwrap();
    }
    let ga = recv(&mut a_ws).await;
    let gb = recv(&mut b_ws).await;
    assert_eq!((ga.from.as_str(), ga.payload.as_slice()), ("mpeer", &b"for A"[..]));
    assert_eq!((gb.from.as_str(), gb.payload.as_slice()), ("mpeer", &b"for B"[..]));

    // B answers: the peer sees which device wrote.
    let m = RelayMessage { from: String::new(), to: "mpeer".into(), payload: b"hi from B".to_vec(), kind: PayloadKind::Envelope };
    b_ws.send(Message::Binary(serde_json::to_vec(&m).unwrap().into())).await.unwrap();
    let gp = recv(&mut p_ws).await;
    assert_eq!(gp.from, b_addr);

    // B's view of the account; B signs A (the primary) out and takes over.
    let mine: serde_json::Value = http.get(format!("{base}/v1/me/devices")).bearer_auth(&b.token).send().await.unwrap().json().await.unwrap();
    assert_eq!(mine.as_array().unwrap().iter().filter(|d| d["current"] == true).count(), 1);
    let a_id = mine[0]["id"].as_str().unwrap();
    let r = http.delete(format!("{base}/v1/me/devices/{a_id}")).bearer_auth(&b.token).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 204);
    let devs: serde_json::Value = http.get(format!("{base}/v1/devices/multi")).send().await.unwrap().json().await.unwrap();
    let devs = devs.as_array().unwrap();
    assert_eq!(devs.len(), 1);
    assert_eq!(devs[0]["addr"], "multi");
    assert_eq!(devs[0]["id"].as_str().unwrap(), &b_addr["multi~".len()..]);
    // A's key is out.
    let r = http.post(format!("{base}/v1/bundle/multi")).bearer_auth(&a.token).json(&a.fresh_bundle(0)).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 403);
}

/// A bot written for Telegram, talking to this relay through the Telegram
/// Bot API layer: getMe, long polling, commands, an inline keyboard answered
/// by number → callback_query, and a webhook with a secret token.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn telegram_bot_api_compat() {
    let addr = spawn_server().await;
    let base = format!("http://{addr}");
    let http = reqwest::Client::new();
    let mut owner = Client::new("tgowner");
    owner.login(&http, &base).await;
    let reg: serde_json::Value = http.post(format!("{base}/v1/bot/register")).bearer_auth(&owner.token)
        .json(&serde_json::json!({ "username": "tgcompat_bot", "display_name": "Compat" }))
        .send().await.unwrap().json().await.unwrap();
    // The Telegram-shaped token ("<digits>:<secret>") works, like a real one.
    let tg_token = reg["telegram_token"].as_str().unwrap().to_string();
    let (digits, _) = tg_token.split_once(':').unwrap();
    assert!(digits.chars().all(|c| c.is_ascii_digit()));
    let api = format!("{base}/bot{tg_token}");

    let me: serde_json::Value = http.get(format!("{api}/getMe")).send().await.unwrap().json().await.unwrap();
    assert_eq!(me["ok"], true);
    assert_eq!(me["result"]["username"], "tgcompat_bot");
    assert_eq!(me["result"]["is_bot"], true);
    let bad: serde_json::Value = http.get(format!("{base}/bot123:nope/getMe")).send().await.unwrap().json().await.unwrap();
    assert_eq!((bad["ok"].clone(), bad["error_code"].clone()), (serde_json::json!(false), serde_json::json!(401)));

    // A user writes /start.
    let mut user = Client::new("tguser");
    user.login(&http, &base).await;
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/v1/ws?user=tguser&token={}", user.token)).await.unwrap();
    let say = |text: &str| RelayMessage { from: String::new(), to: "tgcompat_bot".into(), payload: text.as_bytes().to_vec(), kind: PayloadKind::Plaintext };
    ws.send(Message::Binary(serde_json::to_vec(&say("/start hello")).unwrap().into())).await.unwrap();

    let ups: serde_json::Value = http.post(format!("{api}/getUpdates")).json(&serde_json::json!({ "timeout": 5 }))
        .send().await.unwrap().json().await.unwrap();
    let u = &ups["result"][0];
    assert_eq!(u["message"]["text"], "/start hello");
    assert_eq!(u["message"]["entities"][0]["type"], "bot_command");
    assert_eq!(u["message"]["chat"]["type"], "private");
    assert_eq!(u["message"]["from"]["username"], "tguser");
    let chat_id = u["message"]["chat"]["id"].as_i64().unwrap();
    let offset = u["update_id"].as_i64().unwrap() + 1;

    // The bot answers with HTML and an inline keyboard (form-encoded, like most libraries).
    let markup = serde_json::json!({ "inline_keyboard": [[
        { "text": "Red", "callback_data": "c:red" }, { "text": "Blue", "callback_data": "c:blue" }
    ]]}).to_string();
    let sent: serde_json::Value = http.post(format!("{api}/sendMessage"))
        .form(&[("chat_id", chat_id.to_string()), ("text", "<b>Pick</b> a colour".into()), ("parse_mode", "HTML".into()), ("reply_markup", markup)])
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(sent["ok"], true, "{sent}");
    let kb_msg = sent["result"]["message_id"].as_i64().unwrap();

    let got = loop {
        let m = tokio::time::timeout(Duration::from_secs(5), ws.next()).await.unwrap().unwrap().unwrap();
        if let Message::Binary(b) = m { break serde_json::from_slice::<RelayMessage>(&b).unwrap(); }
    };
    let text = String::from_utf8(got.payload).unwrap();
    // First line: the keyboard for the app to draw; then the text; the
    // numbered list (for older apps) after an invisible separator.
    let (head, rest) = text.split_once('\n').unwrap();
    let kb: serde_json::Value = serde_json::from_str(head.strip_prefix("[kb:v1]").unwrap()).unwrap();
    assert_eq!(kb["kind"], "inline");
    assert_eq!(kb["m"], kb_msg);
    assert_eq!(kb["rows"][0][1]["cb"], "c:blue");
    assert!(rest.starts_with("**Pick** a colour\u{2063}"), "{rest}");
    assert!(rest.contains("1️⃣ Red") && rest.contains("2️⃣ Blue"), "{rest}");

    // The user answers "2" → the bot gets a callback_query for "Blue".
    ws.send(Message::Binary(serde_json::to_vec(&say("2")).unwrap().into())).await.unwrap();
    let ups: serde_json::Value = http.get(format!("{api}/getUpdates?offset={offset}&timeout=5")).send().await.unwrap().json().await.unwrap();
    let cq = &ups["result"][0]["callback_query"];
    assert_eq!(cq["data"], "c:blue", "{ups}");
    assert_eq!(cq["message"]["message_id"], kb_msg);
    assert_eq!(cq["from"]["id"], chat_id);
    let offset = ups["result"][0]["update_id"].as_i64().unwrap() + 1;

    // A button pressed in the app.
    ws.send(Message::Binary(serde_json::to_vec(&say(&format!("[cb:v1]{{\"m\":{kb_msg},\"d\":\"c:red\"}}"))).unwrap().into())).await.unwrap();
    let ups2: serde_json::Value = http.get(format!("{api}/getUpdates?offset={offset}&timeout=5")).send().await.unwrap().json().await.unwrap();
    assert_eq!(ups2["result"][0]["callback_query"]["data"], "c:red", "{ups2}");
    assert_eq!(ups2["result"][0]["callback_query"]["message"]["message_id"], kb_msg);
    let offset = ups2["result"][0]["update_id"].as_i64().unwrap() + 1;

    // answerCallbackQuery with text reaches the user.
    let r: serde_json::Value = http.post(format!("{api}/answerCallbackQuery"))
        .json(&serde_json::json!({ "callback_query_id": cq["id"], "text": "Blue it is" }))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(r["ok"], true);
    let got = loop {
        let m = tokio::time::timeout(Duration::from_secs(5), ws.next()).await.unwrap().unwrap().unwrap();
        if let Message::Binary(b) = m { break serde_json::from_slice::<RelayMessage>(&b).unwrap(); }
    };
    assert_eq!(String::from_utf8(got.payload).unwrap(), "ℹ️ Blue it is");

    // Unknown methods answer like Telegram does; strangers can't be cold-messaged.
    let r: serde_json::Value = http.get(format!("{api}/sendInvoice")).send().await.unwrap().json().await.unwrap();
    assert_eq!(r["error_code"], 404);
    let r: serde_json::Value = http.post(format!("{api}/sendMessage")).json(&serde_json::json!({ "chat_id": "@tgowner", "text": "hi" }))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(r["error_code"], 403);

    // Webhook: updates are POSTed with the secret header; getUpdates is refused meanwhile.
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(Option<String>, serde_json::Value)>();
    let hook = axum::Router::new().route("/hook", axum::routing::post(move |h: axum::http::HeaderMap, axum::Json(v): axum::Json<serde_json::Value>| {
        let tx = tx.clone();
        async move {
            let secret = h.get("x-telegram-bot-api-secret-token").and_then(|v| v.to_str().ok()).map(str::to_string);
            let _ = tx.send((secret, v));
            "ok"
        }
    }));
    let hl = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let hook_addr = hl.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(hl, hook).await.unwrap() });
    let _ = http.get(format!("{api}/getUpdates?offset={offset}")).send().await.unwrap();
    let r: serde_json::Value = http.post(format!("{api}/setWebhook"))
        .json(&serde_json::json!({ "url": format!("http://{hook_addr}/hook"), "secret_token": "s3cret" }))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(r["ok"], true, "{r}");
    let r: serde_json::Value = http.get(format!("{api}/getUpdates")).send().await.unwrap().json().await.unwrap();
    assert_eq!(r["error_code"], 409);
    ws.send(Message::Binary(serde_json::to_vec(&say("via webhook")).unwrap().into())).await.unwrap();
    let (secret, upd) = tokio::time::timeout(Duration::from_secs(5), rx.recv()).await.unwrap().unwrap();
    assert_eq!(secret.as_deref(), Some("s3cret"));
    assert_eq!(upd["message"]["text"], "via webhook");
    let info: serde_json::Value = http.get(format!("{api}/getWebhookInfo")).send().await.unwrap().json().await.unwrap();
    assert!(info["result"]["url"].as_str().unwrap().ends_with("/hook"));
    let r: serde_json::Value = http.post(format!("{api}/deleteWebhook")).send().await.unwrap().json().await.unwrap();
    assert_eq!(r["ok"], true);
    let _ = std::fs::remove_file("aqua-tg-webhooks.json");
}

/// Sign-up with a taken e-mail fails at once; a forgotten password is reset
/// with the e-mail plus the 24-word recovery key (when the account has one).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn signup_conflict_and_password_reset() {
    use aqua_client_core::backup;
    let addr = spawn_server().await;
    let base = format!("http://{addr}");
    let http = reqwest::Client::new();
    let email = "reset@x.com";
    let req = |purpose: &'static str, email: &'static str| {
        let http = http.clone();
        let base = base.clone();
        async move { http.post(format!("{base}/v1/auth/email/request")).json(&serde_json::json!({ "email": email, "purpose": purpose })).send().await.unwrap() }
    };
    async fn ticket_of(http: &reqwest::Client, base: &str, r: reqwest::Response, email: &str) -> String {
        let code: serde_json::Value = r.json().await.unwrap();
        let t: serde_json::Value = http.post(format!("{base}/v1/auth/email/verify")).json(&serde_json::json!({ "email": email, "code": code["dev_code"] })).send().await.unwrap().json().await.unwrap();
        t["ticket"].as_str().unwrap().to_string()
    }

    // No account yet: sign-in / reset are refused, sign-up goes on.
    assert_eq!(req("signin", email).await.status().as_u16(), 404);
    assert_eq!(req("reset", email).await.status().as_u16(), 404);
    let mut a = Client::new("resetter");
    a.login(&http, &base).await;
    let t = ticket_of(&http, &base, req("signup", email).await, email).await;
    let r = http.post(format!("{base}/v1/users/register")).bearer_auth(&a.token)
        .json(&serde_json::json!({ "ticket": t, "username": "resetter", "password": backup::auth_secret("old password", email) }))
        .send().await.unwrap();
    assert!(r.status().is_success());

    // Now the e-mail is taken.
    let r = req("signup", "Reset@X.com ").await;
    assert_eq!(r.status().as_u16(), 409);
    assert!(r.text().await.unwrap().contains("already exists"));
    assert!(req("signin", email).await.status().is_success());

    // Without a recovery key the e-mail alone resets the password.
    let t = ticket_of(&http, &base, req("reset", email).await, email).await;
    let s2 = backup::auth_secret("second password", email);
    assert_eq!(aqua_client_core::Transport::reset_password(&base, &t, "", &s2).await.unwrap(), "resetter");
    let b = IdentityKeyPair::generate();
    assert_eq!(aqua_client_core::Transport::login_account(&base, &b, &t, &s2).await.unwrap(), "resetter");

    // Register a recovery key: from now on it is required.
    let phrase = backup::new_phrase();
    let check = backup::recovery_check(&phrase).unwrap();
    let r = http.put(format!("{base}/v1/me/recovery")).bearer_auth(&a.token).json(&serde_json::json!({ "check": check })).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 204);
    let st: serde_json::Value = http.get(format!("{base}/v1/me/recovery")).bearer_auth(&a.token).send().await.unwrap().json().await.unwrap();
    assert_eq!(st["set"], true);

    let t = ticket_of(&http, &base, req("reset", email).await, email).await;
    let s3 = backup::auth_secret("third password", email);
    let e = aqua_client_core::Transport::reset_password(&base, &t, "", &s3).await.unwrap_err();
    assert!(e.to_string().contains("recovery key"), "{e}");
    let wrong = backup::recovery_check(&backup::new_phrase()).unwrap();
    assert!(aqua_client_core::Transport::reset_password(&base, &t, &wrong, &s3).await.is_err());
    assert_eq!(aqua_client_core::Transport::reset_password(&base, &t, &check, &s3).await.unwrap(), "resetter");
    // The old password no longer works; the new one does.
    let c = IdentityKeyPair::generate();
    let t = ticket_of(&http, &base, req("signin", email).await, email).await;
    assert!(aqua_client_core::Transport::login_account(&base, &c, &t, &s2).await.is_err());
    assert_eq!(aqua_client_core::Transport::login_account(&base, &c, &t, &s3).await.unwrap(), "resetter");
}


/// Mini apps: initData is signed with the bot token exactly like Telegram's
/// (a bot can check it with the usual library code), sendData reaches the
/// bot as web_app_data, answerWebAppQuery writes to the user, and the menu
/// button is published.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn telegram_mini_apps() {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    let addr = spawn_server().await;
    let base = format!("http://{addr}");
    let http = reqwest::Client::new();
    // A registered user.
    let mut user = Client::new("maker");
    user.login(&http, &base).await;
    let code: serde_json::Value = http.post(format!("{base}/v1/auth/email/request")).json(&serde_json::json!({ "email": "mk@x.com" })).send().await.unwrap().json().await.unwrap();
    let t: serde_json::Value = http.post(format!("{base}/v1/auth/email/verify")).json(&serde_json::json!({ "email": "mk@x.com", "code": code["dev_code"] })).send().await.unwrap().json().await.unwrap();
    let r = http.post(format!("{base}/v1/users/register")).bearer_auth(&user.token)
        .json(&serde_json::json!({ "ticket": t["ticket"], "username": "maker", "password": "pw-secret-hash" })).send().await.unwrap();
    assert!(r.status().is_success());
    let reg: serde_json::Value = http.post(format!("{base}/v1/bot/register")).bearer_auth(&user.token)
        .json(&serde_json::json!({ "username": "shop_bot", "display_name": "Shop" }))
        .send().await.unwrap().json().await.unwrap();
    let tg_token = reg["telegram_token"].as_str().unwrap().to_string();
    let api = format!("{base}/bot{tg_token}");
    // The bot sets a menu button (and so the relay learns how it signs).
    let r: serde_json::Value = http.post(format!("{api}/setChatMenuButton"))
        .json(&serde_json::json!({ "menu_button": { "type": "web_app", "text": "Shop", "web_app": { "url": "https://shop.example/app" } } }))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(r["ok"], true);
    let menu: serde_json::Value = http.get(format!("{base}/v1/webapp/menu/shop_bot")).send().await.unwrap().json().await.unwrap();
    assert_eq!(menu["web_app"]["url"], "https://shop.example/app");

    // The user opens the app.
    let init: serde_json::Value = http.post(format!("{base}/v1/webapp/init")).bearer_auth(&user.token)
        .json(&serde_json::json!({ "bot": "shop_bot", "start_param": "promo" })).send().await.unwrap().json().await.unwrap();
    assert_eq!(init["signed"], true, "{init}");
    let data = init["init_data"].as_str().unwrap();
    // Validate it the way Telegram's docs (and every bot library) do.
    let mut pairs: Vec<(String, String)> = form_urlencoded::parse(data.as_bytes()).into_owned().collect();
    let hash = pairs.iter().find(|(k, _)| k == "hash").unwrap().1.clone();
    pairs.retain(|(k, _)| k != "hash");
    pairs.sort();
    let check = pairs.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("\n");
    let mut sk = <Hmac<Sha256> as Mac>::new_from_slice(b"WebAppData").unwrap();
    sk.update(tg_token.as_bytes());
    let secret = sk.finalize().into_bytes();
    let mut h = <Hmac<Sha256> as Mac>::new_from_slice(&secret).unwrap();
    h.update(check.as_bytes());
    assert_eq!(hex::encode(h.finalize().into_bytes()), hash);
    let user_json: serde_json::Value = serde_json::from_str(&pairs.iter().find(|(k, _)| k == "user").unwrap().1).unwrap();
    assert_eq!(user_json["username"], "maker");
    assert_eq!(pairs.iter().find(|(k, _)| k == "start_param").unwrap().1, "promo");
    let query_id = pairs.iter().find(|(k, _)| k == "query_id").unwrap().1.clone();

    // sendData → web_app_data.
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/v1/ws?user=maker&token={}", user.token)).await.unwrap();
    let say = |text: &str| RelayMessage { from: String::new(), to: "shop_bot".into(), payload: text.as_bytes().to_vec(), kind: PayloadKind::Plaintext };
    ws.send(Message::Binary(serde_json::to_vec(&say(r#"[wad:v1]{"t":"Order","d":"{\"item\":3}"}"#)).unwrap().into())).await.unwrap();
    let ups: serde_json::Value = http.post(format!("{api}/getUpdates")).json(&serde_json::json!({ "timeout": 5 })).send().await.unwrap().json().await.unwrap();
    let wad = &ups["result"][0]["message"]["web_app_data"];
    assert_eq!(wad["data"], r#"{"item":3}"#, "{ups}");
    assert_eq!(wad["button_text"], "Order");

    // answerWebAppQuery writes to the user.
    let r: serde_json::Value = http.post(format!("{api}/answerWebAppQuery"))
        .json(&serde_json::json!({ "web_app_query_id": query_id, "result": { "type": "article", "id": "1", "title": "x",
            "input_message_content": { "message_text": "Order <b>#3</b> placed", "parse_mode": "HTML" } } }))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(r["ok"], true, "{r}");
    let got = loop {
        let m = tokio::time::timeout(Duration::from_secs(5), ws.next()).await.unwrap().unwrap().unwrap();
        if let Message::Binary(b) = m { break serde_json::from_slice::<RelayMessage>(&b).unwrap(); }
    };
    assert_eq!(String::from_utf8(got.payload).unwrap(), "Order **#3** placed");
}
