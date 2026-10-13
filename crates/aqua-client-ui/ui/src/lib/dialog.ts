/**
 * The native "open file" dialog, with a hook for end-to-end tests: a GTK /
 * Win32 dialog can't be driven over WebDriver, so a test pushes the answer
 * into `window.__aquaTestPick` first. Nothing sets it in normal use.
 */
import { open, type OpenDialogOptions } from "@tauri-apps/plugin-dialog";

export function openFileDialog<T extends OpenDialogOptions>(opts: T): ReturnType<typeof open<T>> {
  const q = (window as unknown as { __aquaTestPick?: unknown[] }).__aquaTestPick;
  if (q && q.length) return Promise.resolve(q.shift()) as ReturnType<typeof open<T>>;
  return open(opts);
}
