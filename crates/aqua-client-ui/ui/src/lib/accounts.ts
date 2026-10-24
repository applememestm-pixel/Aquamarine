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
  "aqua.hostRelay", "aqua.player.volume", "aqua.player.speed", "aqua.reader.scale", "aqua.servers.v1",
]);

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
  const r = await api.accountSwitch(slot, snapshot());
  restore(r.store);
  // The app closes and opens again in that account (where a platform can't
  // reopen itself, the next start does).
  await api.appRelaunch();
}
