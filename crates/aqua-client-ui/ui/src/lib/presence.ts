/**
 * Presence — Discord-style online status, custom status line, "now
 * playing" and auto-detected activity. Ours is broadcast to peers over
 * the E2EE control channel ([status:v1]); theirs arrives the same way and
 * is kept in memory. `pv` (profile version) tells peers to re-fetch our
 * public card (avatar / banner / bio) from the server.
 */

import { writable, readable, get } from "svelte/store";
import { profile } from "./profile";
import { chats } from "./stores";
import { api, events } from "./tauri";
import type { Activity } from "./types";
import { allowed, isPublic, privacy } from "./privacy";

export type OnlineStatus = "online" | "idle" | "dnd" | "invisible";

export interface Presence {
  status?: string;
  listening?: { title: string; artist: string; url?: string } | null;
  /** "online" | "idle" | "dnd" | "offline" (we never reveal "invisible"). */
  online?: string;
  activity?: Activity[];
  /** Profile version: bumps when the sender edits their public profile. */
  pv?: number;
  /** Private profile fields this peer may see (kept off the public card). */
  pp?: { about?: string; phone?: string; birthday?: string };
  /** "You may not call me" / "…send me voice messages" / "…add me to groups". */
  nc?: boolean;
  nv?: boolean;
  ni?: boolean;
  /** "You may link to my account when forwarding my messages" (when that isn't public). */
  fl?: boolean;
  /** Local receive time (ms) — not sent; used to expire stale "online". */
  at?: number;
}

/** peerId -> their last-known presence. */
export const peerPresence = writable<Record<string, Presence>>({});

export function setPeerPresence(peer: string, p: Presence) {
  peerPresence.update((m) => ({ ...m, [peer]: { ...p, at: Date.now() } }));
}

/** Peers re-announce presence every HEARTBEAT; silence for longer than
 *  STALE means they closed the app / lost the connection. */
const HEARTBEAT_MS = 90_000;
const STALE_MS = 4 * 60_000;
/** Ticks every 30 s so "online" dots expire without other UI activity. */
export const presenceNow = readable(Date.now(), (set) => {
  const t = setInterval(() => set(Date.now()), 30_000);
  return () => clearInterval(t);
});

/** Effective status of a peer: "online" | "idle" | "dnd" | "offline". */
export function peerStatus(p: Presence | undefined, now = Date.now()): string {
  if (!p || !p.at || now - p.at > STALE_MS) return "offline";
  return p.online && p.online !== "invisible" ? p.online : "offline";
}

/** Should this chat show the green "online" dot? Real people: only what
 *  their (privacy-filtered) presence says — so ghost / invisible works.
 *  Bots keep their static flag; Saved Messages never shows one. */
/** Status circle of a chat in lists: people only ("online" | "idle" |
 *  "dnd" | "offline"); groups, channels, servers, bots and Saved Messages
 *  get none. */
export function chatStatus(
  c: { id: string; live: boolean; has_lock: boolean; channel?: boolean },
  map: Record<string, Presence>,
  now = Date.now(),
): string | null {
  if (c.id === "__saved__" || c.id.startsWith("grp_") || c.channel) return null;
  if (!(c.live && c.has_lock)) return null;
  return peerStatus(map[c.id], now);
}

export function chatOnline(
  c: { id: string; online: boolean; live: boolean; has_lock: boolean },
  map: Record<string, Presence>,
  now = Date.now(),
): boolean {
  if (c.id === "__saved__") return false;
  const person = c.live && c.has_lock && !c.id.startsWith("grp_");
  if (!person) return c.online;
  return peerStatus(map[c.id], now) !== "offline";
}

function persisted<T>(key: string, init: T) {
  let start = init;
  try { const raw = localStorage.getItem(key); if (raw != null) start = JSON.parse(raw) as T; } catch { /* ignore */ }
  const s = writable<T>(start);
  s.subscribe((v) => { try { localStorage.setItem(key, JSON.stringify(v)); } catch { /* ignore */ } });
  return s;
}

/** Our chosen online status (Discord's status picker). */
export const myOnline = persisted<OnlineStatus>("aqua.onlineStatus", "online");
/** Share what we play / listen to / watch with chat partners. */
export const shareActivity = persisted<boolean>("aqua.shareActivity", true);
/** What the detector currently sees on this PC. */
export const myActivity = writable<Activity[]>([]);

/** The presence object we send to `peer` — filtered by our privacy rules. */
export function myPresence(peer = ""): Presence {
  const p = get(profile);
  const online = get(myOnline);
  const rules = get(privacy);
  const ok = (k: Parameters<typeof allowed>[0]) => !peer || allowed(k, peer, rules);
  const hidden = online === "invisible" || !ok("last_seen");
  const act = !hidden && ok("activity");
  const out: Presence = {
    status: hidden ? "" : (p.status ?? ""),
    listening: act && p.listening && p.listening.title ? p.listening : null,
    online: hidden ? "offline" : online,
    activity: act && get(shareActivity) ? get(myActivity) : [],
    pv: p.pv ?? 0,
  };
  if (peer) {
    const pp: NonNullable<Presence["pp"]> = {};
    if (!isPublic("bio", rules) && ok("bio") && p.about) pp.about = p.about;
    if (!isPublic("phone", rules) && ok("phone") && p.phone) pp.phone = p.phone;
    if (!isPublic("birthday", rules) && ok("birthday") && p.birthday) pp.birthday = p.birthday;
    if (Object.keys(pp).length) out.pp = pp;
    if (!ok("calls")) out.nc = true;
    if (!ok("voice")) out.nv = true;
    if (!ok("invites")) out.ni = true;
    if (!isPublic("forwards", rules) && ok("forwards")) out.fl = true;
  }
  return out;
}

export function myPresencePayload(peer = ""): string {
  return "[status:v1]" + JSON.stringify(myPresence(peer));
}

/** Broadcast our presence to one peer (if live). */
export function broadcastPresenceTo(peerId: string) {
  void api.sendControl(peerId, myPresencePayload(peerId)).catch(() => {});
}

/** Broadcast our presence to every live 1:1 chat — call after a change. */
export function broadcastPresence() {
  for (const c of get(chats)) {
    if (c.live && !c.id.startsWith("grp_") && c.id !== "__saved__") void api.sendControl(c.id, myPresencePayload(c.id)).catch(() => {});
  }
}

function sameActivity(a: Activity[], b: Activity[]): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

let started = false;
/** Start following the local activity detector. Call once at boot. */
export function initPresence() {
  if (started) return;
  started = true;
  void events.onActivity((a) => {
    a = Array.isArray(a) ? a : [];
    if (sameActivity(a, get(myActivity))) return;
    myActivity.set(a);
    if (get(shareActivity)) broadcastPresence();
  });
  shareActivity.subscribe((on) => {
    void api.setActivityDetection(on).catch(() => {});
    if (!on) myActivity.set([]);
    else void api.currentActivity().then((a) => myActivity.set(Array.isArray(a) ? a : [])).catch(() => {});
  });
  let first = true;
  myOnline.subscribe(() => { if (first) { first = false; return; } broadcastPresence(); });
  // Keep peers' "online" fresh (they expire it after STALE_MS of silence),
  // and say goodbye when the window closes.
  setInterval(() => broadcastPresence(), HEARTBEAT_MS);
  window.addEventListener("beforeunload", () => broadcastOffline());
}

/** Best-effort "I'm gone" to every live chat (app closing). */
export function broadcastOffline() {
  const off = (peer: string) => "[status:v1]" + JSON.stringify({ ...myPresence(peer), online: "offline", activity: [], listening: null });
  for (const c of get(chats)) {
    if (c.live && !c.id.startsWith("grp_") && c.id !== "__saved__") void api.sendControl(c.id, off(c.id)).catch(() => {});
  }
}

/** Human line for an activity: "Playing Dota 2", "Listening to Spotify". */
export function activityVerb(a: Activity): string {
  return a.kind === "playing" ? "Playing" : a.kind === "listening" ? "Listening to" : "Watching";
}
export function activityLine(a: Activity): string {
  if (a.kind === "listening" && a.details) return `${a.details}${a.state ? " — " + a.state : ""}`;
  if (a.kind === "watching" && a.details) return a.details;
  return "";
}
