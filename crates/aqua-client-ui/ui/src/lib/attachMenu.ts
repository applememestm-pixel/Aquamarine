/**
 * The 📎 menu: which actions sit in it right away and which wait under
 * "More". Chosen in Settings → Chat settings → Attach menu.
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

function load(): AttachId[] {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) || "null");
    if (Array.isArray(v)) return v.filter((x) => ATTACH_ITEMS.some((i) => i.id === x));
  } catch { /* storage blocked */ }
  return [...DEFAULT];
}

export const attachFront = writable<AttachId[]>(load());
attachFront.subscribe((v) => { try { localStorage.setItem(KEY, JSON.stringify(v)); } catch { /* ignore */ } });

export function toggleAttachFront(id: AttachId) {
  attachFront.update((l) => (l.includes(id) ? l.filter((x) => x !== id) : [...l, id]));
}
export function resetAttachMenu() { attachFront.set([...DEFAULT]); }
