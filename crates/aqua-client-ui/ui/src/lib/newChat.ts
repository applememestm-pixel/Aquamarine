/**
 * Dialog helpers:
 *   - openNewChatDialog(username?)   → "New message" (NewChatSheet)
 *   - openFirstLaunchWizard()        → one-shot setup on fresh install
 *                                      (no server/username saved yet)
 */

import { openNewChat } from "./components/NewChatSheet.svelte";

export function openNewChatDialog(prefillUsername = "") {
  // Telegram's "New message": search, pick a person, write.
  openNewChat(prefillUsername);
}

/**
 * First-launch flow. Opens the full registration wizard (email → code
 * → @username + profile). After a successful sign-in the hints are
 * persisted, and from then on the next launch reconnects silently.
 */
export async function openFirstLaunchWizard() {
  const m = await import("./components/RegistrationWizard.svelte");
  m.openRegistration();
}
