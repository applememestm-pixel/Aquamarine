/** The password dialogs for locked chats — from the chat menu and from
 *  Settings → Privacy → Locked chats. */
import { get } from "svelte/store";
import { askInput, closeInput } from "./components/InputPrompt.svelte";
import { showToast } from "./components/Toast.svelte";
import { tr } from "./i18n";
import { activeId } from "./stores";
import { setChatLock, removeChatLock, unlockChat, lockNowChat } from "./chatLocks";
import { hasPasscode, checkPasscode, confirmBiometric } from "./lock";

export function askSetLock(id: string) {
  askInput({
    title: tr("Password for this chat"),
    description: tr("The chat's name and messages are hidden until you enter it. Only you are affected; it goes with your encrypted backup to your other devices."),
    fields: [{ label: tr("Password"), secret: true }, { label: tr("Repeat password"), secret: true }, { label: tr("Hint (optional)") }],
    submitLabel: tr("Set password"),
    onSubmit: async ([a, b, hint]) => {
      if ((a ?? "").length < 4) return tr("At least 4 characters");
      if (a !== b) return tr("Passwords don't match");
      await setChatLock(id, a, hint);
      if (get(activeId) === id) lockNowChat(id);
      closeInput();
      showToast(tr("The chat is locked"));
      return null;
    },
  });
}

/** Taking a chat's password off asks for the app's passcode (the one the
 *  whole messenger opens with) — or the fingerprint — not the chat's own:
 *  it's the owner's decision, like Telegram. With no app passcode set, the
 *  chat's password still does. */
export async function askRemoveLock(id: string) {
  const done = () => { removeChatLock(id); closeInput(); showToast(tr("Password removed")); };
  if (await confirmBiometric(tr("Remove the password"))) { done(); return; }
  const app = hasPasscode();
  askInput({
    title: tr("Remove the password"),
    description: app ? tr("Enter the passcode of Aquamarine.") : undefined,
    fields: [{ label: app ? tr("App passcode") : tr("Current password"), secret: true }],
    submitLabel: tr("Remove"),
    onSubmit: async ([p]) => {
      const ok = app ? await checkPasscode(p ?? "") : await unlockChat(id, p ?? "");
      if (!ok) return tr(app ? "Wrong passcode" : "Wrong password");
      done();
      return null;
    },
  });
}

export function askChangeLock(id: string) {
  askInput({
    title: tr("Change the password"),
    fields: [{ label: tr("Current password"), secret: true }, { label: tr("New password"), secret: true }, { label: tr("Repeat password"), secret: true }, { label: tr("Hint (optional)") }],
    submitLabel: tr("Change"),
    onSubmit: async ([cur, a, b, hint]) => {
      if (!(await unlockChat(id, cur ?? ""))) return tr("Wrong password");
      if ((a ?? "").length < 4) return tr("At least 4 characters");
      if (a !== b) return tr("Passwords don't match");
      await setChatLock(id, a, hint);
      closeInput();
      showToast(tr("Password changed"));
      return null;
    },
  });
}
