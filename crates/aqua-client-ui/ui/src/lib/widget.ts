/**
 * Android home-screen widget (android/widget/AquaWidget.kt): the unread
 * chats. Names and counts only, unless Settings → Notifications → "Message
 * text in the widget" is on; with an app passcode set, only the count.
 */
import { derived } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { chats } from "./stores";
import { mutedChats, archived } from "./chatPrefs";
import { settings } from "./theme";
import { lockConf } from "./lock";
import { tr } from "./i18n";

const ANDROID = typeof navigator !== "undefined" && /android/i.test(navigator.userAgent);

export function startWidget(): () => void {
  if (!ANDROID) return () => {};
  let timer: ReturnType<typeof setTimeout> | null = null;
  let last = "";
  const src = derived([chats, mutedChats, archived, settings, lockConf], (x) => x);
  const un = src.subscribe(([list, muted, arch, s, lock]) => {
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => {
      const unreadChats = list.filter((c) => c.unread > 0 && !muted.has(c.id) && !arch.has(c.id) && !c.id.startsWith("__"));
      const total = unreadChats.reduce((n, c) => n + c.unread, 0);
      const hidden = !!lock?.hash;
      const data = JSON.stringify({
        title: "Aquamarine",
        unread: total,
        empty: hidden && total ? tr("{n} unread", { n: total }) : tr("No new messages"),
        rows: hidden ? [] : unreadChats.slice(0, 3).map((c) => ({
          n: c.name,
          p: s.widgetText ? (c.preview ?? "").replace(/\s+/g, " ").slice(0, 80) : "",
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
