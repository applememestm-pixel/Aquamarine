//! A real mixed network: relays of different builds running as their own
//! processes (not in-process copies). Not run by default — start the relays
//! first (`tools/fed-mixed.sh` does it with this build and two released
//! binaries), then:
//!
//! ```text
//! AQUA_MIXED="NEW=http://127.0.0.1:17771,KEYED=http://127.0.0.1:17772,OLD=http://127.0.0.1:17773" \
//! AQUA_MIXED_ADMIN=<admin token of NEW> AQUA_MIXED_SECRET=<network secret> cargo test -p aqua-server --test mixed_binaries -- --ignored --nocapture
//! ```
//!
//! NEW = this build, KEYED = a release with node keys but not required
//! (1.21.0-23), OLD = a release without node keys (1.21.0-22). Checked:
//! accounts and messages go every way during the transition; once NEW
//! requires keys, OLD is cut off and KEYED keeps working.

use std::time::{Duration, Instant};

use aqua_core::keys::IdentityKeyPair;
use aqua_server::{auth::{ChallengeRequest, ChallengeResponse, VerifyRequest, VerifyResponse}, PayloadKind, RelayMessage};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

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
    assert!(r.status().is_success(), "register {user} at {base}: {}", r.text().await.unwrap());
    tok
}

/// Does `base` know `user` (its key, from any relay of the network)?
async fn knows(http: &reqwest::Client, base: &str, user: &str) -> bool {
    let r = http.get(format!("{base}/v1/users/search?q={user}")).send().await;
    let Ok(r) = r else { return false };
    let v: serde_json::Value = r.json().await.unwrap_or_default();
    v.as_array().is_some_and(|a| a.iter().any(|x| x["username"] == user))
}

async fn wait_knows(http: &reqwest::Client, base: &str, user: &str, secs: u64) -> bool {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        if knows(http, base, user).await { return true; }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    false
}

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn ws(base: &str, user: &str, tok: &str) -> Ws {
    let addr = base.trim_start_matches("http://");
    tokio_tungstenite::connect_async(format!("ws://{addr}/v1/ws?user={user}&token={tok}&v=1.21.0-23")).await.unwrap().0
}

async fn recv(ws: &mut Ws, secs: u64) -> Option<RelayMessage> {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        let left = end.saturating_duration_since(Instant::now());
        let m = tokio::time::timeout(left, ws.next()).await.ok()??.ok()?;
        match m {
            Message::Binary(b) => return serde_json::from_slice(&b).ok(),
            Message::Text(t) => return serde_json::from_str(&t).ok(),
            _ => continue,
        }
    }
}

fn msg(from: &str, to: &str, body: &[u8]) -> Message {
    Message::Binary(serde_json::to_vec(&RelayMessage { from: from.into(), to: to.into(), payload: body.to_vec(), kind: PayloadKind::Envelope, ts: None, id: None }).unwrap().into())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs relays running as separate processes (tools/fed-mixed.sh)"]
async fn mixed_binaries_transition_then_enforced() {
    let urls: std::collections::HashMap<String, String> = std::env::var("AQUA_MIXED").expect("AQUA_MIXED")
        .split(',').filter_map(|kv| kv.split_once('=')).map(|(k, v)| (k.trim().to_string(), v.trim().trim_end_matches('/').to_string())).collect();
    let (n, k, o) = (&urls["NEW"], &urls["KEYED"], &urls["OLD"]);
    let admin = std::env::var("AQUA_MIXED_ADMIN").expect("AQUA_MIXED_ADMIN");
    let http = reqwest::Client::builder().user_agent("aqua-client-core/1.21.0-23").timeout(Duration::from_secs(20)).build().unwrap();
    for u in [n, k, o] {
        let v: serde_json::Value = http.get(format!("{u}/v1/stats")).send().await.unwrap().json().await.unwrap();
        println!("{u}: version {}", v["version"]);
    }
    let tag = format!("{:x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() % 0xfffff);
    let name = |s: &str| format!("{s}{tag}");
    let (ia, ib, ic) = (IdentityKeyPair::generate(), IdentityKeyPair::generate(), IdentityKeyPair::generate());
    let ta = register(&http, n, &ia, &name("anew")).await;
    let _tb = register(&http, k, &ib, &name("bkeyed")).await;
    let tc = register(&http, o, &ic, &name("cold")).await;

    // Transition: every account reaches every relay.
    for (base, user) in [(n, name("bkeyed")), (n, name("cold")), (k, name("anew")), (k, name("cold")), (o, name("anew")), (o, name("bkeyed"))] {
        assert!(wait_knows(&http, base, &user, 120).await, "{base} should know {user} during the transition");
        println!("ok: {base} knows {user}");
    }
    // A message from NEW to someone on OLD.
    let mut wa = ws(n, &name("anew"), &ta).await;
    let mut wc = ws(o, &name("cold"), &tc).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    wa.send(msg(&name("anew"), &name("cold"), b"new to old")).await.unwrap();
    let got = recv(&mut wc, 40).await.expect("OLD got NEW's message");
    assert_eq!(got.payload, b"new to old");
    wc.send(msg(&name("cold"), &name("anew"), b"old to new")).await.unwrap();
    assert_eq!(recv(&mut wa, 40).await.expect("NEW got OLD's message").payload, b"old to new");
    println!("ok: messages NEW <-> OLD in the transition");

    // NEW's view: KEYED pinned, OLD without a key.
    let fed: serde_json::Value = http.get(format!("{n}/v1/admin/federation")).bearer_auth(&admin).send().await.unwrap().json().await.unwrap();
    println!("NEW auth: {}", fed["auth"]);
    assert_eq!(fed["auth"]["mode"], "transition");
    assert!(fed["pins"].as_array().unwrap().iter().any(|p| p["url"] == k.as_str()), "NEW pinned KEYED: {}", fed["pins"]);
    assert!(fed["auth"]["unpinned"].as_array().unwrap().iter().any(|u| u == o.as_str()), "OLD has no key");

    // Keys required now on NEW.
    let r: serde_json::Value = http.post(format!("{n}/v1/admin/fed/auth")).bearer_auth(&admin).json(&serde_json::json!({ "action": "enforce" })).send().await.unwrap().json().await.unwrap();
    assert_eq!(r["mode"], "enforced");
    let (id, ie) = (IdentityKeyPair::generate(), IdentityKeyPair::generate());
    register(&http, o, &id, &name("dold2")).await;
    register(&http, k, &ie, &name("ekeyed2")).await;
    assert!(wait_knows(&http, n, &name("ekeyed2"), 120).await, "KEYED still syncs to NEW");
    println!("ok: KEYED -> NEW after enforcement");
    // OLD signs with the shared secret only: NEW refuses that now.
    let secret = std::env::var("AQUA_MIXED_SECRET").expect("AQUA_MIXED_SECRET");
    let me = aqua_server::fed::Fed::new(aqua_server::fed::FedConfig { secret, peers: vec![n.clone()], public_url: o.clone(), mode: "network".into(), ephemeral: true, ..Default::default() }).unwrap();
    let path = "/v1/fed/users?since=0";
    let st = http.get(format!("{n}{path}")).header("x-aqua-fed", me.sign("GET", path, b"")).header("user-agent", "aqua-server/1.21.0-22").send().await.unwrap().status().as_u16();
    assert_eq!(st, 401, "a secret-only request to NEW once keys are required");
    println!("ok: OLD's (secret-only) requests refused by NEW");
    // OLD's new account may still reach NEW through KEYED: 1.21.0-23 takes the
    // secret, copies it, and NEW reads KEYED's rows. That's KEYED's choice;
    // printed, not asserted.
    let via_keyed = wait_knows(&http, n, &name("dold2"), 75).await;
    println!("observed: OLD's new account reached NEW through KEYED: {via_keyed}");
    let fed: serde_json::Value = http.get(format!("{n}/v1/admin/federation")).bearer_auth(&admin).send().await.unwrap().json().await.unwrap();
    assert_eq!(fed["auth"]["mode"], "enforced");
    println!("NEW after: {}", fed["auth"]);
}
