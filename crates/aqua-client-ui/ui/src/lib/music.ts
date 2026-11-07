/**
 * The Navidrome add-on (docs/music.md).
 *
 * Two sources: the relay's Navidrome (`server`, aqua-server music.rs — its
 * login stays on the relay) and the user's own (`own`, kept in the vault).
 * Calls go through Rust (src/music.rs); audio and covers stream through the
 * loopback media server, nothing is saved unless the user downloads a track.
 *
 * Also here: the track card (`[track:v1]{…}`), "Listening to …" in our
 * activity, and "listen together" over the E2EE control channel
 * (`[listen:v1]{…}`, 1:1 chats).
 */
import { writable, derived, get } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { settings } from "./theme";
import { info, activeTab, chats } from "./stores";
import { nowPlaying, playQueue, type Track } from "./player";
import { api } from "./tauri";
import { attachFront, attachHidden } from "./attachMenu";
import { tr } from "./i18n";

export type Src = "server" | "own";

export interface MusicStatus {
  server: { enabled: boolean; tag: string; bot?: string | null } | null;
  own: { url: string; user: string; tag: string } | null;
}

export interface Song {
  id: string;
  title: string;
  artist: string;
  album: string;
  albumId?: string;
  artistId?: string;
  duration: number;
  coverArt?: string;
  track?: number;
  year?: number;
  starred?: boolean;
  src: Src;
}

export interface Album { id: string; name: string; artist: string; artistId?: string; coverArt?: string; year?: number; songCount?: number; src: Src }
export interface Artist { id: string; name: string; albumCount?: number; coverArt?: string; src: Src }
export interface Playlist { id: string; name: string; songCount?: number; coverArt?: string; src: Src }

/** A track card in a chat. `src`/`nd` say where it plays. */
export interface TrackCard {
  id: string;
  title: string;
  artist?: string;
  album?: string;
  dur?: number;
  cover?: string;
  src: Src;
  /** Tag of the Navidrome (music.rs `tag_of`): who else can play it. */
  nd: string;
  /** "Listen together" invite: the host's @name. */
  lt?: string;
}

export const TRACK_PREFIX = "[track:v1]";
const LISTEN_PREFIX = "[listen:v1]";

// ---------------------------------------------------------------- status

export const musicOn = derived(settings, (s) => !!s.navidrome);
export const musicStatus = writable<MusicStatus>({ server: null, own: null });
export const musicSources = derived(musicStatus, (st): Src[] => [
  ...(st.server?.enabled ? (["server"] as Src[]) : []),
  ...(st.own ? (["own"] as Src[]) : []),
]);

function persisted<T>(key: string, init: T) {
  let start = init;
  try { const raw = localStorage.getItem(key); if (raw != null) start = JSON.parse(raw) as T; } catch { /* ignore */ }
  const s = writable<T>(start);
  s.subscribe((v) => { try { localStorage.setItem(key, JSON.stringify(v)); } catch { /* ignore */ } });
  return s;
}
/** The source the music page shows. */
export const musicSource = persisted<Src>("aqua.music.src", "server");

let refreshing: Promise<void> | null = null;
export function refreshMusic(): Promise<void> {
  if (refreshing) return refreshing;
  refreshing = (async () => {
    try {
      const st = await invoke<MusicStatus>("music_status");
      musicStatus.set(st);
      const srcs = get(musicSources);
      if (srcs.length && !srcs.includes(get(musicSource))) musicSource.set(srcs[0]);
    } catch { /* not connected: keep what we had */ }
  })().finally(() => { refreshing = null; });
  return refreshing;
}

// Each connection (start, another server, another account): what's here?
let seenFor = "";
info.subscribe((i) => {
  const key = i?.connected ? `${i.server_url}|${i.username}` : "";
  if (key && key !== seenFor && get(musicOn)) { seenFor = key; void refreshMusic(); }
});
musicOn.subscribe((on) => {
  if (!on) return;
  void refreshMusic();
  // The 📎 menu gets "Music" up front — once; then it's the user's to move or hide.
  let done = false;
  try { done = localStorage.getItem("aqua.music.attach") === "1"; localStorage.setItem("aqua.music.attach", "1"); } catch { /* storage blocked */ }
  if (!done && !get(attachHidden).includes("music") && !get(attachFront).includes("music")) attachFront.update((l) => [...l, "music"]);
});

/** Does this card play here, and from which source? */
export function cardSource(c: { src?: Src; nd?: string }, st = get(musicStatus)): Src | null {
  if (!c.nd) return null;
  if (st.server?.enabled && st.server.tag === c.nd) return "server";
  if (st.own && st.own.tag === c.nd) return "own";
  return null;
}

// ------------------------------------------------------------------- api

export function nd<T = any>(src: Src, method: string, params: Record<string, string | number> = {}): Promise<T> {
  const p: Record<string, string> = {};
  for (const [k, v] of Object.entries(params)) p[k] = String(v);
  return invoke<T>("music_api", { src, method, params: p });
}

const coverCache = new Map<string, Promise<string>>();
/** Loopback URL of a cover ("" without one). */
export function coverUrl(src: Src, id: string | undefined, size = 300): Promise<string> {
  if (!id) return Promise.resolve("");
  const key = `${src}|${id}|${size}`;
  let p = coverCache.get(key);
  if (!p) {
    p = invoke<string>("music_url", { src, kind: "cover", id, size }).catch(() => "");
    coverCache.set(key, p);
  }
  return p;
}

const arr = (v: any): any[] => (Array.isArray(v) ? v : v ? [v] : []);

export function toSong(x: any, src: Src): Song {
  return {
    id: String(x.id), title: x.title ?? "", artist: x.artist ?? x.displayArtist ?? "", album: x.album ?? "",
    albumId: x.albumId, artistId: x.artistId, duration: Number(x.duration ?? 0), coverArt: x.coverArt,
    track: x.track, year: x.year, starred: !!x.starred, src,
  };
}
export const toAlbum = (x: any, src: Src): Album => ({ id: String(x.id), name: x.name ?? x.title ?? "", artist: x.artist ?? "", artistId: x.artistId, coverArt: x.coverArt, year: x.year, songCount: x.songCount, src });
export const toArtist = (x: any, src: Src): Artist => ({ id: String(x.id), name: x.name ?? "", albumCount: x.albumCount, coverArt: x.coverArt, src });
export const toPlaylist = (x: any, src: Src): Playlist => ({ id: String(x.id), name: x.name ?? "", songCount: x.songCount, coverArt: x.coverArt, src });

export async function search(src: Src, q: string) {
  const r = await nd(src, "search3", { query: q, songCount: 40, albumCount: 12, artistCount: 8 });
  const s = r?.searchResult3 ?? {};
  return {
    songs: arr(s.song).map((x) => toSong(x, src)),
    albums: arr(s.album).map((x) => toAlbum(x, src)),
    artists: arr(s.artist).map((x) => toArtist(x, src)),
  };
}
export async function albumList(src: Src, type: "newest" | "frequent" | "recent" | "random" | "starred" | "alphabeticalByName", size = 20) {
  const r = await nd(src, "getAlbumList2", { type, size });
  return arr(r?.albumList2?.album).map((x) => toAlbum(x, src));
}
export async function album(src: Src, id: string) {
  const r = await nd(src, "getAlbum", { id });
  const a = r?.album ?? {};
  return { album: toAlbum(a, src), songs: arr(a.song).map((x) => toSong(x, src)) };
}
export async function artist(src: Src, id: string) {
  const r = await nd(src, "getArtist", { id });
  const a = r?.artist ?? {};
  return { artist: toArtist(a, src), albums: arr(a.album).map((x) => toAlbum(x, src)) };
}
export async function playlists(src: Src) {
  const r = await nd(src, "getPlaylists");
  return arr(r?.playlists?.playlist).map((x) => toPlaylist(x, src));
}
export async function playlist(src: Src, id: string) {
  const r = await nd(src, "getPlaylist", { id });
  const p = r?.playlist ?? {};
  return { playlist: toPlaylist(p, src), songs: arr(p.entry).map((x) => toSong(x, src)) };
}
export async function starred(src: Src) {
  const r = await nd(src, "getStarred2");
  return arr(r?.starred2?.song).map((x) => toSong(x, src));
}
export async function randomSongs(src: Src, size = 50) {
  const r = await nd(src, "getRandomSongs", { size });
  return arr(r?.randomSongs?.song).map((x) => toSong(x, src));
}

// ---------------------------------------------------------------- player

/** The player's Track for a song: plays through `nd:<src>:<id>` (media.ts). */
export function songTrack(s: Song, cover = ""): Track {
  return {
    src: `nd:${s.src}:${s.id}`,
    title: s.title,
    sub: [s.artist, s.album].filter(Boolean).join(" — "),
    cover: cover || undefined,
    nd: { src: s.src, id: s.id, artist: s.artist, album: s.album, dur: s.duration, coverArt: s.coverArt },
  };
}

export async function playSongs(list: Song[], index = 0) {
  if (!list.length) return;
  const covers = await Promise.all(list.map((s) => coverUrl(s.src, s.coverArt, 600)));
  playQueue(list.map((s, i) => songTrack(s, covers[i])), index);
}

export async function shuffleSongs(list: Song[]) {
  const a = list.slice();
  for (let i = a.length - 1; i > 0; i--) { const j = Math.floor(Math.random() * (i + 1)); [a[i], a[j]] = [a[j], a[i]]; }
  await playSongs(a, 0);
}

export async function downloadSong(s: { src: Src; id: string; title: string; artist?: string }): Promise<string> {
  const name = s.artist ? `${s.artist} — ${s.title}` : s.title;
  return invoke<string>("music_save", { src: s.src, id: s.id, name });
}

/** Opened from a chat's paperclip: picking a track sends it there. */
export const musicSendTo = writable<string | null>(null);

/** Open the music page (phones: the tab; desktop: the side column). */
export function openMusic(sendTo: string | null = null) {
  if (!get(musicOn)) settings.update((s) => ({ ...s, navidrome: true }));
  musicSendTo.set(sendTo);
  activeTab.set("music");
}

// ---------------------------------------------------------- track cards

export function cardOf(s: Song, lt?: string): TrackCard {
  const st = get(musicStatus);
  const tag = s.src === "server" ? st.server?.tag ?? "" : st.own?.tag ?? "";
  const c: TrackCard = { id: s.id, title: s.title, artist: s.artist, album: s.album, dur: s.duration, cover: s.coverArt, src: s.src, nd: tag };
  if (lt) c.lt = lt;
  return c;
}

export function parseTrack(text: string): TrackCard | null {
  if (!text.startsWith(TRACK_PREFIX)) return null;
  try {
    const v = JSON.parse(text.slice(TRACK_PREFIX.length));
    if (!v || typeof v.id !== "string" || typeof v.title !== "string") return null;
    return { ...v, src: v.src === "own" ? "own" : "server", nd: String(v.nd ?? "") };
  } catch { return null; }
}

export function cardSong(c: TrackCard, src: Src): Song {
  return { id: c.id, title: c.title, artist: c.artist ?? "", album: c.album ?? "", duration: c.dur ?? 0, coverArt: c.cover, src };
}

export async function sendCard(chat: string, s: Song, lt?: string) {
  const text = TRACK_PREFIX + JSON.stringify(cardOf(s, lt));
  if (chat.startsWith("grp_")) await api.groupSend(chat, text);
  else await api.sendMessage(chat, text);
  try { chats.set(await api.listChats()); } catch { /* next refresh */ }
}

// ------------------------------------------------- "Listening to …" + scrobble

/** What we play from Navidrome, as an activity entry (presence.ts adds it). */
export const musicActivity = writable<{ kind: "listening"; name: string; details: string; state: string }[]>([]);

let started: { src: Src; id: string; at: number; dur: number } | null = null;
nowPlaying.subscribe((t) => {
  // Scrobble the one that just stopped, if it played long enough (own source:
  // the relay's Navidrome is read-only for us).
  if (started && started.src === "own") {
    const played = (Date.now() - started.at) / 1000;
    if (played >= Math.min(240, Math.max(30, started.dur / 2))) {
      void nd("own", "scrobble", { id: started.id, submission: "true" }).catch(() => {});
    }
  }
  const n = t?.nd;
  started = n ? { src: n.src, id: n.id, at: Date.now(), dur: n.dur ?? 0 } : null;
  if (n?.src === "own") void nd("own", "scrobble", { id: n.id, submission: "false" }).catch(() => {});
  const share = get(settings).musicShare !== false;
  musicActivity.set(n && share ? [{ kind: "listening", name: "Navidrome", details: t!.title, state: n.artist ?? "" }] : []);
  pushListen();
});

// --------------------------------------------------------- listen together

/** We host "listen together" in this 1:1 chat. */
export const hosting = writable<string | null>(null);
/** We follow `host` in `chat`. */
export const following = writable<{ chat: string; host: string } | null>(null);

let audioEl: HTMLAudioElement | null = null;
/** TopPlayer hands over its element (position, pause, seek). */
export function bindAudio(a: HTMLAudioElement | null) {
  if (audioEl === a) return;
  audioEl = a;
  if (a) for (const ev of ["play", "pause", "seeked"]) a.addEventListener(ev, () => pushListen());
}

function stateMsg(): string | null {
  const t = get(nowPlaying);
  if (!t?.nd) return null;
  const s = cardSong({ id: t.nd.id, title: t.title, artist: t.nd.artist, album: t.nd.album, dur: t.nd.dur, cover: t.nd.coverArt, src: t.nd.src, nd: "" }, t.nd.src);
  return LISTEN_PREFIX + JSON.stringify({
    t: "state", track: cardOf(s), pos: audioEl?.currentTime ?? 0, playing: audioEl ? !audioEl.paused : true, sent: Date.now(),
  });
}

let pushTimer: ReturnType<typeof setTimeout> | null = null;
function pushListen() {
  const chat = get(hosting);
  if (!chat) return;
  if (pushTimer) clearTimeout(pushTimer);
  pushTimer = setTimeout(() => {
    const m = stateMsg();
    if (m) void api.sendControl(chat, m).catch(() => {});
  }, 250);
}
// Drift fix and late joiners: the state every 10 s while hosting.
setInterval(() => { if (get(hosting)) pushListen(); }, 10_000);

export async function startListening(chat: string) {
  const t = get(nowPlaying);
  if (!t?.nd) throw new Error(tr("Play something from Navidrome first"));
  if (chat.startsWith("grp_")) throw new Error(tr("Listening together works in one-to-one chats"));
  hosting.set(chat);
  const me = get(info)?.username ?? "";
  await sendCard(chat, cardSong({ id: t.nd.id, title: t.title, artist: t.nd.artist, album: t.nd.album, dur: t.nd.dur, cover: t.nd.coverArt, src: t.nd.src, nd: "" }, t.nd.src), me);
  pushListen();
}
export function stopHosting() {
  const chat = get(hosting);
  if (chat) void api.sendControl(chat, LISTEN_PREFIX + JSON.stringify({ t: "end" })).catch(() => {});
  hosting.set(null);
}
export function joinListening(chat: string, host: string) {
  following.set({ chat, host });
  void api.sendControl(chat, LISTEN_PREFIX + JSON.stringify({ t: "hello" })).catch(() => {});
}
export function leaveListening() {
  const f = get(following);
  if (f) void api.sendControl(f.chat, LISTEN_PREFIX + JSON.stringify({ t: "bye" })).catch(() => {});
  following.set(null);
}

async function applyState(d: any) {
  const card = d.track as TrackCard | undefined;
  if (!card) return;
  const src = cardSource(card);
  if (!src) { following.set(null); throw new Error(tr("You don't have access to this Navidrome")); }
  const cur = get(nowPlaying);
  const lag = Math.max(0, (Date.now() - Number(d.sent || Date.now())) / 1000);
  const pos = Number(d.pos || 0) + (d.playing ? lag : 0);
  if (!cur?.nd || cur.nd.id !== card.id) {
    await playSongs([cardSong(card, src)], 0);
    // The element loads the new source: seek once it can.
    const seekLater = () => { if (audioEl) { try { audioEl.currentTime = pos; } catch { /* ignore */ } if (!d.playing) audioEl.pause(); } };
    setTimeout(seekLater, 900);
    return;
  }
  if (!audioEl) return;
  if (Math.abs(audioEl.currentTime - pos) > 1.5) { try { audioEl.currentTime = pos; } catch { /* ignore */ } }
  if (d.playing && audioEl.paused) void audioEl.play().catch(() => {});
  if (!d.playing && !audioEl.paused) audioEl.pause();
}

void listen<{ from: string; data: any }>("aqua://listen", (e) => {
  const { from, data } = e.payload;
  if (!data || typeof data !== "object") return;
  // Host side: someone joined — answer with the state right away.
  if (get(hosting) === from && data.t === "hello") { pushListen(); return; }
  const f = get(following);
  if (!f || f.host !== from) return;
  if (data.t === "end") { following.set(null); return; }
  if (data.t === "state") void applyState(data).catch((err) => import("./components/Toast.svelte").then((m) => m.showToast(String(err?.message ?? err))));
});

/** Another account: whatever we hosted or followed belonged to the old one. */
let lastUser = "";
info.subscribe((i) => {
  const u = i?.username ?? "";
  if (lastUser && u && u !== lastUser) { hosting.set(null); following.set(null); }
  if (u) lastUser = u;
});
