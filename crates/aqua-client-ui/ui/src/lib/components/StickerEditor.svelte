<script module lang="ts">
  import { writable, get as getStore } from "svelte/store";

  interface State { imageBytes: number[]; mime: string; sourceName: string; }
  export const stickerEditorState = writable<State | null>(null);

  /** Open the sticker editor pre-filled with the bytes of a picked image.
   *  Returns a promise that resolves `true` when a sticker was saved. */
  export function openStickerEditor(imageBytes: number[], mime: string, sourceName: string) {
    stickerEditorState.set({ imageBytes, mime, sourceName });
  }
  export function closeStickerEditor() { stickerEditorState.set(null); }
</script>

<script lang="ts">
  import { t, tr } from "../i18n";
  import { onMount } from "svelte";
  import { api, bytesToDataUrl } from "../tauri";
  import { showToast } from "./Toast.svelte";

  const EMOJIS = [
    "😀","😂","🥹","😍","🥰","😎","🤔","🤯","😴","😭","🤣","🫢",
    "👍","👎","🙌","👏","💪","🤝","🤌","✌️","🫶","🤷",
    "❤️","💔","🔥","✨","⭐","🎉","🎊","💯","🎁","⚡",
    "🤖","👻","🎃","🐱","🐶","🦊","🐻","🐼","🦄","🐸",
    "🍕","🍔","🍟","🌮","🍜","🍣","🍰","☕","🍺","🍷",
    "🌸","🌊","🌞","🌙","🌟","🌈","🗿","💎","🎵","🎸",
  ];

  // Editor state
  let packName = $state("my-stickers");
  let stickerName = $state("sticker");
  let emoji = $state<string>("😀");
  let scale = $state(1);
  let offsetX = $state(0);
  let offsetY = $state(0);
  let processing = $state(false);
  let img = $state<HTMLImageElement | null>(null);
  let canvasEl = $state<HTMLCanvasElement | null>(null);

  const CANVAS_SIZE = 512;

  let imgUrl = $derived<string | null>($stickerEditorState ? bytesToDataUrl($stickerEditorState.imageBytes, $stickerEditorState.mime) : null);

  // Reset state when a new image loads.
  let prev = -1;
  $effect(() => {
    const s = $stickerEditorState;
    if (s && s.imageBytes.length !== prev) {
      prev = s.imageBytes.length;
      packName = "my-stickers";
      stickerName = (s.sourceName || "sticker").replace(/\.[^.]+$/, "").replace(/[^a-zA-Z0-9а-яА-Я_-]/g, "_").slice(0, 32) || "sticker";
      emoji = "😀";
      scale = 1;
      offsetX = 0;
      offsetY = 0;
      processing = false;
    } else if (!s) {
      prev = -1;
    }
  });

  // Live preview of the cropped sticker (square area, image contained).
  // `loaded` bumps when the source image finishes decoding — before that
  // drawImage paints nothing, which is how blank stickers got saved.
  let loaded = $state(0);
  function draw(): boolean {
    if (!canvasEl || !img || !img.complete || !img.naturalWidth) return false;
    const ctx = canvasEl.getContext("2d");
    if (!ctx) return false;
    if (canvasEl.width !== CANVAS_SIZE) canvasEl.width = CANVAS_SIZE;
    if (canvasEl.height !== CANVAS_SIZE) canvasEl.height = CANVAS_SIZE;
    ctx.clearRect(0, 0, CANVAS_SIZE, CANVAS_SIZE);
    const iw = img.naturalWidth, ih = img.naturalHeight;
    // Fit image into the canvas by its longest side at scale=1.
    const s = (CANVAS_SIZE / Math.max(iw, ih)) * scale;
    const w = iw * s;
    const h = ih * s;
    ctx.drawImage(img, (CANVAS_SIZE - w) / 2 + offsetX, (CANVAS_SIZE - h) / 2 + offsetY, w, h);
    return true;
  }
  $effect(() => {
    void loaded; void scale; void offsetX; void offsetY; void imgUrl;
    draw();
  });

  async function save() {
    const s = $stickerEditorState;
    if (!s || !canvasEl) return;
    if (!packName.trim() || !stickerName.trim()) { showToast(tr("Pack and name required")); return; }
    if (!draw()) { showToast(tr("The picture is still loading — try again in a moment")); return; }
    processing = true;
    try {
      const blob: Blob = await new Promise((res, rej) =>
        canvasEl!.toBlob((b) => (b ? res(b) : rej(new Error("toBlob failed"))), "image/png"),
      );
      const buf = await blob.arrayBuffer();
      const bytes = Array.from(new Uint8Array(buf));
      // Prefix emoji_ onto the filename so the media scanner picks up
      // the emoji tag on next scan.
      const safeName = `${emoji}_${stickerName.trim()}.png`;
      await api.saveSticker(packName.trim(), safeName, bytes);
      showToast(`Saved to pack "${packName.trim()}"`);
      closeStickerEditor();
    } catch (e) {
      showToast(`Save failed: ${e}`);
      processing = false;
    }
  }

  onMount(() => {
    const h = (e: KeyboardEvent) => { if (e.key === "Escape") closeStickerEditor(); };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  });
</script>

{#if $stickerEditorState}
  <div
    class="backdrop"
    role="button"
    tabindex="-1"
    aria-label={$t("Close editor")}
    onclick={(e) => { if ((e.target as HTMLElement).classList.contains("backdrop")) closeStickerEditor(); }}
    onkeydown={(e) => { if (e.key === "Escape") closeStickerEditor(); }}
  >
    <div class="sheet" role="dialog" aria-modal="true" tabindex="-1">
      <div class="head">
        <h2>{$t("Create sticker")}</h2>
        <button class="close" onclick={closeStickerEditor} title={$t("Close")}>×</button>
      </div>

      <div class="body">
        {#if imgUrl}
          <!-- off-canvas image used as the draw source -->
          <img bind:this={img} src={imgUrl} alt="" class="hidden" onload={() => loaded++} />
        {/if}
        <div class="stage">
          <canvas bind:this={canvasEl}></canvas>
        </div>

        <div class="controls">
          <div class="slider">
            <label>{$t("Zoom")} <span>{Math.round(scale * 100)}%</span></label>
            <input type="range" min="0.5" max="3" step="0.05" bind:value={scale} />
          </div>
          <div class="row2">
            <div class="slider">
              <label>{$t("Offset X")} <span>{offsetX}</span></label>
              <input type="range" min="-200" max="200" step="2" bind:value={offsetX} />
            </div>
            <div class="slider">
              <label>{$t("Offset Y")} <span>{offsetY}</span></label>
              <input type="range" min="-200" max="200" step="2" bind:value={offsetY} />
            </div>
          </div>

          <div class="field-row">
            <label class="field">
              <span>{$t("Pack")}</span>
              <input bind:value={packName} placeholder={$t("my-stickers")} />
            </label>
            <label class="field">
              <span>{$t("Name")}</span>
              <input bind:value={stickerName} placeholder={$t("sticker")} />
            </label>
          </div>

          <div class="emojis">
            <div class="emoji-head">{$t("Tag with emoji")} <strong>{emoji}</strong></div>
            <div class="emoji-grid">
              {#each EMOJIS as e}
                <button
                  class="emoji"
                  class:active={emoji === e}
                  onclick={() => (emoji = e)}
                >{e}</button>
              {/each}
            </div>
          </div>
        </div>
      </div>

      <div class="foot">
        <span class="hint">Will save to <code>stickers/{packName || "..."}/{emoji}_{stickerName || "..."}.png</code></span>
        <div>
          <button class="btn-ghost" onclick={closeStickerEditor} disabled={processing}>{$t("Cancel")}</button>
          <button class="btn-primary" onclick={save} disabled={processing}>
            {$t(processing ? "Saving…" : "Save sticker")}
          </button>
        </div>
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed; inset: 0;
    background: rgba(6, 7, 12, 0.72);
    backdrop-filter: blur(4px);
    display: grid; place-items: center;
    z-index: 128;
    padding: 20px;
  }
  .sheet {
    width: min(680px, 94vw);
    max-height: 94vh;
    display: flex; flex-direction: column;
    background: var(--bg-shell);
    border: 1px solid var(--border);
    border-radius: 16px;
    box-shadow: 0 32px 80px -16px rgba(0,0,0,0.6);
    overflow: hidden;
    animation: slide-up 0.22s ease-out;
  }
  @keyframes slide-up {
    from { opacity: 0; transform: translateY(12px); }
    to   { opacity: 1; transform: translateY(0); }
  }
  .head { padding: 12px 18px; display: flex; justify-content: space-between; align-items: center; border-bottom: 1px solid var(--border); }
  .head h2 { margin: 0; font-size: 15px; font-weight: 700; }
  .close { width: 30px; height: 30px; background: transparent; border: 1px solid var(--border); border-radius: 8px; color: var(--fg-dim); font-size: 18px; cursor: pointer; }
  .hidden { display: none; }

  .body { display: grid; grid-template-columns: 240px 1fr; gap: 16px; padding: 16px; overflow: auto; }
  .stage {
    background:
      repeating-conic-gradient(#171b26 0 25%, #0f131c 0 50%) 0 / 24px 24px;
    border-radius: 12px;
    padding: 10px;
    display: grid; place-items: center;
  }
  .stage canvas {
    width: 100%;
    aspect-ratio: 1 / 1;
    display: block;
    border-radius: 6px;
  }

  .controls { display: flex; flex-direction: column; gap: 12px; min-width: 0; }
  .slider { display: flex; flex-direction: column; gap: 4px; }
  .slider label { display: flex; justify-content: space-between; font-size: 11px; font-weight: 700; color: var(--fg-dim); text-transform: uppercase; letter-spacing: 0.06em; }
  .slider label span { color: var(--fg-muted); text-transform: none; font-variant-numeric: tabular-nums; letter-spacing: 0; font-weight: 500; }
  .slider input[type="range"] { width: 100%; accent-color: var(--accent); }
  .row2 { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; }

  .field-row { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; }
  .field { display: flex; flex-direction: column; gap: 4px; }
  .field span { font-size: 11px; font-weight: 700; color: var(--fg-dim); text-transform: uppercase; letter-spacing: 0.06em; }
  .field input {
    background: var(--bg-input);
    border: 1px solid var(--border);
    border-radius: 8px;
    color: var(--fg);
    font-family: inherit;
    font-size: 13px;
    padding: 7px 10px;
    outline: none;
  }
  .field input:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }

  .emojis { display: flex; flex-direction: column; gap: 6px; }
  .emoji-head { font-size: 11px; font-weight: 700; color: var(--fg-dim); text-transform: uppercase; letter-spacing: 0.06em; }
  .emoji-head strong { color: var(--fg); font-size: 15px; }
  .emoji-grid {
    display: grid;
    grid-template-columns: repeat(10, 1fr);
    gap: 4px;
    max-height: 128px;
    overflow-y: auto;
    padding: 4px;
    background: var(--bg-input);
    border-radius: 10px;
    border: 1px solid var(--border);
  }
  .emoji {
    aspect-ratio: 1 / 1;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 6px;
    font-size: 18px;
    cursor: pointer;
    padding: 0;
  }
  .emoji:hover { background: var(--bg-hover); }
  .emoji.active { background: var(--accent-soft); border-color: var(--accent); }

  .foot {
    padding: 12px 18px;
    display: flex; justify-content: space-between; align-items: center; gap: 10px;
    border-top: 1px solid var(--border);
    background: var(--bg-list);
  }
  .hint { font-size: 11px; color: var(--fg-dim); font-family: var(--font-mono); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .hint code { color: var(--accent); background: transparent; }
  .foot > div { display: flex; gap: 10px; }
  .btn-ghost, .btn-primary {
    padding: 8px 18px;
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

  @media (max-width: 640px) {
    .body { grid-template-columns: 1fr; }
    .emoji-grid { grid-template-columns: repeat(8, 1fr); }
  }
</style>
