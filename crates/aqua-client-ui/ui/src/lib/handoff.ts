/**
 * Seamless handoff between my devices: when this app goes to the background
 * it tells my other devices (only them, end-to-end encrypted) which chat was
 * open and what was being typed; they offer "Continue here".
 */
import { writable, get } from "svelte/store";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { activeId, drafts, selectChat } from "./stores";

export interface HandoffOffer { chat: string; draft: string; dev: string; at: number }
export const handoffOffer = writable<HandoffOffer | null>(null);

const KEY = "aqua.handoff.v1";
export const handoffOn = writable<boolean>((() => { try { return localStorage.getItem(KEY) !== "0"; } catch { return true; } })());
handoffOn.subscribe((v) => { try { localStorage.setItem(KEY, v ? "1" : "0"); } catch { /* ignore */ } });

let last = "";
let lastAt = 0;
function send() {
  if (!get(handoffOn)) return;
  const chat = get(activeId);
  if (!chat || chat.startsWith("__") || chat === "roma") return;
  const draft = get(drafts)[chat] ?? "";
  const sig = `${chat}\n${draft}`;
  const now = Date.now();
  if (sig === last && now - lastAt < 10 * 60_000) return;
  if (now - lastAt < 15_000) return;
  last = sig; lastAt = now;
  void invoke("handoff", { chat, draft }).catch(() => {});
}

let started = false;
export function startHandoff() {
  if (started) return;
  started = true;
  document.addEventListener("visibilitychange", () => { if (document.visibilityState === "hidden") send(); });
  window.addEventListener("pagehide", send);
  void listen<{ chat: string; body: Record<string, unknown>; own?: boolean }>("aqua://ctl", (e) => {
    const b = e.payload.body;
    if (!e.payload.own || b?.t !== "handoff") return;
    const at = Number(b.at) || Date.now();
    if (Date.now() - at > 10 * 60_000) return;
    const offer: HandoffOffer = { chat: e.payload.chat, draft: typeof b.draft === "string" ? b.draft.slice(0, 4000) : "", dev: typeof b.dev === "string" ? b.dev.slice(0, 60) : "", at };
    // Already here with the same text: nothing to offer.
    if (document.visibilityState === "visible" && get(activeId) === offer.chat && (get(drafts)[offer.chat] ?? "") === offer.draft) return;
    handoffOffer.set(offer);
    setTimeout(() => { if (get(handoffOffer) === offer) handoffOffer.set(null); }, 3 * 60_000);
  }).catch(() => {});
}

/** Take the offer: open the chat with the other device's draft. */
export async function acceptHandoff() {
  const o = get(handoffOffer);
  if (!o) return;
  handoffOffer.set(null);
  if (o.draft) drafts.update((d) => ({ ...d, [o.chat]: o.draft }));
  await selectChat(o.chat);
}
