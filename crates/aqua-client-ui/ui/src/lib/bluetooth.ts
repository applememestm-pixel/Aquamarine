/**
 * Bluetooth file transfer (OBEX Object Push through BlueZ / obexd on
 * Linux): pick a nearby device, send files, accept files sent to you.
 */
import { writable, get } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export interface BtStatus { supported: boolean; adapter: boolean; powered: boolean; obex: boolean; receiving: boolean; error: string | null }
export interface BtDevice { address: string; name: string; icon: string; paired: boolean; connected: boolean; push: boolean; rssi: number | null }
export interface BtTransfer { id: string; name: string; sent: number; size: number; status: string; path?: string | null; incoming: boolean; error?: string }
export interface BtOffer { id: string; name: string; size: number; from: string }

export const btStatus = writable<BtStatus | null>(null);
/** Files to send when the sheet opens (null = closed; [] = pick in the sheet). */
export const btSheet = writable<string[] | null>(null);
export const btTransfers = writable<Record<string, BtTransfer>>({});
export const btOffers = writable<BtOffer[]>([]);

export const btGetStatus = async () => { const s = await invoke<BtStatus>("bt_status"); btStatus.set(s); return s; };
export const btDevices = (scan = 0) => invoke<BtDevice[]>("bt_devices", { scan });
export const btPower = (on: boolean) => invoke<void>("bt_power", { on });
export const btSend = (address: string, files: string[]) => invoke<string>("bt_send", { address, files });
export function btAnswer(id: string, accept: boolean) {
  btOffers.update((l) => l.filter((o) => o.id !== id));
  void invoke("bt_answer", { id, accept });
}

let started = false;
/** Listen for transfers and, if Bluetooth is there, start receiving. */
export function startBluetooth() {
  if (started) return;
  started = true;
  void listen<BtTransfer>("aqua://bt-progress", (e) => {
    const t = e.payload;
    btTransfers.update((m) => {
      // A failed send ("out-1") marks its files ("out-1-0"…) that aren't done.
      if (t.status === "failed") {
        const next = { ...m };
        for (const [k, v] of Object.entries(next)) if (k.startsWith(t.id + "-") && v.status !== "complete") next[k] = { ...v, status: "error", error: t.error };
        next[t.id] = { ...t, name: "", sent: 0, size: 0, incoming: false };
        return next;
      }
      return { ...m, [t.id]: { ...m[t.id], ...t } };
    });
  }).catch(() => {});
  void listen<BtOffer>("aqua://bt-incoming", (e) => btOffers.update((l) => [...l, e.payload])).catch(() => {});
  // Receiving needs obexd; quietly skip when it isn't there.
  setTimeout(() => {
    void btGetStatus().then((s) => { if (s.supported && s.obex) void invoke("bt_receive").then(() => btGetStatus()).catch(() => {}); }).catch(() => {});
  }, 4000);
}

export function clearDone() {
  btTransfers.update((m) => Object.fromEntries(Object.entries(m).filter(([, t]) => !["complete", "error", "failed"].includes(t.status))));
}
export const anyActive = () => Object.values(get(btTransfers)).some((t) => !["complete", "error", "failed"].includes(t.status));
