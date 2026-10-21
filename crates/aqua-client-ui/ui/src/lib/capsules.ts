/**
 * Sealed messages ("a message to a trusted person"): written now, readable
 * only when a condition is met — a date, or the sender being away for N
 * days — and, if the sender wants, only after they had a window to say no.
 *
 * Split knowledge: the text is encrypted with a random key (AES-GCM, on
 * this device). The ciphertext goes to the recipient now, end-to-end
 * encrypted like any message; the key goes to the sender's relay with the
 * condition. Neither of them can read it alone until the relay releases the
 * key to the recipient.
 *
 *   [capsule:v1]{"id","ct","iv","hint","kind":"date"|"inactive","open_at"?,"days"?,"grace_h","relay","from"}
 */
import { writable, get } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { info } from "./stores";

export interface Capsule {
  id: string; ct: string; iv: string; hint: string;
  kind: "date" | "inactive"; open_at?: number; days?: number; grace_h: number;
  relay: string; from: string;
}

function b64(b: ArrayBuffer | Uint8Array): string {
  const u = b instanceof Uint8Array ? b : new Uint8Array(b);
  let s = "";
  for (let i = 0; i < u.length; i += 0x8000) s += String.fromCharCode(...u.subarray(i, i + 0x8000));
  return btoa(s);
}
const unb64 = (s: string) => Uint8Array.from(atob(s), (c) => c.charCodeAt(0));
const hex = (b: Uint8Array) => [...b].map((x) => x.toString(16).padStart(2, "0")).join("");
const unhex = (s: string) => new Uint8Array((s.match(/../g) ?? []).map((h) => parseInt(h, 16)));

export function parseCapsule(text: string): Capsule | null {
  if (!text.startsWith("[capsule:v1]")) return null;
  try {
    const v = JSON.parse(text.slice(12));
    if (typeof v?.id !== "string" || typeof v?.ct !== "string" || typeof v?.iv !== "string" || typeof v?.relay !== "string") return null;
    if (v.kind !== "date" && v.kind !== "inactive") return null;
    return {
      id: v.id.slice(0, 64), ct: v.ct.slice(0, 200_000), iv: v.iv.slice(0, 40), hint: typeof v.hint === "string" ? v.hint.slice(0, 200) : "",
      kind: v.kind, open_at: typeof v.open_at === "number" ? v.open_at : undefined, days: typeof v.days === "number" ? v.days : undefined,
      grace_h: typeof v.grace_h === "number" ? v.grace_h : 0, relay: v.relay.slice(0, 200), from: typeof v.from === "string" ? v.from.slice(0, 40) : "",
    };
  } catch { return null; }
}

/** Seal `text`: registers the key on my relay, returns the message to send. */
export async function seal(recipient: string, text: string, hint: string, cond: { kind: "date"; open_at: number } | { kind: "inactive"; days: number }, grace_h: number): Promise<string> {
  const keyBytes = crypto.getRandomValues(new Uint8Array(32));
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const key = await crypto.subtle.importKey("raw", keyBytes, "AES-GCM", false, ["encrypt"]);
  const ct = await crypto.subtle.encrypt({ name: "AES-GCM", iv }, key, new TextEncoder().encode(text));
  const id = "cap-" + hex(crypto.getRandomValues(new Uint8Array(10)));
  await invoke("capsule_create", { body: { id, recipient, key: hex(keyBytes), kind: cond.kind, open_at: cond.kind === "date" ? cond.open_at : 0, days: cond.kind === "inactive" ? cond.days : 0, grace_h } });
  keyBytes.fill(0);
  const i = get(info);
  const msg: Capsule = { id, ct: b64(ct), iv: b64(iv), hint: hint.slice(0, 200), ...cond, grace_h, relay: i?.server_url ?? "", from: i?.username ?? "" } as Capsule;
  return "[capsule:v1]" + JSON.stringify(msg);
}

/** Opened sealed messages, kept on this device. */
export const openedCapsules = writable<Record<string, string>>((() => { try { return JSON.parse(localStorage.getItem("aqua.capsulesOpened.v1") ?? "{}"); } catch { return {}; } })());
openedCapsules.subscribe((v) => { try { localStorage.setItem("aqua.capsulesOpened.v1", JSON.stringify(v)); } catch { /* ignore */ } });

export type OpenResult = { state: "open"; text: string } | { state: "sealed"; until?: number; inactive_days?: number } | { state: "waiting"; until: number } | { state: "cancelled" };

export async function tryOpen(c: Capsule): Promise<OpenResult> {
  const done = get(openedCapsules)[c.id];
  if (done !== undefined) return { state: "open", text: done };
  const r = await invoke<Record<string, unknown>>("capsule_open", { serverUrl: c.relay, id: c.id });
  if (r.state === "open" && typeof r.key === "string") {
    const key = await crypto.subtle.importKey("raw", unhex(r.key), "AES-GCM", false, ["decrypt"]);
    const pt = await crypto.subtle.decrypt({ name: "AES-GCM", iv: unb64(c.iv) }, key, unb64(c.ct));
    const text = new TextDecoder().decode(pt);
    openedCapsules.update((o) => ({ ...o, [c.id]: text }));
    return { state: "open", text };
  }
  if (r.state === "waiting") return { state: "waiting", until: Number(r.until) || 0 };
  if (r.state === "cancelled") return { state: "cancelled" };
  return { state: "sealed", until: typeof r.until === "number" ? r.until : undefined, inactive_days: typeof r.inactive_days === "number" ? r.inactive_days : undefined };
}

/** Sender: requests to open my sealed messages that I can still deny. */
export interface MyCapsule { id: string; recipient: string; kind: string; open_at: number; days: number; grace_h: number; created: number; requested: number; released: number; cancelled: boolean }
export const myCapsules = writable<MyCapsule[]>([]);
export async function refreshMyCapsules() {
  try { myCapsules.set(await invoke<MyCapsule[]>("capsules_mine")); } catch { /* offline */ }
}
export async function stopCapsule(id: string, forever: boolean) {
  await invoke("capsule_stop", { id, forever });
  await refreshMyCapsules();
}
let started = false;
export function startCapsules() {
  if (started) return;
  started = true;
  setTimeout(refreshMyCapsules, 8000);
  setInterval(refreshMyCapsules, 20 * 60_000);
  // Coming back to the app is when a pending request matters most.
  let last = 0;
  window.addEventListener("focus", () => { if (Date.now() - last > 60_000) { last = Date.now(); void refreshMyCapsules(); } });
}
