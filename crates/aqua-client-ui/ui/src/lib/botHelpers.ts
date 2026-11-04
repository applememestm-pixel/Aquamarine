/**
 * Bots in other chats (Telegram Tips, 1.2.0-4).
 *
 * Guest bots: mention a bot (`@weatherbot what's up?`) in a group or a 1:1
 * chat, and that one message — only it — goes to the bot. The bot's answer
 * comes back here and is posted into the chat it was asked in, marked as
 * the bot's ("via @weatherbot"), so everyone there sees it. The bot never
 * sees who else is in the chat or any other message.
 *
 * Bot secretary: a bot of my choice answers people for me — all of them,
 * only new chats, or only people who aren't in my contacts. Their message
 * goes to the bot as `[from @alice]\n<text>`, and the bot's answer is sent
 * to them from my account.
 *
 * Bots aren't end-to-end encrypted: whatever is handed to one, its owner
 * and the server can read. The settings say so before the secretary is on.
 */
import { writable, get } from "svelte/store";
import { api, events } from "./tauri";
import { peerCards, refreshPeer } from "./people";
import { getContact } from "./contacts";

const WAIT_MS = 2 * 60_000;

/** bot → where its next answer goes. */
const guestWaits = new Map<string, { chat: string; at: number }>();
const secretaryWaits = new Map<string, { to: string; at: number }>();

export interface SecretaryConf { bot: string; scope: "all" | "new" | "noncontacts" }
const KEY = "aqua.botSecretary";
function loadConf(): SecretaryConf | null {
  try { const v = JSON.parse(localStorage.getItem(KEY) ?? "null"); return v?.bot ? v : null; } catch { return null; }
}
export const secretary = writable<SecretaryConf | null>(loadConf());
secretary.subscribe((v) => { try { if (v) localStorage.setItem(KEY, JSON.stringify(v)); else localStorage.removeItem(KEY); } catch { /* ignore */ } });

async function isBot(name: string): Promise<boolean> {
  if (!get(peerCards)[name]) { try { await refreshPeer(name); } catch { /* offline */ } }
  return !!get(peerCards)[name]?.is_bot;
}

const BOT_ANSWER = /^\[fwd:v1\]\{[^\n]*"kind":"guest"/;

/** After sending `text` to `chat`: hand it to every bot it mentions. */
export async function routeGuestMentions(chat: string, text: string) {
  if (BOT_ANSWER.test(text)) return;
  const names = [...new Set([...text.matchAll(/(?:^|[^\w@])@([a-z0-9_]{3,32})/gi)].map((m) => m[1].toLowerCase()))].slice(0, 3);
  for (const n of names) {
    if (n === chat || !(await isBot(n))) continue;
    try {
      await api.botHandOver(n, text);
      guestWaits.set(n, { chat, at: Date.now() });
    } catch (e) { console.warn("guest bot", n, e); }
  }
}

function sendTo(chat: string, text: string) {
  return chat.startsWith("grp_") ? api.groupSend(chat, text) : api.sendMessage(chat, text);
}

let started = false;
export function startBotHelpers() {
  if (started) return;
  started = true;
  void events.onIncoming(async (p) => {
    const from = p.peer_id;
    const text = (p.text ?? "").trim();
    if (!text || from.startsWith("grp_")) return;
    // A bot answering: a guest question, or the secretary.
    const g = guestWaits.get(from);
    if (g && Date.now() - g.at < WAIT_MS) {
      g.at = Date.now();
      const head = JSON.stringify({ from, name: "@" + from, kind: "guest" });
      try { await sendTo(g.chat, `[fwd:v1]${head}\n${text}`); } catch (e) { console.warn("guest answer", e); }
      return;
    }
    const s = secretaryWaits.get(from);
    if (s && Date.now() - s.at < WAIT_MS) {
      secretaryWaits.delete(from);
      try { await api.sendMessage(s.to, text); } catch (e) { console.warn("secretary answer", e); }
      return;
    }
    // A person writing to me: the secretary, if it should answer them.
    const conf = get(secretary);
    if (!conf || from === conf.bot || (await isBot(from))) return;
    if (conf.scope === "noncontacts" && getContact(from)) return;
    if (conf.scope === "new" && !p.is_new_chat) return;
    try {
      await api.botHandOver(conf.bot, `[from @${from}]\n${text}`);
      secretaryWaits.set(conf.bot, { to: from, at: Date.now() });
    } catch (e) { console.warn("secretary", e); }
  }).catch(() => {});
}
