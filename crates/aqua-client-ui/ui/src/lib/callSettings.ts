/**
 * Voice & video preferences (persisted in localStorage). Read by the call
 * engine at call time; edited in Settings → Voice & Video.
 */
import { writable } from "svelte/store";
import type { CodecPref, ContentHint, FpsPreset, ResPreset } from "./rtcQuality";

export interface CallSettings {
  micId: string;
  speakerId: string;
  camId: string;

  echoCancellation: boolean;
  noiseSuppression: boolean;
  autoGainControl: boolean;
  /** Mic gain applied before sending, 0..2 (1 = unchanged). */
  inputVolume: number;
  /** Master output volume, 0..1. */
  outputVolume: number;
  /** Noise gate threshold in dBFS; -100 = gate off (Discord's "input sensitivity"). */
  gateDb: number;

  pushToTalk: boolean;
  /** KeyboardEvent.code, or "Mouse3"/"Mouse4" for side buttons. */
  pttKey: string;

  /** Opus voice bitrate, kbps. */
  voiceKbps: number;

  screenRes: ResPreset;
  screenFps: FpsPreset;
  screenHint: ContentHint;
  screenAudio: boolean;
  camRes: "720" | "1080";
  codec: CodecPref;
  /** Upper bound for any outgoing video, kbps. 0 = automatic per preset. */
  maxVideoKbps: number;

  turnUrl: string;
  turnUser: string;
  turnPass: string;

  ringtone: boolean;
  showStats: boolean;
  /** Per-peer playback volume 0..1, keyed by peer id. */
  peerVolume: Record<string, number>;
  streamVolume: Record<string, number>;
}

export const DEFAULT_CALL_SETTINGS: CallSettings = {
  micId: "default",
  speakerId: "default",
  camId: "default",
  echoCancellation: true,
  noiseSuppression: true,
  autoGainControl: true,
  inputVolume: 1,
  outputVolume: 1,
  gateDb: -100,
  pushToTalk: false,
  pttKey: "Backquote",
  voiceKbps: 96,
  screenRes: "1080",
  screenFps: 60,
  screenHint: "motion",
  screenAudio: true,
  camRes: "720",
  codec: "auto",
  maxVideoKbps: 0,
  turnUrl: "",
  turnUser: "",
  turnPass: "",
  ringtone: true,
  showStats: false,
  peerVolume: {},
  streamVolume: {},
};

const KEY = "aqua.call.v1";

function load(): CallSettings {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw) return { ...DEFAULT_CALL_SETTINGS, ...JSON.parse(raw) };
  } catch { /* ignore */ }
  return { ...DEFAULT_CALL_SETTINGS };
}

export const callSettings = writable<CallSettings>(load());
callSettings.subscribe((v) => {
  try { localStorage.setItem(KEY, JSON.stringify(v)); } catch { /* ignore */ }
});

export function patchCallSettings(p: Partial<CallSettings>) {
  callSettings.update((s) => ({ ...s, ...p }));
}

/** ICE servers: two public STUNs plus the user's TURN relay if configured.
 *  TURN is what makes calls work between two strict NATs (mobile, CGNAT). */
export function iceServersFor(s: CallSettings): RTCIceServer[] {
  const list: RTCIceServer[] = [
    { urls: ["stun:stun.l.google.com:19302", "stun:stun1.l.google.com:19302"] },
    { urls: "stun:stun.cloudflare.com:3478" },
  ];
  const urls = s.turnUrl.split(/[\s,]+/).map((u) => u.trim()).filter(Boolean);
  if (urls.length) list.push({ urls, username: s.turnUser || undefined, credential: s.turnPass || undefined });
  return list;
}

/** Pretty name for a PTT binding. */
export function keyLabel(code: string): string {
  if (code === "Mouse3") return "Mouse 4";
  if (code === "Mouse4") return "Mouse 5";
  if (code.startsWith("Key")) return code.slice(3);
  if (code.startsWith("Digit")) return code.slice(5);
  if (code === "Backquote") return "`";
  return code;
}
