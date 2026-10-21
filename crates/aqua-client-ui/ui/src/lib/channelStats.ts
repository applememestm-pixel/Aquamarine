/**
 * Channel statistics, end-to-end encrypted like everything else:
 *
 * - A subscriber's app reports the posts it showed to the channel's admins
 *   only: `[ctl:v1]{"t":"views","keys":[post…]}` (group_control_admins).
 * - Admin apps keep, per post, who viewed it and when, plus views per hour
 *   for the first two days. Full detail is kept for 30 days after a post's
 *   first view; after that only the counters stay (Telegram-like retention).
 * - Admins send the counters back to everyone (`{"t":"viewcounts"}`), so
 *   subscribers see "👁 123" under posts.
 *
 * All of it lives on the devices (and in their encrypted backups) — relays
 * never see who read what.
 */
import { writable, get } from "svelte/store";
import { listen } from "@tauri-apps/api/event";
import { api } from "./tauri";
import { info } from "./stores";

export const DETAIL_DAYS = 30;
const HOURS = 48;

export interface PostStats {
  /** Total views (never shrinks). */
  n: number;
  /** viewer → first view (ms); null once the detail expired. */
  v: Record<string, number> | null;
  /** First view (ms). */
  first: number;
  /** Views per hour since the first view (first 48 h). */
  h: number[];
}

function persisted<T>(key: string, init: T) {
  let v = init;
  try { const raw = localStorage.getItem(key); if (raw) v = JSON.parse(raw); } catch { /* ignore */ }
  const s = writable<T>(v);
  s.subscribe((x) => { try { localStorage.setItem(key, JSON.stringify(x)); } catch { /* ignore */ } });
  return s;
}

/** Admins: channel id → post key → stats. */
export const channelStats = persisted<Record<string, Record<string, PostStats>>>("aqua.chanStats.v1", {});
/** Everyone: channel id → post key → view count (from the admins). */
export const viewCounts = persisted<Record<string, Record<string, number>>>("aqua.viewCounts.v1", {});

// ---- subscriber side: report what was shown ----
const reported = new Set<string>(JSON.parse((() => { try { return localStorage.getItem("aqua.viewsSent.v1") ?? "[]"; } catch { return "[]"; } })()));
const pending = new Map<string, Set<string>>();
let flushTimer: ReturnType<typeof setTimeout> | null = null;

/** A post of channel `gid` was on screen. */
export function noteView(gid: string, key: string) {
  const id = `${gid}|${key}`;
  if (reported.has(id)) return;
  reported.add(id);
  if (!pending.has(gid)) pending.set(gid, new Set());
  pending.get(gid)!.add(key);
  if (!flushTimer) flushTimer = setTimeout(flush, 4000);
}

function flush() {
  flushTimer = null;
  for (const [gid, keys] of pending) {
    const list = [...keys].slice(0, 100);
    if (list.length) void api.groupControlAdmins(gid, "[ctl:v1]" + JSON.stringify({ t: "views", keys: list })).catch(() => {});
  }
  pending.clear();
  try { localStorage.setItem("aqua.viewsSent.v1", JSON.stringify([...reported].slice(-5000))); } catch { /* ignore */ }
}

// ---- admin side: count, keep detail for a month, share the counters ----
const dirtyCounts = new Map<string, Set<string>>();
let shareTimer: ReturnType<typeof setTimeout> | null = null;

function record(gid: string, viewer: string, keys: string[], at = Date.now()) {
  channelStats.update((all) => {
    const ch = { ...(all[gid] ?? {}) };
    for (const key of keys) {
      const cur: PostStats = ch[key] ? { ...ch[key], h: [...ch[key].h] } : { n: 0, v: {}, first: at, h: [] };
      if (cur.v && cur.v[viewer]) continue; // already counted
      cur.n += 1;
      if (cur.v) cur.v = { ...cur.v, [viewer]: at };
      const hour = Math.floor((at - cur.first) / 3_600_000);
      if (hour >= 0 && hour < HOURS) { while (cur.h.length <= hour) cur.h.push(0); cur.h[hour] += 1; }
      ch[key] = cur;
      if (!dirtyCounts.has(gid)) dirtyCounts.set(gid, new Set());
      dirtyCounts.get(gid)!.add(key);
    }
    return { ...all, [gid]: ch };
  });
  if (!shareTimer) shareTimer = setTimeout(shareCounts, 30_000);
}

function shareCounts() {
  shareTimer = null;
  const all = get(channelStats);
  for (const [gid, keys] of dirtyCounts) {
    const c: Record<string, number> = {};
    for (const k of keys) if (all[gid]?.[k]) c[k] = all[gid][k].n;
    if (Object.keys(c).length) {
      viewCounts.update((v) => ({ ...v, [gid]: { ...(v[gid] ?? {}), ...c } }));
      void api.groupControl(gid, "[ctl:v1]" + JSON.stringify({ t: "viewcounts", c })).catch(() => {});
    }
  }
  dirtyCounts.clear();
}

/** Drop who-viewed-when older than DETAIL_DAYS (counters stay). */
function prune() {
  const old = Date.now() - DETAIL_DAYS * 86_400_000;
  channelStats.update((all) => {
    let changed = false;
    const out: typeof all = {};
    for (const [gid, posts] of Object.entries(all)) {
      out[gid] = {};
      for (const [k, p] of Object.entries(posts)) {
        if (p.v && p.first < old) { out[gid][k] = { ...p, v: null }; changed = true; }
        else out[gid][k] = p;
      }
    }
    return changed ? out : all;
  });
}

let started = false;
export function startChannelStats() {
  if (started) return;
  started = true;
  prune();
  setInterval(prune, 6 * 3_600_000);
  void listen<{ from: string; chat: string; body: Record<string, unknown>; own?: boolean }>("aqua://ctl", (e) => {
    const { chat, body, from } = e.payload;
    if (!chat?.startsWith("grp_")) return;
    if (body?.t === "views" && Array.isArray(body.keys)) {
      const keys = (body.keys as unknown[]).filter((k): k is string => typeof k === "string" && k.length < 200).slice(0, 100);
      if (from && from !== get(info)?.username) record(chat, from, keys);
    } else if (body?.t === "viewcounts" && body.c && typeof body.c === "object") {
      const c: Record<string, number> = {};
      for (const [k, n] of Object.entries(body.c as Record<string, unknown>)) if (typeof n === "number" && n >= 0 && k.length < 200) c[k] = Math.floor(n);
      viewCounts.update((v) => {
        const cur = v[chat] ?? {};
        const next = { ...cur };
        for (const [k, n] of Object.entries(c)) next[k] = Math.max(next[k] ?? 0, n);
        return { ...v, [chat]: next };
      });
    }
  }).catch(() => {});
}
