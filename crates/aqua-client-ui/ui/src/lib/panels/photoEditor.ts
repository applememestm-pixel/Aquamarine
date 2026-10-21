/** Opening state of PhotoEditor — kept out of the component so the component itself can load lazily. */
import { writable, get as getStore } from "svelte/store";

export interface EditRequest {
  path: string;
  bytes: number[];
  mime: string;
  name: string;
  peerId: string;
  resolve: (sent: boolean) => void;
}

export const editorState = writable<EditRequest | null>(null);

/** Open the editor for an image. Resolves with `true` if the user
 *  sent the edited image (the caller should then NOT send the
 *  original), `false` if cancelled. */
export function editAndSend(req: Omit<EditRequest, "resolve">): Promise<boolean> {
  return new Promise((resolve) => {
    editorState.set({ ...req, resolve });
  });
}

export function closeEditor(sent = false) {
  const s = getStore(editorState);
  if (s) s.resolve(sent);
  editorState.set(null);
}
