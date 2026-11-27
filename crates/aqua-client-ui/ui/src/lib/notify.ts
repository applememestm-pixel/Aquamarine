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
      const { invoke } = await import("@tauri-apps/api/core");
      granted = await invoke<boolean>("notify_permission", { ask });
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
export async function osNotify(title: string, body: string, peer?: string): Promise<void> {
  if (!get(settings).notifyDesktop) return;
  if (!(await ensureNotifyPermission(false))) return;
  try {
    if (inTauri()) {
      if (peer) {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("notify_message", { peer, title, preview: body });
        return;
      }
      const n = await import("@tauri-apps/plugin-notification");
      n.sendNotification({ title, body, autoCancel: true, ...(/android/i.test(navigator.userAgent) ? { icon: "ic_stat_aquamarine" } : {}) });
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
let backAt = -Infinity;
/** Right after coming back, the paused events arrive at once — Rust has
 *  already shown those, so the page doesn't repeat them. */
export function justResumed(): boolean {
  return PHONE && performance.now() - backAt < 3000;
}
export function startBgNotify(): () => void {
  if (!inTauri()) return () => {};
  const send = (cmd: string, args: Record<string, unknown>) =>
    void import("@tauri-apps/api/core").then((c) => c.invoke(cmd, args)).catch(() => {});
  // Background connection, with the line the reminder after a reboot shows,
  // in the UI language.
  let bgSent = "";
  let unBg = () => {};
  let bgDone = false;
  if (PHONE) void import("./i18n").then((i18n) => {
    if (bgDone) return;
    const push = () => {
      const on = get(settings).bgConnection !== false;
      const reminder = i18n.tr("Open the app to receive new messages again");
      if (`${on} ${reminder}` === bgSent) return;
      bgSent = `${on} ${reminder}`;
      send("bg_sync_set", { on, reminder });
    };
    const subs = [settings.subscribe(push), i18n.lang.subscribe(push)];
    unBg = () => subs.forEach((off) => off());
  });
  const reportsFront = PHONE && !/android/i.test(navigator.userAgent);
  const vis = () => {
    const front = document.visibilityState === "visible";
    if (front) backAt = performance.now();
    if (reportsFront) send("notify_front", { front });
  };
  let beat: ReturnType<typeof setInterval> | undefined;
  if (PHONE) {
    document.addEventListener("visibilitychange", vis);
    if (reportsFront) {
      send("notify_front", { front: document.visibilityState === "visible" });
      beat = setInterval(() => {
        if (document.visibilityState === "visible") send("notify_front", { front: true });
      }, 1500);
    }
    if (get(settings).notifyDesktop) void ensureNotifyPermission(true);
  }
  let stopped = false;
  let unsubscribe: Array<() => void> = [];
  void Promise.all([
    import("./chatPrefs"), import("./i18n"), import("./chatLocks"), import("./lock"),
    import("./privacy"), import("./contacts"), import("./wrote"),
  ]).then(([cp, i18n, cl, lock, privacy, contacts, wrote]) => {
    if (stopped) return;
    let queued = false;
    const push = () => {
      if (queued) return;
      queued = true;
      queueMicrotask(() => {
        queued = false;
        if (stopped || !get(lock.lockLoaded) || get(lock.locked)) return;
        const s = get(settings);
        const rules = get(privacy.privacy).messages;
        const allowed = new Set(get(wrote.wroteTo));
        for (const peer of rules.allow) if (!rules.deny.includes(peer)) allowed.add(peer);
        if (rules.who === "contacts") {
          for (const peer of Object.keys(get(contacts.contacts))) if (!rules.deny.includes(peer)) allowed.add(peer);
        }
        send("notify_prefs", { prefs: {
          enabled: !!s.notifyDesktop,
          muted: [...get(cp.mutedChats)],
          locked: Object.keys(get(cl.chatLocks)),
          locked_text: i18n.tr("New message in a locked chat"),
          new_text: i18n.tr("New message"),
          read_text: i18n.tr("Mark as read"),
          reply_text: i18n.tr("Reply"),
          preview: s.notifyPreview ?? "none",
          restricted: rules.who !== "everybody",
          denied: rules.deny,
          allowed: [...allowed],
          archived: [...get(cp.archived)],
          widget_text: s.widgetText !== false,
          widget_empty: i18n.tr("No new messages"),
          call_text: i18n.tr("Incoming call"),
        } });
      });
    };
    unsubscribe = [settings, cp.mutedChats, cp.archived, cl.chatLocks, lock.locked, lock.lockLoaded,
      privacy.privacy, contacts.contacts, wrote.wroteTo, i18n.lang].map((store) => store.subscribe(push));
  });
  return () => {
    stopped = true;
    bgDone = true;
    unBg();
    clearInterval(beat);
    document.removeEventListener("visibilitychange", vis);
    unsubscribe.forEach((un) => un());
  };
}
