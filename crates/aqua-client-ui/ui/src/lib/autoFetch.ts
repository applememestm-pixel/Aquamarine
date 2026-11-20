/**
 * Media of every chat comes down when it arrives, in the background — not
 * only in the chat that's open. The relay is a delivery point, not an
 * archive: a photo nobody fetched could be gone after a while (or, before
 * 1.21.0-20, after a restart of the server), and the chat showed
 * "Photo unavailable". Same rules as the open chat (Settings → Data and
 * storage → Automatic media download): on/off, by kind of chat, up to a size.
 * Files land in per-chat folders (download_file).
 */
import { get } from "svelte/store";
import { api } from "./tauri";
import { chats, activeId } from "./stores";
import { settings, AUTODL_DEFAULT } from "./theme";
import type { ChatMessage } from "./types";

const SMALL_FILE = 3 * 1024 * 1024;
/** How far back a chat is looked at on each arrival (a burst after reconnecting). */
const LOOK_BACK = 40;

const tried = new Set<string>();
const timers = new Map<string, ReturnType<typeof setTimeout>>();
let queue: Promise<void> = Promise.resolve();

function inline(c: { category: string | null; mime: string | null; name: string }): boolean {
  const cat = (c.category ?? "").toLowerCase();
  if (cat === "document") return false;
  if (["sticker", "gif", "voice", "circle"].includes(cat) || cat.startsWith("live")) return true;
  const mime = (c.mime ?? "").toLowerCase();
  return mime.startsWith("image/") || mime.startsWith("video/") || mime.startsWith("audio/") || /\.(tgs|webm|mp3|ogg|opus|m4a)$/i.test(c.name);
}

/** Should this message's file come down by itself? */
export function wanted(chatId: string, m: ChatMessage): boolean {
  if (m.kind.type !== "file") return false;
  const c = m.kind.content;
  if (c.saved_at) return false;
  const cat = (c.category ?? "").toLowerCase();
  // View-once is opened on purpose, never fetched behind your back.
  if (cat === "once" || cat === "once-seen") return false;
  // A streamed video plays from the relay piece by piece (startStream).
  if (c.chunk) return false;
  const s = get(settings);
  if (!s.autoDownloadMedia) return false;
  const ad = { ...AUTODL_DEFAULT, ...(s.autoDl ?? {}) };
  const chat = get(chats).find((x) => x.id === chatId);
  const where = chat?.channel ? ad.channels : chatId.startsWith("grp_") ? ad.groups : ad.private;
  if (!where) return false;
  const cap = ad.limitMb * 1024 * 1024;
  return inline(c) ? c.size <= cap : c.size <= Math.min(SMALL_FILE, cap);
}

async function fetchChat(chatId: string) {
  let msgs: ChatMessage[];
  try { msgs = await api.getMessages(chatId); } catch { return; }
  for (const m of msgs.slice(-LOOK_BACK)) {
    const key = `${chatId}/${m.id}`;
    if (tried.has(key) || !wanted(chatId, m)) continue;
    tried.add(key);
    try { await api.downloadFile(chatId, m.id); } catch (e) { console.warn("[autofetch]", chatId, m.id, String(e)); }
  }
}

/** A message came to `chatId`: fetch its new media shortly (once per burst). */
export function autoFetchSoon(chatId: string) {
  // Saved Messages too: a file saved on my other device comes down here.
  if (!chatId || (chatId.startsWith("__") && chatId !== "__saved__")) return;
  // The open chat does it itself (ChatWindow), with streaming for big videos.
  if (get(activeId) === chatId) return;
  const t = timers.get(chatId);
  if (t) clearTimeout(t);
  timers.set(chatId, setTimeout(() => {
    timers.delete(chatId);
    // One chat at a time: a reconnect brings many at once.
    queue = queue.then(() => fetchChat(chatId)).catch(() => {});
  }, 1500));
}
