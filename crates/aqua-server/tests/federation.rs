//! Two federated relays in one process: accounts replicate, envelopes are
//! routed to whichever relay the recipient is on, mail waiting on one relay
//! is collected by the other, and bundles / files are fetched across.

use std::time::Duration;

use aqua_core::keys::{IdentityKeyPair, OneTimePreKey, PreKeyBundle, SignedPreKey};
use aqua_server::{
    app,
    auth::{ChallengeRequest, ChallengeResponse, VerifyRequest, VerifyResponse},
    fed::{self, FedConfig},
    AppState, BundleUpload, PayloadKind, RelayMessage,
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
        .json(&VerifyRequest { identity: id.public(), nonce: chal.nonce, signature: URL_SAFE_NO_PAD.encode(sig) })
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
    tokio_tungstenite::connect_async(format!("ws://{addr}/v1/ws?user={user}&token={tok}")).await.unwrap().0
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
    Message::Binary(serde_json::to_vec(&RelayMessage { from: from.into(), to: to.into(), payload: body.to_vec(), kind: PayloadKind::Envelope }).unwrap().into())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_relays_act_as_one() {
    let la = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let lb = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (a, b) = (la.local_addr().unwrap().to_string(), lb.local_addr().unwrap().to_string());
    let (ua, ub) = (format!("http://{a}"), format!("http://{b}"));
    let tmp = tempfile::tempdir().unwrap();
    let cfg = |me: &str, peer: &str| FedConfig { secret: SECRET.into(), peers: vec![peer.into()], public_url: me.into() };
    let sa = AppState::isolated(tmp.path().join("a"), Some(cfg(&ua, &ub)));
    let sb = AppState::isolated(tmp.path().join("b"), Some(cfg(&ub, &ua)));
    { let s = sa.clone(); tokio::spawn(async move { axum::serve(la, app(s)).await.unwrap() }); }
    { let s = sb.clone(); tokio::spawn(async move { axum::serve(lb, app(s)).await.unwrap() }); }
    tokio::time::sleep(Duration::from_millis(100)).await;
    let http = reqwest::Client::new();

    // Unsigned federation calls are refused.
    let r = http.post(format!("{ua}/v1/fed/deliver")).body("{}").send().await.unwrap();
    assert_eq!(r.status().as_u16(), 401);
    let peers: serde_json::Value = http.get(format!("{ua}/v1/fed/peers")).send().await.unwrap().json().await.unwrap();
    assert_eq!(peers["servers"][0], ub.as_str());

    // alice registers on A, bob and carol on B.
    let (ida, idb, idc) = (IdentityKeyPair::generate(), IdentityKeyPair::generate(), IdentityKeyPair::generate());
    register(&http, &ua, &ida, "alice").await;
    register(&http, &ub, &idb, "bob").await;
    register(&http, &ub, &idc, "carol").await;

    // Replication: each relay now knows the others' accounts.
    fed::sync_once(&sa, &mut Default::default()).await;
    fed::sync_once(&sb, &mut Default::default()).await;
    assert_eq!(sa.directory.identity_for("bob"), Some(*idb.public().dh_pub.as_bytes()));
    assert_eq!(sb.directory.identity_for("alice"), Some(*ida.public().dh_pub.as_bytes()));

    // bob (on B) uploads his bundle; alice fetches it from A.
    let tb = token(&http, &ub, &idb).await;
    let spk = SignedPreKey::generate(1, &idb);
    let up = BundleUpload { identity: idb.public(), signed_prekey: spk.public(), one_time_prekeys: vec![OneTimePreKey::generate(1).public()] };
    assert!(http.post(format!("{ub}/v1/bundle/bob")).bearer_auth(&tb).json(&up).send().await.unwrap().status().is_success());
    let bundle: PreKeyBundle = http.get(format!("{ua}/v1/bundle/bob")).send().await.unwrap().json().await.unwrap();
    assert_eq!(bundle.identity, idb.public());

    // Live routing: alice on A → bob on B.
    let ta = token(&http, &ua, &ida).await;
    let mut wa = ws(&a, "alice", &ta).await;
    let mut wb = ws(&b, "bob", &tb).await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    wa.send(msg("alice", "bob", b"hi from A")).await.unwrap();
    let got = recv(&mut wb).await;
    assert_eq!((got.from.as_str(), got.payload.as_slice()), ("alice", &b"hi from A"[..]));
    wb.send(msg("bob", "alice", b"hi from B")).await.unwrap();
    assert_eq!(recv(&mut wa).await.payload, b"hi from B");

    // Offline: carol isn't connected anywhere; alice writes via A, carol
    // later connects to B and gets it from A's queue.
    wa.send(msg("alice", "carol", b"while you were away")).await.unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    let tc = token(&http, &ub, &idc).await;
    let mut wc = ws(&b, "carol", &tc).await;
    assert_eq!(recv(&mut wc).await.payload, b"while you were away");

    // Files: uploaded to A, downloaded from B.
    let blob = b"ciphertext bytes".to_vec();
    let cid = hex::encode(<sha2::Sha256 as sha2::Digest>::digest(&blob));
    assert!(http.post(format!("{ua}/v1/blob/{cid}")).bearer_auth(&ta).body(blob.clone()).send().await.unwrap().status().is_success());
    let got = http.get(format!("{ub}/v1/blob/{cid}")).send().await.unwrap();
    assert!(got.status().is_success());
    assert_eq!(got.bytes().await.unwrap().to_vec(), blob);

    // Backups replicate too.
    assert!(http.put(format!("{ua}/v1/backup")).bearer_auth(&ta).body(b"AQBK1 sealed".to_vec()).send().await.unwrap().status().is_success());
    fed::sync_once(&sb, &mut Default::default()).await;
    assert_eq!(sb.directory.get_backup("alice").unwrap(), b"AQBK1 sealed");
}
