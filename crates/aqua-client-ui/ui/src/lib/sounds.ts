/**
 * Notification sounds: a few built-in ones (synthesised with WebAudio — no
 * files) plus whatever the user drops into the app's Sounds folder.
 * Per-chat sounds live in settings.chatSounds.
 */
import { writable, get } from "svelte/store";
import { api } from "./tauri";
import { settings } from "./theme";
import { streamUrl } from "./media";

export interface SoundFile { id: string; name: string; path: string }
export const BUILTIN_SOUNDS: { id: string; name: string }[] = [
  { id: "ding", name: "Ding" },
  { id: "pop", name: "Pop" },
  { id: "chime", name: "Chime" },
  { id: "bell", name: "Bell" },
  { id: "drop", name: "Drop" },
  { id: "knock", name: "Knock" },
  { id: "blip", name: "Blip" },
  { id: "none", name: "Silent" },
];

export const customSounds = writable<SoundFile[]>([]);
export async function refreshSounds() {
  try { customSounds.set(await api.listSounds()); } catch { /* not in Tauri */ }
}

export function soundLabel(id: string): string {
  if (id.startsWith("file:")) return id.slice(5).replace(/\.[^.]+$/, "");
  return BUILTIN_SOUNDS.find((b) => b.id === id)?.name ?? id;
}

/** The sound for a message in `chatId` (per-chat override, else the default). */
export function soundFor(chatId: string): string {
  const s = get(settings);
  return s.chatSounds?.[chatId] ?? s.messageSound ?? "ding";
}

let ctx: AudioContext | null = null;
function audio(): AudioContext | null {
  const Ctx = window.AudioContext || (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
  if (!Ctx) return null;
  ctx ??= new Ctx();
  if (ctx.state === "suspended") void ctx.resume();
  return ctx;
}

function tone(c: AudioContext, freq: number, start: number, dur: number, vol: number, type: OscillatorType = "sine", glideTo?: number) {
  const o = c.createOscillator();
  const g = c.createGain();
  o.type = type;
  o.frequency.setValueAtTime(freq, c.currentTime + start);
  if (glideTo) o.frequency.exponentialRampToValueAtTime(glideTo, c.currentTime + start + dur);
  g.gain.setValueAtTime(0.0001, c.currentTime + start);
  g.gain.exponentialRampToValueAtTime(Math.max(0.0002, vol), c.currentTime + start + 0.012);
  g.gain.exponentialRampToValueAtTime(0.0001, c.currentTime + start + dur);
  o.connect(g); g.connect(c.destination);
  o.start(c.currentTime + start); o.stop(c.currentTime + start + dur + 0.02);
}

function synth(id: string, vol: number) {
  const c = audio();
  if (!c) return;
  const v = 0.16 * vol;
  switch (id) {
    case "pop": tone(c, 520, 0, 0.09, v, "sine", 920); break;
    case "chime": tone(c, 1046, 0, 0.35, v * 0.8); tone(c, 1568, 0.09, 0.45, v * 0.6); break;
    case "bell": tone(c, 660, 0, 0.9, v, "triangle"); tone(c, 1320, 0, 0.6, v * 0.3); break;
    case "drop": tone(c, 1400, 0, 0.18, v, "sine", 380); break;
    case "knock": tone(c, 180, 0, 0.07, v * 1.6, "square"); tone(c, 160, 0.13, 0.07, v * 1.6, "square"); break;
    case "blip": tone(c, 880, 0, 0.06, v, "square"); tone(c, 1320, 0.07, 0.06, v * 0.8, "square"); break;
    case "none": break;
    default: tone(c, 880, 0, 0.25, v); // ding
  }
}

/** Play a sound id at `volume` (defaults to the user's sound volume). */
export async function playSound(id: string, volume?: number) {
  const vol = volume ?? get(settings).soundVolume ?? 0.7;
  if (!id.startsWith("file:")) { synth(id, vol); return; }
  const name = id.slice(5);
  let list = get(customSounds);
  if (!list.length) { await refreshSounds(); list = get(customSounds); }
  const f = list.find((x) => x.id === name);
  if (!f) { synth("ding", vol); return; }
  try {
    const el = new Audio(await streamUrl(f.path));
    el.crossOrigin = "anonymous";
    el.volume = Math.max(0, Math.min(1, vol));
    await el.play();
  } catch { synth("ding", vol); }
}

/** Add sound files the user picked (bytes via the Rust side). */
export async function addSoundBytes(name: string, bytes: Uint8Array): Promise<SoundFile> {
  const f = await api.saveSound(name, Array.from(bytes));
  await refreshSounds();
  return f;
}
