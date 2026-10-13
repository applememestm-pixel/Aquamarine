/**
 * Share your whole look as one file (`<name>.aquatheme`, JSON): theme or
 * custom colours with transparency, wallpaper, font, chat sizes, autoplay
 * switches and the message sound (custom sound files travel inside).
 * Like Telegram themes — send it in a chat, the other side taps "Apply".
 */
import { get } from "svelte/store";
import { save, open } from "@tauri-apps/plugin-dialog";
import { api } from "./tauri";
import { settings, type Settings } from "./theme";
import { customSounds, refreshSounds } from "./sounds";
import { streamUrl } from "./media";

export const THEME_EXT = "aquatheme";
const SHARED_KEYS = [
  "mode", "palette", "density", "fontScale", "wallpaper", "wallpaperCustom", "customColors",
  "customFontName", "customFontData", "themePreset", "alphas", "stickerSize", "mediaMaxWidth",
  "bubbleRadius", "autoplayGifs", "autoplayStickers", "autoplayVideos", "messageSound", "soundVolume",
] as const satisfies readonly (keyof Settings)[];

export interface ThemeFile {
  format: "aquamarine-theme";
  version: 1;
  name: string;
  createdAt: string;
  settings: Partial<Settings>;
  /** Custom sound files used by the theme (base64). */
  sounds?: { name: string; data: string }[];
}

function toB64(bytes: Uint8Array): string {
  let bin = "";
  for (let i = 0; i < bytes.length; i += 0x8000) bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(bin);
}
function fromB64(s: string): Uint8Array {
  const bin = atob(s);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

export async function buildThemeFile(name: string): Promise<ThemeFile> {
  const s = get(settings);
  const picked: Partial<Settings> = {};
  for (const k of SHARED_KEYS) if (s[k] !== undefined) (picked as Record<string, unknown>)[k] = s[k];
  const file: ThemeFile = { format: "aquamarine-theme", version: 1, name: name.trim() || "My theme", createdAt: new Date().toISOString(), settings: picked };
  if (s.messageSound?.startsWith("file:")) {
    await refreshSounds();
    const f = get(customSounds).find((x) => x.id === s.messageSound.slice(5));
    if (f) {
      try {
        const buf = new Uint8Array(await (await fetch(await streamUrl(f.path))).arrayBuffer());
        file.sounds = [{ name: f.id, data: toB64(buf) }];
      } catch { /* share without the sound */ }
    }
  }
  return file;
}

const fileName = (name: string) => `${(name.trim() || "theme").replace(/[\\/:*?"<>|]+/g, "_")}.${THEME_EXT}`;

/** Save dialog → file on disk. Returns the path, or null if cancelled. */
export async function exportThemeToFile(name: string): Promise<string | null> {
  const path = await save({ title: "Save theme", defaultPath: fileName(name), filters: [{ name: "Aquamarine theme", extensions: [THEME_EXT] }] });
  if (!path) return null;
  await api.writeThemeFile(path, JSON.stringify(await buildThemeFile(name), null, 1));
  return path;
}

/** Send the theme into a chat as a file. */
export async function sendThemeToChat(peerId: string, name: string): Promise<void> {
  const bytes = new TextEncoder().encode(JSON.stringify(await buildThemeFile(name)));
  await api.attachBytes(peerId, fileName(name), "application/json", Array.from(bytes));
}

function validate(raw: unknown): ThemeFile {
  const f = raw as ThemeFile;
  if (!f || f.format !== "aquamarine-theme" || typeof f.settings !== "object") throw new Error("not an Aquamarine theme file");
  const clean: Partial<Settings> = {};
  for (const k of SHARED_KEYS) {
    const v = (f.settings as Record<string, unknown>)[k];
    if (v !== undefined) (clean as Record<string, unknown>)[k] = v;
  }
  // Wallpaper / font must stay data: URLs or CSS gradients (no remote fetches).
  if (typeof clean.wallpaperCustom === "string" && /url\(\s*["']?(?!data:)/i.test(clean.wallpaperCustom)) delete clean.wallpaperCustom;
  if (typeof clean.customFontData === "string" && !clean.customFontData.startsWith("data:")) delete clean.customFontData;
  return { ...f, settings: clean };
}

/** Apply a theme from its JSON text. Returns the theme's name. */
export async function applyThemeText(text: string): Promise<string> {
  const f = validate(JSON.parse(text));
  for (const snd of f.sounds ?? []) {
    try { await api.saveSound(snd.name, Array.from(fromB64(snd.data))); } catch { /* skip */ }
  }
  if (f.sounds?.length) await refreshSounds();
  settings.update((s) => ({ ...s, ...f.settings }));
  return f.name;
}

export async function applyThemeFromPath(path: string): Promise<string> {
  return applyThemeText(await api.readThemeFile(path));
}

/** Open dialog → apply. Returns the name, or null if cancelled. */
export async function pickAndApplyTheme(): Promise<string | null> {
  const res = await open({ multiple: false, directory: false, title: "Open theme", filters: [{ name: "Aquamarine theme", extensions: [THEME_EXT, "json"] }] });
  const path = Array.isArray(res) ? res[0] : res;
  if (!path) return null;
  return applyThemeFromPath(path);
}
