/**
 * Voice & video preferences (persisted in localStorage). Read by the call
 * engine at call time; edited in Settings → Voice & Video.
 */
import { writable } from "svelte/store";
import { IS_MOBILE } from "./platform";
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
    // Push-to-talk needs a keyboard: never on a phone, even if a PC's
    // settings came over with a backup.
    if (raw) return { ...DEFAULT_CALL_SETTINGS, ...JSON.parse(raw), ...(IS_MOBILE ? { pushToTalk: false } : {}) };
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
/** My server's TURN relay (aqua-server turnsrv.rs): without one, calls
 *  between a phone on mobile data and most home routers never connected. */
let relayTurn: RTCIceServer | null = null;
let relayAt = 0;
export async function refreshRelayIce(force = false): Promise<void> {
  // Credentials live 12 h: fetched again after 6.
  if (!force && relayTurn && Date.now() - relayAt < 6 * 3600_000) return;
  try {
    const { api } = await import("./tauri");
    const r = await api.relayIce();
    if (r) { relayTurn = r; relayAt = Date.now(); }
  } catch { /* offline: keep the old one */ }
}

export function iceServersFor(s: CallSettings): RTCIceServer[] {
  const list: RTCIceServer[] = [
    { urls: ["stun:stun.l.google.com:19302", "stun:stun1.l.google.com:19302"] },
    { urls: "stun:stun.cloudflare.com:3478" },
  ];
  const urls = s.turnUrl.split(/[\s,]+/).map((u) => u.trim()).filter(Boolean);
  if (urls.length) list.push({ urls, username: s.turnUser || undefined, credential: s.turnPass || undefined });
  if (relayTurn) list.push(relayTurn);
  // Look again in the background (the next call gets fresh credentials).
  void refreshRelayIce();
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
