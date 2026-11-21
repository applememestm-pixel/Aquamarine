//! Opt-in processing by an operator-run loopback worker.

use axum::{body::Bytes, extract::State, http::StatusCode, Json};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{quota, AppState, Caller};

pub const AUDIO_LIMIT: usize = 16_000 * 2 * 600;
type Failure = (StatusCode, &'static str);

fn worker(var: &str) -> Option<reqwest::Url> {
    let url = reqwest::Url::parse(&std::env::var(var).ok()?).ok()?;
    let host = url
        .host_str()?
        .trim_matches(['[', ']'])
        .parse::<std::net::IpAddr>()
        .ok()?;
    (url.scheme() == "http"
        && host.is_loopback()
        && url.username().is_empty()
        && url.password().is_none())
    .then_some(url)
}

pub async fn capabilities(State(state): State<AppState>, Caller(dh): Caller) -> Json<Value> {
    let available = quota::paid(quota::caller(&state, &dh).await.1);
    Json(json!({
        "pro_bots": available,
        "speech": available && worker("AQUA_STT_WORKER").is_some(),
        "translation": available && worker("AQUA_TRANSLATION_WORKER").is_some(),
    }))
}

async fn permitted(state: &AppState, dh: &[u8; 32], key: &str) -> Result<(), Failure> {
    let (owner, tier) = quota::caller(state, dh).await;
    if !quota::paid(tier) {
        return Err((StatusCode::FORBIDDEN, "Stars subscription required"));
    }
    if !state
        .bot_limits
        .take(&format!("paid:{key}:{owner}"), 3.0, 0.1)
    {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            "too many processing requests",
        ));
    }
    Ok(())
}

fn client(timeout: u64) -> Result<reqwest::Client, Failure> {
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(timeout))
        .build()
        .map_err(|_| unavailable())
}

fn unavailable() -> Failure {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        "processing worker unavailable",
    )
}

async fn output(req: reqwest::RequestBuilder) -> Result<Json<Value>, Failure> {
    let mut response = req
        .send()
        .await
        .map_err(|_| unavailable())?
        .error_for_status()
        .map_err(|_| unavailable())?;
    let mut bytes = Vec::new();
    while let Some(part) = response.chunk().await.map_err(|_| unavailable())? {
        if bytes.len() + part.len() > 256 * 1024 {
            return Err(unavailable());
        }
        bytes.extend_from_slice(&part);
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| unavailable())?;
    let text = value["text"].as_str().ok_or_else(unavailable)?;
    Ok(Json(json!({ "text": text })))
}

#[derive(Deserialize)]
pub struct Translation {
    text: String,
    target: String,
}

pub async fn translate(
    State(state): State<AppState>,
    Caller(dh): Caller,
    Json(body): Json<Translation>,
) -> Result<Json<Value>, Failure> {
    let target = body.target.trim();
    if body.text.trim().is_empty()
        || body.text.len() > 32 * 1024
        || !(2..=16).contains(&target.len())
        || !target.bytes().all(|b| b.is_ascii_alphabetic() || b == b'-')
    {
        return Err((StatusCode::BAD_REQUEST, "invalid translation request"));
    }
    permitted(&state, &dh, "translation").await?;
    let url = worker("AQUA_TRANSLATION_WORKER").ok_or_else(unavailable)?;
    output(
        client(60)?
            .post(url)
            .json(&json!({ "text": body.text, "target": target })),
    )
    .await
}

pub async fn speech(
    State(state): State<AppState>,
    Caller(dh): Caller,
    body: Bytes,
) -> Result<Json<Value>, Failure> {
    if body.is_empty() || body.len() > AUDIO_LIMIT || body.len() % 2 != 0 {
        return Err((StatusCode::BAD_REQUEST, "invalid 16 kHz mono PCM"));
    }
    permitted(&state, &dh, "speech").await?;
    let url = worker("AQUA_STT_WORKER").ok_or_else(unavailable)?;
    output(
        client(180)?
            .post(url)
            .header("Content-Type", "application/octet-stream")
            .body(body),
    )
    .await
}
