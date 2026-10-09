/**
 * Dialog helpers:
 *   - openNewChatDialog(username?)   → "Add contact & start chat"
 *   - openFirstLaunchWizard()        → one-shot setup on fresh install
 *                                      (no server/username saved yet)
 */
import { get } from "svelte/store";
import { info, search, startChat } from "./stores";
import { askInput } from "./components/InputPrompt.svelte";
import { showToast } from "./components/Toast.svelte";

export function openNewChatDialog(prefillUsername = "") {
  const i = get(info);
  if (!i?.connected) {
    showToast("Not connected — the client is auto-retrying in the background. Try again in a moment.", 4000);
    return;
  }
  askInput({
    title: "Add contact & start chat",
    description:
      "Username is required — it's how peers are routed on the server. " +
      "Full name and phone are optional; they're saved to your local contact book so chats show the real name instead of the username.",
    fields: [
      { label: "Full name (ФИО)", placeholder: "Иван Иванов", value: "" },
      { label: "Phone",           placeholder: "+7 999 123-45-67", value: "" },
      { label: "Username *",      placeholder: "e.g. ivan2027", value: prefillUsername },
      { label: "First message *", placeholder: "Привет!" },
    ],
    submitLabel: "Send",
    onSubmit: async ([fullName, phone, peer, msg]) => {
      if (!peer || !msg) return "Username and first message are required";
      const err = await startChat(peer.trim(), msg, {
        full_name: fullName?.trim() || undefined,
        phone:     phone?.trim()    || undefined,
      });
      if (!err) search.set("");
      return err;
    },
  });
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
