<script module lang="ts">
  import { writable } from "svelte/store";
  let resolver: ((v: string | null) => void) | null = null;
  const open = writable(false);
  /** Open the camera and wait for a QR code; null when cancelled. */
  export function scanQr(): Promise<string | null> {
    resolver?.(null);
    open.set(true);
    return new Promise((r) => (resolver = r));
  }
  function finish(v: string | null) {
    open.set(false);
    const r = resolver; resolver = null; r?.(v);
  }
</script>

<script lang="ts">
  import { t } from "../i18n";
  import { readQr } from "../qr";

  let video = $state<HTMLVideoElement | null>(null);
  let canvas = $state<HTMLCanvasElement | null>(null);
  let error = $state("");
  let pasted = $state("");

  $effect(() => {
    if (!$open || !video) return;
    let stream: MediaStream | null = null;
    let stop = false;
    error = "";
    void (async () => {
      try {
        stream = await navigator.mediaDevices.getUserMedia({ video: { facingMode: "environment" }, audio: false });
        if (stop) { stream.getTracks().forEach((x) => x.stop()); return; }
        video!.srcObject = stream;
        await video!.play().catch(() => {});
        const tick = async () => {
          if (stop || !video || !canvas) return;
          const v = await readQr(video, canvas).catch(() => null);
          if (v) { finish(v); return; }
          setTimeout(tick, 220);
        };
        void tick();
      } catch (e) {
        error = String(e);
      }
    })();
    return () => { stop = true; stream?.getTracks().forEach((x) => x.stop()); };
  });
</script>

{#if $open}
  <div class="qs-bg" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) finish(null); }}>
    <div class="qs" role="dialog" aria-modal="true">
      <div class="qs-head">
        <b>{$t("Scan a QR code")}</b>
        <button class="x" onclick={() => finish(null)} aria-label={$t("Close")}>×</button>
      </div>
      <div class="cam">
        <!-- svelte-ignore a11y_media_has_caption -->
        <video bind:this={video} playsinline muted></video>
        <div class="frame"></div>
        {#if error}<div class="err">{$t("No camera here. Paste the code's text below instead.")}</div>{/if}
      </div>
      <canvas bind:this={canvas} hidden></canvas>
      <div class="paste">
        <input bind:value={pasted} placeholder="aqua:@name" onkeydown={(e) => { if (e.key === "Enter" && pasted.trim()) finish(pasted.trim()); }} />
        <button disabled={!pasted.trim()} onclick={() => finish(pasted.trim())}>{$t("Open")}</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .qs-bg { position: fixed; inset: 0; z-index: 400; background: rgba(4, 6, 10, 0.72); backdrop-filter: blur(6px); display: grid; place-items: center; padding: 16px; }
  .qs { width: min(420px, 100%); background: var(--bg-surface); border: 1px solid var(--border-strong); border-radius: calc(18px * var(--rk, 1)); padding: 14px; display: flex; flex-direction: column; gap: 12px; }
  .qs-head { display: flex; align-items: center; justify-content: space-between; }
  .x { border: none; background: none; color: var(--fg-muted); font-size: 22px; cursor: pointer; }
  .cam { position: relative; aspect-ratio: 1; border-radius: calc(14px * var(--rk, 1)); overflow: hidden; background: #000; }
  video { width: 100%; height: 100%; object-fit: cover; }
  .frame { position: absolute; inset: 18%; border: 3px solid var(--accent); border-radius: calc(18px * var(--rk, 1)); box-shadow: 0 0 0 999px rgba(0,0,0,0.35); }
  .err { position: absolute; inset: auto 12px 12px; color: #fff; font-size: 13px; text-align: center; background: rgba(0,0,0,.6); border-radius: calc(10px * var(--rk, 1)); padding: 8px; }
  .paste { display: flex; gap: 8px; }
  .paste input { flex: 1; min-width: 0; background: var(--bg-input); border: 1px solid var(--border); border-radius: calc(10px * var(--rk, 1)); color: var(--fg); padding: 9px 12px; font: inherit; }
  .paste button { border: none; border-radius: calc(10px * var(--rk, 1)); padding: 0 16px; font: inherit; font-weight: 700; background: var(--accent); color: #06070C; cursor: pointer; }
  .paste button:disabled { opacity: .5; cursor: default; }
</style>
