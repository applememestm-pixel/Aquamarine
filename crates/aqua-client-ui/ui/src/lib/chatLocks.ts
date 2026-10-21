/**
 * Passwords on single chats (only for me): a locked chat shows a blurred
 * name and no preview in the list, and asks for its password when opened.
 * Each chat can have its own password. Stored as salted PBKDF2-SHA256 hashes
 * in this device's settings, which travel inside the encrypted backup — so
 * the locks follow the account to other devices (the relays keep the backup
 * but can't read it). The other side of the chat sees nothing.
 */
import { writable, get } from "svelte/store";

export interface ChatLock { salt: string; hash: string; hint?: string; at: number }

const KEY = "aqua.chatLocks.v1";
function load(): Record<string, ChatLock> {
  try { return JSON.parse(localStorage.getItem(KEY) ?? "{}"); } catch { return {}; }
}
export const chatLocks = writable<Record<string, ChatLock>>(load());
chatLocks.subscribe((v) => { try { localStorage.setItem(KEY, JSON.stringify(v)); } catch { /* ignore */ } });

/** Chats unlocked in this session (lock again when you leave them for a while). */
export const unlocked = writable<Record<string, number>>({});
/** Minutes a chat stays open after you leave it. */
export const RELOCK_MIN = 5;

const hex = (b: ArrayBuffer) => [...new Uint8Array(b)].map((x) => x.toString(16).padStart(2, "0")).join("");

async function derive(pass: string, saltHex: string): Promise<string> {
  const salt = new Uint8Array(saltHex.match(/../g)!.map((h) => parseInt(h, 16)));
  const key = await crypto.subtle.importKey("raw", new TextEncoder().encode(pass.normalize("NFC")), "PBKDF2", false, ["deriveBits"]);
  const bits = await crypto.subtle.deriveBits({ name: "PBKDF2", hash: "SHA-256", salt, iterations: 200_000 }, key, 256);
  return hex(bits);
}

export function isLocked(chat: string): boolean {
  return !!get(chatLocks)[chat] && !get(unlocked)[chat];
}

export async function setChatLock(chat: string, pass: string, hint?: string) {
  const salt = hex(crypto.getRandomValues(new Uint8Array(16)).buffer);
  const hash = await derive(pass, salt);
  chatLocks.update((l) => ({ ...l, [chat]: { salt, hash, hint: hint?.trim().slice(0, 60) || undefined, at: Date.now() } }));
  unlocked.update((u) => ({ ...u, [chat]: Date.now() }));
}

export function removeChatLock(chat: string) {
  chatLocks.update((l) => { const n = { ...l }; delete n[chat]; return n; });
}

/** Try a password; true = the chat opens. Slows down guessing a little. */
let fails = 0;
export async function unlockChat(chat: string, pass: string): Promise<boolean> {
  const l = get(chatLocks)[chat];
  if (!l) return true;
  if (fails > 3) await new Promise((r) => setTimeout(r, Math.min(8000, 500 * 2 ** (fails - 4))));
  const ok = (await derive(pass, l.salt)) === l.hash;
  if (ok) { fails = 0; unlocked.update((u) => ({ ...u, [chat]: Date.now() })); }
  else fails++;
  return ok;
}

export function lockNowChat(chat: string) {
  unlocked.update((u) => { const n = { ...u }; delete n[chat]; return n; });
}

/** Leaving a chat: it locks again RELOCK_MIN minutes later. */
export function noteLeft(chat: string) {
  if (!get(chatLocks)[chat]) return;
  const at = Date.now();
  setTimeout(() => {
    const u = get(unlocked)[chat];
    if (u && u <= at) lockNowChat(chat);
  }, RELOCK_MIN * 60_000);
  unlocked.update((u) => (u[chat] ? { ...u, [chat]: at } : u));
}
