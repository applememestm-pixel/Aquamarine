/**
 * A look for one chat, like Telegram's chat themes: a preset (emoji-named)
 * and/or own colours — accent, background, both bubbles. Only on this
 * device. ChatWindow puts `chatThemeStyle(id)` on its root, so the CSS
 * variables below override the app theme inside that chat only.
 */
import { writable, get } from "svelte/store";

export type ChatTheme = {
  preset?: string;
  accent?: string;
  /** CSS background of the chat area. */
  wall?: string;
  /** Own bubbles / the other side's bubbles. */
  out?: string;
  in?: string;
};

export type Preset = { id: string; emoji: string; name: string; accent: string; accent2: string; wall: string; out: string; in: string };

export const PRESETS: Preset[] = [
  { id: "sea",    emoji: "🌊", name: "Sea",    accent: "#2EA6FF", accent2: "#00E5C7", wall: "linear-gradient(160deg,#0B2A3F 0%,#08303A 50%,#0A1E2E 100%)", out: "#1E5A84", in: "#14283A" },
  { id: "forest", emoji: "🌿", name: "Forest", accent: "#3DDC84", accent2: "#A8E063", wall: "linear-gradient(160deg,#0E2418 0%,#13301C 55%,#0B1A12 100%)", out: "#23603C", in: "#16271D" },
  { id: "sunset", emoji: "🌅", name: "Sunset", accent: "#FF8A5B", accent2: "#FF5CA8", wall: "linear-gradient(165deg,#3A1A2A 0%,#4A2420 50%,#1E1220 100%)", out: "#8A3B3B", in: "#2C1B24" },
  { id: "night",  emoji: "🌙", name: "Night",  accent: "#9B8CFF", accent2: "#5865F2", wall: "radial-gradient(900px 500px at 80% 10%,#2A2457 0%,transparent 60%),#0D0C1D", out: "#3B347A", in: "#1B1A33" },
  { id: "sakura", emoji: "🌸", name: "Sakura", accent: "#FF7CAC", accent2: "#FFB3C7", wall: "linear-gradient(160deg,#3A1C2C 0%,#2B1830 60%,#1C1220 100%)", out: "#7A2F55", in: "#2A1A26" },
  { id: "desert", emoji: "🏜", name: "Desert", accent: "#F0B232", accent2: "#E07A2F", wall: "linear-gradient(160deg,#33280F 0%,#2B2210 55%,#1A150C 100%)", out: "#6E5418", in: "#2A2214" },
  { id: "ice",    emoji: "❄️", name: "Ice",    accent: "#7FD4FF", accent2: "#B9F0FF", wall: "linear-gradient(160deg,#E8F4FB 0%,#D6ECF7 100%)", out: "#BFE3F7", in: "#FFFFFF" },
  { id: "paper",  emoji: "📜", name: "Paper",  accent: "#C27C2C", accent2: "#8A5A2B", wall: "linear-gradient(160deg,#F6EFE2 0%,#EFE4CF 100%)", out: "#EAD9B8", in: "#FFFBF2" },
];

const KEY = "aqua.chatThemes";
function load(): Record<string, ChatTheme> {
  try { return JSON.parse(localStorage.getItem(KEY) ?? "{}") ?? {}; } catch { return {}; }
}
export const chatThemes = writable<Record<string, ChatTheme>>(load());
chatThemes.subscribe((v) => { try { localStorage.setItem(KEY, JSON.stringify(v)); } catch { /* ignore */ } });

export function setChatTheme(id: string, t: ChatTheme | null) {
  chatThemes.update((all) => {
    const next = { ...all };
    if (!t || (!t.preset && !t.accent && !t.wall && !t.out && !t.in)) delete next[id];
    else next[id] = t;
    return next;
  });
}

/** Relative luminance of #rrggbb (0..1). */
function lum(hex: string): number {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim());
  if (!m) return 0;
  const n = parseInt(m[1], 16);
  const ch = (v: number) => { const c = v / 255; return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4; };
  return 0.2126 * ch((n >> 16) & 255) + 0.7152 * ch((n >> 8) & 255) + 0.0722 * ch(n & 255);
}

/** The resolved colours of a chat's theme (preset first, own choices on top). */
export function resolve(t: ChatTheme | undefined): Partial<Preset> | null {
  if (!t) return null;
  const p = PRESETS.find((x) => x.id === t.preset);
  const r: Partial<Preset> = { ...(p ?? {}) };
  if (t.accent) { r.accent = t.accent; r.accent2 = t.accent; }
  if (t.wall) r.wall = t.wall;
  if (t.out) r.out = t.out;
  if (t.in) r.in = t.in;
  return Object.keys(r).length ? r : null;
}

/** Inline CSS variables for the chat's root element ("" = the app theme). */
export function themeStyle(t: ChatTheme | undefined): string {
  const r = resolve(t);
  if (!r) return "";
  const v: string[] = [];
  if (r.accent) {
    v.push(`--accent:${r.accent}`, `--accent-2:${r.accent2 ?? r.accent}`,
      `--accent-soft:color-mix(in srgb, ${r.accent} 15%, transparent)`, `--accent-glow:color-mix(in srgb, ${r.accent} 45%, transparent)`);
  }
  if (r.wall) v.push(`--chat-wallpaper:${r.wall}`);
  // Bubble text stays readable on any colour.
  if (r.out) v.push(`--bubble-out:${r.out}`, `--bubble-out-fg:${lum(r.out) > 0.4 ? "#15181F" : "#F2F5FA"}`);
  if (r.in) v.push(`--bubble-in:${r.in}`, `--bubble-in-fg:${lum(r.in) > 0.4 ? "#15181F" : "#F2F5FA"}`);
  return v.join(";");
}

export const chatThemeStyle = (id: string) => themeStyle(get(chatThemes)[id]);

// ---- "For both of us": the theme goes to the other person too ----------
// Travels as `[ctl:v1]{"t":"theme","th":…}` (like noShare.ts). What comes in
// is taken only as plain colours and gradients — never url(...) or anything
// else that could load a page or a picture from elsewhere.

const SHARED_KEY = "aqua.chatThemeShared";
function loadShared(): Set<string> {
  try { return new Set(JSON.parse(localStorage.getItem(SHARED_KEY) ?? "[]")); } catch { return new Set(); }
}
/** 1:1 chats whose theme is kept the same on both sides. */
export const sharedThemes = writable<Set<string>>(loadShared());
sharedThemes.subscribe((v) => { try { localStorage.setItem(SHARED_KEY, JSON.stringify([...v])); } catch { /* ignore */ } });

const HEX = /^#[0-9a-f]{6}$/i;
/** A background made only of colours: #hex, gradients of them, angles, %. */
const SAFE_WALL = /^(?:#[0-9a-f]{3,8}|(?:linear|radial)-gradient\((?:[\s,.%0-9a-z#-]|\((?:[\s,.%0-9a-z#-])*\))*\)(?:\s*,\s*(?:#[0-9a-f]{3,8}|(?:linear|radial)-gradient\((?:[\s,.%0-9a-z#-]|\((?:[\s,.%0-9a-z#-])*\))*\)))*)$/i;

function clean(t: unknown): ChatTheme | null {
  if (!t || typeof t !== "object") return null;
  const x = t as Record<string, unknown>;
  const out: ChatTheme = {};
  if (typeof x.preset === "string" && PRESETS.some((p) => p.id === x.preset)) out.preset = x.preset;
  for (const k of ["accent", "out", "in"] as const) if (typeof x[k] === "string" && HEX.test(x[k] as string)) out[k] = x[k] as string;
  if (typeof x.wall === "string" && x.wall.length < 600 && !/url\s*\(|expression|image-set|var\(/i.test(x.wall) && SAFE_WALL.test(x.wall.trim())) out.wall = x.wall.trim();
  return out.preset || out.accent || out.wall || out.out || out.in ? out : null;
}

let sendTimer: ReturnType<typeof setTimeout> | null = null;
async function sendTheme(chat: string) {
  const { sendChatControl } = await import("./tauri");
  await sendChatControl(chat, "[ctl:v1]" + JSON.stringify({ t: "theme", th: get(chatThemes)[chat] ?? null, at: Date.now() })).catch(() => {});
}
/** Turn "for both of us" on / off for a 1:1 chat (on sends the theme now). */
export function setThemeShared(chat: string, on: boolean) {
  sharedThemes.update((s) => { const n = new Set(s); if (on) n.add(chat); else n.delete(chat); return n; });
  if (on) void sendTheme(chat);
}
/** The theme of a shared chat changed here: tell the other side (a moment later — the colour pickers fire a lot). */
export function themeChanged(chat: string) {
  if (!get(sharedThemes).has(chat)) return;
  if (sendTimer) clearTimeout(sendTimer);
  sendTimer = setTimeout(() => void (chat.startsWith("grp_") ? sendGroupTheme(chat) : sendTheme(chat)), 700);
}

// ---- Groups and channels: the theme is part of the group ---------------
// An admin's "For all members" writes it into the group's data
// (`theme`, `theme_at`), so every member — and whoever joins later — gets it
// with the group itself. Taken through `clean` like the 1:1 one.

const APPLIED_KEY = "aqua.groupThemeAt";
function appliedAt(): Record<string, number> {
  try { return JSON.parse(localStorage.getItem(APPLIED_KEY) ?? "{}"); } catch { return {}; }
}
async function sendGroupTheme(gid: string) {
  const [{ api }, { groupById, refreshGroups }] = await Promise.all([import("./tauri"), import("./groups")]);
  const g = groupById(gid);
  if (!g) return;
  const next = JSON.parse(JSON.stringify(g)) as typeof g;
  next.theme = get(chatThemes)[gid] ?? null;
  next.theme_at = Date.now();
  const a = appliedAt(); a[gid] = next.theme_at;
  try { localStorage.setItem(APPLIED_KEY, JSON.stringify(a)); } catch { /* ignore */ }
  try { await api.updateGroup(next); await refreshGroups(); } catch (e) { console.warn("group theme", e); }
}
/** Admin: the group's theme for everyone on / off (on sends it now). */
export function setGroupThemeShared(gid: string, on: boolean) {
  sharedThemes.update((s) => { const n = new Set(s); if (on) n.add(gid); else n.delete(gid); return n; });
  if (on) void sendGroupTheme(gid);
}
/** A group came with a newer theme than this device last applied. */
function applyGroupThemes(list: { id: string; theme?: unknown; theme_at?: number }[]) {
  const a = appliedAt();
  let changed = false;
  for (const g of list) {
    if (!g.theme_at || (a[g.id] ?? 0) >= g.theme_at) continue;
    a[g.id] = g.theme_at;
    changed = true;
    const th = clean(g.theme);
    chatThemes.update((all) => {
      const next = { ...all };
      if (th) next[g.id] = th; else delete next[g.id];
      return next;
    });
  }
  if (changed) try { localStorage.setItem(APPLIED_KEY, JSON.stringify(a)); } catch { /* ignore */ }
}

let started = false;
export function startChatThemes() {
  if (started) return;
  started = true;
  void import("./groups").then(({ groups }) => groups.subscribe((list) => applyGroupThemes(list))).catch(() => {});
  void import("@tauri-apps/api/event").then(({ listen }) => listen<{ chat: string; body: Record<string, unknown>; own?: boolean }>("aqua://ctl", (e) => {
    const b = e.payload.body;
    const chat = e.payload.chat;
    if (b?.t !== "theme" || !chat || chat.startsWith("grp_")) return;
    chatThemes.update((all) => {
      const next = { ...all };
      const th = clean(b.th);
      if (th) next[chat] = th; else delete next[chat];
      return next;
    });
    sharedThemes.update((s) => new Set(s).add(chat));
  })).catch(() => {});
}
