/**
 * "Disable sharing" in a 1:1 chat (Telegram Tips, 1.2.0-4): either person
 * turns it on, and for both of them messages of the chat can't be
 * forwarded, copied or saved (the same rule groups have as
 * `protected_content`). Travels as `[ctl:v1]{"t":"noshare","on":bool}`
 * to the other person and to my other devices.
 *
 * It's a promise between two apps, not DRM: a modified app or a camera
 * can still copy — the UI says so.
 */
import { writable, get } from "svelte/store";
import { listen } from "@tauri-apps/api/event";
import { sendChatControl } from "./tauri";

const KEY = "aqua.noShare";
function load(): Set<string> {
  try { return new Set(JSON.parse(localStorage.getItem(KEY) ?? "[]")); } catch { return new Set(); }
}
export const noShare = writable<Set<string>>(load());
noShare.subscribe((v) => { try { localStorage.setItem(KEY, JSON.stringify([...v])); } catch { /* ignore */ } });

function put(chat: string, on: boolean) {
  noShare.update((s) => { const n = new Set(s); if (on) n.add(chat); else n.delete(chat); return n; });
}

/** Turn it on / off for both people of a 1:1 chat. */
export async function setNoShare(chat: string, on: boolean) {
  put(chat, on);
  await sendChatControl(chat, "[ctl:v1]" + JSON.stringify({ t: "noshare", on, at: Date.now() }));
}

export const isNoShare = (chat: string) => get(noShare).has(chat);

let started = false;
export function startNoShare() {
  if (started) return;
  started = true;
  void listen<{ chat: string; body: Record<string, unknown> }>("aqua://ctl", (e) => {
    const b = e.payload.body;
    if (b?.t !== "noshare" || e.payload.chat.startsWith("grp_")) return;
    put(e.payload.chat, b.on === true);
  }).catch(() => {});
}
