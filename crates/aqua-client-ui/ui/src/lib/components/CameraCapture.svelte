<script module lang="ts">
  import { writable } from "svelte/store";
  interface Ask { resolve: (path: string | null) => void }
  const pending = writable<Ask | null>(null);
  /**
   * The camera in the app (Telegram / AyuGram): tap the shutter for a
   * photo, hold it for a video. Resolves with the saved file's path (it
   * then goes through the usual send sheet: caption, compress, edit), or
   * null when closed.
   */
  export function openCamera(): Promise<string | null> {
    return new Promise((resolve) => pending.set({ resolve }));
  }
</script>

<script lang="ts">
  import { t, tr } from "../i18n";
  import { api } from "../tauri";
  import { onBack } from "../back";
  import { startRecorder, VIDEO_TYPES, extFor } from "../recorder";
  import { IS_MOBILE } from "../platform";
  import { showToast } from "./Toast.svelte";

  let video = $state<HTMLVideoElement | null>(null);
  let stream: MediaStream | null = null;
  let facing = $state<"user" | "environment">(IS_MOBILE ? "environment" : "user");
  let err = $state("");
  let recording = $state(false);
  let secs = $state(0);
  let busy = $state(false);
  let flash = $state(false);
  let recorder: MediaRecorder | null = null;
  let chunks: Blob[] = [];
  let tick: ReturnType<typeof setInterval> | null = null;
  let holdT: ReturnType<typeof setTimeout> | null = null;
  let held = false;
  const MAX = 300;
  // ---- Dual camera (Telegram): the other camera in a small window ----
  let dual = $state(false);
  let video2 = $state<HTMLVideoElement | null>(null);
  let stream2: MediaStream | null = null;
  /** The small window's place (share of the screen, its top-left corner). */
  let pip = $state({ x: 0.04, y: 0.12 });
  let pipDrag: { id: number; dx: number; dy: number } | null = null;
  let mixer: { canvas: HTMLCanvasElement; raf: number } | null = null;

  /** Bumped by every start / stop: a camera that finishes opening after the
   *  window closed or the side was switched again is closed at once (it
   *  stayed on, the light too, until the app was killed). */
  let gen = 0;
  const drop = (s: MediaStream) => s.getTracks().forEach((x) => x.stop());
  async function start() {
    stop();
    const my = gen;
    err = "";
    try {
      // Full HD asked as a minimum of 720 lines too: some Android WebViews
      // otherwise hand out 176×144 / 320×240 (the "144p" story), and without
      // the microphone the second try used to ask for no size at all.
      const vid: MediaTrackConstraints = { facingMode: { ideal: facing }, width: { ideal: 1920 }, height: { ideal: 1080 }, frameRate: { ideal: 30 } };
      stream = await navigator.mediaDevices.getUserMedia({ video: vid, audio: true })
        .catch(() => navigator.mediaDevices.getUserMedia({ video: vid, audio: false }))
        .catch(() => navigator.mediaDevices.getUserMedia({ video: { facingMode: { ideal: facing } }, audio: false }));
      if (my !== gen) { drop(stream); return; }
      const tr0 = stream.getVideoTracks()[0];
      const st = tr0?.getSettings?.();
      if (tr0 && st && Math.min(st.width ?? 0, st.height ?? 0) < 720) {
        await tr0.applyConstraints({ width: { min: 1280, ideal: 1920 }, height: { min: 720, ideal: 1080 } }).catch(() => {});
      }
      if (video) { video.srcObject = stream; video.muted = true; void video.play().catch(() => {}); }
      // A black preview with the camera "in use" (some WebViews): no frames
      // after a moment — ask again with plain constraints.
      const mine = stream;
      setTimeout(async () => {
        if (stream !== mine || my !== gen || !video || video.videoWidth) return;
        try {
          // Only the main camera: the small second one keeps running.
          mine.getTracks().forEach((x) => x.stop());
          stream = null;
          const again = await navigator.mediaDevices.getUserMedia({ video: { facingMode: facing }, audio: true })
            .catch(() => navigator.mediaDevices.getUserMedia({ video: true, audio: false }));
          if (my !== gen) { drop(again); return; }
          stream = again;
          if (video) { video.srcObject = stream; void video.play().catch(() => {}); }
        } catch { /* the error above stays */ }
      }, 1500);
      if (dual) void startSecond();
    } catch (e) {
      const name = (e as DOMException)?.name;
      err = name === "NotAllowedError" ? tr("No access to the camera — allow it in the system settings")
        : name === "NotFoundError" ? tr("No camera found")
        : `${tr("Camera failed")}: ${e instanceof Error ? e.message : e}`;
    }
  }
  function stop() {
    gen++;
    stream?.getTracks().forEach((x) => x.stop());
    stream = null;
    stopSecond();
  }
  function stopSecond() {
    stream2?.getTracks().forEach((x) => x.stop());
    stream2 = null;
  }
  async function startSecond() {
    stopSecond();
    const my = gen;
    try {
      const s2 = await navigator.mediaDevices.getUserMedia({ video: { facingMode: { exact: facing === "user" ? "environment" : "user" }, width: { ideal: 720 }, height: { ideal: 720 } }, audio: false });
      if (my !== gen || !dual) { drop(s2); return; }
      stream2?.getTracks().forEach((x) => x.stop());
      stream2 = s2;
      if (video2) { video2.srcObject = stream2; video2.muted = true; void video2.play().catch(() => {}); }
    } catch {
      dual = false;
      showToast(tr("This device can't use both cameras at once"));
    }
  }
  function toggleDual() {
    dual = !dual;
    if (dual) void startSecond(); else stopSecond();
  }
  $effect(() => { if (video2 && stream2 && video2.srcObject !== stream2) { video2.srcObject = stream2; void video2.play().catch(() => {}); } });
  function pipDown(e: PointerEvent) {
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    const r = el.getBoundingClientRect();
    pipDrag = { id: e.pointerId, dx: e.clientX - r.left, dy: e.clientY - r.top };
    e.stopPropagation();
  }
  function pipMove(e: PointerEvent) {
    if (!pipDrag || e.pointerId !== pipDrag.id) return;
    const el = e.currentTarget as HTMLElement;
    pip = { x: Math.max(0, Math.min(1 - el.offsetWidth / innerWidth, (e.clientX - pipDrag.dx) / innerWidth)), y: Math.max(0, Math.min(1 - el.offsetHeight / innerHeight, (e.clientY - pipDrag.dy) / innerHeight)) };
  }
  function pipUp() { pipDrag = null; }
  /** Draw the screen as seen: the main camera filling it, the small one where it sits. */
  function compose(g: CanvasRenderingContext2D, w: number, h: number) {
    if (!video) return;
    const vw = video.videoWidth, vh = video.videoHeight;
    const k = Math.max(w / vw, h / vh);
    g.save();
    if (facing === "user") { g.translate(w, 0); g.scale(-1, 1); }
    g.drawImage(video, (w - vw * k) / 2, (h - vh * k) / 2, vw * k, vh * k);
    g.restore();
    if (dual && video2 && video2.videoWidth) {
      const pw = Math.round(w * 0.32), ph = Math.round(pw * 4 / 3);
      const px = Math.round(pip.x * w), py = Math.round(pip.y * h);
      const sw = video2.videoWidth, sh = video2.videoHeight, kk = Math.max(pw / sw, ph / sh);
      g.save();
      const rr = pw * 0.12;
      g.beginPath(); g.roundRect(px, py, pw, ph, rr); g.clip();
      if (facing !== "user") { g.translate(px * 2 + pw, 0); g.scale(-1, 1); }
      g.drawImage(video2, px + (pw - sw * kk) / 2, py + (ph - sh * kk) / 2, sw * kk, sh * kk);
      g.restore();
      g.lineWidth = Math.max(2, w / 220); g.strokeStyle = "rgba(255,255,255,.9)";
      g.beginPath(); g.roundRect(px, py, pw, ph, rr); g.stroke();
    }
  }
  $effect(() => {
    if (!$pending) return;
    void facing;
    if (video) void start();
    return () => stop();
  });
  $effect(() => { if ($pending) return onBack(() => finish(null)); });

  function finish(path: string | null) {
    const a = $pending;
    if (mixer) { cancelAnimationFrame(mixer.raf); mixer = null; }
    if (tick) { clearInterval(tick); tick = null; }
    if (holdT) { clearTimeout(holdT); holdT = null; }
    if (recording) { recording = false; try { recorder!.onstop = null; recorder?.stop(); } catch { /* gone */ } }
    stop();
    busy = false;
    secs = 0;
    pending.set(null);
    a?.resolve(path);
  }
  const stamp = () => new Date().toISOString().replace(/[:.]/g, "-");

  async function photo() {
    if (!video || !video.videoWidth || busy) return;
    busy = true;
    flash = true;
    setTimeout(() => (flash = false), 180);
    const c = document.createElement("canvas");
    const g = c.getContext("2d")!;
    if (dual) {
      // Both cameras: the photo is the screen as it looks (portrait, the small window in place).
      const sc = 1920 / Math.max(innerWidth, innerHeight);
      c.width = Math.round(innerWidth * sc); c.height = Math.round(innerHeight * sc);
      compose(g, c.width, c.height);
    } else {
      c.width = video.videoWidth;
      c.height = video.videoHeight;
      // The front camera is shown mirrored; the photo is taken as people see you.
      g.drawImage(video, 0, 0);
    }
    const blob = await new Promise<Blob | null>((r) => c.toBlob(r, "image/jpeg", 0.92));
    if (!blob) { busy = false; return; }
    try {
      finish(await api.saveTempFile(`photo-${stamp()}.jpg`, Array.from(new Uint8Array(await blob.arrayBuffer()))));
    } catch (e) { err = String(e); busy = false; }
  }
  function record() {
    if (!stream) return;
    chunks = [];
    try {
      let src = stream;
      if (dual) {
        // Both cameras: record a canvas drawn every frame, with the main stream's sound.
        const c = document.createElement("canvas");
        const w = 720, h = Math.round(720 * innerHeight / innerWidth / 2) * 2;
        c.width = w; c.height = h;
        const g = c.getContext("2d")!;
        const draw = () => { compose(g, w, h); if (mixer) mixer.raf = requestAnimationFrame(draw); };
        mixer = { canvas: c, raf: requestAnimationFrame(draw) };
        const cs = (c as HTMLCanvasElement & { captureStream: (fps: number) => MediaStream }).captureStream(30);
        stream.getAudioTracks().forEach((t0) => cs.addTrack(t0));
        src = cs;
      }
      const st = startRecorder(src, VIDEO_TYPES, (b) => chunks.push(b), 1000, 5_000_000);
      recorder = st.recorder;
      const mime = st.mime;
      recorder.onstop = async () => {
        if (mixer) { cancelAnimationFrame(mixer.raf); mixer = null; }
        const blob = new Blob(chunks, { type: mime || "video/webm" });
        if (!blob.size) { recording = false; return; }
        busy = true;
        try {
          finish(await api.saveTempFile(`video-${stamp()}.${extFor(blob.type, "webm")}`, Array.from(new Uint8Array(await blob.arrayBuffer()))));
        } catch (e) { err = String(e); busy = false; }
      };
      recording = true;
      secs = 0;
      tick = setInterval(() => { secs += 1; if (secs >= MAX) stopRec(); }, 1000);
    } catch (e) { err = e instanceof Error ? e.message : String(e); }
  }
  function stopRec() {
    if (tick) { clearInterval(tick); tick = null; }
    recording = false;
    try { recorder?.stop(); } catch { /* gone */ }
  }
  function down(e: PointerEvent) {
    if (e.button !== 0 || busy) return;
    if (recording) { stopRec(); return; } // a tap while recording (locked) stops it
    held = false;
    holdT = setTimeout(() => { held = true; record(); }, 300);
  }
  function up() {
    if (holdT) { clearTimeout(holdT); holdT = null; }
    if (held) { held = false; if (recording) stopRec(); return; }
    if (!recording) void photo();
  }
  const fmt = (s: number) => `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
</script>

{#if $pending}
  <div class="cam" data-phone-full data-own-back role="dialog" aria-modal="true" aria-label={$t("Camera")}>
    <!-- svelte-ignore a11y_media_has_caption -->
    <video bind:this={video} class:mirror={facing === "user"} playsinline autoplay muted></video>
    {#if dual}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="pip" style:left="{pip.x * 100}%" style:top="{pip.y * 100}%" onpointerdown={pipDown} onpointermove={pipMove} onpointerup={pipUp} onpointercancel={pipUp}>
        <!-- svelte-ignore a11y_media_has_caption -->
        <video bind:this={video2} class:mirror={facing !== "user"} playsinline autoplay muted></video>
      </div>
    {/if}
    {#if flash}<div class="flash"></div>{/if}
    <div class="top">
      <button class="ic" data-x data-keep-x onclick={() => finish(null)} aria-label={$t("Close")}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M6 6l12 12M18 6 6 18"/></svg>
      </button>
      {#if recording}<span class="rec"><i></i>{fmt(secs)}</span>{/if}
      <span class="right">
      {#if IS_MOBILE}
        <button class="ic" class:on={dual} disabled={recording} onclick={toggleDual} aria-pressed={dual} aria-label={$t("Both cameras")} title={$t("Both cameras")}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="2.5" y="5" width="13" height="14" rx="2.5"/><rect x="12" y="9" width="9.5" height="10" rx="2.2" fill="currentColor" fill-opacity=".25"/><circle cx="9" cy="10.5" r="2.2"/></svg>
        </button>
      {/if}
      <button class="ic" disabled={recording} onclick={() => (facing = facing === "user" ? "environment" : "user")} aria-label={$t("Switch camera")} title={$t("Switch camera")}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20 7h-3l-2-3H9L7 7H4a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2z"/><path d="M9 13.5a3 3 0 0 1 5.2-2"/><path d="M15 12.5a3 3 0 0 1-5.2 2"/><path d="M14.5 9.5v2h-2M9.5 16.5v-2h2"/></svg>
      </button>
      </span>
    </div>
    {#if err}<div class="err">{err}</div>{/if}
    <div class="bottom">
      <span class="hint">{$t(recording ? "Release or tap to stop" : "Tap for a photo, hold for a video")}</span>
      <button class="shutter" class:recording disabled={!!err || busy} aria-label={$t("Take a photo")}
        onpointerdown={down} onpointerup={up} onpointercancel={up} oncontextmenu={(e) => e.preventDefault()}><i></i></button>
    </div>
  </div>
{/if}

<style>
  .cam { position: fixed; inset: 0; z-index: 10095; background: #000; color: #fff; display: flex; flex-direction: column; justify-content: space-between; touch-action: none; user-select: none; }
  video { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: cover; background: #000; }
  :global(html:not(.phone)) video { object-fit: contain; }
  video.mirror { transform: scaleX(-1); }
  .flash { position: absolute; inset: 0; background: #fff; opacity: .7; pointer-events: none; animation: fl .18s ease-out forwards; }
  @keyframes fl { to { opacity: 0; } }
  .top { position: relative; display: flex; align-items: center; justify-content: space-between; padding: calc(12px + var(--sat, env(safe-area-inset-top, 0px))) 14px 12px;
    background: linear-gradient(rgba(0,0,0,.45), transparent); }
  .ic { width: 42px; height: 42px; border-radius: 50%; border: none; display: grid; place-items: center; color: #fff; background: rgba(0,0,0,.35); cursor: pointer; }
  .ic:disabled { opacity: .35; }
  .ic.on { background: #fff; color: #000; }
  .right { display: flex; gap: 10px; }
  .pip { position: absolute; z-index: 2; width: 32%; aspect-ratio: 3 / 4; border-radius: calc(16px * var(--rk, 1)); overflow: hidden; border: 2px solid rgba(255,255,255,.9);
    box-shadow: 0 10px 30px -10px rgba(0,0,0,.7); touch-action: none; cursor: grab; animation: pip-in .25s cubic-bezier(.2,1.1,.3,1); }
  .pip video { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: cover; }
  @keyframes pip-in { from { transform: scale(.6); opacity: 0; } }
  .ic svg { width: 22px; height: 22px; }
  .rec { display: flex; align-items: center; gap: 7px; font: 700 15px var(--font-ui, inherit); font-variant-numeric: tabular-nums; padding: 5px 12px; border-radius: calc(14px * var(--rk, 1)); background: rgba(0,0,0,.45); }
  .rec i { width: 9px; height: 9px; border-radius: 50%; background: #ff3b30; animation: blink 1s steps(2) infinite; }
  @keyframes blink { 50% { opacity: 0; } }
  .err { position: relative; margin: 0 20px; padding: 12px 14px; border-radius: calc(12px * var(--rk, 1)); background: rgba(0,0,0,.6); font-size: 14px; text-align: center; }
  .bottom { position: relative; display: flex; flex-direction: column; align-items: center; gap: 14px; padding: 20px 0 calc(28px + var(--sab, env(safe-area-inset-bottom, 0px)));
    background: linear-gradient(transparent, rgba(0,0,0,.5)); }
  .hint { font-size: 13px; opacity: .85; text-shadow: 0 1px 3px rgba(0,0,0,.6); }
  .shutter { width: 76px; height: 76px; border-radius: 50%; border: 4px solid #fff; background: transparent; padding: 0; display: grid; place-items: center; cursor: pointer; transition: transform .15s ease; }
  .shutter i { width: 60px; height: 60px; border-radius: 50%; background: #fff; transition: all .2s cubic-bezier(.2,.9,.3,1.2); }
  .shutter:active { transform: scale(.94); }
  .shutter.recording { transform: scale(1.12); border-color: rgba(255,255,255,.7); }
  .shutter.recording i { width: 30px; height: 30px; border-radius: calc(8px * var(--rk, 1)); background: #ff3b30; }
  .shutter:disabled { opacity: .45; }
</style>
