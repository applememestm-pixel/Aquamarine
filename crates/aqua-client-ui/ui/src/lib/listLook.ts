/**
 * How rows in the chat list look (Settings → Appearance → Chat list):
 * two lines, or three like Telegram — a bigger avatar, the sender of a
 * group's last message on a line of its own, and two lines of the text.
 */
import { writable } from "svelte/store";

const KEY = "aqua.chatLines.v1";
let first: 2 | 3 = 2;
try { if (localStorage.getItem(KEY) === "3") first = 3; } catch { /* storage blocked */ }
export const chatLines = writable<2 | 3>(first);
chatLines.subscribe((v) => { try { localStorage.setItem(KEY, String(v)); } catch { /* ignore */ } });
