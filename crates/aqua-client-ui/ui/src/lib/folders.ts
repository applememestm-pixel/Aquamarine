/**
 * Chat folders — Telegram-style. A folder is just a named set of chat
 * ids; the chat list filters to the active folder. Stored in
 * localStorage (per device). Ported in spirit from the old PipoHuy
 * FolderManager / Android FolderStore.
 */

import { writable, get } from "svelte/store";

export interface Folder {
  id: string;
  name: string;
  chatIds: string[];
}

const KEY = "aqua-folders";
const ACTIVE_KEY = "aqua-active-folder";

function load(): Folder[] {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.filter((f) => f && typeof f.id === "string");
  } catch {
    return [];
  }
}

export const folders = writable<Folder[]>(load());
folders.subscribe((f) => {
  try { localStorage.setItem(KEY, JSON.stringify(f)); } catch { /* ignore */ }
});

/** "all" (everything) or a folder id. */
export const activeFolder = writable<string>(
  (() => { try { return localStorage.getItem(ACTIVE_KEY) ?? "all"; } catch { return "all"; } })(),
);
activeFolder.subscribe((v) => {
  try { localStorage.setItem(ACTIVE_KEY, v); } catch { /* ignore */ }
});

function uid(): string {
  return "f" + Math.random().toString(36).slice(2, 9);
}

export function createFolder(name: string): string {
  const id = uid();
  folders.update((list) => [...list, { id, name: name.trim() || "Folder", chatIds: [] }]);
  return id;
}

export function renameFolder(id: string, name: string) {
  folders.update((list) => list.map((f) => (f.id === id ? { ...f, name: name.trim() || f.name } : f)));
}

export function deleteFolder(id: string) {
  folders.update((list) => list.filter((f) => f.id !== id));
  if (get(activeFolder) === id) activeFolder.set("all");
}

export function isInFolder(id: string, chatId: string): boolean {
  const f = get(folders).find((x) => x.id === id);
  return !!f && f.chatIds.includes(chatId);
}

export function toggleChatInFolder(id: string, chatId: string) {
  folders.update((list) =>
    list.map((f) => {
      if (f.id !== id) return f;
      const has = f.chatIds.includes(chatId);
      return { ...f, chatIds: has ? f.chatIds.filter((c) => c !== chatId) : [...f.chatIds, chatId] };
    }),
  );
}

/** Add a chat to a folder (no-op if already there). */
export function addChatToFolder(id: string, chatId: string) {
  folders.update((list) =>
    list.map((f) => (f.id === id && !f.chatIds.includes(chatId) ? { ...f, chatIds: [...f.chatIds, chatId] } : f)),
  );
}
