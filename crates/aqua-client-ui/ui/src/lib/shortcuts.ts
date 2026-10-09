/**
 * Keyboard control — use the whole messenger without a mouse.
 *
 * Global keys live here; component-owned actions (composer, in-chat search,
 * reply/edit) are requested via `aqua:*` window events that the owning
 * component listens for. Ctrl+/ (or F1) shows the cheat-sheet.
 */
import { writable, derived, get } from "svelte/store";
import { visibleChats, activeId, selectChat, activeTab, type NavTab } from "./stores";
import { folders, activeFolder } from "./folders";
import { openSettings, settingsOpen } from "./components/SettingsPanel.svelte";
import { openProfile } from "./components/ProfileModal.svelte";
import { toggleCallMute, toggleCallDeafen } from "./components/CallModal.svelte";
import { openNewChatDialog } from "./newChat";

export const shortcutsOpen = writable(false);

/** The chats in the order the list shows them (folder filter applied). */
export const orderedChats = derived([visibleChats, folders, activeFolder], ([v, fs, af]) => {
  if (af === "all") return v;
  const f = fs.find((x) => x.id === af);
  if (!f) return v;
  const set = new Set(f.chatIds);
  return v.filter((c) => set.has(c.id));
});

export function emit(name: string, detail?: unknown) {
  window.dispatchEvent(new CustomEvent(name, { detail }));
}

export interface ShortcutDoc { keys: string; what: string; group: string }
export const SHORTCUTS: ShortcutDoc[] = [
  { group: "Navigation", keys: "Ctrl+K", what: "Search chats and people" },
  { group: "Navigation", keys: "↑ / ↓ / Enter", what: "Pick a chat from the search results" },
  { group: "Navigation", keys: "Alt+↑ / Alt+↓", what: "Previous / next chat" },
  { group: "Navigation", keys: "Ctrl+Tab / Ctrl+Shift+Tab", what: "Next / previous chat" },
  { group: "Navigation", keys: "Ctrl+1…9", what: "Jump to chat 1–9 in the list" },
  { group: "Navigation", keys: "Ctrl+0", what: "Saved Messages" },
  { group: "Navigation", keys: "Alt+1…4", what: "Chats · Calls · Files · Contacts" },
  { group: "Navigation", keys: "Ctrl+N", what: "New chat" },
  { group: "Chat", keys: "A…Z", what: "Start typing — focuses the message box" },
  { group: "Chat", keys: "Ctrl+F", what: "Search in this chat" },
  { group: "Chat", keys: "↑", what: "Edit your last message (empty box)" },
  { group: "Chat", keys: "Ctrl+↑ / Ctrl+↓", what: "Reply to an earlier / later message" },
  { group: "Chat", keys: "Esc", what: "Cancel reply / edit, close panels" },
  { group: "Chat", keys: "Ctrl+E", what: "Emoji, stickers & GIFs" },
  { group: "Chat", keys: "Ctrl+End", what: "Jump to the latest message" },
  { group: "Chat", keys: "Ctrl+Shift+C", what: "Call this chat" },
  { group: "Calls", keys: "Ctrl+Shift+M", what: "Mute / unmute" },
  { group: "Calls", keys: "Ctrl+Shift+D", what: "Deafen / undeafen" },
  { group: "App", keys: "Ctrl+,", what: "Settings" },
  { group: "App", keys: "Ctrl+P", what: "Your profile" },
  { group: "App", keys: "Ctrl+/ / F1", what: "This cheat-sheet" },
];

function isEditable(el: Element | null): boolean {
  if (!el) return false;
  const h = el as HTMLElement;
  return h.tagName === "INPUT" || h.tagName === "TEXTAREA" || h.tagName === "SELECT" || h.isContentEditable;
}

function overlayOpen(): boolean {
  // Any modal-ish layer on screen? Then plain keys belong to it.
  return get(settingsOpen) || get(shortcutsOpen) || !!document.querySelector(".backdrop, .picker-backdrop, [aria-modal='true']");
}

function moveChat(delta: number) {
  const list = get(orderedChats);
  if (!list.length) return;
  const i = list.findIndex((c) => c.id === get(activeId));
  const next = list[(i + delta + list.length) % list.length] ?? list[0];
  activeTab.set("chats");
  void selectChat(next.id);
}

function onKeyDown(e: KeyboardEvent) {
  const ctrl = e.ctrlKey || e.metaKey;
  const code = e.code;
  let handled = true;

  if ((ctrl && (e.key === "/" || code === "Slash")) || e.key === "F1") {
    shortcutsOpen.update((v) => !v);
  } else if (ctrl && e.shiftKey && code === "KeyM") {
    toggleCallMute();
  } else if (ctrl && e.shiftKey && code === "KeyD") {
    toggleCallDeafen();
  } else if (ctrl && e.shiftKey && code === "KeyC") {
    emit("aqua:call");
  } else if (ctrl && code === "KeyK") {
    activeTab.set("chats");
    queueMicrotask(() => document.querySelector<HTMLInputElement>("#search")?.focus());
  } else if (ctrl && code === "KeyF") {
    emit("aqua:chat-search");
  } else if (ctrl && code === "KeyE") {
    emit("aqua:toggle-emoji");
  } else if (ctrl && code === "KeyN") {
    openNewChatDialog();
  } else if (ctrl && code === "KeyP") {
    openProfile();
  } else if (ctrl && e.key === ",") {
    openSettings();
  } else if (ctrl && code === "End") {
    emit("aqua:jump-bottom");
  } else if (ctrl && code === "Tab") {
    moveChat(e.shiftKey ? -1 : 1);
  } else if (e.altKey && !ctrl && (code === "ArrowUp" || code === "ArrowDown")) {
    moveChat(code === "ArrowUp" ? -1 : 1);
  } else if (ctrl && !e.shiftKey && /^Digit[0-9]$/.test(code)) {
    const n = Number(code.slice(5));
    activeTab.set("chats");
    if (n === 0) void selectChat("__saved__");
    else {
      const c = get(orderedChats)[n - 1];
      if (c) void selectChat(c.id);
    }
    queueMicrotask(() => emit("aqua:focus-composer"));
  } else if (e.altKey && !ctrl && /^Digit[1-4]$/.test(code)) {
    const tabs: NavTab[] = ["chats", "calls", "files", "contacts"];
    activeTab.set(tabs[Number(code.slice(5)) - 1]);
  } else if (
    !ctrl && !e.altKey && e.key.length === 1 && !isEditable(document.activeElement) && !overlayOpen()
  ) {
    // Type-to-compose: the keystroke lands in the message box.
    emit("aqua:focus-composer");
    handled = false; // let the character through to the now-focused box
  } else {
    handled = false;
  }

  if (handled) {
    e.preventDefault();
    e.stopPropagation();
  }
}

/** Install the global key handler. Returns an uninstaller. */
export function initShortcuts(): () => void {
  window.addEventListener("keydown", onKeyDown, true);
  return () => window.removeEventListener("keydown", onKeyDown, true);
}
