/** Opening state of StickerEditor — kept out of the component so the component itself can load lazily. */
import { writable, get as getStore } from "svelte/store";
import { api as tauriApi } from "../tauri";

export interface State { imageBytes: number[]; mime: string; sourceName: string; pack?: string }
export const stickerEditorState = writable<State | null>(null);
/** Bumped after a sticker is saved, so libraries rescan. */
export const stickerLibraryTick = writable(0);

/** Open the editor with a picture (or empty bytes for a blank sticker:
 *  text and drawing only). `pack` preselects the target pack. */
export function openStickerEditor(imageBytes: number[], mime: string, sourceName: string, pack?: string) {
  stickerEditorState.set({ imageBytes, mime, sourceName, pack });
}
export function closeStickerEditor() { stickerEditorState.set(null); stickerQueue.set(null); }

/** Making a whole pack: the editor walks through several pictures. */
export interface Queue { paths: string[]; index: number; pack: string; saved: number }
export const stickerQueue = writable<Queue | null>(null);
export const mimeOf = (p: string) => { const l = p.toLowerCase(); return l.endsWith(".png") ? "image/png" : l.endsWith(".webp") ? "image/webp" : l.endsWith(".gif") ? "image/gif" : "image/jpeg"; };
export async function openStickerQueue(paths: string[], pack: string) {
  if (!paths.length) return;
  stickerQueue.set({ paths, index: 0, pack, saved: 0 });
  await openQueued();
}
export async function openQueued() {
  const q = getStore(stickerQueue);
  if (!q) return;
  const p = q.paths[q.index];
  const bytes = await tauriApi.readUserImage(p);
  stickerEditorState.set({ imageBytes: bytes, mime: mimeOf(p), sourceName: p.split(/[\\/]/).pop() ?? "sticker", pack: q.pack });
}
/** Next picture of the pack (after a save or a skip); false when done. */
export async function nextInQueue(savedOne: boolean): Promise<boolean> {
  const q = getStore(stickerQueue);
  if (!q) return false;
  const saved = q.saved + (savedOne ? 1 : 0);
  if (q.index + 1 >= q.paths.length) { stickerQueue.set({ ...q, saved }); return false; }
  stickerQueue.set({ ...q, index: q.index + 1, saved });
  await openQueued();
  return true;
}
