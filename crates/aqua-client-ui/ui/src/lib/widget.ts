/**
 * Android home-screen widget: the unread chats — who wrote and what (the
 * text can be turned off: Settings → Notifications → "Message text in the
 * widget"). Chats with their own password show names only.
 * While the page sleeps, src/bgnotify.rs redraws it from Rust.
 */
import { derived } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { chats } from "./stores";
import { mutedChats, archived } from "./chatPrefs";
import { settings } from "./theme";
import { tr } from "./i18n";

const ANDROID = typeof navigator !== "undefined" && /android/i.test(navigator.userAgent);

export function startWidget(): () => void {
  if (!ANDROID) return () => {};
  let timer: ReturnType<typeof setTimeout> | null = null;
  let last = "";
  const src = derived([chats, mutedChats, archived, settings], (x) => x);
  const un = src.subscribe(([list, muted, arch, s]) => {
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => {
      let locked: Record<string, unknown> = {};
      try { locked = JSON.parse(localStorage.getItem("aqua.chatLocks.v1") ?? "{}"); } catch { /* ignore */ }
      const unreadChats = list.filter((c) => c.unread > 0 && !muted.has(c.id) && !arch.has(c.id) && !c.id.startsWith("__"));
      const total = unreadChats.reduce((n, c) => n + c.unread, 0);
      const data = JSON.stringify({
        title: "Aquamarine",
        unread: total,
        empty: tr("No new messages"),
        rows: unreadChats.slice(0, 3).map((c) => ({
          n: c.name,
          p: s.widgetText !== false && !locked[c.id] ? (c.preview ?? "").replace(/\s+/g, " ").slice(0, 80) : "",
          u: c.unread,
        })),
      });
      if (data === last) return;
      last = data;
      void invoke("widget_update", { data }).catch(() => {});
    }, 1500);
  });
  return () => { un(); if (timer) clearTimeout(timer); };
}

export const hasWidget = ANDROID;
