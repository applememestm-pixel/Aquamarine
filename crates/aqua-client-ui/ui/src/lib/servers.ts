/**
 * Failover between federated relays. After a successful connect we ask the
 * relay for its peers and remember them; when the connection drops (or the
 * relay is down at start), we try the others until one answers. The
 * account lives on all of them (federation replicates it).
 */
import { api } from "./tauri";

const KEY = "aqua.servers.v1";

export function knownServers(): string[] {
  try { return JSON.parse(localStorage.getItem(KEY) ?? "[]") as string[]; } catch { return []; }
}

function save(list: string[]) {
  try { localStorage.setItem(KEY, JSON.stringify([...new Set(list)].slice(0, 12))); } catch { /* ignore */ }
}

const norm = (u: string) => u.trim().replace(/\/+$/, "");

/** Remember `current` first, then whatever peers it reports. */
export async function learnPeers(current: string): Promise<void> {
  const cur = norm(current);
  try {
    const r = await api.fedPeers(cur);
    // `self`: the relay's own other address — e.g. its white IP for when this
    // device leaves the home Wi-Fi it signed in on.
    save([cur, ...(r.self ? [norm(r.self)] : []), ...(r.servers ?? []).map(norm), ...knownServers()]);
  } catch {
    save([cur, ...knownServers()]);
  }
}

/** Other relays to try when `failed` doesn't answer. */
export function alternatives(failed: string): string[] {
  const f = norm(failed);
  return knownServers().filter((s) => norm(s) !== f);
}
