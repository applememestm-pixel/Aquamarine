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

/**
 * Phones: Android pauses this page in the background, so the Rust side
 * (src/bgnotify.rs) announces messages then. Keep it told whether we're on
 * screen and what the user wants notified.
 */
const PHONE = typeof navigator !== "undefined" && /android|iphone|ipad/i.test(navigator.userAgent);
let backAt = 0;
/** Right after coming back, the paused events arrive at once — Rust has
 *  already shown those, so the page doesn't repeat them. */
export function justResumed(): boolean {
  return PHONE && performance.now() - backAt < 3000;
}
export function startBgNotify(): () => void {
  if (!PHONE || !inTauri()) return () => {};
  const send = (cmd: string, args: Record<string, unknown>) =>
    void import("@tauri-apps/api/core").then((c) => c.invoke(cmd, args)).catch(() => {});
  const vis = () => {
    const front = document.visibilityState === "visible";
    if (front) backAt = performance.now();
    send("notify_front", { front });
  };
  document.addEventListener("visibilitychange", vis);
  vis();
  let un = () => {};
  void Promise.all([import("./chatPrefs"), import("./i18n")]).then(([cp, i18n]) => {
    const push = () => {
      let locked: string[] = [];
      try { locked = Object.keys(JSON.parse(localStorage.getItem("aqua.chatLocks.v1") ?? "{}")); } catch { /* ignore */ }
      send("notify_prefs", { prefs: {
        enabled: !!get(settings).notifyDesktop,
        muted: [...get(cp.mutedChats)],
        locked,
        locked_text: i18n.tr("New message in a locked chat"),
      } });
    };
    const a = settings.subscribe(push), b = cp.mutedChats.subscribe(push);
    window.addEventListener("storage", push);
    un = () => { a(); b(); window.removeEventListener("storage", push); };
  });
  return () => { document.removeEventListener("visibilitychange", vis); un(); };
}
