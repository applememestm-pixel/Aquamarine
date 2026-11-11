<script lang="ts">
  /** The check mark the relay's @verify bot gives (Telegram's blue rosette). */
  import { t } from "../i18n";
  let { size = 16 }: { size?: number } = $props();
</script>

<span class="om" title={$t("Verified by Aquamarine")} style:--s="{size}px">
  <svg viewBox="0 0 24 24" aria-hidden="true">
    <path fill="currentColor" d="M12 1.6l2.3 1.7 2.8-.3 1.1 2.6 2.6 1.1-.3 2.8 1.7 2.3-1.7 2.3.3 2.8-2.6 1.1-1.1 2.6-2.8-.3L12 22.4l-2.3-1.7-2.8.3-1.1-2.6-2.6-1.1.3-2.8L1.8 12l1.7-2.3-.3-2.8 2.6-1.1 1.1-2.6 2.8.3z"/>
    <path fill="none" stroke="var(--bg-shell, #0c0f17)" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" d="M8 12.2l2.7 2.7L16.2 9.4"/>
  </svg>
</span>

<style>
  .om { display: inline-grid; place-items: center; width: var(--s); height: var(--s); flex-shrink: 0; color: var(--accent); vertical-align: -0.15em; }
  .om svg { width: 100%; height: 100%; display: block; }
</style>
