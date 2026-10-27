/**
 * Chats opened in their own windows — remembered so that "Restore last
 * opened windows" (Settings → Advanced) brings them back on the next start.
 * A window leaves the list when you close it yourself, not when the app quits.
 */
import { get } from "svelte/store";
import { api } from "./tauri";
import { settings } from "./theme";

const KEY = "aqua.popouts.v1";
type Pop = { id: string; name: string };
function load(): Pop[] { try { return JSON.parse(localStorage.getItem(KEY) || "[]"); } catch { return []; } }
function save(l: Pop[]) { try { localStorage.setItem(KEY, JSON.stringify(l.slice(0, 12))); } catch { /* ignore */ } }

export async function openPopout(id: string, name: string): Promise<void> {
  await api.openChatWindow(id, name);
  save([...load().filter((p) => p.id !== id), { id, name }]);
}
/** This popout window was closed by hand. */
export function forgetPopout(id: string) { save(load().filter((p) => p.id !== id)); }

let restored = false;
export function restorePopouts() {
  if (restored || document.documentElement.classList.contains("popout")) return;
  restored = true;
  if (!get(settings).restoreWindows) return;
  for (const p of load()) void api.openChatWindow(p.id, p.name).catch(() => forgetPopout(p.id));
}
