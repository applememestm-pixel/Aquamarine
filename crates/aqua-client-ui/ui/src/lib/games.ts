/**
 * Game names → catalogue ids (Discord's detectable list, served by the
 * relay) and their icons. Ids picked in the editor are remembered; other
 * names (played games, friends' cards) are looked up once.
 */
import { writable, get } from "svelte/store";
import { api, assetUrl } from "./tauri";

const KEY = "aqua.gameIds.v1";
function load(): Record<string, string> {
  try { return JSON.parse(localStorage.getItem(KEY) ?? "{}") as Record<string, string>; } catch { return {}; }
}
/** lower-cased name → id ("" = looked up, not found). */
export const gameIds = writable<Record<string, string>>(load());
gameIds.subscribe((v) => { try { localStorage.setItem(KEY, JSON.stringify(v)); } catch { /* ignore */ } });

/** id → asset URL of the icon ("" = none). */
export const gameIcons = writable<Record<string, string>>({});

const norm = (s: string) => s.trim().toLowerCase();
export function rememberGame(name: string, id: string) { gameIds.update((m) => ({ ...m, [norm(name)]: id })); }

const pending = new Set<string>();
/** Find the id for `name` (once) and fetch its icon. */
export function resolveGame(name: string, known?: string) {
  const k = norm(name);
  if (!k) return;
  if (known && !get(gameIds)[k]) rememberGame(name, known);
  const id = known || get(gameIds)[k];
  if (id) { loadIcon(id); return; }
  if (id === "" || pending.has(k)) return;
  pending.add(k);
  void api.gamesSearch(name).then((hits) => {
    const hit = hits.find((h) => norm(h.name) === k) ?? (hits.length === 1 ? hits[0] : undefined);
    gameIds.update((m) => ({ ...m, [k]: hit?.id ?? "" }));
    if (hit) loadIcon(hit.id);
  }).catch(() => { /* offline: try again next start */ }).finally(() => pending.delete(k));
}

const iconPending = new Set<string>();
function loadIcon(id: string) {
  if (id in get(gameIcons) || iconPending.has(id)) return;
  iconPending.add(id);
  void api.gameIcon(id).then((p) => {
    gameIcons.update((m) => ({ ...m, [id]: p ? assetUrl(p) : "" }));
  }).catch(() => {}).finally(() => iconPending.delete(id));
}

/** Icon URL for a name if we have it (reactive via the stores). */
export function iconFor(name: string, ids: Record<string, string>, icons: Record<string, string>, extra?: Record<string, string>): string {
  const id = extra?.[name] || ids[norm(name)];
  return id ? icons[id] ?? "" : "";
}
