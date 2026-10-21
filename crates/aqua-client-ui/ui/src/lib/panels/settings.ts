/** Opening state of SettingsPanel — kept out of the component so the component itself can load lazily. */
import { writable } from "svelte/store";
export const settingsOpen = writable<boolean>(false);
/** Section to jump to on the next open (e.g. "voice" from the call UI). */
export const settingsSection = writable<string | null>(null);
export function openSettings(section?: string) {
  if (section) settingsSection.set(section);
  settingsOpen.set(true);
}
export function closeSettings() { settingsOpen.set(false); }
