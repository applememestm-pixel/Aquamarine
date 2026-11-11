/**
 * Chat folders — Telegram-style. A folder is just a named set of chat
 * ids; the chat list filters to the active folder. Stored in
 * localStorage (per device). Ported in spirit from the old PipoHuy
 * FolderManager / Android FolderStore.
 */

import { writable, get } from "svelte/store";

export type FolderKind = "contacts" | "others" | "groups" | "channels";
export type FolderSkip = "muted" | "read" | "archived";
export interface Folder {
  id: string;
  name: string;
  /** Chats added by hand. */
  chatIds: string[];
  /** Telegram's folder icon, shown on the tab. */
  emoji?: string;
  /** Tag colour (FOLDER_COLORS index); none: no tag on the rows. */
  color?: number;
  /** Whole kinds of chats in the folder (Telegram: "Included chats"). */
  kinds?: FolderKind[];
  /** Left out even when included (Telegram: "Excluded chats"). */
  skip?: FolderSkip[];
  excludeIds?: string[];
}

/** Tag colours — built from the theme's own colours. */
export const FOLDER_COLORS = [
  "var(--accent)",
  "var(--accent-2)",
  "var(--online)",
  "var(--danger)",
  "color-mix(in srgb, var(--danger) 55%, var(--online))",
  "color-mix(in srgb, var(--accent-2) 50%, var(--danger))",
  "var(--fg-muted)",
];

/** Facts about a chat the folder rules look at. */
export interface ChatFacts { id: string; kind: "people" | "groups" | "channels"; contact: boolean; muted: boolean; unread: boolean; archived: boolean }

/** Whether the folder shows this chat. */
export function folderHas(f: Folder, c: ChatFacts): boolean {
  if (f.excludeIds?.includes(c.id)) return false;
  const byHand = f.chatIds.includes(c.id);
  const k = f.kinds ?? [];
  const byKind = (c.kind === "groups" && k.includes("groups")) || (c.kind === "channels" && k.includes("channels"))
    || (c.kind === "people" && k.includes(c.contact ? "contacts" : "others"));
  if (!byHand && !byKind) return false;
  const sk = f.skip ?? [];
  if (sk.includes("muted") && c.muted) return false;
  if (sk.includes("read") && !c.unread) return false;
  if (sk.includes("archived") && c.archived) return false;
  return true;
}

export function updateFolder(id: string, patch: Partial<Folder>) {
  folders.update((list) => list.map((f) => (f.id === id ? { ...f, ...patch } : f)));
}
export function moveFolder(id: string, dir: -1 | 1) {
  folders.update((list) => {
    const i = list.findIndex((f) => f.id === id), j = i + dir;
    if (i < 0 || j < 0 || j >= list.length) return list;
    const n = [...list];
    [n[i], n[j]] = [n[j], n[i]];
    return n;
  });
}

/** Folder tags on the rows of "All" (Telegram). */
export const folderTags = writable<boolean>((() => { try { return localStorage.getItem("aqua.folderTags") !== "0"; } catch { return true; } })());
folderTags.subscribe((v) => { try { localStorage.setItem("aqua.folderTags", v ? "1" : "0"); } catch { /* ignore */ } });

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

export function createFolder(name: string, patch: Partial<Folder> = {}): string {
  const id = uid();
  folders.update((list) => [...list, { id, name: name.trim() || "Folder", chatIds: [], color: list.length % FOLDER_COLORS.length, ...patch }]);
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
      return {
        ...f,
        chatIds: has ? f.chatIds.filter((c) => c !== chatId) : [...f.chatIds, chatId],
        // Added by hand: no longer left out.
        excludeIds: has ? f.excludeIds : (f.excludeIds ?? []).filter((c) => c !== chatId),
      };
    }),
  );
}

/** Add a chat to a folder (no-op if already there). */
export function addChatToFolder(id: string, chatId: string) {
  folders.update((list) =>
    list.map((f) => (f.id === id && !f.chatIds.includes(chatId) ? { ...f, chatIds: [...f.chatIds, chatId] } : f)),
  );
}
