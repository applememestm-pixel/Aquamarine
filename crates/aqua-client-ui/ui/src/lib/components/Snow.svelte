<script module lang="ts">
  /** Chat id → when it was last open (ms). The snow pile grows with the time
   *  away from a chat (PC only; Settings → Appearance → Snow). */
  const KEY = "aqua.chatSeenAt.v1";
  function load(): Record<string, number> { try { return JSON.parse(localStorage.getItem(KEY) || "{}"); } catch { return {}; } }
  export function lastOpened(id: string): number | null { return load()[id] ?? null; }
  export function markOpened(id: string) {
    try {
      const m = load();
      m[id] = Date.now();
      const keys = Object.keys(m);
      if (keys.length > 3000) for (const k of keys.slice(0, keys.length - 3000)) delete m[k];
      localStorage.setItem(KEY, JSON.stringify(m));
    } catch { /* storage blocked: no piles */ }
  }
  /** Pile height (px) for being away `ms`: an hour ≈ a dusting, a day ≈ a
   *  drift, a week and more — knee-deep (capped). */
  export function pileFor(ms: number): number {
    const h = ms / 3_600_000;
    if (h < 1) return 0;
    return Math.round(Math.min(120, 6 + Math.log2(1 + h) * 14));
  }
</script>

<script lang="ts">
  /** exteraGram's snow: a few flakes drifting down; `pile` — a snowdrift at
   *  the bottom that melts away after a while (or at a tap). */
  let { count = 18, pile = 0 }: { count?: number; pile?: number } = $props();
  const flakes = Array.from({ length: count }, () => ({ x: Math.random() * 100, d: 9 + Math.random() * 10, dl: -Math.random() * 18, s: 2 + Math.random() * 3.5, dx: (Math.random() - 0.5) * 80 }));
  let melting = $state(false);
  $effect(() => {
    void pile;
    melting = false;
    if (!pile) return;
    const t = setTimeout(() => (melting = true), 25_000);
    return () => clearTimeout(t);
  });
  // A bumpy top edge, the same for a given height.
  const bumps = $derived(Array.from({ length: 9 }, (_, i) => 30 + ((i * 37 + pile) % 23)));
</script>

<div class="snow" aria-hidden="true">
  {#each flakes as f, i (i)}<i style:left="{f.x}%" style:--d="{f.d}s" style:--dl="{f.dl}s" style:--s="{f.s}px" style:--dx="{f.dx}px"></i>{/each}
</div>
{#if pile > 0}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="pile" class:melting style:--h="{pile}px" onclick={() => (melting = true)} title="❄">
    <svg viewBox="0 0 100 40" preserveAspectRatio="none">
      <path d={`M0 40 L0 ${bumps[0] / 2} ${bumps.map((b, i) => `Q ${i * 12.5 + 6} ${b / 3} ${(i + 1) * 12.5} ${b / 2}`).join(" ")} L100 40 Z`} />
    </svg>
  </div>
{/if}

<style>
  .snow { position: absolute; inset: 0; z-index: 4; pointer-events: none; overflow: hidden; }
  .snow i { position: absolute; top: -10px; width: var(--s); height: var(--s); border-radius: 50%; background: #fff; opacity: .7; filter: blur(.4px);
    animation: snow var(--d) linear var(--dl) infinite; }
  @keyframes snow { to { transform: translate(var(--dx), calc(100vh + 20px)); } }
  .pile { position: absolute; left: 0; right: 0; bottom: 0; height: var(--h); z-index: 5; cursor: pointer;
    filter: drop-shadow(0 -2px 6px rgba(160, 200, 255, .35)); transition: height 6s ease-in, opacity 6s ease-in; animation: pile-in .8s ease-out; }
  .pile svg { width: 100%; height: 100%; display: block; }
  .pile path { fill: #f4f8ff; }
  .pile.melting { height: 0; opacity: 0; }
  @keyframes pile-in { from { transform: translateY(100%); } }
  :global(html.reduce-motion) .snow, :global(html.reduce-motion) .pile { display: none; }
</style>
