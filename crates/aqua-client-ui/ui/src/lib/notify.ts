/**
 * System notifications through the app (tauri-plugin-notification): the
 * webviews' own `Notification` is missing on Android and unreliable on
 * Linux and Windows. Falls back to it outside Tauri (dev in a browser).
 */
import { get } from "svelte/store";
import { settings } from "./theme";

const inTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
let granted: boolean | null = null;

/** Ask the OS once; true when notifications may be shown. */
export async function ensureNotifyPermission(ask = true): Promise<boolean> {
  if (granted) return true;
  try {
    if (inTauri()) {
      const n = await import("@tauri-apps/plugin-notification");
      granted = await n.isPermissionGranted();
      if (!granted && ask) granted = (await n.requestPermission()) === "granted";
      return !!granted;
    }
    if ("Notification" in window) {
      if (Notification.permission === "granted") return (granted = true);
      if (ask && Notification.permission !== "denied") granted = (await Notification.requestPermission()) === "granted";
      return !!granted;
    }
  } catch (e) {
    console.warn("[aqua] notifications:", e);
  }
  return false;
}

/** Show a notification if the user wants them and the OS allows. */
export async function osNotify(title: string, body: string): Promise<void> {
  if (!get(settings).notifyDesktop) return;
  if (!(await ensureNotifyPermission(false))) return;
  try {
    if (inTauri()) {
      const n = await import("@tauri-apps/plugin-notification");
      n.sendNotification({ title, body });
    } else {
      new Notification(title, { body });
    }
  } catch (e) {
    console.warn("[aqua] notify:", e);
  }
}

/** Is the user looking at the app right now? */
export function appInFront(): boolean {
  return document.visibilityState === "visible" && document.hasFocus();
}
