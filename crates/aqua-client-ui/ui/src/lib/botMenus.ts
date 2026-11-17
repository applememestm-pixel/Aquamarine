/**
 * Bots with a mini app behind their menu button: remembered when their chat
 * was opened, so the chat list can show Telegram's "Open" button on the
 * row and start the mini app without opening the chat.
 */
import { writable } from "svelte/store";

const KEY = "aqua.botMenus.v1";
export interface BotMenu { text: string; url: string }
let first: Record<string, BotMenu> = {};
try {
  const raw = JSON.parse(localStorage.getItem(KEY) ?? "{}");
  if (raw && typeof raw === "object") for (const [k, v] of Object.entries(raw)) {
    const m = v as BotMenu;
    if (typeof m?.url === "string" && /^(https:\/\/|aqua:relay\/)/.test(m.url)) first[k] = { text: String(m.text ?? "").slice(0, 32), url: m.url };
  }
} catch { first = {}; }
export const botMenus = writable<Record<string, BotMenu>>(first);
botMenus.subscribe((v) => { try { localStorage.setItem(KEY, JSON.stringify(v)); } catch { /* ignore */ } });

export function rememberBotMenu(bot: string, m: BotMenu | null) {
  botMenus.update((all) => {
    const cur = all[bot];
    if (!m) { if (!cur) return all; const n = { ...all }; delete n[bot]; return n; }
    if (cur && cur.url === m.url && cur.text === m.text) return all;
    return { ...all, [bot]: m };
  });
}
