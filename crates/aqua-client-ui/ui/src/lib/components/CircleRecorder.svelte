<script module lang="ts">
  import { writable, get as getStore } from "svelte/store";

  interface State { peerId: string; }
  export const circleState = writable<State | null>(null);

  export function openCircleRecorder(peerId: string) {
    circleState.set({ peerId });
  }
  export function closeCircleRecorder() { circleState.set(null); }
</script>

<script lang="ts">
  import { t, tr } from "../i18n";
  import { startRecorder, VIDEO_TYPES, extFor } from "../recorder";
  import { onMount, onDestroy } from "svelte";
  import { api } from "../tauri";
  import { loadActiveMessages } from "../stores";
  import { showToast } from "./Toast.svelte";

  const MAX_SECONDS = 60;

  let video = $state<HTMLVideoElement | null>(null);
  let stream: MediaStream | null = null;
  let recorder: MediaRecorder | null = null;
  let chunks: Blob[] = [];

  let phase = $state<"preview" | "recording" | "review">("preview");
  let elapsed = $state(0);
  let timer: ReturnType<typeof setInterval> | null = null;
  let recordedUrl = $state<string | null>(null);
  let recordedBlob: Blob | null = null;
  let sending = $state(false);
  let errorMsg = $state<string | null>(null);

  async function openCamera() {
    try {
      const s = await navigator.mediaDevices.getUserMedia({
        video: { width: { ideal: 480 }, height: { ideal: 480 }, facingMode: "user" },
        audio: true,
      });
      stream = s;
      if (video) {
        video.srcObject = s;
        video.muted = true;
        void video.play();
      }
    } catch (e) {
      errorMsg = `Camera access denied: ${e}`;
    }
  }

  function stopStream() {
    stream?.getTracks().forEach((t) => t.stop());
    stream = null;
  }

  async function startRecording() {
    if (!stream) return;
    chunks = [];
    try {
      const st = startRecorder(stream, VIDEO_TYPES, (b) => chunks.push(b), 1000);
      recorder = st.recorder;
      const mimeType = st.mime;
      recorder.onerror = (ev) => { errorMsg = `${tr("Recording failed")}: ${(ev as unknown as { error?: Error }).error?.message ?? "error"}`; };
      recorder.onstop = () => {
        if (timer) { clearInterval(timer); timer = null; }
        const blob = new Blob(chunks, { type: mimeType || "video/webm" });
        if (blob.size === 0) {
          phase = "preview";
          return;
        }
        recordedBlob = blob;
        recordedUrl = URL.createObjectURL(blob);
        phase = "review";
      };
      phase = "recording";
      elapsed = 0;
      timer = setInterval(() => {
        elapsed += 1;
        if (elapsed >= MAX_SECONDS) stopRecording();
      }, 1000);
    } catch (e) {
      errorMsg = `${tr("Could not start recording")}: ${e instanceof Error ? e.message : e}`;
    }
  }

  function stopRecording() {
    try { recorder?.stop(); } catch { /* ignore */ }
  }

  function retake() {
    if (recordedUrl) URL.revokeObjectURL(recordedUrl);
    recordedUrl = null;
    recordedBlob = null;
    phase = "preview";
  }

  async function sendCircle() {
    const s = $circleState;
    if (!s || !recordedBlob) return;
    sending = true;
    try {
      const buf = await recordedBlob.arrayBuffer();
      const bytes = Array.from(new Uint8Array(buf));
      const name = `circle-${new Date().toISOString().replace(/[:.]/g, "-")}.${extFor(recordedBlob.type, "webm")}`;
      await api.sendCircleVideo(s.peerId, name, recordedBlob.type || "video/webm", bytes);
      await loadActiveMessages();
      close();
    } catch (e) {
      showToast(`Send failed: ${e}`);
      sending = false;
    }
  }

  function close() {
    if (timer) { clearInterval(timer); timer = null; }
    try { recorder?.stop(); } catch { /* ignore */ }
    stopStream();
    if (recordedUrl) URL.revokeObjectURL(recordedUrl);
    recordedUrl = null;
    recordedBlob = null;
    phase = "preview";
    elapsed = 0;
    errorMsg = null;
    sending = false;
    closeCircleRecorder();
  }

  // Open camera when the recorder opens.
  let prev = "";
  $effect(() => {
    const s = $circleState;
    if (s && s.peerId !== prev) {
      prev = s.peerId;
      void openCamera();
    } else if (!s) {
      prev = "";
    }
  });

  onDestroy(() => {
    if (timer) clearInterval(timer);
    stopStream();
    if (recordedUrl) URL.revokeObjectURL(recordedUrl);
  });

  onMount(() => {
    const h = (e: KeyboardEvent) => { if (e.key === "Escape") close(); };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  });

  function fmt(s: number): string {
    const m = Math.floor(s / 60); const ss = s % 60;
    return `${m}:${String(ss).padStart(2, "0")}`;
  }
</script>

{#if $circleState}
  <div
    class="backdrop"
    role="button"
    tabindex="-1"
    aria-label={$t("Close recorder")}
    onclick={(e) => { if ((e.target as HTMLElement).classList.contains("backdrop")) close(); }}
    onkeydown={(e) => { if (e.key === "Escape") close(); }}
  >
    <div class="sheet" role="dialog" aria-modal="true" tabindex="-1">
      <div class="head">
        <h2>{$t("Record a circle video")}</h2>
        <button class="close" onclick={close} title={$t("Close (Esc)")}>×</button>
      </div>

      <div class="stage">
        {#if errorMsg}
          <div class="error">{errorMsg}</div>
        {:else}
          <div class="circle">
            {#if phase === "review" && recordedUrl}
              <!-- svelte-ignore a11y_media_has_caption -->
              <!-- No native `controls`: loops for review, click toggles play/pause -->
              <video
                src={recordedUrl}
                autoplay loop playsinline
                onclick={(e) => { const v = e.currentTarget as HTMLVideoElement; if (v.paused) v.play(); else v.pause(); }}
              ></video>
            {:else}
              <!-- svelte-ignore a11y_media_has_caption -->
              <video bind:this={video} autoplay muted playsinline></video>
            {/if}
            {#if phase === "recording"}
              <div class="rec-badge">
                <span class="dot"></span> REC {fmt(elapsed)}
              </div>
              <svg class="progress" viewBox="0 0 100 100">
                <circle cx="50" cy="50" r="48" fill="none" stroke="var(--danger)" stroke-width="2"
                  stroke-dasharray={`${(elapsed / MAX_SECONDS) * 301.6} 301.6`}
                  transform="rotate(-90 50 50)" />
              </svg>
            {/if}
          </div>
        {/if}
      </div>

      <div class="tools">
        {#if phase === "preview"}
          <button class="btn-rec" onclick={startRecording} disabled={!stream || !!errorMsg}>
            <span class="rec-dot"></span> {$t("Start recording")}
          </button>
          <div class="hint">Up to {MAX_SECONDS} seconds. Centred selfie-cam view.</div>
        {:else if phase === "recording"}
          <button class="btn-stop" onclick={stopRecording}>{$t("■ Stop")}</button>
          <div class="hint">Recording · auto-stops at {MAX_SECONDS}s</div>
        {:else}
          <div class="review-row">
            <button class="btn-ghost" onclick={retake} disabled={sending}>{$t("Retake")}</button>
            <button class="btn-primary" onclick={sendCircle} disabled={sending}>
              {$t(sending ? "Sending…" : "Send circle")}
            </button>
          </div>
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed; inset: 0;
    background: rgba(6, 7, 12, 0.75);
    backdrop-filter: blur(4px);
    display: grid; place-items: center;
    z-index: 125;
    padding: 20px;
  }
  .sheet {
    width: min(440px, 94vw);
    background: var(--bg-shell);
    border: 1px solid var(--border);
    border-radius: 16px;
    box-shadow: 0 32px 80px -16px rgba(0,0,0,0.6);
    overflow: hidden;
    animation: slide-up 0.22s ease-out;
    display: flex; flex-direction: column;
  }
  @keyframes slide-up {
    from { opacity: 0; transform: translateY(12px); }
    to   { opacity: 1; transform: translateY(0); }
  }
  .head {
    padding: 12px 18px;
    display: flex; justify-content: space-between; align-items: center;
    border-bottom: 1px solid var(--border);
  }
  .head h2 { margin: 0; font-size: 15px; font-weight: 700; }
  .close { width: 30px; height: 30px; background: transparent; border: 1px solid var(--border); border-radius: 8px; color: var(--fg-dim); font-size: 18px; cursor: pointer; }

  .stage { padding: 24px; display: grid; place-items: center; background: #06080f; }
  .error { color: var(--danger); font-size: 13px; text-align: center; padding: 40px; }
  .circle {
    position: relative;
    width: 320px; height: 320px;
    border-radius: 50%;
    overflow: hidden;
    background: #000;
    box-shadow: 0 0 0 4px color-mix(in srgb, var(--accent) 30%, transparent), 0 0 40px -4px var(--accent-glow);
  }
  .circle video {
    width: 100%; height: 100%;
    object-fit: cover;
    transform: scaleX(-1); /* mirror selfie preview */
  }
  .rec-badge {
    position: absolute; top: 14px; left: 50%; transform: translateX(-50%);
    display: inline-flex; align-items: center; gap: 6px;
    padding: 4px 10px;
    background: rgba(0,0,0,0.6);
    border-radius: 100px;
    color: #fff;
    font-weight: 700; font-size: 11.5px;
    font-variant-numeric: tabular-nums;
    letter-spacing: 0.04em;
  }
  .rec-badge .dot {
    width: 8px; height: 8px;
    border-radius: 50%;
    background: var(--danger);
    box-shadow: 0 0 6px var(--danger);
    animation: rec-blink 1s ease-in-out infinite;
  }
  @keyframes rec-blink { 0%,100% { opacity: 1; } 50% { opacity: 0.3; } }
  .progress {
    position: absolute; inset: 0;
    width: 100%; height: 100%;
    pointer-events: none;
  }

  .tools { padding: 16px 18px; border-top: 1px solid var(--border); display: flex; flex-direction: column; gap: 8px; align-items: center; }
  .btn-rec, .btn-stop {
    padding: 10px 24px;
    border: none;
    border-radius: 100px;
    font-size: 14px;
    font-weight: 700;
    cursor: pointer;
    display: inline-flex; align-items: center; gap: 8px;
  }
  .btn-rec {
    background: linear-gradient(135deg, var(--accent), var(--accent-2));
    color: #06070C;
    box-shadow: 0 6px 20px -4px var(--accent-glow);
  }
  .btn-rec:hover:not(:disabled) { transform: translateY(-1px); }
  .btn-rec .rec-dot {
    width: 10px; height: 10px; border-radius: 50%;
    background: var(--danger);
  }
  .btn-stop {
    background: var(--danger);
    color: #fff;
    box-shadow: 0 6px 20px -4px rgba(255,77,109,0.5);
    animation: rec-pulse 1s ease-in-out infinite;
  }
  @keyframes rec-pulse { 0%,100% { box-shadow: 0 6px 20px -4px rgba(255,77,109,0.5); } 50% { box-shadow: 0 6px 24px -2px rgba(255,77,109,0.8); } }
  .hint { font-size: 11.5px; color: var(--fg-dim); }

  .review-row { display: flex; gap: 10px; }
  .btn-ghost, .btn-primary {
    padding: 8px 20px;
    border-radius: 10px;
    font-size: 13px;
    font-weight: 700;
    cursor: pointer;
    border: none;
  }
  .btn-ghost { background: transparent; border: 1px solid var(--border); color: var(--fg-muted); }
  .btn-ghost:hover:not(:disabled) { color: var(--fg); border-color: var(--border-strong); }
  .btn-primary {
    background: linear-gradient(135deg, var(--accent), var(--accent-2));
    color: #06070C;
    box-shadow: 0 6px 16px -4px var(--accent-glow);
  }
  .btn-primary:hover:not(:disabled) { transform: translateY(-1px); }
  button:disabled { opacity: 0.5; cursor: not-allowed; }
</style>
