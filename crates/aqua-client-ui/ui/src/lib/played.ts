/**
 * "Game collection": every game the activity detector sees, with play time.
 * Kept on this device; the top entries are published on the profile card
 * (only while activity sharing is on).
 */
import { writable, get } from "svelte/store";
import { myActivity } from "./presence";

export interface Played { name: string; secs: number; last: number; first: number }
const KEY = "aqua.played.v1";

function load(): Record<string, Played> {
  try { return JSON.parse(localStorage.getItem(KEY) ?? "{}") as Record<string, Played>; } catch { return {}; }
}
export const played = writable<Record<string, Played>>(load());
played.subscribe((m) => { try { localStorage.setItem(KEY, JSON.stringify(m)); } catch { /* ignore */ } });

let current = "";
let since = 0;
function flush(now: number) {
  if (!current || !since) return;
  const add = Math.max(0, Math.round((now - since) / 1000));
  since = now;
  if (!add) return;
  played.update((m) => {
    const p = m[current] ?? { name: current, secs: 0, last: now, first: now };
    return { ...m, [current]: { ...p, secs: p.secs + add, last: now } };
  });
}

let started = false;
/** Start counting (once, from App). */
export function trackPlayed(): void {
  if (started) return;
  started = true;
  myActivity.subscribe((acts) => {
    const now = Date.now();
    flush(now);
    const g = acts.find((a) => a.kind === "playing")?.name ?? "";
    if (g !== current) {
      current = g;
      since = g ? now : 0;
      if (g) played.update((m) => (m[g] ? { ...m, [g]: { ...m[g], last: now } } : { ...m, [g]: { name: g, secs: 0, last: now, first: now } }));
    }
  });
  setInterval(() => flush(Date.now()), 60_000);
}

/** Most-played first. */
export function playedTop(n = 12, m = get(played)): Played[] {
  return Object.values(m).sort((a, b) => b.secs - a.secs || b.last - a.last).slice(0, n);
}

export function forgetPlayed(name: string): void {
  played.update((m) => { const { [name]: _, ...rest } = m; return rest; });
}

export function fmtPlaytime(secs: number): string {
  const h = Math.floor(secs / 3600);
  if (h >= 1) return `${h} h`;
  const m = Math.max(1, Math.round(secs / 60));
  return `${m} min`;
}

/** A stable two-colour gradient per game name (no cover art yet). */
export function gameGradient(name: string): string {
  let h = 2166136261;
  for (let i = 0; i < name.length; i++) { h ^= name.charCodeAt(i); h = Math.imul(h, 16777619); }
  const a = (h >>> 0) % 360;
  const b = (a + 40 + ((h >>> 9) % 80)) % 360;
  return `linear-gradient(135deg, hsl(${a} 70% 45%), hsl(${b} 75% 30%))`;
}
