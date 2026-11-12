/**
 * Plugins (Settings → Plugins), like exteraGram's, but JavaScript.
 *
 * Each enabled plugin runs in its own Web Worker: no page, no access to the
 * app, its storage or the Tauri bridge. Before the plugin's code runs, the
 * worker loses every way out to the network (fetch, XHR, WebSocket,
 * importScripts, nested workers…), and the app's CSP allows only local
 * addresses anyway — a plugin sees the messages it's given but can't send
 * them anywhere. It talks to the app by messages:
 *
 *   aqua.register({ name, version, description, author })
 *   aqua.onSend((msg) => string | false | void)   // change or cancel what you send
 *   aqua.onIncoming((msg) => void)                // a message came in (read only)
 *   aqua.command("name", "description", (args, chat) => string | void)  // "/name" in the field
 *   aqua.toast(text)
 *   aqua.storage.get(key) / aqua.storage.set(key, value)   // the plugin's own, saved
 *   aqua.button({ id, label, icon }, (ctx) => …)  // its own item in the 📎 menu (1.2.11)
 *   aqua.music.open() / .search(q) / .play(song) / .send(chat, song)   // the Navidrome player
 *   aqua.fetch(url, { method, headers, body })    // only to hosts in "// @connect", if the user allowed
 *
 * Handlers may be async; each call gets at most 800 ms, then the message goes
 * on unchanged. See docs/plugins.md.
 */
import { writable, get, derived } from "svelte/store";
import { showToast } from "./components/Toast.svelte";
import { events } from "./tauri";

export interface PluginMeta { name: string; version: string; description: string; author: string; connect: string[] }
export interface Plugin extends PluginMeta {
  id: string; code: string; enabled: boolean; installed: number; data: Record<string, unknown>;
  /** Hosts from `// @connect` the user allowed at install (others: no network). */
  allowed?: string[];
}
export interface PluginCommand { plugin: string; cmd: string; desc: string }
export interface PluginButton { plugin: string; id: string; label: string; icon: string }

const KEY = "aqua.plugins.v1";
const ON_KEY = "aqua.pluginsOn.v1";
const MAX_CODE = 256 * 1024;

function load<T>(key: string, fallback: T): T {
  try { const v = localStorage.getItem(key); return v ? (JSON.parse(v) as T) : fallback; } catch { return fallback; }
}
function save(key: string, v: unknown) { try { localStorage.setItem(key, JSON.stringify(v)); } catch { /* full or blocked */ } }

/** The whole feature is off until switched on in Settings → Plugins. */
export const pluginsOn = writable<boolean>(load(ON_KEY, false));
pluginsOn.subscribe((v) => save(ON_KEY, v));
export const plugins = writable<Plugin[]>(load<Plugin[]>(KEY, []).filter((p) => p && typeof p.code === "string"));
plugins.subscribe((v) => save(KEY, v));

/** Slash commands registered by running plugins. */
const commands = writable<PluginCommand[]>([]);
export const pluginCommands = derived([commands, pluginsOn], ([c, on]) => (on ? c : []));
/** 📎 menu items registered by running plugins (`aqua.button`). */
const buttons = writable<PluginButton[]>([]);
export const pluginButtons = derived([buttons, pluginsOn], ([b, on]) => (on ? b : []));

/** `// @name Foo` style header → metadata (before the code runs). */
export function parseMeta(code: string): PluginMeta {
  const tag = (k: string) => new RegExp(`^\\s*//\\s*@${k}\\s+(.+)$`, "m").exec(code)?.[1]?.trim() ?? "";
  const connect = tag("connect").split(/[\s,]+/).map((h) => h.trim().toLowerCase().replace(/^https?:\/\//, "").replace(/\/.*$/, "")).filter(Boolean).slice(0, 16);
  return { name: tag("name") || "Untitled plugin", version: tag("version") || "1.0", description: tag("description"), author: tag("author"), connect };
}

/** `allow` decides about the `// @connect` hosts (the settings page asks the user). */
export function installPlugin(code: string, allow: (hosts: string[], name: string) => boolean = () => false): Plugin {
  if (code.length > MAX_CODE) throw new Error("plugin is too large");
  const meta = parseMeta(code);
  const id = meta.name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "plugin";
  const prev = get(plugins).find((p) => p.id === id);
  const same = !!prev?.allowed && meta.connect.every((h) => prev.allowed!.includes(h));
  const allowed = meta.connect.length ? (same || allow(meta.connect, meta.name) ? meta.connect : []) : [];
  const p: Plugin = { ...meta, id, code, enabled: true, installed: Date.now(), data: prev?.data ?? {}, allowed };
  plugins.update((l) => [...l.filter((x) => x.id !== id), p]);
  return p;
}
/** What a plugin does, for grouping it in Settings → Plugins: its own
 *  `// @category` if it names one of these, else read off the calls it makes. */
export const PLUGIN_CATS = [
  { id: "commands", label: "Commands", icon: "⌨️" },
  { id: "sending", label: "Sending", icon: "✏️" },
  { id: "incoming", label: "Incoming messages", icon: "📥" },
  { id: "menu", label: "Attach menu", icon: "📎" },
  { id: "music", label: "Music", icon: "🎵" },
  { id: "other", label: "Other", icon: "🧩" },
] as const;
export type PluginCat = (typeof PLUGIN_CATS)[number]["id"];
export function pluginCategory(code: string): PluginCat {
  const tag = /^\s*\/\/\s*@category\s+(\w+)/m.exec(code)?.[1]?.toLowerCase();
  if (tag && PLUGIN_CATS.some((c) => c.id === tag)) return tag as PluginCat;
  if (/aqua\.music\./.test(code)) return "music";
  if (/aqua\.button\s*\(/.test(code)) return "menu";
  if (/aqua\.command\s*\(/.test(code)) return "commands";
  if (/aqua\.onSend\s*\(/.test(code)) return "sending";
  if (/aqua\.onIncoming\s*\(/.test(code)) return "incoming";
  return "other";
}

export function removePlugin(id: string) { plugins.update((l) => l.filter((p) => p.id !== id)); }
export function togglePlugin(id: string) { plugins.update((l) => l.map((p) => (p.id === id ? { ...p, enabled: !p.enabled } : p))); }

// ---------------- the sandbox ----------------

interface Host { id: string; worker: Worker; url: string; code: string; pending: Map<number, (v: unknown) => void> }
const hosts = new Map<string, Host>();
let seq = 1;

/** What runs in the worker before the plugin: no way out, and the `aqua` object. */
const BOOT = `
(() => {
  const post = self.postMessage.bind(self);
  const G = self, P = Object.getPrototypeOf(self);
  for (const k of ["fetch", "XMLHttpRequest", "WebSocket", "EventSource", "importScripts", "Worker", "SharedWorker",
                   "caches", "WebTransport", "RTCPeerConnection", "BroadcastChannel", "indexedDB"]) {
    for (let o = G; o; o = Object.getPrototypeOf(o)) { try { delete o[k]; } catch (e) {} }
    try { Object.defineProperty(G, k, { value: undefined, writable: false, configurable: false }); } catch (e) {}
  }
  void P;
  let ready = false; const queue = [];
  const send = (m) => (ready ? post(m) : queue.push(m));
  const h = { send: [], incoming: [], cmd: {}, btn: {} };
  const store = {};
  const calls = new Map(); let cseq = 1;
  const call = (fn, args) => new Promise((res, rej) => { const cid = cseq++; calls.set(cid, [res, rej]); send({ t: "call", cid, fn, args }); });
  self.aqua = {
    register: (meta) => send({ t: "meta", meta }),
    onSend: (f) => { h.send.push(f); send({ t: "hook", hook: "send" }); },
    onIncoming: (f) => { h.incoming.push(f); send({ t: "hook", hook: "incoming" }); },
    command: (cmd, desc, f) => { cmd = String(cmd).toLowerCase().replace(/[^a-z0-9_]/g, ""); if (!cmd) return; h.cmd[cmd] = f; send({ t: "cmd", cmd, desc: String(desc || "") }); },
    toast: (text) => send({ t: "toast", text: String(text).slice(0, 300) }),
    storage: { get: (k) => store[k], set: (k, v) => { store[k] = v; send({ t: "store", k: String(k), v }); } },
    button: (o, f) => { o = o || {}; const id = String(o.id || o.label || "").slice(0, 40); if (!id || typeof f !== "function") return; h.btn[id] = f;
      send({ t: "button", id, label: String(o.label || id).slice(0, 40), icon: String(o.icon || "🧩").slice(0, 8) }); },
    fetch: (url, o) => { o = o || {}; return call("fetch", { url: String(url), method: String(o.method || "GET"), headers: o.headers || {}, body: o.body == null ? null : String(o.body) }); },
    music: {
      open: () => call("music.open", {}),
      search: (q) => call("music.search", { q: String(q || "") }),
      play: (song) => call("music.play", { song }),
      send: (chat, song) => call("music.send", { chat: String(chat || ""), song }),
    },
  };
  const run = async (fs, arg) => { let cur = arg.text; for (const f of fs) { const r = await f({ ...arg, text: cur }); if (r === false) return false; if (typeof r === "string") cur = r; } return cur; };
  self.onmessage = async (ev) => {
    const m = ev.data || {};
    if (m.t === "init") { Object.assign(store, m.data || {}); ready = true; for (const q of queue.splice(0)) post(q); return; }
    if (m.t === "result") { const c = calls.get(m.cid); if (c) { calls.delete(m.cid); m.ok ? c[0](m.value) : c[1](new Error(m.error || "failed")); } return; }
    let out = null;
    try {
      if (m.t === "send") out = await run(h.send, m.arg);
      else if (m.t === "incoming") { for (const f of h.incoming) await f(m.arg); }
      else if (m.t === "cmd" && h.cmd[m.cmd]) { const r = await h.cmd[m.cmd](m.args, m.chat); out = typeof r === "string" ? r : null; }
      else if (m.t === "button" && h.btn[m.bid]) await h.btn[m.bid]({ chat: m.chat });
    } catch (err) { send({ t: "error", text: String(err && err.message || err) }); }
    if (m.id) send({ t: "reply", id: m.id, out });
  };
  self.addEventListener("error", (e) => send({ t: "error", text: String(e.message) }));
})();
`;

function start(p: Plugin) {
  if (hosts.has(p.id) || typeof Worker === "undefined") return;
  // The plugin's code goes after BOOT in one script: "use strict" can't
  // be used to undo what BOOT took away.
  const url = URL.createObjectURL(new Blob([BOOT, "\n;\n", p.code], { type: "text/javascript" }));
  let worker: Worker;
  try { worker = new Worker(url, { name: `plugin:${p.id}` }); } catch (e) { URL.revokeObjectURL(url); console.warn("plugin", p.id, e); return; }
  const host: Host = { id: p.id, worker, url, code: p.code, pending: new Map() };
  worker.onmessage = (ev) => onPluginMessage(host, ev.data);
  worker.onerror = (e) => console.warn(`[plugin ${p.id}]`, e.message);
  worker.postMessage({ t: "init", data: p.data ?? {} });
  hosts.set(p.id, host);
}

function stop(id: string) {
  const h = hosts.get(id);
  if (!h) return;
  h.worker.terminate();
  URL.revokeObjectURL(h.url);
  hosts.delete(id);
  commands.update((l) => l.filter((c) => c.plugin !== id));
  buttons.update((l) => l.filter((b) => b.plugin !== id));
}

function onPluginMessage(h: Host, m: { t?: string; [k: string]: unknown }) {
  switch (m?.t) {
    case "cmd": {
      const cmd = String(m.cmd ?? ""), desc = String(m.desc ?? "").slice(0, 120);
      commands.update((l) => [...l.filter((c) => !(c.plugin === h.id && c.cmd === cmd)), { plugin: h.id, cmd, desc }]);
      break;
    }
    case "toast": showToast(`🧩 ${String(m.text ?? "")}`); break;
    case "button": {
      const id = String(m.id ?? ""), label = String(m.label ?? id).slice(0, 40), icon = String(m.icon ?? "🧩").slice(0, 8);
      buttons.update((l) => [...l.filter((b) => !(b.plugin === h.id && b.id === id)), { plugin: h.id, id, label, icon }]);
      break;
    }
    case "call": void hostCall(h, Number(m.cid), String(m.fn ?? ""), (m.args ?? {}) as Record<string, any>); break;
    case "store":
      plugins.update((l) => l.map((p) => (p.id === h.id ? { ...p, data: { ...p.data, [String(m.k)]: m.v } } : p)));
      break;
    case "error": console.warn(`[plugin ${h.id}]`, m.text); break;
    case "reply": { const f = h.pending.get(Number(m.id)); if (f) { h.pending.delete(Number(m.id)); f(m.out); } break; }
  }
}

function ask(h: Host, msg: Record<string, unknown>, ms = 800): Promise<unknown> {
  return new Promise((resolve) => {
    const id = seq++;
    const timer = setTimeout(() => { h.pending.delete(id); resolve(undefined); }, ms);
    h.pending.set(id, (v) => { clearTimeout(timer); resolve(v); });
    h.worker.postMessage({ ...msg, id });
  });
}

/** What a plugin may ask of the app (`aqua.fetch`, `aqua.music.*`). */
async function hostCall(h: Host, cid: number, fn: string, a: Record<string, any>) {
  const reply = (ok: boolean, value?: unknown, error?: string) => h.worker.postMessage({ t: "result", cid, ok, value, error });
  try {
    if (fn === "fetch") {
      const p = get(plugins).find((x) => x.id === h.id);
      let host = "";
      try { const u = new URL(String(a.url)); if (!/^https?:$/.test(u.protocol)) throw 0; host = u.hostname.toLowerCase(); } catch { return reply(false, null, "bad URL"); }
      const ok = (p?.allowed ?? []).some((x) => x === host || (x.startsWith("*.") && (host === x.slice(2) || host.endsWith(x.slice(1)))));
      if (!ok) return reply(false, null, `not allowed: ${host} (add it to // @connect and reinstall)`);
      const { invoke } = await import("@tauri-apps/api/core");
      const headers: Record<string, string> = {};
      for (const [k, v] of Object.entries(a.headers ?? {})) headers[String(k)] = String(v);
      const r = await invoke("plugin_fetch", { url: String(a.url), method: String(a.method || "GET"), headers, body: a.body ?? null });
      return reply(true, r);
    }
    const music = await import("./music");
    const { get: g } = await import("svelte/store");
    const src = (g(music.musicSources) as string[]).includes(g(music.musicSource)) ? g(music.musicSource) : (g(music.musicSources)[0] ?? null);
    if (fn === "music.open") { music.openMusic(); return reply(true, true); }
    if (!src) return reply(false, null, "no Navidrome is connected");
    if (fn === "music.search") {
      const r = await music.search(src, String(a.q ?? ""));
      return reply(true, r.songs.slice(0, 50));
    }
    const song = a.song && typeof a.song === "object" && typeof a.song.id === "string"
      ? { ...music.cardSong({ id: a.song.id, title: String(a.song.title ?? ""), artist: a.song.artist, album: a.song.album, dur: Number(a.song.duration ?? a.song.dur ?? 0), cover: a.song.coverArt ?? a.song.cover, src, nd: "" }, (a.song.src === "own" || a.song.src === "server") ? a.song.src : src) }
      : null;
    if (!song) return reply(false, null, "song needed (one from aqua.music.search)");
    if (fn === "music.play") { await music.playSongs([song], 0); return reply(true, true); }
    if (fn === "music.send") {
      const chat = String(a.chat ?? "");
      if (!chat) return reply(false, null, "chat needed");
      await music.sendCard(chat, song);
      return reply(true, true);
    }
    reply(false, null, `unknown call ${fn}`);
  } catch (e) {
    reply(false, null, String(e instanceof Error ? e.message : e));
  }
}

/** A plugin's 📎 button was pressed in `chat`. */
export function pressPluginButton(b: PluginButton, chat: string) {
  const h = hosts.get(b.plugin);
  if (h) h.worker.postMessage({ t: "button", bid: b.id, chat });
}

/** Keep the running sandboxes in step with the list and the switch. */
export function startPlugins(): () => void {
  const sync = () => {
    const on = get(pluginsOn);
    const want = on ? get(plugins).filter((p) => p.enabled) : [];
    for (const id of [...hosts.keys()]) {
      const p = want.find((x) => x.id === id);
      if (!p || p.code !== hosts.get(id)!.code) stop(id);
    }
    for (const p of want) start(p);
  };
  const u1 = pluginsOn.subscribe(sync), u2 = plugins.subscribe(sync);
  let u3: (() => void) | null = null;
  void events.onIncoming((p) => pluginsOnIncoming(p.peer_id, p.text)).then((u) => (u3 = u)).catch(() => {});
  return () => { u1(); u2(); u3?.(); for (const id of [...hosts.keys()]) stop(id); };
}

/** Outgoing text through every plugin's onSend: the new text, or null to cancel. */
export async function pluginsOnSend(chat: string, text: string): Promise<string | null> {
  if (!get(pluginsOn) || !hosts.size) return text;
  let cur = text;
  for (const h of hosts.values()) {
    const out = await ask(h, { t: "send", arg: { chat, text: cur } });
    if (out === false) return null;
    if (typeof out === "string" && out.trim()) cur = out.slice(0, 16000);
  }
  return cur;
}

export function pluginsOnIncoming(chat: string, text: string) {
  if (!get(pluginsOn)) return;
  for (const h of hosts.values()) h.worker.postMessage({ t: "incoming", arg: { chat, text } });
}

/** Run a plugin's slash command; its answer (if any) is the text to send. */
export async function runPluginCommand(c: PluginCommand, args: string, chat: string): Promise<string | null> {
  const h = hosts.get(c.plugin);
  if (!h) return null;
  const out = await ask(h, { t: "cmd", cmd: c.cmd, args, chat }, 3000);
  return typeof out === "string" && out.trim() ? out.slice(0, 16000) : null;
}

/** A small plugin to start from (Settings → Plugins → Example). */
export const EXAMPLE_PLUGIN = `// @name Sparkle & Mock
// @version 1.0
// @author Aquamarine
// @description /sparkle wraps text in ✨, /mock makes tExT lIkE tHiS, and "->" becomes an arrow.

aqua.register({ name: "Sparkle & Mock" });

aqua.command("sparkle", "✨ text ✨", (args) => "✨ " + (args || "✨") + " ✨");

aqua.command("mock", "tExT lIkE tHiS", (args) =>
  [...(args || "")].map((c, i) => (i % 2 ? c.toUpperCase() : c.toLowerCase())).join(""));

aqua.onSend((msg) => msg.text.replace(/->/g, "→"));

let n = aqua.storage.get("sent") || 0;
aqua.onSend(() => { aqua.storage.set("sent", ++n); });
`;
