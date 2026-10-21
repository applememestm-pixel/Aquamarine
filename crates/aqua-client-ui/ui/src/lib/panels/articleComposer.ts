/** Opening state of ArticleComposer — kept out of the component so the component itself can load lazily. */
import { writable } from "svelte/store";

export interface ComposerState {
  peerId: string;
}
export const articleState = writable<ComposerState | null>(null);

export function openArticleComposer(peerId: string) {
  articleState.set({ peerId });
}
export function closeArticleComposer() {
  articleState.set(null);
}
