/**
 * Encrypted backups from the UI side: what of the UI's own state goes into
 * a backup (localStorage settings), restoring it, and the periodic auto
 * backup. The sealing itself happens in Rust (aqua-client-core::backup).
 */
import { get, writable } from "svelte/store";
import { api, type BackupStatus } from "./tauri";
import { groups } from "./groups";
import { chats } from "./stores";
import { profile } from "./profile";

/** Device-only or regenerable entries that don't belong in a backup. */
const SKIP = [/^aqua\.reactionPack\.cache/, /^aqua\.tgBotToken/, /^aqua\.lock\./, /^aqua\.player\./, /^aqua\.picker\./];

export function collectUiState(): string {
  const out: Record<string, string> = {};
  try {
    for (let i = 0; i < localStorage.length; i++) {
      const k = localStorage.key(i);
      if (!k || !k.startsWith("aqua") || SKIP.some((re) => re.test(k))) continue;
      const v = localStorage.getItem(k);
      if (v != null) out[k] = v;
    }
  } catch { /* ignore */ }
  return JSON.stringify(out);
}

/** Put a restored backup's settings back (then the app reloads). */
export function applyUiState(json: string): void {
  if (!json) return;
  try {
    const m = JSON.parse(json) as Record<string, string>;
    for (const [k, v] of Object.entries(m)) if (k.startsWith("aqua") && typeof v === "string") localStorage.setItem(k, v);
  } catch { /* ignore */ }
}

export const backupState = writable<BackupStatus | null>(null);

export async function refreshBackup(): Promise<BackupStatus | null> {
  try { const s = await api.backupStatus(); backupState.set(s); return s; } catch { return null; }
}

export async function backupNow(force = true): Promise<BackupStatus> {
  const s = await api.backupNow(collectUiState(), force);
  backupState.set(s);
  return s;
}

// ---- automatic backups ------------------------------------------------------
//
// The backup is the account's "main data" on the relay (and, through
// federation, on every relay of the network). It is re-uploaded:
//   * within ~20 s of an important change — groups (membership, settings,
//     rooms, roles), the chat list / contacts, the profile, settings,
//     folders, pins, privacy, blocks — at most once every 2 minutes;
//   * after new messages, at most every 20 minutes;
//   * and at least every 6 hours.
const NOISY = [/^aqua\.rooms\.(seen|active)/, /^aqua\.played/, /^aqua\.recent\./, /^aqua\.peerCards/, /^aqua\.listWidth/,
  /^aqua\.reader\./, /^aqua\.gameIds/, /^aqua\.call\./, /^aqua\.onlineStatus/];
const MIN_GAP = 2 * 60_000, MSG_GAP = 20 * 60_000, QUIET = 20_000;

/** Settings writes since the last upload (set by a cheap localStorage hook,
 *  so nothing is re-read or hashed on a timer). */
let uiDirty = false;
function watchStorage() {
  try {
    const proto = Storage.prototype as Storage & { __aquaHooked?: boolean };
    if (proto.__aquaHooked) return;
    proto.__aquaHooked = true;
    const set = proto.setItem, del = proto.removeItem;
    const touch = (k: string) => { if (k.startsWith("aqua") && !NOISY.some((re) => re.test(k)) && !SKIP.some((re) => re.test(k))) uiDirty = true; };
    proto.setItem = function (k: string, v: string) {
      if (this === localStorage && !uiDirty) { try { if (localStorage.getItem(k) !== v) touch(k); } catch { /* ignore */ } }
      return set.call(this, k, v);
    };
    proto.removeItem = function (k: string) { if (this === localStorage) touch(k); return del.call(this, k); };
  } catch { /* ignore */ }
}
/** Small fingerprints of the stores that matter (a few hundred bytes). */
function importantPrint(): string {
  const g = get(groups).map((x) => `${x.id}@${x.version}:${x.members.length}`).join(",");
  const c = get(chats).map((x) => `${x.id}:${x.name}:${x.has_lock ? 1 : 0}`).join(",");
  const p = get(profile) as unknown as Record<string, unknown>;
  const pv = Object.keys(p).sort().map((k) => typeof p[k] === "string" ? `${k}=${(p[k] as string).length}:${(p[k] as string).slice(0, 64)}` : `${k}=${JSON.stringify(p[k])?.length}`).join(";");
  return `${g}|${c}|${pv}`;
}
function messagePrint(): string {
  return get(chats).map((x) => `${x.id}:${x.time}:${x.preview.length}`).join(",");
}

let timer: ReturnType<typeof setInterval> | null = null;
let lastImportant = "", lastMessages = "", lastUpload = 0, lastFloor = 0, changedAt = 0, msgChangedAt = 0, running = false;

async function upload(): Promise<void> {
  if (running) return;
  running = true;
  try {
    const s = await refreshBackup();
    const imp = importantPrint(), msg = messagePrint();
    if (!s?.enabled) { uiDirty = false; changedAt = 0; msgChangedAt = 0; lastImportant = imp; lastMessages = msg; return; }
    uiDirty = false;
    await backupNow(true);
    lastUpload = Date.now();
    lastImportant = imp; lastMessages = msg; changedAt = 0; msgChangedAt = 0;
  } catch { uiDirty = true; /* offline — the next tick retries */ } finally {
    running = false;
  }
}

/** Ask for an upload soon (after the burst of changes settles). */
export function backupSoon(): void { changedAt = changedAt || Date.now(); }

export function startAutoBackup(): void {
  if (timer) return;
  watchStorage();
  const tick = async () => {
    const now = Date.now();
    const imp = importantPrint(), msg = messagePrint();
    if (!lastImportant) { lastImportant = imp; lastMessages = msg; }
    if ((imp !== lastImportant || uiDirty) && !changedAt) changedAt = now;
    if (msg !== lastMessages && !msgChangedAt) msgChangedAt = now;
    const importantDue = changedAt && now - changedAt >= QUIET && now - lastUpload >= MIN_GAP;
    const messagesDue = msgChangedAt && now - lastUpload >= MSG_GAP;
    if (importantDue || messagesDue) { await upload(); return; }
    // The 6-hour floor (backup_now without force checks the age itself).
    if (now - lastFloor > 30 * 60_000) {
      lastFloor = now;
      const s = await refreshBackup();
      if (s?.enabled) { try { await backupNow(false); } catch { /* offline */ } }
    }
  };
  setTimeout(() => void tick(), 60_000);
  timer = setInterval(() => void tick(), 10_000);
  // Closing the app: upload what changed since the last backup.
  window.addEventListener("beforeunload", () => { if (changedAt || msgChangedAt) void upload(); });
}
