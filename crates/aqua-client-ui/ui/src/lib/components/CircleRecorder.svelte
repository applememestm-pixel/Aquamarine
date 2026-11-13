<script module lang="ts">
  import { writable, get as getStore } from "svelte/store";

  /** hold: opened by holding the composer's camera button (Telegram):
   *  recording starts as soon as the camera is up, and letting go sends. */
  interface State { peerId: string; hold?: boolean; }
  export const circleState = writable<State | null>(null);
  /** The finger on the composer's button: "down" (holding), "send" (let go:
   *  stop and send), "lock" (slid up: keep recording hands-free), "cancel"
   *  (slid left: throw it away). */
  export const circleHold = writable<"down" | "send" | "lock" | "cancel">("down");
  /** How far the finger moved from where it went down (px) — drives the
   *  "slide to cancel" and lock hints, like Telegram. */
  export const circleDrag = writable<{ x: number; y: number }>({ x: 0, y: 0 });

  export function openCircleRecorder(peerId: string, hold = false) {
    circleHold.set("down");
    circleDrag.set({ x: 0, y: 0 });
    circleState.set({ peerId, hold });
  }
  export function releaseCircleHold(lock = false) {
    if (getStore(circleState)?.hold) circleHold.set(lock ? "lock" : "send");
  }
  export function cancelCircleHold() {
    if (getStore(circleState)?.hold) circleHold.set("cancel");
  }
  export function closeCircleRecorder() { circleState.set(null); }
  /** Finger this far left of where it went down: the recording is dropped. */
  export const CANCEL_PX = 120;
</script>

<script lang="ts">
  import { t, tr } from "../i18n";
  import { startRecorder, VIDEO_TYPES, extFor } from "../recorder";
  import { onMount, onDestroy } from "svelte";
  import { api } from "../tauri";
  import { loadActiveMessages } from "../stores";
  import { showToast } from "./Toast.svelte";

  const MAX_SECONDS = 60;
  /** Side of the recorded square (the circle is cut from it on playback). */
  const SIDE = 480;

  let live = $state<HTMLVideoElement | null>(null);
  let stream: MediaStream | null = null;
  let recorder: MediaRecorder | null = null;
  let chunks: Blob[] = [];
  let discard = false;
  // Frames go through a canvas: a square clip (no wasted pixels outside the
  // circle) and the camera can be switched mid-recording, as in Telegram.
  let canvas: HTMLCanvasElement | null = null;
  let raf = 0;

  let phase = $state<"preview" | "recording" | "review">("preview");
  let facing = $state<"user" | "environment">("user");
  let canFlip = $state(false);
  let flipping = $state(false);
  let ready = $state(false);
  let startedAt = 0;
  let elapsedMs = $state(0);
  let timer: ReturnType<typeof setInterval> | null = null;
  let recordedUrl = $state<string | null>(null);
  let recordedBlob: Blob | null = null;
  let sending = $state(false);
  let errorMsg = $state<string | null>(null);
  let reviewVid = $state<HTMLVideoElement | null>(null);
  let reviewPaused = $state(false);

  function videoConstraints(f: "user" | "environment"): MediaTrackConstraints {
    return { width: { ideal: 720 }, height: { ideal: 720 }, facingMode: f };
  }

  async function openCamera() {
    const want = $circleState;
    try {
      const s = await navigator.mediaDevices.getUserMedia({ video: videoConstraints(facing), audio: true });
      // Closed while the camera was coming up: before, it stayed on.
      if (!$circleState || $circleState !== want) { s.getTracks().forEach((x) => x.stop()); return; }
      stream = s;
      attach();
      ready = true;
      void navigator.mediaDevices.enumerateDevices()
        .then((d) => { canFlip = d.filter((x) => x.kind === "videoinput").length > 1; })
        .catch(() => {});
      if ($circleState?.hold) {
        // Let go (or slid away) before the camera came up: too short to record.
        if ($circleHold === "send") { close(); showToast(tr("Hold the button to record a video message")); return; }
        if ($circleHold === "cancel") { close(); return; }
        void startRecording();
      }
    } catch (e) {
      if (!$circleState) return;
      errorMsg = tr("Camera access denied: {e}", { e: String(e) });
    }
  }

  function attach() {
    if (live && stream) {
      live.srcObject = stream;
      live.muted = true;
      void live.play().catch(() => {});
    }
  }
  // The live <video> is there from the start; give it the stream if it
  // mounted after the camera opened.
  $effect(() => { if (live && stream && live.srcObject !== stream) attach(); });

  async function flip() {
    if (!stream || flipping) return;
    flipping = true;
    const next = facing === "user" ? "environment" : "user";
    try {
      const v = await navigator.mediaDevices.getUserMedia({ video: videoConstraints(next) });
      const track = v.getVideoTracks()[0];
      if (!track || !stream) { v.getTracks().forEach((x) => x.stop()); return; }
      for (const old of stream.getVideoTracks()) { stream.removeTrack(old); old.stop(); }
      stream.addTrack(track);
      facing = next;
      attach();
    } catch (e) {
      showToast(`${tr("Switch camera")}: ${e instanceof Error ? e.message : e}`);
    } finally {
      flipping = false;
    }
  }

  /** What the recorder takes: the canvas's square picture + the microphone;
   *  the camera itself where the WebView can't capture a canvas. */
  function recordStream(s: MediaStream): MediaStream {
    try {
      canvas ??= document.createElement("canvas");
      canvas.width = SIDE; canvas.height = SIDE;
      const ctx = canvas.getContext("2d");
      const cap = (canvas as HTMLCanvasElement & { captureStream?: (fps?: number) => MediaStream }).captureStream?.(30);
      if (!ctx || !cap) return s;
      const draw = () => {
        const v = live;
        if (v && v.videoWidth) {
          const side = Math.min(v.videoWidth, v.videoHeight);
          ctx.drawImage(v, (v.videoWidth - side) / 2, (v.videoHeight - side) / 2, side, side, 0, 0, SIDE, SIDE);
        }
        raf = requestAnimationFrame(draw);
      };
      draw();
      return new MediaStream([...cap.getVideoTracks(), ...s.getAudioTracks()]);
    } catch {
      return s;
    }
  }
  let viaCanvas = $state(false);

  async function startRecording() {
    if (!stream || phase === "recording") return;
    chunks = [];
    discard = false;
    try {
      const src = recordStream(stream);
      viaCanvas = src !== stream;
      const st = startRecorder(src, VIDEO_TYPES, (b) => chunks.push(b), 1000, 1_200_000);
      recorder = st.recorder;
      const mimeType = st.mime;
      recorder.onerror = (ev) => { errorMsg = `${tr("Recording failed")}: ${(ev as unknown as { error?: Error }).error?.message ?? "error"}`; };
      recorder.onstop = () => {
        stopTimer();
        cancelAnimationFrame(raf);
        if (discard) { chunks = []; return; }
        const blob = new Blob(chunks, { type: mimeType || "video/webm" });
        if (blob.size === 0) { phase = "preview"; return; }
        recordedBlob = blob;
        recordedUrl = URL.createObjectURL(blob);
        reviewPaused = false;
        phase = "review";
        // Held and let go — or held to the time limit: straight out, like
        // Telegram. Locked: the review stays up.
        if ($circleState?.hold && ($circleHold === "send" || $circleHold === "down")) void sendCircle();
      };
      phase = "recording";
      startedAt = performance.now();
      elapsedMs = 0;
      timer = setInterval(() => {
        elapsedMs = performance.now() - startedAt;
        if (elapsedMs >= MAX_SECONDS * 1000) stopRecording();
      }, 50);
    } catch (e) {
      cancelAnimationFrame(raf);
      errorMsg = `${tr("Could not start recording")}: ${e instanceof Error ? e.message : e}`;
    }
  }

  function stopTimer() { if (timer) { clearInterval(timer); timer = null; } }

  function stopRecording() {
    try { recorder?.stop(); } catch { /* ignore */ }
  }

  function retake() {
    if (recordedUrl) URL.revokeObjectURL(recordedUrl);
    recordedUrl = null;
    recordedBlob = null;
    phase = "preview";
    elapsedMs = 0;
    attach();
  }

  async function sendCircle() {
    const s = $circleState;
    if (!s || !recordedBlob || sending) return;
    sending = true;
    try {
      const buf = await recordedBlob.arrayBuffer();
      const bytes = Array.from(new Uint8Array(buf));
      const name = `circle-${new Date().toISOString().replace(/[:.]/g, "-")}.${extFor(recordedBlob.type, "webm")}`;
      await api.sendCircleVideo(s.peerId, name, recordedBlob.type || "video/webm", bytes);
      await loadActiveMessages();
      close();
    } catch (e) {
      showToast(`${tr("Send failed")}: ${e}`);
      sending = false;
    }
  }

  function close() {
    stopTimer();
    cancelAnimationFrame(raf);
    discard = true;
    try { if (recorder?.state !== "inactive") recorder?.stop(); } catch { /* ignore */ }
    recorder = null;
    stream?.getTracks().forEach((x) => x.stop());
    stream = null;
    if (recordedUrl) URL.revokeObjectURL(recordedUrl);
    recordedUrl = null;
    recordedBlob = null;
    phase = "preview";
    elapsedMs = 0;
    errorMsg = null;
    sending = false;
    ready = false;
    facing = "user";
    closeCircleRecorder();
  }

  // The finger on the composer's button.
  $effect(() => {
    const h = $circleHold;
    if (h === "send" && phase === "recording") stopRecording();
    else if (h === "cancel") close();
  });

  // Open the camera when the recorder opens.
  let opened: State | null = null;
  $effect(() => {
    const s = $circleState;
    if (s && s !== opened) { opened = s; void openCamera(); }
    else if (!s) opened = null;
  });

  onDestroy(() => {
    stopTimer();
    cancelAnimationFrame(raf);
    stream?.getTracks().forEach((x) => x.stop());
    if (recordedUrl) URL.revokeObjectURL(recordedUrl);
  });

  onMount(() => {
    const h = (e: KeyboardEvent) => { if (e.key === "Escape" && $circleState) close(); };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  });

  /** 0:07,4 — Telegram's timer, with tenths. */
  function fmt(ms: number): string {
    const s = Math.floor(ms / 1000);
    return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")},${Math.floor((ms % 1000) / 100)}`;
  }

  function toggleReview() {
    const v = reviewVid;
    if (!v) return;
    if (v.paused) { void v.play().catch(() => {}); reviewPaused = false; } else { v.pause(); reviewPaused = true; }
  }

  // Held and not let go yet: the hints follow the finger.
  let holding = $derived(!!$circleState?.hold && $circleHold === "down");
  let slide = $derived(holding ? Math.min(0, $circleDrag.x) : 0);
  let lift = $derived(holding ? Math.max(0, -$circleDrag.y) : 0);
  let progress = $derived(Math.min(1, elapsedMs / (MAX_SECONDS * 1000)));
  // Ring: r = 49 in a 100-box → circumference ≈ 307.9.
  const RING = 2 * Math.PI * 49;
  // Preview mirrored for the front camera (as you see yourself); the
  // recording isn't, so the review shows what the other side gets.
  let mirror = $derived(facing === "user");
</script>

{#if $circleState}
  <div class="cr" role="dialog" aria-modal="true" aria-label={$t("Record a circle video")} tabindex="-1">
    <button class="cr-dim" aria-label={$t("Close recorder")} tabindex="-1"
      onclick={() => { if (!holding && phase !== "recording") close(); }}></button>

    <div class="cr-stage">
      <div class="cr-circle" class:rec={phase === "recording"}>
        {#if errorMsg}
          <div class="cr-err">{errorMsg}</div>
        {:else}
          <!-- svelte-ignore a11y_media_has_caption -->
          <video class="cr-live" class:mirror class:gone={phase === "review"} bind:this={live} autoplay muted playsinline></video>
          {#if phase === "review" && recordedUrl}
            <!-- svelte-ignore a11y_media_has_caption -->
            <video class="cr-review" src={recordedUrl} bind:this={reviewVid} autoplay loop playsinline onclick={toggleReview}></video>
            {#if reviewPaused}
              <span class="cr-play" aria-hidden="true"><svg viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg></span>
            {/if}
          {/if}
          {#if !ready}<div class="cr-wait"></div>{/if}
        {/if}
        <svg class="cr-ring" viewBox="0 0 100 100" aria-hidden="true">
          <circle cx="50" cy="50" r="49" class="track" />
          {#if phase !== "preview"}
            <circle cx="50" cy="50" r="49" class="fill"
              stroke-dasharray={`${(phase === "review" ? 1 : progress) * RING} ${RING}`}
              transform="rotate(-90 50 50)" />
          {/if}
        </svg>
      </div>

      {#if canFlip && phase !== "review" && (phase === "preview" || viaCanvas)}
        <button class="cr-flip" onclick={flip} disabled={flipping} title={$t("Switch camera")} aria-label={$t("Switch camera")}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M20 7h-3l-2-3H9L7 7H4a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2z"/><path d="M9.2 12.2a3 3 0 0 1 5.3-1.6"/><polyline points="14.8 8.6 14.6 10.8 12.4 10.6"/><path d="M14.8 13.8a3 3 0 0 1-5.3 1.6"/><polyline points="9.2 17.4 9.4 15.2 11.6 15.4"/></svg>
        </button>
      {/if}
    </div>

    {#if holding && phase === "recording"}
      <!-- Slide up to keep recording hands-free. -->
      <div class="cr-lock" style:transform={`translateY(${-Math.min(lift, 40)}px)`} style:opacity={1 - Math.min(lift, 40) / 80}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="5" y="11" width="14" height="10" rx="2"/><path d="M8 11V7a4 4 0 0 1 8 0v4"/></svg>
        <svg class="up" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 15 12 9 18 15"/></svg>
      </div>
    {/if}

    <div class="cr-bar">
      {#if phase === "review"}
        <button class="cr-ic danger" onclick={close} disabled={sending} title={$t("Delete")} aria-label={$t("Delete")}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"/><path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6"/><path d="M10 11v6M14 11v6"/><path d="M9 6V4a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2"/></svg>
        </button>
        <span class="cr-time">{fmt(elapsedMs)}</span>
        <button class="cr-text" onclick={retake} disabled={sending}>{$t("Retake")}</button>
        <button class="cr-big" onclick={sendCircle} disabled={sending} title={$t("Send")} aria-label={$t("Send")}>
          {#if sending}<span class="cr-spin"></span>{:else}
            <svg viewBox="0 0 24 24" fill="currentColor"><path d="M3.4 20.4 21 12 3.4 3.6 3.4 10l12.6 2-12.6 2z"/></svg>
          {/if}
        </button>
      {:else if phase === "recording"}
        <span class="cr-dot"></span>
        <span class="cr-time">{fmt(elapsedMs)}</span>
        {#if holding}
          <span class="cr-slide" style:transform={`translateX(${slide}px)`} style:opacity={1 + slide / CANCEL_PX}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 18 9 12 15 6"/></svg>
            {$t("Slide to cancel")}
          </span>
          <span class="cr-big held" style:transform={`translate(${slide * 0.4}px, ${-Math.min(lift, 40)}px) scale(1.1)`} aria-hidden="true">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M23 7l-7 5 7 5V7z"/><rect x="1" y="5" width="15" height="14" rx="2"/></svg>
          </span>
        {:else}
          <button class="cr-text" onclick={close}>{$t("Cancel")}</button>
          <button class="cr-ic" onclick={stopRecording} title={$t("Stop")} aria-label={$t("Stop")}>
            <svg viewBox="0 0 24 24" fill="currentColor"><rect x="6.5" y="6.5" width="11" height="11" rx="2.2"/></svg>
          </button>
          <button class="cr-big" onclick={() => { circleHold.set("send"); stopRecording(); }} title={$t("Send")} aria-label={$t("Send")}>
            <svg viewBox="0 0 24 24" fill="currentColor"><path d="M3.4 20.4 21 12 3.4 3.6 3.4 10l12.6 2-12.6 2z"/></svg>
          </button>
        {/if}
      {:else}
        <button class="cr-text" onclick={close}>{$t("Cancel")}</button>
        <span class="cr-hint">{$t("Up to {n} seconds", { n: MAX_SECONDS })}</span>
        <button class="cr-big rec" onclick={startRecording} disabled={!ready || !!errorMsg} title={$t("Start recording")} aria-label={$t("Start recording")}>
          <span class="cr-recdot"></span>
        </button>
      {/if}
    </div>
  </div>
{/if}

<style>
  .cr {
    position: fixed; inset: 0; z-index: 125;
    display: flex; flex-direction: column;
    animation: cr-in 0.2s ease-out;
    touch-action: none;
  }
  @keyframes cr-in { from { opacity: 0; } to { opacity: 1; } }
  .cr-dim {
    position: absolute; inset: 0; border: none; padding: 0; cursor: default;
    background: color-mix(in srgb, var(--bg-shell) 55%, rgba(0, 0, 0, 0.55));
    backdrop-filter: blur(14px) saturate(1.2);
    -webkit-backdrop-filter: blur(14px) saturate(1.2);
  }
  .cr-stage {
    position: relative; flex: 1; min-height: 0;
    display: grid; place-items: center;
    pointer-events: none;
    padding: max(24px, env(safe-area-inset-top)) 16px 8px;
  }
  .cr-circle {
    --d: min(78vw, 58vh, 380px);
    position: relative; width: var(--d); height: var(--d);
    border-radius: 50%;
    pointer-events: auto;
    animation: cr-pop 0.28s cubic-bezier(.2, 1.3, .4, 1);
  }
  @keyframes cr-pop { from { transform: scale(0.4); opacity: 0; } to { transform: scale(1); opacity: 1; } }
  .cr-circle > video, .cr-wait, .cr-err {
    position: absolute; inset: 5px;
    width: calc(100% - 10px); height: calc(100% - 10px);
    border-radius: 50%; object-fit: cover; background: #000;
  }
  .cr-live.mirror { transform: scaleX(-1); }
  .cr-live.gone { visibility: hidden; }
  .cr-review { cursor: pointer; }
  .cr-wait { background: radial-gradient(circle at 50% 40%, #1d2130, #050608); animation: cr-breathe 1.2s ease-in-out infinite; }
  @keyframes cr-breathe { 50% { opacity: 0.6; } }
  .cr-err {
    display: grid; place-items: center; padding: 28px; text-align: center;
    color: #fff; font-size: 13px; background: #15080b;
  }
  .cr-play {
    position: absolute; left: 50%; top: 50%; transform: translate(-50%, -50%);
    width: 56px; height: 56px; border-radius: 50%; display: grid; place-items: center;
    background: rgba(0, 0, 0, 0.45); color: #fff; pointer-events: none;
  }
  .cr-play svg { width: 26px; height: 26px; margin-left: 3px; }
  .cr-ring { position: absolute; inset: 0; width: 100%; height: 100%; pointer-events: none; overflow: visible; }
  .cr-ring circle { fill: none; stroke-width: 1.6; }
  .cr-ring .track { stroke: color-mix(in srgb, var(--fg) 18%, transparent); }
  .cr-ring .fill { stroke: var(--accent); stroke-linecap: round; transition: stroke-dasharray 0.05s linear; }

  .cr-flip {
    position: absolute; pointer-events: auto;
    left: calc(50% - min(78vw, 58vh, 380px) / 2 - 4px);
    top: calc(50% + min(78vw, 58vh, 380px) / 2 - 44px);
    width: 44px; height: 44px; border-radius: 50%; border: none; cursor: pointer;
    display: grid; place-items: center;
    background: color-mix(in srgb, var(--bg-shell) 70%, transparent);
    color: var(--fg); box-shadow: 0 4px 14px rgba(0, 0, 0, 0.3);
    backdrop-filter: blur(10px); -webkit-backdrop-filter: blur(10px);
  }
  .cr-flip svg { width: 22px; height: 22px; }
  .cr-flip:disabled { opacity: 0.5; }

  .cr-lock {
    position: absolute; right: 22px; bottom: calc(84px + env(safe-area-inset-bottom));
    width: 40px; padding: 10px 0 6px; border-radius: 20px;
    display: flex; flex-direction: column; align-items: center; gap: 2px;
    background: var(--bg-shell); color: var(--fg-dim);
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.25);
    pointer-events: none;
  }
  .cr-lock svg { width: 18px; height: 18px; }
  .cr-lock .up { width: 14px; height: 14px; animation: cr-up 1.2s ease-in-out infinite; }
  @keyframes cr-up { 50% { transform: translateY(-3px); } }

  .cr-bar {
    position: relative;
    display: flex; align-items: center; gap: 12px;
    min-height: 64px;
    padding: 8px 14px calc(8px + env(safe-area-inset-bottom)) 18px;
    background: var(--bg-shell);
    border-top: 1px solid var(--border);
  }
  .cr-dot {
    width: 10px; height: 10px; border-radius: 50%; flex-shrink: 0;
    background: var(--danger); animation: cr-blink 1s ease-in-out infinite;
  }
  @keyframes cr-blink { 50% { opacity: 0.2; } }
  /* Above the sliding hint, which passes under it. */
  .cr-time { position: relative; z-index: 1; background: var(--bg-shell); padding-right: 8px; font-variant-numeric: tabular-nums; font-size: 15px; color: var(--fg); min-width: 58px; }
  .cr-slide {
    flex: 1; display: inline-flex; align-items: center; justify-content: center; gap: 4px;
    color: var(--fg-dim); font-size: 14px; white-space: nowrap; overflow: hidden;
    animation: cr-nudge 1.4s ease-in-out infinite;
  }
  .cr-slide svg { width: 16px; height: 16px; flex-shrink: 0; }
  @keyframes cr-nudge { 50% { margin-right: 10px; } }
  .cr-hint { flex: 1; text-align: center; color: var(--fg-dim); font-size: 13px; }
  .cr-text {
    flex: 1; border: none; background: none; cursor: pointer;
    color: var(--accent); font-size: 15px; font-weight: 600; padding: 8px;
  }
  .cr-text:first-child { flex: 0 0 auto; }
  .cr-ic {
    width: 40px; height: 40px; flex-shrink: 0; border-radius: 50%; border: none; cursor: pointer;
    display: grid; place-items: center; background: var(--bg-input); color: var(--fg);
  }
  .cr-ic svg { width: 20px; height: 20px; }
  .cr-ic.danger { background: none; color: var(--danger); }
  .cr-big {
    width: 56px; height: 56px; flex-shrink: 0; border-radius: 50%; border: none; cursor: pointer;
    display: grid; place-items: center;
    background: var(--accent); color: var(--bg-shell);
    box-shadow: 0 6px 18px -4px var(--accent-glow);
    transition: transform 0.12s;
  }
  .cr-big svg { width: 24px; height: 24px; }
  .cr-big.held { margin-left: auto; box-shadow: 0 0 0 10px color-mix(in srgb, var(--accent) 22%, transparent), 0 6px 18px -4px var(--accent-glow); }
  .cr-big.rec { background: var(--bg-input); }
  .cr-recdot { width: 24px; height: 24px; border-radius: 50%; background: var(--danger); }
  .cr-spin {
    width: 22px; height: 22px; border-radius: 50%;
    border: 2.5px solid currentColor; border-right-color: transparent;
    animation: cr-rot 0.8s linear infinite;
  }
  @keyframes cr-rot { to { transform: rotate(360deg); } }
  button:disabled { opacity: 0.5; cursor: default; }
</style>
