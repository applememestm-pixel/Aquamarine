/** The password dialogs for locked chats — from the chat menu and from
 *  Settings → Privacy → Locked chats. */
import { get } from "svelte/store";
import { askInput, closeInput } from "./components/InputPrompt.svelte";
import { showToast } from "./components/Toast.svelte";
import { tr } from "./i18n";
import { activeId } from "./stores";
import { setChatLock, removeChatLock, unlockChat, lockNowChat } from "./chatLocks";

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

export function askRemoveLock(id: string) {
  askInput({
    title: tr("Remove the password"),
    fields: [{ label: tr("Current password"), secret: true }],
    submitLabel: tr("Remove"),
    onSubmit: async ([p]) => {
      if (!(await unlockChat(id, p ?? ""))) return tr("Wrong password");
      removeChatLock(id);
      closeInput();
      showToast(tr("Password removed"));
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
