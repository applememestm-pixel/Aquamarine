/**
 * Chat groups: several chats gathered under one name, like Telegram's
 * communities. "Show as one chat" folds them into a single row of the list
 * (newest message, total unread); open it to see and search its chats.
 * A chat belongs to at most one group. Kept on this device.
 */
import { writable, get } from "svelte/store";

export interface ChatGroup {
  id: string;
  name: string;
  /** Emoji shown instead of the stacked avatars (optional). */
  emoji?: string;
  chatIds: string[];
  /** Show as one row in the chat list. */
  collapsed: boolean;
}

const KEY = "aqua.chatGroups.v1";

function load(): ChatGroup[] {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) ?? "[]");
    return Array.isArray(v) ? v.filter((g) => g && typeof g.id === "string" && Array.isArray(g.chatIds)) : [];
  } catch {
    return [];
  }
}

export const chatGroups = writable<ChatGroup[]>(load());
chatGroups.subscribe((v) => { try { localStorage.setItem(KEY, JSON.stringify(v)); } catch { /* ignore */ } });

/** The group that holds the chat (sheet that opens a group row). */
export const openGroupId = writable<string | null>(null);

export function groupOf(chatId: string): ChatGroup | undefined {
  return get(chatGroups).find((g) => g.chatIds.includes(chatId));
}

/** New group with these chats (they leave any other group). */
export function createGroup(name: string, chatIds: string[]): string {
  const id = "cg_" + Math.random().toString(36).slice(2, 10);
  const ids = [...new Set(chatIds)];
  chatGroups.update((gs) => [
    ...gs.map((g) => ({ ...g, chatIds: g.chatIds.filter((c) => !ids.includes(c)) })).filter((g) => g.chatIds.length > 0),
    { id, name: name.trim().slice(0, 64) || "Group", chatIds: ids, collapsed: true },
  ]);
  return id;
}

export function addToGroup(gid: string, chatId: string) {
  chatGroups.update((gs) =>
    gs
      .map((g) => (g.id === gid
        ? { ...g, chatIds: g.chatIds.includes(chatId) ? g.chatIds : [...g.chatIds, chatId] }
        : { ...g, chatIds: g.chatIds.filter((c) => c !== chatId) }))
      .filter((g) => g.chatIds.length > 0),
  );
}

/** Take a chat out; a group left empty disappears. */
export function removeFromGroup(chatId: string) {
  chatGroups.update((gs) => gs.map((g) => ({ ...g, chatIds: g.chatIds.filter((c) => c !== chatId) })).filter((g) => g.chatIds.length > 0));
}

export function renameGroup(gid: string, name: string, emoji?: string) {
  chatGroups.update((gs) => gs.map((g) => (g.id === gid ? { ...g, name: name.trim().slice(0, 64) || g.name, emoji: emoji?.trim() || undefined } : g)));
}

export function setCollapsed(gid: string, on: boolean) {
  chatGroups.update((gs) => gs.map((g) => (g.id === gid ? { ...g, collapsed: on } : g)));
}

/** Ungroup: the chats stay, the group goes. */
export function deleteGroup(gid: string) {
  chatGroups.update((gs) => gs.filter((g) => g.id !== gid));
  if (get(openGroupId) === gid) openGroupId.set(null);
}
