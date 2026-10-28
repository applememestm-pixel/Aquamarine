/**
 * The phone's bottom tab bar: which tabs, in what order, whether it hides
 * while scrolling, or no bar at all (swipe between tabs then; Settings is
 * the gear at the top of the chat list). Settings → Appearance → Tab bar.
 */
import { writable } from "svelte/store";

export type NavId = "chats" | "files" | "contacts" | "settings";
export const NAV_ITEMS: { id: NavId; label: string }[] = [
  { id: "chats", label: "Chats" },
  { id: "files", label: "Files" },
  { id: "contacts", label: "Contacts" },
  { id: "settings", label: "Settings" },
];
const isNav = (x: unknown): x is NavId => NAV_ITEMS.some((i) => i.id === x);

export interface NavConf { order: NavId[]; hidden: NavId[]; on: boolean; autoHide: boolean; labels: boolean }
const DEFAULT: NavConf = { order: ["chats", "files", "contacts", "settings"], hidden: [], on: true, autoHide: true, labels: true };
const KEY = "aqua.navBar.v1";

function load(): NavConf {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) || "null") as Partial<NavConf> | null;
    if (v && typeof v === "object") {
      const order = (Array.isArray(v.order) ? v.order.filter(isNav) : []) as NavId[];
      for (const it of NAV_ITEMS) if (!order.includes(it.id)) order.push(it.id);
      return {
        order,
        // Chats can't be hidden: it's where the app starts.
        hidden: (Array.isArray(v.hidden) ? v.hidden.filter(isNav) : []).filter((x) => x !== "chats") as NavId[],
        on: v.on !== false, autoHide: v.autoHide !== false, labels: v.labels !== false,
      };
    }
  } catch { /* storage blocked */ }
  return { ...DEFAULT, order: [...DEFAULT.order] };
}

export const navConf = writable<NavConf>(load());
navConf.subscribe((v) => { try { localStorage.setItem(KEY, JSON.stringify(v)); } catch { /* ignore */ } });

export function moveNav(id: NavId, dir: -1 | 1) {
  navConf.update((c) => {
    const i = c.order.indexOf(id), j = i + dir;
    if (i < 0 || j < 0 || j >= c.order.length) return c;
    const order = [...c.order];
    [order[i], order[j]] = [order[j], order[i]];
    return { ...c, order };
  });
}
export function toggleNavHidden(id: NavId) {
  if (id === "chats") return;
  navConf.update((c) => ({ ...c, hidden: c.hidden.includes(id) ? c.hidden.filter((x) => x !== id) : [...c.hidden, id] }));
}
export function resetNav() { navConf.set({ ...DEFAULT, order: [...DEFAULT.order] }); }
