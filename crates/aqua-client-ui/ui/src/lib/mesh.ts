/**
 * Messages without the internet (mesh.rs): when the relay is away,
 * encrypted messages wait here and are swapped over Bluetooth with
 * contacts nearby — the way Briar works — or sent once the relay is back.
 */
import { writable } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export interface MeshPeer { addr: string; name: string; last: number }
export interface MeshReport { at: number; device: string; peer: string; sent: number; received: number; error: string | null }
export interface MeshStatus {
  supported: boolean; enabled: boolean; ready: boolean;
  queued: number; per_chat: Record<string, number>;
  peers: MeshPeer[]; last: MeshReport[]; syncing: boolean; note: string | null;
}

export const meshStatus = writable<MeshStatus | null>(null);

let started = false;
export function startMesh() {
  if (started) return;
  started = true;
  void refreshMesh();
  void listen<MeshStatus>("aqua://mesh", (e) => meshStatus.set(e.payload)).catch(() => {});
}
export async function refreshMesh() {
  try { meshStatus.set(await invoke<MeshStatus>("mesh_status")); } catch { /* browser preview */ }
}
export async function meshSyncNow(): Promise<string | null> {
  try { meshStatus.set(await invoke<MeshStatus>("mesh_sync_now")); return null; }
  catch (e) { await refreshMesh(); return String(e); }
}
export async function meshSetEnabled(on: boolean) {
  try { meshStatus.set(await invoke<MeshStatus>("mesh_set_enabled", { on })); } catch { /* ignore */ }
}
