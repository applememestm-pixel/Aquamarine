/**
 * Stories (Telegram): a photo or a short video that lives 6–48 hours.
 *
 * The file is encrypted and put on the relay like any attachment (only a
 * key opens it); the reference — CID + key — travels to the people you
 * chat with inside the E2EE presence message (`st`, presence.ts), so the
 * server never learns who posted what to whom. Who gets it is decided per
 * person (everyone / my contacts / close friends / chosen people), the
 * same way the other presence fields follow the privacy rules. Receivers
 * fetch and decrypt it on first view (fetch_attachment) and tell the
 * owner they've seen it (`[ctl:v1]{"t":"st_view"}`, with a reaction when
 * they tapped ❤) — unless ghost mode keeps read receipts off.
 */
import { writable, readable, get } from "svelte/store";
import { api } from "./tauri";
import type { ArticleMedia } from "./types";
import { contacts } from "./contacts";
import { ghost } from "./ghost";

/** The default life of a story, and what older builds assume for all. */
export const STORY_TTL_MS = 24 * 60 * 60 * 1000;
/** The longest a story may live (48 h, as in Telegram). */
const MAX_TTL_MS = 48 * 60 * 60 * 1000;
const MAX_STORIES = 10;

export type StoryAudience = "all" | "contacts" | "close" | "selected";

export interface Story {
  id: string;
  media: ArticleMedia;
  /** Posted at (ms). */
  at: number;
  caption?: string;
  /** Expires at (ms); absent = posted + 24 h. */
  exp?: number;
  /** Who sees it — mine only, never sent. */
  aud?: StoryAudience;
  /** The chosen people for "selected" — mine only. */
  sel?: string[];
  /** For close friends only (a green ring, as in Telegram). */
  cf?: boolean;
}

export interface StoryView { u: string; at: number; r?: string }

function persisted<T>(key: string, init: T) {
  let start = init;
  try { const raw = localStorage.getItem(key); if (raw != null) start = JSON.parse(raw) as T; } catch { /* storage blocked */ }
  const s = writable<T>(start);
  s.subscribe((v) => { try { localStorage.setItem(key, JSON.stringify(v)); } catch { /* ignore */ } });
  return s;
}

export const expiresAt = (s: Story) => (typeof s.exp === "number" ? s.exp : s.at + STORY_TTL_MS);
export const isLive = (s: Story, now = Date.now()) => now < expiresAt(s);
const fresh = (list: Story[], now = Date.now()) => list.filter((s) => isLive(s, now));

/** Stories switched off altogether: none shown, none posted, mine leave
 *  everyone's lists (Settings → Chat settings, or the strip's own menu). */
export const storiesOff = persisted<boolean>("aqua.storiesOff.v1", false);

/** My stories (newest last). */
export const myStories = persisted<Story[]>("aqua.myStories.v1", []);
myStories.update((l) => (Array.isArray(l) ? fresh(l) : []));
/** Ids of stories I've watched. */
export const seenStories = persisted<string[]>("aqua.seenStories.v1", []);
/** Close friends: who sees my "close friends" stories (a green ring). */
export const closeFriends = persisted<string[]>("aqua.closeFriends.v1", []);
/** People whose stories I've hidden (Telegram's "Hide stories"): they leave
 *  the strip and the rings, and sit at its end, dimmed. */
export const hiddenStories = persisted<string[]>("aqua.hiddenStories.v1", []);
/** Who watched my stories: story id → views (newest last). */
export const storyViews = persisted<Record<string, StoryView[]>>("aqua.storyViews.v1", {});
/** Stories I've reacted to (❤), so the heart stays red. */
export const likedStories = persisted<string[]>("aqua.likedStories.v1", []);
/** The audience and life last picked when posting (Telegram remembers it). */
export const storyDefaults = persisted<{ aud: StoryAudience; sel: string[]; hours: number }>("aqua.storyDefaults.v1", { aud: "all", sel: [], hours: 24 });

/** A story reference as it travels in presence — checked before use. */
export function saneStories(x: unknown, now = Date.now()): Story[] {
  if (!Array.isArray(x)) return [];
  const out: Story[] = [];
  for (const v of x.slice(0, MAX_STORIES)) {
    const s = v as Story;
    const m = s?.media;
    if (typeof s?.id !== "string" || !/^[a-z0-9-]{4,40}$/i.test(s.id)) continue;
    if (typeof s.at !== "number" || s.at - now > 5 * 60_000) continue;
    const exp = typeof s.exp === "number" && s.exp > s.at && s.exp - s.at <= MAX_TTL_MS ? s.exp : s.at + STORY_TTL_MS;
    if (now >= exp) continue;
    if (!m || typeof m.cid !== "string" || typeof m.key_b64 !== "string" || typeof m.nonce_b64 !== "string") continue;
    if (m.kind !== "image" && m.kind !== "video") continue;
    out.push({ id: s.id, at: s.at, exp, cf: s.cf === true || undefined, caption: typeof s.caption === "string" ? s.caption.slice(0, 200) : undefined,
      media: { kind: m.kind, cid: m.cid, key_b64: m.key_b64, nonce_b64: m.nonce_b64, name: String(m.name ?? "story").slice(0, 80), size: Number(m.size) || 0, mime: typeof m.mime === "string" ? m.mime : null } });
  }
  return out;
}

/** May `peer` see this story of mine? */
export function storyFor(s: Story, peer: string): boolean {
  switch (s.aud ?? "all") {
    case "contacts": return !!get(contacts)[peer];
    case "close": return get(closeFriends).includes(peer);
    case "selected": return (s.sel ?? []).includes(peer);
    default: return true;
  }
}

/** What goes into my presence for `peer` (only live ones it may see). */
export function storiesForPresence(peer = ""): Story[] {
  if (get(storiesOff)) return [];
  return fresh(get(myStories))
    .filter((s) => !peer || storyFor(s, peer))
    .slice(-MAX_STORIES)
    // Only what the other side needs: no audience lists.
    .map((s) => ({ id: s.id, media: s.media, at: s.at, exp: expiresAt(s), caption: s.caption, cf: s.aud === "close" || undefined }));
}

export interface PostOptions { caption?: string; aud?: StoryAudience; sel?: string[]; hours?: number }

/** Post a photo / video as a story; resolves when it's up. */
export async function postStory(path: string, opts: PostOptions = {}): Promise<void> {
  const media = await api.uploadAttachment("__story__", path, false);
  if (media.kind !== "image" && media.kind !== "video") throw new Error("a story is a photo or a video");
  const at = Date.now();
  const hours = [6, 12, 24, 48].includes(Number(opts.hours)) ? Number(opts.hours) : 24;
  const s: Story = {
    id: `s${at.toString(36)}${Math.random().toString(36).slice(2, 7)}`, media, at, exp: at + hours * 3600_000,
    caption: opts.caption?.trim() || undefined, aud: opts.aud ?? "all", sel: opts.aud === "selected" ? [...(opts.sel ?? [])] : undefined,
  };
  myStories.update((l) => [...fresh(l), s].slice(-MAX_STORIES));
  const { broadcastPresence } = await import("./presence");
  broadcastPresence();
}

export async function deleteStory(id: string) {
  myStories.update((l) => l.filter((s) => s.id !== id));
  storyViews.update((m) => { const n = { ...m }; delete n[id]; return n; });
  const { broadcastPresence } = await import("./presence");
  broadcastPresence();
}

export function markSeen(id: string) {
  seenStories.update((l) => (l.includes(id) ? l : [...l.slice(-400), id]));
}

/** Tell the owner I've watched (and maybe liked) their story. */
export function sendStoryView(owner: string, id: string, r?: string) {
  if (!owner || (get(ghost).noReadReceipts && !r)) return;
  const body: Record<string, unknown> = { t: "st_view", id };
  if (r !== undefined) body.r = r;
  void api.sendControl(owner, "[ctl:v1]" + JSON.stringify(body)).catch(() => {});
}

/** ❤ on someone's story (again = take it back). */
export function toggleLike(owner: string, id: string) {
  const on = !get(likedStories).includes(id);
  likedStories.update((l) => (on ? [...l.slice(-300), id] : l.filter((x) => x !== id)));
  sendStoryView(owner, id, on ? "❤️" : "");
}

export function toggleHidden(user: string) {
  hiddenStories.update((l) => (l.includes(user) ? l.filter((x) => x !== user) : [...l, user]));
}

/** Viewer state: whose stories, and which one. "" = mine. */
export const storyViewer = writable<{ users: string[]; user: string; index: number } | null>(null);
export function openStories(users: string[], user: string) {
  storyViewer.set({ users, user, index: 0 });
}

/** Ticks every minute so expired stories drop out of the strip. */
export const storyClock = readable(Date.now(), (set) => {
  const t = setInterval(() => set(Date.now()), 60_000);
  return () => clearInterval(t);
});

/** Views of my stories coming in (call once at start). */
let viewsStarted = false;
export function startStoryViews() {
  if (viewsStarted) return;
  viewsStarted = true;
  void import("@tauri-apps/api/event").then(({ listen }) => listen<{ from: string; chat: string; body: Record<string, unknown>; own?: boolean }>("aqua://ctl", (e) => {
    const b = e.payload?.body;
    if (!b || b.t !== "st_view" || e.payload.own || typeof b.id !== "string") return;
    const id = b.id;
    const from = String(e.payload.from ?? "");
    if (!from || !get(myStories).some((s) => s.id === id)) return;
    const r = typeof b.r === "string" ? Array.from(b.r).slice(0, 4).join("") : undefined;
    storyViews.update((m) => {
      const list = [...(m[id] ?? [])];
      const i = list.findIndex((v) => v.u === from);
      const prev = i >= 0 ? list.splice(i, 1)[0] : null;
      const v: StoryView = { u: from, at: prev?.at ?? Date.now() };
      const react = r === undefined ? prev?.r : r || undefined;
      if (react) v.r = react;
      list.push(v);
      return { ...m, [id]: list.slice(-500) };
    });
  })).catch(() => {});
}
