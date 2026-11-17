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
/** Telegram's "Hide above the list": the archive row isn't shown until the
 *  list is pulled down at its top (the wheel up on a computer). Off: it sits
 *  pinned above the chats. */
export const archiveHidden = writable<boolean>((() => { try { return localStorage.getItem("aqua.archiveHidden") === "1"; } catch { return false; } })());
archiveHidden.subscribe((v) => { try { localStorage.setItem("aqua.archiveHidden", v ? "1" : "0"); } catch { /* ignore */ } });
export const setMuted = (id: string, on?: boolean) => toggleIn(mutedChats, id, on);
/** "Mark as unread" (Telegram): a dot on the chat until it's opened again. */
export const markedUnread = persistedSet("aqua.markedUnread");
export const setMarkedUnread = (id: string, on?: boolean) => toggleIn(markedUnread, id, on);
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

/** A new chat follows the others of its kind (the owner's "template"): when
 *  every other channel (or group) is archived, a new one goes to the archive
 *  too; when every other is muted, it comes muted. Chats seen once are
 *  remembered, so this happens only on arrival. */
const SEEN_KEY = "aqua.seenChats.v1";
let seenChats: Set<string> | null = null;
export function applyNewChatTemplate(chats: { id: string }[], kindOf: (id: string) => string | null) {
  if (!seenChats) {
    try { seenChats = new Set(JSON.parse(localStorage.getItem(SEEN_KEY) ?? "null") ?? []); } catch { seenChats = new Set(); }
    // First run: everything already here counts as seen.
    if (!seenChats.size) { for (const c of chats) seenChats.add(c.id); save(); return; }
  }
  const fresh = chats.filter((c) => !seenChats!.has(c.id));
  if (!fresh.length) return;
  const arch = get(archived), muted = get(mutedChats);
  for (const c of fresh) {
    const kind = kindOf(c.id);
    // A group whose card hasn't come yet: decided when it has.
    if (c.id.startsWith("grp_") && !kind) continue;
    seenChats.add(c.id);
    if (kind !== "channel" && kind !== "group" && kind !== "supergroup") continue;
    const same = chats.filter((x) => x.id !== c.id && seenChats!.has(x.id) && kindOf(x.id) === kind);
    if (same.length < 2) continue;
    if (same.every((x) => arch.has(x.id))) setArchived(c.id, true);
    if (same.every((x) => muted.has(x.id))) setMuted(c.id, true);
  }
  save();
  function save() { try { localStorage.setItem(SEEN_KEY, JSON.stringify([...seenChats!])); } catch { /* ignore */ } }
}
