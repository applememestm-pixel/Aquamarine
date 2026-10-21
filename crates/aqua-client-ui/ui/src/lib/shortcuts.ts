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
import { openSettings, settingsOpen } from "./panels/settings";
import { openProfile } from "./components/ProfileModal.svelte";
import { toggleCallMute, toggleCallDeafen } from "./components/CallModal.svelte";
import { openNewChatDialog } from "./newChat";
import { matches, formattingAction, loadKeybindingsFile, type ActionId } from "./keybindings";

/** The cheat-sheet is waiting for a new key combo: global keys stay quiet. */
export const rebinding = writable<string | null>(null);

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

export { ACTIONS, FIXED } from "./keybindings";

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

/** Text selected in a text field — formatting shortcuts win then. */
function formattingSelection(): boolean {
  if (!isEditable(document.activeElement)) return false;
  const el = document.activeElement as HTMLInputElement | HTMLTextAreaElement;
  if ("selectionStart" in el && typeof el.selectionStart === "number" && el.tagName !== "DIV") return el.selectionStart !== el.selectionEnd;
  const s = getSelection();
  return !!s && !s.isCollapsed;
}

function onKeyDown(e: KeyboardEvent) {
  const ctrl = e.ctrlKey || e.metaKey;
  const code = e.code;
  let handled = true;
  const is = (id: ActionId) => matches(e, id);
  // The cheat-sheet is capturing a new combo for an action.
  if (get(rebinding)) return;

  // Formatting keys on selected text belong to the text field, not to the
  // app-wide shortcuts (Ctrl+K there adds a link instead of searching).
  if (formattingSelection() && formattingAction(e)) return;

  if (is("cheatsheet")) {
    shortcutsOpen.update((v) => !v);
  } else if (is("callMute")) {
    toggleCallMute();
  } else if (is("callDeafen")) {
    toggleCallDeafen();
  } else if (is("call")) {
    emit("aqua:call");
  } else if (is("search")) {
    activeTab.set("chats");
    queueMicrotask(() => document.querySelector<HTMLInputElement>("#search")?.focus());
  } else if (is("chatSearch")) {
    emit("aqua:chat-search");
  } else if (is("emoji")) {
    emit("aqua:toggle-emoji");
  } else if (is("newChat")) {
    openNewChatDialog();
  } else if (is("profile")) {
    openProfile();
  } else if (is("settings")) {
    openSettings();
  } else if (is("jumpBottom")) {
    emit("aqua:jump-bottom");
  } else if (is("nextChat")) {
    moveChat(1);
  } else if (is("prevChat")) {
    moveChat(-1);
  } else if (is("savedMessages")) {
    activeTab.set("chats");
    void selectChat("__saved__");
    queueMicrotask(() => emit("aqua:focus-composer"));
  } else if (is("tabChats") || is("tabCalls") || is("tabFiles") || is("tabContacts")) {
    activeTab.set(is("tabChats") ? "chats" : is("tabCalls") ? "calls" : is("tabFiles") ? "files" : "contacts");
  } else if (ctrl && !e.shiftKey && !e.altKey && /^Digit[1-9]$/.test(code)) {
    const n = Number(code.slice(5));
    activeTab.set("chats");
    const c = get(orderedChats)[n - 1];
    if (c) void selectChat(c.id);
    queueMicrotask(() => emit("aqua:focus-composer"));
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
  void loadKeybindingsFile();
  window.addEventListener("keydown", onKeyDown, true);
  return () => window.removeEventListener("keydown", onKeyDown, true);
}
