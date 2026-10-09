/**
 * Presence — Discord-style online status, custom status line, "now
 * playing" and auto-detected activity. Ours is broadcast to peers over
 * the E2EE control channel ([status:v1]); theirs arrives the same way and
 * is kept in memory. `pv` (profile version) tells peers to re-fetch our
 * public card (avatar / banner / bio) from the server.
 */

import { writable, get } from "svelte/store";
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
}

/** peerId -> their last-known presence. */
export const peerPresence = writable<Record<string, Presence>>({});

export function setPeerPresence(peer: string, p: Presence) {
  peerPresence.update((m) => ({ ...m, [peer]: p }));
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
