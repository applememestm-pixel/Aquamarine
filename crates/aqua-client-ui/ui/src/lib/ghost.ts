/**
 * AyuGram-style "ghost" and history options, plus the block list. Kept on
 * this device; the Rust side gets the parts it enforces (keep deleted /
 * edit history / drop blocked people) via `set_local_prefs`.
 */
import { writable, get } from "svelte/store";
import { api } from "./tauri";

export interface GhostPrefs {
  /** Don't tell people you've read their messages. */
  noReadReceipts: boolean;
  /** Don't send "typing…". */
  noTyping: boolean;
  /** Keep messages after the sender deletes them. */
  keepDeleted: boolean;
  /** Keep previous versions of edited messages. */
  keepEdits: boolean;
  /** Mark messages you read while in ghost mode as read once you leave it. */
  sendReadLater: boolean;
}

// Deleted messages disappear by default, as in Telegram ("keep" was the
// default before and made deleting look broken on the other side).
const DEF: GhostPrefs = { noReadReceipts: false, noTyping: false, keepDeleted: false, keepEdits: true, sendReadLater: false };
try {
  if (!localStorage.getItem("aqua.ghost.del2")) {
    const raw = localStorage.getItem("aqua.ghost.v1");
    if (raw) localStorage.setItem("aqua.ghost.v1", JSON.stringify({ ...JSON.parse(raw), keepDeleted: false }));
    localStorage.setItem("aqua.ghost.del2", "1");
  }
} catch { /* ignore */ }

function persisted<T>(key: string, init: T) {
  let v = init;
  try { const raw = localStorage.getItem(key); if (raw) v = { ...init, ...JSON.parse(raw) }; } catch { /* ignore */ }
  const s = writable<T>(v);
  s.subscribe((x) => { try { localStorage.setItem(key, JSON.stringify(x)); } catch { /* ignore */ } });
  return s;
}

export const ghost = persisted<GhostPrefs>("aqua.ghost.v1", DEF);

/** Chats where I don't send read receipts (the other side never sees "read"). */
let nrInit: string[] = [];
try { nrInit = JSON.parse(localStorage.getItem("aqua.noReceipts.v1") ?? "[]"); } catch { /* ignore */ }
export const noReceiptChats = writable<Set<string>>(new Set(nrInit));
noReceiptChats.subscribe((b) => { try { localStorage.setItem("aqua.noReceipts.v1", JSON.stringify([...b])); } catch { /* ignore */ } });
export function setReceipts(chat: string, on: boolean) {
  noReceiptChats.update((s) => { const n = new Set(s); if (on) n.delete(chat); else n.add(chat); return n; });
}

let blockedInit: string[] = [];
try { blockedInit = JSON.parse(localStorage.getItem("aqua.blocked.v1") ?? "[]"); } catch { /* ignore */ }
export const blocked = writable<Set<string>>(new Set(blockedInit));
blocked.subscribe((b) => { try { localStorage.setItem("aqua.blocked.v1", JSON.stringify([...b])); } catch { /* ignore */ } });

export function isBlocked(u: string): boolean { return get(blocked).has(u); }
export function setBlocked(u: string, on: boolean) {
  blocked.update((b) => { const n = new Set(b); if (on) n.add(u); else n.delete(u); return n; });
}

/** Push the Rust-enforced parts. Call on boot and on every change. */
export function syncLocalPrefs() {
  const g = get(ghost);
  void api.setLocalPrefs(g.keepDeleted, g.keepEdits, [...get(blocked)]).catch(() => {});
}
ghost.subscribe(() => syncLocalPrefs());
blocked.subscribe(() => syncLocalPrefs());
