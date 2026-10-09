/**
 * Per-message UI metadata kept on this device (localStorage):
 *   - reactions: emoji reactions you've placed on a message
 *   - pins:      the pinned message per chat
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
export const REACTION_SET = ["👍", "❤️", "🔥", "😂", "😮", "😢", "🙏", "🎉"];

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
type PinMap = Record<string, string>; // peerId -> pinned msgId
const P_KEY = "aqua-pins";

function loadPins(): PinMap {
  try { return JSON.parse(localStorage.getItem(P_KEY) ?? "{}") as PinMap; }
  catch { return {}; }
}
export const pins = writable<PinMap>(loadPins());
pins.subscribe((p) => { try { localStorage.setItem(P_KEY, JSON.stringify(p)); } catch { /* ignore */ } });

export function pinnedId(peer: string): string | null {
  return get(pins)[peer] ?? null;
}
export function pinMessage(peer: string, msgId: string) {
  pins.update((p) => ({ ...p, [peer]: msgId }));
}
export function unpinMessage(peer: string) {
  pins.update((p) => { const { [peer]: _drop, ...rest } = p; return rest; });
}
