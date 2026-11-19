//! @music beyond Navidrome: Spotify and YouTube through their official APIs.
//!
//! Search and metadata only (tracks, albums, artists, videos, covers, links).
//! Nothing is downloaded from them — full files come from the network's
//! Navidromes alone; a Spotify/YouTube result opens in their own player.
//!
//! Keys (the relay's owner gets them from Spotify / Google):
//!   AQUA_SPOTIFY_ID, AQUA_SPOTIFY_SECRET  — Spotify Web API (client credentials)
//!   AQUA_YOUTUBE_KEY                       — YouTube Data API v3
//! Without them that source is simply not asked.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

fn env(k: &str) -> Option<String> {
    std::env::var(k).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

pub fn spotify_on() -> bool {
    env("AQUA_SPOTIFY_ID").is_some() && env("AQUA_SPOTIFY_SECRET").is_some()
}
pub fn youtube_on() -> bool {
    env("AQUA_YOUTUBE_KEY").is_some()
}

fn client() -> reqwest::Client {
    reqwest::Client::builder().timeout(Duration::from_secs(8)).build().unwrap_or_default()
}

static SPOTIFY_TOKEN: Mutex<Option<(String, Instant)>> = Mutex::new(None);

async fn spotify_token() -> Option<String> {
    if let Some((t, until)) = SPOTIFY_TOKEN.lock().ok().and_then(|g| g.clone()) {
        if Instant::now() < until {
            return Some(t);
        }
    }
    let (id, secret) = (env("AQUA_SPOTIFY_ID")?, env("AQUA_SPOTIFY_SECRET")?);
    let r = client()
        .post("https://accounts.spotify.com/api/token")
        .basic_auth(id, Some(secret))
        .form(&[("grant_type", "client_credentials")])
        .send()
        .await
        .ok()?;
    let v: Value = r.json().await.ok()?;
    let t = v["access_token"].as_str()?.to_string();
    let life = v["expires_in"].as_u64().unwrap_or(3600).saturating_sub(60);
    if let Ok(mut g) = SPOTIFY_TOKEN.lock() {
        *g = Some((t.clone(), Instant::now() + Duration::from_secs(life)));
    }
    Some(t)
}

fn s(v: &Value, p: &str) -> String {
    v.pointer(p).and_then(|x| x.as_str()).unwrap_or("").to_string()
}

fn best_image(images: &Value) -> String {
    images.as_array().and_then(|a| a.iter().find(|i| i["width"].as_u64().is_some_and(|w| w <= 320)).or(a.first()))
        .and_then(|i| i["url"].as_str()).unwrap_or("").to_string()
}

/// Spotify: `{songs, albums, artists}` in the app's item shape.
pub async fn spotify(q: &str, limit: usize) -> Value {
    let Some(tok) = spotify_token().await else { return json!({}) };
    let r = client()
        .get("https://api.spotify.com/v1/search")
        .bearer_auth(tok)
        .query(&[("q", q), ("type", "track,album,artist"), ("limit", &limit.min(20).to_string())])
        .send()
        .await;
    let Ok(r) = r else { return json!({}) };
    let v: Value = r.json().await.unwrap_or_default();
    let songs: Vec<Value> = v.pointer("/tracks/items").and_then(|x| x.as_array()).into_iter().flatten().map(|t| json!({
        "src": "spotify", "kind": "song", "id": s(t, "/id"),
        "title": s(t, "/name"),
        "artist": t["artists"].as_array().map(|a| a.iter().filter_map(|x| x["name"].as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default(),
        "album": s(t, "/album/name"),
        "dur": t["duration_ms"].as_u64().unwrap_or(0) / 1000,
        "cover": best_image(&t["album"]["images"]),
        "url": s(t, "/external_urls/spotify"),
    })).collect();
    let albums: Vec<Value> = v.pointer("/albums/items").and_then(|x| x.as_array()).into_iter().flatten().map(|a| json!({
        "src": "spotify", "kind": "album", "id": s(a, "/id"), "title": s(a, "/name"),
        "artist": a["artists"].as_array().map(|x| x.iter().filter_map(|y| y["name"].as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default(),
        "year": s(a, "/release_date").chars().take(4).collect::<String>(),
        "cover": best_image(&a["images"]), "url": s(a, "/external_urls/spotify"),
    })).collect();
    let artists: Vec<Value> = v.pointer("/artists/items").and_then(|x| x.as_array()).into_iter().flatten().map(|a| json!({
        "src": "spotify", "kind": "artist", "id": s(a, "/id"), "title": s(a, "/name"),
        "cover": best_image(&a["images"]), "url": s(a, "/external_urls/spotify"),
    })).collect();
    json!({ "songs": songs, "albums": albums, "artists": artists })
}

/// "PT4M52S" → 292.
fn iso_secs(d: &str) -> u64 {
    let mut total = 0u64;
    let mut n = 0u64;
    for c in d.chars() {
        match c {
            '0'..='9' => n = n * 10 + c.to_digit(10).unwrap_or(0) as u64,
            'H' => { total += n * 3600; n = 0 }
            'M' => { total += n * 60; n = 0 }
            'S' => { total += n; n = 0 }
            _ => n = 0,
        }
    }
    total
}

/// YouTube: `{videos}` (music first) in the app's item shape.
pub async fn youtube(q: &str, limit: usize) -> Value {
    let Some(key) = env("AQUA_YOUTUBE_KEY") else { return json!({}) };
    let c = client();
    let r = c
        .get("https://www.googleapis.com/youtube/v3/search")
        .query(&[("part", "snippet"), ("type", "video"), ("videoCategoryId", "10"), ("maxResults", &limit.min(20).to_string()), ("q", q), ("key", key.as_str())])
        .send()
        .await;
    let Ok(r) = r else { return json!({}) };
    let v: Value = r.json().await.unwrap_or_default();
    let items: Vec<Value> = v["items"].as_array().cloned().unwrap_or_default();
    let ids: Vec<String> = items.iter().filter_map(|i| i.pointer("/id/videoId").and_then(|x| x.as_str()).map(str::to_string)).collect();
    // Durations and views: a second, cheap call.
    let mut meta = std::collections::HashMap::new();
    if !ids.is_empty() {
        if let Ok(r) = c.get("https://www.googleapis.com/youtube/v3/videos")
            .query(&[("part", "contentDetails,statistics"), ("id", &ids.join(",")), ("key", key.as_str())])
            .send().await
        {
            let d: Value = r.json().await.unwrap_or_default();
            for it in d["items"].as_array().into_iter().flatten() {
                meta.insert(s(it, "/id"), (iso_secs(&s(it, "/contentDetails/duration")), s(it, "/statistics/viewCount").parse::<u64>().unwrap_or(0)));
            }
        }
    }
    let videos: Vec<Value> = items.iter().filter_map(|i| {
        let id = i.pointer("/id/videoId")?.as_str()?.to_string();
        let (dur, views) = meta.get(&id).copied().unwrap_or((0, 0));
        Some(json!({
            "src": "youtube", "kind": "video", "id": id,
            "title": s(i, "/snippet/title").replace("&quot;", "\"").replace("&#39;", "'").replace("&amp;", "&"),
            "artist": s(i, "/snippet/channelTitle"),
            "dur": dur, "views": views,
            "cover": s(i, "/snippet/thumbnails/high/url"),
            "url": format!("https://www.youtube.com/watch?v={id}"),
        }))
    }).collect();
    json!({ "videos": videos })
}

#[cfg(test)]
mod tests {
    #[test]
    fn iso_durations() {
        assert_eq!(super::iso_secs("PT4M52S"), 292);
        assert_eq!(super::iso_secs("PT1H2M3S"), 3723);
        assert_eq!(super::iso_secs("PT45S"), 45);
    }
}
