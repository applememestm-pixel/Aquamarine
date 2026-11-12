/**
 * One account, the same setup on every device of it.
 *
 * - Settings that belong to the account (look, language, privacy, folders,
 *   chat themes, blocked people…) go to my other devices whenever they
 *   change here; the newest change wins (`at`).
 * - A new device asks for them once (`prefs_req`) and takes the main
 *   device's (or any newer) copy.
 * - My profile lives on the relay as my card (people.ts): after an edit
 *   here my other devices are told to fetch it again (`card`), instead of
 *   waiting for their next reconnect.
 *
 * What only makes sense on one device stays there: UI scale, text size,
 * Enter-to-send, tray, a wallpaper / font / icon file picked from its disk.
 * Everything rides `[ctl:v1]` to my own devices (multidev::sync_own),
 * end-to-end encrypted.
 */
import { get, type Writable } from "svelte/store";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { api } from "./tauri";
import { info } from "./stores";
import { settings, type Settings } from "./theme";
import { lang } from "./i18n";
import { privacy } from "./privacy";
import { folders, folderTags } from "./folders";
import { chatThemes } from "./chatTheme";
import { blocked, noReceiptChats, ghost } from "./ghost";
import { profile } from "./profile";
import { adoptMyCard } from "./people";

/** Fields of the settings that stay on this device. */
const DEVICE_FIELDS: (keyof Settings)[] = [
  "uiScale", "fontScale", "enterToSend", "wallpaperPath", "wallpaperCustom", "customFontName", "customFontData", "appIcon",
  "tray", "trayMono", "restoreWindows", "askDownloadPath", "power", "powerMode", "widgetText", "notifyDesktop",
  "titleChatName", "titleAccount", "titleUnread",
];

interface Entry {
  key: string;
  store: Writable<any>;
  /** Store value → what goes out. */
  out: (v: any) => unknown;
  /** What came in (+ the current value) → the new store value; undefined = keep. */
  take: (v: unknown, cur: any) => any;
}

const plainObj = (v: unknown) => (v && typeof v === "object" && !Array.isArray(v) ? v : undefined);
const strArr = (v: unknown) => (Array.isArray(v) ? v.filter((x) => typeof x === "string") : undefined);

const ENTRIES: Entry[] = [
  {
    key: "settings", store: settings,
    out: (s: Settings) => {
      const o: Record<string, unknown> = { ...s };
      for (const f of DEVICE_FIELDS) delete o[f];
      // A wallpaper picked from this device's disk isn't on the others.
      if (s.wallpaper === "custom") delete o.wallpaper;
      return o;
    },
    take: (v, cur: Settings) => {
      const o = plainObj(v) as Record<string, unknown> | undefined;
      if (!o) return undefined;
      const next: Record<string, unknown> = { ...cur, ...o };
      for (const f of DEVICE_FIELDS) next[f] = (cur as unknown as Record<string, unknown>)[f];
      if (next.wallpaper === "custom" && !cur.wallpaperPath && !cur.wallpaperCustom) next.wallpaper = cur.wallpaper === "custom" ? "aurora" : cur.wallpaper;
      return next;
    },
  },
  { key: "lang", store: lang, out: (v) => v, take: (v) => (v === "en" || v === "ru" ? v : undefined) },
  { key: "privacy", store: privacy, out: (v) => v, take: (v, cur) => (plainObj(v) ? { ...cur, ...(v as object) } : undefined) },
  { key: "folders", store: folders, out: (v) => v, take: (v) => (Array.isArray(v) ? v.filter((f) => f && typeof f.id === "string") : undefined) },
  { key: "folderTags", store: folderTags, out: (v) => v, take: (v) => (typeof v === "boolean" ? v : undefined) },
  { key: "chatThemes", store: chatThemes, out: (v) => v, take: (v) => plainObj(v) },
  { key: "blocked", store: blocked, out: (v: Set<string>) => [...v], take: (v) => { const a = strArr(v); return a ? new Set(a) : undefined; } },
  { key: "noReceipts", store: noReceiptChats, out: (v: Set<string>) => [...v], take: (v) => { const a = strArr(v); return a ? new Set(a) : undefined; } },
  { key: "ghost", store: ghost, out: (v) => v, take: (v, cur) => (plainObj(v) ? { ...cur, ...(v as object) } : undefined) },
];

/** When the account's settings last changed (ms; 0 = never synced here). */
const AT_KEY = "aqua.prefsSync.v1";
function loadAt(): number { try { return Number(localStorage.getItem(AT_KEY)) || 0; } catch { return 0; } }
function saveAt(v: number) { try { localStorage.setItem(AT_KEY, String(v)); } catch { /* ignore */ } }

let at = 0;
let applying = false;
let snap = "";
let timer: ReturnType<typeof setTimeout> | null = null;

function collect(): Record<string, unknown> {
  const o: Record<string, unknown> = {};
  for (const e of ENTRIES) o[e.key] = e.out(get(e.store));
  return o;
}

function send(body: Record<string, unknown>) {
  if (!get(info)?.connected) return;
  void invoke("sync_own", { body: JSON.stringify(body) }).catch(() => {});
}

function sendPrefs(stamp = at) {
  send({ t: "prefs", at: stamp, data: collect() });
}

/** Something changed here (not from another device): stamp it, send it soon. */
function changed() {
  if (applying) return;
  const s = JSON.stringify(collect());
  if (s === snap) return;
  snap = s;
  at = Date.now();
  saveAt(at);
  if (timer) clearTimeout(timer);
  timer = setTimeout(() => { timer = null; sendPrefs(); }, 1500);
}

function apply(data: Record<string, unknown>, stamp: number) {
  applying = true;
  try {
    for (const e of ENTRIES) {
      if (!(e.key in data)) continue;
      try {
        const v = e.take(data[e.key], get(e.store));
        if (v !== undefined) e.store.set(v);
      } catch { /* one bad value doesn't stop the rest */ }
    }
  } finally {
    applying = false;
  }
  snap = JSON.stringify(collect());
  at = stamp;
  saveAt(at);
}

/** Is this the account's main device (the first one, plain `user` address)? */
async function isMain(): Promise<boolean> {
  try { return !!(await api.listDevices()).find((d) => d.current)?.primary; } catch { return false; }
}

const me = () => get(info)?.username ?? "";

/** Taking my card from the relay (another device's edit): not an edit of mine. */
let adopting = false;
async function adoptCard() {
  const u = me();
  if (!u) return;
  adopting = true;
  try { await adoptMyCard(u); } finally { adopting = false; }
}

let started = false;
let wasOn = false;
export function startOwnSync() {
  if (started) return;
  started = true;
  at = loadAt();
  snap = JSON.stringify(collect());
  for (const e of ENTRIES) e.store.subscribe(() => changed());

  // My card: an edit made here, once published (`pv` moves after every
  // successful publish) → my other devices fetch it.
  let lastEdited = get(profile).edited ?? 0;
  let lastPv = get(profile).pv ?? 0;
  let editPending = false;
  profile.subscribe((p) => {
    if (adopting) { lastEdited = p.edited ?? 0; lastPv = p.pv ?? 0; return; }
    if ((p.edited ?? 0) > lastEdited) { lastEdited = p.edited ?? 0; editPending = true; }
    if ((p.pv ?? 0) !== lastPv) {
      lastPv = p.pv ?? 0;
      if (editPending) { editPending = false; send({ t: "card" }); }
    }
  });

  // Connected: a device that never got the account's settings asks for them.
  info.subscribe((i) => {
    const on = !!i?.connected;
    if (on && !wasOn) {
      setTimeout(async () => {
        if (!get(info)?.connected) return;
        if (at === 0 && !(await isMain())) send({ t: "prefs_req" });
        else if (at > 0) sendPrefs();
      }, 2500);
    }
    wasOn = on;
  });

  void listen<{ chat: string; body: Record<string, unknown>; own?: boolean }>("aqua://ctl", async (ev) => {
    const b = ev.payload.body;
    if (!ev.payload.own || !b || typeof b.t !== "string") return;
    if (b.t === "prefs_req") {
      // The main device answers even before anything was ever changed (its
      // setup is the account's); others only with a real change of their own.
      if (at > 0) sendPrefs();
      else if (await isMain()) sendPrefs(1);
      return;
    }
    if (b.t === "prefs") {
      const stamp = Number(b.at) || 0;
      const data = plainObj(b.data) as Record<string, unknown> | undefined;
      if (!data || stamp <= at) return;
      apply(data, stamp);
      // A new device: the profile card too.
      void adoptCard();
      return;
    }
    if (b.t === "card") void adoptCard();
  }).catch(() => {});
}
