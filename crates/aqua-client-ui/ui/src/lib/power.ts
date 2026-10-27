/**
 * Power saving, like Telegram's: a mode (off / when the battery is below N%
 * / always on) and what may move. While the mode is on, everything below
 * counts as off; otherwise each switch decides by itself.
 *
 * The classes on <html> (`ps-ui`, `ps-chat`, `ps-calls`, `ps-emoji`,
 * `ps-particles`) do the CSS part (app.css); components read `power`.
 */
import { derived, writable, type Readable } from "svelte/store";
import { settings, POWER_DEFAULT, type Settings } from "./theme";

export type PowerKey = keyof typeof POWER_DEFAULT;

/** The battery, where the system tells it (Android's WebView, Chrome on
 *  laptops). `null`: unknown — "when below N%" then never switches on. */
export const battery = writable<{ level: number; charging: boolean } | null>(null);

interface BatteryLike extends EventTarget { level: number; charging: boolean }
let started = false;
export function startPower() {
  if (started || typeof navigator === "undefined") return;
  started = true;
  const nav = navigator as Navigator & { getBattery?: () => Promise<BatteryLike> };
  if (!nav.getBattery) return;
  void nav.getBattery().then((b) => {
    const put = () => battery.set({ level: Math.round(b.level * 100), charging: b.charging });
    put();
    b.addEventListener("levelchange", put);
    b.addEventListener("chargingchange", put);
  }).catch(() => {});
}

/** 0 = off, 100 = always on, anything between = "when below that %". */
export function powerMode(s: Settings): number {
  return typeof s.powerMode === "number" ? s.powerMode : 0;
}

/** Is power saving on right now? */
export const powerActive: Readable<boolean> = derived([settings, battery], ([s, b]) => {
  const m = powerMode(s);
  if (m >= 100) return true;
  if (m <= 0 || !b) return false;
  return !b.charging && b.level < m;
});

/** What may move now: the switches, all off while the mode is on. */
export const power: Readable<typeof POWER_DEFAULT & { videos: boolean; gifs: boolean }> = derived([settings, powerActive], ([s, on]) => {
  const p = { ...POWER_DEFAULT, ...(s.power ?? {}) };
  const videos = s.autoplayVideos !== false;
  const gifs = s.autoplayGifs !== false;
  if (on) return { stickers: false, emoji: false, chat: false, calls: false, ui: false, particles: false, videos: false, gifs: false };
  return { ...p, videos, gifs };
});

// The CSS side follows the effective switches.
if (typeof document !== "undefined") {
  power.subscribe((p) => {
    const root = document.documentElement;
    root.classList.toggle("ps-ui", !p.ui);
    root.classList.toggle("ps-chat", !p.chat);
    root.classList.toggle("ps-calls", !p.calls);
    root.classList.toggle("ps-emoji", !p.emoji);
    root.classList.toggle("ps-particles", !p.particles);
  });
}
