/**
 * The app's own logo — a shield holding a chat bubble with an "A" — drawn as
 * SVG so it can follow the theme. Three modes (Settings → Appearance):
 *   theme  — colours derived from the current accent (default);
 *   preset — one of ICON_PRESETS;
 *   custom — the user's own square picture.
 * The result feeds the title bar, the nav rail, the favicon and the
 * window / taskbar icon (X11, Windows, macOS; Wayland reads .desktop).
 */
import { derived, get } from "svelte/store";
import { settings } from "./theme";
import { invoke } from "@tauri-apps/api/core";

export interface IconColors { fg: string; bg1: string; bg2: string }
export interface AppIconSetting { mode: "theme" | "preset" | "custom"; preset?: string; custom?: string }

export const ICON_PRESETS: ({ id: string; name: string } & IconColors)[] = [
  { id: "aqua",    name: "Aquamarine", fg: "#3FD9FF", bg1: "#10A57A", bg2: "#053A2D" },
  { id: "neon",    name: "Neon",       fg: "#7CF9FF", bg1: "#2B2F8F", bg2: "#0B0C2A" },
  { id: "sunset",  name: "Sunset",     fg: "#FFE07A", bg1: "#E2552B", bg2: "#5A1414" },
  { id: "violet",  name: "Violet",     fg: "#F0D8FF", bg1: "#7C3AED", bg2: "#2A0E5C" },
  { id: "rose",    name: "Rose",       fg: "#FFE3EE", bg1: "#E8437A", bg2: "#4A0F26" },
  { id: "mono",    name: "Graphite",   fg: "#FFFFFF", bg1: "#3A4150", bg2: "#11141B" },
  { id: "paper",   name: "Paper",      fg: "#0E8FD6", bg1: "#F4FBF9", bg2: "#CDEDE4" },
];

// ---- colour helpers ------------------------------------------------------
function hexToHsl(hex: string): [number, number, number] {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim());
  const n = m ? parseInt(m[1], 16) : 0x00e5c7;
  const r = ((n >> 16) & 255) / 255, g = ((n >> 8) & 255) / 255, b = (n & 255) / 255;
  const max = Math.max(r, g, b), min = Math.min(r, g, b);
  const l = (max + min) / 2;
  if (max === min) return [0, 0, l];
  const d = max - min;
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
  const h = max === r ? (g - b) / d + (g < b ? 6 : 0) : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
  return [h * 60, s, l];
}
const hsl = (h: number, s: number, l: number) =>
  `hsl(${Math.round(((h % 360) + 360) % 360)} ${Math.round(Math.min(1, Math.max(0, s)) * 100)}% ${Math.round(Math.min(1, Math.max(0, l)) * 100)}%)`;

/** Logo colours that follow an accent: body in the accent, neon glyph a little bluer. */
export function colorsFromAccent(accent: string): IconColors {
  const [h, s, l] = hexToHsl(accent);
  return {
    fg: hsl(h + 24, Math.max(s, 0.85), Math.max(l, 0.62)),
    bg1: hsl(h - 8, Math.min(1, s * 0.9 + 0.05), Math.min(0.42, l * 0.75)),
    bg2: hsl(h - 14, Math.min(1, s * 0.8), 0.11),
  };
}

function currentAccent(): string {
  if (typeof document === "undefined") return "#00E5C7";
  const v = getComputedStyle(document.documentElement).getPropertyValue("--accent").trim();
  return /^#[0-9a-f]{6}$/i.test(v) ? v : "#00E5C7";
}

// ---- drawing -------------------------------------------------------------
export function logoSvg(c: IconColors): string {
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512">
<defs>
<linearGradient id="bg" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="${c.bg1}"/><stop offset="1" stop-color="${c.bg2}"/></linearGradient>
<radialGradient id="sh" cx="0.5" cy="0.3" r="0.7"><stop offset="0" stop-color="${c.fg}" stop-opacity="0.22"/><stop offset="1" stop-color="${c.fg}" stop-opacity="0"/></radialGradient>
<filter id="glow" x="-40%" y="-40%" width="180%" height="180%"><feGaussianBlur stdDeviation="9" result="b"/><feMerge><feMergeNode in="b"/><feMergeNode in="b"/><feMergeNode in="SourceGraphic"/></feMerge></filter>
</defs>
<rect width="512" height="512" rx="116" fill="url(#bg)"/>
<g fill="none" stroke="${c.fg}" stroke-linejoin="round" stroke-linecap="round" filter="url(#glow)">
<path d="M256 62 L410 116 V246 C410 350 344 418 256 454 C168 418 102 350 102 246 V116 Z" fill="url(#sh)" stroke-width="22"/>
<path d="M186 170 H326 a32 32 0 0 1 32 32 V286 a32 32 0 0 1 -32 32 H266 L222 356 V318 H186 a32 32 0 0 1 -32 -32 V202 a32 32 0 0 1 32 -32 Z" stroke-width="17"/>
<path d="M218 292 L256 196 L294 292 M232 258 H280" stroke-width="20"/>
</g>
</svg>`;
}

export function svgDataUrl(svg: string): string {
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}

/** Bump to re-read the accent after a theme change applied CSS vars. */
export const appIconUrl = derived(settings, (s, set) => {
  // The theme store writes CSS vars in its own subscriber; read them a tick later.
  const id = setTimeout(() => set(resolveIcon(s.appIcon)), 0);
  return () => clearTimeout(id);
}, svgDataUrl(logoSvg(ICON_PRESETS[0])));

export function resolveIcon(a: AppIconSetting | undefined): string {
  if (a?.mode === "custom" && a.custom) return a.custom;
  if (a?.mode === "preset") {
    const p = ICON_PRESETS.find((x) => x.id === a.preset) ?? ICON_PRESETS[0];
    return svgDataUrl(logoSvg(p));
  }
  return svgDataUrl(logoSvg(colorsFromAccent(currentAccent())));
}

// ---- favicon + window icon ----------------------------------------------
async function rasterize(url: string, size: number): Promise<Uint8ClampedArray | null> {
  const img = new Image();
  img.decoding = "async";
  img.src = url;
  try { await img.decode(); } catch { return null; }
  const c = document.createElement("canvas");
  c.width = c.height = size;
  const g = c.getContext("2d");
  if (!g) return null;
  // Cover-fit a custom picture; the SVG is already square.
  const k = Math.max(size / img.naturalWidth || 1, size / img.naturalHeight || 1);
  const w = img.naturalWidth * k, h = img.naturalHeight * k;
  g.drawImage(img, (size - w) / 2, (size - h) / 2, w, h);
  return g.getImageData(0, 0, size, size).data;
}

let lastApplied = "";
let timer: ReturnType<typeof setTimeout> | null = null;
/** Keep favicon and window icon in step with the chosen logo. */
export function startAppIcon() {
  return appIconUrl.subscribe((url) => {
    if (typeof document === "undefined") return;
    let link = document.querySelector<HTMLLinkElement>("link[rel~='icon']");
    if (!link) { link = document.createElement("link"); link.rel = "icon"; document.head.appendChild(link); }
    link.href = url;
    if (url === lastApplied) return;
    if (timer) clearTimeout(timer);
    timer = setTimeout(async () => {
      lastApplied = url;
      const size = 128;
      const px = await rasterize(url, size);
      if (!px) return;
      try { await invoke("set_window_icon", { rgba: Array.from(px), size }); } catch { /* browser / Wayland */ }
    }, 400);
  });
}

export function setAppIcon(a: AppIconSetting) {
  settings.update((s) => ({ ...s, appIcon: a }));
}
export const appIconSetting = () => get(settings).appIcon ?? { mode: "theme" as const };
