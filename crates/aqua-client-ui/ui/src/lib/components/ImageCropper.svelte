<script module lang="ts">
  import { writable } from "svelte/store";

  interface CropRequest {
    src: string;
    /** width / height of the result (1 for avatars, 3 for banners). */
    aspect: number;
    round: boolean;
    outWidth: number;
    title: string;
    resolve: (b: Blob | null) => void;
  }
  const req = writable<CropRequest | null>(null);

  /** Ask the user to frame an image. Resolves to a WEBP blob, or null if cancelled. */
  export function cropImage(src: string, opts: { aspect: number; round?: boolean; outWidth?: number; title?: string }): Promise<Blob | null> {
    return new Promise((resolve) => {
      req.set({
        src, aspect: opts.aspect, round: !!opts.round,
        outWidth: opts.outWidth ?? (opts.aspect >= 2 ? 1500 : 512),
        title: opts.title ?? "Crop image",
        resolve,
      });
    });
  }
</script>

<script lang="ts">
  import { t } from "../i18n";
  let img: HTMLImageElement | undefined = $state();
  let natW = $state(0), natH = $state(0);
  let zoom = $state(1);          // 1 = image just covers the frame
  let ox = $state(0), oy = $state(0); // offset of the image's centre from the frame's centre, in frame px
  let busy = $state(false);

  // Frame size on screen.
  const FW = $derived($req ? ($req.aspect >= 1 ? 440 : 440 * $req.aspect) : 440);
  const FH = $derived($req ? FW / $req.aspect : 440);
  // Scale (screen px per image px) at zoom 1 = cover the frame.
  const base = $derived(natW && natH ? Math.max(FW / natW, FH / natH) : 1);
  const scale = $derived(base * zoom);
  const dispW = $derived(natW * scale);
  const dispH = $derived(natH * scale);

  function clamp() {
    const mx = Math.max(0, (dispW - FW) / 2), my = Math.max(0, (dispH - FH) / 2);
    ox = Math.max(-mx, Math.min(mx, ox));
    oy = Math.max(-my, Math.min(my, oy));
  }

  $effect(() => {
    if ($req) { zoom = 1; ox = 0; oy = 0; natW = 0; natH = 0; }
  });

  function onLoad() {
    if (!img) return;
    natW = img.naturalWidth; natH = img.naturalHeight;
  }

  let drag: { x: number; y: number; ox: number; oy: number } | null = null;
  function down(e: PointerEvent) {
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    drag = { x: e.clientX, y: e.clientY, ox, oy };
  }
  function move(e: PointerEvent) {
    if (!drag) return;
    ox = drag.ox + (e.clientX - drag.x);
    oy = drag.oy + (e.clientY - drag.y);
    clamp();
  }
  function up() { drag = null; }
  function wheel(e: WheelEvent) {
    e.preventDefault();
    zoom = Math.max(1, Math.min(5, zoom * (e.deltaY < 0 ? 1.08 : 1 / 1.08)));
    clamp();
  }

  function finish(b: Blob | null) {
    const r = $req;
    req.set(null);
    r?.resolve(b);
  }

  async function save() {
    const r = $req;
    if (!r || !img || !natW) return;
    busy = true;
    try {
      // Frame → image coordinates.
      const sw = FW / scale, sh = FH / scale;
      const sx = natW / 2 - ox / scale - sw / 2;
      const sy = natH / 2 - oy / scale - sh / 2;
      const outW = Math.min(r.outWidth, Math.round(sw * 2) || r.outWidth);
      const outH = Math.round(outW / r.aspect);
      const c = document.createElement("canvas");
      c.width = outW; c.height = outH;
      const g = c.getContext("2d")!;
      g.imageSmoothingQuality = "high";
      g.drawImage(img, sx, sy, sw, sh, 0, 0, outW, outH);
      const blob = await new Promise<Blob | null>((res) => c.toBlob(res, "image/webp", 0.92));
      finish(blob);
    } finally {
      busy = false;
    }
  }

  function onKey(e: KeyboardEvent) {
    if (!$req) return;
    if (e.key === "Escape") { e.preventDefault(); finish(null); }
    else if (e.key === "Enter") { e.preventDefault(); void save(); }
    else if (e.key === "+" || e.key === "=") { zoom = Math.min(5, zoom * 1.1); clamp(); }
    else if (e.key === "-") { zoom = Math.max(1, zoom / 1.1); clamp(); }
    else if (e.key.startsWith("Arrow")) {
      e.preventDefault();
      const d = e.shiftKey ? 30 : 8;
      if (e.key === "ArrowLeft") ox += d; if (e.key === "ArrowRight") ox -= d;
      if (e.key === "ArrowUp") oy += d; if (e.key === "ArrowDown") oy -= d;
      clamp();
    }
  }
</script>

<svelte:window onkeydown={onKey} />

{#if $req}
  <div class="backdrop" role="presentation">
    <div class="sheet" role="dialog" aria-modal="true" aria-label={$req.title}>
      <div class="title">{$req.title}</div>
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="frame"
        class:round={$req.round}
        style:width="{FW}px"
        style:height="{FH}px"
        onpointerdown={down}
        onpointermove={move}
        onpointerup={up}
        onwheel={wheel}
      >
        <img
          bind:this={img}
          src={$req.src}
          alt=""
          draggable="false"
          onload={onLoad}
          style:width="{dispW}px"
          style:height="{dispH}px"
          style:transform="translate(calc(-50% + {ox}px), calc(-50% + {oy}px))"
        />
        <div class="mask" class:round={$req.round}></div>
      </div>
      <div class="zoom">
        <span>−</span>
        <input type="range" min="1" max="5" step="0.01" bind:value={zoom} oninput={clamp} aria-label={$t("Zoom")} />
        <span>+</span>
      </div>
      <div class="hint">{$t("Drag to move · scroll or +/− to zoom · Enter to save")}</div>
      <div class="actions">
        <button class="ghost" onclick={() => finish(null)}>{$t("Cancel")}</button>
        <button class="primary" onclick={save} disabled={busy || !natW}>{$t(busy ? "Saving…" : "Save")}</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop { position: fixed; inset: 0; z-index: 280; background: rgba(6, 7, 12, 0.7); display: grid; place-items: center; }
  .sheet {
    background: var(--bg-shell); border: 1px solid var(--border); border-radius: 16px;
    padding: 18px; display: flex; flex-direction: column; align-items: center; gap: 12px;
    box-shadow: 0 32px 80px -16px rgba(0, 0, 0, 0.7);
  }
  .title { align-self: flex-start; font-weight: 800; font-size: 16px; color: var(--fg); }
  .frame { position: relative; overflow: hidden; border-radius: 12px; background: #000; cursor: grab; touch-action: none; }
  .frame:active { cursor: grabbing; }
  .frame img { position: absolute; left: 50%; top: 50%; max-width: none; user-select: none; pointer-events: none; }
  .mask { position: absolute; inset: 0; pointer-events: none; box-shadow: inset 0 0 0 2px rgba(255, 255, 255, 0.5); border-radius: 12px; }
  .mask.round { border-radius: 50%; box-shadow: 0 0 0 999px rgba(0, 0, 0, 0.55), inset 0 0 0 2px rgba(255, 255, 255, 0.6); }
  .zoom { display: flex; align-items: center; gap: 10px; width: 100%; color: var(--fg-dim); font-weight: 700; }
  .zoom input { flex: 1; accent-color: var(--accent); }
  .hint { font-size: 11.5px; color: var(--fg-dim); }
  .actions { display: flex; gap: 8px; align-self: flex-end; }
  .actions button { border: none; border-radius: 10px; padding: 8px 16px; font-weight: 700; cursor: pointer; font-size: 13.5px; }
  .ghost { background: var(--bg-input); color: var(--fg); }
  .primary { background: var(--accent); color: #06070C; }
  .primary:disabled { opacity: 0.6; cursor: default; }
</style>
