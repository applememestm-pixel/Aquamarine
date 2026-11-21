/**
 * Android home-screen widget: the unread chats — who wrote and what (the
 * text can be turned off: Settings → Notifications → "Message text in the
 * widget"). Protected chats hide the sender and message.
 * While the page sleeps, src/bgnotify.rs redraws it from Rust.
 */
import { derived } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { chats } from "./stores";
import { mutedChats, archived } from "./chatPrefs";
import { settings } from "./theme";
import { tr } from "./i18n";
import { chatLocks } from "./chatLocks";
import { locked } from "./lock";

const ANDROID = typeof navigator !== "undefined" && /android/i.test(navigator.userAgent);

export function startWidget(): () => void {
  if (!ANDROID) return () => {};
  let timer: ReturnType<typeof setTimeout> | null = null;
  let last = "";
  const src = derived([chats, mutedChats, archived, settings, chatLocks, locked], (x) => x);
  const un = src.subscribe(([list, muted, arch, s, locks, appLocked]) => {
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => {
      const unreadChats = list.filter((c) => c.unread > 0 && !muted.has(c.id) && !arch.has(c.id) && !c.id.startsWith("__"));
      const total = unreadChats.reduce((n, c) => n + c.unread, 0);
      const data = JSON.stringify({
        title: "Aquamarine",
        unread: total,
        empty: tr("No new messages"),
        rows: unreadChats.slice(0, appLocked ? 0 : 3).map((c) => ({
          n: locks[c.id] ? tr("New message in a locked chat") : c.name,
          p: s.widgetText !== false && !locks[c.id] ? (c.preview ?? "").replace(/\s+/g, " ").slice(0, 80) : "",
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
