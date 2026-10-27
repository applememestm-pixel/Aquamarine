<script module lang="ts">
  import { writable, get as getStore } from "svelte/store";
  import { logCall } from "../callHistory";
  import { peerPresence } from "../presence";
  import { showToast as toastM } from "./Toast.svelte";

  type CallState =
    | { kind: "idle" }
    | { kind: "outgoing"; peer: string; started: number }
    | { kind: "incoming"; peer: string; started: number }
    | { kind: "connected"; peer: string; started: number; direction: "in" | "out"; placed_at: number }
    | { kind: "ended"; peer: string; reason: string };

  export const callState = writable<CallState>({ kind: "idle" });
  /** Self-mute / self-deafen. Live outside calls too (Discord-style), so the
   *  user panel can set them before a call starts. */
  export const callMuted = writable<boolean>(false);
  export const callDeafened = writable<boolean>(false);

  export function toggleCallMute() {
    callMuted.update((m) => !m);
    // Un-muting while deafened also un-deafens (same as Discord).
    if (!getStore(callMuted) && getStore(callDeafened)) callDeafened.set(false);
  }
  export function toggleCallDeafen() {
    callDeafened.update((d) => !d);
  }

  /** The call was placed as a video call: turn the camera on once connected. */
  let camOnConnect = false;
  export function takeCamOnConnect() { const v = camOnConnect; camOnConnect = false; return v; }
  export function startOutgoingCall(peer: string, video = false) {
    camOnConnect = video;
    if (getStore(peerPresence)[peer]?.nc) {
      toastM(`@${peer} doesn't accept calls from you`);
      return;
    }
    callState.set({ kind: "outgoing", peer, started: Date.now() });
  }

  /**
   * End the current call. `reason` describes what happened —
   *   "local-hangup" | "remote-hangup" | "rejected-local" | "remote-reject" |
   *   "local-error" | "connection-lost" | "no-answer" | …
   * Also appends a record to the Calls tab history.
   */
  export function endCall(reason: string) {
    const s = getStore(callState);
    if (s.kind === "idle" || s.kind === "ended") return;
    const peer = (s as any).peer as string;

    let direction: "in" | "out" = "out";
    let placedAt = (s as any).started as number;
    let durationSeconds = 0;
    let status: "completed" | "missed" | "rejected" | "failed";

    if (s.kind === "connected") {
      direction = s.direction;
      placedAt = s.placed_at;
      durationSeconds = Math.max(0, Math.floor((Date.now() - s.started) / 1000));
      status = "completed";
    } else if (s.kind === "outgoing") {
      direction = "out";
      status = reason === "remote-reject" ? "rejected" : "missed";
    } else {
      direction = "in";
      status = reason === "rejected-local" ? "rejected" : "missed";
    }
    if (reason === "local-error") status = "failed";

    logCall({ peer_id: peer, direction, status, started_at: placedAt, duration_seconds: durationSeconds });

    const ended: CallState = { kind: "ended", peer, reason };
    callState.set(ended);
    // Only fall back to idle if nothing new (e.g. an incoming call) replaced us.
    setTimeout(() => { if (getStore(callState) === ended) callState.set({ kind: "idle" }); }, 1500);
  }
</script>

<script lang="ts">
  import Select from "./Select.svelte";
  import { t, tr } from "../i18n";
  import { avatars } from "../people";
  import { onMount } from "svelte";
  import { get } from "svelte/store";
  import type { Action } from "svelte/action";
  import { api, events } from "../tauri";
  import { info } from "../stores";
  import { showToast } from "./Toast.svelte";
  import { allowed } from "../privacy";
  import { openSettings } from "../panels/settings";
  import { onBack } from "../back";
  import { callSettings, iceServersFor, patchCallSettings, keyLabel } from "../callSettings";
  import {
    tuneSdp, orderCodecs, tuneSendParameters, displayMediaOptions, screenVideoConstraints,
    micConstraints, cameraConstraints, targetVideoKbps, kbpsForFrame, qualityLabel, RES_LABEL,
    SDP_VIDEO_MAX_KBPS, SDP_VIDEO_START_KBPS, SDP_VIDEO_MIN_KBPS,
    type ResPreset, type FpsPreset, type ContentHint, type SdpTuning, type SenderTuning, captureScreenOrCamera, canCaptureScreen } from "../rtcQuality";

  // ---------------------------------------------------------------------------
  // Media layout. Every call negotiates the same four m-lines up front, so
  // turning camera / stream / stream-audio on and off is just replaceTrack —
  // no renegotiation, no glare, and camera + screen can run together.
  // ---------------------------------------------------------------------------
  const T_MIC = 0, T_CAM = 1, T_SCR = 2, T_SCRA = 3;

  let pc: RTCPeerConnection | null = null;
  let tx: RTCRtpTransceiver[] = [];
  let pendingIce: { from: string; c: RTCIceCandidateInit }[] = [];
  let peerId = $state("");
  let polite = false;
  let makingOffer = false;
  let signalChain: Promise<void> = Promise.resolve();

  // Local media
  let micRaw: MediaStream | null = null;
  let micTrack: MediaStreamTrack | null = null;   // what we actually send (post gain/gate)
  let inCtx: AudioContext | null = null;
  let inGain: GainNode | null = null;
  let gateGain: GainNode | null = null;
  let analyser: AnalyserNode | null = null;
  let levelBuf: Float32Array | null = null;
  let camStream = $state.raw<MediaStream | null>(null);
  /** Phones: which camera — the front one, or the back one to show what's
   *  in front of you (phones can't share their screen). */
  let facing = $state<"user" | "environment">("user");
  const touchUi = typeof matchMedia === "function" && matchMedia("(pointer: coarse)").matches;
  const screenShareOk = canCaptureScreen();
  let screenStream = $state.raw<MediaStream | null>(null);

  // Remote media
  let rMic: MediaStream | null = null;
  let rScreenAudio: MediaStream | null = null;
  let rCam = $state.raw<MediaStream | null>(null);
  let rScreen = $state.raw<MediaStream | null>(null);
  let remoteAudio: HTMLAudioElement | undefined = $state();
  let remoteStreamAudio: HTMLAudioElement | undefined = $state();

  // UI state
  type RemoteMedia = { muted: boolean; deaf: boolean; cam: boolean; screen: boolean; screenAudio: boolean; label: string };
  const NO_REMOTE: RemoteMedia = { muted: false, deaf: false, cam: false, screen: false, screenAudio: false, label: "" };
  let remote = $state<RemoteMedia>({ ...NO_REMOTE });
  let deafened = $state(false);
  let pttHeld = $state(false);
  let speakingMe = $state(false);
  let speakingPeer = $state(false);
  let reconnecting = $state(false);
  let expanded = $state(true);
  let fullscreen = $state(false);
  type TileId = "r-screen" | "r-cam" | "l-screen" | "l-cam" | "r-ava";
  let focus = $state<TileId | null>(null);
  let goLiveOpen = $state(false);
  let volOpen = $state(false);
  // In-call devices panel (the ⚙ button): pick the mic / speaker and levels
  // here instead of being thrown out of the call into Settings.
  let devOpen = $state(false);
  let devices = $state<{ mics: MediaDeviceInfo[]; speakers: MediaDeviceInfo[] }>({ mics: [], speakers: [] });
  async function openDevices() {
    goLiveOpen = false; volOpen = false;
    devOpen = !devOpen;
    if (!devOpen) return;
    try {
      const all = await navigator.mediaDevices.enumerateDevices();
      devices = { mics: all.filter((d) => d.kind === "audioinput"), speakers: all.filter((d) => d.kind === "audiooutput") };
    } catch { devices = { mics: [], speakers: [] }; }
  }
  async function switchMic(id: string) {
    callSettings.update((c) => ({ ...c, micId: id }));
    try { micRaw?.getTracks().forEach((t) => t.stop()); } catch { /* gone */ }
    try { await inCtx?.close(); } catch { /* closed */ }
    inCtx = null;
    await startMic();
  }
  const canPickSpeaker = typeof HTMLMediaElement !== "undefined" && "setSinkId" in HTMLMediaElement.prototype;
  let statsOpen = $state(getStore(callSettings).showStats);
  let elapsedSeconds = $state(0);
  let liveCfg = $state<{ res: ResPreset; fps: FpsPreset; hint: ContentHint; audio: boolean } | null>(null);
  let gl = $state({ res: "1080" as ResPreset, fps: 60 as FpsPreset, hint: "motion" as ContentHint, audio: true });

  interface VStat { w: number; h: number; fps: number; kbps: number; codec: string; impl: string; limit: string; loss: number }
  interface StatsView {
    upScreen?: VStat; upCam?: VStat; downScreen?: VStat; downCam?: VStat;
    rttMs?: number; bweKbps?: number; route?: string; voiceLoss?: number; jitterMs?: number;
    voiceInKbps?: number; voiceOutKbps?: number;
  }
  let stats = $state<StatsView | null>(null);
  let netBars = $state(3);

  let timer: ReturnType<typeof setInterval> | null = null;
  let levelTimer: ReturnType<typeof setInterval> | null = null;
  let statsTimer: ReturnType<typeof setInterval> | null = null;
  let restartTimer: ReturnType<typeof setTimeout> | null = null;
  let giveUpTimer: ReturnType<typeof setTimeout> | null = null;
  let ringTimeout: ReturnType<typeof setTimeout> | null = null;
  let gateOpenUntil = 0;
  let meHoldUntil = 0;
  let peerHoldUntil = 0;
  const prevBytes = new Map<string, { b: number; t: number }>();

  // ---------------------------------------------------------------------------
  // Lifecycle
  // ---------------------------------------------------------------------------
  onMount(() => {
    const unsubSig = events.onCallSignal((e) => {
      const { from, signal } = e;
      signalChain = signalChain.then(() => onSignal(from, signal as any)).catch((err) => console.error("call signal", err));
    });
    const unsubSettings = callSettings.subscribe(() => { if (pc) { applyInput(); void applyOutput(); } });
    // Mute / deafen can be flipped from the user panel as well as here.
    let firstM = true, firstD = true;
    const unsubMuted = callMuted.subscribe(() => {
      if (firstM) { firstM = false; return; }
      if (pc) { applyInput(); sendMedia(); }
    });
    const unsubDeaf = callDeafened.subscribe((d) => {
      deafened = d;
      if (firstD) { firstD = false; return; }
      if (pc) { applyInput(); void applyOutput(); sendMedia(); }
    });

    const onKeyDown = (e: KeyboardEvent) => {
      const s = get(callState);
      if (s.kind !== "connected" && s.kind !== "outgoing") return;
      const cs = get(callSettings);
      if (cs.pushToTalk && e.code === cs.pttKey && !isTyping(e)) {
        e.preventDefault();
        if (!pttHeld) { pttHeld = true; applyInput(); }
        return;
      }
      // Ctrl+Shift+M / D are global (lib/shortcuts.ts).
      if (e.key === "Escape") {
        if (goLiveOpen || volOpen || devOpen) { goLiveOpen = false; volOpen = false; devOpen = false; }
        else if (fullscreen) void toggleFullscreen();
      }
    };
    const onKeyUp = (e: KeyboardEvent) => {
      const cs = get(callSettings);
      if (cs.pushToTalk && e.code === cs.pttKey && pttHeld) { pttHeld = false; applyInput(); }
    };
    const onMouse = (down: boolean) => (e: MouseEvent) => {
      const cs = get(callSettings);
      const code = e.button === 3 ? "Mouse3" : e.button === 4 ? "Mouse4" : "";
      if (!cs.pushToTalk || !code || code !== cs.pttKey || !pc) return;
      e.preventDefault();
      pttHeld = down; applyInput();
    };
    const md = onMouse(true), mu = onMouse(false);
    const onBlur = () => { if (pttHeld) { pttHeld = false; applyInput(); } };
    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("keyup", onKeyUp, true);
    window.addEventListener("mousedown", md, true);
    window.addEventListener("mouseup", mu, true);
    window.addEventListener("blur", onBlur);
    return () => {
      unsubSig.then((u) => u());
      unsubSettings();
      unsubMuted();
      unsubDeaf();
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("keyup", onKeyUp, true);
      window.removeEventListener("mousedown", md, true);
      window.removeEventListener("mouseup", mu, true);
      window.removeEventListener("blur", onBlur);
    };
  });

  function isTyping(e: KeyboardEvent): boolean {
    // Letter keys used as PTT shouldn't hijack typing in the composer.
    const t = e.target as HTMLElement | null;
    const typing = !!t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable);
    return typing && /^(Key|Digit)/.test(e.code);
  }

  // Elapsed counter + ring timeout.
  $effect(() => {
    const s = $callState;
    if (timer) { clearInterval(timer); timer = null; }
    if (ringTimeout) { clearTimeout(ringTimeout); ringTimeout = null; }
    if (s.kind === "outgoing" || s.kind === "incoming" || s.kind === "connected") {
      elapsedSeconds = Math.floor((Date.now() - s.started) / 1000);
      timer = setInterval(() => { elapsedSeconds = Math.floor((Date.now() - s.started) / 1000); }, 1000);
    } else {
      elapsedSeconds = 0;
    }
    if (s.kind === "outgoing") {
      ringTimeout = setTimeout(() => { showToast(tr("No answer")); void hangup("no-answer"); }, 45_000);
    } else if (s.kind === "incoming") {
      ringTimeout = setTimeout(() => teardown("missed"), 45_000);
    }
  });

  $effect(() => {
    const s = $callState;
    if (s.kind === "outgoing" && !pc) void placeCall(s.peer);
  });

  // ---------------------------------------------------------------------------
  // Signalling
  // ---------------------------------------------------------------------------
  function sdpTuning(): SdpTuning {
    const cs = get(callSettings);
    const max = cs.maxVideoKbps > 0 ? Math.max(1000, cs.maxVideoKbps) : SDP_VIDEO_MAX_KBPS;
    return {
      voiceKbps: cs.voiceKbps,
      musicKbps: 192,
      videoMaxKbps: max,
      videoStartKbps: Math.min(SDP_VIDEO_START_KBPS, max),
      videoMinKbps: SDP_VIDEO_MIN_KBPS,
    };
  }

  async function sendSig(sig: object) {
    if (!peerId) return;
    try { await api.sendCallSignal(peerId, sig); }
    catch (e) { console.warn("sendCallSignal", e); }
  }

  async function placeCall(peer: string) {
    try {
      peerId = peer;
      polite = false;
      setupPeer(true);
      startRinging("out");
      await startMic();
      const offer = await pc!.createOffer();
      await pc!.setLocalDescription(offer);
      await api.sendCallSignal(peer, { type: "offer", sdp: tuneSdp(offer.sdp!, sdpTuning()) });
    } catch (e) {
      showToast(`Call failed: ${e}`);
      await hangup("local-error");
    }
  }

  async function onSignal(from: string, sig: any) {
    const s = get(callState);
    const active = s.kind === "outgoing" || s.kind === "incoming" || s.kind === "connected";

    if (sig.type === "offer") {
      if (pc && active && from === peerId) {
        // Renegotiation (ICE restart, or an older client adding a track).
        const collision = makingOffer || pc.signalingState !== "stable";
        if (collision && !polite) return;
        if (collision) { try { await pc.setLocalDescription({ type: "rollback" } as RTCLocalSessionDescriptionInit); } catch { /* ignore */ } }
        await pc.setRemoteDescription({ type: "offer", sdp: sig.sdp });
        await drainPendingIce();
        const ans = await pc.createAnswer();
        await pc.setLocalDescription(ans);
        await sendSig({ type: "answer", sdp: tuneSdp(ans.sdp!, sdpTuning()) });
        return;
      }
      if (active) {
        await api.sendCallSignal(from, { type: "reject", reason: "busy" }).catch(() => {});
        return;
      }
      if (!allowed("calls", from)) {
        // Privacy → Calls: decline without ringing.
        await api.sendCallSignal(from, { type: "reject", reason: "privacy" }).catch(() => {});
        return;
      }
      peerId = from;
      polite = true;
      callState.set({ kind: "incoming", peer: from, started: Date.now() });
      setupPeer(false);
      await pc!.setRemoteDescription({ type: "offer", sdp: sig.sdp });
      tx = pc!.getTransceivers();
      for (const t of tx) { try { t.direction = "sendrecv"; } catch { /* ignore */ } }
      applyCodecPrefs();
      attachRemote();
      await drainPendingIce();
      startRinging("in");
      return;
    }

    if (sig.type === "ice") {
      if (!sig.candidate) return;
      if (pc && from === peerId && pc.remoteDescription) {
        await pc.addIceCandidate(sig.candidate).catch((e) => console.warn("addIceCandidate", e));
      } else {
        pendingIce.push({ from, c: sig.candidate });
      }
      return;
    }

    if (from !== peerId || !active) return;

    switch (sig.type) {
      case "answer": {
        if (!pc || pc.signalingState !== "have-local-offer") return;
        await pc.setRemoteDescription({ type: "answer", sdp: sig.sdp });
        await drainPendingIce();
        if (s.kind === "outgoing") {
          callState.set({ kind: "connected", peer: from, started: Date.now(), direction: "out", placed_at: s.started });
          onConnected();
        }
        break;
      }
      case "reject":
        showToast(sig.reason === "busy" ? `${from} is in another call` : sig.reason === "privacy" ? `@${from} doesn't accept calls from you` : "Call declined");
        teardown("remote-reject");
        break;
      case "hangup":
        teardown("remote-hangup");
        break;
      case "media":
        remote = {
          muted: !!sig.muted, deaf: !!sig.deaf, cam: !!sig.cam, screen: !!sig.screen,
          screenAudio: !!sig.screenAudio, label: typeof sig.label === "string" ? sig.label : "",
        };
        break;
      case "restart":
        if (!polite) iceRestart();
        break;
    }
  }

  function setupPeer(asCaller: boolean) {
    pc = new RTCPeerConnection({
      iceServers: iceServersFor(get(callSettings)),
      bundlePolicy: "max-bundle",
      rtcpMuxPolicy: "require",
    });
    pendingIce = pendingIce.filter((p) => p.from === peerId);
    if (asCaller) {
      tx = [
        pc.addTransceiver("audio", { direction: "sendrecv" }),
        pc.addTransceiver("video", { direction: "sendrecv" }),
        pc.addTransceiver("video", { direction: "sendrecv" }),
        pc.addTransceiver("audio", { direction: "sendrecv" }),
      ];
      applyCodecPrefs();
      attachRemote();
    }
    pc.onicecandidate = (ev) => {
      if (ev.candidate) void sendSig({ type: "ice", candidate: ev.candidate.toJSON() });
    };
    pc.onconnectionstatechange = onConnState;
    pc.ontrack = (ev) => {
      // Our four transceivers are wired in attachRemote(); this only catches
      // tracks an older client adds by renegotiating.
      if (!tx.length || tx.includes(ev.transceiver)) return;
      if (ev.track.kind === "video") {
        rScreen = ev.streams[0] ?? new MediaStream([ev.track]);
        remote = { ...remote, screen: true };
        ev.track.onended = () => { remote = { ...remote, screen: false }; };
      }
    };
  }

  async function drainPendingIce() {
    if (!pc) return;
    const mine = pendingIce.filter((p) => p.from === peerId);
    pendingIce = pendingIce.filter((p) => p.from !== peerId);
    for (const p of mine) await pc.addIceCandidate(p.c).catch(() => {});
  }

  function applyCodecPrefs() {
    const recv = typeof RTCRtpReceiver !== "undefined" ? RTCRtpReceiver.getCapabilities?.("video")?.codecs : undefined;
    if (!recv?.length) return;
    const send = RTCRtpSender.getCapabilities?.("video")?.codecs;
    const ordered = orderCodecs(recv, get(callSettings).codec, send);
    for (const i of [T_CAM, T_SCR]) {
      const t = tx[i];
      if (!t || t.receiver.track.kind !== "video") continue;
      try { t.setCodecPreferences(ordered); } catch (e) { console.warn("setCodecPreferences", e); }
    }
  }

  function attachRemote() {
    const tr = (i: number) => tx[i]?.receiver.track ?? null;
    const m = tr(T_MIC), c = tr(T_CAM), sc = tr(T_SCR), sa = tr(T_SCRA);
    rMic = m ? new MediaStream([m]) : null;
    rCam = c?.kind === "video" ? new MediaStream([c]) : null;
    rScreen = sc?.kind === "video" ? new MediaStream([sc]) : null;
    rScreenAudio = sa?.kind === "audio" ? new MediaStream([sa]) : null;
    if (remoteAudio) { remoteAudio.srcObject = rMic; remoteAudio.play().catch(() => {}); }
    if (remoteStreamAudio) { remoteStreamAudio.srcObject = rScreenAudio; remoteStreamAudio.play().catch(() => {}); }
    void applyOutput();
  }

  // ---------------------------------------------------------------------------
  // Connection health: ICE restart instead of hanging up on a blip
  // ---------------------------------------------------------------------------
  function clearNetTimers() {
    if (restartTimer) { clearTimeout(restartTimer); restartTimer = null; }
    if (giveUpTimer) { clearTimeout(giveUpTimer); giveUpTimer = null; }
  }

  function onConnState() {
    if (!pc) return;
    const st = pc.connectionState;
    if (st === "connected") {
      if (reconnecting) showToast(tr("Reconnected"));
      reconnecting = false;
      clearNetTimers();
      void retuneAll();
    } else if (st === "disconnected" || st === "failed") {
      reconnecting = true;
      if (!restartTimer) {
        restartTimer = setTimeout(() => {
          restartTimer = null;
          if (pc && pc.connectionState !== "connected") iceRestart();
        }, st === "failed" ? 0 : 2500);
      }
      if (!giveUpTimer) {
        giveUpTimer = setTimeout(() => {
          giveUpTimer = null;
          if (pc && pc.connectionState !== "connected") {
            const relay = !!get(callSettings).turnUrl;
            showToast(relay ? "Connection lost" : "Connection lost — if this keeps happening, add a TURN relay in Settings → Voice & Video", 6000);
            void hangup("connection-lost");
          }
        }, 30_000);
      }
    }
  }

  function iceRestart() {
    if (!pc) return;
    if (polite) { void sendSig({ type: "restart" }); return; }
    try { pc.restartIce(); } catch { /* ignore */ }
    void renegotiate();
  }

  async function renegotiate() {
    if (!pc) return;
    try {
      makingOffer = true;
      const o = await pc.createOffer();
      await pc.setLocalDescription(o);
      await sendSig({ type: "offer", sdp: tuneSdp(o.sdp!, sdpTuning()) });
    } catch (e) {
      console.warn("renegotiate", e);
    } finally {
      makingOffer = false;
    }
  }

  // ---------------------------------------------------------------------------
  // Sender tuning
  // ---------------------------------------------------------------------------
  async function tuneSender(sender: RTCRtpSender | undefined, t: SenderTuning) {
    if (!sender || !sender.track) return;
    try {
      await sender.setParameters(tuneSendParameters(sender.getParameters(), t));
    } catch {
      try {
        const p = tuneSendParameters(sender.getParameters(), t) as any;
        delete p.degradationPreference;
        await sender.setParameters(p);
      } catch (e2) { console.warn("setParameters", e2); }
    }
  }

  function screenTuning(): SenderTuning | null {
    const v = screenStream?.getVideoTracks()[0];
    if (!v || !liveCfg) return null;
    const st = v.getSettings();
    const kbps = st.width && st.height
      ? kbpsForFrame(st.width, st.height, liveCfg.fps, get(callSettings).maxVideoKbps)
      : targetVideoKbps(liveCfg.res, liveCfg.fps, get(callSettings).maxVideoKbps);
    // 60 fps asked for: keep the frame rate and lower the resolution when the
    // link or the encoder can't keep up (with "text & detail" it used to drop
    // to ~30 fps instead).
    return { maxKbps: kbps, maxFps: liveCfg.fps, hint: liveCfg.fps >= 50 ? "motion" : liveCfg.hint };
  }

  async function retuneAll() {
    const cs = get(callSettings);
    await tuneSender(tx[T_MIC]?.sender, { maxKbps: cs.voiceKbps, hint: "voice" });
    if (camStream) await tuneSender(tx[T_CAM]?.sender, { maxKbps: targetVideoKbps(cs.camRes, 30, cs.maxVideoKbps), maxFps: 30, hint: "camera" });
    const st = screenTuning();
    if (st) await tuneSender(tx[T_SCR]?.sender, st);
    if (screenStream?.getAudioTracks().length) await tuneSender(tx[T_SCRA]?.sender, { maxKbps: 192, hint: "music" });
  }

  // ---------------------------------------------------------------------------
  // Microphone: gain → noise gate → send; analyser drives the speaking ring
  // ---------------------------------------------------------------------------
  async function startMic() {
    const cs = get(callSettings);
    try {
      micRaw = await navigator.mediaDevices.getUserMedia({ audio: micConstraints(cs), video: false });
    } catch (e) {
      showToast(`Microphone unavailable (${e}) — you can still listen`, 5000);
      micRaw = null;
      return;
    }
    const raw = micRaw.getAudioTracks()[0];
    try { raw.contentHint = "speech"; } catch { /* ignore */ }
    let out: MediaStreamTrack = raw;
    try {
      inCtx = new AudioContext({ latencyHint: "interactive" });
      const src = inCtx.createMediaStreamSource(micRaw);
      inGain = inCtx.createGain();
      gateGain = inCtx.createGain();
      analyser = inCtx.createAnalyser();
      analyser.fftSize = 1024;
      analyser.smoothingTimeConstant = 0.1;
      levelBuf = new Float32Array(analyser.fftSize);
      const dest = inCtx.createMediaStreamDestination();
      dest.channelCount = 1;
      src.connect(inGain);
      inGain.connect(analyser);
      inGain.connect(gateGain);
      gateGain.connect(dest);
      out = dest.stream.getAudioTracks()[0];
      void inCtx.resume();
    } catch (e) {
      console.warn("mic graph unavailable, sending raw mic", e);
      inCtx = null; inGain = null; gateGain = null; analyser = null;
    }
    micTrack = out;
    applyInput();
    await tx[T_MIC]?.sender.replaceTrack(out);
    if (!levelTimer) levelTimer = setInterval(levelTick, 50);
  }

  function micLive(): boolean {
    const cs = get(callSettings);
    return !get(callMuted) && !deafened && (!cs.pushToTalk || pttHeld);
  }

  function applyInput() {
    const cs = get(callSettings);
    if (inGain) inGain.gain.value = Math.max(0, Math.min(2, cs.inputVolume));
    if (micTrack) micTrack.enabled = micLive();
  }

  function levelTick() {
    const now = performance.now();
    const cs = get(callSettings);
    if (analyser && levelBuf) {
      analyser.getFloatTimeDomainData(levelBuf as Float32Array<ArrayBuffer>);
      let sum = 0;
      for (let i = 0; i < levelBuf.length; i++) sum += levelBuf[i] * levelBuf[i];
      const db = 20 * Math.log10(Math.sqrt(sum / levelBuf.length) + 1e-9);
      const gateOff = cs.gateDb <= -99;
      if (gateOff || db > cs.gateDb) gateOpenUntil = now + 350;
      const open = now < gateOpenUntil;
      if (gateGain && inCtx) gateGain.gain.setTargetAtTime(open ? 1 : 0, inCtx.currentTime, open ? 0.005 : 0.05);
      if (micLive() && open && db > Math.max(-48, cs.gateDb)) meHoldUntil = now + 250;
    }
    const me = now < meHoldUntil && micLive();
    if (me !== speakingMe) speakingMe = me;

    const src = tx[T_MIC]?.receiver.getSynchronizationSources?.()[0];
    if (src && (src.audioLevel ?? 0) > 0.02) peerHoldUntil = now + 250;
    const peer = now < peerHoldUntil && !deafened;
    if (peer !== speakingPeer) speakingPeer = peer;
  }

  // ---------------------------------------------------------------------------
  // Output: per-peer volume, output device, deafen
  // ---------------------------------------------------------------------------
  async function applyOutput() {
    const cs = get(callSettings);
    const pairs: [HTMLAudioElement | undefined, number][] = [
      [remoteAudio, cs.peerVolume[peerId] ?? 1],
      [remoteStreamAudio, cs.streamVolume[peerId] ?? 1],
    ];
    const sink = cs.speakerId && cs.speakerId !== "default" ? cs.speakerId : "";
    for (const [el, v] of pairs) {
      if (!el) continue;
      el.volume = Math.max(0, Math.min(1, v * cs.outputVolume));
      el.muted = deafened;
      const anyEl = el as HTMLAudioElement & { sinkId?: string; setSinkId?: (id: string) => Promise<void> };
      if (anyEl.setSinkId && anyEl.sinkId !== sink) await anyEl.setSinkId(sink).catch(() => {});
    }
  }

  // ---------------------------------------------------------------------------
  // Controls
  // ---------------------------------------------------------------------------
  function sendMedia() {
    void sendSig({
      type: "media",
      muted: get(callMuted) || !micTrack,
      deaf: deafened,
      cam: !!camStream,
      screen: !!screenStream,
      screenAudio: !!screenStream?.getAudioTracks().length,
      label: liveCfg && screenStream ? qualityLabel(liveCfg.res, liveCfg.fps) : "",
    });
  }

  const toggleMute = toggleCallMute;
  const toggleDeafen = toggleCallDeafen;

  $effect(() => {
    const k = $callState.kind;
    if (k === "connected" && pc && !camStream && takeCamOnConnect()) setTimeout(() => void toggleCam(), 400);
    if (k === "ended" || k === "idle") takeCamOnConnect();
  });

  async function toggleCam() {
    if (camStream) return stopCam();
    if (!pc || !tx[T_CAM] || tx[T_CAM].receiver.track.kind !== "video") { showToast(tr("Your contact's app is too old for video — ask them to update")); return; }
    const cs = get(callSettings);
    try {
      const s = await navigator.mediaDevices.getUserMedia({ video: camVideo(cs.camId, cs.camRes), audio: false });
      const v = s.getVideoTracks()[0];
      try { v.contentHint = "motion"; } catch { /* ignore */ }
      await tx[T_CAM].sender.replaceTrack(v);
      camStream = s;
      await tuneSender(tx[T_CAM].sender, { maxKbps: targetVideoKbps(cs.camRes, 30, cs.maxVideoKbps), maxFps: 30, hint: "camera" });
      v.onended = () => void stopCam();
      expanded = true;
      sendMedia();
    } catch (e) {
      showToast(`Could not start camera: ${e}`);
    }
  }

  /** Camera constraints; on phones the chosen side (front / back). */
  function camVideo(camId: string | undefined, res: "720" | "1080"): MediaTrackConstraints {
    const c = cameraConstraints(touchUi ? undefined : camId, res);
    if (touchUi) c.facingMode = { ideal: facing };
    return c;
  }

  /** Front ↔ back camera, without dropping the video. */
  async function flipCam() {
    if (!camStream || !tx[T_CAM]) return;
    const cs = get(callSettings);
    const next = facing === "user" ? "environment" : "user";
    const prev = facing;
    facing = next;
    try {
      // Some phones can't open two cameras at once: let go of the old one first.
      camStream.getVideoTracks().forEach((t) => { t.onended = null; t.stop(); });
      const s = await navigator.mediaDevices.getUserMedia({ video: camVideo(cs.camId, cs.camRes), audio: false });
      const v = s.getVideoTracks()[0];
      try { v.contentHint = "motion"; } catch { /* ignore */ }
      await tx[T_CAM].sender.replaceTrack(v);
      camStream = s;
      v.onended = () => void stopCam();
    } catch (e) {
      facing = prev;
      showToast(tr("Could not switch the camera: {e}", { e: String(e) }));
      await stopCam();
    }
  }

  async function stopCam() {
    try { await tx[T_CAM]?.sender.replaceTrack(null); } catch { /* ignore */ }
    camStream?.getTracks().forEach((t) => t.stop());
    camStream = null;
    sendMedia();
  }

  function openGoLive() {
    volOpen = false;
    if (goLiveOpen) { goLiveOpen = false; return; }
    const cs = get(callSettings);
    gl = liveCfg ? { ...liveCfg } : { res: cs.screenRes, fps: cs.screenFps, hint: cs.screenHint, audio: cs.screenAudio };
    goLiveOpen = true;
  }

  async function goLive() {
    goLiveOpen = false;
    const cfg = { ...gl };
    patchCallSettings({ screenRes: cfg.res, screenFps: cfg.fps, screenHint: cfg.hint, screenAudio: cfg.audio });
    if (screenStream) return changeStreamQuality(cfg);
    if (!pc || !tx[T_SCR] || tx[T_SCR].receiver.track.kind !== "video") { showToast(tr("Your contact's app is too old for screen share — ask them to update")); return; }
    try {
      const { stream: s, camera } = await captureScreenOrCamera(displayMediaOptions(cfg.res, cfg.fps, cfg.audio), cfg.fps);
      if (camera) showToast(tr("Phones can't share the screen — streaming your camera instead"), 4000);
      const v = s.getVideoTracks()[0];
      try { v.contentHint = cfg.hint === "detail" && cfg.fps < 50 ? "detail" : "motion"; } catch { /* ignore */ }
      await tx[T_SCR].sender.replaceTrack(v);
      const a = s.getAudioTracks()[0];
      if (a && tx[T_SCRA]) {
        try { a.contentHint = "music"; } catch { /* ignore */ }
        await tx[T_SCRA].sender.replaceTrack(a);
      } else if (cfg.audio && !camera) {
        showToast(tr("No system audio — on Windows it's only available when sharing a whole screen"), 5000);
      }
      v.onended = () => void stopScreen();
      screenStream = s;
      liveCfg = cfg;
      focus = "l-screen";
      expanded = true;
      await retuneAll();
      sendMedia();
    } catch (e) {
      if ((e as DOMException)?.name !== "NotAllowedError") showToast(`Could not start stream: ${e}`);
    }
  }

  async function changeStreamQuality(cfg: typeof gl) {
    const v = screenStream?.getVideoTracks()[0];
    if (!v) return;
    try { await v.applyConstraints(screenVideoConstraints(cfg.res, cfg.fps)); } catch (e) { console.warn("applyConstraints", e); }
    try { v.contentHint = cfg.hint === "detail" && cfg.fps < 50 ? "detail" : "motion"; } catch { /* ignore */ }
    liveCfg = { ...cfg, audio: !!screenStream?.getAudioTracks().length };
    await retuneAll();
    sendMedia();
    showToast(`Stream: ${qualityLabel(cfg.res, cfg.fps)}`);
  }

  async function stopScreen() {
    goLiveOpen = false;
    try { await tx[T_SCR]?.sender.replaceTrack(null); } catch { /* ignore */ }
    try { await tx[T_SCRA]?.sender.replaceTrack(null); } catch { /* ignore */ }
    screenStream?.getTracks().forEach((t) => t.stop());
    screenStream = null;
    liveCfg = null;
    sendMedia();
  }

  function setPeerVolume(kind: "peerVolume" | "streamVolume", v: number) {
    const cs = get(callSettings);
    patchCallSettings({ [kind]: { ...cs[kind], [peerId]: v } } as any);
  }

  // ---- Watching a shared screen on a phone: fullscreen, landscape, pinch ----
  let zoom = $state({ s: 1, x: 0, y: 0 });
  let pinch: { d: number; s: number; x: number; y: number; px: number; py: number } | null = null;
  const dist = (t: TouchList) => Math.hypot(t[0].clientX - t[1].clientX, t[0].clientY - t[1].clientY);
  function zoomStart(e: TouchEvent) {
    const t = e.touches;
    pinch = { d: t.length > 1 ? dist(t) : 0, s: zoom.s, x: zoom.x, y: zoom.y, px: t[0].clientX, py: t[0].clientY };
  }
  function zoomMove(e: TouchEvent) {
    if (!pinch) return;
    const t = e.touches;
    if (t.length > 1 && pinch.d) {
      e.preventDefault();
      zoom = { ...zoom, s: Math.min(5, Math.max(1, pinch.s * (dist(t) / pinch.d))) };
    } else if (zoom.s > 1) {
      e.preventDefault();
      const dx = t[0].clientX - pinch.px, dy = t[0].clientY - pinch.py;
      // Turned sideways: the picture's axes are the screen's, rotated.
      zoom = watchSideways ? { ...zoom, x: pinch.x + dy, y: pinch.y - dx } : { ...zoom, x: pinch.x + dx, y: pinch.y + dy };
    }
  }
  // Phones: the shared screen opens in its own layer over everything —
  // turned sideways when it's wide and the phone is upright (Android's
  // webview can't lock the orientation), pinch to zoom, tap for controls.
  let watch = $state(false);
  let watchUi = $state(true);
  let watchSideways = $state(false);
  function openWatch() {
    const v = rScreen?.getVideoTracks()[0]?.getSettings();
    const wide = !!(v?.width && v?.height && v.width > v.height);
    watchSideways = wide && innerHeight > innerWidth;
    zoom = { s: 1, x: 0, y: 0 };
    watch = true; watchUi = true;
  }
  function closeWatch() { watch = false; zoom = { s: 1, x: 0, y: 0 }; }
  $effect(() => { if (watch) return onBack(closeWatch); });
  $effect(() => { if (watch && !rScreen) closeWatch(); });
  function zoomEnd(e: TouchEvent) {
    if (e.touches.length) { zoomStart(e); return; }
    pinch = null;
    if (zoom.s <= 1.05) zoom = { s: 1, x: 0, y: 0 };
  }
  $effect(() => {
    if (!fullscreen) {
      try { screen.orientation?.unlock?.(); } catch { /* ignore */ }
      zoom = { s: 1, x: 0, y: 0 };
    }
  });

  async function toggleFullscreen() {
    const next = !fullscreen;
    try {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      await getCurrentWindow().setFullscreen(next);
    } catch {
      try {
        if (next) await document.documentElement.requestFullscreen();
        else if (document.fullscreenElement) await document.exitFullscreen();
      } catch { /* ignore */ }
    }
    fullscreen = next;
  }

  // ---------------------------------------------------------------------------
  // Accept / reject / hang up
  // ---------------------------------------------------------------------------
  function onConnected() {
    stopRinging();
    blip(true);
    if (!levelTimer) levelTimer = setInterval(levelTick, 50);
    if (!statsTimer) statsTimer = setInterval(() => void pollStats(), 1000);
    sendMedia();
    void retuneAll();
  }

  async function accept() {
    const s = get(callState);
    if (s.kind !== "incoming" || !pc) return;
    stopRinging();
    try {
      await startMic();
      const answer = await pc.createAnswer();
      await pc.setLocalDescription(answer);
      await api.sendCallSignal(s.peer, { type: "answer", sdp: tuneSdp(answer.sdp!, sdpTuning()) });
      callState.set({ kind: "connected", peer: s.peer, started: Date.now(), direction: "in", placed_at: s.started });
      onConnected();
    } catch (e) {
      showToast(`Could not accept: ${e}`);
      await hangup("local-error");
    }
  }

  async function reject() {
    const s = get(callState);
    if (s.kind !== "incoming") return;
    await sendSig({ type: "reject" });
    teardown("rejected-local");
  }

  async function hangup(reason: string) {
    const s = get(callState);
    if (s.kind === "connected" || s.kind === "outgoing") await sendSig({ type: "hangup" });
    teardown(reason);
  }

  function teardown(reason: string) {
    const wasConnected = get(callState).kind === "connected";
    stopRinging();
    clearNetTimers();
    if (levelTimer) { clearInterval(levelTimer); levelTimer = null; }
    if (statsTimer) { clearInterval(statsTimer); statsTimer = null; }
    if (pc) {
      pc.onconnectionstatechange = null;
      pc.onicecandidate = null;
      pc.ontrack = null;
      try { pc.close(); } catch { /* ignore */ }
      pc = null;
    }
    micRaw?.getTracks().forEach((t) => t.stop());
    micRaw = null;
    if (micTrack) { try { micTrack.stop(); } catch { /* ignore */ } micTrack = null; }
    if (inCtx) { void inCtx.close().catch(() => {}); inCtx = null; }
    inGain = null; gateGain = null; analyser = null; levelBuf = null;
    camStream?.getTracks().forEach((t) => t.stop());
    screenStream?.getTracks().forEach((t) => t.stop());
    camStream = null; screenStream = null; liveCfg = null;
    rMic = null; rScreenAudio = null; rCam = null; rScreen = null;
    if (remoteAudio) remoteAudio.srcObject = null;
    if (remoteStreamAudio) remoteStreamAudio.srcObject = null;
    tx = [];
    pendingIce = [];
    prevBytes.clear();
    remote = { ...NO_REMOTE };
    speakingMe = false; speakingPeer = false; reconnecting = false; pttHeld = false;
    goLiveOpen = false; volOpen = false; devOpen = false; focus = null; stats = null; expanded = true;
    if (fullscreen) void toggleFullscreen();
    peerId = ""; polite = false; makingOffer = false;
    if (wasConnected) blip(false);
    endCall(reason);
  }

  // ---------------------------------------------------------------------------
  // Stats (stream info overlay + network bars)
  // ---------------------------------------------------------------------------
  function rate(id: string, bytes: number, ts: number): number {
    const p = prevBytes.get(id);
    prevBytes.set(id, { b: bytes, t: ts });
    if (!p || ts <= p.t) return 0;
    return Math.round(((bytes - p.b) * 8) / (ts - p.t)); // bytes/ms → kbps
  }

  async function pollStats() {
    if (!pc) return;
    let report: RTCStatsReport;
    try { report = await pc.getStats(); } catch { return; }
    const slotOf = new Map<string, number>();
    tx.forEach((t, i) => { if (t.mid) slotOf.set(t.mid, i); });
    const codecName = (id?: string) => {
      const c = id ? (report.get(id) as any) : null;
      return c?.mimeType ? String(c.mimeType).split("/")[1] : "";
    };
    const v: StatsView = {};
    let pairId: string | undefined;
    report.forEach((s: any) => {
      if (s.type === "transport" && s.selectedCandidatePairId) pairId = s.selectedCandidatePairId;
    });
    report.forEach((s: any) => {
      const slot = s.mid != null ? slotOf.get(String(s.mid)) : undefined;
      if (s.type === "outbound-rtp") {
        const kbps = rate(s.id, s.bytesSent ?? 0, s.timestamp);
        if (s.kind === "video" && (slot === T_SCR || slot === T_CAM)) {
          const vs: VStat = {
            w: s.frameWidth ?? 0, h: s.frameHeight ?? 0, fps: Math.round(s.framesPerSecond ?? 0), kbps,
            codec: codecName(s.codecId), impl: s.encoderImplementation ?? "", limit: s.qualityLimitationReason ?? "", loss: 0,
          };
          if (slot === T_SCR) v.upScreen = vs; else v.upCam = vs;
        } else if (s.kind === "audio" && slot === T_MIC) {
          v.voiceOutKbps = kbps;
        }
      } else if (s.type === "inbound-rtp") {
        const kbps = rate(s.id, s.bytesReceived ?? 0, s.timestamp);
        const lost = s.packetsLost ?? 0, got = s.packetsReceived ?? 0;
        const loss = got + lost > 0 ? (100 * lost) / (got + lost) : 0;
        if (s.kind === "video" && (slot === T_SCR || slot === T_CAM)) {
          const vs: VStat = {
            w: s.frameWidth ?? 0, h: s.frameHeight ?? 0, fps: Math.round(s.framesPerSecond ?? 0), kbps,
            codec: codecName(s.codecId), impl: s.decoderImplementation ?? "", limit: "", loss,
          };
          if (slot === T_SCR) v.downScreen = vs; else v.downCam = vs;
        } else if (s.kind === "audio" && slot === T_MIC) {
          v.voiceInKbps = kbps;
          v.voiceLoss = loss;
          v.jitterMs = Math.round((s.jitter ?? 0) * 1000);
        }
      } else if (s.type === "candidate-pair" && (s.id === pairId || (!pairId && s.nominated && s.state === "succeeded"))) {
        if (s.currentRoundTripTime != null) v.rttMs = Math.round(s.currentRoundTripTime * 1000);
        if (s.availableOutgoingBitrate != null) v.bweKbps = Math.round(s.availableOutgoingBitrate / 1000);
        const lc = report.get(s.localCandidateId) as any;
        const rc = report.get(s.remoteCandidateId) as any;
        if (lc && rc) v.route = lc.candidateType === "relay" || rc.candidateType === "relay" ? "TURN relay" : lc.candidateType === "host" && rc.candidateType === "host" ? "LAN direct" : "P2P direct";
      }
    });
    stats = v;
    const loss = v.voiceLoss ?? 0, rtt = v.rttMs ?? 0;
    netBars = reconnecting ? 0 : rtt < 150 && loss < 2 ? 3 : rtt < 300 && loss < 6 ? 2 : 1;
  }

  function fmtStat(s: VStat | undefined): string {
    if (!s || !s.w) return "—";
    const mbps = s.kbps >= 1000 ? `${(s.kbps / 1000).toFixed(1)} Mbps` : `${s.kbps} kbps`;
    return `${s.w}×${s.h} · ${s.fps} fps · ${mbps} · ${s.codec}`;
  }
  function implLabel(impl: string): string {
    if (!impl) return "";
    if (/MediaFoundation|D3D11|NVENC|QSV|AMF|VideoToolbox|VAAPI|ExternalEncoder|ExternalDecoder/i.test(impl)) return "hardware";
    if (/libvpx|OpenH264|FFmpeg|dav1d|libaom|SimulcastEncoderAdapter/i.test(impl)) return "software";
    return impl;
  }

  // ---------------------------------------------------------------------------
  // Sounds (generated — no assets)
  // ---------------------------------------------------------------------------
  let ring: { ctx: AudioContext; t: ReturnType<typeof setInterval> } | null = null;

  function tone(ctx: AudioContext, at: number, freqs: number[], dur: number, vol: number) {
    const g = ctx.createGain();
    g.gain.setValueAtTime(0, at);
    g.gain.linearRampToValueAtTime(vol, at + 0.02);
    g.gain.setValueAtTime(vol, at + dur - 0.05);
    g.gain.linearRampToValueAtTime(0, at + dur);
    g.connect(ctx.destination);
    for (const f of freqs) {
      const o = ctx.createOscillator();
      o.type = "sine";
      o.frequency.value = f;
      o.connect(g);
      o.start(at);
      o.stop(at + dur + 0.02);
    }
  }

  function startRinging(kind: "in" | "out") {
    stopRinging();
    if (!get(callSettings).ringtone) return;
    try {
      const ctx = new AudioContext();
      void ctx.resume();
      const play = () => {
        const t = ctx.currentTime + 0.03;
        if (kind === "in") {
          tone(ctx, t, [784, 1175], 0.16, 0.09);
          tone(ctx, t + 0.2, [988, 1319], 0.16, 0.09);
          tone(ctx, t + 0.4, [784, 1175], 0.3, 0.09);
        } else {
          tone(ctx, t, [440, 480], 1.2, 0.05);
        }
      };
      play();
      ring = { ctx, t: setInterval(play, kind === "in" ? 1600 : 3200) };
    } catch { /* no audio */ }
  }

  function stopRinging() {
    if (!ring) return;
    clearInterval(ring.t);
    void ring.ctx.close().catch(() => {});
    ring = null;
  }

  function blip(up: boolean) {
    try {
      const ctx = new AudioContext();
      const t = ctx.currentTime + 0.02;
      tone(ctx, t, [up ? 660 : 880], 0.09, 0.06);
      tone(ctx, t + 0.1, [up ? 880 : 660], 0.12, 0.06);
      setTimeout(() => void ctx.close().catch(() => {}), 600);
    } catch { /* ignore */ }
  }

  // ---------------------------------------------------------------------------
  // View helpers
  // ---------------------------------------------------------------------------
  const srcObject: Action<HTMLVideoElement, MediaStream | null> = (node, stream) => {
    const set = (s: MediaStream | null) => {
      if (node.srcObject !== s) {
        node.srcObject = s;
        if (s) node.play().catch(() => {});
      }
    };
    set(stream ?? null);
    return { update: (s) => set(s ?? null) };
  };

  const tiles = $derived.by(() => {
    const t: TileId[] = [];
    if (remote.screen && rScreen) t.push("r-screen");
    if (remote.cam && rCam) t.push("r-cam");
    if (screenStream) t.push("l-screen");
    if (camStream) t.push("l-cam");
    return t;
  });
  const hasVideo = $derived(tiles.length > 0);
  const focused = $derived<TileId | null>(focus && tiles.includes(focus) ? focus : (tiles[0] ?? null));
  const thumbs = $derived.by<TileId[]>(() => {
    const t = tiles.filter((x) => x !== focused);
    if (!remote.cam) t.push("r-ava");
    return t;
  });
  const showStage = $derived($callState.kind === "connected" && hasVideo && expanded);
  const peerName = $derived($callState.kind === "idle" ? "" : $callState.peer);
  const myLetter = $derived(($info?.username ?? "?").charAt(0).toUpperCase());
  const peerLetter = $derived(peerName.charAt(0).toUpperCase() || "?");
  const glEstimate = $derived(targetVideoKbps(gl.res, gl.fps, $callSettings.maxVideoKbps));

  function tileTitle(id: TileId): string {
    switch (id) {
      case "r-screen": return `${peerName} — stream${remote.label ? ` · ${remote.label}` : ""}`;
      case "r-cam": return peerName;
      case "l-screen": return `You — stream${liveCfg ? ` · ${qualityLabel(liveCfg.res, liveCfg.fps)}` : ""}`;
      case "l-cam": return "You";
      default: return peerName;
    }
  }

  function fmt(sec: number): string {
    const h = Math.floor(sec / 3600), m = Math.floor((sec % 3600) / 60), s = sec % 60;
    const mm = m.toString().padStart(2, "0"), ss = s.toString().padStart(2, "0");
    return h ? `${h}:${mm}:${ss}` : `${mm}:${ss}`;
  }

  const svg = (p: string) =>
    `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">${p}</svg>`;
  const SLASH = `<line x1="3" y1="3" x2="21" y2="21"/>`;
  const MIC = `<rect x="9" y="2" width="6" height="12" rx="3"/><path d="M5 10v1a7 7 0 0 0 14 0v-1"/><line x1="12" y1="18" x2="12" y2="22"/>`;
  const HEAD = `<path d="M3 18v-6a9 9 0 0 1 18 0v6"/><path d="M21 19a2 2 0 0 1-2 2h-1v-6h3zM3 19a2 2 0 0 0 2 2h1v-6H3z"/>`;
  const CAM = `<path d="M23 7l-7 5 7 5V7z"/><rect x="1" y="5" width="15" height="14" rx="2"/>`;
  const I = {
    mic: svg(MIC),
    micOff: svg(MIC + SLASH),
    head: svg(HEAD),
    headOff: svg(HEAD + SLASH),
    cam: svg(CAM),
    camOff: svg(CAM + SLASH),
    screen: svg(`<rect x="2" y="3" width="20" height="14" rx="2"/><line x1="8" y1="21" x2="16" y2="21"/><line x1="12" y1="17" x2="12" y2="21"/><path d="M12 13V7M9 10l3-3 3 3"/>`),
    gear: svg(`<line x1="4" y1="21" x2="4" y2="14"/><line x1="4" y1="10" x2="4" y2="3"/><line x1="12" y1="21" x2="12" y2="12"/><line x1="12" y1="8" x2="12" y2="3"/><line x1="20" y1="21" x2="20" y2="16"/><line x1="20" y1="12" x2="20" y2="3"/><line x1="1" y1="14" x2="7" y2="14"/><line x1="9" y1="8" x2="15" y2="8"/><line x1="17" y1="16" x2="23" y2="16"/>`),
    hang: svg(`<path d="M10.68 13.31a16 16 0 0 0 3.41 2.6l1.27-1.27a2 2 0 0 1 2.11-.45 12.84 12.84 0 0 0 2.81.7 2 2 0 0 1 1.72 2v3a2 2 0 0 1-2.18 2 19.79 19.79 0 0 1-8.63-3.07 19.42 19.42 0 0 1-3.33-2.67m-2.67-3.34a19.79 19.79 0 0 1-3.07-8.63A2 2 0 0 1 4.11 2h3a2 2 0 0 1 2 1.72 12.84 12.84 0 0 0 .7 2.81 2 2 0 0 1-.45 2.11L8.09 9.91"/><line x1="23" y1="1" x2="1" y2="23"/>`),
    phone: svg(`<path d="M22 16.92v3a2 2 0 0 1-2.18 2 19.79 19.79 0 0 1-8.63-3.07 19.5 19.5 0 0 1-6-6 19.79 19.79 0 0 1-3.07-8.67A2 2 0 0 1 4.11 2h3a2 2 0 0 1 2 1.72 12.84 12.84 0 0 0 .7 2.81 2 2 0 0 1-.45 2.11L8.09 9.91a16 16 0 0 0 6 6l1.27-1.27a2 2 0 0 1 2.11-.45 12.84 12.84 0 0 0 2.81.7A2 2 0 0 1 22 16.92z"/>`),
    expand: svg(`<polyline points="15 3 21 3 21 9"/><polyline points="9 21 3 21 3 15"/><line x1="21" y1="3" x2="14" y2="10"/><line x1="3" y1="21" x2="10" y2="14"/>`),
    shrink: svg(`<polyline points="4 14 10 14 10 20"/><polyline points="20 10 14 10 14 4"/><line x1="14" y1="10" x2="21" y2="3"/><line x1="3" y1="21" x2="10" y2="14"/>`),
    full: svg(`<path d="M8 3H5a2 2 0 0 0-2 2v3m18 0V5a2 2 0 0 0-2-2h-3m0 18h3a2 2 0 0 0 2-2v-3M3 16v3a2 2 0 0 0 2 2h3"/>`),
    stats: svg(`<polyline points="22 12 18 12 15 21 9 3 6 12 2 12"/>`),
    vol: svg(`<polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5"/><path d="M15.54 8.46a5 5 0 0 1 0 7.07M19.07 4.93a10 10 0 0 1 0 14.14"/>`),
    x: svg(`<line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>`),
    flip: svg(`<path d="M20 7h-3.2l-1.6-2H8.8L7.2 7H4a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2z"/><path d="M9 13.5a3 3 0 0 1 5.2-2"/><path d="M15 12.5a3 3 0 0 1-5.2 2"/><polyline points="14.5 9.5 14.4 11.6 12.3 11.4"/><polyline points="9.5 16.5 9.6 14.4 11.7 14.6"/>`),
  };
</script>

<audio bind:this={remoteAudio} autoplay hidden data-call-media></audio>
<audio bind:this={remoteStreamAudio} autoplay hidden data-call-media></audio>

{#snippet bars()}
  <span class="bars" title={stats?.rttMs != null ? `Ping ${stats.rttMs} ms${stats.route ? ` · ${stats.route}` : ""}` : "Connection"} data-q={netBars}>
    <i></i><i></i><i></i>
  </span>
{/snippet}

{#snippet avatar(letter: string, speaking: boolean, size: "lg" | "md" | "sm", muted = false, deaf = false, img = "")}
  <div class="ava {size}" class:speaking style:background-image={img ? `url("${img}")` : undefined} style:background-size={img ? "cover" : undefined} style:background-position={img ? "center" : undefined}>
    {#if !img}<span>{letter}</span>{/if}
    {#if deaf}<b class="ava-badge">{@html I.headOff}</b>{:else if muted}<b class="ava-badge">{@html I.micOff}</b>{/if}
  </div>
{/snippet}

{#snippet tile(id: TileId, big: boolean)}
  <div
    class="tile"
    class:big
    class:speaking={(id === "r-cam" || id === "r-ava") && speakingPeer}
    class:me-speaking={id === "l-cam" && speakingMe}
    role="button"
    tabindex="0"
    title={$t(big ? "Double-click for fullscreen" : "Click to focus")}
    onclick={() => {
      if (touchUi && id === "r-screen") { openWatch(); return; }
      if (!big && id !== "r-ava") focus = id;
      else if (touchUi && id === "r-screen") openWatch();
    }}
    onkeydown={(e) => { if (e.key === "Enter" && !big && id !== "r-ava") focus = id; }}
    ondblclick={() => { if (big) void toggleFullscreen(); }}
  >
    {#if id === "r-ava"}
      <div class="tile-ava">{@render avatar(peerLetter, speakingPeer, big ? "lg" : "md", remote.muted, remote.deaf, $avatars[peerId] ?? "")}</div>
    {:else}
      <!-- svelte-ignore a11y_media_has_caption -->
      <video
        use:srcObject={id === "r-screen" ? rScreen : id === "r-cam" ? rCam : id === "l-screen" ? screenStream : camStream}
        class:mirror={id === "l-cam" && facing === "user"}
        class:contain={id === "r-screen" || id === "l-screen" || (big && id === "r-cam")}
        style:transform={id === "r-screen" && big && zoom.s > 1 ? `translate(${zoom.x}px, ${zoom.y}px) scale(${zoom.s})` : undefined}
        ontouchstart={id === "r-screen" && big ? zoomStart : undefined}
        ontouchmove={id === "r-screen" && big ? zoomMove : undefined}
        ontouchend={id === "r-screen" && big ? zoomEnd : undefined}
        autoplay playsinline muted data-call-media
      ></video>
    {/if}
    <div class="tile-tag">
      {#if id === "r-screen" || id === "l-screen"}<span class="live">LIVE</span>{/if}
      <span class="tile-name">{tileTitle(id)}</span>
      {#if id === "r-cam" || id === "r-ava"}
        {#if remote.deaf}<b class="tag-ico">{@html I.headOff}</b>{:else if remote.muted}<b class="tag-ico">{@html I.micOff}</b>{/if}
      {/if}
    </div>
    {#if big && id === "r-screen" && stats?.downScreen?.w}
      <div class="tile-res">{stats.downScreen.w}×{stats.downScreen.h} · {stats.downScreen.fps} fps</div>
    {/if}
  </div>
{/snippet}

{#snippet goLivePanel()}
  <div class="pop golive" role="dialog" aria-label={$t("Screen share")}>
    <div class="pop-title">{$t(screenStream ? "Stream quality" : "Go Live")}</div>
    <div class="pop-label">{$t("Resolution")}</div>
    <div class="pop-seg">
      {#each (["720", "1080", "1440", "source"] as ResPreset[]) as r}
        <button class:active={gl.res === r} onclick={() => (gl.res = r)}>{RES_LABEL[r]}</button>
      {/each}
    </div>
    <div class="pop-label">{$t("Frame rate")}</div>
    <div class="pop-seg">
      {#each ([15, 30, 60] as FpsPreset[]) as f}
        <button class:active={gl.fps === f} onclick={() => (gl.fps = f)}>{f} FPS</button>
      {/each}
    </div>
    <div class="pop-label">{$t("Optimize for")}</div>
    <div class="pop-seg">
      <button class:active={gl.hint === "motion"} onclick={() => (gl.hint = "motion")}>{$t("Motion")}</button>
      <button class:active={gl.hint === "detail"} onclick={() => (gl.hint = "detail")}>Text &amp; detail</button>
    </div>
    <label class="pop-check" class:disabled={!!screenStream}>
      <input type="checkbox" bind:checked={gl.audio} disabled={!!screenStream} />
      {$t("Share system audio")}
    </label>
    <div class="pop-est">
      up to {(glEstimate / 1000).toFixed(1)} Mbps{gl.res === "source" ? " (scales with your screen)" : ""}
    </div>
    <div class="pop-actions">
      {#if screenStream}<button class="pop-btn danger" onclick={stopScreen}>{$t("Stop streaming")}</button>{/if}
      <button class="pop-btn primary" onclick={goLive}>{$t(screenStream ? "Apply" : "Go Live")}</button>
    </div>
  </div>
{/snippet}

{#snippet devicesPanel()}
  <div class="pop dev" role="dialog" aria-label={$t("Microphone and sound")}>
    <div class="pop-label">{$t("Microphone")}</div>
    <Select class="pop-select" value={$callSettings.micId}
      options={[{ value: "default", label: $t("System default") }, ...devices.mics.filter((d) => d.deviceId && d.deviceId !== "default").map((d) => ({ value: d.deviceId, label: d.label || $t("Microphone") }))]}
      onchange={(v) => void switchMic(v)} />
    {#if canPickSpeaker}
      <div class="pop-label">{$t("Speaker")}</div>
      <Select class="pop-select" value={$callSettings.speakerId}
        options={[{ value: "default", label: $t("System default") }, ...devices.speakers.filter((d) => d.deviceId && d.deviceId !== "default").map((d) => ({ value: d.deviceId, label: d.label || $t("Speaker") }))]}
        onchange={(v) => callSettings.update((c) => ({ ...c, speakerId: v }))} />
    {/if}
    <div class="pop-label">{$t("Microphone volume")}</div>
    <div class="pop-range">
      <input type="range" min="0" max="2" step="0.01" value={$callSettings.inputVolume}
        oninput={(e) => callSettings.update((c) => ({ ...c, inputVolume: parseFloat((e.target as HTMLInputElement).value) }))} />
      <span>{Math.round($callSettings.inputVolume * 100)}%</span>
    </div>
    <div class="pop-label">{$t("Sound volume")}</div>
    <div class="pop-range">
      <input type="range" min="0" max="1" step="0.01" value={$callSettings.outputVolume}
        oninput={(e) => callSettings.update((c) => ({ ...c, outputVolume: parseFloat((e.target as HTMLInputElement).value) }))} />
      <span>{Math.round($callSettings.outputVolume * 100)}%</span>
    </div>
    <label class="pop-check"><input type="checkbox" checked={$callSettings.noiseSuppression}
      onchange={(e) => { callSettings.update((c) => ({ ...c, noiseSuppression: (e.target as HTMLInputElement).checked })); void switchMic($callSettings.micId); }} /> {$t("Noise suppression")}</label>
    <button class="pop-btn" onclick={() => { devOpen = false; openSettings("voice"); }}>{$t("All voice & video settings")}</button>
  </div>
{/snippet}

{#snippet volumePanel()}
  <div class="pop vol" role="dialog" aria-label={$t("Volume")}>
    <div class="pop-title">{peerName}</div>
    <div class="pop-label">{$t("Voice volume")}</div>
    <div class="pop-range">
      <input type="range" min="0" max="1" step="0.01" value={$callSettings.peerVolume[peerId] ?? 1}
        oninput={(e) => setPeerVolume("peerVolume", parseFloat((e.target as HTMLInputElement).value))} />
      <span>{Math.round(($callSettings.peerVolume[peerId] ?? 1) * 100)}%</span>
    </div>
    <div class="pop-label">{$t("Stream volume")}</div>
    <div class="pop-range">
      <input type="range" min="0" max="1" step="0.01" value={$callSettings.streamVolume[peerId] ?? 1}
        oninput={(e) => setPeerVolume("streamVolume", parseFloat((e.target as HTMLInputElement).value))} />
      <span>{Math.round(($callSettings.streamVolume[peerId] ?? 1) * 100)}%</span>
    </div>
  </div>
{/snippet}

{#snippet controls()}
  <div class="controls">
    <button class="cb" class:off={$callMuted} onclick={toggleMute}
      title={$callSettings.pushToTalk ? `Push to talk: ${keyLabel($callSettings.pttKey)} · Ctrl+Shift+M to mute` : "Mute (Ctrl+Shift+M)"}>
      {@html $callMuted ? I.micOff : I.mic}
      {#if $callSettings.pushToTalk && !$callMuted}<span class="ptt" class:held={pttHeld}>PTT</span>{/if}
    </button>
    <button class="cb" class:off={deafened} onclick={toggleDeafen} title={$t("Deafen (Ctrl+Shift+D)")}>{@html deafened ? I.headOff : I.head}</button>
    <button class="cb" class:on={!!camStream} onclick={toggleCam} title={$t(camStream ? "Turn camera off" : "Turn camera on")}>{@html camStream ? I.cam : I.camOff}</button>
    {#if touchUi && camStream}
      <button class="cb" onclick={flipCam} title={$t(facing === "user" ? "Back camera" : "Front camera")} aria-label={$t("Switch camera")}>{@html I.flip}</button>
    {/if}
    {#if screenShareOk || screenStream}
      <div class="anchor">
        <button class="cb" class:live={!!screenStream} onclick={openGoLive} title={$t(screenStream ? "Stream settings" : "Share your screen")}>{@html I.screen}</button>
        {#if goLiveOpen}{@render goLivePanel()}{/if}
      </div>
    {/if}
    <div class="anchor">
      <button class="cb" class:on={volOpen} onclick={() => { goLiveOpen = false; devOpen = false; volOpen = !volOpen; }} title={$t("Volume")}>{@html I.vol}</button>
      {#if volOpen}{@render volumePanel()}{/if}
    </div>
    <div class="anchor">
      <button class="cb" class:on={devOpen} onclick={openDevices} title={$t("Microphone and sound")}>{@html I.gear}</button>
      {#if devOpen}{@render devicesPanel()}{/if}
    </div>
    <button class="cb hang" onclick={() => hangup("local-hangup")} title={$t("Hang up")}>{@html I.hang}</button>
  </div>
{/snippet}

{#snippet statsPanel()}
  <div class="stats">
    {#if stats}
      {#if stats.downScreen?.w}<div><b>{$t("↓ stream")}</b> {fmtStat(stats.downScreen)} <em>{implLabel(stats.downScreen.impl)}</em>{stats.downScreen.loss > 0.05 ? ` · loss ${stats.downScreen.loss.toFixed(1)}%` : ""}</div>{/if}
      {#if stats.downCam?.w}<div><b>{$t("↓ camera")}</b> {fmtStat(stats.downCam)}</div>{/if}
      {#if stats.upScreen?.w}<div><b>{$t("↑ stream")}</b> {fmtStat(stats.upScreen)} <em>{implLabel(stats.upScreen.impl)}</em>{stats.upScreen.limit && stats.upScreen.limit !== "none" ? ` · limited by ${stats.upScreen.limit}` : ""}</div>{/if}
      {#if stats.upCam?.w}<div><b>{$t("↑ camera")}</b> {fmtStat(stats.upCam)}</div>{/if}
      <div><b>{$t("voice")}</b> ↑{stats.voiceOutKbps ?? 0} ↓{stats.voiceInKbps ?? 0} kbps · loss {(stats.voiceLoss ?? 0).toFixed(1)}% · jitter {stats.jitterMs ?? 0} ms</div>
      <div><b>{$t("net")}</b> ping {stats.rttMs ?? "—"} ms{stats.bweKbps ? ` · uplink ≈ ${(stats.bweKbps / 1000).toFixed(1)} Mbps` : ""}{stats.route ? ` · ${stats.route}` : ""}</div>
    {:else}
      <div>{$t("collecting…")}</div>
    {/if}
  </div>
{/snippet}

{#if watch && rScreen}
  <div class="watch" role="dialog" aria-label={$t("Screen share")}>
    <div class="watch-in" class:sideways={watchSideways}>
      <!-- svelte-ignore a11y_media_has_caption -->
      <video use:srcObject={rScreen} autoplay playsinline muted
        style:transform={zoom.s > 1 ? `translate(${zoom.x}px, ${zoom.y}px) scale(${zoom.s})` : undefined}
        ontouchstart={zoomStart} ontouchmove={zoomMove} ontouchend={zoomEnd}
        onclick={() => (watchUi = !watchUi)}></video>
    </div>
    {#if watchUi}
      <div class="watch-bar">
        <button onclick={closeWatch} aria-label={$t("Close")}>×</button>
        <span>{tileTitle("r-screen")}</span>
        <button class:on={watchSideways} onclick={() => { watchSideways = !watchSideways; zoom = { s: 1, x: 0, y: 0 }; }} aria-label={$t("Rotate")} title={$t("Rotate")}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 12a9 9 0 1 0 3-6.7L3 8"/><path d="M3 3v5h5"/></svg>
        </button>
      </div>
    {/if}
  </div>
{/if}

{#if showStage}
  <div class="stage" class:fs={fullscreen} role="dialog" aria-label={$t("Call")}>
    <header class="st-head">
      <div class="st-peer">
        {@render avatar(peerLetter, speakingPeer, "sm", remote.muted, remote.deaf, $avatars[peerId] ?? "")}
        <span class="st-name">{peerName}</span>
        <span class="st-time">{fmt(elapsedSeconds)}</span>
        {#if reconnecting}<span class="recon">{$t("Reconnecting…")}</span>{/if}
      </div>
      <div class="st-tools">
        {@render bars()}
        <button class="tb" class:on={statsOpen} onclick={() => { statsOpen = !statsOpen; patchCallSettings({ showStats: statsOpen }); }} title={$t("Stream info")}>{@html I.stats}</button>
        <button class="tb" onclick={() => (expanded = false)} title={$t("Minimize")}>{@html I.shrink}</button>
        <button class="tb" onclick={toggleFullscreen} title={$t(fullscreen ? "Exit fullscreen (Esc)" : "Fullscreen")}>{@html I.full}</button>
      </div>
    </header>
    <div class="st-main">
      {#if focused}{@render tile(focused, true)}{/if}
      {#if statsOpen}{@render statsPanel()}{/if}
    </div>
    {#if thumbs.length}
      <div class="st-strip">
        {#each thumbs as t (t)}{@render tile(t, false)}{/each}
      </div>
    {/if}
    <div class="st-controls">{@render controls()}</div>
  </div>
{:else if $callState.kind !== "idle"}
  <div class="mini" class:ended={$callState.kind === "ended"}>
    {#if $callState.kind === "connected" && hasVideo && focused}
      <div class="mini-video">
        {@render tile(focused, false)}
        <button class="mini-expand" onclick={() => (expanded = true)} title={$t("Expand")}>{@html I.expand}</button>
      </div>
    {/if}
    <div class="mini-top">
      {@render avatar(peerLetter, speakingPeer, "md", remote.muted, remote.deaf, $avatars[peerId] ?? "")}
      <div class="mini-text">
        <div class="mini-name">{$callState.kind === "ended" ? "Call ended" : peerName}</div>
        <div class="mini-status">
          {#if $callState.kind === "outgoing"}
            calling<span class="dots"></span>
          {:else if $callState.kind === "incoming"}
            incoming call
          {:else if $callState.kind === "connected"}
            {#if reconnecting}<span class="recon">{$t("reconnecting…")}</span>{:else}<span class="live-dot"></span>{fmt(elapsedSeconds)}{/if}
            {#if screenStream}<span class="live sm">LIVE</span>{/if}
          {:else}
            {$callState.reason.replace(/-/g, " ")}
          {/if}
        </div>
      </div>
      {#if $callState.kind === "connected"}
        <div class="mini-me">{@render avatar(myLetter, speakingMe, "sm", $callMuted, deafened, $avatars[$info?.username ?? ""] ?? "")}</div>
        {@render bars()}
      {/if}
    </div>

    {#if $callState.kind === "incoming"}
      <div class="controls">
        <button class="cb hang" onclick={reject} title={$t("Decline")}>{@html I.x}</button>
        <button class="cb accept" onclick={accept} title={$t("Accept")}>{@html I.phone}</button>
      </div>
    {:else if $callState.kind === "outgoing"}
      <div class="controls">
        <button class="cb" class:off={$callMuted} onclick={toggleMute} title={$t("Mute")}>{@html $callMuted ? I.micOff : I.mic}</button>
        <button class="cb hang" onclick={() => hangup("local-cancel")} title={$t("Cancel")}>{@html I.hang}</button>
      </div>
    {:else if $callState.kind === "connected"}
      {@render controls()}
      {#if statsOpen && !hasVideo}{@render statsPanel()}{/if}
      <button class="mini-statsbtn" class:on={statsOpen} onclick={() => { statsOpen = !statsOpen; patchCallSettings({ showStats: statsOpen }); }}>
        {$t(statsOpen ? "hide connection info" : "connection info")}
      </button>
    {/if}
  </div>
{/if}

<style>
  /* ---------- shared ---------- */
  .ava {
    position: relative;
    border-radius: var(--ava-r, 50%);
    background: linear-gradient(135deg, var(--accent) 0%, var(--accent-2) 100%);
    display: grid; place-items: center;
    color: #06070C; font-weight: 700; letter-spacing: -0.02em;
    flex-shrink: 0;
    box-shadow: 0 0 0 2px transparent;
    transition: box-shadow 0.12s ease;
  }
  .ava.lg { width: 96px; height: 96px; font-size: 38px; }
  .ava.md { width: 44px; height: 44px; font-size: 18px; }
  .ava.sm { width: 28px; height: 28px; font-size: 12px; }
  .ava.speaking { box-shadow: 0 0 0 2px var(--bg-surface), 0 0 0 4px #3BA55D; }
  .ava-badge {
    position: absolute; right: -3px; bottom: -3px;
    width: 18px; height: 18px; border-radius: var(--ava-r, 50%);
    background: var(--danger); color: #fff;
    display: grid; place-items: center;
    border: 2px solid var(--bg-surface);
  }
  .ava.sm .ava-badge { width: 14px; height: 14px; right: -4px; bottom: -4px; }
  .ava-badge :global(svg) { width: 60%; height: 60%; }

  .bars { display: inline-flex; align-items: flex-end; gap: 2px; height: 14px; padding: 0 2px; }
  .bars i { width: 3px; border-radius: 1px; background: var(--fg-dim); opacity: 0.35; }
  .bars i:nth-child(1) { height: 5px; } .bars i:nth-child(2) { height: 9px; } .bars i:nth-child(3) { height: 13px; }
  .bars[data-q="3"] i { background: #3BA55D; opacity: 1; }
  .bars[data-q="2"] i:nth-child(-n+2) { background: #FAA61A; opacity: 1; }
  .bars[data-q="1"] i:nth-child(1) { background: var(--danger); opacity: 1; }

  .live {
    background: #ED4245; color: #fff; font-size: 10px; font-weight: 800;
    letter-spacing: 0.06em; padding: 1px 5px; border-radius: calc(4px * var(--rk, 1));
  }
  .live.sm { margin-left: 6px; font-size: 9px; }
  .recon { color: #FAA61A; font-weight: 600; }

  .controls { display: flex; gap: 8px; justify-content: center; align-items: center; }
  .anchor { position: relative; }
  .cb {
    position: relative;
    width: 44px; height: 44px; border-radius: 50%;
    border: none;
    background: var(--bg-input);
    color: var(--fg);
    cursor: pointer;
    display: grid; place-items: center;
    transition: background 0.12s ease, color 0.12s ease, transform 0.12s ease;
  }
  .cb :global(svg) { width: 20px; height: 20px; }
  .cb:hover { background: var(--bg-hover); }
  .cb.off { background: rgba(237, 66, 69, 0.16); color: #ED4245; }
  .cb.on { background: var(--accent-soft); color: var(--accent); }
  .cb.live { background: #ED4245; color: #fff; }
  .cb.hang { background: #ED4245; color: #fff; width: 52px; border-radius: calc(22px * var(--rk, 1)); }
  .cb.hang:hover { background: #C93B3E; }
  .cb.accept { background: #3BA55D; color: #fff; width: 52px; border-radius: calc(22px * var(--rk, 1)); }
  .ptt {
    position: absolute; bottom: -4px; left: 50%; transform: translateX(-50%);
    font-size: 8.5px; font-weight: 800; letter-spacing: 0.05em;
    background: var(--bg-surface); color: var(--fg-dim);
    border: 1px solid var(--border-strong); border-radius: calc(4px * var(--rk, 1)); padding: 0 3px;
  }
  .ptt.held { color: #3BA55D; border-color: #3BA55D; }

  /* ---------- popovers ---------- */
  .pop {
    position: absolute; bottom: calc(100% + 10px); left: 50%; transform: translateX(-50%);
    width: 300px;
    background: var(--bg-surface);
    border: 1px solid var(--border-strong);
    border-radius: calc(12px * var(--rk, 1));
    padding: 14px;
    box-shadow: 0 18px 40px -10px rgba(0, 0, 0, 0.7);
    z-index: 5;
    text-align: left;
  }
  .pop.vol { width: 240px; }
  .pop.dev { width: 280px; left: auto; right: -40px; transform: none; }
  .pop-select { width: 100%; padding: 8px 10px; border-radius: calc(9px * var(--rk, 1)); border: 1px solid var(--border-strong); background: var(--bg-input); color: var(--fg); font: inherit; font-size: 13px; }
  .pop-check { display: flex; align-items: center; gap: 8px; margin: 12px 0 4px; font-size: 13px; color: var(--fg); cursor: pointer; }
  .pop-check input { accent-color: var(--accent); }
  .pop.dev .pop-btn { width: 100%; margin-top: 10px; }
  /* Buttons keep their size whatever is open: nothing jumps. */
  .controls :global(.cb) { flex: 0 0 auto; }
  .pop-title { font-weight: 700; font-size: 14px; color: var(--fg); margin-bottom: 8px; }
  .pop-label { font-size: 11px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.06em; color: var(--fg-dim); margin: 10px 0 6px; }
  .pop-seg { display: flex; gap: 4px; background: var(--bg-input); border-radius: calc(8px * var(--rk, 1)); padding: 3px; }
  .pop-seg button {
    flex: 1; padding: 6px 4px; border: none; border-radius: calc(6px * var(--rk, 1)); background: transparent;
    color: var(--fg-muted); font-size: 12px; font-weight: 600; cursor: pointer; white-space: nowrap;
  }
  .pop-seg button:hover { color: var(--fg); }
  .pop-seg button.active { background: var(--accent-soft); color: var(--accent); }
  .pop-check { display: flex; align-items: center; gap: 8px; margin-top: 12px; font-size: 13px; color: var(--fg); cursor: pointer; }
  .pop-check.disabled { opacity: 0.55; cursor: default; }
  .pop-check input { accent-color: var(--accent); }
  .pop-est { margin-top: 8px; font-size: 11.5px; color: var(--fg-dim); }
  .pop-actions { display: flex; gap: 8px; justify-content: flex-end; margin-top: 14px; }
  .pop-btn { border: none; border-radius: calc(8px * var(--rk, 1)); padding: 8px 14px; font-weight: 700; font-size: 13px; cursor: pointer; }
  .pop-btn.primary { background: var(--accent); color: #06070C; }
  .pop-btn.danger { background: rgba(237, 66, 69, 0.16); color: #ED4245; }
  .pop-range { display: flex; align-items: center; gap: 10px; }
  .pop-range input { flex: 1; accent-color: var(--accent); }
  .pop-range span { min-width: 38px; text-align: right; font-size: 12px; color: var(--fg-muted); font-variant-numeric: tabular-nums; }

  /* ---------- mini card ---------- */
  .mini {
    position: fixed; right: 20px; bottom: 20px; z-index: 250;
    width: 340px;
    background: var(--bg-surface);
    border: 1px solid var(--border-strong);
    border-radius: calc(16px * var(--rk, 1));
    padding: 14px;
    box-shadow: 0 24px 48px -12px rgba(0, 0, 0, 0.6), var(--shadow-accent);
    display: flex; flex-direction: column; gap: 12px;
    animation: slide-in 0.2s ease-out;
  }
  .mini.ended { opacity: 0.85; }
  @keyframes slide-in { from { opacity: 0; transform: translateY(10px); } to { opacity: 1; transform: none; } }
  .mini-top { display: flex; align-items: center; gap: 12px; }
  .mini-text { flex: 1; min-width: 0; }
  .mini-name { font-weight: 700; font-size: 15px; color: var(--fg); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .mini-status { font-size: 12.5px; color: var(--fg-muted); display: flex; align-items: center; gap: 6px; font-variant-numeric: tabular-nums; }
  .mini-me { margin-right: 2px; }
  .mini-video { position: relative; border-radius: calc(10px * var(--rk, 1)); overflow: hidden; aspect-ratio: 16 / 9; background: #000; }
  .mini-video .tile { width: 100%; height: 100%; border-radius: 0; }
  .mini-expand {
    position: absolute; top: 8px; right: 8px;
    width: 30px; height: 30px; border-radius: calc(8px * var(--rk, 1)); border: none;
    background: rgba(0, 0, 0, 0.55); color: #fff; cursor: pointer; display: grid; place-items: center;
  }
  .mini-expand :global(svg) { width: 16px; height: 16px; }
  .mini-statsbtn {
    align-self: center; background: none; border: none; color: var(--fg-dim);
    font-size: 11px; cursor: pointer; padding: 0;
  }
  .mini-statsbtn:hover, .mini-statsbtn.on { color: var(--fg-muted); }
  .mini .stats { position: static; width: auto; }
  .live-dot {
    width: 7px; height: 7px; border-radius: 50%;
    background: #3BA55D; box-shadow: 0 0 6px #3BA55D;
    display: inline-block;
  }
  .dots::after { content: '...'; animation: ellipsis 1.5s steps(4, end) infinite; }
  @keyframes ellipsis { from { opacity: 0; } to { opacity: 1; } }

  /* ---------- stage ---------- */
  .stage {
    position: fixed; inset: 14px; z-index: 250;
    background: #0B0C10;
    border: 1px solid var(--border-strong);
    border-radius: calc(16px * var(--rk, 1));
    box-shadow: 0 30px 80px -20px rgba(0, 0, 0, 0.8);
    display: grid;
    grid-template-rows: auto minmax(0, 1fr) auto auto;
    overflow: hidden;
    animation: slide-in 0.18s ease-out;
  }
  .stage.fs { inset: 0; border-radius: 0; border: none; }
  .watch { position: fixed; inset: 0; z-index: 2000; background: #000; overflow: hidden; touch-action: none; }
  .watch-in { position: absolute; inset: 0; display: grid; place-items: center; overflow: hidden; }
  .watch-in.sideways { inset: auto; top: 50%; left: 50%; width: 100vh; height: 100vw; transform: translate(-50%, -50%) rotate(90deg); }
  .watch video { width: 100%; height: 100%; object-fit: contain; transform-origin: center; }
  .watch-bar { position: absolute; left: 0; right: 0; top: 0; display: flex; align-items: center; gap: 10px;
    padding: calc(env(safe-area-inset-top, 0px) + 10px) 12px 18px; background: linear-gradient(rgba(0,0,0,.65), transparent); color: #fff; }
  .watch-bar span { flex: 1; min-width: 0; font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .watch-bar button { width: 42px; height: 42px; border-radius: 50%; border: none; background: rgba(255,255,255,.14); color: #fff; font-size: 24px; display: grid; place-items: center; }
  .watch-bar button.on { background: var(--accent); color: #06070C; }
  .watch-bar svg { width: 20px; height: 20px; }
  .st-head { display: flex; align-items: center; justify-content: space-between; padding: 10px 14px; }
  .st-peer { display: flex; align-items: center; gap: 10px; min-width: 0; }
  .st-name { font-weight: 700; color: #fff; font-size: 14px; }
  .st-time { color: rgba(255, 255, 255, 0.55); font-size: 12.5px; font-variant-numeric: tabular-nums; }
  .st-tools { display: flex; align-items: center; gap: 6px; }
  .tb {
    width: 32px; height: 32px; border-radius: calc(8px * var(--rk, 1)); border: none;
    background: transparent; color: rgba(255, 255, 255, 0.7); cursor: pointer;
    display: grid; place-items: center;
  }
  .tb:hover { background: rgba(255, 255, 255, 0.08); color: #fff; }
  .tb.on { color: var(--accent); }
  .tb :global(svg) { width: 17px; height: 17px; }
  .st-main { position: relative; padding: 0 14px; min-height: 0; display: flex; }
  .st-main > :global(.tile) { flex: 1; }
  .st-strip { display: flex; gap: 10px; justify-content: center; padding: 10px 14px 0; height: 118px; }
  .st-strip > :global(.tile) { width: 190px; flex: 0 0 auto; }
  .st-controls { padding: 12px; display: flex; justify-content: center; }
  .st-controls .controls { background: rgba(0, 0, 0, 0.35); padding: 8px 10px; border-radius: 30px; }
  .stage .cb { background: rgba(255, 255, 255, 0.08); color: #fff; }
  .stage .cb:hover { background: rgba(255, 255, 255, 0.14); }
  .stage .cb.off { background: rgba(237, 66, 69, 0.22); color: #ff6b6e; }
  .stage .cb.on { background: var(--accent-soft); color: var(--accent); }
  .stage .cb.live, .stage .cb.hang { background: #ED4245; color: #fff; }

  /* ---------- tiles ---------- */
  .tile {
    position: relative;
    background: #15171C;
    border-radius: calc(12px * var(--rk, 1));
    overflow: hidden;
    min-width: 0; min-height: 0;
    cursor: pointer;
    box-shadow: inset 0 0 0 2px transparent;
    transition: box-shadow 0.12s ease;
  }
  .tile.big { cursor: default; }
  .tile.speaking, .tile.me-speaking { box-shadow: inset 0 0 0 3px #3BA55D; }
  .tile video { width: 100%; height: 100%; display: block; object-fit: cover; background: #000; }
  .tile video.contain { object-fit: contain; }
  .tile.big video.contain { touch-action: none; transform-origin: center; }
  .tile video.mirror { transform: scaleX(-1); }
  .tile-ava { width: 100%; height: 100%; display: grid; place-items: center; }
  .tile-tag {
    position: absolute; left: 8px; bottom: 8px;
    display: flex; align-items: center; gap: 6px;
    max-width: calc(100% - 16px);
    background: rgba(0, 0, 0, 0.6); color: #fff;
    padding: 3px 8px; border-radius: calc(6px * var(--rk, 1));
    font-size: 12px; font-weight: 600;
  }
  .tile-name { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .tag-ico { width: 14px; height: 14px; color: #ff6b6e; display: grid; place-items: center; }
  .tag-ico :global(svg) { width: 14px; height: 14px; }
  .tile-res {
    position: absolute; right: 8px; top: 8px;
    background: rgba(0, 0, 0, 0.6); color: rgba(255, 255, 255, 0.85);
    padding: 2px 7px; border-radius: calc(6px * var(--rk, 1)); font-size: 11px; font-variant-numeric: tabular-nums;
  }

  .stats {
    position: absolute; left: 24px; top: 10px;
    width: min(460px, calc(100% - 48px));
    background: rgba(0, 0, 0, 0.72);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: calc(10px * var(--rk, 1));
    padding: 8px 10px;
    font-family: var(--font-mono, ui-monospace, monospace);
    font-size: 11px; line-height: 1.6;
    color: rgba(255, 255, 255, 0.85);
    pointer-events: none;
    z-index: 2;
  }
  .stats b { color: var(--accent); font-weight: 700; display: inline-block; min-width: 70px; }
  .stats em { color: rgba(255, 255, 255, 0.5); font-style: normal; }
  .mini .stats { background: var(--bg-input); color: var(--fg-muted); border-color: var(--border); }
  .mini .stats em { color: var(--fg-dim); }
</style>
