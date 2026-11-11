/**
 * The app's logo — the shield with the "A" chat bubble (public/logo.webp,
 * the same picture as the installed icons). Settings → Appearance can swap
 * it for the user's own square picture. The result feeds the title bar, the
 * nav rail, the favicon and the window / taskbar icon (X11, Windows, macOS;
 * Wayland reads .desktop).
 */
import { derived, get } from "svelte/store";
import { settings } from "./theme";
import { invoke } from "@tauri-apps/api/core";
import { IS_WINDOWS } from "./platform";

/** "theme" / "preset" are left over from the old drawn logo: shown as the default. */
export interface AppIconSetting { mode: "default" | "custom" | "theme" | "preset"; preset?: string; custom?: string }

export const DEFAULT_ICON = "/logo.webp";

export const appIconUrl = derived(settings, (s) => resolveIcon(s.appIcon), DEFAULT_ICON);

export function resolveIcon(a: AppIconSetting | undefined): string {
  return a?.mode === "custom" && a.custom ? a.custom : DEFAULT_ICON;
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
  // Cover-fit a custom picture.
  const k = Math.max(size / img.naturalWidth || 1, size / img.naturalHeight || 1);
  const w = img.naturalWidth * k, h = img.naturalHeight * k;
  g.drawImage(img, (size - w) / 2, (size - h) / 2, w, h);
  return g.getImageData(0, 0, size, size).data;
}

let lastApplied = "";
let lastCustom = false;
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
      // The bundled icon is already the window icon; only a custom one needs pushing.
      if (url === DEFAULT_ICON && !lastCustom) { if (IS_WINDOWS && (() => { try { return localStorage.getItem("aqua.shortcutIcon"); } catch { return null; } })()) void syncShortcuts(url); return; }
      lastCustom = url !== DEFAULT_ICON;
      const size = 128;
      const px = await rasterize(url, size);
      if (!px) return;
      try { await invoke("set_window_icon", { rgba: Array.from(px), size }); } catch { /* browser / Wayland */ }
      if (IS_WINDOWS) void syncShortcuts(url);
    }, 400);
  });
}

/** Windows: desktop and Start-menu shortcuts follow the logo — only when it changed. */
async function syncShortcuts(url: string) {
  let h = 0;
  for (let i = 0; i < url.length; i += Math.max(1, Math.floor(url.length / 4096))) h = (h * 31 + url.charCodeAt(i)) | 0;
  const tag = `${url.length}:${h}`;
  try { if (localStorage.getItem("aqua.shortcutIcon") === tag) return; } catch { /* storage blocked */ }
  let png: number[] | null = null;
  if (url !== DEFAULT_ICON) {
    const px = await rasterize(url, 256);
    if (!px) return;
    const c = document.createElement("canvas");
    c.width = c.height = 256;
    c.getContext("2d")!.putImageData(new ImageData(px, 256, 256), 0, 0);
    const blob = await new Promise<Blob | null>((r) => c.toBlob(r, "image/png"));
    if (!blob) return;
    png = Array.from(new Uint8Array(await blob.arrayBuffer()));
  }
  try {
    await invoke("set_shortcut_icon", { png });
    try { localStorage.setItem("aqua.shortcutIcon", tag); } catch { /* ignore */ }
  } catch { /* not this platform */ }
}

export function setAppIcon(a: AppIconSetting) {
  settings.update((s) => ({ ...s, appIcon: a }));
}
export const appIconSetting = () => get(settings).appIcon ?? { mode: "default" as const };
