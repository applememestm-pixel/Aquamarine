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
      if (url === DEFAULT_ICON && !lastCustom) return;
      lastCustom = url !== DEFAULT_ICON;
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
export const appIconSetting = () => get(settings).appIcon ?? { mode: "default" as const };
