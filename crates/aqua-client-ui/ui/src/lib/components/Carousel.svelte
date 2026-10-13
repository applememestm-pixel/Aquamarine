<script module lang="ts">
  export interface CarouselItem {
    kind: "image" | "video";
    /** Image URL, or a ready video URL. */
    src?: string;
    /** Still frame shown before a video starts. */
    poster?: string;
    /** Videos that still have to be downloaded / decrypted. */
    resolve?: () => Promise<string>;
    label?: string;
  }
</script>

<script lang="ts">
  /**
   * Photo & video carousel — swipe / drag, arrows, ← → keys, dots or a
   * counter, optional thumbnails. Photos open full screen on click; videos
   * load on first play and play right in the slide (paused when you swipe
   * away), with a button to go full screen.
   */
  import { openLightbox } from "./Lightbox.svelte";
  import { mediaErrorText } from "../media";
  import { tr } from "../i18n";

  interface Props {
    items: CarouselItem[];
    /** Width / height of the stage. */
    aspect?: number;
    thumbs?: boolean;
    onopen?: (index: number) => void;
  }
  let { items, aspect = 4 / 3, thumbs = false, onopen }: Props = $props();

  let index = $state(0);
  let dragX = $state(0);
  let dragging = $state(false);
  let stage: HTMLDivElement | undefined = $state();
  let startX = 0;
  let startY = 0;
  let moved = false;
  let pid: number | null = null;

  /** Per-slide video state. */
  let vsrc = $state<Record<number, string>>({});
  let vstate = $state<Record<number, "loading" | "error" | undefined>>({});
  let verr = $state<Record<number, string>>({});

  let n = $derived(items.length);
  $effect(() => { if (index > n - 1) index = Math.max(0, n - 1); });

  function go(i: number) {
    const next = Math.max(0, Math.min(n - 1, i));
    if (next !== index) stage?.querySelectorAll("video").forEach((v) => v.pause());
    index = next;
  }

  async function playVideo(i: number) {
    const it = items[i];
    if (!it || it.kind !== "video" || vstate[i] === "loading") return;
    if (!vsrc[i]) {
      if (it.src) vsrc[i] = it.src;
      else if (it.resolve) {
        vstate[i] = "loading";
        try { vsrc[i] = await it.resolve(); vstate[i] = undefined; }
        catch (e) { vstate[i] = "error"; verr[i] = String(e); return; }
      }
    }
  }
  function fullscreen(i: number) {
    const url = vsrc[i];
    if (url) {
      stage?.querySelectorAll("video").forEach((v) => v.pause());
      openLightbox({ url, kind: "video", name: items[i]?.label });
    }
  }

  function isControl(t: EventTarget | null): boolean {
    return t instanceof Element && !!t.closest("video, .vbtn, .nav, .dot");
  }
  function down(e: PointerEvent) {
    if (n < 2 || e.button !== 0 || isControl(e.target)) return;
    pid = e.pointerId;
    startX = e.clientX; startY = e.clientY;
    moved = false; dragging = true; dragX = 0;
  }
  function move(e: PointerEvent) {
    if (!dragging || e.pointerId !== pid) return;
    const dx = e.clientX - startX;
    if (!moved && Math.abs(dx) > 6 && Math.abs(dx) > Math.abs(e.clientY - startY)) {
      moved = true;
      stage?.setPointerCapture(e.pointerId);
    }
    if (moved) {
      const edge = (index === 0 && dx > 0) || (index === n - 1 && dx < 0);
      dragX = edge ? dx / 3 : dx;
    }
  }
  function up(e: PointerEvent) {
    if (!dragging || e.pointerId !== pid) return;
    dragging = false; pid = null;
    const w = stage?.clientWidth ?? 1;
    if (moved && Math.abs(dragX) > w * 0.18) go(index + (dragX < 0 ? 1 : -1));
    dragX = 0;
  }
  function click(i: number) {
    if (moved) { moved = false; return; }
    if (items[i]?.kind === "video") void playVideo(i);
    else onopen?.(i);
  }
  function key(e: KeyboardEvent) {
    if (e.target instanceof HTMLVideoElement) return;
    if (e.key === "ArrowLeft") { e.preventDefault(); e.stopPropagation(); go(index - 1); }
    else if (e.key === "ArrowRight") { e.preventDefault(); e.stopPropagation(); go(index + 1); }
    else if (e.key === "Enter") { e.preventDefault(); click(index); }
  }
</script>

{#if n > 0}
  <div class="carousel" style={`--ar: ${aspect};`}>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
    <div
      class="stage"
      bind:this={stage}
      role="region"
      aria-roledescription="carousel"
      aria-label={`${index + 1} / ${n}`}
      tabindex="0"
      onpointerdown={down}
      onpointermove={move}
      onpointerup={up}
      onpointercancel={up}
      onkeydown={key}
    >
      <div class="track" class:anim={!dragging} style={`transform: translate3d(calc(${-index * 100}% + ${dragX}px), 0, 0);`}>
        {#each items as it, i (i)}
          <div class="slide" class:video={it.kind === "video"}>
            {#if Math.abs(i - index) <= 1}
              {#if it.kind === "image"}
                <button class="hit" onclick={() => click(i)} tabindex="-1" aria-label={`Photo ${i + 1}`}>
                  {#if it.src}<img src={it.src} alt="" draggable="false" decoding="async" />{:else}<span class="ph"></span>{/if}
                </button>
              {:else if vsrc[i]}
                <!-- svelte-ignore a11y_media_has_caption -->
                <video src={vsrc[i]} poster={it.poster} controls autoplay playsinline preload="metadata" onerror={(e) => { verr[i] = tr(mediaErrorText(e.currentTarget as HTMLVideoElement)); vstate[i] = "error"; vsrc[i] = ""; }}></video>
                <button class="vbtn fs" onclick={() => fullscreen(i)} title="Full screen" aria-label="Full screen">⤢</button>
              {:else}
                <button class="hit" onclick={() => click(i)} tabindex="-1" aria-label={`Video ${i + 1}`}>
                  {#if it.poster}<img src={it.poster} alt="" draggable="false" decoding="async" />{:else}<span class="ph vid"></span>{/if}
                  {#if vstate[i] === "loading"}
                    <span class="spin"></span>
                  {:else}
                    <span class="play"><svg viewBox="0 0 24 24"><path d="M8 5v14l11-7z" fill="currentColor"/></svg></span>
                  {/if}
                  {#if it.label}<span class="vlabel">{vstate[i] === "error" ? "⚠ " + (verr[i] ?? "") : it.label}</span>{/if}
                </button>
              {/if}
            {/if}
          </div>
        {/each}
      </div>
      {#if n > 1}
        {#if index > 0}
          <button class="nav prev" onclick={() => go(index - 1)} aria-label="Previous">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 18 9 12 15 6"/></svg>
          </button>
        {/if}
        {#if index < n - 1}
          <button class="nav next" onclick={() => go(index + 1)} aria-label="Next">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="9 18 15 12 9 6"/></svg>
          </button>
        {/if}
        <span class="count">{index + 1} / {n}</span>
        {#if n <= 12 && !(items[index]?.kind === "video" && vsrc[index])}
          <div class="dots">
            {#each items as _, i (i)}
              <button class="dot" class:on={i === index} onclick={() => go(i)} aria-label={`Slide ${i + 1}`}></button>
            {/each}
          </div>
        {/if}
      {/if}
    </div>
    {#if thumbs && n > 1}
      <div class="thumbs">
        {#each items as it, i (i)}
          <button class="thumb" class:on={i === index} onclick={() => go(i)} aria-label={`Slide ${i + 1}`}>
            {#if it.kind === "image" ? it.src : it.poster}<img src={it.kind === "image" ? it.src : it.poster} alt="" draggable="false" loading="lazy" decoding="async" />{/if}
            {#if it.kind === "video"}<span class="tplay">▶</span>{/if}
          </button>
        {/each}
      </div>
    {/if}
  </div>
{/if}

<style>
  .carousel { width: 100%; }
  .stage {
    position: relative;
    width: 100%;
    aspect-ratio: var(--ar);
    max-height: 70vh;
    overflow: hidden;
    border-radius: 12px;
    background: #05060a;
    touch-action: pan-y;
    user-select: none;
    outline: none;
    contain: layout paint;
  }
  .stage:focus-visible { box-shadow: 0 0 0 2px var(--accent); }
  .track { display: flex; height: 100%; will-change: transform; }
  .track.anim { transition: transform 0.28s cubic-bezier(0.2, 0.7, 0.2, 1); }
  .slide { position: relative; flex: 0 0 100%; height: 100%; display: grid; place-items: center; }
  .hit {
    position: relative; width: 100%; height: 100%;
    padding: 0; border: none; background: transparent;
    cursor: zoom-in; display: grid; place-items: center;
  }
  .video .hit { cursor: pointer; }
  .slide img { width: 100%; height: 100%; object-fit: contain; display: block; pointer-events: none; }
  .slide video { width: 100%; height: 100%; object-fit: contain; background: #000; display: block; }
  .ph { width: 100%; height: 100%; background: #0b0d14; }
  .ph.vid { background: radial-gradient(circle at 50% 50%, #1a1f2e, #07080d); }
  .play {
    position: absolute; left: 50%; top: 50%; transform: translate(-50%, -50%);
    width: 58px; height: 58px; border-radius: 50%;
    display: grid; place-items: center;
    background: rgba(0, 0, 0, 0.6); color: #fff;
    border: 2px solid rgba(255, 255, 255, 0.85);
  }
  .play svg { width: 26px; height: 26px; margin-left: 3px; }
  .spin {
    position: absolute; left: 50%; top: 50%; margin: -20px 0 0 -20px;
    width: 40px; height: 40px; border-radius: 50%;
    border: 3px solid rgba(255, 255, 255, 0.25); border-top-color: #fff;
    animation: c-spin 0.8s linear infinite;
  }
  @keyframes c-spin { to { transform: rotate(360deg); } }
  .vlabel {
    position: absolute; left: 10px; bottom: 10px; max-width: calc(100% - 20px);
    padding: 2px 8px; border-radius: 8px; background: rgba(0, 0, 0, 0.6); color: #fff;
    font-size: 11.5px; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }
  .vbtn {
    position: absolute; top: 10px; left: 10px;
    width: 32px; height: 32px; border-radius: 8px; border: none;
    background: rgba(0, 0, 0, 0.55); color: #fff; cursor: pointer; font-size: 16px;
  }
  .nav {
    position: absolute; top: 50%; transform: translateY(-50%);
    width: 36px; height: 36px; border-radius: 50%;
    border: none; background: rgba(0, 0, 0, 0.55); color: #fff;
    display: grid; place-items: center; cursor: pointer;
    opacity: 0; transition: opacity 0.15s;
  }
  .nav svg { width: 18px; height: 18px; }
  .stage:hover .nav, .stage:focus-within .nav { opacity: 1; }
  .prev { left: 10px; }
  .next { right: 10px; }
  .count {
    position: absolute; top: 10px; right: 10px;
    padding: 2px 9px; border-radius: 10px;
    background: rgba(0, 0, 0, 0.55); color: #fff;
    font-size: 11.5px; font-weight: 700; font-variant-numeric: tabular-nums;
    pointer-events: none;
  }
  .dots {
    position: absolute; bottom: 10px; left: 0; right: 0;
    display: flex; justify-content: center; gap: 6px; pointer-events: none;
  }
  .dot {
    width: 7px; height: 7px; border-radius: 50%; padding: 0;
    border: none; background: rgba(255, 255, 255, 0.45); cursor: pointer; pointer-events: auto;
    transition: transform 0.15s, background 0.15s;
  }
  .dot.on { background: #fff; transform: scale(1.25); }
  .thumbs { display: flex; gap: 6px; margin-top: 8px; overflow-x: auto; padding-bottom: 2px; }
  .thumb {
    position: relative; flex: 0 0 64px; height: 48px; padding: 0;
    border-radius: 8px; overflow: hidden; cursor: pointer;
    border: 2px solid transparent; background: var(--bg-input); opacity: 0.6;
  }
  .thumb.on { border-color: var(--accent); opacity: 1; }
  .thumb img { width: 100%; height: 100%; object-fit: cover; display: block; }
  .tplay { position: absolute; inset: 0; display: grid; place-items: center; color: #fff; font-size: 13px; text-shadow: 0 0 4px #000; }
</style>
