<script lang="ts">
  /**
   * The CeZeOs star (aqua-server stars.rs; checked against the owner's key in
   * src/stars.rs, so nobody can give themselves one): a sun cut in eight, in
   * its level's colour. 1–5 — donation levels (Boosty), 6 — above them, only
   * the owner gives it. Older statements: 0 ("official") is 6, 7–8 are 5.
   * Small sizes (next to a name) get a simpler sun — 8 thick rays — whose
   * rays don't vanish at 13–16 px.
   */
  import { t, tr } from "../i18n";
  let { kind, size = 16 }: { kind: number; size?: number } = $props();
  /** Bronze, silver, gold, sapphire, amethyst; the owner's green. */
  const LEVELS = ["#C77B3A", "#B4C0CC", "#F5B417", "#2F86FF", "#A24DFF", "#189820"];
  const level = $derived(kind === 0 ? 6 : Math.max(1, Math.min(kind > 6 ? 5 : kind, 6)));
  const color = $derived(LEVELS[level - 1]);
  const small = $derived(size <= 20);
  const mid = `cz${Math.random().toString(36).slice(2, 8)}`;
  const label = $derived(level === 6 ? $t("CeZeOs star · from the owner") : tr("CeZeOs star · level {n}", { n: String(level) }));
</script>

<span class="sm" class:top={level === 6} title={label} aria-label={label} style:--s="{size}px" style:--g={color}>
  <svg viewBox="0 0 1024 1024" aria-hidden="true" fill={color}>
    <defs><mask id={mid}><rect width="1024" height="1024" fill="#fff" />
      <path stroke="#000" stroke-width={small ? 46 : 18} d="M512 190V834M190 512H834M284 284L740 740M740 284L284 740" /></mask></defs>
    {#if small}
      <circle cx="512" cy="512" r="330" mask="url(#{mid})" />
      <path d="M552 212L512 12L472 212ZM752 328L866 158L696 272ZM812 552L1012 512L812 472ZM696 752L866 866L752 696ZM472 812L512 1012L552 812ZM272 696L158 866L328 752ZM212 472L12 512L212 552ZM328 272L158 158L272 328Z" />
    {:else}
      <circle cx="512" cy="512" r="310" mask="url(#{mid})" />
      <path d="M521 190L512 60L503 190ZM654 190L683 100L639 184ZM746 296L843 181L728 278ZM840 385L924 341L834 370ZM834 521L964 512L834 503ZM834 654L924 683L840 639ZM728 746L843 843L746 728ZM639 840L683 924L654 834ZM503 834L512 964L521 834ZM370 834L341 924L385 840ZM278 728L181 843L296 746ZM184 639L100 683L190 654ZM190 503L60 512L190 521ZM190 370L100 341L184 385ZM296 278L181 181L278 296ZM385 184L341 100L370 190Z" />
    {/if}
  </svg>
</span>

<style>
  .sm { display: inline-grid; place-items: center; width: var(--s); height: var(--s); flex-shrink: 0; vertical-align: -0.15em; margin-left: 3px; }
  .sm svg { width: 100%; height: 100%; display: block; filter: drop-shadow(0 0 2px color-mix(in srgb, var(--g) 55%, transparent)); }
  .sm.top svg { filter: drop-shadow(0 0 4px color-mix(in srgb, var(--g) 80%, transparent)); }
</style>
