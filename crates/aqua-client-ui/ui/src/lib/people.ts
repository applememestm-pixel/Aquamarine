/**
 * Public profile cards of other people (and the publishing of ours).
 *
 * Cards live on the server directory (name, bio, extras) with the avatar
 * and banner images next to them, so everyone sees the same picture.
 * Fetched lazily, cached here (and the images on disk by the backend),
 * refreshed when a peer's presence says their profile version moved.
 */
import { writable, derived, get } from "svelte/store";
import { api, assetUrl } from "./tauri";
import { profile, updateProfile, type Profile } from "./profile";
import { info } from "./stores";
import { broadcastPresence, shareActivity } from "./presence";
import { playedTop } from "./played";
import { privacy, isPublic, photoAcl, contactSet } from "./privacy";

export interface PeerCard {
  username: string;
  display_name: string;
  about: string;
  phone: string;
  is_bot: boolean;
  extra: Record<string, unknown>;
  avatar_ver: number;
  banner_ver: number;
  /** "Favourite track" audio version (0 = none). */
  music_ver?: number;
  /** Asset URLs of the cached images ("" when none). */
  avatar: string;
  banner: string;
  /** Asset URL of the cached favourite track ("" when none). */
  music?: string;
  /** When we last fetched (ms). */
  fetched: number;
  /** Highest profile version we've seen announced in presence. */
  pv: number;
}

const KEY = "aqua.peerCards.v1";
function load(): Record<string, PeerCard> {
  try { return JSON.parse(localStorage.getItem(KEY) ?? "{}"); } catch { return {}; }
}
export const peerCards = writable<Record<string, PeerCard>>(load());
peerCards.subscribe((v) => { try { localStorage.setItem(KEY, JSON.stringify(v)); } catch { /* ignore */ } });

/** username → avatar URL, including ourselves. */
/** Avatars from elsewhere (group / channel photos), merged into `avatars`. */
export const extraAvatars = writable<Record<string, string>>({});

export const avatars = derived([peerCards, profile, info, extraAvatars], ([cards, me, i, extra]) => {
  const out: Record<string, string> = { ...extra };
  for (const [u, c] of Object.entries(cards)) if (c.avatar) out[u] = c.avatar;
  if (i?.username && me.avatar_url) out[i.username] = me.avatar_url;
  return out;
});

export interface ClanInfo { text: string; badge: string; color: string; gid?: string; title?: string; owner?: string }

function asClan(v: unknown): ClanInfo | null {
  if (!v || typeof v !== "object") return null;
  const o = v as Record<string, unknown>;
  const text = typeof o.text === "string" ? o.text.slice(0, 4) : "";
  if (!text) return null;
  return {
    text,
    badge: typeof o.badge === "string" ? o.badge : "star",
    color: typeof o.color === "string" && /^#[0-9a-fA-F]{3,8}$/.test(o.color) ? o.color : "",
    gid: typeof o.gid === "string" ? o.gid : undefined,
    title: typeof o.title === "string" ? o.title.slice(0, 64) : undefined,
    owner: typeof o.owner === "string" ? o.owner : undefined,
  };
}

/** username → the server tag they wear (Discord-style), including ours. */
export const clans = derived([peerCards, profile, info], ([cards, me, i]) => {
  const out: Record<string, ClanInfo> = {};
  for (const [u, c] of Object.entries(cards)) { const t = asClan(c.extra?.clan); if (t) out[u] = t; }
  const mine = asClan(me.clan);
  if (i?.username && mine) out[i.username] = mine;
  return out;
});

export function str(v: unknown): string { return typeof v === "string" ? v : ""; }

const inflight = new Map<string, Promise<void>>();
const STALE_MS = 10 * 60_000;

function skip(u: string): boolean {
  return !u || u.startsWith("grp_") || u.startsWith("__") || u === get(info)?.username;
}

/** Fetch (or refresh) one person's public card + images. */
export function refreshPeer(username: string, force = false): Promise<void> {
  if (skip(username) || !get(info)?.connected) return Promise.resolve();
  const cur = get(peerCards)[username];
  if (!force && cur && Date.now() - cur.fetched < STALE_MS) return Promise.resolve();
  const running = inflight.get(username);
  if (running) return running;
  const job = (async () => {
    try {
      const card = await api.fetchDirectoryProfile(username);
      if (!card) {
        peerCards.update((m) => ({ ...m, [username]: { ...(m[username] ?? blank(username)), fetched: Date.now() } }));
        return;
      }
      const av = card.avatar_ver ?? 0, bv = card.banner_ver ?? 0, mv = card.music_ver ?? 0;
      let music = cur && cur.music_ver === mv ? (cur.music ?? "") : "";
      if (mv && !music) music = assetUrl(await api.profileMediaPath(username, "music", mv).catch(() => null));
      let avatar = cur && cur.avatar_ver === av ? cur.avatar : "";
      let banner = cur && cur.banner_ver === bv ? cur.banner : "";
      if (av && !avatar) avatar = assetUrl(await api.profileMediaPath(username, "avatar", av).catch(() => null));
      if (bv && !banner) banner = assetUrl(await api.profileMediaPath(username, "banner", bv).catch(() => null));
      peerCards.update((m) => ({
        ...m,
        [username]: {
          username,
          display_name: card.display_name === "You" ? "" : card.display_name,
          about: card.about,
          phone: card.phone,
          is_bot: card.is_bot,
          extra: (card.extra ?? {}) as Record<string, unknown>,
          avatar_ver: av, banner_ver: bv, music_ver: mv,
          avatar: av ? avatar : "", banner: bv ? banner : "", music: mv ? music : "",
          fetched: Date.now(),
          pv: m[username]?.pv ?? 0,
        },
      }));
    } catch { /* offline — keep the cached card */ }
    finally { inflight.delete(username); }
  })();
  inflight.set(username, job);
  return job;
}

function blank(username: string): PeerCard {
  return { username, display_name: "", about: "", phone: "", is_bot: false, extra: {}, avatar_ver: 0, banner_ver: 0, avatar: "", banner: "", fetched: 0, pv: 0 };
}

/** A peer announced profile version `pv` — refetch if it's new. */
export function notePeerProfileVersion(username: string, pv: number | undefined) {
  if (!pv) return;
  const cur = get(peerCards)[username];
  if (cur && cur.pv >= pv) return;
  peerCards.update((m) => ({ ...m, [username]: { ...(m[username] ?? blank(username)), pv } }));
  void refreshPeer(username, true);
}

// ---------------------------------------------------------------------------
// Our own public profile
// ---------------------------------------------------------------------------

function myExtra(p: Profile): Record<string, unknown> {
  return {
    banner_a: p.banner_a, banner_b: p.banner_b,
    status: p.status ?? "", pronouns: p.pronouns ?? "",
    // Restricted fields stay off the public card; allowed peers get them
    // over E2EE presence instead (presence.ts `pp`).
    birthday: isPublic("birthday") ? (p.birthday ?? "") : "", favourite_music: p.favourite_music ?? "",
    avatar_emoji: p.avatar_emoji ?? "",
    // Forwards of my messages may link to my account (Privacy → Forwarded messages).
    fwd_link: isPublic("forwards"),
    music_title: p.music_ver ? (p.music_title ?? "") : "",
    clan: p.clan ?? null,
    card: /^#[0-9a-f]{6}$/i.test(p.card ?? "") ? p.card : "",
    pattern: (p.pattern ?? "").slice(0, 16),
    links: (p.links ?? []).filter((l) => /^https?:\/\//i.test(l.url)).slice(0, 8),
    games: (p.games ?? []).slice(0, 12),
    game_ids: Object.fromEntries(Object.entries(p.game_ids ?? {}).filter(([n]) => (p.games ?? []).includes(n)).slice(0, 12)),
    game_note: (p.game_note ?? "").slice(0, 140),
    // Auto "game collection": top games by play time, only while sharing activity.
    played: get(shareActivity) ? playedTop(8).map((g) => ({ n: g.name, h: Math.round(g.secs / 360) / 10 })) : [],
  };
}

/** The name to publish: never the "You" placeholder of a fresh device. */
export function myPublicName(p: Profile = get(profile)): string {
  const n = (p.display_name ?? "").trim();
  return n && n !== "You" ? n : (get(info)?.username ?? "");
}

/** Signed in on a new device: take my name, bio and colours from the card
 *  already on the server instead of publishing the "You" placeholder. */
export async function adoptMyCard(username: string): Promise<void> {
  try {
    const card = await api.fetchDirectoryProfile(username);
    if (!card) return;
    const p = get(profile);
    const x = (card.extra ?? {}) as Record<string, unknown>;
    const patch: Partial<Profile> = {};
    const fresh = !p.display_name || p.display_name === "You";
    if (fresh && card.display_name && card.display_name !== "You") patch.display_name = card.display_name;
    if (fresh && card.about && (!p.about || p.about === "An Aquamarine user.")) patch.about = card.about;
    if (fresh && card.phone && !p.phone) patch.phone = card.phone;
    if (fresh && typeof x.birthday === "string" && !p.birthday) patch.birthday = x.birthday;
    if (fresh && typeof x.banner_a === "string") patch.banner_a = x.banner_a;
    if (fresh && typeof x.banner_b === "string") patch.banner_b = x.banner_b;
    if (fresh && typeof x.card === "string") patch.card = x.card;
    if (fresh && typeof x.pattern === "string") patch.pattern = x.pattern;
    if (Object.keys(patch).length) updateProfile(patch);
  } catch { /* offline: keep what we have */ }
}

/** Push our card (name, bio, extras) to the server and tell peers. */
export async function publishMyProfile(): Promise<void> {
  const p = get(profile);
  await api.publishProfileFull(
    myPublicName(p),
    isPublic("bio") ? p.about : "",
    isPublic("phone") ? p.phone : "",
    myExtra(p),
  );
  updateProfile({ pv: Date.now() });
  broadcastPresence();
}

/** Upload a new avatar / banner from a picked file. */
export async function setMyImage(kind: "avatar" | "banner", path: string): Promise<void> {
  const r = await api.setProfileImage(kind, path);
  applyMyImage(kind, r);
}

/** Picked while offline (or the upload failed): shown here right away,
 *  uploaded as soon as the relay is reachable. */
const PENDING_KEY = "aqua.pendingImage.v1";
interface PendingImage { kind: "avatar" | "banner"; mime: string; b64: string }
function bytesToB64(bytes: number[]): string {
  let s = "";
  for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.slice(i, i + 0x8000));
  return btoa(s);
}
function b64ToBytes(b64: string): number[] { const s = atob(b64); const out = new Array<number>(s.length); for (let i = 0; i < s.length; i++) out[i] = s.charCodeAt(i); return out; }

export async function setMyImageBytes(kind: "avatar" | "banner", mime: string, bytes: number[]): Promise<void> {
  try {
    const r = await api.setProfileImageBytes(kind, mime, bytes);
    applyMyImage(kind, r);
    clearPending(kind);
  } catch (e) {
    // Show it now; keep it for the next connection.
    const b64 = bytesToB64(bytes);
    const local = `data:${mime};base64,${b64}`;
    if (kind === "avatar") updateProfile({ avatar_url: local }); else updateProfile({ banner_url: local });
    updateProfile({ pv: Date.now() });
    try {
      const list = (JSON.parse(localStorage.getItem(PENDING_KEY) ?? "[]") as PendingImage[]).filter((p) => p.kind !== kind);
      list.push({ kind, mime, b64 });
      localStorage.setItem(PENDING_KEY, JSON.stringify(list));
    } catch { /* too big for storage: the next try needs a new pick */ }
    throw new Error(`${e} — saved here, it is uploaded when the server is reachable`);
  }
}
function clearPending(kind: "avatar" | "banner") {
  try {
    const list = (JSON.parse(localStorage.getItem(PENDING_KEY) ?? "[]") as PendingImage[]).filter((p) => p.kind !== kind);
    if (list.length) localStorage.setItem(PENDING_KEY, JSON.stringify(list)); else localStorage.removeItem(PENDING_KEY);
  } catch { /* ignore */ }
}
let flushing = false;
async function flushPendingImages() {
  if (flushing) return;
  let list: PendingImage[] = [];
  try { list = JSON.parse(localStorage.getItem(PENDING_KEY) ?? "[]"); } catch { return; }
  if (!list.length) return;
  flushing = true;
  try {
    for (const p of list) {
      try {
        const r = await api.setProfileImageBytes(p.kind, p.mime, b64ToBytes(p.b64));
        applyMyImage(p.kind, r);
        clearPending(p.kind);
      } catch { /* still offline */ }
    }
  } finally { flushing = false; }
}
info.subscribe((i) => { if (i?.connected) void flushPendingImages(); });

function applyMyImage(kind: "avatar" | "banner", r: { ver: number; path: string }) {
  const url = assetUrl(r.path) + `?v=${r.ver}`;
  if (kind === "avatar") updateProfile({ avatar_ver: r.ver, avatar_url: url });
  else updateProfile({ banner_ver: r.ver, banner_url: url });
  updateProfile({ pv: Date.now() });
  broadcastPresence();
}

/** Animated banner (GIF / short video, made silent + light) or favourite track. */
export async function setMyMediaFile(kind: "banner" | "music", path: string): Promise<void> {
  const r = await api.setProfileMediaFile(kind, path);
  if (kind === "music") {
    const title = (path.split(/[\\/]/).pop() ?? "").replace(/\.[^.]+$/, "").replace(/_/g, " ");
    updateProfile({ music_ver: r.ver, music_url: assetUrl(r.path), music_title: title });
    await publishMyProfile();
  } else {
    applyMyImage("banner", r);
  }
}

export async function clearMyMusic(): Promise<void> {
  await api.clearProfileMedia("music");
  updateProfile({ music_ver: 0, music_url: "", music_title: "" });
  await publishMyProfile();
}

export async function clearMyImage(kind: "avatar" | "banner"): Promise<void> {
  await api.clearProfileImage(kind);
  if (kind === "avatar") updateProfile({ avatar_ver: 0, avatar_url: "" });
  else updateProfile({ banner_ver: 0, banner_url: "" });
  updateProfile({ pv: Date.now() });
  broadcastPresence();
}

function dataUrlToBytes(url: string): { mime: string; bytes: number[] } | null {
  const m = /^data:([^;,]+);base64,(.*)$/.exec(url);
  if (!m) return null;
  const bin = atob(m[2]);
  const bytes = new Array<number>(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return { mime: m[1], bytes };
}

/** On connect: publish our card, and migrate an old local-only avatar
 *  (a data: URL from before avatars lived on the server) up to it. */
export async function syncMyProfileOnConnect(): Promise<void> {
  const p = get(profile);
  try {
    if (p.avatar_url?.startsWith("data:") && !p.avatar_ver) {
      const d = dataUrlToBytes(p.avatar_url);
      if (d) await setMyImageBytes("avatar", d.mime, d.bytes);
    }
    await publishMyProfile();
    await syncPhotoPrivacy();
  } catch (e) {
    console.warn("profile sync", e);
  }
}

/** Push the profile-photo allow / deny list to the server. */
export async function syncPhotoPrivacy(): Promise<void> {
  const acl = photoAcl();
  await api.setPhotoPrivacy(acl.everybody, acl.allow, acl.deny);
}

// Re-apply privacy whenever the rules (or who counts as a contact) change:
// republish the public card, the photo list, and per-peer presence.
let privTimer: ReturnType<typeof setTimeout> | null = null;
let rulesSig = "", contactsSig = "";
let pending = { publish: false, photo: false };
function onPrivacyChanged() {
  // The chat list refreshes on every message, so only act when the rules or
  // the actual set of contacts moved.
  const rules = get(privacy);
  const rs = JSON.stringify(rules);
  const cs = [...get(contactSet)].sort().join(",");
  const rulesMoved = rulesSig !== "" && rs !== rulesSig;
  const contactsMoved = contactsSig !== "" && cs !== contactsSig && rules.photo.who === "contacts";
  rulesSig = rs; contactsSig = cs;
  if (!rulesMoved && !contactsMoved) return;
  if (rulesMoved) pending.publish = true;
  pending.photo = true;
  if (privTimer) clearTimeout(privTimer);
  privTimer = setTimeout(async () => {
    privTimer = null;
    const job = pending;
    pending = { publish: false, photo: false };
    if (!get(info)?.connected) { if (job.publish) broadcastPresence(); return; }
    if (job.publish) { try { await publishMyProfile(); } catch (e) { console.warn("publish after privacy change", e); } }
    if (job.photo) { try { await syncPhotoPrivacy(); } catch (e) { console.warn("photo privacy", e); } }
  }, 700);
}
privacy.subscribe(onPrivacyChanged);
contactSet.subscribe(onPrivacyChanged);
