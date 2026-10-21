/** Opening state of GroupPanel — kept out of the component so the component itself can load lazily. */
import { writable } from "svelte/store";
export const groupPanelId = writable<string | null>(null);
/** Open straight at a page (e.g. a room's settings from its context menu). */
export const groupPanelStart = writable<{ page: string; room?: string } | null>(null);
export function openGroupPanel(gid: string, start?: { page: "room-edit" | "room-perms" | "rooms"; room?: string }) {
  groupPanelStart.set(start ?? null);
  groupPanelId.set(null);
  groupPanelId.set(gid);
}
export function closeGroupPanel() { groupPanelId.set(null); }
