/**
 * Privacy rules (Telegram-style): for each item, a base audience —
 * Everybody / My contacts / Nobody — plus per-person exceptions
 * ("Always allow" / "Never allow"). The per-user screen in a profile
 * (Telegram X) writes the same exceptions, so they show up here too.
 *
 * Enforcement:
 *  - last seen / activity / bio / phone / birthday: our presence is built
 *    per peer, so each person only receives what they're allowed to see;
 *    restricted fields are left off the public server card.
 *  - profile photo: the server keeps an allow / deny list and only hands
 *    our avatar / banner to the people on it.
 *  - calls: offers from people who may not call us are declined
 *    automatically; their client is told up front (presence flag).
 *  - voice messages: their client is told and won't record for us.
 *  - group invites: groups created by someone who may not add us are left
 *    as soon as they arrive.
 */
import { writable, get, derived } from "svelte/store";
import { contacts } from "./contacts";
import { chats } from "./stores";
import { tr } from "./i18n";
import { isBlocked } from "./ghost";

export type Audience = "everybody" | "contacts" | "nobody";
export type PrivKey = "last_seen" | "photo" | "bio" | "phone" | "birthday" | "activity" | "calls" | "voice" | "invites";
export interface Rule { who: Audience; allow: string[]; deny: string[] }
export type Rules = Record<PrivKey, Rule>;

export const PRIV_ITEMS: { key: PrivKey; label: string; q: string; noNobody?: boolean }[] = [
  { key: "last_seen", label: "Last seen & online", q: "Who can see my last seen and online status?" },
  { key: "photo",     label: "Profile photos",     q: "Who can see my profile photo and banner?" },
  { key: "bio",       label: "Bio",                q: "Who can see my bio?" },
  { key: "phone",     label: "Phone number",       q: "Who can see my phone number?" },
  { key: "birthday",  label: "Date of birth",      q: "Who can see my date of birth?" },
  { key: "activity",  label: "Activity & music",   q: "Who can see what I play, watch and listen to?" },
  { key: "calls",     label: "Calls",              q: "Who can call me?" },
  { key: "voice",     label: "Voice messages",     q: "Who can send me voice and video messages?" },
  { key: "invites",   label: "Invites",            q: "Who can add me to groups and channels?" },
];

const DEFAULTS: Rules = {
  last_seen: { who: "everybody", allow: [], deny: [] },
  photo:     { who: "everybody", allow: [], deny: [] },
  bio:       { who: "everybody", allow: [], deny: [] },
  phone:     { who: "everybody", allow: [], deny: [] },
  birthday:  { who: "contacts",  allow: [], deny: [] },
  activity:  { who: "everybody", allow: [], deny: [] },
  calls:     { who: "everybody", allow: [], deny: [] },
  voice:     { who: "everybody", allow: [], deny: [] },
  invites:   { who: "everybody", allow: [], deny: [] },
};

function load(): Rules {
  try {
    const raw = JSON.parse(localStorage.getItem("aqua.privacy.v1") ?? "{}") as Partial<Rules>;
    const out = structuredClone(DEFAULTS);
    for (const k of Object.keys(DEFAULTS) as PrivKey[]) {
      const r = raw[k];
      if (r && (r.who === "everybody" || r.who === "contacts" || r.who === "nobody")) {
        out[k] = { who: r.who, allow: Array.isArray(r.allow) ? r.allow : [], deny: Array.isArray(r.deny) ? r.deny : [] };
      }
    }
    return out;
  } catch {
    return structuredClone(DEFAULTS);
  }
}

export const privacy = writable<Rules>(load());
privacy.subscribe((v) => { try { localStorage.setItem("aqua.privacy.v1", JSON.stringify(v)); } catch { /* ignore */ } });

/** "My contacts" = people in the contact book plus everyone we have a 1:1 chat with. */
export const contactSet = derived([contacts, chats], ([c, ch]) => {
  const s = new Set<string>(Object.keys(c));
  for (const x of ch) if (!x.id.startsWith("grp_") && !x.id.startsWith("__") && x.live) s.add(x.id);
  return s;
});

export function isContact(u: string): boolean {
  return get(contactSet).has(u);
}

/** May `peer` see / do `key`? */
export function allowed(key: PrivKey, peer: string, rules: Rules = get(privacy)): boolean {
  if (isBlocked(peer)) return false;
  const r = rules[key];
  if (r.deny.includes(peer)) return false;
  if (r.allow.includes(peer)) return true;
  if (r.who === "everybody") return true;
  if (r.who === "contacts") return isContact(peer);
  return false;
}

/** Is `key` visible to literally everyone (so it may go on the public card)? */
export function isPublic(key: PrivKey, rules: Rules = get(privacy)): boolean {
  return rules[key].who === "everybody" && rules[key].deny.length === 0;
}

export type Exception = "allow" | "deny" | null;
export function exceptionOf(key: PrivKey, peer: string, rules: Rules = get(privacy)): Exception {
  const r = rules[key];
  return r.deny.includes(peer) ? "deny" : r.allow.includes(peer) ? "allow" : null;
}

export function setException(key: PrivKey, peer: string, v: Exception) {
  const u = peer.trim().replace(/^@/, "");
  if (!u) return;
  privacy.update((all) => {
    const r = all[key];
    const allow = r.allow.filter((x) => x !== u);
    const deny = r.deny.filter((x) => x !== u);
    if (v === "allow") allow.push(u);
    if (v === "deny") deny.push(u);
    return { ...all, [key]: { ...r, allow, deny } };
  });
}

export function setAudience(key: PrivKey, who: Audience) {
  privacy.update((all) => ({ ...all, [key]: { ...all[key], who } }));
}

export function audienceLabel(r: Rule): string {
  const base = tr(r.who === "everybody" ? "Everybody" : r.who === "contacts" ? "My contacts" : "Nobody");
  const extra: string[] = [];
  if (r.deny.length) extra.push(`−${r.deny.length}`);
  if (r.allow.length) extra.push(`+${r.allow.length}`);
  return extra.length ? `${base} (${extra.join(", ")})` : base;
}

/** How many exceptions mention this person (for the profile screen badge). */
export function exceptionCount(peer: string, rules: Rules = get(privacy)): number {
  return (Object.keys(rules) as PrivKey[]).filter((k) => exceptionOf(k, peer, rules) !== null).length;
}

/** Server-side list for the profile photo. */
export function photoAcl(rules: Rules = get(privacy)): { everybody: boolean; allow: string[]; deny: string[] } {
  const r = rules.photo;
  if (r.who === "everybody") return { everybody: true, allow: [], deny: r.deny };
  const allow = new Set(r.allow);
  if (r.who === "contacts") for (const u of get(contactSet)) if (!r.deny.includes(u)) allow.add(u);
  return { everybody: false, allow: [...allow], deny: r.deny };
}
