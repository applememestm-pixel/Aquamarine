/**
 * Group / supergroup / channel voice & video chats (Telegram "voice chat"
 * + Discord voice channel). Full mesh WebRTC: every participant keeps one
 * peer connection to every other — great up to ~8 people, no media server.
 *
 * Signalling rides the group's E2EE fan-out:
 *   broadcast  [gcall:v1]{t:"join"|"leave"|"state", call, ...}
 *   targeted   [gcall:v1]{t:"here"|"offer"|"answer"|"ice", call, ...}
 * The participant with the lexically smaller username makes the offer, so
 * two people never offer to each other at once.
 *
 * Channels: only admins send audio/video; subscribers join as listeners.
 */
import { writable, get } from "svelte/store";
import { api, events } from "./tauri";
import { info } from "./stores";
import { groupMap } from "./groups";
import { isAdmin } from "./groupRules";
import { callSettings, iceServersFor } from "./callSettings";
import { showToast } from "./components/Toast.svelte";
import { tr } from "./i18n";

export interface Peer {
  user: string;
  stream: MediaStream | null;
  muted: boolean;
  video: boolean;
  speaking: boolean;
  state: string;
}

export interface GroupCallState {
  gid: string;
  callId: string;
  /** Joined (media flowing) vs. just "a call is going on here". */
  joined: boolean;
  listener: boolean;
  muted: boolean;
  deafened: boolean;
  camera: boolean;
  screen: boolean;
  peers: Record<string, Peer>;
  local: MediaStream | null;
  started: number;
  meSpeaking: boolean;
}

export const groupCall = writable<GroupCallState | null>(null);
/** gid → people currently in that group's call (from join/leave/state). */
export const activeGroupCalls = writable<Record<string, { callId: string; users: string[]; since: number }>>({});

const pcs = new Map<string, RTCPeerConnection>();
const pendingIce = new Map<string, RTCIceCandidateInit[]>();
let analysers = new Map<string, { an: AnalyserNode; buf: Uint8Array<ArrayBuffer> }>();
let audioCtx: AudioContext | null = null;
let meterTimer: ReturnType<typeof setInterval> | null = null;
let heartbeat: ReturnType<typeof setInterval> | null = null;

const me = () => get(info)?.username ?? "";
const sig = (o: Record<string, unknown>) => "[gcall:v1]" + JSON.stringify(o);

function patch(fn: (s: GroupCallState) => void) {
  groupCall.update((s) => { if (!s) return s; const n = { ...s, peers: { ...s.peers } }; fn(n); return n; });
}
function setPeer(user: string, p: Partial<Peer>) {
  patch((s) => { s.peers[user] = { ...(s.peers[user] ?? { user, stream: null, muted: false, video: false, speaking: false, state: "new" }), ...p }; });
}
function noteUsers(gid: string, callId: string, add?: string, remove?: string) {
  activeGroupCalls.update((m) => {
    const cur = m[gid] && m[gid].callId === callId ? m[gid] : { callId, users: [], since: Date.now() };
    let users = cur.users;
    if (add && !users.includes(add)) users = [...users, add];
    if (remove) users = users.filter((u) => u !== remove);
    const next = { ...m };
    if (users.length) next[gid] = { ...cur, users }; else delete next[gid];
    return next;
  });
}

async function sendTo(gid: string, to: string, o: Record<string, unknown>) {
  try { await api.groupControlTo(gid, to, sig(o)); } catch (e) { console.warn("gcall send", e); }
}
async function broadcast(gid: string, o: Record<string, unknown>) {
  try { await api.groupControl(gid, sig(o)); } catch (e) { console.warn("gcall bcast", e); }
}

const videoSenders = new Map<string, RTCRtpSender>();
const remoteStreams = new Map<string, MediaStream>();

/** Attach our audio / video to a connection's transceivers (both sides use
 *  sendrecv m-lines, so turning the camera on later is just replaceTrack). */
function attachLocal(user: string, pc: RTCPeerConnection) {
  const s = get(groupCall);
  const a = s?.local?.getAudioTracks()[0] ?? null;
  const v = s?.local?.getVideoTracks()[0] ?? null;
  for (const t of pc.getTransceivers()) {
    const kind = t.receiver.track.kind;
    t.direction = "sendrecv";
    if (kind === "audio") void t.sender.replaceTrack(a);
    if (kind === "video") { void t.sender.replaceTrack(v); videoSenders.set(user, t.sender); }
  }
}

function makePc(gid: string, callId: string, user: string, offerer: boolean): RTCPeerConnection {
  pcs.get(user)?.close();
  const pc = new RTCPeerConnection({ iceServers: iceServersFor(get(callSettings)) });
  pcs.set(user, pc);
  if (offerer) {
    pc.addTransceiver("audio", { direction: "sendrecv" });
    pc.addTransceiver("video", { direction: "sendrecv" });
    attachLocal(user, pc);
  }
  pc.onicecandidate = (e) => { if (e.candidate) void sendTo(gid, user, { t: "ice", call: callId, cand: e.candidate.toJSON() }); };
  pc.ontrack = (e) => {
    let stream = remoteStreams.get(user);
    if (!stream) { stream = new MediaStream(); remoteStreams.set(user, stream); }
    stream.addTrack(e.track);
    const refresh = () => setPeer(user, { stream: stream!, video: stream!.getVideoTracks().some((t) => !t.muted && t.readyState === "live") });
    e.track.onmute = refresh;
    e.track.onunmute = refresh;
    refresh();
    if (e.track.kind === "audio") watchLevel(user, stream);
  };
  pc.onconnectionstatechange = () => {
    setPeer(user, { state: pc.connectionState });
    if (pc.connectionState === "failed" && offerer) void offer(gid, callId, user, pc, true);
  };
  setPeer(user, { state: "connecting" });
  if (offerer) void offer(gid, callId, user, pc, false);
  return pc;
}

async function offer(gid: string, callId: string, user: string, pc: RTCPeerConnection, restart: boolean) {
  try {
    await pc.setLocalDescription(await pc.createOffer(restart ? { iceRestart: true } : undefined));
    await sendTo(gid, user, { t: "offer", call: callId, sdp: pc.localDescription?.sdp });
  } catch (e) { console.warn("offer", e); }
}

function watchLevel(user: string, stream: MediaStream) {
  try {
    audioCtx ??= new AudioContext();
    const src = audioCtx.createMediaStreamSource(stream);
    const an = audioCtx.createAnalyser();
    an.fftSize = 512;
    src.connect(an);
    analysers.set(user, { an, buf: new Uint8Array(new ArrayBuffer(an.fftSize)) });
  } catch { /* no meter */ }
  meterTimer ??= setInterval(() => {
    for (const [u, { an, buf }] of analysers) {
      an.getByteTimeDomainData(buf);
      let peak = 0;
      for (const v of buf) peak = Math.max(peak, Math.abs(v - 128));
      const speaking = peak > 10;
      const cur = get(groupCall)?.peers[u]?.speaking ?? (u === me() ? undefined : false);
      if (u === me()) { const v = speaking && !get(groupCall)?.muted; if (get(groupCall)?.meSpeaking !== v) patch((s) => { s.meSpeaking = v; }); continue; }
      if (cur !== speaking) setPeer(u, { speaking });
    }
  }, 150);
}

async function getMedia(video: boolean): Promise<MediaStream | null> {
  const cs = get(callSettings) as unknown as Record<string, unknown>;
  try {
    return await navigator.mediaDevices.getUserMedia({
      audio: {
        deviceId: typeof cs.inputDevice === "string" && cs.inputDevice ? { exact: cs.inputDevice } : undefined,
        echoCancellation: true, noiseSuppression: true, autoGainControl: true,
      },
      video: video ? { width: { ideal: 1280 }, height: { ideal: 720 }, frameRate: { ideal: 30 } } : false,
    });
  } catch (e) {
    showToast(tr("Microphone unavailable: {e}", { e: String(e) }));
    return null;
  }
}

/** Start (or join) the call in group `gid`. */
export async function joinGroupCall(gid: string, opts: { video?: boolean } = {}) {
  const cur = get(groupCall);
  if (cur?.joined) { if (cur.gid === gid) return; await leaveGroupCall(); }
  const g = get(groupMap)[gid];
  if (!g) return;
  const listener = g.kind === "channel" && !isAdmin(g, me());
  const local = listener ? null : await getMedia(!!opts.video);
  if (!listener && !local) return;
  const callId = get(activeGroupCalls)[gid]?.callId ?? `gc-${Date.now().toString(36)}`;
  groupCall.set({ gid, callId, joined: true, listener, muted: false, deafened: false, camera: !!opts.video, screen: false, peers: {}, local, started: Date.now(), meSpeaking: false });
  if (local) watchLevel(me(), local);
  noteUsers(gid, callId, me());
  await broadcast(gid, { t: "join", call: callId, listener });
  heartbeat = setInterval(() => {
    const s = get(groupCall);
    if (s?.joined) void broadcast(s.gid, { t: "state", call: s.callId, users: [me(), ...Object.keys(s.peers)], muted: s.muted });
  }, 25_000);
}

export async function leaveGroupCall() {
  const s = get(groupCall);
  if (!s) return;
  void broadcast(s.gid, { t: "leave", call: s.callId });
  for (const pc of pcs.values()) pc.close();
  pcs.clear(); pendingIce.clear(); analysers.clear(); videoSenders.clear(); remoteStreams.clear();
  s.local?.getTracks().forEach((t) => t.stop());
  if (heartbeat) { clearInterval(heartbeat); heartbeat = null; }
  if (meterTimer) { clearInterval(meterTimer); meterTimer = null; }
  noteUsers(s.gid, s.callId, undefined, me());
  groupCall.set(null);
}

export function toggleGroupMute() {
  const s = get(groupCall);
  if (!s?.local) return;
  const muted = !s.muted;
  s.local.getAudioTracks().forEach((t) => (t.enabled = !muted));
  patch((n) => { n.muted = muted; });
  void broadcast(s.gid, { t: "state", call: s.callId, muted });
}
export function toggleGroupDeafen() { patch((n) => { n.deafened = !n.deafened; }); }

async function replaceVideo(track: MediaStreamTrack | null) {
  const s = get(groupCall);
  if (!s?.local) return;
  for (const old of s.local.getVideoTracks()) { s.local.removeTrack(old); old.stop(); }
  if (track) s.local.addTrack(track);
  for (const snd of videoSenders.values()) await snd.replaceTrack(track).catch(() => {});
}

export async function toggleGroupCamera() {
  const s = get(groupCall);
  if (!s || s.listener) return;
  if (s.camera || s.screen) { await replaceVideo(null); patch((n) => { n.camera = false; n.screen = false; }); return; }
  try {
    const v = await navigator.mediaDevices.getUserMedia({ video: { width: { ideal: 1280 }, height: { ideal: 720 }, frameRate: { ideal: 30 } } });
    await replaceVideo(v.getVideoTracks()[0]);
    patch((n) => { n.camera = true; });
  } catch (e) { showToast(tr("Camera unavailable: {e}", { e: String(e) })); }
}

export async function toggleGroupScreen() {
  const s = get(groupCall);
  if (!s || s.listener) return;
  if (s.screen) { await replaceVideo(null); patch((n) => { n.screen = false; }); return; }
  try {
    const d = await navigator.mediaDevices.getDisplayMedia({ video: { frameRate: { ideal: 30 }, width: { ideal: 1920 } }, audio: false });
    const tr0 = d.getVideoTracks()[0];
    tr0.onended = () => { if (get(groupCall)?.screen) void toggleGroupScreen(); };
    await replaceVideo(tr0);
    patch((n) => { n.screen = true; n.camera = false; });
  } catch { /* cancelled */ }
}

/** In a channel stream, listeners only connect to speakers (admins). */
function needLink(gid: string, other: string): boolean {
  const g = get(groupMap)[gid];
  if (!g || g.kind !== "channel") return true;
  return isAdmin(g, me()) || isAdmin(g, other);
}

async function onSignal(from: string, gid: string, m: Record<string, unknown>) {
  const callId = String(m.call ?? "");
  const t = String(m.t ?? "");
  if (t === "join") { noteUsers(gid, callId, from); }
  if (t === "leave") { noteUsers(gid, callId, undefined, from); }
  if (t === "state" && Array.isArray(m.users)) for (const u of m.users) noteUsers(gid, callId, String(u));
  const s = get(groupCall);
  if (!s?.joined || s.gid !== gid) return;
  // Calls merge: whoever had the older id wins (rare double-start race).
  if (callId && callId !== s.callId && (t === "join" || t === "here")) {
    if (callId < s.callId) patch((n) => { n.callId = callId; });
  }
  switch (t) {
    case "join":
      if (!needLink(gid, from)) break;
      // Tell the newcomer we're here; the smaller name will offer.
      void sendTo(gid, from, { t: "here", call: get(groupCall)!.callId });
      if (me() < from) makePc(gid, s.callId, from, true);
      else setPeer(from, { state: "connecting" });
      break;
    case "here":
      noteUsers(gid, callId, from);
      if (!needLink(gid, from)) break;
      if (!pcs.has(from) && me() < from) makePc(gid, s.callId, from, true);
      else if (!pcs.has(from)) setPeer(from, { state: "connecting" });
      break;
    case "offer": {
      const existing = pcs.get(from);
      const pc = existing && existing.remoteDescription ? existing : makePc(gid, s.callId, from, false);
      await pc.setRemoteDescription({ type: "offer", sdp: String(m.sdp ?? "") });
      attachLocal(from, pc);
      for (const c of pendingIce.get(from) ?? []) await pc.addIceCandidate(c).catch(() => {});
      pendingIce.delete(from);
      await pc.setLocalDescription(await pc.createAnswer());
      await sendTo(gid, from, { t: "answer", call: s.callId, sdp: pc.localDescription?.sdp });
      break;
    }
    case "answer": {
      const pc = pcs.get(from);
      if (pc && pc.signalingState === "have-local-offer") {
        await pc.setRemoteDescription({ type: "answer", sdp: String(m.sdp ?? "") });
        for (const c of pendingIce.get(from) ?? []) await pc.addIceCandidate(c).catch(() => {});
        pendingIce.delete(from);
      }
      break;
    }
    case "ice": {
      const pc = pcs.get(from);
      const cand = m.cand as RTCIceCandidateInit;
      if (pc?.remoteDescription) await pc.addIceCandidate(cand).catch(() => {});
      else pendingIce.set(from, [...(pendingIce.get(from) ?? []), cand]);
      break;
    }
    case "state":
      if (typeof m.muted === "boolean" && s.peers[from]) setPeer(from, { muted: m.muted });
      break;
    case "leave":
      pcs.get(from)?.close(); pcs.delete(from); analysers.delete(from); videoSenders.delete(from); remoteStreams.delete(from);
      patch((n) => { delete n.peers[from]; });
      break;
  }
}

let wired = false;
export function initGroupCalls() {
  if (wired) return;
  wired = true;
  void events.onGroupCall((p) => { void onSignal(p.from, p.gid, p.signal).catch((e) => console.warn("gcall", e)); });
  window.addEventListener("beforeunload", () => { void leaveGroupCall(); });
}
