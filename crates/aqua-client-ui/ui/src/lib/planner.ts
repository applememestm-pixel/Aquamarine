/**
 * Shared calendar events and checklists (Telegram-style tasks, with a
 * "k people / n %" threshold) — structured chat messages:
 *
 *   [event:v1]{"id","title","at":<ms>,"dur"?:min,"place"?,"note"?,"remind"?:[min…]}
 *   [tasks:v1]{"id","title","items":[{"id","t"}],"need":{"mode":"all"|"k"|"pct","n"},"seq"?,"open"?}
 *
 * Answers travel as `[ctl:v1]` controls (never stored as messages):
 *   {"t":"rsvp","ev","s":"yes"|"maybe"|"no"}
 *   {"t":"tick","list","item","on":bool}
 *   {"t":"task_add","list","id","text"}
 *
 * Everything else (who answered what, my reminders) is kept on this device.
 */
import { writable, get } from "svelte/store";
import { listen } from "@tauri-apps/api/event";
import { api, sendChatControl } from "./tauri";
import { settings } from "./theme";
import { playSound } from "./sounds";
import { tr } from "./i18n";
import { info } from "./stores";
import { ttlLabel } from "./ttl";

export const ME = "__me__";

export interface EventMsg { id: string; title: string; at: number; dur?: number; place?: string; note?: string; remind?: number[] }
export interface TaskItem { id: string; t: string; by?: string }
export interface TasksMsg { id: string; title: string; items: TaskItem[]; need: { mode: "all" | "k" | "pct"; n: number }; seq?: boolean; open?: boolean }

export type Special = { kind: "event"; ev: EventMsg } | { kind: "tasks"; tl: TasksMsg };

const clip = (s: unknown, n: number) => (typeof s === "string" ? s.slice(0, n) : "");

/** A structured planner message, or null. Fields are checked: peers write them. */
export function parseSpecial(text: string): Special | null {
  try {
    if (text.startsWith("[event:v1]")) {
      const v = JSON.parse(text.slice(10));
      if (typeof v?.id !== "string" || typeof v?.at !== "number" || !isFinite(v.at)) return null;
      return { kind: "event", ev: {
        id: clip(v.id, 40), title: clip(v.title, 120) || "…", at: v.at,
        dur: typeof v.dur === "number" ? Math.max(0, Math.min(24 * 60 * 14, v.dur)) : undefined,
        place: clip(v.place, 200) || undefined, note: clip(v.note, 1000) || undefined,
        remind: Array.isArray(v.remind) ? v.remind.filter((x: unknown) => typeof x === "number" && x >= 0 && x <= 60 * 24 * 30).slice(0, 6) : undefined,
      } };
    }
    if (text.startsWith("[tasks:v1]")) {
      const v = JSON.parse(text.slice(10));
      if (typeof v?.id !== "string" || !Array.isArray(v.items)) return null;
      const mode = v.need?.mode === "k" || v.need?.mode === "pct" ? v.need.mode : "all";
      return { kind: "tasks", tl: {
        id: clip(v.id, 40), title: clip(v.title, 120) || "…",
        items: v.items.filter((i: TaskItem) => typeof i?.id === "string" && typeof i?.t === "string").slice(0, 50).map((i: TaskItem) => ({ id: clip(i.id, 20), t: clip(i.t, 300) })),
        need: { mode, n: Math.max(1, Math.min(mode === "pct" ? 100 : 1000, Number(v.need?.n) || 1)) },
        seq: !!v.seq, open: !!v.open,
      } };
    }
  } catch { /* not ours */ }
  return null;
}

function persisted<T>(key: string, init: T) {
  let v = init;
  try { const raw = localStorage.getItem(key); if (raw) v = JSON.parse(raw); } catch { /* ignore */ }
  const s = writable<T>(v);
  s.subscribe((x) => { try { localStorage.setItem(key, JSON.stringify(x)); } catch { /* ignore */ } });
  return s;
}

/** event id → user → answer (ME = this account). */
export const rsvps = persisted<Record<string, Record<string, "yes" | "maybe" | "no">>>("aqua.rsvp.v1", {});
/** event id → my reminder offsets in minutes. */
export const myReminders = persisted<Record<string, number[]>>("aqua.remind.v1", {});
/** Events I know about (for reminders even when the chat is closed). */
export const knownEvents = persisted<Record<string, EventMsg & { chat: string }>>("aqua.events.v1", {});
const fired = persisted<Record<string, number>>("aqua.remindFired.v1", {});
/** list id → item id → users who ticked it; plus items people added. */
export const taskState = persisted<Record<string, { ticks: Record<string, string[]>; added: TaskItem[] }>>("aqua.tasks.v1", {});

export const REMIND_PRESETS = [5, 15, 60, 24 * 60] as const;
export const DEFAULT_REMIND = [60, 5];

export function remindLabel(min: number): string {
  if (min === 0) return tr("At the start");
  if (min % (24 * 60) === 0) return tr("{n} d before", { n: min / (24 * 60) });
  if (min % 60 === 0) return tr("{n} h before", { n: min / 60 });
  return tr("{n} min before", { n: min });
}

export function newId(): string {
  return Date.now().toString(36) + Math.random().toString(36).slice(2, 7);
}

export function remember(chat: string, ev: EventMsg) {
  const cur = get(knownEvents)[ev.id];
  if (cur && cur.at === ev.at && cur.title === ev.title && cur.chat === chat) return;
  knownEvents.update((k) => {
    const n = { ...k, [ev.id]: { ...ev, chat } };
    // Forget events that ended more than a month ago.
    const old = Date.now() - 30 * 86400_000;
    for (const [id, e] of Object.entries(n)) if (e.at < old) delete n[id];
    return n;
  });
  // A new / moved event may be due right away.
  if (started) setTimeout(check, 300);
}

export function remindersFor(ev: EventMsg): number[] {
  return get(myReminders)[ev.id] ?? ev.remind ?? DEFAULT_REMIND;
}

export async function answer(chat: string, ev: EventMsg, s: "yes" | "maybe" | "no") {
  rsvps.update((r) => ({ ...r, [ev.id]: { ...(r[ev.id] ?? {}), [ME]: s } }));
  remember(chat, ev);
  await sendChatControl(chat, "[ctl:v1]" + JSON.stringify({ t: "rsvp", ev: ev.id, s }));
}

export async function tick(chat: string, list: string, item: string, on: boolean) {
  setTick(list, item, ME, on);
  await sendChatControl(chat, "[ctl:v1]" + JSON.stringify({ t: "tick", list, item, on }));
}

export async function addTask(chat: string, list: string, text: string) {
  const it: TaskItem = { id: newId(), t: text.slice(0, 300), by: ME };
  taskState.update((s) => {
    const cur = s[list] ?? { ticks: {}, added: [] };
    return { ...s, [list]: { ...cur, added: [...cur.added, it] } };
  });
  await sendChatControl(chat, "[ctl:v1]" + JSON.stringify({ t: "task_add", list, id: it.id, text: it.t }));
}

function setTick(list: string, item: string, who: string, on: boolean) {
  taskState.update((s) => {
    const cur = s[list] ?? { ticks: {}, added: [] };
    const set = new Set(cur.ticks[item] ?? []);
    if (on) set.add(who); else set.delete(who);
    return { ...s, [list]: { ...cur, ticks: { ...cur.ticks, [item]: [...set] } } };
  });
}

/** How many ticks finish an item, for `members` people in the chat. */
export function needed(tl: TasksMsg, members: number): number {
  const m = Math.max(1, members);
  if (tl.need.mode === "k") return Math.min(m, tl.need.n);
  if (tl.need.mode === "pct") return Math.max(1, Math.ceil((m * tl.need.n) / 100));
  return m;
}

/** A control from someone (or from my other device: `own`). */
function onCtl(p: { from: string; chat: string; body: Record<string, unknown>; own?: boolean }) {
  const b = p.body ?? {};
  const me = get(info)?.username;
  const who = p.own || p.from === me ? ME : p.from;
  switch (b.t) {
    case "rsvp": {
      const ev = clip(b.ev, 40), s = b.s;
      if (!ev || (s !== "yes" && s !== "maybe" && s !== "no")) return;
      rsvps.update((r) => ({ ...r, [ev]: { ...(r[ev] ?? {}), [who]: s } }));
      break;
    }
    case "tick": {
      const list = clip(b.list, 40), item = clip(b.item, 20);
      if (list && item) setTick(list, item, who, !!b.on);
      break;
    }
    case "task_add": {
      const list = clip(b.list, 40), id = clip(b.id, 20), text = clip(b.text, 300);
      if (!list || !id || !text) return;
      taskState.update((s) => {
        const cur = s[list] ?? { ticks: {}, added: [] };
        if (cur.added.some((x) => x.id === id)) return s;
        return { ...s, [list]: { ...cur, added: [...cur.added, { id, t: text, by: who }] } };
      });
      break;
    }
  }
}

/** Reminder pop-ups waiting to be shown. */
export interface Reminder { key: string; chat: string; ev: EventMsg; min: number }
export const reminders = writable<Reminder[]>([]);

function check() {
  const now = Date.now();
  const done = get(fired);
  const due: Reminder[] = [];
  for (const ev of Object.values(get(knownEvents))) {
    if (get(rsvps)[ev.id]?.[ME] === "no") continue;
    for (const min of remindersFor(ev)) {
      const key = `${ev.id}:${min}`;
      const at = ev.at - min * 60_000;
      // Due now (or within the last 10 minutes, if the app was closed).
      if (!done[key] && now >= at && now - at < 10 * 60_000 && now < ev.at + 10 * 60_000) due.push({ key, chat: ev.chat, ev, min });
    }
  }
  if (!due.length) return;
  fired.update((f) => {
    const n = { ...f };
    for (const d of due) n[d.key] = now;
    for (const [k, t] of Object.entries(n)) if (now - t > 40 * 86400_000) delete n[k];
    return n;
  });
  reminders.update((r) => [...r, ...due].slice(-6));
  void playSound(get(settings).reminderSound ?? "alarm");
  if (get(settings).notifyDesktop) {
    const d = due[0];
    try { new Notification(d.ev.title, { body: d.min ? tr("Starts in {t}", { t: ttlLabel(d.min * 60) }) : tr("Starting now") }); } catch { /* ignore */ }
  }
}

let started = false;
/** Listen for answers and start the reminder clock (once). */
export function startPlanner() {
  if (started) return;
  started = true;
  void listen<{ from: string; chat: string; body: Record<string, unknown>; own?: boolean }>("aqua://ctl", (e) => onCtl(e.payload)).catch(() => {});
  // New events arriving in any chat are remembered for reminders.
  void listen<{ peer_id: string; text: string }>("aqua://incoming", (e) => {
    const sp = e.payload.text ? parseSpecial(e.payload.text) : null;
    if (sp?.kind === "event") remember(e.payload.peer_id, sp.ev);
  }).catch(() => {});
  check();
  setInterval(check, 15_000);
  // Chats whose preview is an event: learn their events once at start.
  setTimeout(async () => {
    try {
      const chats = await api.listChats();
      for (const c of chats.filter((c) => c.preview.startsWith("📅"))) {
        for (const m of await api.getMessages(c.id)) {
          if (m.kind.type === "text") { const sp = parseSpecial(m.kind.content.text); if (sp?.kind === "event") remember(c.id, sp.ev); }
        }
      }
    } catch { /* ignore */ }
  }, 4000);
}

/** iCalendar text for an event (to put it in the system calendar). */
export function toIcs(ev: EventMsg): string {
  const f = (t: number) => new Date(t).toISOString().replace(/[-:]/g, "").replace(/\.\d{3}/, "");
  const esc = (s: string) => s.replace(/[\\;,]/g, (c) => "\\" + c).replace(/\n/g, "\\n");
  return [
    "BEGIN:VCALENDAR", "VERSION:2.0", "PRODID:-//Aquamarine//EN", "BEGIN:VEVENT",
    `UID:${ev.id}@aquamarine`, `DTSTAMP:${f(Date.now())}`, `DTSTART:${f(ev.at)}`, `DTEND:${f(ev.at + (ev.dur ?? 60) * 60_000)}`,
    `SUMMARY:${esc(ev.title)}`, ev.place ? `LOCATION:${esc(ev.place)}` : "", ev.note ? `DESCRIPTION:${esc(ev.note)}` : "",
    ...remindersFor(ev).map((m) => `BEGIN:VALARM\r\nACTION:DISPLAY\r\nDESCRIPTION:${esc(ev.title)}\r\nTRIGGER:-PT${m}M\r\nEND:VALARM`),
    "END:VEVENT", "END:VCALENDAR",
  ].filter(Boolean).join("\r\n");
}
