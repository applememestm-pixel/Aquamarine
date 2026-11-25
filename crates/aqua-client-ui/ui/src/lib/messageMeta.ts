/**
 * Per-message UI metadata kept on this device (localStorage):
 *   - reactions: emoji reactions you've placed on a message
 *   - pins:      pinned messages per chat or room (this device)
 *
 * These are device-local for now (reactions don't sync because messages
 * don't yet carry a shared cross-peer id). Pins are inherently per-view.
 */

import { writable, get } from "svelte/store";

// ---- Reactions (synced across a 1:1 chat) ----
// A reaction is keyed by a *content key* both peers compute identically:
//   `<authorUsername>:<hash(content)>`
// so the same logical message maps to the same key on both sides even
// though its local message id differs. `mine` is what this device added;
// `peer` counts what the other side added (over the [react:v1] channel).
export interface Reaction {
  mine: string[];
  peer: Record<string, number>;
}
type ReactionMap = Record<string, Reaction>;
const R_KEY = "aqua-reactions";

function loadReactions(): ReactionMap {
  try {
    const raw = JSON.parse(localStorage.getItem(R_KEY) ?? "{}");
    // Migrate the old `key -> string[]` shape.
    const out: ReactionMap = {};
    for (const [k, v] of Object.entries(raw as Record<string, unknown>)) {
      if (Array.isArray(v)) out[k] = { mine: v as string[], peer: {} };
      else if (v && typeof v === "object") out[k] = v as Reaction;
    }
    return out;
  } catch { return {}; }
}
export const reactions = writable<ReactionMap>(loadReactions());
reactions.subscribe((r) => { try { localStorage.setItem(R_KEY, JSON.stringify(r)); } catch { /* ignore */ } });

/** FNV-1a hash of a string → short hex, for the content key. */
export function hashContent(s: string): string {
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) { h ^= s.charCodeAt(i); h = Math.imul(h, 0x01000193); }
  return (h >>> 0).toString(36);
}

/** The chips to render for a key: merged mine+peer with counts. */
export function reactionChips(r: Reaction | undefined): { emoji: string; count: number; mine: boolean }[] {
  if (!r) return [];
  const emojis = new Set<string>([...r.mine, ...Object.keys(r.peer)]);
  const out: { emoji: string; count: number; mine: boolean }[] = [];
  for (const e of emojis) {
    const mine = r.mine.includes(e);
    const count = (mine ? 1 : 0) + (r.peer[e] ?? 0);
    if (count > 0) out.push({ emoji: e, count, mine });
  }
  return out;
}

/** Toggle my reaction locally. Returns `true` if it was added, `false`
 *  if removed — the caller broadcasts the change to the peer. */
export function toggleMyReaction(key: string, emoji: string): boolean {
  let added = false;
  reactions.update((r) => {
    const cur: Reaction = r[key] ?? { mine: [], peer: {} };
    const has = cur.mine.includes(emoji);
    added = !has;
    const mine = has ? cur.mine.filter((e) => e !== emoji) : [...cur.mine, emoji];
    const next: Reaction = { mine, peer: cur.peer };
    if (mine.length === 0 && Object.keys(next.peer).length === 0) {
      const { [key]: _drop, ...rest } = r; return rest;
    }
    return { ...r, [key]: next };
  });
  return added;
}

/** Apply a reaction received from the peer. */
export function applyPeerReaction(key: string, emoji: string, add: boolean) {
  reactions.update((r) => {
    const cur: Reaction = r[key] ?? { mine: [], peer: {} };
    const peer = { ...cur.peer };
    peer[emoji] = Math.max(0, (peer[emoji] ?? 0) + (add ? 1 : -1));
    if (peer[emoji] === 0) delete peer[emoji];
    const next: Reaction = { mine: cur.mine, peer };
    if (next.mine.length === 0 && Object.keys(peer).length === 0) {
      const { [key]: _drop, ...rest } = r; return rest;
    }
    return { ...r, [key]: next };
  });
}

// ---- Pins ----
// Scope → pinned message ids, newest first. The scope is the chat id, or
// "<chat>#<room>" for a room of a group: each room has its own pins (before
// 1.21.0-23 one slot per chat — pinning in room B silently replaced room A's
// pin). Old entries ("<chat>": "<id>") load as a one-pin list for the chat.
type PinMap = Record<string, string[]>;
const P_KEY = "aqua-pins";
/** Most pins kept per chat or room. */
export const MAX_PINS = 10;

function loadPins(): PinMap {
  try {
    const raw = JSON.parse(localStorage.getItem(P_KEY) ?? "{}") as Record<string, unknown>;
    const out: PinMap = {};
    for (const [k, v] of Object.entries(raw)) {
      if (typeof v === "string" && v) out[k] = [v];
      else if (Array.isArray(v)) out[k] = v.filter((x): x is string => typeof x === "string" && !!x).slice(0, MAX_PINS);
    }
    return out;
  } catch { return {}; }
}
export const pins = writable<PinMap>(loadPins());
pins.subscribe((p) => { try { localStorage.setItem(P_KEY, JSON.stringify(p)); } catch { /* ignore */ } });

/** Where pins of this chat (and room) live. */
export function pinScope(chat: string, room?: string | null): string {
  return room ? `${chat}#${room}` : chat;
}
/** Pinned ids of a scope, newest first. A room also sees the chat's old
 *  one-slot pin (the caller shows only ids found among the room's rows). */
export function pinnedIds(scope: string, map: PinMap = get(pins)): string[] {
  const own = map[scope] ?? [];
  const hash = scope.indexOf("#");
  const legacy = hash > 0 ? (map[scope.slice(0, hash)] ?? []) : [];
  return [...own, ...legacy.filter((id) => !own.includes(id))];
}
export function pinnedId(scope: string): string | null {
  return pinnedIds(scope)[0] ?? null;
}
export function isPinned(scope: string, msgId: string): boolean {
  return pinnedIds(scope).includes(msgId);
}
export function pinMessage(scope: string, msgId: string) {
  pins.update((p) => ({ ...p, [scope]: [msgId, ...(p[scope] ?? []).filter((x) => x !== msgId)].slice(0, MAX_PINS) }));
}
/** Unpin one message (from the scope and, for a room, the chat's old slot). */
export function unpinMessage(scope: string, msgId: string) {
  pins.update((p) => {
    const next = { ...p };
    const hash = scope.indexOf("#");
    for (const k of hash > 0 ? [scope, scope.slice(0, hash)] : [scope]) {
      const left = (next[k] ?? []).filter((x) => x !== msgId);
      if (left.length) next[k] = left; else delete next[k];
    }
    return next;
  });
}
