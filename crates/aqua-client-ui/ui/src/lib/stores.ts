import { writable, derived, get } from "svelte/store";
import type { ChatMessage, ChatSummary, InitInfo } from "./types";
import { api, events } from "./tauri";
import { contacts, contactMatches, displayName, saveContact } from "./contacts";
import { settings } from "./theme";

/** Play a short chime + optional desktop notification for an incoming
 *  message, honouring the user's notification settings. */
function notifyIncoming(peerId: string, text: string) {
  const s = get(settings);
  if (s.notifySound) {
    try {
      // Tiny synthesized "ding" via WebAudio — no asset file needed.
      const Ctx = (window as any).AudioContext || (window as any).webkitAudioContext;
      if (Ctx) {
        const ctx = new Ctx();
        const o = ctx.createOscillator();
        const g = ctx.createGain();
        o.type = "sine";
        o.frequency.value = 880;
        g.gain.setValueAtTime(0.0001, ctx.currentTime);
        g.gain.exponentialRampToValueAtTime(0.12, ctx.currentTime + 0.01);
        g.gain.exponentialRampToValueAtTime(0.0001, ctx.currentTime + 0.25);
        o.connect(g); g.connect(ctx.destination);
        o.start(); o.stop(ctx.currentTime + 0.26);
        setTimeout(() => ctx.close().catch(() => {}), 400);
      }
    } catch { /* ignore */ }
  }
  if (s.notifyDesktop && "Notification" in window && Notification.permission === "granted") {
    try {
      const name = displayName(peerId, peerId);
      new Notification(name, { body: text.slice(0, 120) });
    } catch { /* ignore */ }
  }
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
  const q = $search.trim();
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
  events.onTransportError((msg) => {
    console.error("transport error:", msg);
    info.update((i) => (i ? { ...i, connected: false } : i));
  });
  events.onDisconnected(() => {
    info.update((i) => (i ? { ...i, connected: false } : i));
  });
}

export async function connect(server_url: string, username: string): Promise<string | null> {
  try {
    const i = await api.connectToServer(server_url, username);
    info.set(i);
    // Also refresh the chat list — some chats become .live=true once
    // the transport is open and sessions reattach.
    try { chats.set(await api.listChats()); } catch { /* non-fatal */ }
    return null;
  } catch (e) {
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
    try {
      const i = await api.connectToServer(server_url, username);
      info.set(i);
      try { chats.set(await api.listChats()); } catch { /* non-fatal */ }
      console.info(`[aqua] auto-reconnected to ${server_url} as ${username}`);
      return;
    } catch (e) {
      console.warn(`[aqua] auto-reconnect attempt ${attempt + 1} failed:`, e);
    }
  }
  console.warn("[aqua] auto-reconnect gave up — user can click the plug icon");
}

export async function startChat(
  peer_username: string,
  first_message: string,
  extra?: { full_name?: string; phone?: string },
): Promise<string | null> {
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

export async function selectChat(id: string) {
  activeId.set(id);
  chats.update((cs) => cs.map((c) => (c.id === id ? { ...c, unread: 0 } : c)));
  // On a phone-width layout this switches the single column from the chat
  // list to the open conversation (see the <=720px rules in app.css).
  try {
    const wasOpen = document.body.classList.contains("chat-open");
    document.body.classList.add("chat-open");
    // Give the phone's system Back button something to pop: one history
    // entry per "list → chat" step (see the popstate hook in App.svelte).
    if (!wasOpen && window.matchMedia("(max-width: 720px)").matches) {
      history.pushState({ aquaChat: true }, "");
    }
  } catch { /* ignore */ }
  await loadActiveMessages();
}

/** Mobile: go back from an open chat to the chat list. `fromHistory` is
 *  set when the system Back button already popped our history entry. */
export function backToList(fromHistory = false) {
  try {
    document.body.classList.remove("chat-open");
    if (!fromHistory && history.state?.aquaChat) history.back();
  } catch { /* ignore */ }
}

export async function loadActiveMessages() {
  const id = get(activeId);
  if (!id) { activeMessages.set([]); return; }
  try {
    const msgs = await api.getMessages(id);
    activeMessages.set(msgs);
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
    activeMessages.set(msgs);
    chats.set(updated);
    // Clear draft.
    drafts.update((d) => { const { [id]: _, ...rest } = d; return rest; });
    return null;
  } catch (e) {
    console.error("send_message failed", e);
    return String(e);
  }
}
