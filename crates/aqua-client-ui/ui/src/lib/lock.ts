/**
 * Passcode lock (Telegram's "Passcode lock"): the app asks for a passcode on
 * start and after N minutes without activity.
 *
 * The passcode lives in the vault (src/applock.rs), not in this webview's
 * storage: deleting the webview's data folder used to remove it and open
 * every chat. Here only the salted PBKDF2-SHA256 hash is derived (the
 * passcode itself never leaves the page); while locked, every command of
 * the app is refused on the Rust side, so taking this screen away gets
 * nothing either. The store keeps `hash: "set"` as a marker of "a passcode
 * exists" for the screens that ask that.
 */
import { writable, get } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";

interface LockConf { hash: string; salt: string; autoMin: number; skipped?: boolean; /** "pin": 4 digits on a keypad (Telegram); otherwise a password. */ kind?: "pin" | "password"; /** Phones: fingerprint / face opens it too. */ bio?: boolean }
interface RustState { has: boolean; locked: boolean; salt: string; kind: string; auto_min: number; bio: boolean; wait_ms: number }

/** Before 1.20.1-3 the whole conf (hash included) lived here; now only
 *  "the user said not now" does. */
const KEY = "aqua.lock.v1";
function loadLocal(): LockConf | null {
  try { return JSON.parse(localStorage.getItem(KEY) ?? "null"); } catch { return null; }
}
function saveLocal(c: LockConf | null) {
  try {
    if (c?.skipped && !c.hash) localStorage.setItem(KEY, JSON.stringify({ hash: "", salt: "", autoMin: 0, skipped: true }));
    else localStorage.removeItem(KEY);
  } catch { /* ignore */ }
}

const legacy = loadLocal();
export const lockConf = writable<LockConf | null>(legacy?.hash ? { ...legacy, hash: "set" } : legacy);
lockConf.subscribe(saveLocal);

/** Locked right now? A passcode kept here before 1.20.1-3 locks at once;
 *  otherwise the vault says so a moment later (loadLock). */
export const locked = writable<boolean>(!!legacy?.hash);
/** The vault has answered (loadLock): until then "no passcode" isn't known. */
export const lockLoaded = writable<boolean>(false);
/** Show the "set a passcode" screen (after sign-up / first launch). */
export const passcodeSetupOpen = writable<boolean>(false);
/** The app started locked: what it loaded behind the lock was refused, so
 *  the first unlock reloads the page. */
let startedLocked = !!legacy?.hash;

const hex = (b: ArrayBuffer) => [...new Uint8Array(b)].map((x) => x.toString(16).padStart(2, "0")).join("");

async function derive(pass: string, saltHex: string): Promise<string> {
  const salt = new Uint8Array(saltHex.match(/../g)!.map((h) => parseInt(h, 16)));
  const key = await crypto.subtle.importKey("raw", new TextEncoder().encode(pass), "PBKDF2", false, ["deriveBits"]);
  const bits = await crypto.subtle.deriveBits({ name: "PBKDF2", hash: "SHA-256", salt, iterations: 150_000 }, key, 256);
  return hex(bits);
}

let rustSalt = "";
function adopt(s: RustState) {
  rustSalt = s.salt;
  const skipped = get(lockConf)?.skipped;
  lockConf.set(s.has ? { hash: "set", salt: s.salt, autoMin: s.auto_min, kind: s.kind === "pin" ? "pin" : "password", bio: s.bio } : (skipped ? { hash: "", salt: "", autoMin: 0, skipped: true } : null));
  if (s.locked) { startedLocked = startedLocked || !get(locked); locked.set(true); }
}

/** At start: the vault's word on the passcode; a passcode still kept in
 *  this webview (before 1.20.1-3) moves to the vault. */
export async function loadLock(): Promise<void> {
  try {
    let s = await invoke<RustState>("applock_state");
    if (!s.has && legacy?.hash && /^[0-9a-f]{64}$/i.test(legacy.hash)) {
      await invoke("applock_set", { hash: legacy.hash, salt: legacy.salt, kind: legacy.kind ?? "password", autoMin: legacy.autoMin ?? 0 });
      if (legacy.bio) await invoke("applock_options", { autoMin: null, bio: true });
      await invoke("applock_lock");
      s = await invoke<RustState>("applock_state");
    }
    adopt(s);
  } catch { /* not in the app (browser preview) */ }
  lockLoaded.set(true);
}

export function hasPasscode(): boolean { return !!get(lockConf)?.hash; }

export async function setPasscode(pass: string, autoMin = 5, kind: "pin" | "password" = "password") {
  const salt = hex(crypto.getRandomValues(new Uint8Array(16)).buffer);
  const hash = await derive(pass, salt);
  await invoke("applock_set", { hash, salt, kind, autoMin });
  rustSalt = salt;
  lockConf.set({ hash: "set", salt, autoMin, kind });
  locked.set(false);
}

export function setAutoLock(min: number) {
  const c = get(lockConf);
  if (!c) return;
  lockConf.set({ ...c, autoMin: min });
  void invoke("applock_options", { autoMin: min, bio: null }).catch(() => {});
}

function opened() {
  locked.set(false);
  bump();
  if (startedLocked) {
    startedLocked = false;
    // Everything the app tried to load behind the lock was refused: start over, now open.
    location.reload();
  }
}

/** Wrong tries make the next one wait (applock.rs); the reply says how long. */
export async function unlock(pass: string): Promise<boolean> {
  const c = get(lockConf);
  if (!c?.hash) { locked.set(false); return true; }
  try {
    const ok = await invoke<boolean>("applock_unlock", { hash: await derive(pass, rustSalt || c.salt) });
    if (ok) opened();
    return ok;
  } catch (e) {
    const m = /wait:(\d+)/.exec(String(e));
    if (m) throw new Error(`wait:${m[1]}`);
    return false;
  }
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
  if (!c) return;
  lockConf.set({ ...c, bio: on });
  void invoke("applock_options", { autoMin: null, bio: on }).catch(() => {});
}

/** Ask for the fingerprint / face; true = unlocked. The passcode keeps
 *  working as the fallback. */
export async function unlockBiometric(reason: string): Promise<boolean> {
  const c = get(lockConf);
  if (!c?.bio || !(await biometricAvailable())) return false;
  try {
    const { authenticate } = await import("@tauri-apps/plugin-biometric");
    await authenticate(reason, { allowDeviceCredential: false, cancelTitle: "", fallbackTitle: "" });
    if (!(await invoke<boolean>("applock_unlock_bio"))) return false;
    opened();
    return true;
  } catch { return false; }
}

/** Check the app's passcode without touching the lock (removing a chat's
 *  password asks for it). Only while unlocked. */
export async function checkPasscode(pass: string): Promise<boolean> {
  const c = get(lockConf);
  if (!c?.hash || get(locked)) return false;
  try { return await invoke<boolean>("applock_unlock", { hash: await derive(pass, rustSalt || c.salt) }); } catch { return false; }
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

export function lockNow() {
  if (!hasPasscode()) return;
  locked.set(true);
  void invoke("applock_lock").catch(() => {});
}

/** Forgot the passcode: a code to the account's e-mail (applock.rs). */
export async function forgotSend(): Promise<string> {
  const r = await invoke<{ sent_to: string }>("applock_forgot_send");
  return r.sent_to;
}
export async function forgotCheck(code: string): Promise<boolean> {
  const ok = await invoke<boolean>("applock_forgot_check", { code });
  if (ok) { lockConf.set({ hash: "", salt: "", autoMin: 0, skipped: true }); opened(); }
  return ok;
}

// ---- inactivity timer ----
let last = Date.now();
function bump() { last = Date.now(); }
let started = false;
export function initLock() {
  if (started) return;
  started = true;
  void loadLock();
  for (const ev of ["pointerdown", "keydown", "wheel", "pointermove"]) window.addEventListener(ev, bump, { passive: true });
  setInterval(() => {
    const c = get(lockConf);
    if (!c?.hash || !c.autoMin || get(locked)) return;
    if (Date.now() - last > c.autoMin * 60_000) lockNow();
  }, 5000);
}
