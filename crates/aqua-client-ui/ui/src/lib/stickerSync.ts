/** Portable sticker metadata. Files are downloaded from the relay by pack source. */
import { get, writable } from "svelte/store";
import { api } from "./tauri";
import type { StickerPack } from "./types";
import { stickerLibraryTick } from "./panels/stickerEditor";

interface Use { at: number; removed: boolean }
export interface StickerState {
  packs: Record<string, Use>;
  recent: Record<string, Use>;
  cleared: number;
}
const KEY = "aqua.stickers.account.v1";
const empty = (): StickerState => ({ packs: Object.create(null), recent: Object.create(null), cleared: 0 });
const sourceOk = (s: string) => /^[a-zA-Z0-9_]{1,128}$/.test(s);
export function checkedStickerState(value: unknown): StickerState {
  const result = empty();
  if (!value || typeof value !== "object") return result;
  const v = value as Partial<StickerState>;
  result.cleared = Number.isFinite(v.cleared) ? Math.max(0, v.cleared!) : 0;
  for (const field of ["packs", "recent"] as const) {
    if (!v[field] || typeof v[field] !== "object") continue;
    for (const [key, use] of Object.entries(v[field]!).slice(0, 2000)) {
      if (key.length > 500 || !use || !Number.isFinite(use.at) || use.at < 0) continue;
      if (field === "packs" && !sourceOk(key)) continue;
      if (field === "recent" && !/^tg:[A-Za-z0-9_]+\/[^/\\]+$/.test(key)) continue;
      result[field][key] = { at: use.at, removed: !!use.removed };
    }
  }
  return result;
}
function load(): StickerState {
  try { return checkedStickerState(JSON.parse(localStorage.getItem(KEY) ?? "null")); } catch { return empty(); }
}
export const stickerState = writable<StickerState>(load());
stickerState.subscribe((state) => {
  try { localStorage.setItem(KEY, JSON.stringify(state)); } catch { /* storage unavailable */ }
});

export function mergeStickerState(remote: unknown, current: StickerState): StickerState {
  const next = checkedStickerState(remote);
  next.cleared = Math.max(next.cleared, current.cleared);
  for (const field of ["packs", "recent"] as const) {
    for (const [key, use] of Object.entries(current[field])) {
      const incoming = next[field][key];
      if (!incoming || use.at > incoming.at || (use.at === incoming.at && use.removed)) next[field][key] = use;
    }
  }
  return next;
}

export function stickerKey(pack: StickerPack, path: string): string | null {
  const file = path.split(/[\\/]/).pop();
  return pack.source && sourceOk(pack.source) && file ? `tg:${pack.source}/${file}` : null;
}
export function noteSticker(pack: StickerPack, path: string, removed = false) {
  const key = stickerKey(pack, path);
  if (!key) return;
  stickerState.update((s) => ({ ...s, recent: { ...s.recent, [key]: { at: Date.now(), removed } } }));
}
export function clearRecentStickers() {
  stickerState.update((s) => ({ ...s, cleared: Date.now() }));
}
export function notePack(source: string, removed = false) {
  if (!sourceOk(source)) return;
  stickerState.update((s) => ({ ...s, packs: { ...s.packs, [source]: { at: Date.now(), removed } } }));
}

let restoring = false;
let restorePending = false;
/** Called on reconnect and after changes, retrying missing packs next time if offline. */
export async function restoreStickerPacks() {
  if (restoring) {
    restorePending = true;
    return;
  }
  restoring = true;
  try {
    do {
      restorePending = false;
      await restoreOnce();
    } while (restorePending);
  } finally { restoring = false; }
}

async function restoreOnce() {
  const library = await api.listLocalMedia();
  const state = get(stickerState);
  const installed = new Set(library.packs.map((p) => p.source).filter(Boolean));
  let changed = false;
  for (const [source, use] of Object.entries(state.packs)) {
    // Another device may change this operation while an earlier pack downloads.
    const latest = get(stickerState).packs[source];
    if (!latest || latest.at !== use.at || latest.removed !== use.removed) {
      restorePending = true;
      continue;
    }
    if (use.removed) {
      for (const pack of library.packs.filter((p) => p.source === source)) {
        try { await api.deleteStickerPack(pack.title); changed = true; } catch { /* retry on reconnect */ }
      }
      continue;
    }
    if (installed.has(source)) continue;
    try { await api.importPackViaServer(source); changed = true; } catch { /* retry on reconnect */ }
  }
  if (changed) stickerLibraryTick.update((n) => n + 1);
  // Older installations become portable without reinstalling each pack.
  stickerState.update((s) => {
    const packs = { ...s.packs };
    for (const p of library.packs) {
      if (p.source && sourceOk(p.source) && !packs[p.source]) packs[p.source] = { at: 1, removed: false };
    }
    return { ...s, packs };
  });
}
