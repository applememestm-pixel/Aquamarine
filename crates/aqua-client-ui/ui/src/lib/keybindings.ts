/**
 * Rebindable keyboard shortcuts.
 *
 * Every action has default key combos; the user can replace them in the
 * cheat-sheet (Ctrl+/) or in `~/.config/aquamarine/keybindings.json`:
 *
 *   { "settings": ["Ctrl+Comma"], "callMute": ["Ctrl+Shift+KeyM", "F9"] }
 *
 * A combo is modifiers (Ctrl, Alt, Shift — Cmd counts as Ctrl) plus the
 * key's `KeyboardEvent.code` ("KeyM", "Digit1", "Comma", "F1", "ArrowUp"…).
 * Only overrides are stored; everything else keeps its default.
 */
import { writable, derived, get } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";

export type ActionId =
  | "cheatsheet" | "search" | "newChat" | "settings" | "profile"
  | "nextChat" | "prevChat" | "savedMessages"
  | "tabChats" | "tabCalls" | "tabFiles" | "tabContacts"
  | "chatSearch" | "emoji" | "jumpBottom" | "call"
  | "callMute" | "callDeafen"
  | "fmtBold" | "fmtItalic" | "fmtUnderline" | "fmtLink" | "fmtStrike" | "fmtSpoiler" | "fmtMono" | "fmtQuote" | "fmtClear";

export interface ActionDef { id: ActionId; group: string; what: string; defaults: string[] }
/** Context keys that can't be rebound (shown in the cheat-sheet as they are). */
export interface FixedDef { group: string; what: string; keys: string[][] }

export const ACTIONS: ActionDef[] = [
  { id: "search", group: "Navigation", what: "Search chats and people", defaults: ["Ctrl+KeyK"] },
  { id: "prevChat", group: "Navigation", what: "Previous chat", defaults: ["Alt+ArrowUp", "Ctrl+Shift+Tab"] },
  { id: "nextChat", group: "Navigation", what: "Next chat", defaults: ["Alt+ArrowDown", "Ctrl+Tab"] },
  { id: "savedMessages", group: "Navigation", what: "Saved Messages", defaults: ["Ctrl+Digit0"] },
  { id: "tabChats", group: "Navigation", what: "Chats", defaults: ["Alt+Digit1"] },
  { id: "tabCalls", group: "Navigation", what: "Calls", defaults: ["Alt+Digit2"] },
  { id: "tabFiles", group: "Navigation", what: "Files", defaults: ["Alt+Digit3"] },
  { id: "tabContacts", group: "Navigation", what: "Contacts", defaults: ["Alt+Digit4"] },
  { id: "newChat", group: "Navigation", what: "New chat", defaults: ["Ctrl+KeyN"] },
  { id: "chatSearch", group: "Chat", what: "Search in this chat", defaults: ["Ctrl+KeyF"] },
  { id: "emoji", group: "Chat", what: "Emoji, stickers & GIFs", defaults: ["Ctrl+KeyE"] },
  { id: "jumpBottom", group: "Chat", what: "Jump to the latest message", defaults: ["Ctrl+End"] },
  { id: "call", group: "Chat", what: "Call this chat", defaults: ["Ctrl+Shift+KeyC"] },
  { id: "fmtBold", group: "Formatting", what: "Bold (select text first)", defaults: ["Ctrl+KeyB"] },
  { id: "fmtItalic", group: "Formatting", what: "Italic", defaults: ["Ctrl+KeyI"] },
  { id: "fmtUnderline", group: "Formatting", what: "Underline", defaults: ["Ctrl+KeyU"] },
  { id: "fmtStrike", group: "Formatting", what: "Strikethrough", defaults: ["Ctrl+Shift+KeyX"] },
  { id: "fmtSpoiler", group: "Formatting", what: "Spoiler", defaults: ["Ctrl+Shift+KeyP"] },
  { id: "fmtMono", group: "Formatting", what: "Monospace", defaults: ["Ctrl+Shift+KeyM"] },
  { id: "fmtQuote", group: "Formatting", what: "Quote", defaults: ["Ctrl+Shift+Period"] },
  { id: "fmtLink", group: "Formatting", what: "Add a link", defaults: ["Ctrl+KeyK"] },
  { id: "fmtClear", group: "Formatting", what: "Clear formatting", defaults: ["Ctrl+Shift+KeyN"] },
  { id: "callMute", group: "Calls", what: "Mute / unmute", defaults: ["Ctrl+Shift+KeyM"] },
  { id: "callDeafen", group: "Calls", what: "Deafen / undeafen", defaults: ["Ctrl+Shift+KeyD"] },
  { id: "settings", group: "App", what: "Settings", defaults: ["Ctrl+Comma"] },
  { id: "profile", group: "App", what: "Your profile", defaults: ["Ctrl+KeyP"] },
  { id: "cheatsheet", group: "App", what: "Keyboard shortcuts (this sheet)", defaults: ["Ctrl+Slash", "F1"] },
];

export const FIXED: FixedDef[] = [
  { group: "Navigation", what: "Pick a chat from the search results", keys: [["↑"], ["↓"], ["Enter"]] },
  { group: "Navigation", what: "Jump to chat 1–9 in the list", keys: [["Ctrl", "1…9"]] },
  { group: "Chat", what: "Start typing — focuses the message box", keys: [["A…Z"]] },
  { group: "Chat", what: "Edit your last message (empty box)", keys: [["↑"]] },
  { group: "Chat", what: "Reply to an earlier / later message", keys: [["Ctrl", "↑"], ["Ctrl", "↓"]] },
  { group: "Chat", what: "Cancel reply / edit, close panels", keys: [["Esc"]] },
  { group: "Chat", what: "Commands: /article, /poll, /gif, /call…", keys: [["/"]] },
];

const LS_KEY = "aqua.keybindings.v1";
type Overrides = Partial<Record<ActionId, string[]>>;

function loadLocal(): Overrides {
  try { return JSON.parse(localStorage.getItem(LS_KEY) ?? "{}") as Overrides; } catch { return {}; }
}

/** The user's overrides (only changed actions). */
export const keyOverrides = writable<Overrides>(loadLocal());

/** Effective combos of every action. */
export const bindings = derived(keyOverrides, (o) => {
  const m = {} as Record<ActionId, string[]>;
  for (const a of ACTIONS) m[a.id] = o[a.id] ?? a.defaults;
  return m;
});

let fileLoaded = false;
keyOverrides.subscribe((o) => {
  try { localStorage.setItem(LS_KEY, JSON.stringify(o)); } catch { /* ignore */ }
  if (fileLoaded) void invoke("save_keybindings", { json: JSON.stringify(o, null, 2) }).catch(() => {});
});

/** Pick up edits made to keybindings.json by hand (on start). */
export async function loadKeybindingsFile() {
  try {
    const raw = await invoke<string | null>("load_keybindings");
    if (raw) {
      const parsed = JSON.parse(raw) as Record<string, unknown>;
      const clean: Overrides = {};
      for (const a of ACTIONS) {
        const v = parsed[a.id];
        if (Array.isArray(v)) clean[a.id] = v.filter((x): x is string => typeof x === "string" && isCombo(x));
        else if (typeof v === "string" && isCombo(v)) clean[a.id] = [v];
      }
      keyOverrides.set(clean);
    }
  } catch { /* browser preview / no file */ }
  fileLoaded = true;
}

const MODS = ["Ctrl", "Alt", "Shift"] as const;

function isCombo(s: string): boolean {
  const parts = s.split("+");
  const code = parts.pop() ?? "";
  return !!code && parts.every((p) => (MODS as readonly string[]).includes(p));
}

/** "Ctrl+Shift+KeyM" for a key event (`null` for a bare modifier press). */
export function comboOf(e: KeyboardEvent): string | null {
  if (["ControlLeft", "ControlRight", "ShiftLeft", "ShiftRight", "AltLeft", "AltRight", "MetaLeft", "MetaRight", "OSLeft", "OSRight"].includes(e.code)) return null;
  const mods: string[] = [];
  if (e.ctrlKey || e.metaKey) mods.push("Ctrl");
  if (e.altKey) mods.push("Alt");
  if (e.shiftKey) mods.push("Shift");
  return [...mods, e.code || e.key].join("+");
}

export function matches(e: KeyboardEvent, id: ActionId): boolean {
  const c = comboOf(e);
  return !!c && get(bindings)[id].includes(c);
}

/** The formatting action a key event triggers, if any. */
export function formattingAction(e: KeyboardEvent): ActionId | null {
  const c = comboOf(e);
  if (!c) return null;
  const b = get(bindings);
  for (const id of ["fmtBold", "fmtItalic", "fmtUnderline", "fmtLink", "fmtStrike", "fmtSpoiler", "fmtMono", "fmtQuote", "fmtClear"] as ActionId[]) {
    if (b[id].includes(c)) return id;
  }
  return null;
}

const PRETTY: Record<string, string> = {
  Comma: ",", Period: ".", Slash: "/", Backslash: "\\", Semicolon: ";", Quote: "'", BracketLeft: "[", BracketRight: "]",
  Minus: "−", Equal: "=", Backquote: "`", ArrowUp: "↑", ArrowDown: "↓", ArrowLeft: "←", ArrowRight: "→",
  Escape: "Esc", Space: "Space", Enter: "Enter", Tab: "Tab", Backspace: "⌫", Delete: "Del", End: "End", Home: "Home",
  PageUp: "PgUp", PageDown: "PgDn", Insert: "Ins",
};

/** "Ctrl+Shift+KeyM" → ["Ctrl", "Shift", "M"]. */
export function prettyCombo(c: string): string[] {
  const parts = c.split("+");
  const code = parts.pop() ?? "";
  let key = PRETTY[code] ?? code;
  if (/^Key[A-Z]$/.test(code)) key = code.slice(3);
  else if (/^Digit\d$/.test(code)) key = code.slice(5);
  else if (/^Numpad\d$/.test(code)) key = "Num " + code.slice(6);
  return [...parts, key];
}

/** Rebind an action. Taking a combo away from another action is reported. */
export function setBinding(id: ActionId, combos: string[]): ActionId[] {
  const taken: ActionId[] = [];
  const all = get(bindings);
  keyOverrides.update((o) => {
    const next: Overrides = { ...o, [id]: combos };
    for (const a of ACTIONS) {
      if (a.id === id || !combos.some((c) => all[a.id].includes(c))) continue;
      // Formatting keys only act inside the message box, so they may share
      // a combo with an app-wide action (Ctrl+K: search / add a link).
      const fmt = (x: string) => x.startsWith("fmt");
      if (fmt(a.id) !== fmt(id)) continue;
      next[a.id] = all[a.id].filter((c) => !combos.includes(c));
      taken.push(a.id);
    }
    return next;
  });
  return taken;
}

export function resetBinding(id: ActionId) {
  keyOverrides.update((o) => { const n = { ...o }; delete n[id]; return n; });
}

export function resetAllBindings() {
  keyOverrides.set({});
}
