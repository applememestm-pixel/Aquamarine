<script lang="ts">
  /**
   * A star the project owner gave (aqua-server stars.rs; checked against the
   * owner's key in src/stars.rs, so nobody can give themselves one):
   * 0 — the official star; 1–8 — Boosty support levels, each its own colour.
   */
  import { t, tr } from "../i18n";
  let { kind, size = 16 }: { kind: number; size?: number } = $props();
  // Bronze, silver, gold, emerald, sapphire, amethyst, ruby, diamond.
  const LEVELS: [string, string][] = [
    ["#ffd54a", "#ff9f1c"], // official
    ["#e2a46d", "#9a5b2c"], ["#eef2f6", "#8e99a6"], ["#ffe082", "#d4a017"], ["#5ee6a8", "#0f9d64"],
    ["#7ab8ff", "#1f5fd1"], ["#d6a3ff", "#8a3ff0"], ["#ff8aa0", "#d1123f"], ["#ffffff", "#9fe8ff"],
  ];
  const c = $derived(LEVELS[Math.max(0, Math.min(8, kind))]);
  const gid = `sg${Math.random().toString(36).slice(2, 8)}`;
  const label = $derived(kind === 0 ? $t("Official star from Aquamarine") : tr("Aquamarine supporter · level {n}", { n: String(kind) }));
</script>

<span class="sm" class:official={kind === 0} class:diamond={kind === 8} title={label} aria-label={label} style:--s="{size}px" style:--g={c[0]}>
  <svg viewBox="0 0 24 24" aria-hidden="true">
    <defs><linearGradient id={gid} x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color={c[0]} /><stop offset="1" stop-color={c[1]} /></linearGradient></defs>
    <path fill="url(#{gid})" stroke="rgba(0,0,0,.25)" stroke-width=".8" stroke-linejoin="round"
      d="M12 1.8l3.1 6.3 6.9 1-5 4.9 1.2 6.9L12 17.6l-6.2 3.3L7 14l-5-4.9 6.9-1z" />
    {#if kind === 0}<path fill="none" stroke="rgba(255,255,255,.85)" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" d="M9 12.3l2.1 2.1 4-4.2" />{/if}
  </svg>
</span>

<style>
  .sm { display: inline-grid; place-items: center; width: var(--s); height: var(--s); flex-shrink: 0; vertical-align: -0.15em; margin-left: 3px; }
  .sm svg { width: 100%; height: 100%; display: block; filter: drop-shadow(0 0 3px color-mix(in srgb, var(--g) 55%, transparent)); }
  .sm.official svg { filter: drop-shadow(0 0 5px color-mix(in srgb, var(--g) 80%, transparent)); }
  .sm.diamond svg { animation: shine 3s ease-in-out infinite; }
  @keyframes shine { 50% { filter: drop-shadow(0 0 6px #9fe8ff) hue-rotate(40deg); } }
</style>
