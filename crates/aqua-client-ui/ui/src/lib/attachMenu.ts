/**
 * The 📎 menu: which actions sit in it right away (in their order), which
 * wait under "More", which are hidden, bots added to it, and whether phones
 * show the actions as a floating bar. Settings → Behaviour → Attach menu.
 */
import { writable } from "svelte/store";

export type AttachId = "media" | "file" | "poll" | "event" | "sealed" | "article" | "document" | "board" | "checklist" | "bluetooth";

export const ATTACH_ITEMS: { id: AttachId; label: string }[] = [
  { id: "media", label: "Photo or video" },
  { id: "file", label: "File" },
  { id: "poll", label: "Poll" },
  { id: "event", label: "Event" },
  { id: "checklist", label: "Checklist" },
  { id: "sealed", label: "Sealed message" },
  { id: "article", label: "Article" },
  { id: "document", label: "Document" },
  { id: "board", label: "Board" },
  { id: "bluetooth", label: "Bluetooth" },
];

/** Shown right away; the rest go under "More". */
const DEFAULT: AttachId[] = ["media", "file", "poll", "event"];
const KEY = "aqua.attachMenu.v1";
const isId = (x: unknown): x is AttachId => ATTACH_ITEMS.some((i) => i.id === x);

function stored<T>(key: string, fallback: T, check: (v: unknown) => T | null) {
  let v = fallback;
  try { const raw = localStorage.getItem(key); if (raw != null) v = check(JSON.parse(raw)) ?? fallback; } catch { /* storage blocked */ }
  const s = writable<T>(v);
  s.subscribe((x) => { try { localStorage.setItem(key, JSON.stringify(x)); } catch { /* ignore */ } });
  return s;
}

/** Right away, in this order. */
export const attachFront = stored<AttachId[]>(KEY, [...DEFAULT], (v) => (Array.isArray(v) ? v.filter(isId) : null));
/** Not shown at all, not even under "More". */
export const attachHidden = stored<AttachId[]>("aqua.attachHidden.v1", [], (v) => (Array.isArray(v) ? v.filter(isId) : null));
/** Phones: the actions as a floating bar under the gallery (off: inside the sheet). */
export const attachDock = stored<boolean>("aqua.attachDock.v1", true, (v) => (typeof v === "boolean" ? v : null));

/** A bot's mini app in the 📎 menu (Telegram's attachment-menu bots). */
export interface AttachBot { bot: string; title: string; url: string }
export const attachBots = stored<AttachBot[]>("aqua.attachBots.v1", [], (v) =>
  Array.isArray(v) ? (v as AttachBot[]).filter((b) => typeof b?.bot === "string" && typeof b?.url === "string" && /^https:\/\//.test(b.url)) : null);

export function toggleAttachFront(id: AttachId) {
  attachFront.update((l) => (l.includes(id) ? l.filter((x) => x !== id) : [...l, id]));
  attachHidden.update((l) => l.filter((x) => x !== id));
}
export function toggleAttachHidden(id: AttachId) {
  attachHidden.update((l) => (l.includes(id) ? l.filter((x) => x !== id) : [...l, id]));
  attachFront.update((l) => l.filter((x) => x !== id));
}
/** Move a front action one place left (-1) or right (+1). */
export function moveAttach(id: AttachId, dir: -1 | 1) {
  attachFront.update((l) => {
    const i = l.indexOf(id), j = i + dir;
    if (i < 0 || j < 0 || j >= l.length) return l;
    const n = [...l];
    [n[i], n[j]] = [n[j], n[i]];
    return n;
  });
}
export function toggleAttachBot(b: AttachBot) {
  attachBots.update((l) => (l.some((x) => x.bot === b.bot) ? l.filter((x) => x.bot !== b.bot) : [...l, b]));
}
export function resetAttachMenu() { attachFront.set([...DEFAULT]); attachHidden.set([]); attachDock.set(true); }
