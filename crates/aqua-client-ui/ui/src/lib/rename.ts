/**
 * @username changes.
 *
 * Mine: the relay renames the account (Rust `change_username` also tells
 * every contact), then we reconnect under the new name.
 * Theirs: Rust moved the chat and sessions (`aqua://renamed {old, new}`);
 * here the things the UI keeps per chat id follow — the open chat, the
 * contact book entry, archived / muted / pinned.
 */
import { get } from "svelte/store";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { info, activeId, chats, connect } from "./stores";
import { api } from "./tauri";
import { contacts } from "./contacts";
import { archived, mutedChats, pinnedChats } from "./chatPrefs";

export const USERNAME_RE = /^[a-z0-9_.]{3,32}$/;

/** Change my @username; resolves to the name as stored. */
export async function changeMyUsername(wanted: string): Promise<string> {
  const name = wanted.trim().replace(/^@+/, "").toLowerCase();
  if (!USERNAME_RE.test(name)) throw new Error("bad username");
  const old = get(info)?.username ?? "";
  const got = await invoke<string>("change_username", { username: name });
  if (got !== old) {
    const url = get(info)?.server_url;
    if (url) await connect(url, got);
    else info.update((i) => (i ? { ...i, username: got } : i));
  }
  return got;
}

function moveKey(old: string, nw: string) {
  if (get(activeId) === old) activeId.set(nw);
  contacts.update((c) => {
    if (!c[old]) return c;
    const { [old]: entry, ...rest } = c;
    return { ...rest, [nw]: { ...entry, ...(rest[nw] ?? {}) } };
  });
  for (const set of [archived, mutedChats]) {
    set.update((s) => { if (!s.has(old)) return s; const n = new Set(s); n.delete(old); n.add(nw); return n; });
  }
  pinnedChats.update((l) => l.map((x) => (x === old ? nw : x)));
}

export function startRenames(): () => void {
  let un: (() => void) | null = null;
  // Renamed on another device of mine: reconnect under the new name here.
  let un2: (() => void) | null = null;
  void listen<{ old: string; new: string }>("aqua://self_renamed", async (e) => {
    const url = get(info)?.server_url;
    if (url) await connect(url, e.payload.new);
  }).then((u) => (un2 = u));
  void listen<{ old: string; new: string }>("aqua://renamed", async (e) => {
    moveKey(e.payload.old, e.payload.new);
    try { chats.set(await api.listChats()); } catch { /* next refresh */ }
  }).then((u) => (un = u));
  return () => { un?.(); un2?.(); }
}
