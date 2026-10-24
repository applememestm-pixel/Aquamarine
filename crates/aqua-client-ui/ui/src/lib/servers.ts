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

/**
 * Two addresses of one relay: its address on the home network and its white
 * IP. At home the white IP usually doesn't answer (most routers don't loop a
 * forwarded port back inside — "NAT loopback"), outside the home one can't;
 * so wherever one of them is used, the other is tried right after.
 */
const ALIAS_KEY = "aqua.serverAliases.v1";
function aliasPairs(): [string, string][] {
  try { return JSON.parse(localStorage.getItem(ALIAS_KEY) ?? "[]") as [string, string][]; } catch { return []; }
}
export function rememberAlias(a: string, b: string) {
  const [x, y] = [norm(a), norm(b)];
  if (!x || !y || x === y) return;
  const rest = aliasPairs().filter(([p, q]) => !(p === x && q === y) && !(p === y && q === x));
  try { localStorage.setItem(ALIAS_KEY, JSON.stringify([[x, y], ...rest].slice(0, 12))); } catch { /* ignore */ }
}
/** The relay's other addresses. */
export function aliasesOf(url: string): string[] {
  const u = norm(url);
  return aliasPairs().flatMap(([p, q]) => (p === u ? [q] : q === u ? [p] : []));
}

/** Remember `current` first, then whatever peers it reports. */
export async function learnPeers(current: string): Promise<void> {
  const cur = norm(current);
  try {
    const r = await api.fedPeers(cur);
    if (r.self) rememberAlias(cur, r.self);
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
  // The same relay's other address first, then the federation.
  return [...new Set([...aliasesOf(f), ...knownServers().map(norm)])].filter((s) => s !== f);
}
