/**
 * Settings → Network & proxy, Telegram's proxy screen made ours.
 *
 * Servers: the relays where this account works — the one we're on, its
 * other address (home / white IP, servers.ts aliases) and its federation
 * (servers.ts knownServers) — each with a live ping. "Pick the fastest by
 * itself" moves the connection to a clearly faster one (or off a dead one).
 * "Stay on this server" locks it: no moving at all, not even off a dead one
 * — then the app is offline until that server is back.
 *
 * Proxies: a list of SOCKS5 / HTTP proxies with a ping to our relay through
 * each. "Use proxy" routes everything through the chosen one; "Switch
 * proxies by itself" moves to a working one when it stops answering.
 */
import { writable, get } from "svelte/store";
import { api } from "./tauri";
import { info, connect } from "./stores";
import { knownServers, aliasesOf, learnPeers } from "./servers";
import { showToast } from "./components/Toast.svelte";
import { tr } from "./i18n";

export interface NetPrefs {
  autoServer: boolean;
  useProxy: boolean;
  proxies: string[];
  autoProxy: boolean;
  /** The server we never move away from ("" = not locked). */
  lockServer: string;
}
const KEY = "aqua.net.v1";
const DEF: NetPrefs = { autoServer: true, useProxy: false, proxies: [], autoProxy: true, lockServer: "" };
function load(): NetPrefs {
  try { return { ...DEF, ...JSON.parse(localStorage.getItem(KEY) || "{}") }; } catch { return { ...DEF }; }
}
export const netPrefs = writable<NetPrefs>(load());
netPrefs.subscribe((p) => { try { localStorage.setItem(KEY, JSON.stringify(p)); } catch { /* storage blocked */ } });

/** url → ms, null = no answer, undefined = not measured yet. */
export const serverPings = writable<Record<string, number | null>>({});
export const proxyPings = writable<Record<string, number | null>>({});

const norm = (u: string) => u.trim().replace(/\/+$/, "");

/** The locked server, or "" when the app may move between servers. */
export const lockedServer = () => norm(get(netPrefs).lockServer ?? "");

/** Relays where this account works, the current one first. */
export function serverCandidates(): string[] {
  const cur = norm(get(info)?.server_url ?? "");
  const list = [cur, ...aliasesOf(cur), ...knownServers().map(norm)].filter(Boolean);
  return [...new Set(list)].slice(0, 12);
}

/** A proxy address as typed: "host:port" means SOCKS5. */
export function normProxy(p: string): string {
  const s = p.trim();
  if (!s) return "";
  return /^[a-z0-9+]+:\/\//i.test(s) ? s : `socks5://${s}`;
}

async function ping(url: string, via?: string): Promise<number | null> {
  try { return await api.serverPing(url, via); } catch { return null; }
}

export async function measureServers(): Promise<Record<string, number | null>> {
  const urls = serverCandidates();
  const got = await Promise.all(urls.map(async (u) => [u, await ping(u)] as const));
  const map = Object.fromEntries(got);
  serverPings.set(map);
  return map;
}

export async function measureProxies(): Promise<Record<string, number | null>> {
  const cur = norm(get(info)?.server_url ?? "");
  const list = get(netPrefs).proxies;
  if (!cur || !list.length) { proxyPings.set({}); return {}; }
  const got = await Promise.all(list.map(async (p) => [p, await ping(cur, p)] as const));
  const map = Object.fromEntries(got);
  proxyPings.set(map);
  return map;
}

/** Move the connection to another relay of the account. */
export async function switchServer(url: string): Promise<string | null> {
  const user = get(info)?.username;
  if (!user) return tr("Not signed in");
  const err = await connect(url, user);
  if (!err) {
    void learnPeers(url);
    // Picked by hand while locked: the lock moves with the user.
    if (lockedServer()) netPrefs.update((p) => ({ ...p, lockServer: norm(url) }));
  }
  return err;
}

/** Route everything through `proxy` ("" = direct) and reconnect. */
export async function applyProxy(proxy: string): Promise<string | null> {
  try {
    info.set(await api.setNetworkProxy(proxy));
    const i = get(info);
    if (i?.server_url && i.username) return await connect(i.server_url, i.username);
    return null;
  } catch (e) { return String(e); }
}

/** The proxy in use right now ("" = direct). */
export const currentProxy = () => get(info)?.proxy ?? "";

// ---- By itself ----
let fasterRounds = 0;
let deadRounds = 0;
let proxyMisses = 0;
let busy = false;

async function round() {
  const i = get(info);
  if (busy || !i?.username || !i.server_url || document.visibilityState !== "visible") return;
  busy = true;
  try {
    const p = get(netPrefs);
    // Proxy first: if it's dead, server pings through it mean nothing.
    if (p.useProxy && p.autoProxy && p.proxies.length > 1) {
      const pp = await measureProxies();
      const cur = currentProxy();
      if (cur && pp[cur] == null) {
        proxyMisses++;
        const best = Object.entries(pp).filter(([, ms]) => ms != null).sort((a, b) => (a[1] as number) - (b[1] as number))[0];
        if (proxyMisses >= 2 && best) {
          proxyMisses = 0;
          const err = await applyProxy(best[0]);
          if (!err) showToast(tr("The proxy stopped answering — switched to {p}", { p: best[0].replace(/^\w+:\/\//, "") }));
        }
      } else proxyMisses = 0;
    }
    if (p.autoServer && !lockedServer()) {
      const sp = await measureServers();
      const cur = norm(i.server_url);
      const curMs = sp[cur];
      const others = Object.entries(sp).filter(([u, ms]) => u !== cur && ms != null).sort((a, b) => (a[1] as number) - (b[1] as number));
      const best = others[0];
      // Off a dead server at once; to a faster one after two rounds in a row
      // (a single slow ping doesn't move anyone).
      const clearlyFaster = best && curMs != null && (best[1] as number) < curMs * 0.7 && curMs - (best[1] as number) > 40;
      fasterRounds = clearlyFaster ? fasterRounds + 1 : 0;
      deadRounds = curMs == null ? deadRounds + 1 : 0;
      const dead = curMs == null && (!i.connected || deadRounds >= 2);
      if (best && (dead || fasterRounds >= 2)) {
        fasterRounds = 0;
        deadRounds = 0;
        const err = await switchServer(best[0]);
        if (!err) showToast(tr("Switched to a faster server: {u} ({ms} ms)", { u: best[0].replace(/^https?:\/\//, ""), ms: best[1] as number }));
      }
    }
  } finally { busy = false; }
}

export function startNetwork(): () => void {
  // A proxy set before this page existed joins the list.
  const unInit = info.subscribe((i) => {
    const p = i?.proxy ?? "";
    if (p && !get(netPrefs).proxies.includes(p)) netPrefs.update((x) => ({ ...x, useProxy: true, proxies: [p, ...x.proxies] }));
  });
  const first = setTimeout(round, 20_000);
  const every = setInterval(round, 60_000);
  // Lost the connection: look sooner.
  let wasUp = true;
  const un = info.subscribe((i) => {
    const up = !!i?.connected;
    if (wasUp && !up) setTimeout(round, 8_000);
    wasUp = up;
  });
  return () => { clearTimeout(first); clearInterval(every); un(); unInit(); };
}
