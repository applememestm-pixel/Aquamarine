/**
 * Theme + per-user settings. Applied by writing CSS custom properties
 * (--accent, --accent-2, --bg-shell, …) on <html>, so every styled
 * element in the app re-colours live. All of it saved in localStorage
 * so the choice survives a restart.
 *
 * Inspired by the PipoHuy ThemeStore / AppSettings pair — Discord-like
 * presets with a dark default, two palette knobs and a density toggle.
 */

import { writable, get } from "svelte/store";

export type Mode = "dark" | "light" | "auto";
export type Density = "comfortable" | "compact";

export interface Palette {
  /** Canonical slug of the preset ("aqua", "blurple", …). */
  id: string;
  /** Human-readable label for the picker. */
  name: string;
  /** Primary accent — buttons, verified dots, outgoing bubbles. */
  accent: string;
  /** Secondary accent — gradients, two-tone glow. */
  accent2: string;
  /** Soft tint of accent used for hover backgrounds + bubble lifts. */
  accentSoft: string;
  /** Thin halo colour applied as the box-shadow glow. */
  accentGlow: string;
}

/** Palette presets inspired by what felt right in PipoHuy + what reads
 *  as "ours". The first entry is the current default. */
export const PALETTES: Palette[] = [
  { id: "aqua",     name: "Aquamarine",   accent: "#00E5C7", accent2: "#7B5CFF", accentSoft: "rgba(0,229,199,0.14)",  accentGlow: "rgba(0,229,199,0.45)"  },
  { id: "blurple",  name: "Blurple",      accent: "#5865F2", accent2: "#9B6CFF", accentSoft: "rgba(88,101,242,0.15)", accentGlow: "rgba(88,101,242,0.5)"  },
  { id: "rose",     name: "Rose",         accent: "#FF7CAC", accent2: "#FFB067", accentSoft: "rgba(255,124,172,0.14)",accentGlow: "rgba(255,124,172,0.45)"},
  { id: "emerald",  name: "Emerald",      accent: "#2ECC71", accent2: "#1ABC9C", accentSoft: "rgba(46,204,113,0.14)", accentGlow: "rgba(46,204,113,0.45)" },
  { id: "sunset",   name: "Sunset",       accent: "#FF6B6B", accent2: "#FFD93D", accentSoft: "rgba(255,107,107,0.14)",accentGlow: "rgba(255,107,107,0.45)"},
  { id: "midnight", name: "Midnight",     accent: "#A788FF", accent2: "#3E8EFF", accentSoft: "rgba(167,136,255,0.14)",accentGlow: "rgba(167,136,255,0.5)" },
];

/** Pre-canned chat background wallpapers. "none" uses the room's
 *  natural dark gradient; the rest are soft CSS gradients evocative
 *  of Telegram-style patterns. "custom" lets the user drop in a
 *  data URL from a local image. */
export interface Wallpaper {
  id: string;
  name: string;
  /** Full CSS `background` value (gradients, image URLs, …). */
  css: string;
}

export const WALLPAPERS: Wallpaper[] = [
  { id: "none",      name: "Default",    css: "" },
  { id: "aurora",    name: "Aurora",     css: "radial-gradient(1200px 500px at 20% 10%, color-mix(in srgb, var(--accent) 14%, transparent), transparent 60%), radial-gradient(900px 420px at 85% 90%, color-mix(in srgb, var(--accent-2) 14%, transparent), transparent 60%), var(--bg-shell)" },
  { id: "sunrise",   name: "Sunrise",    css: "radial-gradient(1000px 500px at 15% 90%, rgba(255,124,172,0.17), transparent 60%), radial-gradient(900px 420px at 85% 10%, rgba(255,176,103,0.14), transparent 60%), #15121A" },
  { id: "ocean",     name: "Ocean",      css: "linear-gradient(135deg, #0a2a3a 0%, #0f1b2d 50%, #0a1420 100%)" },
  { id: "forest",    name: "Forest",     css: "radial-gradient(900px 500px at 20% 10%, rgba(46,204,113,0.14), transparent 60%), radial-gradient(900px 420px at 85% 90%, rgba(26,188,156,0.12), transparent 60%), #0D1715" },
  { id: "midnight",  name: "Midnight",   css: "linear-gradient(135deg, #0F0A26 0%, #1A1240 50%, #0A0820 100%)" },
  { id: "parchment", name: "Parchment",  css: "radial-gradient(1000px 500px at 50% 0%, #faf6ea, #f0e9d2 70%, #e9e0c0)" },
  { id: "stars",     name: "Stars",      css: "radial-gradient(2px 2px at 20% 30%, rgba(255,255,255,0.4), transparent 2px), radial-gradient(1.5px 1.5px at 70% 60%, rgba(255,255,255,0.3), transparent 2px), radial-gradient(2px 2px at 40% 80%, rgba(255,255,255,0.4), transparent 2px), radial-gradient(1.5px 1.5px at 85% 20%, rgba(255,255,255,0.3), transparent 2px), linear-gradient(180deg, #0A0E1A, #06080F)" },
];

/**
 * Complete themes (surfaces + text + accents), à la editor themes.
 * Picking one sets the mode too; the theme editor and transparency still
 * apply on top.
 */
export interface ThemePreset {
  id: string;
  name: string;
  mode: "dark" | "light";
  colors: Record<string, string>;
}
const T = (id: string, name: string, mode: "dark" | "light", shell: string, rail: string, list: string, input: string, bubble: string, fg: string, muted: string, dim: string, accent: string, accent2: string): ThemePreset => ({
  id, name, mode,
  colors: {
    "--bg-shell": shell, "--bg-rail": rail, "--bg-list": list, "--bg-input": input,
    "--bg-bubble-in": bubble, "--bg-surface": input, "--bg-surface-2": bubble, "--bg-void": rail,
    "--fg": fg, "--fg-muted": muted, "--fg-dim": dim, "--accent": accent, "--accent-2": accent2,
  },
});
export const THEMES: ThemePreset[] = [
  T("github-dark", "GitHub Dark", "dark", "#0d1117", "#010409", "#0d1117", "#161b22", "#161b22", "#e6edf3", "#9198a1", "#6e7681", "#2f81f7", "#a371f7"),
  T("github-light", "GitHub Light", "light", "#ffffff", "#f6f8fa", "#f6f8fa", "#ffffff", "#f6f8fa", "#1f2328", "#59636e", "#8c959f", "#0969da", "#8250df"),
  T("vscode-dark", "VS Code Dark+", "dark", "#1e1e1e", "#181818", "#252526", "#2d2d2d", "#2d2d30", "#d4d4d4", "#a0a0a0", "#6a6a6a", "#007acc", "#c586c0"),
  T("vscode-light", "VS Code Light+", "light", "#ffffff", "#f3f3f3", "#f3f3f3", "#ffffff", "#f3f3f3", "#1f1f1f", "#616161", "#8a8a8a", "#007acc", "#af00db"),
  T("darcula", "IntelliJ Darcula", "dark", "#2b2b2b", "#313335", "#3c3f41", "#45494a", "#313335", "#bbbbbb", "#a9b7c6", "#787878", "#cc7832", "#6897bb"),
  T("dracula", "Dracula", "dark", "#282a36", "#191a21", "#21222c", "#343746", "#343746", "#f8f8f2", "#bdbfc9", "#6272a4", "#bd93f9", "#ff79c6"),
  T("one-dark", "One Dark", "dark", "#282c34", "#21252b", "#21252b", "#2c313c", "#2c313c", "#abb2bf", "#8b919d", "#5c6370", "#61afef", "#c678dd"),
  T("tokyo-night", "Tokyo Night", "dark", "#1a1b26", "#16161e", "#16161e", "#24283b", "#24283b", "#c0caf5", "#a9b1d6", "#565f89", "#7aa2f7", "#bb9af7"),
  T("nord", "Nord", "dark", "#2e3440", "#272c36", "#3b4252", "#434c5e", "#3b4252", "#eceff4", "#d8dee9", "#7b88a1", "#88c0d0", "#b48ead"),
  T("catppuccin-mocha", "Catppuccin Mocha", "dark", "#1e1e2e", "#11111b", "#181825", "#313244", "#313244", "#cdd6f4", "#a6adc8", "#6c7086", "#cba6f7", "#89b4fa"),
  T("catppuccin-latte", "Catppuccin Latte", "light", "#eff1f5", "#dce0e8", "#e6e9ef", "#ffffff", "#e6e9ef", "#4c4f69", "#6c6f85", "#9ca0b0", "#8839ef", "#1e66f5"),
  T("gruvbox", "Gruvbox", "dark", "#282828", "#1d2021", "#1d2021", "#3c3836", "#3c3836", "#ebdbb2", "#bdae93", "#7c6f64", "#fabd2f", "#fe8019"),
  T("monokai", "Monokai", "dark", "#272822", "#1e1f1c", "#1e1f1c", "#3e3d32", "#3e3d32", "#f8f8f2", "#cfcfc2", "#75715e", "#a6e22e", "#f92672"),
  T("solarized-dark", "Solarized Dark", "dark", "#002b36", "#00212b", "#073642", "#073642", "#073642", "#eee8d5", "#93a1a1", "#657b83", "#2aa198", "#6c71c4"),
  T("solarized-light", "Solarized Light", "light", "#fdf6e3", "#eee8d5", "#eee8d5", "#fdf6e3", "#eee8d5", "#073642", "#586e75", "#93a1a1", "#268bd2", "#d33682"),
  T("rose-pine-dawn", "Rosé Pine Dawn", "light", "#faf4ed", "#f2e9e1", "#fffaf3", "#fffaf3", "#f2e9e1", "#575279", "#797593", "#9893a5", "#d7827e", "#907aa9"),
  T("tg-night", "Telegram Night", "dark", "#212121", "#181818", "#212121", "#2c2c2c", "#2c2c2c", "#ffffff", "#aaaaaa", "#707579", "#8774e1", "#6c5ce7"),
  T("tg-day", "Telegram Day", "light", "#ffffff", "#f4f4f5", "#ffffff", "#f4f4f5", "#ffffff", "#000000", "#707579", "#a2acb4", "#3390ec", "#6c5ce7"),
];

/** A fully user-editable colour slot. The theme editor exposes these as
 *  colour pickers; whatever the user sets overrides the palette/mode
 *  defaults live. Keys are the CSS custom properties written on <html>. */
export interface EditableColor {
  key: string;   // CSS var, e.g. "--accent"
  label: string;
  group: "Accent" | "Surfaces" | "Text";
  /** Baseline hex for dark / light so the picker shows something sensible. */
  dark: string;
  light: string;
  /** Human description of what this colour drives — shown as a tooltip. */
  desc: string;
}

export const EDITABLE_COLORS: EditableColor[] = [
  { key: "--accent",       label: "Primary accent",   group: "Accent",   dark: "#00E5C7", light: "#00B59E", desc: "Main brand colour: buttons, your outgoing bubbles, active tabs, links and highlights." },
  { key: "--accent-2",     label: "Secondary accent", group: "Accent",   dark: "#7B5CFF", light: "#7B5CFF", desc: "Second colour of gradients and glows (send button, avatars, accent fades)." },
  { key: "--bg-shell",     label: "App background",   group: "Surfaces", dark: "#0C0F17", light: "#F7F8FB", desc: "The main window background behind the chat area." },
  { key: "--bg-rail",      label: "Side rail",        group: "Surfaces", dark: "#070A12", light: "#EEF0F6", desc: "The narrow icon rail on the far left (and the mobile bottom bar)." },
  { key: "--bg-list",      label: "Chat list",        group: "Surfaces", dark: "#0A0D15", light: "#F2F3F8", desc: "Background of the chat list and side panels." },
  { key: "--bg-input",     label: "Inputs / cards",   group: "Surfaces", dark: "#141825", light: "#FFFFFF", desc: "Text fields, cards, the message composer and other raised surfaces." },
  { key: "--bg-bubble-in", label: "Incoming bubble",  group: "Surfaces", dark: "#141825", light: "#FFFFFF", desc: "Background of messages you receive from other people." },
  { key: "--fg",           label: "Text",             group: "Text",     dark: "#E5EAF2", light: "#15181F", desc: "Primary text colour used for message content and headings." },
  { key: "--fg-muted",     label: "Muted text",       group: "Text",     dark: "#9CA3B2", light: "#4F5663", desc: "Secondary text: previews, subtitles, less important labels." },
  { key: "--fg-dim",       label: "Dim text",         group: "Text",     dark: "#5E6675", light: "#8A91A0", desc: "Faint text: timestamps, hints, section captions." },
];

export interface Settings {
  mode: Mode;
  palette: string;   // palette id
  density: Density;
  fontScale: number; // 0.85–1.25
  wallpaper: string; // wallpaper id, or "custom"
  wallpaperCustom?: string; // CSS value for custom wallpaper (data: URL)
  // Behaviour
  enterToSend: boolean;       // Enter sends (true) vs newline (false)
  autoDownloadMedia: boolean; // auto-fetch inline media on receive
  linkPreviews: boolean;      // fetch previews for links (and videos behind them)
  // Notifications
  notifySound: boolean;       // play a chime on incoming message
  notifyDesktop: boolean;     // OS desktop notification on incoming
  // --- Full customisation (the theme editor) ---
  /** Per-var colour overrides (CSS var → hex). Wins over palette/mode. */
  customColors?: Record<string, string>;
  /** A user-loaded UI font: label + the font file as a data: URL. */
  customFontName?: string;
  customFontData?: string;
  /** Complete theme (THEMES id); empty = the built-in look. */
  themePreset?: string;
  /** Opacity of block backgrounds, 0.3–1 (wallpaper shows through). */
  alphas?: { bubbles?: number; panels?: number; inputs?: number };
  // --- Chat look & media ---
  stickerSize: number;     // px
  mediaMaxWidth: number;   // px, photos / GIFs / videos in bubbles
  bubbleRadius: number;    // px
  autoplayGifs: boolean;   // GIFs animate in chats (off: tap to play)
  autoplayStickers: boolean; // animated stickers / emoji play
  autoplayVideos: boolean; // round videos & link videos start muted by themselves
  // --- Sounds ---
  /** Message sound: built-in id ("ding", "pop", …) or "file:<name>". */
  messageSound: string;
  soundVolume: number;     // 0–1
  /** Per chat (peer / group id) sound, same ids as messageSound; "none" = silent. */
  chatSounds?: Record<string, string>;
}

export const DEFAULTS: Settings = {
  mode: "dark",
  palette: "aqua",
  density: "comfortable",
  fontScale: 1,
  wallpaper: "aurora",
  enterToSend: true,
  autoDownloadMedia: true,
  linkPreviews: true,
  notifySound: true,
  notifyDesktop: false,
  stickerSize: 128,
  mediaMaxWidth: 360,
  bubbleRadius: 14,
  autoplayGifs: true,
  autoplayStickers: true,
  autoplayVideos: true,
  messageSound: "ding",
  soundVolume: 0.7,
};

const KEY = "aqua-settings";

function load(): Settings {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return { ...DEFAULTS };
    const parsed = JSON.parse(raw) as Partial<Settings>;
    return { ...DEFAULTS, ...parsed };
  } catch {
    return { ...DEFAULTS };
  }
}

export const settings = writable<Settings>(load());

settings.subscribe((s) => {
  try { localStorage.setItem(KEY, JSON.stringify(s)); } catch { /* ignore */ }
  apply(s);
});

/** Write the chosen settings into CSS variables on <html>. */
function apply(s: Settings) {
  if (typeof document === "undefined") return;
  const root = document.documentElement;

  // Resolve mode (auto → prefers-color-scheme)
  const presetMode = s.themePreset ? THEMES.find((x) => x.id === s.themePreset)?.mode : undefined;
  const effectiveMode: Exclude<Mode, "auto"> = presetMode ??
    (s.mode === "auto"
      ? (window.matchMedia?.("(prefers-color-scheme: light)").matches ? "light" : "dark")
      : s.mode);

  root.dataset.theme = effectiveMode;
  root.style.setProperty("color-scheme", effectiveMode);

  // Resolve palette
  const p = PALETTES.find((x) => x.id === s.palette) ?? PALETTES[0];
  root.style.setProperty("--accent",       p.accent);
  root.style.setProperty("--accent-2",     p.accent2);
  root.style.setProperty("--accent-soft",  p.accentSoft);
  root.style.setProperty("--accent-glow",  p.accentGlow);

  // Dark vs light surfaces
  if (effectiveMode === "light") {
    root.style.setProperty("--bg-shell",      "#F7F8FB");
    root.style.setProperty("--bg-rail",       "#EEF0F6");
    root.style.setProperty("--bg-list",       "#F2F3F8");
    root.style.setProperty("--bg-input",      "#FFFFFF");
    root.style.setProperty("--bg-hover",      "rgba(0, 0, 0, 0.04)");
    root.style.setProperty("--bg-active",     "rgba(0, 0, 0, 0.06)");
    root.style.setProperty("--bg-bubble-in",  "#FFFFFF");
    root.style.setProperty("--bg-surface-2",  "#FFFFFF");
    root.style.setProperty("--border",        "rgba(0, 0, 0, 0.08)");
    root.style.setProperty("--border-strong", "rgba(0, 0, 0, 0.16)");
    root.style.setProperty("--fg",            "#15181F");
    root.style.setProperty("--fg-muted",      "#4F5663");
    root.style.setProperty("--fg-dim",        "#8A91A0");
  } else {
    // Restore dark defaults (same values as the baked-in :root styles)
    root.style.setProperty("--bg-shell",      "#0C0F17");
    root.style.setProperty("--bg-rail",       "#070A12");
    root.style.setProperty("--bg-list",       "#0A0D15");
    root.style.setProperty("--bg-input",      "#141825");
    root.style.setProperty("--bg-hover",      "rgba(255, 255, 255, 0.04)");
    root.style.setProperty("--bg-active",     "rgba(0, 229, 199, 0.08)");
    root.style.setProperty("--bg-bubble-in",  "#141825");
    root.style.setProperty("--bg-surface-2",  "#1A1F2E");
    root.style.setProperty("--border",        "rgba(255, 255, 255, 0.06)");
    root.style.setProperty("--border-strong", "rgba(255, 255, 255, 0.14)");
    root.style.setProperty("--fg",            "#E5EAF2");
    root.style.setProperty("--fg-muted",      "#9CA3B2");
    root.style.setProperty("--fg-dim",        "#5E6675");
  }

  // Density affects base padding & gap scale through a multiplier.
  root.style.setProperty("--density", s.density === "compact" ? "0.84" : "1");

  // Font scale
  root.style.setProperty("--font-scale", String(s.fontScale));
  root.style.fontSize = `${16 * s.fontScale}px`;

  // Chat wallpaper — ChatWindow reads `--chat-wallpaper` on the
  // .messages container. "none" clears it; "custom" uses the stored
  // data URL; everything else is a preset's CSS value.
  let wallCss = "";
  if (s.wallpaper === "custom" && s.wallpaperCustom) {
    wallCss = s.wallpaperCustom;
  } else {
    const wp = WALLPAPERS.find((w) => w.id === s.wallpaper);
    wallCss = wp?.css ?? "";
  }
  root.style.setProperty("--chat-wallpaper", wallCss || "transparent");

  // --- Complete theme preset (GitHub, VS Code, Dracula, …) ---
  const preset = s.themePreset ? THEMES.find((x) => x.id === s.themePreset) : undefined;
  if (preset) {
    for (const [k, v] of Object.entries(preset.colors)) root.style.setProperty(k, v);
    root.style.setProperty("--accent-soft", hexToRgba(preset.colors["--accent"], 0.14));
    root.style.setProperty("--accent-glow", hexToRgba(preset.colors["--accent"], 0.42));
    root.style.setProperty("--accent-strong", preset.colors["--accent"]);
    root.style.setProperty("--accent-2-glow", hexToRgba(preset.colors["--accent-2"], 0.35));
    const light = preset.mode === "light";
    root.style.setProperty("--border", light ? "rgba(0,0,0,0.09)" : "rgba(255,255,255,0.07)");
    root.style.setProperty("--border-strong", light ? "rgba(0,0,0,0.17)" : "rgba(255,255,255,0.14)");
    root.style.setProperty("--bg-hover", light ? "rgba(0,0,0,0.045)" : "rgba(255,255,255,0.045)");
    root.style.setProperty("--bg-active", hexToRgba(preset.colors["--accent"], 0.1));
  }

  // --- Custom colour overrides (theme editor) win over everything. ---
  const custom = s.customColors ?? {};
  for (const { key } of EDITABLE_COLORS) {
    const v = custom[key];
    if (v) root.style.setProperty(key, v);
  }
  // If the accent was customised, keep the soft/glow halos in step so
  // hovers and shadows match the new colour instead of the old teal.
  if (custom["--accent"]) {
    root.style.setProperty("--accent-soft", hexToRgba(custom["--accent"], 0.14));
    root.style.setProperty("--accent-glow", hexToRgba(custom["--accent"], 0.45));
    root.style.setProperty("--accent-strong", custom["--accent"]);
  }

  // --- Transparency of blocks (wallpaper shows through) ---
  const a = s.alphas ?? {};
  const fade = (key: string, alpha: number | undefined) => {
    if (alpha == null || alpha >= 0.999) return;
    const cur = root.style.getPropertyValue(key).trim();
    if (/^#[0-9a-f]{6}$/i.test(cur)) root.style.setProperty(key, hexToRgba(cur, Math.max(0.15, alpha)));
  };
  fade("--bg-bubble-in", a.bubbles);
  fade("--bg-list", a.panels);
  fade("--bg-rail", a.panels);
  fade("--bg-input", a.inputs);

  // --- Chat look ---
  root.style.setProperty("--sticker-size", `${s.stickerSize ?? 128}px`);
  root.style.setProperty("--media-max", `${s.mediaMaxWidth ?? 360}px`);
  root.style.setProperty("--bubble-r", `${s.bubbleRadius ?? 14}px`);
  root.classList.toggle("no-autoplay-stickers", s.autoplayStickers === false);

  // Custom UI font loaded from a file the user picked.
  applyCustomFont(s);
}

/** Turn "#RRGGBB" into "rgba(r,g,b,a)". Falls back to the input on
 *  anything that isn't a 6-digit hex. */
function hexToRgba(hex: string, alpha: number): string {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim());
  if (!m) return hex;
  const n = parseInt(m[1], 16);
  const r = (n >> 16) & 255, g = (n >> 8) & 255, b = n & 255;
  return `rgba(${r},${g},${b},${alpha})`;
}

const CUSTOM_FONT_FAMILY = "AquaUserFont";
let loadedFontData: string | null = null;

/** Register (once per data URL) a user-supplied font and point --font-ui
 *  at it. Clearing the font restores the default stack. */
function applyCustomFont(s: Settings) {
  if (typeof document === "undefined") return;
  const root = document.documentElement;
  if (s.customFontData) {
    if (loadedFontData !== s.customFontData && "FontFace" in window) {
      try {
        const face = new FontFace(CUSTOM_FONT_FAMILY, `url(${s.customFontData})`);
        face.load().then((f) => {
          // @ts-ignore — document.fonts is a FontFaceSet
          document.fonts.add(f);
          loadedFontData = s.customFontData ?? null;
        }).catch(() => { /* bad font file — ignore */ });
      } catch { /* ignore */ }
    }
    root.style.setProperty(
      "--font-ui",
      `"${CUSTOM_FONT_FAMILY}", "Onest", ui-sans-serif, system-ui, sans-serif`,
    );
  } else {
    root.style.removeProperty("--font-ui");
  }
}

/** Call once from App.svelte's onMount to kick-start the live
 *  prefers-color-scheme listener when the mode is "auto". */
export function initTheme() {
  apply(get(settings));
  if (typeof window !== "undefined" && window.matchMedia) {
    const mq = window.matchMedia("(prefers-color-scheme: light)");
    const listener = () => {
      if (get(settings).mode === "auto") apply(get(settings));
    };
    try { mq.addEventListener("change", listener); } catch { mq.addListener(listener); }
  }
}
