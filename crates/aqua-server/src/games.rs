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
}

/// What a search returns.
#[derive(Debug, Clone, Serialize)]
pub struct Hit {
    pub id: String,
    pub name: String,
    pub has_icon: bool,
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
    }
    serde_json::from_slice::<Vec<App>>(raw)
        .map(|v| v.into_iter()
            .filter(|a| !a.name.trim().is_empty() && a.id.chars().all(|c| c.is_ascii_digit()))
            .map(|a| Game { id: a.id, name: a.name.trim().to_string(), icon: a.icon, aliases: a.aliases })
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
    scored.into_iter().take(limit).map(|(_, _, g)| Hit { id: g.id.clone(), name: g.name.clone(), has_icon: g.icon.is_some() }).collect()
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
            if !fresh {
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse_and_search() {
        let raw = br#"[{"id":"1","name":"Minecraft","icon":"abc","aliases":["MC"],"executables":[{"name":"javaw.exe","os":"win32"}]},
                      {"id":"2","name":"Teardown","icon":null},
                      {"id":"3","name":"Minecraft Dungeons","icon":"def"},
                      {"id":"x","name":"bad id"},
                      {"id":"4","name":"Portal 2","icon":"p2"}]"#;
        let l = parse_detectable(raw);
        assert_eq!(l.len(), 4);
        let h = search(&l, "minec", 10);
        assert_eq!(h[0].name, "Minecraft");
        assert_eq!(h[1].name, "Minecraft Dungeons");
        assert_eq!(search(&l, "mc", 10)[0].id, "1");
        assert_eq!(search(&l, "2", 10)[0].name, "Portal 2");
        assert!(!search(&l, "teardown", 10)[0].has_icon);
        assert!(search(&l, "", 10).is_empty());
    }
}
