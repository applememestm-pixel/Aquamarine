/**
 * Failover between federated relays. After a successful connect we ask the
 * relay for its federation peers and remember them — per relay: only the
 * relays that relay itself names are "the same account elsewhere". A relay
 * we merely visited once is not one of them: moving there would leave the
 * chats on the old one (messages "half arriving").
 */
import { api } from "./tauri";

const KEY = "aqua.federations.v1";
type Feds = Record<string, string[]>;

function feds(): Feds {
  try { return JSON.parse(localStorage.getItem(KEY) ?? "{}") as Feds; } catch { return {}; }
}
function saveFeds(f: Feds) {
  try { localStorage.setItem(KEY, JSON.stringify(f)); } catch { /* ignore */ }
}
// The old list mixed every relay ever used; it can't tell them apart.
try { localStorage.removeItem("aqua.servers.v1"); } catch { /* ignore */ }

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
function forgetAliasesOf(url: string) {
  const u = norm(url);
  try { localStorage.setItem(ALIAS_KEY, JSON.stringify(aliasPairs().filter(([p, q]) => p !== u && q !== u))); } catch { /* ignore */ }
}

// ---- Servers added by hand (Settings → Network & proxy → Add server) ----
const MANUAL_KEY = "aqua.manualServers.v1";
export function manualServers(): string[] {
  try { return (JSON.parse(localStorage.getItem(MANUAL_KEY) ?? "[]") as string[]).map(norm).filter(Boolean); } catch { return []; }
}
export function addManualServer(url: string) {
  const u = norm(url);
  if (!u) return;
  try { localStorage.setItem(MANUAL_KEY, JSON.stringify([u, ...manualServers().filter((x) => x !== u)].slice(0, 12))); } catch { /* ignore */ }
}

/** Forget a relay everywhere this device remembered it (it stays usable by hand). */
export function forgetServer(url: string) {
  const u = norm(url);
  forgetAliasesOf(u);
  try { localStorage.setItem(MANUAL_KEY, JSON.stringify(manualServers().filter((x) => x !== u))); } catch { /* ignore */ }
  const f = feds();
  delete f[u];
  for (const k of Object.keys(f)) f[k] = f[k].filter((x) => x !== u);
  saveFeds(f);
}

/** The relay's other addresses. */
export function aliasesOf(url: string): string[] {
  const u = norm(url);
  return aliasPairs().flatMap(([p, q]) => (p === u ? [q] : q === u ? [p] : []));
}

/** The relays of `url`'s federation (as it reported them), itself excluded. */
export function federationOf(url: string): string[] {
  const f = feds();
  const u = norm(url);
  const own = [u, ...aliasesOf(u)];
  return [...new Set(own.flatMap((x) => f[x] ?? []))].filter((x) => !own.includes(x));
}

/** Every relay we know a federation for — the sign-in screen's suggestions. */
export function knownServers(): string[] {
  const f = feds();
  return [...new Set([...manualServers(), ...Object.entries(f).flatMap(([k, v]) => [k, ...v])])].slice(0, 12);
}

/** Ask `current` for its federation and remember exactly that. */
export async function learnPeers(current: string): Promise<void> {
  const cur = norm(current);
  try {
    const r = await api.fedPeers(cur);
    // `self`: the relay's own other address — e.g. its white IP for when this
    // device leaves the home Wi-Fi it signed in on.
    if (r.self) rememberAlias(cur, r.self);
    // The relay no longer names an internet address of its own (since 1.0.61
    // it only announces one that leads back to it): forget what an older one
    // said — VPN exits like the "Frankfurt" servers nobody could reach.
    else forgetAliasesOf(cur);
    const f = feds();
    f[cur] = r.federated ? [...new Set((r.servers ?? []).map(norm))].filter((x) => x && x !== cur).slice(0, 12) : [];
    // Keep the map small: the relays we're on now and lately.
    const keys = Object.keys(f);
    if (keys.length > 12) for (const k of keys.slice(0, keys.length - 12)) delete f[k];
    saveFeds(f);
  } catch { /* an old relay: no federation known */ }
}

/** Other relays to try when `failed` doesn't answer. */
export function alternatives(failed: string): string[] {
  const f = norm(failed);
  // The same relay's other address first, then its federation.
  return [...new Set([...aliasesOf(f), ...federationOf(f)])].filter((s) => s !== f);
}
