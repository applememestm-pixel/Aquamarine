/**
 * The user's own profile — Discord-style: display name, @username,
 * bio, birthday, favourite music, banner gradient. All stored in
 * localStorage for now; will move to the vault (and later be published
 * as a public profile card on the server) when registration lands.
 */

import { writable, get } from "svelte/store";

export interface Profile {
  display_name: string;
  about: string;
  phone: string;
  birthday: string;       // free-form, "15 March 1998"
  favourite_music: string;
  banner_a: string;       // hex
  banner_b: string;       // hex
  avatar_emoji?: string;  // single emoji used as avatar when no image is set
  avatar_url?: string;    // data: URL of a chosen avatar image
  status?: string;        // Discord-style one-line custom status
  pronouns?: string;      // e.g. "she/her", "он/его"
  /** Manually-set "now playing" shown to peers (no OAuth; you set it). */
  listening?: { title: string; artist: string; url?: string };
  /** Server version + local cached copy of our public avatar / banner. */
  avatar_ver?: number;
  banner_ver?: number;
  banner_url?: string;
  /** "Favourite track" — an audio file on the server (+ local copy). */
  music_ver?: number;
  music_url?: string;
  music_title?: string;
  /** Links shown on the profile (site, GitHub, Steam, Twitch…). */
  links?: { label: string; url: string }[];
  /** Favourite games, picked by hand (the played list is automatic). */
  games?: string[];
  /** Catalogue ids of those games (name → id), for icons. */
  game_ids?: Record<string, string>;
  /** A line about the first (favourite) game. */
  game_note?: string;
  /** Profile version — bumped on every publish so peers refetch the card. */
  pv?: number;
  /** When I last changed my profile on this device (ms). Sent on the card as
   *  `pt`: on connecting, the newer side wins — a new device takes the card
   *  instead of publishing its blank one over it (see people.ts
   *  syncMyProfileOnConnect). */
  edited?: number;
  /** The emoji next to my name: a pack emoji token (`ce:pack/file|😀`) or a
   *  plain emoji; "" = none. */
  estatus?: string;
  /** Colour of the whole profile card; "" = taken from the banner. */
  card?: string;
  /** How strong the card colour is at the top, 0–100 (default 62). */
  card_i?: number;
  /** How far down the colour fades, 0–100 (0 = solid, default 60). */
  card_g?: number;
  /** An emoji repeated over the banner in a soft pattern (Telegram-style). */
  pattern?: string;
  /** My name and my quotes' colour in chats (Telegram's profile colour); "" = automatic. */
  name_color?: string;
  /** Discord-style server tag I'm wearing (from a group I'm in). */
  clan?: { text: string; badge: string; color: string; gid?: string; title?: string; owner?: string };
}

const KEY = "aqua-profile";

const DEFAULTS: Profile = {
  display_name: "You",
  about: "An Aquamarine user.",
  phone: "",
  birthday: "",
  favourite_music: "",
  banner_a: "#00E5C7",
  banner_b: "#7B5CFF",
};

function load(): Profile {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return { ...DEFAULTS };
    return { ...DEFAULTS, ...(JSON.parse(raw) as Partial<Profile>) };
  } catch {
    return { ...DEFAULTS };
  }
}

export const profile = writable<Profile>(load());

profile.subscribe((p) => {
  try { localStorage.setItem(KEY, JSON.stringify(p)); } catch { /* ignore */ }
});

const NOT_EDITS = new Set<string>(["pv", "edited"]);
/** `adopt`: the change came from the card on the server, not from me — it
 *  doesn't count as an edit. */
export function updateProfile(patch: Partial<Profile>, adopt = false) {
  const edit = !adopt && Object.keys(patch).some((k) => !NOT_EDITS.has(k));
  profile.update((p) => ({ ...p, ...patch, ...(edit ? { edited: Date.now() } : {}) }));
}

/** Before `edited` existed: a profile someone set up (a picture, colours, a
 *  name) counts as edited a moment after 1970 — older than any real edit,
 *  newer than a blank one — so a set-up device keeps its card and a blank
 *  one takes it. */
{
  const p = get(profile);
  if (p.edited === undefined) {
    const setUp = !!(p.avatar_ver || p.banner_ver || p.avatar_url || p.banner_url || p.card || p.estatus || p.pattern || p.name_color
      || (p.banner_a && p.banner_a !== DEFAULTS.banner_a) || (p.banner_b && p.banner_b !== DEFAULTS.banner_b));
    profile.update((x) => ({ ...x, edited: setUp ? 1 : 0 }));
  }
}

export function resetProfile() {
  profile.set({ ...DEFAULTS });
}

/** Short helper to read without subscribing. */
export function currentProfile(): Profile {
  return get(profile);
}
