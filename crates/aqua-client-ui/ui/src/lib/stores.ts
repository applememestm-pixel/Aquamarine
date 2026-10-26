import { listen } from "@tauri-apps/api/event";
import { writable, derived, get } from "svelte/store";
import type { ChatMessage, ChatSummary, InitInfo } from "./types";
import { api, events } from "./tauri";
import { onBack } from "./back";
import { contacts, contactMatches, displayName, saveContact } from "./contacts";
import { settings } from "./theme";
import { playSound, soundFor } from "./sounds";
import { tr } from "./i18n";
import { osNotify, appInFront } from "./notify";

/** Play a short chime + optional desktop notification for an incoming
 *  message, honouring the user's notification settings. */
function notifyIncoming(peerId: string, text: string) {
  const s = get(settings);
  if (s.notifySound) void playSound(soundFor(peerId));
  // A banner only when the user isn't already reading that chat.
  if (appInFront() && get(activeId) === peerId) return;
  // A chat with a password: nothing of it in the notification.
  let lockedChat = false;
  try { lockedChat = !!JSON.parse(localStorage.getItem("aqua.chatLocks.v1") ?? "{}")[peerId]; } catch { /* ignore */ }
  if (lockedChat) void osNotify("Aquamarine", tr("New message in a locked chat"));
  else void osNotify(displayName(peerId, peerId), text.slice(0, 120));
}

/** Startup info from the backend. */
export const info = writable<InitInfo | null>(null);

/** All chats, in server order. */
export const chats = writable<ChatSummary[]>([]);

/** Selected chat id. */
export const activeId = writable<string>("roma");

/** Which nav tab is active — drives what the middle column renders. */
export type NavTab = "chats" | "calls" | "files" | "contacts";
export const activeTab = writable<NavTab>("chats");

/** Messages of the currently-active chat. */
export const activeMessages = writable<ChatMessage[]>([]);

/** Local search filter. */
export const search = writable<string>("");

/** Per-chat draft, persisted to localStorage. */
const DRAFTS_KEY = "aqua-drafts";
export const drafts = writable<Record<string, string>>(loadDrafts());

drafts.subscribe((v) => {
  try {
    localStorage.setItem(DRAFTS_KEY, JSON.stringify(v));
  } catch { /* private mode, blocked storage — ignore */ }
});

function loadDrafts(): Record<string, string> {
  try {
    return JSON.parse(localStorage.getItem(DRAFTS_KEY) || "{}");
  } catch {
    return {};
  }
}

/** Derived: chats filtered by search. Matches against the chat name,
 *  the stored full name, phone, or username — so if the user set
 *  up a contact for "roma" with name "Roman R." and phone "+7...",
 *  typing any of those three finds the chat. Also enriches each
 *  chat with the stored display_name when available. */
export const visibleChats = derived([chats, search, contacts], ([$chats, $search, _$c]) => {
  const q = $search.trim().replace(/^@+/, "");
  const enriched = $chats.map((c) => ({
    ...c,
    name: displayName(c.id, c.name),
  }));
  if (!q) return enriched;
  const lower = q.toLowerCase();
  return enriched.filter((c) =>
    c.name.toLowerCase().includes(lower) || contactMatches(c.id, q),
  );
});

/** Derived: the active chat's summary (with stored display name). */
export const activeChat = derived([chats, activeId, contacts], ([$chats, $id, _$c]) => {
  const c = $chats.find((x) => x.id === $id);
  if (!c) return null;
  return { ...c, name: displayName(c.id, c.name) };
});

// ---------- Actions ----------

export async function bootstrap() {
  try {
    const [i, c] = await Promise.all([api.initApp(), api.listChats()]);
    info.set(i);
    chats.set(c);
    const currentId = get(activeId);
    if (!c.find((x) => x.id === currentId)) {
      activeId.set(c[0]?.id ?? "");
    }
    await loadActiveMessages();

    // Auto-reconnect vs first-launch wizard. No "Connect" button in
    // the UI any more — the right thing happens on its own:
    //   - saved hints   → silent reconnect (with 4-attempt backoff)
    //   - no hints yet  → one-shot setup dialog (default server URL
    //                     + a user-chosen handle), then auto-reconnect
    //                     every launch after that.
    // No silent auto-reconnect: the entry window is always shown on launch
    // (registered users get a one-click "Continue as @me" there). This keeps
    // registration/login testable every time and never auto-enters a stale
    // session behind the user's back.
  } catch (e) {
    console.error("bootstrap failed", e);
  }

  // Load groups/channels and keep them live.
  import("./groups").then((m) => m.initGroups());

  // Wire live event listeners.
  events.onIncoming(async (p) => {
    // Refresh chat list + messages for the affected peer.
    const updated = await api.listChats();
    chats.set(updated);
    const isActive = get(activeId) === p.peer_id;
    if (isActive) {
      await loadActiveMessages();
    }
    // Notify (sound + desktop) for messages that aren't in the chat
    // you're actively looking at.
    // Archived + unmuted chats pop back out; muted chats stay silent.
    const prefs = await import("./chatPrefs");
    prefs.onIncomingForArchive(p.peer_id);
    if (!isActive && !prefs.isMuted(p.peer_id)) notifyIncoming(p.peer_id, p.text);
    // Refresh info (opk count, etc.)
    api.initApp().then((i) => info.set(i)).catch(() => {});
  });
  events.onChatsUpdated(async () => {
    const updated = await api.listChats();
    chats.set(updated);
    api.initApp().then((i) => info.set(i)).catch(() => {});
  });
  // Auto-delete timers removed messages.
  listen<{ peer_id: string }>("aqua://expired", async (e) => {
    if (get(activeId) === e.payload.peer_id) await loadActiveMessages();
  });
  // Sent from another device of this account: show it in the open chat.
  listen<{ peer_id: string }>("aqua://own_sent", async (e) => {
    chats.set(await api.listChats());
    if (get(activeId) === e.payload.peer_id) await loadActiveMessages();
  });
  events.onTransportError((msg) => {
    console.error("transport error:", msg);
    info.update((i) => (i ? { ...i, connected: false } : i));
    // The socket is gone (network switched, connection reset): come back.
    void reconnectLoop();
  });
  // Network back / app back in front: don't wait for the next retry.
  const kick = () => { const i = get(info); if (i && !i.connected) { reconnectNow = true; void reconnectLoop(); } };
  addEventListener("online", kick);
  document.addEventListener("visibilitychange", () => { if (document.visibilityState === "visible") kick(); });
  events.onDisconnected(() => {
    info.update((i) => (i ? { ...i, connected: false } : i));
    void reconnectLoop();
  });
}

let reconnecting = false;
/** Set when the network came back: the loop skips its current pause. */
let reconnectNow = false;
/** The relay dropped us: retry it, then the other federated relays, with
 *  a growing pause, until one of them takes us back. */
async function reconnectLoop() {
  if (reconnecting) return;
  reconnecting = true;
  try {
    const delays = [1000, 3000, 7000, 15000, 30000];
    for (let n = 0; ; n++) {
      const i = get(info);
      if (!i || i.connected || !i.username || !i.server_url) return;
      // Wait, but wake up early if the network comes back meanwhile.
      const until = Date.now() + delays[Math.min(n, delays.length - 1)];
      while (Date.now() < until && !reconnectNow) await new Promise((r) => setTimeout(r, 250));
      if (reconnectNow) { reconnectNow = false; n = -1; }
      const cur = get(info);
      if (!cur || cur.connected || !cur.username || !cur.server_url) return;
      const err = await connect(cur.server_url, cur.username);
      if (!err) return;
    }
  } finally {
    reconnecting = false;
  }
}

export async function connect(server_url: string, username: string): Promise<string | null> {
  const { learnPeers, alternatives } = await import("./servers");
  const tryOne = async (url: string) => {
    const i = await api.connectToServer(url, username);
    info.set(i);
    // Also refresh the chat list — some chats become .live=true once
    // the transport is open and sessions reattach.
    try { chats.set(await api.listChats()); } catch { /* non-fatal */ }
    void learnPeers(url);
  };
  try {
    await tryOne(server_url);
    return null;
  } catch (e) {
    // Federated: the same account works on any of the relays we know —
    // unless the user locked this one (then only its own other addresses).
    const { lockedServer } = await import("./network");
    const { aliasesOf } = await import("./servers");
    const alts = lockedServer() ? aliasesOf(server_url) : alternatives(server_url);
    for (const alt of alts) {
      try {
        await tryOne(alt);
        console.info(`[aqua] ${server_url} is down — switched to ${alt}`);
        return null;
      } catch { /* next */ }
    }
    return String(e);
  }
}

/**
 * Silent reconnect used on boot. Retries a few times with a small
 * backoff — the server may still be coming up when the client
 * launches. Never shows the connect dialog; a lasting failure just
 * leaves the plug icon in its disconnected state so the user can
 * click it manually.
 */
export async function quickConnect(server_url: string, username: string) {
  const DELAYS_MS = [0, 1000, 3000, 7000]; // ~11s total before giving up
  for (let attempt = 0; attempt < DELAYS_MS.length; attempt++) {
    if (DELAYS_MS[attempt] > 0) {
      await new Promise((r) => setTimeout(r, DELAYS_MS[attempt]));
    }
    // The relay's other address too: its home one when the white IP doesn't
    // answer from inside, its white IP away from home.
    const { aliasesOf } = await import("./servers");
    for (const url of [server_url, ...aliasesOf(server_url)]) {
      try {
        const i = await api.connectToServer(url, username);
        info.set(i);
        try { chats.set(await api.listChats()); } catch { /* non-fatal */ }
        console.info(`[aqua] auto-reconnected to ${url} as ${username}`);
        return;
      } catch (e) {
        console.warn(`[aqua] auto-reconnect attempt ${attempt + 1} (${url}) failed:`, e);
      }
    }
  }
  console.warn("[aqua] auto-reconnect gave up — user can click the plug icon");
}

export async function startChat(
  peer_username: string,
  first_message: string,
  extra?: { full_name?: string; phone?: string },
): Promise<string | null> {
  // "@ivan" and "ivan" are the same person.
  peer_username = peer_username.trim().replace(/^@+/, "").toLowerCase();
  try {
    await api.startChatWith(peer_username, first_message);
    // Save contact metadata (name, phone) locally so the chat renders
    // as "Roman R." instead of just the raw username.
    if (extra && (extra.full_name || extra.phone)) {
      saveContact(peer_username, extra);
    }
    const updated = await api.listChats();
    chats.set(updated);
    await selectChat(peer_username);
    return null;
  } catch (e) {
    return String(e);
  }
}

/** The open chat is on screen: the app is in front, and on a phone the
 *  conversation (not the list) is showing. */
function chatOnScreen(): boolean {
  try {
    if (document.visibilityState !== "visible") return false;
    return !matchMedia("(max-width: 720px)").matches || document.body.classList.contains("chat-open");
  } catch { return true; }
}
/** Messages arriving in the open chat are read at once; the counter is
 *  zeroed on the Rust side too, so a list refresh doesn't bring it back. */
function readActive() {
  const id = get(activeId);
  if (!id || !chatOnScreen()) return;
  const c = get(chats).find((x) => x.id === id);
  if (!c || !c.unread) return;
  chats.update((cs) => cs.map((x) => (x.id === id ? { ...x, unread: 0 } : x)));
  void api.markChatRead(id).catch(() => {});
}
chats.subscribe(() => queueMicrotask(readActive));
try { document.addEventListener("visibilitychange", readActive); } catch { /* no document */ }

export async function selectChat(id: string) {
  activeId.set(id);
  chats.update((cs) => cs.map((c) => (c.id === id ? { ...c, unread: 0 } : c)));
  void api.markChatRead(id).catch(() => {});
  // On a phone-width layout this switches the single column from the chat
  // list to the open conversation (see the <=720px rules in app.css).
  try {
    const wasOpen = document.body.classList.contains("chat-open");
    document.body.classList.add("chat-open");
    // The phone's Back button / gesture: back to the list (lib/back.ts).
    if (!wasOpen && window.matchMedia("(max-width: 720px)").matches) {
      releaseChatBack?.();
      releaseChatBack = onBack(() => { releaseChatBack = null; backToList(); });
    }
  } catch { /* ignore */ }
  await loadActiveMessages();
}

let releaseChatBack: (() => void) | null = null;

/** Mobile: go back from an open chat to the chat list. */
export function backToList(_fromHistory = false) {
  try { document.body.classList.remove("chat-open"); } catch { /* ignore */ }
  const r = releaseChatBack;
  releaseChatBack = null;
  r?.();
}

/** Keep the old object for every message that didn't change, so a
 *  refresh after one new message doesn't re-render the whole chat. */
function reuseUnchanged(prev: ChatMessage[], next: ChatMessage[]): ChatMessage[] {
  if (!prev.length) return next;
  const old = new Map(prev.map((m) => [m.id, m]));
  let same = prev.length === next.length;
  const out = next.map((m, i) => {
    const o = old.get(m.id);
    const keep = o && JSON.stringify(o) === JSON.stringify(m) ? o : m;
    if (keep !== prev[i]) same = false;
    return keep;
  });
  return same ? prev : out;
}

export async function loadActiveMessages() {
  const id = get(activeId);
  if (!id) { activeMessages.set([]); return; }
  try {
    const msgs = await api.getMessages(id);
    activeMessages.set(reuseUnchanged(get(activeId) === id ? get(activeMessages) : [], msgs));
  } catch (e) {
    console.error("get_messages failed", e);
    activeMessages.set([]);
  }
}

/**
 * Try to send a message in the active chat.
 * Returns `null` on success, or a human-readable error string on failure
 * (the caller is expected to show it as a toast and restore the input).
 */
export async function sendCurrent(text: string): Promise<string | null> {
  const id = get(activeId);
  if (!id) return "No chat selected";
  if (!text.trim()) return "Message is empty";
  try {
    // Groups fan the message out to every member; 1:1 uses the direct path.
    if (id.startsWith("grp_")) {
      await api.groupSend(id, text);
    } else {
      await api.sendMessage(id, text);
    }
    // Refresh both messages and chat list (backend bumps chat to top).
    const [msgs, updated] = await Promise.all([api.getMessages(id), api.listChats()]);
    activeMessages.set(reuseUnchanged(get(activeMessages), msgs));
    chats.set(updated);
    // Clear draft.
    drafts.update((d) => { const { [id]: _, ...rest } = d; return rest; });
    return null;
  } catch (e) {
    console.error("send_message failed", e);
    return String(e);
  }
}
