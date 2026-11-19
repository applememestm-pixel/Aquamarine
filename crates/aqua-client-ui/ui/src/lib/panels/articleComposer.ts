/** Opening state of ArticleComposer — kept out of the component so the component itself can load lazily. */
import { writable } from "svelte/store";

export interface ComposerState {
  peerId: string;
  /** Text carried over from the message field (its "expand" button). */
  body?: string;
}
export const articleState = writable<ComposerState | null>(null);

export function openArticleComposer(peerId: string, body?: string) {
  articleState.set(body ? { peerId, body } : { peerId });
}
export function closeArticleComposer() {
  articleState.set(null);
}
