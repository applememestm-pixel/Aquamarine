//! Discord-style "rich presence": what the user is playing, listening to
//! or watching, inferred locally from the visible windows on Windows
//! (process image path + window title). Nothing is read from inside other
//! programs; no screenshots. The result is only shared with chat peers when
//! the user leaves "Share my activity" on.
//!
//! `classify` is pure (testable anywhere); `snapshot` enumerates windows via
//! a few user32/kernel32 calls and is empty on non-Windows targets.
//!
//! Type-check the Windows code on Linux with:
//!   RUSTFLAGS="--cfg aqua_wincheck" cargo check

use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Activity {
    /// "playing" | "listening" | "watching"
    pub kind: String,
    /// App or game name ("Spotify", "YouTube", "Counter-Strike 2").
    pub name: String,
    /// Track / video title; empty for games.
    pub details: String,
    /// Artist / channel, when known.
    pub state: String,
}

#[derive(Debug, Clone)]
pub struct WindowInfo {
    /// Full image path of the owning process.
    pub exe: String,
    pub title: String,
    pub foreground: bool,
}

fn exe_name(path: &str) -> String {
    path.rsplit(['\\', '/']).next().unwrap_or(path).to_ascii_lowercase()
}

/// Well-known games whose exe (or folder) name isn't their real name.
const KNOWN_GAMES: &[(&str, &str)] = &[
    ("cs2.exe", "Counter-Strike 2"),
    ("csgo.exe", "Counter-Strike: Global Offensive"),
    ("dota2.exe", "Dota 2"),
    ("valorant-win64-shipping.exe", "VALORANT"),
    ("league of legends.exe", "League of Legends"),
    ("r5apex.exe", "Apex Legends"),
    ("r5apex_dx12.exe", "Apex Legends"),
    ("fortniteclient-win64-shipping.exe", "Fortnite"),
    ("robloxplayerbeta.exe", "Roblox"),
    ("minecraft.windows.exe", "Minecraft"),
    ("osu!.exe", "osu!"),
    ("gta5.exe", "Grand Theft Auto V"),
    ("gta5_enhanced.exe", "Grand Theft Auto V"),
    ("rdr2.exe", "Red Dead Redemption 2"),
    ("overwatch.exe", "Overwatch 2"),
    ("tslgame.exe", "PUBG: BATTLEGROUNDS"),
    ("rustclient.exe", "Rust"),
    ("eldenring.exe", "ELDEN RING"),
    ("cyberpunk2077.exe", "Cyberpunk 2077"),
    ("witcher3.exe", "The Witcher 3"),
    ("wow.exe", "World of Warcraft"),
    ("hearthstone.exe", "Hearthstone"),
    ("destiny2.exe", "Destiny 2"),
    ("rocketleague.exe", "Rocket League"),
    ("terraria.exe", "Terraria"),
    ("warframe.x64.exe", "Warframe"),
    ("escapefromtarkov.exe", "Escape from Tarkov"),
    ("genshinimpact.exe", "Genshin Impact"),
    ("starrail.exe", "Honkai: Star Rail"),
    ("zenlesszonezero.exe", "Zenless Zone Zero"),
    ("worldoftanks.exe", "World of Tanks"),
    ("project8.exe", "Deadlock"),
    ("marvelrivals-win64-shipping.exe", "Marvel Rivals"),
    ("deadbydaylight-win64-shipping.exe", "Dead by Daylight"),
    ("rainbowsix.exe", "Rainbow Six Siege"),
    ("rainbowsix_vulkan.exe", "Rainbow Six Siege"),
    ("bf2042.exe", "Battlefield 2042"),
    ("cod.exe", "Call of Duty"),
    ("factorio.exe", "Factorio"),
    ("stardew valley.exe", "Stardew Valley"),
    ("hollow_knight.exe", "Hollow Knight"),
    ("sekiro.exe", "Sekiro"),
    ("bg3.exe", "Baldur's Gate 3"),
    ("bg3_dx11.exe", "Baldur's Gate 3"),
    ("palworld-win64-shipping.exe", "Palworld"),
    ("helldivers2.exe", "HELLDIVERS 2"),
    ("phasmophobia.exe", "Phasmophobia"),
    ("lethal company.exe", "Lethal Company"),
];

/// Folders under `steamapps\common` (etc.) that are tools, not games.
const NOT_GAMES: &[&str] = &[
    "wallpaper_engine", "steamworks shared", "steam controller configs", "steamvr",
    "steam linux runtime", "steamlinuxruntime", "proton", "obs studio", "soundpad", "spacewar",
];

/// Helper processes that live inside game folders but aren't the game.
const HELPER_EXES: &[&str] = &[
    "unitycrashhandler64.exe", "unitycrashhandler32.exe", "crashreportclient.exe",
    "easyanticheat.exe", "easyanticheat_eos.exe", "battleye.exe", "beservice.exe",
    "launcher.exe", "steamerrorreporter.exe", "vc_redist.x64.exe", "dxsetup.exe",
];

const BROWSERS: &[&str] = &[
    "chrome.exe", "msedge.exe", "firefox.exe", "opera.exe", "browser.exe", "brave.exe",
    "vivaldi.exe", "yandex.exe", "arc.exe", "floorp.exe", "zen.exe", "librewolf.exe",
];

const BROWSER_SUFFIXES: &[&str] = &[
    " - Google Chrome", " - Microsoft​ Edge", " - Microsoft Edge", " — Mozilla Firefox",
    " - Mozilla Firefox", " - Opera", " - Yandex Browser", " — Яндекс Браузер", " - Яндекс Браузер",
    " - Brave", " - Vivaldi", " — Firefox", " - Arc",
];

fn game_from_path(path: &str) -> Option<String> {
    let lower = path.to_ascii_lowercase().replace('/', "\\");
    let exe = exe_name(&lower);
    if HELPER_EXES.contains(&exe.as_str()) {
        return None;
    }
    // Native Linux builds drop the ".exe" (cs2, dota2, …).
    let exe = if exe.contains('.') { exe } else { format!("{exe}.exe") };
    if let Some((_, name)) = KNOWN_GAMES.iter().find(|(e, _)| *e == exe) {
        return Some((*name).to_string());
    }
    for marker in ["\\steamapps\\common\\", "\\epic games\\", "\\riot games\\", "\\gog games\\", "\\xboxgames\\"] {
        if let Some(i) = lower.find(marker) {
            // Keep the original casing of the folder name.
            let rest = &path.replace('/', "\\")[i + marker.len()..];
            let folder = rest.split('\\').next().unwrap_or("").trim();
            let fl = folder.to_ascii_lowercase();
            if folder.is_empty() || fl == "launcher" || fl.contains("launcher") || NOT_GAMES.iter().any(|n| fl.starts_with(n)) {
                return None;
            }
            return Some(folder.to_string());
        }
    }
    None
}

fn split_artist_title(s: &str) -> Option<(String, String)> {
    for sep in [" - ", " — ", " – "] {
        if let Some((a, b)) = s.split_once(sep) {
            let (a, b) = (a.trim(), b.trim());
            if !a.is_empty() && !b.is_empty() {
                return Some((a.to_string(), b.to_string()));
            }
        }
    }
    None
}

fn strip_browser_suffix(title: &str) -> &str {
    let mut t = title.trim();
    for suf in BROWSER_SUFFIXES {
        if let Some(x) = t.strip_suffix(suf) {
            t = x.trim();
            break;
        }
    }
    // Leading "(3) " unread counters on YouTube / Twitch tabs.
    if t.starts_with('(') {
        if let Some(i) = t.find(") ") {
            if t[1..i].chars().all(|c| c.is_ascii_digit()) {
                t = t[i + 2..].trim();
            }
        }
    }
    t
}

fn music_app(exe: &str, title: &str) -> Option<Activity> {
    let title = title.trim();
    let listening = |name: &str, artist: String, track: String| Activity {
        kind: "listening".into(), name: name.into(), details: track, state: artist,
    };
    match exe {
        "spotify.exe" => {
            if title.is_empty() || title.starts_with("Spotify") {
                return None; // paused / idle shows "Spotify Premium"
            }
            let (artist, track) = split_artist_title(title)?;
            Some(listening("Spotify", artist, track))
        }
        "aimp.exe" | "aimp3.exe" => {
            let (artist, track) = split_artist_title(title.trim_end_matches(" - AIMP").trim())?;
            Some(listening("AIMP", artist, track))
        }
        "foobar2000.exe" => {
            let t = title.split(" [foobar2000").next()?.trim();
            if t.is_empty() || t.starts_with("foobar2000") {
                return None;
            }
            let (artist, track) = split_artist_title(t)?;
            Some(listening("foobar2000", artist, track))
        }
        "yandex music.exe" | "яндекс музыка.exe" | "yandexmusic.exe" => {
            if title.is_empty() || title.contains("Яндекс Музыка") && !title.contains(" — ") && !title.contains(" - ") {
                return None;
            }
            let (track, artist) = split_artist_title(title)?;
            Some(listening("Яндекс Музыка", artist, track))
        }
        "applemusic.exe" | "itunes.exe" => {
            let (artist, track) = split_artist_title(title)?;
            Some(listening("Apple Music", artist, track))
        }
        _ => None,
    }
}

fn video_player(exe: &str, title: &str) -> Option<Activity> {
    let watching = |name: &str, details: &str| Activity {
        kind: "watching".into(), name: name.into(), details: details.trim().into(), state: String::new(),
    };
    let t = title.trim();
    match exe {
        "vlc.exe" => {
            let d = t.strip_suffix(" - VLC media player")?;
            Some(watching("VLC", d))
        }
        "potplayermini64.exe" | "potplayermini.exe" | "potplayer.exe" => {
            let d = t.strip_suffix(" - PotPlayer")?;
            Some(watching("PotPlayer", d))
        }
        "mpc-hc64.exe" | "mpc-hc.exe" | "mpc-be64.exe" => {
            if t.is_empty() || t.starts_with("MPC") { return None; }
            Some(watching("MPC", t))
        }
        "mpv.exe" => {
            let d = t.strip_suffix(" - mpv").unwrap_or(t);
            if d.is_empty() || d == "mpv" { return None; }
            Some(watching("mpv", d))
        }
        _ => None,
    }
}

fn browser_tab(title: &str) -> Option<Activity> {
    let t = strip_browser_suffix(title);
    let act = |kind: &str, name: &str, details: &str, state: &str| Activity {
        kind: kind.into(), name: name.into(), details: details.trim().into(), state: state.trim().into(),
    };
    if let Some(d) = t.strip_suffix(" - YouTube") {
        return Some(act("watching", "YouTube", d, ""));
    }
    if let Some(d) = t.strip_suffix(" - YouTube Music") {
        let (track, artist) = split_artist_title(d).unwrap_or((d.to_string(), String::new()));
        return Some(act("listening", "YouTube Music", &track, &artist));
    }
    if let Some(d) = t.strip_suffix(" - Twitch") {
        return Some(act("watching", "Twitch", d, ""));
    }
    if let Some(d) = t.strip_suffix(" | Netflix").or_else(|| t.strip_suffix(" - Netflix")) {
        return Some(act("watching", "Netflix", d, ""));
    }
    if t.contains("Кинопоиск") {
        let d = t.split(" — Кинопоиск").next().unwrap_or(t);
        return Some(act("watching", "Кинопоиск", d, ""));
    }
    if t.contains("Яндекс Музык") || t.contains("Yandex Music") {
        let d = t.split(". Слушать").next().unwrap_or(t);
        let d = d.split(" — Яндекс Музык").next().unwrap_or(d);
        let (track, artist) = split_artist_title(d)?;
        return Some(act("listening", "Яндекс Музыка", &track, &artist));
    }
    // Spotify Web player tab: "Track • Artist" while playing.
    if let Some((track, artist)) = t.split_once(" • ") {
        if !track.trim().is_empty() && !artist.trim().is_empty() && !artist.contains("Spotify") {
            return Some(act("listening", "Spotify", track, artist));
        }
    }
    if t.contains("SoundCloud") && t.starts_with("Stream ") {
        let d = t.trim_start_matches("Stream ");
        let d = d.split(" | ").next().unwrap_or(d);
        let (track, artist) = d.split_once(" by ").map(|(a, b)| (a.to_string(), b.to_string()))?;
        return Some(act("listening", "SoundCloud", &track, &artist));
    }
    None
}

/// Turn the visible windows into at most one game plus one
/// listening/watching activity. Foreground windows win ties.
pub fn classify(windows: &[WindowInfo]) -> Vec<Activity> {
    let mut ordered: Vec<&WindowInfo> = windows.iter().collect();
    ordered.sort_by_key(|w| !w.foreground);

    let mut game: Option<Activity> = None;
    let mut media: Option<Activity> = None;
    let mut watch: Option<Activity> = None;

    for w in ordered {
        let exe = exe_name(&w.exe);
        if game.is_none() {
            // Minecraft Java runs as javaw.exe; recognise it by its title.
            if (exe == "javaw.exe" || exe == "java.exe") && w.title.starts_with("Minecraft") {
                game = Some(Activity { kind: "playing".into(), name: "Minecraft".into(), details: String::new(), state: String::new() });
                continue;
            }
            if let Some(name) = game_from_path(&w.exe) {
                game = Some(Activity { kind: "playing".into(), name, details: String::new(), state: String::new() });
                continue;
            }
        }
        if media.is_none() {
            if let Some(a) = music_app(&exe, &w.title) {
                media = Some(a);
                continue;
            }
        }
        if watch.is_none() {
            if let Some(a) = video_player(&exe, &w.title) {
                watch = Some(a);
                continue;
            }
            if BROWSERS.contains(&exe.as_str()) {
                if let Some(a) = browser_tab(&w.title) {
                    if a.kind == "listening" {
                        if media.is_none() { media = Some(a); }
                    } else {
                        watch = Some(a);
                    }
                }
            }
        }
    }

    let mut out = Vec::new();
    out.extend(game);
    // Music beats video for the second slot (Discord shows Spotify the same way).
    if let Some(m) = media { out.push(m); } else if let Some(v) = watch { out.push(v); }
    out
}

/// Visible top-level windows right now (Windows only; empty elsewhere).
pub fn snapshot() -> Vec<WindowInfo> {
    #[cfg(any(windows, aqua_wincheck))]
    {
        win::enumerate()
    }
    #[cfg(all(target_os = "linux", not(aqua_wincheck)))]
    {
        linux::processes()
    }
    #[cfg(not(any(windows, aqua_wincheck, target_os = "linux")))]
    {
        Vec::new()
    }
}

/// Everything detected right now: games / players from `snapshot`, plus —
/// on Linux — what any MPRIS player reports (Spotify, a browser tab playing
/// Spotify Web / YouTube / SoundCloud, mpv, …), which beats window titles.
pub async fn detect() -> Vec<Activity> {
    let mut acts = tokio::task::spawn_blocking(|| classify(&snapshot())).await.unwrap_or_default();
    #[cfg(target_os = "linux")]
    if let Some(m) = linux::mpris_now_playing().await {
        acts.retain(|a| a.kind == "playing");
        acts.push(m);
    }
    acts
}

/// What an MPRIS player reports, turned into an activity.
pub fn from_player(identity: &str, bus_name: &str, title: &str, artists: &[String], url: &str) -> Option<Activity> {
    let title = title.trim();
    if title.is_empty() {
        return None;
    }
    let artist = artists.iter().map(|a| a.trim()).filter(|a| !a.is_empty()).collect::<Vec<_>>().join(", ");
    let host = url.split("://").nth(1).unwrap_or("").split('/').next().unwrap_or("").trim_start_matches("www.").to_ascii_lowercase();
    let bus = bus_name.to_ascii_lowercase();
    let act = |kind: &str, name: &str, details: &str, state: &str| Activity {
        kind: kind.into(), name: name.into(), details: details.trim().into(), state: state.trim().into(),
    };
    let service = match host.as_str() {
        "open.spotify.com" => Some(("listening", "Spotify")),
        "music.youtube.com" => Some(("listening", "YouTube Music")),
        "youtube.com" | "m.youtube.com" | "youtu.be" => Some(("watching", "YouTube")),
        "twitch.tv" => Some(("watching", "Twitch")),
        "soundcloud.com" => Some(("listening", "SoundCloud")),
        "music.yandex.ru" | "music.yandex.com" => Some(("listening", "Яндекс Музыка")),
        "vk.com" | "vk.ru" => Some(("listening", "VK Музыка")),
        "netflix.com" => Some(("watching", "Netflix")),
        _ => None,
    };
    if let Some((kind, name)) = service {
        return Some(act(kind, name, title, &artist));
    }
    if bus.contains("spotify") {
        return Some(act("listening", "Spotify", title, &artist));
    }
    let id = if identity.trim().is_empty() { bus_name.rsplit('.').next().unwrap_or("Player") } else { identity.trim() };
    let video_players = ["mpv", "vlc", "celluloid", "totem", "haruna", "smplayer", "mplayer", "kodi"];
    if video_players.iter().any(|v| bus.contains(v)) && artist.is_empty() {
        return Some(act("watching", id, title, ""));
    }
    // Browsers without a known site, music players: with an artist it's music.
    if !artist.is_empty() {
        return Some(act("listening", id, title, &artist));
    }
    let browsers = ["firefox", "chromium", "chrome", "brave", "vivaldi", "opera", "edge", "librewolf", "zen", "floorp", "yandex"];
    if browsers.iter().any(|b| bus.contains(b)) {
        return Some(act("watching", id, title, ""));
    }
    Some(act("listening", id, title, ""))
}

#[cfg(target_os = "linux")]
pub(crate) mod linux {
    use super::{from_player, Activity, WindowInfo};
    use std::collections::HashMap;
    use zbus::zvariant::{OwnedValue, Value};

    /// The user's processes as "windows" without titles — enough for the
    /// game rules (Steam `steamapps/common/<Game>`, Proton / Wine `.exe`
    /// command lines, known exe names, Minecraft's Java main class).
    pub fn processes() -> Vec<WindowInfo> {
        let mut out = Vec::new();
        let Ok(rd) = std::fs::read_dir("/proc") else { return out };
        for e in rd.flatten() {
            let name = e.file_name();
            let Some(pid) = name.to_str().filter(|n| n.bytes().all(|b| b.is_ascii_digit())) else { continue };
            let dir = std::path::Path::new("/proc").join(pid);
            let exe = std::fs::read_link(dir.join("exe")).map(|p| p.display().to_string()).unwrap_or_default();
            if exe.is_empty() {
                continue; // not ours / kernel thread
            }
            let cmd = std::fs::read(dir.join("cmdline")).unwrap_or_default();
            let args: Vec<String> = cmd.split(|b| *b == 0).filter(|a| !a.is_empty()).map(|a| String::from_utf8_lossy(a).into_owned()).collect();
            // Wine / Proton: the Windows path of the game is in argv.
            if let Some(win) = args.iter().find(|a| a.to_ascii_lowercase().ends_with(".exe") && (a.contains('\\') || a.contains('/'))) {
                out.push(WindowInfo { exe: win.clone(), title: String::new(), foreground: false });
                continue;
            }
            if args.iter().any(|a| a.starts_with("net.minecraft") || a.contains("minecraft/versions")) {
                out.push(WindowInfo { exe: "javaw.exe".into(), title: "Minecraft".into(), foreground: false });
                continue;
            }
            out.push(WindowInfo { exe, title: String::new(), foreground: false });
        }
        out
    }

    fn text(v: Option<&OwnedValue>) -> String {
        match v.map(|v| &**v) {
            Some(Value::Str(s)) => s.to_string(),
            Some(Value::ObjectPath(p)) => p.to_string(),
            _ => String::new(),
        }
    }
    fn texts(v: Option<&OwnedValue>) -> Vec<String> {
        match v.map(|v| &**v) {
            Some(Value::Array(a)) => a.iter().filter_map(|x| if let Value::Str(s) = x { Some(s.to_string()) } else { None }).collect(),
            Some(Value::Str(s)) => vec![s.to_string()],
            _ => Vec::new(),
        }
    }

    /// The first MPRIS player that is *playing*, as an activity.
    pub async fn mpris_now_playing() -> Option<Activity> {
        let run = async {
            let conn = zbus::Connection::session().await.ok()?;
            let dbus = zbus::fdo::DBusProxy::new(&conn).await.ok()?;
            let names = dbus.list_names().await.ok()?;
            for n in names.iter().filter(|n| n.as_str().starts_with("org.mpris.MediaPlayer2.")) {
                let props = |iface: &'static str| {
                    let conn = conn.clone();
                    let n = n.to_string();
                    async move {
                        let p = zbus::fdo::PropertiesProxy::builder(&conn)
                            .destination(n).ok()?
                            .path("/org/mpris/MediaPlayer2").ok()?
                            .build().await.ok()?;
                        p.get_all(zbus::names::InterfaceName::from_static_str(iface).ok()?).await.ok()
                    }
                };
                let Some(player) = props("org.mpris.MediaPlayer2.Player").await else { continue };
                if text(player.get("PlaybackStatus")) != "Playing" {
                    continue;
                }
                let meta: HashMap<String, OwnedValue> = player
                    .get("Metadata")
                    .and_then(|m| HashMap::<String, OwnedValue>::try_from(m.try_clone().ok()?).ok())
                    .unwrap_or_default();
                let identity = props("org.mpris.MediaPlayer2").await.map(|r| text(r.get("Identity"))).unwrap_or_default();
                let found = from_player(
                    &identity,
                    n.as_str(),
                    &text(meta.get("xesam:title")),
                    &texts(meta.get("xesam:artist")),
                    &text(meta.get("xesam:url")),
                );
                if found.is_some() {
                    return found;
                }
            }
            None
        };
        tokio::time::timeout(std::time::Duration::from_secs(3), run).await.ok().flatten()
    }
}

#[cfg(any(windows, aqua_wincheck))]
#[allow(clippy::upper_case_acronyms)]
mod win {
    use super::WindowInfo;
    use std::collections::HashMap;
    use std::ffi::c_void;

    type HWND = *mut c_void;
    type HANDLE = *mut c_void;
    type BOOL = i32;
    type LPARAM = isize;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;

    #[cfg_attr(windows, link(name = "user32"))]
    extern "system" {
        fn EnumWindows(cb: Option<unsafe extern "system" fn(HWND, LPARAM) -> BOOL>, lparam: LPARAM) -> BOOL;
        fn IsWindowVisible(hwnd: HWND) -> BOOL;
        fn GetWindowTextLengthW(hwnd: HWND) -> i32;
        fn GetWindowTextW(hwnd: HWND, buf: *mut u16, max: i32) -> i32;
        fn GetWindowThreadProcessId(hwnd: HWND, pid: *mut u32) -> u32;
        fn GetForegroundWindow() -> HWND;
    }

    #[cfg_attr(windows, link(name = "kernel32"))]
    extern "system" {
        fn OpenProcess(access: u32, inherit: BOOL, pid: u32) -> HANDLE;
        fn QueryFullProcessImageNameW(h: HANDLE, flags: u32, buf: *mut u16, size: *mut u32) -> BOOL;
        fn CloseHandle(h: HANDLE) -> BOOL;
    }

    unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let list = &mut *(lparam as *mut Vec<HWND>);
        list.push(hwnd);
        1
    }

    fn window_title(hwnd: HWND) -> String {
        unsafe {
            let len = GetWindowTextLengthW(hwnd);
            if len <= 0 {
                return String::new();
            }
            let mut buf = vec![0u16; len as usize + 1];
            let n = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
            String::from_utf16_lossy(&buf[..n.max(0) as usize])
        }
    }

    fn image_path(pid: u32) -> Option<String> {
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() {
                return None;
            }
            let mut buf = vec![0u16; 1024];
            let mut size = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut size);
            CloseHandle(h);
            if ok == 0 {
                return None;
            }
            Some(String::from_utf16_lossy(&buf[..size as usize]))
        }
    }

    pub fn enumerate() -> Vec<WindowInfo> {
        let mut hwnds: Vec<HWND> = Vec::new();
        unsafe {
            EnumWindows(Some(collect), &mut hwnds as *mut Vec<HWND> as LPARAM);
        }
        let fg = unsafe { GetForegroundWindow() };
        let mut exe_cache: HashMap<u32, Option<String>> = HashMap::new();
        let mut out = Vec::new();
        for hwnd in hwnds {
            if unsafe { IsWindowVisible(hwnd) } == 0 {
                continue;
            }
            let title = window_title(hwnd);
            if title.trim().is_empty() {
                continue;
            }
            let mut pid = 0u32;
            unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
            if pid == 0 {
                continue;
            }
            let exe = exe_cache.entry(pid).or_insert_with(|| image_path(pid)).clone();
            let Some(exe) = exe else { continue };
            out.push(WindowInfo { exe, title, foreground: hwnd == fg });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn mpris_players() {
        let a = from_player("Mozilla Firefox", "org.mpris.MediaPlayer2.firefox.instance_1_23", "self-hacking", &["Ling tosite sigure".into()], "https://open.spotify.com/track/1");
        assert_eq!(a.unwrap(), Activity { kind: "listening".into(), name: "Spotify".into(), details: "self-hacking".into(), state: "Ling tosite sigure".into() });
        let a = from_player("Mozilla Firefox", "org.mpris.MediaPlayer2.firefox.instance_1_23", "Lofi stream", &[], "https://www.youtube.com/watch?v=x").unwrap();
        assert_eq!((a.kind.as_str(), a.name.as_str()), ("watching", "YouTube"));
        let a = from_player("Spotify", "org.mpris.MediaPlayer2.spotify", "Numb", &["Linkin Park".into()], "").unwrap();
        assert_eq!((a.name.as_str(), a.state.as_str()), ("Spotify", "Linkin Park"));
        let a = from_player("mpv", "org.mpris.MediaPlayer2.mpv", "movie.mkv", &[], "file:///x/movie.mkv").unwrap();
        assert_eq!(a.kind, "watching");
        assert!(from_player("x", "org.mpris.MediaPlayer2.x", "  ", &[], "").is_none());
    }

    #[test]
    fn linux_and_wine_game_paths() {
        let w = |exe: &str| WindowInfo { exe: exe.into(), title: String::new(), foreground: false };
        let a = classify(&[w("/home/u/.local/share/Steam/steamapps/common/Counter-Strike Global Offensive/game/bin/linuxsteamrt64/cs2")]);
        assert_eq!(a[0].name, "Counter-Strike 2");
        let a = classify(&[w("Z:\\home\\u\\.steam\\steam\\steamapps\\common\\ELDEN RING\\Game\\eldenring.exe")]);
        assert_eq!(a[0].name, "ELDEN RING");
        let a = classify(&[w("/home/u/.local/share/Steam/steamapps/common/Hades II/Hades2")]);
        assert!(classify(&[w("/home/u/.local/share/Steam/steamapps/common/Proton 9.0/files/bin/wineserver")]).is_empty());
        assert!(classify(&[w("/home/u/.local/share/Steam/steamapps/common/SteamLinuxRuntime_sniper/pressure-vessel/bin/pv-adverb")]).is_empty());
        assert_eq!(a[0].name, "Hades II");
    }

    use super::*;

    fn w(exe: &str, title: &str) -> WindowInfo {
        WindowInfo { exe: exe.into(), title: title.into(), foreground: false }
    }

    #[test]
    fn games_from_known_exes_and_store_folders() {
        let a = classify(&[w(r"C:\Program Files (x86)\Steam\steamapps\common\Counter-Strike Global Offensive\game\bin\win64\cs2.exe", "Counter-Strike 2")]);
        assert_eq!(a[0], Activity { kind: "playing".into(), name: "Counter-Strike 2".into(), details: "".into(), state: "".into() });
        let a = classify(&[w(r"D:\SteamLibrary\steamapps\common\Lethal Company\Lethal Company.exe", "Lethal Company")]);
        assert_eq!(a[0].name, "Lethal Company");
        let a = classify(&[w(r"D:\SteamLibrary\steamapps\common\Hades II\Ship\Hades2.exe", "Hades II")]);
        assert_eq!(a[0].name, "Hades II");
        let a = classify(&[w(r"C:\Program Files\Epic Games\Fortnite\FortniteGame\Binaries\Win64\FortniteClient-Win64-Shipping.exe", "Fortnite  ")]);
        assert_eq!(a[0].name, "Fortnite");
        let a = classify(&[w(r"C:\Program Files\Java\bin\javaw.exe", "Minecraft* 1.21.1 - Singleplayer")]);
        assert_eq!(a[0].name, "Minecraft");
        // Tools and launchers are not games.
        assert!(classify(&[w(r"C:\Steam\steamapps\common\wallpaper_engine\wallpaper64.exe", "Wallpaper Engine")]).is_empty());
        assert!(classify(&[w(r"C:\Program Files (x86)\Epic Games\Launcher\Portal\Binaries\Win64\EpicGamesLauncher.exe", "Epic Games Launcher")]).is_empty());
        assert!(classify(&[w(r"C:\Steam\steamapps\common\Rust\UnityCrashHandler64.exe", "x")]).is_empty());
        assert!(classify(&[w(r"C:\Windows\explorer.exe", "Downloads")]).is_empty());
    }

    #[test]
    fn music_and_video() {
        let a = classify(&[w(r"C:\Users\v\AppData\Roaming\Spotify\Spotify.exe", "Daft Punk - Harder, Better, Faster, Stronger")]);
        assert_eq!(a[0].kind, "listening");
        assert_eq!(a[0].name, "Spotify");
        assert_eq!(a[0].details, "Harder, Better, Faster, Stronger");
        assert_eq!(a[0].state, "Daft Punk");
        // Paused Spotify shows its product name → nothing.
        assert!(classify(&[w(r"C:\x\Spotify.exe", "Spotify Premium")]).is_empty());

        let a = classify(&[w(r"C:\Program Files\Google\Chrome\Application\chrome.exe", "(3) Lofi hip hop radio - YouTube - Google Chrome")]);
        assert_eq!((a[0].kind.as_str(), a[0].name.as_str(), a[0].details.as_str()), ("watching", "YouTube", "Lofi hip hop radio"));
        let a = classify(&[w(r"C:\x\firefox.exe", "xQc - Twitch — Mozilla Firefox")]);
        assert_eq!(a[0].name, "Twitch");
        let a = classify(&[w(r"C:\x\vlc.exe", "movie.mkv - VLC media player")]);
        assert_eq!((a[0].name.as_str(), a[0].details.as_str()), ("VLC", "movie.mkv"));
        let a = classify(&[w(r"C:\x\browser.exe", "Группа крови — Кино. Слушать онлайн на Яндекс Музыке - Яндекс Браузер")]);
        assert_eq!((a[0].kind.as_str(), a[0].name.as_str()), ("listening", "Яндекс Музыка"));
        assert_eq!((a[0].details.as_str(), a[0].state.as_str()), ("Группа крови", "Кино"));
        // Ordinary browsing isn't an activity.
        assert!(classify(&[w(r"C:\x\chrome.exe", "Inbox - Gmail - Google Chrome")]).is_empty());
    }

    #[test]
    fn game_plus_music_and_priorities() {
        let a = classify(&[
            w(r"C:\x\chrome.exe", "Some video - YouTube - Google Chrome"),
            w(r"C:\x\Spotify.exe", "Artist - Song"),
            w(r"C:\Steam\steamapps\common\dota 2 beta\game\bin\win64\dota2.exe", "Dota 2"),
        ]);
        assert_eq!(a.len(), 2);
        assert_eq!(a[0].name, "Dota 2");
        assert_eq!(a[1].name, "Spotify", "music beats video for the second slot");
    }
}

#[cfg(all(test, target_os = "linux"))]
mod live {
    /// Needs a session bus with an MPRIS player: AQUA_TEST_MPRIS=1.
    #[tokio::test]
    async fn reads_a_real_mpris_player() {
        if std::env::var_os("AQUA_TEST_MPRIS").is_none() { return; }
        let a = super::linux::mpris_now_playing().await.expect("player found");
        assert_eq!((a.name.as_str(), a.details.as_str(), a.state.as_str()), ("Spotify", "self-hacking", "Ling tosite sigure"));
    }
}
