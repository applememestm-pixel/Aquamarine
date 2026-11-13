<script lang="ts">
  /**
   * The check mark the relay's @verify bot gives: "this is the real person"
   * (Telegram's blue rosette). In the theme's own colours — a rosette from
   * the accent to the second accent, a white check — so it matches whatever
   * theme is on; set apart from the CeZeOs star (a support mark).
   */
  import { t } from "../i18n";
  let { size = 16 }: { size?: number } = $props();
  const gid = `om${Math.random().toString(36).slice(2, 8)}`;
</script>

<span class="om" title={$t("Verified by Aquamarine")} style:--s="{size}px">
  <svg viewBox="0 0 24 24" aria-hidden="true">
    <defs><linearGradient id={gid} x1="0" y1="0" x2="1" y2="1">
      <stop offset="0" style="stop-color: var(--accent, #00E5C7)" /><stop offset="1" style="stop-color: var(--accent-2, #7B5CFF)" />
    </linearGradient></defs>
    <path fill="url(#{gid})" d="M12 1.6l2.3 1.7 2.8-.3 1.1 2.6 2.6 1.1-.3 2.8 1.7 2.3-1.7 2.3.3 2.8-2.6 1.1-1.1 2.6-2.8-.3L12 22.4l-2.3-1.7-2.8.3-1.1-2.6-2.6-1.1.3-2.8L1.8 12l1.7-2.3-.3-2.8 2.6-1.1 1.1-2.6 2.8.3z"/>
    <path fill="none" stroke="#fff" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" d="M7.9 12.3l2.8 2.8 5.5-5.6"/>
  </svg>
</span>

<style>
  .om { display: inline-grid; place-items: center; width: var(--s); height: var(--s); flex-shrink: 0; vertical-align: -0.15em; margin-left: 3px; }
  .om svg { width: 100%; height: 100%; display: block; filter: drop-shadow(0 0 2px color-mix(in srgb, var(--accent, #00E5C7) 45%, transparent)); }
</style>
