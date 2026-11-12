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

  async function start() {
    stop();
    err = "";
    try {
      // Full HD asked as a minimum of 720 lines too: some Android WebViews
      // otherwise hand out 176×144 / 320×240 (the "144p" story), and without
      // the microphone the second try used to ask for no size at all.
      const vid: MediaTrackConstraints = { facingMode: { ideal: facing }, width: { ideal: 1920 }, height: { ideal: 1080 }, frameRate: { ideal: 30 } };
      stream = await navigator.mediaDevices.getUserMedia({ video: vid, audio: true })
        .catch(() => navigator.mediaDevices.getUserMedia({ video: vid, audio: false }))
        .catch(() => navigator.mediaDevices.getUserMedia({ video: { facingMode: { ideal: facing } }, audio: false }));
      const tr0 = stream.getVideoTracks()[0];
      const st = tr0?.getSettings?.();
      if (tr0 && st && Math.min(st.width ?? 0, st.height ?? 0) < 720) {
        await tr0.applyConstraints({ width: { min: 1280, ideal: 1920 }, height: { min: 720, ideal: 1080 } }).catch(() => {});
      }
      if (video) { video.srcObject = stream; video.muted = true; void video.play().catch(() => {}); }
    } catch (e) {
      const name = (e as DOMException)?.name;
      err = name === "NotAllowedError" ? tr("No access to the camera — allow it in the system settings")
        : name === "NotFoundError" ? tr("No camera found")
        : `${tr("Camera failed")}: ${e instanceof Error ? e.message : e}`;
    }
  }
  function stop() {
    stream?.getTracks().forEach((x) => x.stop());
    stream = null;
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
    c.width = video.videoWidth;
    c.height = video.videoHeight;
    const g = c.getContext("2d")!;
    // The front camera is shown mirrored; the photo is taken as people see you.
    g.drawImage(video, 0, 0);
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
      const st = startRecorder(stream, VIDEO_TYPES, (b) => chunks.push(b), 1000, 5_000_000);
      recorder = st.recorder;
      const mime = st.mime;
      recorder.onstop = async () => {
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
    {#if flash}<div class="flash"></div>{/if}
    <div class="top">
      <button class="ic" data-x data-keep-x onclick={() => finish(null)} aria-label={$t("Close")}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M6 6l12 12M18 6 6 18"/></svg>
      </button>
      {#if recording}<span class="rec"><i></i>{fmt(secs)}</span>{/if}
      <button class="ic" disabled={recording} onclick={() => (facing = facing === "user" ? "environment" : "user")} aria-label={$t("Switch camera")} title={$t("Switch camera")}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20 7h-3l-2-3H9L7 7H4a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2z"/><path d="M9 13.5a3 3 0 0 1 5.2-2"/><path d="M15 12.5a3 3 0 0 1-5.2 2"/><path d="M14.5 9.5v2h-2M9.5 16.5v-2h2"/></svg>
      </button>
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
