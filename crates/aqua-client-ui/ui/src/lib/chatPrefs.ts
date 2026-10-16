/**
 * Per-chat preferences kept on this device: archived chats and muted chats
 * (Telegram semantics: an archived chat that isn't muted pops back out when a
 * new message arrives; a muted one stays archived and silent).
 */
import { writable, get } from "svelte/store";

function persistedSet(key: string) {
  let init: string[] = [];
  try { init = JSON.parse(localStorage.getItem(key) ?? "[]"); } catch { /* ignore */ }
  const s = writable<Set<string>>(new Set(init));
  s.subscribe((v) => { try { localStorage.setItem(key, JSON.stringify([...v])); } catch { /* ignore */ } });
  return s;
}

export const archived = persistedSet("aqua.archived");
export const mutedChats = persistedSet("aqua.mutedChats");

function toggleIn(store: typeof archived, id: string, on?: boolean) {
  store.update((s) => {
    const n = new Set(s);
    const want = on ?? !n.has(id);
    if (want) n.add(id); else n.delete(id);
    return n;
  });
}
export const setArchived = (id: string, on?: boolean) => toggleIn(archived, id, on);
export const setMuted = (id: string, on?: boolean) => toggleIn(mutedChats, id, on);
export const isMuted = (id: string) => get(mutedChats).has(id);

/** Pinned chats, in pin order (Telegram: up to 10, newest pin on top). */
function persistedList(key: string) {
  let init: string[] = [];
  try { init = JSON.parse(localStorage.getItem(key) ?? "[]"); } catch { /* ignore */ }
  const s = writable<string[]>(Array.isArray(init) ? init : []);
  s.subscribe((v) => { try { localStorage.setItem(key, JSON.stringify(v)); } catch { /* ignore */ } });
  return s;
}
export const pinnedChats = persistedList("aqua.pinnedChats");
export const MAX_PINS = 10;
/** Pin or unpin; returns false when the pin limit is reached. */
export function togglePin(id: string): boolean {
  const cur = get(pinnedChats);
  if (cur.includes(id)) { pinnedChats.set(cur.filter((x) => x !== id)); return true; }
  if (cur.length >= MAX_PINS) return false;
  pinnedChats.set([id, ...cur]);
  return true;
}

/** Move a pinned chat to position `to` among the pins. */
export function movePin(id: string, to: number) {
  const cur = get(pinnedChats).filter((x) => x !== id);
  if (!get(pinnedChats).includes(id)) return;
  cur.splice(Math.max(0, Math.min(cur.length, to)), 0, id);
  pinnedChats.set(cur);
}

/** A new message arrived in `id`: unmuted archived chats come back. */
export function onIncomingForArchive(id: string) {
  if (get(archived).has(id) && !get(mutedChats).has(id)) setArchived(id, false);
}
