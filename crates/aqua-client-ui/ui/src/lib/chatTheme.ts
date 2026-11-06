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
