/**
 * Local call history — persisted to localStorage. CallModal appends
 * an entry whenever a call ends; the Calls tab reads them back.
 */
import { writable, get } from "svelte/store";

export interface CallRecord {
  id: string;
  peer_id: string;
  direction: "in" | "out";
  status: "completed" | "missed" | "rejected" | "failed";
  started_at: number;      // epoch ms
  duration_seconds: number;
}

const KEY = "aqua-call-history";

function load(): CallRecord[] {
  try {
    return JSON.parse(localStorage.getItem(KEY) ?? "[]");
  } catch {
    return [];
  }
}

export const callHistory = writable<CallRecord[]>(load());

callHistory.subscribe((v) => {
  try {
    // Trim to last 500 to avoid runaway growth.
    const trimmed = v.slice(-500);
    localStorage.setItem(KEY, JSON.stringify(trimmed));
  } catch { /* ignore */ }
});

export function logCall(rec: Omit<CallRecord, "id">) {
  const entry: CallRecord = { ...rec, id: `${rec.started_at}-${Math.random().toString(36).slice(2, 8)}` };
  callHistory.update((all) => [...all, entry]);
}

export function clearCallHistory() {
  callHistory.set([]);
}

export function recentCalls(): CallRecord[] {
  return [...get(callHistory)].sort((a, b) => b.started_at - a.started_at);
}
