/**
 * Discord's "sensitive media" filter: incoming photos, GIFs and stickers
 * are looked at on this device (src/nsfw.rs — the detector runs after
 * decryption, nothing is sent anywhere) and explicit ones are blurred until
 * tapped. Settings → Stickers / media: blur from everyone, only from people
 * not in my contacts (the default, as in Discord), or never.
 */
import { writable, derived } from "svelte/store";
import { api } from "./tauri";

export type BlurMode = "all" | "strangers" | "off";

function persisted<T>(key: string, init: T) {
  let start = init;
  try { const raw = localStorage.getItem(key); if (raw != null) start = JSON.parse(raw) as T; } catch { /* storage blocked */ }
  const s = writable<T>(start);
  s.subscribe((v) => { try { localStorage.setItem(key, JSON.stringify(v)); } catch { /* ignore */ } });
  return s;
}

export const sensitiveBlur = persisted<BlurMode>("aqua.sensitiveBlur.v1", "strangers");

/** path → 1 clean (or not a picture it reads), 2 explicit. */
const results = writable<Record<string, 1 | 2>>({});
const asked = new Set<string>();

async function look(path: string) {
  let v: 1 | 2 = 1;
  try { v = (await api.nsfwCheck(path))?.explicit ? 2 : 1; } catch { /* unreadable: leave it be */ }
  results.update((r) => { r[path] = v; return r; });
}

/** "pending" (being looked at — keep it covered), "clean" or "explicit".
 *  Asking starts the look (once per file; the app keeps the verdicts). */
export const nsfwOf = derived(results, (r) => (path: string | null | undefined): "pending" | "clean" | "explicit" => {
  if (!path) return "clean";
  const v = r[path];
  if (v) return v === 2 ? "explicit" : "clean";
  if (!asked.has(path)) { asked.add(path); queueMicrotask(() => void look(path)); }
  return "pending";
});
