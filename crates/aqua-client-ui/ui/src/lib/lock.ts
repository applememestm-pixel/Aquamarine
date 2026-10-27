/**
 * Local passcode lock (Telegram's "Passcode lock"): the app asks for a
 * passcode on start and after N minutes without activity. The passcode is
 * stored as a salted PBKDF2-SHA256 hash on this device only.
 */
import { writable, get } from "svelte/store";

interface LockConf { hash: string; salt: string; autoMin: number; skipped?: boolean; /** "pin": 4 digits on a keypad (Telegram); otherwise a password. */ kind?: "pin" | "password"; /** Phones: fingerprint / face opens it too. */ bio?: boolean }
const KEY = "aqua.lock.v1";

function loadConf(): LockConf | null {
  try { return JSON.parse(localStorage.getItem(KEY) ?? "null"); } catch { return null; }
}
function saveConf(c: LockConf | null) {
  try { if (c) localStorage.setItem(KEY, JSON.stringify(c)); else localStorage.removeItem(KEY); } catch { /* ignore */ }
}

export const lockConf = writable<LockConf | null>(loadConf());
lockConf.subscribe(saveConf);

/** Locked right now? Starts locked when a passcode exists. */
export const locked = writable<boolean>(!!loadConf()?.hash);
/** Show the "set a passcode" screen (after sign-up / first launch). */
export const passcodeSetupOpen = writable<boolean>(false);

const hex = (b: ArrayBuffer) => [...new Uint8Array(b)].map((x) => x.toString(16).padStart(2, "0")).join("");

async function derive(pass: string, saltHex: string): Promise<string> {
  const salt = new Uint8Array(saltHex.match(/../g)!.map((h) => parseInt(h, 16)));
  const key = await crypto.subtle.importKey("raw", new TextEncoder().encode(pass), "PBKDF2", false, ["deriveBits"]);
  const bits = await crypto.subtle.deriveBits({ name: "PBKDF2", hash: "SHA-256", salt, iterations: 150_000 }, key, 256);
  return hex(bits);
}

export function hasPasscode(): boolean { return !!get(lockConf)?.hash; }

export async function setPasscode(pass: string, autoMin = 5, kind: "pin" | "password" = "password") {
  const salt = hex(crypto.getRandomValues(new Uint8Array(16)).buffer);
  lockConf.set({ hash: await derive(pass, salt), salt, autoMin, kind });
  locked.set(false);
}

export function removePasscode() { lockConf.set(null); locked.set(false); }

export function setAutoLock(min: number) {
  const c = get(lockConf);
  if (c) lockConf.set({ ...c, autoMin: min });
}

export async function unlock(pass: string): Promise<boolean> {
  const c = get(lockConf);
  if (!c?.hash) { locked.set(false); return true; }
  const ok = (await derive(pass, c.salt)) === c.hash;
  if (ok) { locked.set(false); bump(); }
  return ok;
}

// ---- fingerprint / face (Android, iOS): tauri-plugin-biometric ----
const PHONE = typeof document !== "undefined" && /android|iphone|ipad/i.test(navigator.userAgent);

/** Can this device unlock with a fingerprint or the face? */
export async function biometricAvailable(): Promise<boolean> {
  if (!PHONE || !("__TAURI_INTERNALS__" in window)) return false;
  try {
    const { checkStatus } = await import("@tauri-apps/plugin-biometric");
    return (await checkStatus()).isAvailable;
  } catch { return false; }
}

export function setBiometric(on: boolean) {
  const c = get(lockConf);
  if (c) lockConf.set({ ...c, bio: on });
}

/** Ask for the fingerprint / face; true = unlocked. The passcode keeps
 *  working as the fallback. */
export async function unlockBiometric(reason: string): Promise<boolean> {
  const c = get(lockConf);
  if (!c?.bio || !(await biometricAvailable())) return false;
  try {
    const { authenticate } = await import("@tauri-apps/plugin-biometric");
    await authenticate(reason, { allowDeviceCredential: false, cancelTitle: "", fallbackTitle: "" });
    locked.set(false);
    bump();
    return true;
  } catch { return false; }
}

/** Check the app's passcode without touching the lock (removing a chat's
 *  password asks for it). */
export async function checkPasscode(pass: string): Promise<boolean> {
  const c = get(lockConf);
  if (!c?.hash) return false;
  return (await derive(pass, c.salt)) === c.hash;
}

/** Fingerprint / face as proof that it's the owner (a locked chat, removing
 *  a chat's password). Works whenever the device has it. */
export async function confirmBiometric(reason: string): Promise<boolean> {
  if (!(await biometricAvailable())) return false;
  try {
    const { authenticate } = await import("@tauri-apps/plugin-biometric");
    await authenticate(reason, { allowDeviceCredential: false, cancelTitle: "", fallbackTitle: "" });
    return true;
  } catch { return false; }
}

export function lockNow() { if (hasPasscode()) locked.set(true); }

// ---- inactivity timer ----
let last = Date.now();
function bump() { last = Date.now(); }
let started = false;
export function initLock() {
  if (started) return;
  started = true;
  for (const ev of ["pointerdown", "keydown", "wheel", "pointermove"]) window.addEventListener(ev, bump, { passive: true });
  setInterval(() => {
    const c = get(lockConf);
    if (!c?.hash || !c.autoMin || get(locked)) return;
    if (Date.now() - last > c.autoMin * 60_000) locked.set(true);
  }, 5000);
}
