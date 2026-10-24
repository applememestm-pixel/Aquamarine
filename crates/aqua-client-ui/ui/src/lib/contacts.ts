/**
 * Local contact book — extra per-peer metadata the backend doesn't
 * know about: full name (ФИО) and phone number. Keyed by username
 * (the peer_id the backend uses for routing).
 *
 * Lives in localStorage for now, which is per-vault-directory in
 * practice because each instance has its own webview profile. When
 * registration-by-email lands we'll move this into the encrypted
 * vault alongside the sessions table.
 */
import { writable, get } from "svelte/store";

export interface Contact {
  full_name?: string;
  phone?: string;
}

const KEY = "aqua-contacts";

function load(): Record<string, Contact> {
  try {
    return JSON.parse(localStorage.getItem(KEY) ?? "{}");
  } catch {
    return {};
  }
}

export const contacts = writable<Record<string, Contact>>(load());

contacts.subscribe((v) => {
  try {
    localStorage.setItem(KEY, JSON.stringify(v));
  } catch {
    /* private mode / blocked storage — fine, we just lose metadata */
  }
});

/** Save (or update) one contact. Empty strings are stripped so an
 *  entry with no useful fields disappears from the book. */
export function saveContact(username: string, info: Contact) {
  const u = username.trim();
  if (!u) return;
  const clean: Contact = {};
  if (info.full_name?.trim()) clean.full_name = info.full_name.trim();
  if (info.phone?.trim())     clean.phone     = info.phone.trim();
  contacts.update((all) => {
    if (Object.keys(clean).length === 0) {
      const { [u]: _, ...rest } = all;
      return rest;
    }
    return { ...all, [u]: clean };
  });
}

export function getContact(username: string): Contact | undefined {
  return get(contacts)[username.trim()];
}

/**
 * Return the best display name for a chat / peer. Falls back to the
 * backend-supplied name (usually the username) if we have no stored
 * contact.
 */
export function displayName(username: string, fallback: string): string {
  const c = get(contacts)[username.trim()];
  return c?.full_name && c.full_name.length > 0 ? c.full_name : fallback;
}

/**
 * Case-insensitive substring match against any of a contact's
 * fields (full_name, phone, username). Powers the global search bar.
 */
export function contactMatches(username: string, query: string): boolean {
  const q = query.trim().replace(/^@+/, "").toLowerCase();
  if (!q) return true;
  if (username.toLowerCase().includes(q)) return true;
  const c = get(contacts)[username.trim()];
  if (!c) return false;
  if (c.full_name && c.full_name.toLowerCase().includes(q)) return true;
  if (c.phone     && c.phone.toLowerCase().includes(q))     return true;
  return false;
}
