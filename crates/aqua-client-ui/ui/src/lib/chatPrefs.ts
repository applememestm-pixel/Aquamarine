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

/** A new message arrived in `id`: unmuted archived chats come back. */
export function onIncomingForArchive(id: string) {
  if (get(archived).has(id) && !get(mutedChats).has(id)) setArchived(id, false);
}
