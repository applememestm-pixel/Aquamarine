/**
 * "Send message as…" (Telegram): in a group or a channel's comments, write
 * as yourself, as this group / channel, or as another one you run.
 *
 * The claim rides in the group envelope (`as`, commands.rs::group_send).
 * Nothing is shown on the claim alone:
 *  - `ok` — checked on arrival: the sender is an admin of that group, which
 *    the receiver is in (this group itself, or one it shares);
 *  - otherwise a public channel's @name is looked up and its owner must be
 *    the sender. Anything else shows the real person.
 */
import { writable, get } from "svelte/store";
import { api } from "./tauri";

export interface AsClaim { gid: string; title: string; kind: "group" | "supergroup" | "channel"; name?: string; ok?: boolean }

const KEY = "aqua.sendAs";
function load(): Record<string, string> {
  try { const v = JSON.parse(localStorage.getItem(KEY) ?? "{}"); return v && typeof v === "object" ? v : {}; } catch { return {}; }
}
/** Chat id → gid I write as there ("" or missing: myself). */
export const sendAsPick = writable<Record<string, string>>(load());
sendAsPick.subscribe((v) => { try { localStorage.setItem(KEY, JSON.stringify(v)); } catch { /* private mode */ } });
export function setSendAs(chat: string, gid: string) {
  sendAsPick.update((m) => { const n = { ...m }; if (gid) n[chat] = gid; else delete n[chat]; return n; });
}
export function sendAsFor(chat: string): string | undefined {
  return get(sendAsPick)[chat] || undefined;
}

/** `name|sender` → checked against the public @name's owner. */
export const asVerified = writable<Record<string, boolean>>({});
const asking = new Set<string>();

function claimOf(meta: unknown): AsClaim | null {
  const a = (meta as { as?: unknown } | null | undefined)?.as as Record<string, unknown> | undefined;
  if (!a || typeof a.gid !== "string" || !a.gid.startsWith("grp_")) return null;
  const kind = a.kind === "group" || a.kind === "supergroup" ? a.kind : "channel";
  return { gid: a.gid, title: typeof a.title === "string" ? a.title : "", kind, name: typeof a.name === "string" ? a.name : "", ok: a.ok === true };
}

/** Who a message is shown as — the group / channel, once checked; else null
 *  (show the real sender). Pass `$asVerified` so the caller re-renders. */
export function shownAs(meta: unknown, from: string, verified: Record<string, boolean>): AsClaim | null {
  const a = claimOf(meta);
  if (!a) return null;
  if (a.ok) return a;
  if (!a.name) return null;
  const k = `${a.name}|${from}`;
  if (k in verified) return verified[k] ? a : null;
  if (!asking.has(k)) {
    asking.add(k);
    void api.groupLookup(a.name).then((g) => {
      const ok = !!g && g.gid === a.gid && g.admin === from;
      asVerified.update((v) => ({ ...v, [k]: ok }));
    }).catch(() => { asking.delete(k); });
  }
  return null;
}
