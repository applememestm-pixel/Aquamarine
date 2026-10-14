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
  /** Profile version — bumped on every publish so peers refetch the card. */
  pv?: number;
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

export function updateProfile(patch: Partial<Profile>) {
  profile.update((p) => ({ ...p, ...patch }));
}

export function resetProfile() {
  profile.set({ ...DEFAULTS });
}

/** Short helper to read without subscribing. */
export function currentProfile(): Profile {
  return get(profile);
}
