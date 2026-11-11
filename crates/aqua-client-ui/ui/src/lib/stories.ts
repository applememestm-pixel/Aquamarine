/**
 * Stories (Telegram): a photo or a short video that lives 24 hours.
 *
 * The file is encrypted and put on the relay like any attachment (only a
 * key opens it); the reference — CID + key — travels to the people you
 * chat with inside the E2EE presence message (`st`, presence.ts), so the
 * server never learns who posted what to whom. Receivers fetch and decrypt
 * it on first view (fetch_attachment). Kept on this device: my own list
 * and which stories I've seen.
 */
import { writable, readable, get } from "svelte/store";
import { api } from "./tauri";
import type { ArticleMedia } from "./types";

export const STORY_TTL_MS = 24 * 60 * 60 * 1000;
const MAX_STORIES = 10;

export interface Story {
  id: string;
  media: ArticleMedia;
  /** Posted at (ms). */
  at: number;
  caption?: string;
}

function persisted<T>(key: string, init: T) {
  let start = init;
  try { const raw = localStorage.getItem(key); if (raw != null) start = JSON.parse(raw) as T; } catch { /* storage blocked */ }
  const s = writable<T>(start);
  s.subscribe((v) => { try { localStorage.setItem(key, JSON.stringify(v)); } catch { /* ignore */ } });
  return s;
}

const fresh = (list: Story[], now = Date.now()) => list.filter((s) => now - s.at < STORY_TTL_MS);

/** My stories (newest last). */
export const myStories = persisted<Story[]>("aqua.myStories.v1", []);
myStories.update((l) => (Array.isArray(l) ? fresh(l) : []));
/** Ids of stories I've watched. */
export const seenStories = persisted<string[]>("aqua.seenStories.v1", []);

/** A story reference as it travels in presence — checked before use. */
export function saneStories(x: unknown, now = Date.now()): Story[] {
  if (!Array.isArray(x)) return [];
  const out: Story[] = [];
  for (const v of x.slice(0, MAX_STORIES)) {
    const s = v as Story;
    const m = s?.media;
    if (typeof s?.id !== "string" || !/^[a-z0-9-]{4,40}$/i.test(s.id)) continue;
    if (typeof s.at !== "number" || now - s.at >= STORY_TTL_MS || s.at - now > 5 * 60_000) continue;
    if (!m || typeof m.cid !== "string" || typeof m.key_b64 !== "string" || typeof m.nonce_b64 !== "string") continue;
    if (m.kind !== "image" && m.kind !== "video") continue;
    out.push({ id: s.id, at: s.at, caption: typeof s.caption === "string" ? s.caption.slice(0, 200) : undefined,
      media: { kind: m.kind, cid: m.cid, key_b64: m.key_b64, nonce_b64: m.nonce_b64, name: String(m.name ?? "story").slice(0, 80), size: Number(m.size) || 0, mime: typeof m.mime === "string" ? m.mime : null } });
  }
  return out;
}

/** What goes into my presence (only live ones). */
export function storiesForPresence(): Story[] {
  return fresh(get(myStories)).slice(-MAX_STORIES);
}

/** Post a photo / video as a story; resolves when it's up. */
export async function postStory(path: string, caption = ""): Promise<void> {
  const media = await api.uploadAttachment("__story__", path, false);
  if (media.kind !== "image" && media.kind !== "video") throw new Error("a story is a photo or a video");
  const s: Story = { id: `s${Date.now().toString(36)}${Math.random().toString(36).slice(2, 7)}`, media, at: Date.now(), caption: caption.trim() || undefined };
  myStories.update((l) => [...fresh(l), s].slice(-MAX_STORIES));
  const { broadcastPresence } = await import("./presence");
  broadcastPresence();
}

export async function deleteStory(id: string) {
  myStories.update((l) => l.filter((s) => s.id !== id));
  const { broadcastPresence } = await import("./presence");
  broadcastPresence();
}

export function markSeen(id: string) {
  seenStories.update((l) => (l.includes(id) ? l : [...l.slice(-400), id]));
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
