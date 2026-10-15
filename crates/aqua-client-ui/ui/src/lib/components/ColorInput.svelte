<script lang="ts">
  /**
   * Our own colour picker (the native <input type=color> opens GTK's palette
   * dialog on Linux). A swatch; clicking it opens a popover with a
   * saturation/brightness square, a hue strip, HEX and R/G/B fields.
   */
  import { onDestroy, tick } from "svelte";

  let { value = $bindable("#00e5c7"), oninput, disabled = false, size = 30, title = "" }:
    { value?: string; oninput?: (v: string) => void; disabled?: boolean; size?: number; title?: string } = $props();

  let open = $state(false);
  let btn = $state<HTMLButtonElement | null>(null);
  let pos = $state({ x: 0, y: 0 });
  let h = $state(0), sv = $state({ s: 1, v: 1 });
  let hex = $state("");

  function parse(c: string): [number, number, number] {
    const m = /^#?([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(c.trim());
    if (!m) return [0, 229, 199];
    let x = m[1];
    if (x.length === 3) x = x.split("").map((ch) => ch + ch).join("");
    const n = parseInt(x, 16);
    return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
  }
  const toHex = (r: number, g: number, b: number) => "#" + [r, g, b].map((v) => Math.round(Math.max(0, Math.min(255, v))).toString(16).padStart(2, "0")).join("");
  function rgbToHsv(r: number, g: number, b: number) {
    r /= 255; g /= 255; b /= 255;
    const max = Math.max(r, g, b), min = Math.min(r, g, b), d = max - min;
    let hh = 0;
    if (d) hh = max === r ? ((g - b) / d) % 6 : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
    return { h: (hh * 60 + 360) % 360, s: max ? d / max : 0, v: max };
  }
  function hsvToRgb(hh: number, s: number, v: number): [number, number, number] {
    const f = (n: number) => { const k = (n + hh / 60) % 6; return v - v * s * Math.max(0, Math.min(k, 4 - k, 1)); };
    return [f(5) * 255, f(3) * 255, f(1) * 255];
  }
  let rgb = $derived(hsvToRgb(h, sv.s, sv.v));

  function load(c: string) {
    const [r, g, b] = parse(c);
    const x = rgbToHsv(r, g, b);
    if (x.s > 0 && x.v > 0) h = x.h;
    sv = { s: x.s, v: x.v };
    hex = toHex(r, g, b);
  }
  function emit() {
    const v = toHex(...rgb);
    hex = v;
    value = v;
    oninput?.(v);
  }

  async function toggle() {
    if (disabled) return;
    if (open) { open = false; return; }
    load(value);
    const r = btn!.getBoundingClientRect();
    const W = 248, H = 300;
    pos = {
      x: Math.min(window.innerWidth - W - 8, Math.max(8, r.left)),
      y: r.bottom + H + 8 > window.innerHeight ? Math.max(8, r.top - H - 6) : r.bottom + 6,
    };
    open = true;
    await tick();
  }

  function dragArea(e: PointerEvent, kind: "sv" | "hue") {
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    const move = (ev: PointerEvent) => {
      const r = el.getBoundingClientRect();
      const x = Math.max(0, Math.min(1, (ev.clientX - r.left) / r.width));
      const y = Math.max(0, Math.min(1, (ev.clientY - r.top) / r.height));
      if (kind === "sv") sv = { s: x, v: 1 - y }; else h = x * 360;
      emit();
    };
    move(e);
    const up = () => { el.removeEventListener("pointermove", move); el.removeEventListener("pointerup", up); };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
  }
  function setHex(v: string) {
    hex = v;
    if (/^#?([0-9a-f]{6}|[0-9a-f]{3})$/i.test(v.trim())) { load(v); emit(); }
  }
  function setChannel(i: number, v: number) {
    const c = [...rgb] as [number, number, number];
    c[i] = Math.max(0, Math.min(255, v || 0));
    load(toHex(...c));
    emit();
  }

  function onDoc(e: PointerEvent) {
    if (!open) return;
    const t = e.target as HTMLElement;
    if (t.closest(".cp-pop") || t.closest(".cp-swatch") === btn) return;
    open = false;
  }
  function onKey(e: KeyboardEvent) { if (open && e.key === "Escape") { e.stopPropagation(); e.preventDefault(); open = false; } }
  $effect(() => {
    if (!open) return;
    document.addEventListener("pointerdown", onDoc, true);
    document.addEventListener("keydown", onKey, true);
    return () => { document.removeEventListener("pointerdown", onDoc, true); document.removeEventListener("keydown", onKey, true); };
  });
  onDestroy(() => { open = false; });

  /** Portal: render the popover under <body> so overflow:hidden parents don't clip it. */
  function portal(node: HTMLElement) {
    document.body.appendChild(node);
    return { destroy() { node.remove(); } };
  }
</script>

<button bind:this={btn} type="button" class="cp-swatch" style:width="{size}px" style:height="{size}px" style:background={value}
        onclick={toggle} {disabled} {title} aria-label={title || "Colour"} aria-expanded={open}></button>

{#if open}
  <div class="cp-pop" use:portal style:left="{pos.x}px" style:top="{pos.y}px" role="dialog" aria-label="Colour picker">
    <div class="cp-sv" style:background-color={`hsl(${h} 100% 50%)`} onpointerdown={(e) => dragArea(e, "sv")} role="slider" aria-valuenow={Math.round(sv.s * 100)} tabindex="0">
      <span class="cp-knob" style:left="{sv.s * 100}%" style:top="{(1 - sv.v) * 100}%" style:background={toHex(...rgb)}></span>
    </div>
    <div class="cp-hue" onpointerdown={(e) => dragArea(e, "hue")} role="slider" aria-valuenow={Math.round(h)} tabindex="0">
      <span class="cp-hknob" style:left="{(h / 360) * 100}%"></span>
    </div>
    <div class="cp-fields">
      <span class="cp-prev" style:background={toHex(...rgb)}></span>
      <label class="cp-hex">HEX<input value={hex} maxlength="7" spellcheck="false" oninput={(e) => setHex((e.target as HTMLInputElement).value)} /></label>
    </div>
    <div class="cp-rgb">
      {#each ["R", "G", "B"] as l, i (l)}
        <label>{l}<input type="number" min="0" max="255" value={Math.round(rgb[i])} oninput={(e) => setChannel(i, +(e.target as HTMLInputElement).value)} /></label>
      {/each}
    </div>
  </div>
{/if}

<style>
  .cp-swatch { border-radius: 9px; border: 2px solid var(--border-strong); cursor: pointer; padding: 0; flex-shrink: 0;
    box-shadow: inset 0 0 0 2px rgba(0,0,0,.15); transition: transform .15s, border-color .15s; }
  .cp-swatch:hover:not(:disabled) { transform: scale(1.06); border-color: var(--accent); }
  .cp-swatch:disabled { opacity: .5; cursor: not-allowed; }
  .cp-pop { position: fixed; z-index: 10050; width: 232px; padding: 10px; border-radius: 14px; background: var(--bg-shell);
    border: 1px solid var(--border-strong); box-shadow: 0 24px 60px -16px rgba(0,0,0,.7); display: flex; flex-direction: column; gap: 10px;
    animation: cp-in .2s cubic-bezier(.2,.9,.25,1.1); }
  @keyframes cp-in { from { opacity: 0; transform: translateY(-4px) scale(.97); } }
  .cp-sv { position: relative; height: 150px; border-radius: 10px; cursor: crosshair; touch-action: none;
    background-image: linear-gradient(to top, #000, transparent), linear-gradient(to right, #fff, transparent); }
  .cp-knob { position: absolute; width: 14px; height: 14px; border-radius: 50%; border: 2px solid #fff; box-shadow: 0 0 0 1px rgba(0,0,0,.5);
    transform: translate(-50%, -50%); pointer-events: none; }
  .cp-hue { position: relative; height: 14px; border-radius: 7px; cursor: pointer; touch-action: none;
    background: linear-gradient(to right, #f00, #ff0, #0f0, #0ff, #00f, #f0f, #f00); }
  .cp-hknob { position: absolute; top: 50%; width: 10px; height: 18px; border-radius: 5px; background: #fff; box-shadow: 0 0 0 1px rgba(0,0,0,.5);
    transform: translate(-50%, -50%); pointer-events: none; }
  .cp-fields { display: flex; gap: 8px; align-items: center; }
  .cp-prev { width: 32px; height: 32px; border-radius: 8px; border: 1px solid var(--border-strong); flex-shrink: 0; }
  .cp-hex, .cp-rgb label { display: flex; flex-direction: column; gap: 2px; font-size: 10px; font-weight: 800; color: var(--fg-dim); flex: 1; }
  .cp-rgb { display: flex; gap: 6px; }
  .cp-pop input { width: 100%; box-sizing: border-box; background: var(--bg-input); border: 1px solid var(--border); border-radius: 7px;
    color: var(--fg); font: inherit; font-size: 12px; padding: 5px 6px; outline: none; font-family: var(--font-mono, monospace); }
  .cp-pop input:focus { border-color: var(--accent); }
  .cp-pop input::-webkit-inner-spin-button { opacity: .4; }
</style>
