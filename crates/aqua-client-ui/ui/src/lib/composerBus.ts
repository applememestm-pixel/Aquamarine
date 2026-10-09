/**
 * Tiny bus to insert text (custom-emoji tokens) into the active chat
 * composer from elsewhere (the emoji picker). ChatWindow subscribes and
 * appends whatever is pushed here.
 */
import { writable } from "svelte/store";

/** Each push carries a token to append to the composer, e.g. "[ce:pepe/happy.webp]". */
export const insertToComposer = writable<string | null>(null);

export function insertEmoji(token: string) {
  insertToComposer.set(token);
  // Reset so the same token can be inserted twice in a row.
  queueMicrotask(() => insertToComposer.set(null));
}
