//! Game catalogue for profiles: names and icons of PC games.
//!
//! The source is Discord's public list of "detectable" applications (the
//! one its client uses to show "Playing …"): ~30 000 games with names,
//! aliases and icon hashes. The relay downloads it once, keeps a trimmed
//! copy in `aqua-games.json`, refreshes it weekly and answers searches
//! itself; icons are fetched once and cached under `aqua-games/`. Clients
//! never talk to Discord directly.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};

const SOURCE: &str = "https://discord.com/api/v9/applications/detectable";
const CACHE_FILE: &str = "aqua-games.json";
const ICON_DIR: &str = "aqua-games";
const REFRESH: Duration = Duration::from_secs(7 * 24 * 3600);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Game {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    /// Steam app id, when Discord knows it: gives the portrait cover art.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steam: Option<String>,
}

/// What a search returns.
#[derive(Debug, Clone, Serialize)]
pub struct Hit {
    pub id: String,
    pub name: String,
    pub has_icon: bool,
    /// A portrait cover is available (`/v1/games/cover/:id`).
    pub has_cover: bool,
}

#[derive(Clone, Default)]
pub struct GameDb {
    list: Arc<RwLock<Vec<Game>>>,
    loading: Arc<Mutex<()>>,
}

fn cache_path() -> PathBuf {
    std::env::var("AQUA_GAMES_CACHE").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from(CACHE_FILE))
}

/// Parse Discord's JSON into our trimmed list.
pub fn parse_detectable(raw: &[u8]) -> Vec<Game> {
    #[derive(Deserialize)]
    struct App {
        id: String,
        name: String,
        #[serde(default)]
        icon: Option<String>,
        #[serde(default)]
        aliases: Vec<String>,
        #[serde(default)]
        third_party_skus: Vec<Sku>,
    }
    #[derive(Deserialize)]
    struct Sku {
        #[serde(default)]
        distributor: String,
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        sku: Option<String>,
    }
    serde_json::from_slice::<Vec<App>>(raw)
        .map(|v| v.into_iter()
            .filter(|a| !a.name.trim().is_empty() && a.id.chars().all(|c| c.is_ascii_digit()))
            .map(|a| {
                let steam = a.third_party_skus.iter()
                    .filter(|k| k.distributor == "steam")
                    .find_map(|k| k.id.clone().or_else(|| k.sku.clone()))
                    .filter(|x| !x.is_empty() && x.len() < 12 && x.chars().all(|c| c.is_ascii_digit()));
                Game { id: a.id, name: a.name.trim().to_string(), icon: a.icon, aliases: a.aliases, steam }
            })
            .collect())
        .unwrap_or_default()
}

fn norm(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| c.is_alphanumeric() || c.is_whitespace()).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Best matches for `q`: exact, then prefix, then word-prefix, then substring.
pub fn search(list: &[Game], q: &str, limit: usize) -> Vec<Hit> {
    let q = norm(q);
    if q.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<(u8, usize, &Game)> = Vec::new();
    for g in list {
        let mut best: Option<u8> = None;
        for name in std::iter::once(&g.name).chain(g.aliases.iter()) {
            let n = norm(name);
            let s = if n == q { 0 } else if n.starts_with(&q) { 1 } else if n.split(' ').any(|w| w.starts_with(&q)) { 2 } else if n.contains(&q) { 3 } else { continue };
            best = Some(best.map_or(s, |b| b.min(s)));
        }
        if let Some(s) = best {
            scored.push((s, g.name.len(), g));
        }
    }
    scored.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    scored.into_iter().take(limit).map(|(_, _, g)| Hit { id: g.id.clone(), name: g.name.clone(), has_icon: g.icon.is_some(), has_cover: g.steam.is_some() }).collect()
}

impl GameDb {
    /// The list, loading it from the cache file or Discord the first time
    /// and refreshing it in the background when it is a week old.
    pub async fn get(&self) -> Vec<Game> {
        {
            let l = self.list.read().await;
            if !l.is_empty() {
                return l.clone();
            }
        }
        let _guard = self.loading.lock().await;
        if !self.list.read().await.is_empty() {
            return self.list.read().await.clone();
        }
        let path = cache_path();
        let fresh = std::fs::metadata(&path).and_then(|m| m.modified()).ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .is_some_and(|age| age < REFRESH);
        let cached: Vec<Game> = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        if !cached.is_empty() {
            *self.list.write().await = cached.clone();
            // A cache from before Steam ids were kept: fetch the list again.
            let old_format = !cached.iter().any(|g| g.steam.is_some());
            if !fresh || old_format {
                let me = self.clone();
                tokio::spawn(async move { me.download().await; });
            }
            return cached;
        }
        self.download().await;
        self.list.read().await.clone()
    }

    async fn download(&self) {
        let url = std::env::var("AQUA_GAMES_URL").unwrap_or_else(|_| SOURCE.into());
        let client = match reqwest::Client::builder().timeout(Duration::from_secs(60)).user_agent("aqua-server").build() {
            Ok(c) => c,
            Err(_) => return,
        };
        match client.get(&url).send().await.and_then(|r| r.error_for_status()) {
            Ok(r) => match r.bytes().await {
                Ok(b) => {
                    let list = parse_detectable(&b);
                    if list.is_empty() {
                        tracing::warn!("game list: nothing usable in the response");
                        return;
                    }
                    tracing::info!(games = list.len(), "game list downloaded");
                    if let Ok(json) = serde_json::to_vec(&list) {
                        let _ = std::fs::write(cache_path(), json);
                    }
                    *self.list.write().await = list;
                }
                Err(e) => tracing::warn!(%e, "game list download failed"),
            },
            Err(e) => tracing::warn!(%e, "game list download failed"),
        }
    }

    /// PNG icon of a game (cached on disk).
    pub async fn icon(&self, id: &str) -> Option<Vec<u8>> {
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) || id.len() > 24 {
            return None;
        }
        let dir = PathBuf::from(ICON_DIR);
        let file = dir.join(format!("{id}.png"));
        if let Ok(b) = std::fs::read(&file) {
            return Some(b);
        }
        let hash = self.get().await.into_iter().find(|g| g.id == id)?.icon?;
        if !hash.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return None;
        }
        let url = format!("https://cdn.discordapp.com/app-icons/{id}/{hash}.png?size=128");
        let bytes = reqwest::Client::new().get(url).timeout(Duration::from_secs(20)).send().await.ok()?
            .error_for_status().ok()?.bytes().await.ok()?;
        if bytes.len() > 512 * 1024 {
            return None;
        }
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(&file, &bytes);
        Some(bytes.to_vec())
    }
}

impl GameDb {
    /// Portrait cover art (Steam library capsule, 600×900) of a game, cached
    /// on disk: `(mime, bytes)`. Falls back to the landscape header image.
    pub async fn cover(&self, id: &str) -> Option<(String, Vec<u8>)> {
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) || id.len() > 24 {
            return None;
        }
        let dir = PathBuf::from(ICON_DIR).join("covers");
        let none = dir.join(format!("{id}.none"));
        for ext in ["jpg", "img"] {
            if let Ok(b) = std::fs::read(dir.join(format!("{id}.{ext}"))) {
                if let Some(m) = image_mime(&b) {
                    return Some((m.into(), b));
                }
            }
        }
        // Looked up recently and there was nothing: don't ask again for a day.
        if std::fs::metadata(&none).and_then(|m| m.modified()).ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .is_some_and(|age| age < Duration::from_secs(24 * 3600)) {
            return None;
        }
        let game = self.get().await.into_iter().find(|g| g.id == id)?;
        let client = reqwest::Client::builder().timeout(Duration::from_secs(20))
            .user_agent("aqua-server/0.2 (https://github.com/applememestm-pixel/Aquamarine)").build().ok()?;
        let _ = std::fs::create_dir_all(&dir);
        let steam = match game.steam.clone() {
            Some(s) => Some(s),
            // Not in Discord's list: Steam's store search by name.
            None => steam_app_by_name(&client, &game.name).await,
        };
        let mut urls: Vec<String> = Vec::new();
        if let Some(steam) = &steam {
            // Newer apps keep their art under hashed paths: ask the store.
            let (cap, header) = steam_assets(&client, steam).await;
            urls.extend(cap);
            urls.push(format!("https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/{steam}/library_600x900.jpg"));
            urls.push(format!("https://cdn.akamai.steamstatic.com/steam/apps/{steam}/library_600x900.jpg"));
            // Games that aren't on Steam (Minecraft, Roblox, Fortnite…), or
            // without a portrait capsule: the infobox art of their Wikipedia article.
            urls.push(String::new());
            urls.extend(header);
            urls.push(format!("https://cdn.akamai.steamstatic.com/steam/apps/{steam}/header.jpg"));
        } else {
            urls.push(String::new());
        }
        for u in urls {
            let u = if u.is_empty() {
                match wikipedia_cover(&client, "en", &game.name, "video game").await {
                    Some(w) => w,
                    None => match wikipedia_cover(&client, "ru", &game.name, "компьютерная игра").await { Some(w) => w, None => continue },
                }
            } else { u };
            let Ok(r) = client.get(&u).send().await else { continue };
            if !r.status().is_success() {
                continue;
            }
            let Ok(b) = r.bytes().await else { continue };
            if b.len() < 512 || b.len() > 4 * 1024 * 1024 {
                continue;
            }
            let Some(m) = image_mime(&b) else { continue };
            let _ = std::fs::write(dir.join(format!("{id}.img")), &b);
            return Some((m.into(), b.to_vec()));
        }
        let _ = std::fs::write(&none, b"");
        None
    }
}

/// Portrait capsule and header URLs of a Steam app, from the store API.
async fn steam_assets(client: &reqwest::Client, app: &str) -> (Option<String>, Option<String>) {
    let input = format!(r#"{{"ids":[{{"appid":{app}}}],"context":{{"language":"english","country_code":"US"}},"data_request":{{"include_assets":true}}}}"#);
    let Ok(r) = client.get("https://api.steampowered.com/IStoreBrowseService/GetItems/v1/").query(&[("input_json", input)]).send().await else { return (None, None) };
    let Ok(v) = r.json::<serde_json::Value>().await else { return (None, None) };
    let a = &v["response"]["store_items"][0]["assets"];
    let Some(fmt) = a["asset_url_format"].as_str() else { return (None, None) };
    let url = |k: &str| a[k].as_str().filter(|f| !f.is_empty() && !f.contains("..")).map(|f| format!("https://shared.akamai.steamstatic.com/store_item_assets/{}", fmt.replace("${FILENAME}", f)));
    (url("library_capsule"), url("header"))
}

/// JPEG / PNG / WebP by their magic bytes.
fn image_mime(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if b.starts_with(b"\x89PNG") {
        Some("image/png")
    } else if b.len() > 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

/// Cover art from the game's English Wikipedia article (the infobox image).
async fn wikipedia_cover(client: &reqwest::Client, lang: &str, name: &str, what: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct Resp { #[serde(default)] query: Option<Q> }
    #[derive(Deserialize)]
    struct Q { #[serde(default)] pages: std::collections::HashMap<String, Page> }
    #[derive(Deserialize)]
    struct Page { #[serde(default)] title: String, #[serde(default)] index: Option<u32>, #[serde(default)] original: Option<Img> }
    #[derive(Deserialize)]
    struct Img { source: String }
    let want = norm(name);
    let r: Resp = client
        .get(format!("https://{lang}.wikipedia.org/w/api.php"))
        .query(&[("action", "query"), ("format", "json"), ("redirects", "1"), ("prop", "pageimages"), ("piprop", "original"),
                 ("pilicense", "any"), ("generator", "search"), ("gsrsearch", &format!("{name} {what}")), ("gsrlimit", "5")])
        .send().await.ok()?.error_for_status().ok()?.json().await.ok()?;
    let mut pages: Vec<Page> = r.query?.pages.into_values().collect();
    pages.sort_by_key(|p| p.index.unwrap_or(99));
    // "Peak (video game)" → "peak".
    let base = |t: &str| norm(t.split(" (").next().unwrap_or(t));
    let ok = |p: &Page| p.original.as_ref().is_some_and(|o| {
        let l = o.source.to_lowercase();
        !l.contains(".svg") && (l.contains(".jpg") || l.contains(".jpeg") || l.contains(".png") || l.contains(".webp"))
    });
    let pick = pages.iter().find(|p| base(&p.title) == want && ok(p))
        .or_else(|| pages.iter().find(|p| base(&p.title).starts_with(&want) && ok(p)))?;
    pick.original.as_ref().map(|o| o.source.clone())
}

/// Steam app id of a game by its name (exact, else the base game).
async fn steam_app_by_name(client: &reqwest::Client, name: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct Resp { #[serde(default)] items: Vec<Item> }
    #[derive(Deserialize)]
    struct Item { id: u64, name: String }
    let r: Resp = client
        .get("https://store.steampowered.com/api/storesearch/")
        .query(&[("term", name), ("l", "english"), ("cc", "US")])
        .send().await.ok()?.error_for_status().ok()?.json().await.ok()?;
    let want = norm(name);
    let exact = r.items.iter().find(|i| norm(&i.name) == want);
    exact.or_else(|| r.items.iter().find(|i| {
        let n = norm(&i.name);
        // "Teardown" ↔ "Teardown Folkrace" is fine, "Peak" ↔ "Peaky Blinders" is not.
        n.starts_with(&format!("{want} ")) || n.starts_with(&format!("{want}:"))
    })).map(|i| i.id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse_and_search() {
        let raw = br#"[{"id":"1","name":"Minecraft","icon":"abc","aliases":["MC"],"executables":[{"name":"javaw.exe","os":"win32"}]},
                      {"id":"5","name":"Terraria","icon":"t","third_party_skus":[{"distributor":"steam","id":"105600","sku":"105600"}]},
                      {"id":"2","name":"Teardown","icon":null},
                      {"id":"3","name":"Minecraft Dungeons","icon":"def"},
                      {"id":"x","name":"bad id"},
                      {"id":"4","name":"Portal 2","icon":"p2"}]"#;
        let l = parse_detectable(raw);
        assert_eq!(l.len(), 5);
        assert_eq!(l.iter().find(|g| g.name == "Terraria").unwrap().steam.as_deref(), Some("105600"));
        assert!(search(&l, "terraria", 10)[0].has_cover);
        let h = search(&l, "minec", 10);
        assert_eq!(h[0].name, "Minecraft");
        assert_eq!(h[1].name, "Minecraft Dungeons");
        assert_eq!(search(&l, "mc", 10)[0].id, "1");
        assert_eq!(search(&l, "2", 10)[0].name, "Portal 2");
        assert!(!search(&l, "teardown", 10)[0].has_icon);
        assert!(search(&l, "", 10).is_empty());
    }
}
