/** Tray icon, launch at start-up, restoring windows, the storage limits —
 *  started once by the main window. */
import { get } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { settings } from "./theme";
import { chats } from "./stores";
import { mutedChats } from "./chatPrefs";
import { restorePopouts } from "./popouts";

const DESKTOP = typeof navigator !== "undefined" && !/android|iphone|ipad/i.test(navigator.userAgent) && "__TAURI_INTERNALS__" in window;

/** "Launch Aquamarine when the system starts" — kept by the OS, read back here. */
export async function autostartEnabled(): Promise<boolean> {
  if (!DESKTOP) return false;
  try { const a = await import("@tauri-apps/plugin-autostart"); return await a.isEnabled(); } catch { return false; }
}
export async function setAutostart(on: boolean): Promise<boolean> {
  if (!DESKTOP) return false;
  const a = await import("@tauri-apps/plugin-autostart");
  if (on) await a.enable(); else await a.disable();
  return await a.isEnabled();
}

let started = false;
export function initDesktop() {
  if (started || document.documentElement.classList.contains("popout")) return;
  started = true;
  void invoke("storage_trim").catch(() => {});
  if (!DESKTOP) return;
  let last = "";
  const sync = () => {
    const s = get(settings);
    const unread = get(chats).reduce((n, c) => n + (c.channel || get(mutedChats).has(c.id) ? 0 : c.unread), 0);
    const key = `${!!s.tray}|${!!s.trayMono}|${s.tray ? unread : 0}`;
    if (key === last) return;
    last = key;
    void invoke("tray_set", { on: !!s.tray, mono: !!s.trayMono, unread }).catch(() => {});
  };
  settings.subscribe(sync);
  chats.subscribe(sync);
  setTimeout(restorePopouts, 1500);
}
