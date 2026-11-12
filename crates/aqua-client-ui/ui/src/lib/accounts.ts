import { writable } from "svelte/store";
import { api, type AccountSlot } from "./tauri";

/**
 * Several accounts on one device (Rust: accounts.rs). Each one has its own
 * data directory; the webview's localStorage is shared, so the per-account
 * part of it (pins, folders, privacy, drafts, …) is handed over on a switch
 * and the other account's copy comes back. What belongs to the device —
 * the look, the language, the shortcuts — stays as it is.
 */
const DEVICE_KEYS = new Set([
  "aqua-settings", "aqua.lang", "aqua.keybindings.v1", "aqua.kbH", "aqua.listWidth",
  "aqua.hostRelay", "aqua.player.volume", "aqua.player.speed", "aqua.reader.scale", "aqua.servers.v1", "aqua.updates.v1",
  "aqua.acct",
]);

/** Whose the per-account part of localStorage is (an account slot). */
const OWNER = "aqua.acct";

export const accounts = writable<AccountSlot[]>([]);

export async function loadAccounts(): Promise<AccountSlot[]> {
  try { const l = await api.accountsList(); accounts.set(l); return l; } catch { return []; }
}

function snapshot(): string {
  const out: Record<string, string> = {};
  try {
    for (let i = 0; i < localStorage.length; i++) {
      const k = localStorage.key(i);
      if (k && !DEVICE_KEYS.has(k)) out[k] = localStorage.getItem(k) ?? "";
    }
  } catch { /* storage blocked */ }
  return JSON.stringify(out);
}

function restore(store: string | null) {
  let next: Record<string, string> = {};
  try { next = store ? JSON.parse(store) : {}; } catch { /* a broken copy: start clean */ }
  try {
    const drop: string[] = [];
    for (let i = 0; i < localStorage.length; i++) {
      const k = localStorage.key(i);
      if (k && !DEVICE_KEYS.has(k)) drop.push(k);
    }
    for (const k of drop) localStorage.removeItem(k);
    for (const [k, v] of Object.entries(next)) if (!DEVICE_KEYS.has(k)) localStorage.setItem(k, v);
  } catch { /* storage blocked */ }
}

/** Open another account ("new" = sign in to / create one more). The app
 *  starts over in it. */
export async function switchAccount(slot: string): Promise<void> {
  // This page keeps running until the relaunch (and a phone's WebView may
  // not write the storage out in time), so the other account's copy is put
  // in at the next start, by `accountBoot` — not here, where whatever this
  // page still saved landed on top of it (the name and photo of one account
  // ended up on another).
  try { if (!localStorage.getItem(OWNER)) localStorage.setItem(OWNER, (await api.accountsList()).find((a) => a.active)?.slot ?? ""); } catch { /* storage blocked */ }
  await api.accountSwitch(slot, snapshot());
  // The app closes and opens again in that account (where a platform can't
  // reopen itself, the next start does).
  await api.appRelaunch();
}

/** At start, before the app reads anything: if localStorage still holds
 *  another account's things, keep them as that account's, take this one's
 *  and reload. True = reloading, don't start the app. */
export async function accountBoot(): Promise<boolean> {
  let owner: string | null = null;
  try { owner = localStorage.getItem(OWNER); } catch { return false; }
  let r: { slot: string; store: string | null };
  try { r = await api.accountBoot(owner, snapshot()); } catch { return false; /* no accounts here (browser, older build) */ }
  try { localStorage.setItem(OWNER, r.slot); } catch { /* storage blocked */ }
  if (r.store === null) return false;
  restore(r.store);
  location.reload();
  return true;
}
