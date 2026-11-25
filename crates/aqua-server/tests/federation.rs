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
/// (1.2.0-1): turn it on, as the user would in the settings.
async fn turn_on_2fa(base: &str, id: &IdentityKeyPair) {
    let setup = aqua_client_core::Transport::me_call_at(base, id, "POST", "2fa/setup", Some(serde_json::json!({}))).await.unwrap();
    let secret = aqua_server::totp::unbase32(setup["secret"].as_str().unwrap()).unwrap();
    let code = format!("{:06}", aqua_server::totp::code_at(&secret, aqua_server::totp::now_step()));
    aqua_client_core::Transport::me_call_at(base, id, "POST", "2fa/enable", Some(serde_json::json!({ "code": code }))).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_relays_act_as_one() {
    let la = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let lb = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (a, b) = (la.local_addr().unwrap().to_string(), lb.local_addr().unwrap().to_string());
    let (ua, ub) = (format!("http://{a}"), format!("http://{b}"));
    let tmp = tempfile::tempdir().unwrap();
    let cfg = |me: &str, peer: &str| FedConfig { secret: SECRET.into(), peers: vec![peer.into()], public_url: me.into(), ..Default::default() };
    let sa = AppState::isolated(tmp.path().join("a"), Some(cfg(&ua, &ub)));
    let sb = AppState::isolated(tmp.path().join("b"), Some(cfg(&ub, &ua)));
    { let s = sa.clone(); tokio::spawn(async move { axum::serve(la, app(s)).await.unwrap() }); }
    { let s = sb.clone(); tokio::spawn(async move { axum::serve(lb, app(s)).await.unwrap() }); }
    tokio::time::sleep(Duration::from_millis(100)).await;
    let http = reqwest::Client::builder().user_agent(concat!("aqua-client-core/", env!("CARGO_PKG_VERSION"))).build().unwrap();

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
    let ta = token(&http, &ua, &ida).await;
    // AQUA-SEC-007: without signing in, A doesn't look in the network for
    // a bundle (that would use up one of bob's one-time prekeys on B).
    assert_eq!(http.get(format!("{ua}/v1/bundle/bob")).send().await.unwrap().status().as_u16(), 404);
    let bundle: PreKeyBundle = http.get(format!("{ua}/v1/bundle/bob")).bearer_auth(&ta).send().await.unwrap().json().await.unwrap();
    assert_eq!(bundle.identity, idb.public());

    // Live routing: alice on A → bob on B.
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

    // A push to B that didn't get through (a timeout, a hiccup): the
    // envelope waits on A while bob is connected to B. A asks B to pull,
    // and bob gets it now — not at his next reconnect.
    let stuck = serde_json::to_vec(&RelayMessage { from: "alice".into(), to: "bob".into(), payload: b"pushed late".to_vec(), kind: PayloadKind::Envelope, ts: None, id: None }).unwrap();
    sa.directory.inbox_push("bob", &stuck).unwrap();
    fed::queued_for_absent(&sa);
    assert_eq!(recv(&mut wb).await.payload, b"pushed late");
    assert!(sa.directory.inbox_take("bob").is_empty(), "taken off A's queue");

    // Files: uploaded to A, downloaded from B.
    let blob = b"ciphertext bytes".to_vec();
    let cid = hex::encode(<sha2::Sha256 as sha2::Digest>::digest(&blob));
    assert!(http.post(format!("{ua}/v1/blob/{cid}")).bearer_auth(&ta).body(blob.clone()).send().await.unwrap().status().is_success());
    let got = http.get(format!("{ub}/v1/blob/{cid}")).send().await.unwrap();
    assert!(got.status().is_success());
    assert_eq!(got.bytes().await.unwrap().to_vec(), blob);

    // Backups replicate too.
    turn_on_2fa(&ua, &ida).await;
    assert!(http.put(format!("{ua}/v1/backup")).bearer_auth(&ta).body(b"AQBK1 sealed".to_vec()).send().await.unwrap().status().is_success());
    fed::sync_once(&sb, &mut Default::default()).await;
    assert_eq!(sb.directory.get_backup("alice").unwrap(), b"AQBK1 sealed");
}

/// Changes travel on their own: a relay that changed something pokes its
/// peers, which pull at once (no manual sync, well under the 30 s timer).
/// Profile media is copied along with the account; prekeys survive a
/// restart of the relay process.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn changes_are_pushed_to_peers() {
    let la = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let lb = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (a, b) = (la.local_addr().unwrap().to_string(), lb.local_addr().unwrap().to_string());
    let (ua, ub) = (format!("http://{a}"), format!("http://{b}"));
    let tmp = tempfile::tempdir().unwrap();
    let cfg = |me: &str, peer: &str| FedConfig { secret: SECRET.into(), peers: vec![peer.into()], public_url: me.into(), ..Default::default() };
    let sa = AppState::isolated(tmp.path().join("a"), Some(cfg(&ua, &ub)));
    let sb = AppState::isolated(tmp.path().join("b"), Some(cfg(&ub, &ua)));
    { let s = sa.clone(); tokio::spawn(async move { axum::serve(la, app(s)).await.unwrap() }); }
    { let s = sb.clone(); tokio::spawn(async move { axum::serve(lb, app(s)).await.unwrap() }); }
    fed::spawn_sync(sa.clone());
    fed::spawn_sync(sb.clone());
    tokio::time::sleep(Duration::from_millis(300)).await;
    let http = reqwest::Client::builder().user_agent(concat!("aqua-client-core/", env!("CARGO_PKG_VERSION"))).build().unwrap();

    // Unsigned pokes are refused.
    assert_eq!(http.post(format!("{ua}/v1/fed/poke")).body("{}").send().await.unwrap().status().as_u16(), 401);

    let ida = IdentityKeyPair::generate();
    let ta = register(&http, &ua, &ida, "dora").await;
    let until = |f: Box<dyn Fn() -> bool + Send>| async move {
        for _ in 0..80 {
            if f() { return true; }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        false
    };
    let sb2 = sb.clone();
    assert!(until(Box::new(move || sb2.directory.identity_for("dora").is_some())).await, "account reached B by itself");

    // A profile picture set on A is stored on B too (not just proxied).
    let png: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0x0D, b'I', b'H', b'D', b'R', 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 0x1F, 0x15, 0xC4, 0x89];
    let r = http.post(format!("{ua}/v1/users/dora/media/avatar")).bearer_auth(&ta).header("content-type", "image/png").body(png.to_vec()).send().await.unwrap();
    assert!(r.status().is_success(), "avatar upload: {}", r.status());
    let sb3 = sb.clone();
    assert!(until(Box::new(move || sb3.directory.get_media("dora", aqua_server::directory::MediaKind::Avatar).is_some())).await, "avatar copied to B");

    // Backups too.
    turn_on_2fa(&ua, &ida).await;
    assert!(http.put(format!("{ua}/v1/backup")).bearer_auth(&ta).body(b"AQBK1 v2".to_vec()).send().await.unwrap().status().is_success());
    let sb4 = sb.clone();
    assert!(until(Box::new(move || sb4.directory.get_backup("dora").as_deref() == Some(&b"AQBK1 v2"[..]))).await, "backup copied to B");

    // Prekeys are on disk: forget the in-memory state ("restart") and the
    // bundle is still served.
    let spk = SignedPreKey::generate(1, &ida);
    let up = BundleUpload { identity: ida.public(), signed_prekey: spk.public(), one_time_prekeys: vec![OneTimePreKey::generate(7).public(), OneTimePreKey::generate(8).public()] };
    assert!(http.post(format!("{ua}/v1/bundle/dora")).bearer_auth(&ta).json(&up).send().await.unwrap().status().is_success());
    sa.users.clear();
    // Not signed in: the bundle, but no one-time prekey (AQUA-SEC-007).
    let anon: PreKeyBundle = http.get(format!("{ua}/v1/bundle/dora")).send().await.unwrap().json().await.unwrap();
    assert_eq!(anon.identity, ida.public());
    assert!(anon.one_time_prekey.is_none(), "an anonymous fetch must not use up a one-time prekey");
    sa.users.clear();
    let bundle: PreKeyBundle = http.get(format!("{ua}/v1/bundle/dora")).bearer_auth(&ta).send().await.unwrap().json().await.unwrap();
    assert_eq!(bundle.identity, ida.public());
    assert!(bundle.one_time_prekey.is_some());
    // The used one-time key is gone after another "restart".
    sa.users.clear();
    let b2: PreKeyBundle = http.get(format!("{ua}/v1/bundle/dora")).bearer_auth(&ta).send().await.unwrap().json().await.unwrap();
    assert!(b2.one_time_prekey.is_some(), "the second of two one-time prekeys");
    assert_ne!(serde_json::to_string(&b2.one_time_prekey).unwrap(), serde_json::to_string(&bundle.one_time_prekey).unwrap());
}

/// Relays of one network find each other: C knows only B; after hellos, B
/// knows C and A learns C from B. An invitation round-trips; a relay with
/// another secret isn't "the same network".
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn relays_find_each_other_by_hello() {
    let ls: Vec<TcpListener> = futures_util::future::join_all((0..4).map(|_| TcpListener::bind("127.0.0.1:0"))).await.into_iter().map(|l| l.unwrap()).collect();
    let urls: Vec<String> = ls.iter().map(|l| format!("http://{}", l.local_addr().unwrap())).collect();
    let tmp = tempfile::tempdir().unwrap();
    let cfg = |me: &str, peers: &[&String], secret: &str| FedConfig {
        secret: secret.into(), peers: peers.iter().map(|p| p.to_string()).collect(), public_url: me.into(), mode: "network".into(), ..Default::default()
    };
    let sa = AppState::isolated(tmp.path().join("a"), Some(cfg(&urls[0], &[&urls[1]], SECRET)));
    let sb = AppState::isolated(tmp.path().join("b"), Some(cfg(&urls[1], &[&urls[0]], SECRET)));
    let sc = AppState::isolated(tmp.path().join("c"), Some(cfg(&urls[2], &[&urls[1]], SECRET)));
    let sx = AppState::isolated(tmp.path().join("x"), Some(cfg(&urls[3], &[], "another-network-secret-0000")));
    for (l, s) in ls.into_iter().zip([sa.clone(), sb.clone(), sc.clone(), sx.clone()]) {
        tokio::spawn(async move { axum::serve(l, app(s)).await.unwrap() });
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    let peers = |s: &AppState| fed::current(s).unwrap().cfg.peers.clone();

    // C says hello to B: B adds C (after checking C signs with the secret).
    fed::current(&sc).unwrap().hello_all().await;
    for _ in 0..50 { if peers(&sb).contains(&urls[2]) { break } tokio::time::sleep(Duration::from_millis(40)).await; }
    assert!(peers(&sb).contains(&urls[2]), "B learnt C: {:?}", peers(&sb));

    // A says hello to B and hears about C.
    let new = fed::current(&sa).unwrap().hello_all().await;
    assert!(new.contains(&urls[2]), "A heard of C: {new:?}");
    fed::add_peers(&sa, &new);
    assert!(peers(&sa).contains(&urls[2]));

    // X has another secret: not one of us.
    let f = fed::current(&sa).unwrap();
    assert!(f.same_network(&urls[1]).await);
    assert!(!f.same_network(&urls[3]).await);

    // Invitation: the address and the secret, back again.
    let inv = fed::invite(&f.cfg).unwrap();
    assert!(inv.starts_with("aquafed:"));
    assert_eq!(fed::parse_invite(&inv), Some((urls[0].clone(), SECRET.to_string())));
    assert_eq!(fed::parse_invite("aquafed:garbage"), None);
}

/// 1.2.0-4: a solo relay that joined a network "for names" shares only the
/// registry of (ID, @name) with it. A name is taken once across both, but
/// neither side gets the other's accounts or finds them in search. A pass
/// from a relay of the network lets its person into the private relay.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn solo_relay_shares_only_names() {
    let la = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let ls = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (a, s) = (la.local_addr().unwrap().to_string(), ls.local_addr().unwrap().to_string());
    let (ua, us) = (format!("http://{a}"), format!("http://{s}"));
    let tmp = tempfile::tempdir().unwrap();
    let sa = AppState::isolated(tmp.path().join("a"), Some(FedConfig { secret: SECRET.into(), peers: vec![], public_url: ua.clone(), mode: "network".into(), ..Default::default() }));
    let ss = AppState::isolated(tmp.path().join("s"), Some(FedConfig { secret: SECRET.into(), peers: vec![ua.clone()], public_url: us.clone(), mode: "solo".into(), ..Default::default() }));
    assert!(fed::current(&ss).is_none(), "a solo relay isn't in the network");
    assert!(fed::registry(&ss).is_some());
    { let s = sa.clone(); tokio::spawn(async move { axum::serve(la, app(s)).await.unwrap() }); }
    { let s = ss.clone(); tokio::spawn(async move { axum::serve(ls, app(s)).await.unwrap() }); }
    tokio::time::sleep(Duration::from_millis(100)).await;
    let http = reqwest::Client::builder().user_agent(concat!("aqua-client-core/", env!("CARGO_PKG_VERSION"))).build().unwrap();
    async fn signup(http: &reqwest::Client, base: &str, user: &str, invite: &str) -> (u16, String) {
        let id = IdentityKeyPair::generate();
        let tok = token(http, base, &id).await;
        let r = http.post(format!("{base}/v1/users/signup")).bearer_auth(&tok)
            .json(&serde_json::json!({ "username": user, "password": "a-good-secret", "invite": invite })).send().await.unwrap();
        (r.status().as_u16(), tok)
    }
    let search = |base: String, q: &'static str| { let http = http.clone(); async move {
        http.get(format!("{base}/v1/users/search?q={q}")).send().await.unwrap().json::<serde_json::Value>().await.unwrap().as_array().map(|a| a.len()).unwrap_or(0)
    }};

    // @bob in the network; the solo relay learns only the name.
    let (st, bob_tok) = signup(&http, &ua, "bob", "").await;
    assert_eq!(st, 204);
    fed::sync_once(&ss, &mut Default::default()).await;
    assert_eq!(signup(&http, &us, "bob", "").await.0, 409, "taken in the network");
    assert!(ss.directory.identity_for("bob").is_none(), "no account copied to the solo relay");
    assert_eq!(search(us.clone(), "bob").await, 0);

    // @zed on the solo relay; the network learns only the name.
    assert_eq!(signup(&http, &us, "zed", "").await.0, 204);
    let reg = fed::registry(&ss).unwrap();
    fed::names_hello(&reg).await;
    fed::sync_once(&sa, &mut Default::default()).await;
    assert_eq!(signup(&http, &ua, "zed", "").await.0, 409, "taken on the solo relay");
    assert!(sa.directory.identity_for("zed").is_none());
    assert_eq!(search(ua.clone(), "zed").await, 0);
    assert_eq!(search(ua.clone(), "bob").await, 1);

    // The solo relay is private: only people from A, with a pass.
    {
        let mut c = ss.admin.write().unwrap();
        c.invite_only = true;
        c.allow = vec![format!("*@{}", aqua_server::host_of(&ua))];
    }
    assert_eq!(signup(&http, &us, "bobby", "").await.0, 403);
    let r: serde_json::Value = http.post(format!("{ua}/v1/me/pass")).bearer_auth(&bob_tok).json(&serde_json::json!({})).send().await.unwrap().json().await.unwrap();
    let pass = r["pass"].as_str().unwrap().to_string();
    assert!(pass.starts_with("aqpass:"));
    let forged = format!("{}x", &pass[..pass.len() - 1]);
    assert_eq!(signup(&http, &us, "bobby", &forged).await.0, 403, "a changed pass doesn't work");
    assert_eq!(signup(&http, &us, "bobby", &pass).await.0, 204);
    assert_eq!(signup(&http, &us, "bobby2", &pass).await.0, 403, "a pass works once");

    // Register once: with a fresh pass and no name, bob joins as @bob — the
    // same name and ID as on A, even though the name is reserved here.
    let r: serde_json::Value = http.post(format!("{ua}/v1/me/pass")).bearer_auth(&bob_tok).json(&serde_json::json!({})).send().await.unwrap().json().await.unwrap();
    let pass2 = r["pass"].as_str().unwrap().to_string();
    let id = IdentityKeyPair::generate();
    let tok = token(&http, &us, &id).await;
    let r = http.post(format!("{us}/v1/users/signup")).bearer_auth(&tok)
        .json(&serde_json::json!({ "username": "", "password": "", "invite": pass2 })).send().await.unwrap();
    assert!(r.status().is_success(), "{}", r.status());
    let v: serde_json::Value = r.json().await.unwrap();
    assert_eq!((v["username"].as_str(), v["joined"].as_bool()), (Some("bob"), Some(true)));
    assert_eq!(ss.directory.uid_of("bob"), sa.directory.uid_of("bob"));
    let me: serde_json::Value = http.get(format!("{us}/v1/me")).bearer_auth(&tok).send().await.unwrap().json().await.unwrap();
    assert_eq!(me["username"], "bob");
    // The network sees it as its own @bob, not a clash.
    fed::names_hello(&fed::registry(&ss).unwrap()).await;
    fed::sync_once(&sa, &mut Default::default()).await;
    assert!(sa.directory.identity_for("bob").is_some());
    assert_eq!(sa.directory.username_by_uid(&sa.directory.uid_of("bob").unwrap()).as_deref(), Some("bob"));

    // AQUA-SEC-002: a pass never writes a new key over an account a relay
    // already holds — on A (where @bob lives) a pass + another key is refused
    // and bob's own key stays.
    let bob_key = sa.directory.identity_for("bob");
    let r: serde_json::Value = http.post(format!("{ua}/v1/me/pass")).bearer_auth(&bob_tok).json(&serde_json::json!({})).send().await.unwrap().json().await.unwrap();
    let pass3 = r["pass"].as_str().unwrap().to_string();
    let intruder = IdentityKeyPair::generate();
    let itok = token(&http, &ua, &intruder).await;
    let r = http.post(format!("{ua}/v1/users/signup")).bearer_auth(&itok)
        .json(&serde_json::json!({ "username": "", "password": "", "invite": pass3 })).send().await.unwrap();
    assert!(r.status().is_client_error(), "joining over an existing account must fail: {}", r.status());
    assert_eq!(sa.directory.identity_for("bob"), bob_key, "the account's key is unchanged");
}

/// Register once (1.2.0-5): a relay that joins the network later takes every
/// account on its first sync, and people sign in there without signing up.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_new_relay_knows_everyone_after_its_first_sync() {
    let la = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let lb = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (a, b) = (la.local_addr().unwrap().to_string(), lb.local_addr().unwrap().to_string());
    let (ua, ub) = (format!("http://{a}"), format!("http://{b}"));
    let tmp = tempfile::tempdir().unwrap();
    let sa = AppState::isolated(tmp.path().join("a"), Some(FedConfig { secret: SECRET.into(), peers: vec![], public_url: ua.clone(), mode: "network".into(), ..Default::default() }));
    { let s = sa.clone(); tokio::spawn(async move { axum::serve(la, app(s)).await.unwrap() }); }
    tokio::time::sleep(Duration::from_millis(100)).await;
    let http = reqwest::Client::builder().user_agent(concat!("aqua-client-core/", env!("CARGO_PKG_VERSION"))).build().unwrap();
    // People sign up on A before B exists.
    let mut ids = Vec::new();
    for n in 0..30 {
        let id = IdentityKeyPair::generate();
        let tok = token(&http, &ua, &id).await;
        let r = http.post(format!("{ua}/v1/users/signup")).bearer_auth(&tok)
            .json(&serde_json::json!({ "username": format!("early{n}"), "password": "a-good-secret" })).send().await.unwrap();
        assert_eq!(r.status().as_u16(), 204);
        ids.push(id);
    }
    // B starts and joins.
    let sb = AppState::isolated(tmp.path().join("b"), Some(FedConfig { secret: SECRET.into(), peers: vec![ua.clone()], public_url: ub.clone(), mode: "network".into(), ..Default::default() }));
    { let s = sb.clone(); tokio::spawn(async move { axum::serve(lb, app(s)).await.unwrap() }); }
    tokio::time::sleep(Duration::from_millis(100)).await;
    fed::sync_once(&sb, &mut Default::default()).await;
    for n in 0..30 {
        assert!(sb.directory.identity_for(&format!("early{n}")).is_some(), "early{n} reached B");
    }
    // early7 connects to B with the same key: no sign-up there.
    let tok = token(&http, &ub, &ids[7]).await;
    let me: serde_json::Value = http.get(format!("{ub}/v1/me")).bearer_auth(&tok).send().await.unwrap().json().await.unwrap();
    assert_eq!(me["username"], "early7");
    let w = tokio_tungstenite::connect_async(format!("ws://{b}/v1/ws?user=early7&token={tok}&v=1.21.0-23")).await;
    assert!(w.is_ok());
}

/// REV-001: pages of accounts that share one `updated_at` (1200 here, the
/// page is 500) all reach the other relay — the cursor is
/// `(updated_at, username)`, so a page boundary inside equal timestamps
/// skips nothing; a second sync and a fresh start ("restart") add nothing
/// and lose nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn equal_timestamps_across_pages_are_not_skipped() {
    let la = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let lb = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (a, b) = (la.local_addr().unwrap().to_string(), lb.local_addr().unwrap().to_string());
    let (ua, ub) = (format!("http://{a}"), format!("http://{b}"));
    let tmp = tempfile::tempdir().unwrap();
    let cfg = |me: &str, peer: &str| FedConfig { secret: SECRET.into(), peers: vec![peer.into()], public_url: me.into(), ..Default::default() };
    let sa = AppState::isolated(tmp.path().join("a"), Some(cfg(&ua, &ub)));
    let sb = AppState::isolated(tmp.path().join("b"), Some(cfg(&ub, &ua)));
    { let s = sa.clone(); tokio::spawn(async move { axum::serve(la, app(s)).await.unwrap() }); }
    { let s = sb.clone(); tokio::spawn(async move { axum::serve(lb, app(s)).await.unwrap() }); }
    tokio::time::sleep(Duration::from_millis(100)).await;
    let http = reqwest::Client::builder().user_agent(concat!("aqua-client-core/", env!("CARGO_PKG_VERSION"))).build().unwrap();

    // One real account on A as a template, then 1200 more with one timestamp.
    let id = IdentityKeyPair::generate();
    register(&http, &ua, &id, "tmpl").await;
    let tmpl = sa.directory.fed_users_since(0).into_iter().find(|u| u.username == "tmpl").unwrap();
    const N: usize = 1200;
    let t = 1_900_000_000_000i64;
    for i in 0..N {
        let mut u = tmpl.clone();
        u.username = format!("same{i:04}");
        u.uid = format!("{:010}", 4_000_000_000u64 + i as u64);
        u.identity_dh = hex::encode(IdentityKeyPair::generate().public().dh_pub.as_bytes());
        u.updated_at = t;
        assert!(sa.directory.fed_apply_user(&u).applied, "{i}");
    }
    let on_b = |sb: &AppState| (0..N).filter(|i| sb.directory.identity_for(&format!("same{i:04}")).is_some()).count();

    let mut cursors = Default::default();
    fed::sync_once(&sb, &mut cursors).await;
    assert_eq!(on_b(&sb), N, "every account with the shared timestamp arrived");
    fed::sync_once(&sb, &mut cursors).await;
    assert_eq!(on_b(&sb), N, "a second sync changes nothing");
    fed::sync_once(&sb, &mut Default::default()).await;
    assert_eq!(on_b(&sb), N, "a fresh start (restart) reads them again without loss");
}

/// AQUA-SEC-009 end to end: after A and B say hello they know each other's
/// node keys and talk with v2 signatures; a third party that knows only the
/// shared secret can no longer read B's accounts. A key change on A moves
/// B's pin; revoking A on B shuts A out.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn node_keys_replace_the_shared_secret_once_known() {
    let la = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let lb = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (a, b) = (la.local_addr().unwrap().to_string(), lb.local_addr().unwrap().to_string());
    let (ua, ub) = (format!("http://{a}"), format!("http://{b}"));
    let tmp = tempfile::tempdir().unwrap();
    let cfg = |me: &str, peer: &str| FedConfig { secret: SECRET.into(), peers: vec![peer.into()], public_url: me.into(), mode: "network".into(), ..Default::default() };
    let sa = AppState::isolated(tmp.path().join("a"), Some(cfg(&ua, &ub)));
    let sb = AppState::isolated(tmp.path().join("b"), Some(cfg(&ub, &ua)));
    { let s = sa.clone(); tokio::spawn(async move { axum::serve(la, app(s)).await.unwrap() }); }
    { let s = sb.clone(); tokio::spawn(async move { axum::serve(lb, app(s)).await.unwrap() }); }
    tokio::time::sleep(Duration::from_millis(100)).await;
    let http = reqwest::Client::builder().user_agent(concat!("aqua-client-core/", env!("CARGO_PKG_VERSION"))).build().unwrap();
    let id = IdentityKeyPair::generate();
    let tb = register(&http, &ub, &id, "keyed").await;
    let spk = SignedPreKey::generate(1, &id);
    let up = BundleUpload { identity: id.public(), signed_prekey: spk.public(), one_time_prekeys: vec![] };
    assert!(http.post(format!("{ub}/v1/bundle/keyed")).bearer_auth(&tb).json(&up).send().await.unwrap().status().is_success());

    // Hellos both ways: each pins the other.
    fed::current(&sa).unwrap().hello_all().await;
    for _ in 0..50 {
        if fed::current(&sa).unwrap().strict() && fed::current(&sb).unwrap().strict() { break }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(fed::current(&sb).unwrap().strict(), "B knows A's key");
    assert!(fed::current(&sa).unwrap().strict(), "A knows B's key (B said hello back)");
    assert!(fed::current(&sa).unwrap().fetch_bundle("keyed").await.is_some(), "A reads through v2");

    // Someone with only the secret (a relay of its own at another address).
    let lx = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let ux = format!("http://{}", lx.local_addr().unwrap());
    let sx = AppState::isolated(tmp.path().join("x"), Some(cfg(&ux, &ub)));
    { let s = sx.clone(); tokio::spawn(async move { axum::serve(lx, app(s)).await.unwrap() }); }
    assert!(fed::current(&sx).unwrap().fetch_bundle("keyed").await.is_none(), "secret alone isn't enough any more");
    assert!(sx.directory.identity_for("keyed").is_none());

    // A changes its key: B moves its pin, A still gets through.
    let old = fed::current(&sa).unwrap().node_id();
    let new = fed::rotate_key(&sa).await.unwrap();
    assert_ne!(old, new);
    let pinned = |s: &AppState| fed::current(s).unwrap().pin_for(&ua).map(|p| p.id.clone());
    for _ in 0..50 { if pinned(&sb).as_deref() == Some(new.as_str()) { break } tokio::time::sleep(Duration::from_millis(50)).await; }
    assert_eq!(pinned(&sb).as_deref(), Some(new.as_str()), "B follows the signed key change");
    assert!(fed::current(&sa).unwrap().fetch_bundle("keyed").await.is_some());

    // B revokes A's node id: A is out.
    assert!(fed::revoke(&sb, &new));
    assert!(fed::current(&sa).unwrap().fetch_bundle("keyed").await.is_none(), "a revoked node is refused");
}

/// Three relays: A and B keyed and strict. Spin up the third with only the
/// shared secret and see what it can do.
async fn strict_pair() -> (AppState, AppState, String, String, tempfile::TempDir) {
    let la = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let lb = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (ua, ub) = (format!("http://{}", la.local_addr().unwrap()), format!("http://{}", lb.local_addr().unwrap()));
    let tmp = tempfile::tempdir().unwrap();
    let cfg = |me: &str, peer: &str| FedConfig { secret: SECRET.into(), peers: vec![peer.into()], public_url: me.into(), mode: "network".into(), ..Default::default() };
    let sa = AppState::isolated(tmp.path().join("a"), Some(cfg(&ua, &ub)));
    let sb = AppState::isolated(tmp.path().join("b"), Some(cfg(&ub, &ua)));
    { let s = sa.clone(); tokio::spawn(async move { axum::serve(la, app(s)).await.unwrap() }); }
    { let s = sb.clone(); tokio::spawn(async move { axum::serve(lb, app(s)).await.unwrap() }); }
    tokio::time::sleep(Duration::from_millis(100)).await;
    fed::current(&sa).unwrap().hello_all().await;
    for _ in 0..60 {
        if fed::current(&sa).unwrap().strict() && fed::current(&sb).unwrap().strict() { break }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(fed::current(&sa).unwrap().strict() && fed::current(&sb).unwrap().strict());
    (sa, sb, ua, ub, tmp)
}

async fn third(tmp: &tempfile::TempDir, name: &str, peer: &str, join_token: &str) -> (AppState, String) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let u = format!("http://{}", l.local_addr().unwrap());
    let c = FedConfig { secret: SECRET.into(), peers: vec![peer.into()], public_url: u.clone(), mode: "network".into(), join_token: join_token.into(), ..Default::default() };
    let s = AppState::isolated(tmp.path().join(name), Some(c));
    { let s = s.clone(); tokio::spawn(async move { axum::serve(l, app(s)).await.unwrap() }); }
    tokio::time::sleep(Duration::from_millis(50)).await;
    (s, u)
}

/// REV2-004: impersonation and admission. With only the shared secret a
/// relay can't join a keyed network: it waits for the admin and reads
/// nothing; can't change accounts there; a revoked relay can't come back;
/// an invitation token admits once, and the admitted relay is vouched for
/// to the rest of the network.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn admission_impersonation_and_rejoining() {
    let (sa, sb, ua, ub, tmp) = strict_pair().await;
    let http = reqwest::Client::builder().user_agent(concat!("aqua-client-core/", env!("CARGO_PKG_VERSION"))).build().unwrap();
    let id = IdentityKeyPair::generate();
    let tb = register(&http, &ub, &id, "victim").await;
    let up = BundleUpload { identity: id.public(), signed_prekey: SignedPreKey::generate(1, &id).public(), one_time_prekeys: vec![] };
    assert!(http.post(format!("{ub}/v1/bundle/victim")).bearer_auth(&tb).json(&up).send().await.unwrap().status().is_success());
    let victim_key = sb.directory.identity_for("victim");

    // 1) Secret only, no token: waits, reads nothing, isn't added as a peer.
    let (sx, ux) = third(&tmp, "x", &ub, "").await;
    fed::current(&sx).unwrap().hello_all().await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let b = fed::current(&sb).unwrap();
    assert!(b.cfg.pending_nodes.iter().any(|p| p.url == ux), "X waits for the admin");
    assert!(!b.cfg.peers.contains(&ux), "X isn't a peer");
    assert!(b.strict(), "B stays strict");
    assert!(fed::current(&sx).unwrap().fetch_bundle("victim").await.is_none(), "X reads nothing");
    // X can't change the victim's account on B: B never pulls from it, and
    // its pokes (secret only) are refused.
    let r = http.post(format!("{ub}/v1/fed/poke")).body("{}").send().await.unwrap();
    assert_eq!(r.status().as_u16(), 401);
    fed::sync_once(&sb, &mut Default::default()).await;
    assert_eq!(sb.directory.identity_for("victim"), victim_key, "the account is unchanged");

    // 2) An impostor claiming A's address, with the secret and its own key:
    // B keeps A's key (no signature by it over the new one).
    let a_id = fed::current(&sa).unwrap().node_id();
    let imp = fed::Fed::new(FedConfig { secret: SECRET.into(), peers: vec![ub.clone()], public_url: ua.clone(), mode: "network".into(), ephemeral: true, ..Default::default() }).unwrap();
    assert_ne!(imp.node_id(), a_id);
    imp.hello_to(&ub).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(fed::current(&sb).unwrap().pin_for(&ua).map(|p| p.id.clone()), Some(a_id.clone()), "A's key is kept");
    assert!(imp.fetch_bundle("victim").await.is_none(), "the impostor reads nothing");

    // 3) An invitation token admits once; the admitted relay is vouched for.
    let token = fed::new_admit_token(&sb).unwrap();
    let (sn, un) = third(&tmp, "n", &ub, &token).await;
    fed::current(&sn).unwrap().hello_all().await;
    for _ in 0..40 { if fed::current(&sb).unwrap().pin_for(&un).is_some() { break } tokio::time::sleep(Duration::from_millis(50)).await; }
    assert!(fed::current(&sb).unwrap().pin_for(&un).is_some(), "N joined with the token");
    fed::absorb_vouches(&sn);
    assert!(!fed::current(&sn).unwrap().cfg.vouches.is_empty(), "B vouched for N");
    assert!(fed::current(&sn).unwrap().cfg.join_token.is_empty(), "the token is dropped once admitted");
    // The same token again (another relay): refused.
    let (sy, uy) = third(&tmp, "y", &ub, &token).await;
    fed::current(&sy).unwrap().hello_all().await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(fed::current(&sb).unwrap().pin_for(&uy).is_none(), "a token works once");
    // N says hello to A with B's vouch: A admits it too.
    let mut c = fed::current(&sn).unwrap().cfg.clone();
    c.peers.push(ua.clone());
    if let Ok(mut g) = sn.fed.write() { *g = fed::Fed::new(c); }
    fed::current(&sn).unwrap().hello_all().await;
    for _ in 0..40 { if fed::current(&sa).unwrap().pin_for(&un).is_some() { break } tokio::time::sleep(Duration::from_millis(50)).await; }
    assert!(fed::current(&sa).unwrap().pin_for(&un).is_some(), "A admits N on B's vouch");

    // 4) Re-joining: revoke N on B; its hellos are ignored; with a new key and
    // no token it only waits.
    let n_id = fed::current(&sn).unwrap().node_id();
    assert!(fed::revoke(&sb, &n_id));
    fed::current(&sn).unwrap().hello_to(&ub).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(fed::current(&sb).unwrap().pin_for(&un).is_none(), "a revoked relay isn't pinned again");
    assert!(fed::current(&sn).unwrap().fetch_bundle("victim").await.is_none() || !fed::current(&sb).unwrap().cfg.peers.contains(&un));
    let mut c = fed::current(&sn).unwrap().cfg.clone();
    c.node_key = hex::encode(aqua_core::crypto::SignSecret::generate().to_bytes());
    c.vouches.clear();
    if let Ok(mut g) = sn.fed.write() { *g = fed::Fed::new(c); }
    fed::current(&sn).unwrap().hello_to(&ub).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let b = fed::current(&sb).unwrap();
    assert!(b.pin_for(&un).is_none(), "a new key after revocation isn't let in by itself");
    assert!(b.cfg.pending_nodes.iter().any(|p| p.url == un), "…it waits for the admin");
    // The admin admits it.
    let pid = b.cfg.pending_nodes.iter().find(|p| p.url == un).unwrap().id.clone();
    assert!(fed::admit(&sb, &pid));
    assert!(fed::current(&sb).unwrap().pin_for(&un).is_some());
}

/// 1.21.0-23: what a relay pulls from a keyed peer is signed by that peer's
/// node key over the request. A proxy in between that changes an account in
/// a `/v1/fed/users` answer (a man in the middle on plain http) gets nothing
/// applied; the same path without tampering works.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tampered_sync_answers_are_refused() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let la = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let lp = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let lb = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let real_a = format!("http://{}", la.local_addr().unwrap());
    let (up, ub) = (format!("http://{}", lp.local_addr().unwrap()), format!("http://{}", lb.local_addr().unwrap()));
    let tmp = tempfile::tempdir().unwrap();
    let cfg = |me: &str, peer: &str| FedConfig { secret: SECRET.into(), peers: vec![peer.into()], public_url: me.into(), mode: "network".into(), ..Default::default() };
    // A is known to B only through the proxy's address.
    let sa = AppState::isolated(tmp.path().join("a"), Some(cfg(&up, &ub)));
    let sb = AppState::isolated(tmp.path().join("b"), Some(cfg(&ub, &up)));
    { let s = sa.clone(); tokio::spawn(async move { axum::serve(la, app(s)).await.unwrap() }); }
    { let s = sb.clone(); tokio::spawn(async move { axum::serve(lb, app(s)).await.unwrap() }); }
    let evil = IdentityKeyPair::generate();
    let evil_hex = hex::encode(evil.public().dh_pub.as_bytes());
    static TAMPER: AtomicBool = AtomicBool::new(false);
    {
        let real_a = real_a.clone();
        let evil_hex = evil_hex.clone();
        let client = reqwest::Client::new();
        let proxy = axum::Router::new().fallback(move |req: axum::extract::Request| {
            let (real_a, evil_hex, client) = (real_a.clone(), evil_hex.clone(), client.clone());
            async move {
                let (parts, body) = req.into_parts();
                let body = axum::body::to_bytes(body, 64 << 20).await.unwrap();
                let pq = parts.uri.path_and_query().map(|p| p.as_str().to_string()).unwrap_or_default();
                let mut rb = client.request(parts.method.clone(), format!("{real_a}{pq}")).body(body.to_vec());
                for (k, v) in parts.headers.iter() { if k != "host" { rb = rb.header(k, v); } }
                let r = rb.send().await.unwrap();
                let status = r.status();
                let headers = r.headers().clone();
                let mut bytes = r.bytes().await.unwrap().to_vec();
                if TAMPER.load(Ordering::SeqCst) && pq.starts_with("/v1/fed/users") {
                    let mut rows: Vec<serde_json::Value> = serde_json::from_slice(&bytes).unwrap_or_default();
                    for row in rows.iter_mut() {
                        if row["username"] == "mitmvictim" { row["identity_dh"] = serde_json::Value::String(evil_hex.clone()); }
                    }
                    bytes = serde_json::to_vec(&rows).unwrap();
                }
                let mut out = axum::http::Response::builder().status(status);
                for (k, v) in headers.iter() { if k != "content-length" && k != "transfer-encoding" { out = out.header(k, v); } }
                out.body(axum::body::Body::from(bytes)).unwrap()
            }
        });
        tokio::spawn(async move { axum::serve(lp, proxy).await.unwrap() });
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    let http = reqwest::Client::builder().user_agent(concat!("aqua-client-core/", env!("CARGO_PKG_VERSION"))).build().unwrap();

    // Keys known both ways (hellos go through the proxy untouched).
    fed::current(&sa).unwrap().hello_all().await;
    for _ in 0..60 {
        if fed::current(&sa).unwrap().strict() && fed::current(&sb).unwrap().strict() { break }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(fed::current(&sb).unwrap().pin_for(&up).is_some(), "B knows A's key");

    let id = IdentityKeyPair::generate();
    register(&http, &real_a, &id, "mitmvictim").await;

    // The proxy swaps the victim's key in the answer: B applies nothing.
    TAMPER.store(true, Ordering::SeqCst);
    fed::sync_once(&sb, &mut Default::default()).await;
    assert_ne!(sb.directory.identity_for("mitmvictim"), Some(*evil.public().dh_pub.as_bytes()), "a tampered answer must not change the account");
    assert!(sb.directory.identity_for("mitmvictim").is_none(), "the tampered page was dropped as a whole");

    // Untouched: the real account arrives.
    TAMPER.store(false, Ordering::SeqCst);
    fed::sync_once(&sb, &mut Default::default()).await;
    assert_eq!(sb.directory.identity_for("mitmvictim"), Some(*id.public().dh_pub.as_bytes()));
}
