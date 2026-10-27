<script lang="ts">
  /**
   * A pie / donut you can touch: the slice under the pointer grows and moves
   * out a little, its name and size float next to it; a click picks it.
   */
  interface Seg { id: string; label: string; color: string; value: number }
  let { segs, size = 150, title = "", sub = "", picked = "", onpick, fmt = (n: number) => String(n) }:
    { segs: Seg[]; size?: number; title?: string; sub?: string; picked?: string; onpick?: (id: string) => void; fmt?: (n: number) => string } = $props();

  let hover = $state<string>("");
  let tip = $state({ x: 0, y: 0 });
  const total = $derived(segs.reduce((n, s) => n + Math.max(0, s.value), 0));
  const R = 40, r = 25, C = 50;
  const arcs = $derived.by(() => {
    if (!total) return [];
    let a0 = -Math.PI / 2;
    return segs.filter((s) => s.value > 0).map((s) => {
      const frac = s.value / total;
      const a1 = a0 + frac * Math.PI * 2 - (frac < 1 ? 0.012 : 0.0001);
      const mid = (a0 + a1) / 2;
      const big = a1 - a0 > Math.PI ? 1 : 0;
      const p = (rad: number, a: number) => `${(C + rad * Math.cos(a)).toFixed(3)} ${(C + rad * Math.sin(a)).toFixed(3)}`;
      const d = frac >= 0.9999
        ? `M ${p(R, 0)} A ${R} ${R} 0 1 1 ${p(R, Math.PI)} A ${R} ${R} 0 1 1 ${p(R, 0)} M ${p(r, 0)} A ${r} ${r} 0 1 0 ${p(r, Math.PI)} A ${r} ${r} 0 1 0 ${p(r, 0)} Z`
        : `M ${p(R, a0)} A ${R} ${R} 0 ${big} 1 ${p(R, a1)} L ${p(r, a1)} A ${r} ${r} 0 ${big} 0 ${p(r, a0)} Z`;
      const out = { ...s, d, mid, frac };
      a0 = a1 + (frac < 1 ? 0.012 : 0);
      return out;
    });
  });
  const cur = $derived(arcs.find((a) => a.id === (hover || picked)));
  function move(e: PointerEvent, id: string) {
    hover = id;
    const box = (e.currentTarget as SVGElement).ownerSVGElement?.parentElement?.getBoundingClientRect();
    if (box) tip = { x: e.clientX - box.left, y: e.clientY - box.top };
  }
</script>

<div class="donut" style:width="{size}px" style:height="{size}px">
  <svg viewBox="0 0 100 100" role="img" aria-label={title}>
    <circle cx="50" cy="50" r="32.5" fill="none" stroke="var(--bg-input)" stroke-width="15" />
    {#each arcs as a (a.id)}
      {@const on = a.id === hover || a.id === picked}
      <path d={a.d} fill={a.color} role="button" tabindex="0" aria-label="{a.label}: {fmt(a.value)}"
        class:on class:dim={(hover || picked) && !on}
        style:transform={on ? `translate(${(Math.cos(a.mid) * 3.2).toFixed(2)}px, ${(Math.sin(a.mid) * 3.2).toFixed(2)}px) scale(1.045)` : undefined}
        onpointermove={(e) => move(e, a.id)} onpointerleave={() => (hover = "")}
        onclick={() => onpick?.(a.id)} onkeydown={(e) => { if (e.key === "Enter") onpick?.(a.id); }} />
    {/each}
  </svg>
  <div class="mid">
    {#if cur}
      <b style:color={cur.color}>{Math.round(cur.frac * 100)}%</b>
      <small>{cur.label}</small>
    {:else}
      <b>{title}</b>
      <small>{sub}</small>
    {/if}
  </div>
  {#if hover && cur}
    <div class="tip" style:left="{tip.x + 14}px" style:top="{tip.y - 10}px">
      <i style:background={cur.color}></i>
      <span><b>{cur.label}</b><small>{fmt(cur.value)} · {Math.round(cur.frac * 100)}%</small></span>
    </div>
  {/if}
</div>

<style>
  .donut { position: relative; flex-shrink: 0; }
  svg { width: 100%; height: 100%; overflow: visible; }
  path { transform-origin: 50px 50px; transform-box: view-box; transition: transform .22s cubic-bezier(.2,.9,.3,1.2), opacity .18s, filter .18s; cursor: pointer; outline: none; }
  path.on { filter: drop-shadow(0 3px 6px rgba(0,0,0,.45)); }
  path.dim { opacity: .45; }
  .mid { position: absolute; inset: 0; display: flex; flex-direction: column; align-items: center; justify-content: center; pointer-events: none; text-align: center; padding: 0 28%; }
  .mid b { font-size: 17px; line-height: 1.1; } .mid small { font-size: 10.5px; color: var(--fg-dim); line-height: 1.2; }
  .tip { position: absolute; z-index: 5; pointer-events: none; display: flex; align-items: center; gap: 8px; white-space: nowrap;
    padding: 7px 10px; border-radius: calc(10px * var(--rk, 1)); background: color-mix(in srgb, var(--bg-shell) 92%, transparent);
    border: 1px solid var(--border-strong); box-shadow: 0 10px 28px -10px rgba(0,0,0,.7); backdrop-filter: blur(8px); animation: tip-in .14s ease-out; }
  .tip i { width: 10px; height: 10px; border-radius: 3px; flex-shrink: 0; }
  .tip span { display: flex; flex-direction: column; font-size: 12.5px; } .tip small { color: var(--fg-dim); font-size: 11.5px; }
  @keyframes tip-in { from { opacity: 0; transform: translateY(4px) scale(.96); } }
</style>
