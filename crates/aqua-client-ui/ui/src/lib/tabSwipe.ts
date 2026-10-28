/**
 * Phones, like Telegram: a sideways swipe over the chat list goes to the
 * next / previous folder; with no folders (and on Files / Contacts) it goes
 * to the next / previous tab of the bottom bar. Swipes that start on
 * something that scrolls sideways itself (the folder strip, a carousel),
 * on a field, or with a chat open are left alone.
 */
import { get } from "svelte/store";
import { activeTab, activeId, type NavTab } from "./stores";
import { folders, activeFolder } from "./folders";

const TABS: NavTab[] = ["chats", "files", "contacts"];

export function startTabSwipe(): () => void {
  if (!/android|iphone|ipad/i.test(navigator.userAgent) && !matchMedia("(pointer: coarse)").matches) return () => {};
  let start: { x: number; y: number; t: number } | null = null;
  const skip = (el: Element | null) =>
    !!el?.closest("input, textarea, [contenteditable], .folder-strip, [data-hscroll], [role=dialog], .main-pane, .rail, .settings-open");
  const down = (e: TouchEvent) => {
    if (e.touches.length !== 1 || get(activeId) || matchMedia("(min-width: 721px)").matches) { start = null; return; }
    const tg = e.target as Element | null;
    if (skip(tg) || document.body.classList.contains("settings-open")) { start = null; return; }
    start = { x: e.touches[0].clientX, y: e.touches[0].clientY, t: performance.now() };
  };
  const up = (e: TouchEvent) => {
    if (!start) return;
    const p = e.changedTouches[0];
    const dx = p.clientX - start.x, dy = p.clientY - start.y, dt = performance.now() - start.t;
    start = null;
    if (Math.abs(dx) < 70 || Math.abs(dx) < Math.abs(dy) * 1.8 || dt > 700) return;
    const dir = dx < 0 ? 1 : -1;
    const tab = get(activeTab);
    const fl = get(folders);
    if (tab === "chats" && fl.length) {
      const ids = ["all", ...fl.map((f) => f.id)];
      const i = ids.indexOf(get(activeFolder));
      const j = i + dir;
      if (j >= 0 && j < ids.length) { activeFolder.set(ids[j]); return; }
    }
    const t = TABS.indexOf(tab === "calls" ? "contacts" : tab);
    const k = t + dir;
    if (k >= 0 && k < TABS.length) activeTab.set(TABS[k]);
  };
  document.addEventListener("touchstart", down, { passive: true });
  document.addEventListener("touchend", up, { passive: true });
  return () => {
    document.removeEventListener("touchstart", down);
    document.removeEventListener("touchend", up);
  };
}
