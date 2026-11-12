/**
 * People I've written to myself (1:1). With "Who can write to me" narrowed
 * (privacy.ts), a chat I started never counts as a request — whatever the
 * list says, my own conversation stays in the chat list.
 */
import { writable, get } from "svelte/store";

const KEY = "aqua.wroteTo";
let init: string[] = [];
try { init = JSON.parse(localStorage.getItem(KEY) ?? "[]"); } catch { /* ignore */ }
export const wroteTo = writable<Set<string>>(new Set(init));
wroteTo.subscribe((v) => { try { localStorage.setItem(KEY, JSON.stringify([...v])); } catch { /* ignore */ } });

export function noteWrote(peer: string) {
  if (!peer || peer.startsWith("grp_") || peer.startsWith("__") || get(wroteTo).has(peer)) return;
  wroteTo.update((s) => new Set(s).add(peer));
}
