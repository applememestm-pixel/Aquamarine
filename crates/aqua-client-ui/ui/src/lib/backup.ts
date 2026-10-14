/**
 * Encrypted backups from the UI side: what of the UI's own state goes into
 * a backup (localStorage settings), restoring it, and the periodic auto
 * backup. The sealing itself happens in Rust (aqua-client-core::backup).
 */
import { writable } from "svelte/store";
import { api, type BackupStatus } from "./tauri";

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

let timer: ReturnType<typeof setInterval> | null = null;
/** Every 30 minutes: back up if the last one is older than 6 hours. */
export function startAutoBackup(): void {
  if (timer) return;
  const tick = async () => {
    const s = await refreshBackup();
    if (s?.enabled) { try { await backupNow(false); } catch { /* offline — next time */ } }
  };
  setTimeout(tick, 60_000);
  timer = setInterval(tick, 30 * 60_000);
}
