/**
 * What the relay's admin decided about sticker packs (aqua-server packs.rs
 * `Rules`), and the 18+ filter (Telegram's "sensitive content").
 *
 * The relay lists them with `/v1/packs`: banned packs, stickers removed
 * from a pack, and 18+ packs. Here they apply to everything on this device:
 * a banned pack and a removed sticker disappear from the picker and show as
 * "removed" in chats; an 18+ pack is hidden and its stickers blurred (tap to
 * see) while the filter is on — it is on until the person turns it off.
 * Kept between runs, so it holds while the relay is out of reach.
 */
import { writable, derived } from "svelte/store";
import { api } from "./tauri";

interface Raw { adult: string[]; banned: string[]; removed: Record<string, string[]> }
const KEY = "aqua.packRules.v1";

function persisted<T>(key: string, init: T) {
  let start = init;
  try { const raw = localStorage.getItem(key); if (raw != null) start = JSON.parse(raw) as T; } catch { /* storage blocked */ }
  const s = writable<T>(start);
  s.subscribe((v) => { try { localStorage.setItem(key, JSON.stringify(v)); } catch { /* ignore */ } });
  return s;
}

const raw = persisted<Raw>(KEY, { adult: [], banned: [], removed: {} });
/** Show 18+ stickers and packs (off = the filter is on). */
export const showAdult = persisted<boolean>("aqua.showAdult.v1", false);

export interface Rules { adult: Set<string>; banned: Set<string>; removed: Map<string, Set<string>> }
export const packRules = derived<typeof raw, Rules>(raw, (r) => ({
  adult: new Set((r.adult ?? []).map((x) => x.toLowerCase())),
  banned: new Set((r.banned ?? []).map((x) => x.toLowerCase())),
  removed: new Map(Object.entries(r.removed ?? {}).map(([k, v]) => [k.toLowerCase(), new Set(v)])),
}));

/** The relay's pack name of a sticker's `pack` ("tg:Name" → "name"); "" for my own. */
export function packKey(pack: string | null | undefined): string {
  return pack?.startsWith("tg:") ? pack.slice(3).toLowerCase() : "";
}

export type Verdict = "ok" | "adult" | "gone";
/** A sticker from `pack` (a "tg:…" reference or a short name), file `file`. */
export function verdictOf(rules: Rules, show: boolean, pack: string | null | undefined, file?: string | null): Verdict {
  const k = pack?.startsWith("tg:") ? packKey(pack) : (pack ?? "").toLowerCase();
  if (!k) return "ok";
  if (rules.banned.has(k)) return "gone";
  if (file && rules.removed.get(k)?.has(file)) return "gone";
  if (!show && rules.adult.has(k)) return "adult";
  return "ok";
}
export const stickerVerdict = derived([packRules, showAdult], ([r, s]) => (pack: string | null | undefined, file?: string | null) => verdictOf(r, s, pack, file));

let last = 0;
/** Ask the relay again (at most every 2 minutes unless `force`). */
export async function refreshPackRules(force = false): Promise<void> {
  if (!force && Date.now() - last < 120_000) return;
  last = Date.now();
  try {
    const r = await api.serverPacks() as { packs?: { name: string; adult?: boolean }[]; banned?: string[]; removed?: Record<string, string[]> } | null;
    if (!r) return;
    // A relay before 1.20.1-9 sends no rules: keep what we knew.
    if (!Array.isArray(r.banned) && !r.packs?.some((p) => "adult" in p)) return;
    raw.set({ adult: (r.packs ?? []).filter((p) => p.adult).map((p) => p.name), banned: r.banned ?? [], removed: r.removed ?? {} });
  } catch { last = 0; }
}
