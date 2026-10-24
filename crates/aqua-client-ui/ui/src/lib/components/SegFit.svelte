<script lang="ts" generics="T extends string | number">
  /**
   * A segmented control that never pushes the page sideways: while all its
   * buttons fit the space it's given they show side by side; when they don't
   * (a phone, a long translation) it turns into a drop-down.
   */
  let { options, value, onpick, label }: {
    options: readonly { id: T; label: string }[];
    value: T;
    onpick: (id: T) => void;
    label?: string;
  } = $props();

  let wrap: HTMLElement | undefined = $state();
  let probe: HTMLElement | undefined = $state();
  let fits = $state(true);
  $effect(() => {
    if (!wrap || !probe) return;
    const w = wrap, p = probe;
    // The probe is an invisible copy of the buttons at their natural width.
    const check = () => { fits = p.scrollWidth <= w.clientWidth + 1; };
    const ro = new ResizeObserver(check);
    ro.observe(w);
    ro.observe(p);
    check();
    return () => ro.disconnect();
  });
  const current = $derived(Math.max(0, options.findIndex((o) => o.id === value)));
</script>

<div class="seg-fit" bind:this={wrap}>
  <div class="seg probe" bind:this={probe} aria-hidden="true">
    {#each options as o (o.id)}<span class="seg-opt">{o.label}</span>{/each}
  </div>
  {#if fits}
    <div class="seg" role="radiogroup" aria-label={label}>
      {#each options as o (o.id)}
        <button class="seg-opt" class:active={o.id === value} role="radio" aria-checked={o.id === value} onclick={() => onpick(o.id)}>{o.label}</button>
      {/each}
    </div>
  {:else}
    <select class="seg-select" aria-label={label} value={String(current)}
      onchange={(e) => onpick(options[Number((e.target as HTMLSelectElement).value)].id)}>
      {#each options as o, i (o.id)}<option value={String(i)}>{o.label}</option>{/each}
    </select>
  {/if}
</div>

<style>
  /* clip: the invisible probe must not widen the page (scrollable overflow). */
  .seg-fit { position: relative; flex: 1 1 auto; min-width: 0; display: flex; overflow: clip; overflow-clip-margin: 6px; }
  .seg {
    display: flex;
    background: var(--bg-input);
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 3px;
    gap: 2px;
  }
  .seg.probe { position: absolute; left: 0; top: 0; width: max-content; visibility: hidden; pointer-events: none; }
  .seg-opt {
    padding: 5px 12px;
    background: transparent;
    border: none;
    border-radius: 7px;
    color: var(--fg-muted);
    font-size: 12.5px;
    font-weight: 600;
    white-space: nowrap;
    cursor: pointer;
    transition: all 0.12s ease;
  }
  .seg-opt:hover { color: var(--fg); }
  .seg-opt.active {
    background: var(--accent-soft);
    color: var(--accent);
    box-shadow: 0 0 0 1px color-mix(in srgb, var(--accent) 25%, transparent);
  }
  .seg-select {
    max-width: 100%; min-width: 0;
    background-color: var(--bg-input); color: var(--accent); border: 1px solid var(--border);
    border-radius: 10px; padding: 8px 12px; font: inherit; font-size: 14px; font-weight: 600;
  }
  @media (max-width: 720px) {
    .seg-opt { padding: 8px 12px; font-size: 13.5px; }
  }
</style>
