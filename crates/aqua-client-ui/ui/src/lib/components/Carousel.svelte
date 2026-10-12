<script lang="ts">
  /**
   * Photo carousel — swipe / drag, arrows, ← → keys, dots or a counter, an
   * optional thumbnail strip. Clicking a photo asks the parent to open it
   * full screen (the gallery viewer).
   */
  interface Props {
    srcs: string[];
    /** Width / height of the stage. */
    aspect?: number;
    thumbs?: boolean;
    onopen?: (index: number) => void;
  }
  let { srcs, aspect = 4 / 3, thumbs = false, onopen }: Props = $props();

  let index = $state(0);
  let dragX = $state(0);
  let dragging = $state(false);
  let stage: HTMLDivElement | undefined = $state();
  let startX = 0;
  let startY = 0;
  let moved = false;
  let pid: number | null = null;

  let n = $derived(srcs.length);
  $effect(() => { if (index > n - 1) index = Math.max(0, n - 1); });

  function go(i: number) { index = Math.max(0, Math.min(n - 1, i)); }

  function down(e: PointerEvent) {
    if (n < 2 || e.button !== 0) return;
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
      // Rubber band at the ends.
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
    onopen?.(i);
  }
  function key(e: KeyboardEvent) {
    if (e.key === "ArrowLeft") { e.preventDefault(); e.stopPropagation(); go(index - 1); }
    else if (e.key === "ArrowRight") { e.preventDefault(); e.stopPropagation(); go(index + 1); }
    else if (e.key === "Enter") { e.preventDefault(); onopen?.(index); }
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
      <div class="track" class:anim={!dragging} style={`transform: translateX(calc(${-index * 100}% + ${dragX}px));`}>
        {#each srcs as src, i (i)}
          <button class="slide" onclick={() => click(i)} tabindex="-1" aria-label={`Photo ${i + 1}`}>
            {#if src && Math.abs(i - index) <= 2}
              <img {src} alt="" draggable="false" loading="lazy" />
            {:else}
              <span class="ph"></span>
            {/if}
          </button>
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
        {#if n <= 12}
          <div class="dots">
            {#each srcs as _, i (i)}
              <button class="dot" class:on={i === index} onclick={() => go(i)} aria-label={`Photo ${i + 1}`}></button>
            {/each}
          </div>
        {/if}
      {/if}
    </div>
    {#if thumbs && n > 1}
      <div class="thumbs">
        {#each srcs as src, i (i)}
          <button class="thumb" class:on={i === index} onclick={() => go(i)} aria-label={`Photo ${i + 1}`}>
            {#if src}<img {src} alt="" draggable="false" loading="lazy" />{/if}
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
  }
  .stage:focus-visible { box-shadow: 0 0 0 2px var(--accent); }
  .track { display: flex; height: 100%; }
  .track.anim { transition: transform 0.28s cubic-bezier(0.2, 0.7, 0.2, 1); }
  .slide {
    flex: 0 0 100%;
    height: 100%;
    padding: 0; border: none; background: transparent;
    cursor: zoom-in;
    display: grid; place-items: center;
  }
  .slide img { width: 100%; height: 100%; object-fit: contain; display: block; pointer-events: none; }
  .ph { width: 100%; height: 100%; background: linear-gradient(90deg, #0b0d14, #12151f, #0b0d14); }
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
    flex: 0 0 64px; height: 48px; padding: 0;
    border-radius: 8px; overflow: hidden; cursor: pointer;
    border: 2px solid transparent; background: var(--bg-input); opacity: 0.6;
  }
  .thumb.on { border-color: var(--accent); opacity: 1; }
  .thumb img { width: 100%; height: 100%; object-fit: cover; display: block; }
</style>
