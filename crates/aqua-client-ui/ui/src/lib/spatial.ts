/**
 * Spatial sound in voice rooms: every voice comes from its own place
 * around you (HRTF panning — use headphones), and the room can sound like
 * a small room or a hall (a synthetic reverb). Positions are arranged in an
 * arc in front of you; drag people on the room map to move them.
 *
 * Remote <audio> elements stay mounted but muted while this is on — the
 * sound goes through Web Audio instead.
 */
import { writable, get } from "svelte/store";

export type RoomKind = "none" | "room" | "hall";
interface Conf { on: boolean; room: RoomKind }

function persisted<T>(key: string, init: T) {
  let v = init;
  try { const raw = localStorage.getItem(key); if (raw) v = { ...init, ...JSON.parse(raw) }; } catch { /* ignore */ }
  const s = writable<T>(v);
  s.subscribe((x) => { try { localStorage.setItem(key, JSON.stringify(x)); } catch { /* ignore */ } });
  return s;
}

export const spatial = persisted<Conf>("aqua.spatial.v1", { on: false, room: "none" });
/** "gid|user" → position on the room map, x/y in −1…1 (y −1 = in front). */
export const spatialPos = persisted<Record<string, { x: number; y: number }>>("aqua.spatialPos.v1", {});

let ctx: AudioContext | null = null;
let master: GainNode | null = null;
let dry: GainNode | null = null;
let wet: GainNode | null = null;
let verb: ConvolverNode | null = null;
let room: RoomKind = "none";
const nodes = new Map<string, { src: MediaStreamAudioSourceNode; pan: PannerNode; stream: MediaStream }>();
let gid = "";
let deaf = false;

function graph(): AudioContext {
  if (ctx) return ctx;
  ctx = new AudioContext({ latencyHint: "interactive" });
  master = ctx.createGain();
  dry = ctx.createGain();
  wet = ctx.createGain();
  verb = ctx.createConvolver();
  master.connect(dry).connect(ctx.destination);
  master.connect(verb).connect(wet).connect(ctx.destination);
  setRoom(get(spatial).room);
  master.gain.value = deaf ? 0 : 1;
  return ctx;
}

/** A decaying stereo noise burst: a cheap, convincing room impulse. */
function impulse(c: AudioContext, seconds: number, decay: number): AudioBuffer {
  const len = Math.floor(c.sampleRate * seconds);
  const buf = c.createBuffer(2, len, c.sampleRate);
  for (let ch = 0; ch < 2; ch++) {
    const d = buf.getChannelData(ch);
    for (let i = 0; i < len; i++) d[i] = (Math.random() * 2 - 1) * Math.pow(1 - i / len, decay);
  }
  return buf;
}

function setRoom(r: RoomKind) {
  room = r;
  if (!ctx || !verb || !wet || !dry) return;
  if (r === "none") { wet.gain.value = 0; dry.gain.value = 1; return; }
  verb.buffer = r === "hall" ? impulse(ctx, 2.6, 3.2) : impulse(ctx, 0.7, 2.4);
  wet.gain.value = r === "hall" ? 0.38 : 0.22;
  dry.gain.value = r === "hall" ? 0.85 : 0.92;
}

/** Default place of the i-th of n voices: an arc in front of you. */
export function arcSpot(i: number, n: number): { x: number; y: number } {
  const span = Math.min(150, 40 + n * 25) * (Math.PI / 180);
  const a = n <= 1 ? 0 : -span / 2 + (span * i) / (n - 1);
  return { x: Math.sin(a) * 0.62, y: -Math.cos(a) * 0.62 };
}

export function placeOf(user: string, order: string[]): { x: number; y: number } {
  return get(spatialPos)[gid + "|" + user] ?? arcSpot(Math.max(0, order.indexOf(user)), order.length);
}

function place() {
  if (!ctx) return;
  const order = [...nodes.keys()].sort();
  const t = ctx.currentTime;
  for (const [u, n] of nodes) {
    const p = placeOf(u, order);
    // 1 map unit ≈ 3 m; the map's top is in front of you (−z).
    n.pan.positionX.setTargetAtTime(p.x * 3, t, 0.05);
    n.pan.positionY.setTargetAtTime(0, t, 0.05);
    n.pan.positionZ.setTargetAtTime(p.y * 3, t, 0.05);
  }
}

/** Route a remote voice through the panner (when spatial sound is on). */
export function spatialRoute(user: string, stream: MediaStream, call: string) {
  gid = call;
  if (!get(spatial).on || !stream.getAudioTracks().length) return;
  const old = nodes.get(user);
  if (old?.stream === stream) return;
  old?.src.disconnect();
  try {
    const c = graph();
    void c.resume().catch(() => {});
    const src = c.createMediaStreamSource(stream);
    const pan = new PannerNode(c, { panningModel: "HRTF", distanceModel: "inverse", refDistance: 1, maxDistance: 20, rolloffFactor: 0.8 });
    src.connect(pan).connect(master!);
    nodes.set(user, { src, pan, stream });
    place();
  } catch (e) { console.warn("spatial", e); }
}

export function spatialDrop(user: string) {
  const n = nodes.get(user);
  if (!n) return;
  n.src.disconnect();
  n.pan.disconnect();
  nodes.delete(user);
  place();
}

export function spatialReset() {
  deaf = false;
  for (const u of [...nodes.keys()]) spatialDrop(u);
  if (ctx) { void ctx.close().catch(() => {}); ctx = null; master = dry = wet = null; verb = null; }
}

export function spatialDeafen(on: boolean) {
  deaf = on;
  if (master && ctx) master.gain.setTargetAtTime(on ? 0 : 1, ctx.currentTime, 0.03);
}

/** Move someone on the room map. */
export function movePeer(user: string, x: number, y: number) {
  const c = (v: number) => Math.max(-1, Math.min(1, v));
  spatialPos.update((m) => ({ ...m, [gid + "|" + user]: { x: c(x), y: c(y) } }));
  place();
}
export function resetPlaces() {
  spatialPos.update((m) => Object.fromEntries(Object.entries(m).filter(([k]) => !k.startsWith(gid + "|"))));
  place();
}

/** Turn on/off with the streams currently in the call. */
export function setSpatial(on: boolean, streams: Record<string, MediaStream | null>, call: string) {
  spatial.update((c) => ({ ...c, on }));
  if (!on) { spatialReset(); return; }
  for (const [u, s] of Object.entries(streams)) if (s) spatialRoute(u, s, call);
}
export function setRoomKind(r: RoomKind) {
  spatial.update((c) => ({ ...c, room: r }));
  setRoom(r);
}
export const currentRoom = () => room;
