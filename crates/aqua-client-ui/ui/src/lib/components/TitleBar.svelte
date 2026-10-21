<script lang="ts">
  import { appIconUrl } from "../appIcon";
  import { t } from "../i18n";
  /**
   * Our own window title bar (the native one is turned off in
   * tauri.conf.json → decorations:false). Drag anywhere on the bar, double-
   * click to maximise; neon window buttons on the right, Discord-style.
   */
  import { onMount } from "svelte";
  import { info } from "../stores";

  let maximized = $state(false);
  type Win = Awaited<ReturnType<typeof getWin>>;
  let win: Win | null = null;

  async function getWin() {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    return getCurrentWindow();
  }

  onMount(() => {
    let unlisten: (() => void) | null = null;
    void getWin().then(async (w) => {
      win = w;
      try { maximized = await w.isMaximized(); } catch { /* not in Tauri */ }
      try { unlisten = await w.onResized(async () => { try { maximized = await w.isMaximized(); } catch { /* ignore */ } }); } catch { /* ignore */ }
    }).catch(() => {});
    return () => unlisten?.();
  });

  const act = (f: (w: Win) => Promise<unknown>) => () => { if (win) void f(win).catch(() => {}); };
</script>

<div class="titlebar" data-tauri-drag-region>
  <div class="brand" data-tauri-drag-region>
    <img class="logo" src={$appIconUrl} alt="" data-tauri-drag-region draggable="false" />
    <span class="name" data-tauri-drag-region>{$t("Aquamarine")}</span>
    {#if $info?.connected}<span class="dot" data-tauri-drag-region title={$t("Connected")}></span>{/if}
  </div>
  <div class="spacer" data-tauri-drag-region></div>
  <div class="ctrls">
    <button class="wb" onclick={act((w) => w.minimize())} title={$t("Minimize")} aria-label={$t("Minimize")}>
      <svg viewBox="0 0 12 12"><line x1="2" y1="6.5" x2="10" y2="6.5" /></svg>
    </button>
    <button class="wb" onclick={act((w) => w.toggleMaximize())} title={$t(maximized ? "Restore" : "Maximize")} aria-label={$t("Maximize")}>
      {#if maximized}
        <svg viewBox="0 0 12 12"><rect x="2" y="4" width="6" height="6" rx="1" /><path d="M4 4V2.8A.8.8 0 0 1 4.8 2H9.2a.8.8 0 0 1 .8.8v4.4a.8.8 0 0 1-.8.8H8" /></svg>
      {:else}
        <svg viewBox="0 0 12 12"><rect x="2" y="2" width="8" height="8" rx="1.2" /></svg>
      {/if}
    </button>
    <button class="wb close" onclick={act((w) => w.close())} title={$t("Close")} aria-label={$t("Close")}>
      <svg viewBox="0 0 12 12"><line x1="2.5" y1="2.5" x2="9.5" y2="9.5" /><line x1="9.5" y1="2.5" x2="2.5" y2="9.5" /></svg>
    </button>
  </div>
</div>

<style>
  .titlebar {
    height: 32px;
    flex-shrink: 0;
    display: flex; align-items: center;
    background: var(--bg-rail);
    border-bottom: 1px solid var(--border);
    user-select: none;
    -webkit-user-select: none;
    position: relative;
    z-index: 400;
  }
  .brand { display: flex; align-items: center; gap: 8px; padding-left: 12px; height: 100%; }
  .logo {
    width: 18px; height: 18px; border-radius: 6px;
    display: grid; place-items: center;
    border-radius: 5px; transition: filter .3s;
    filter: drop-shadow(0 0 6px var(--accent-glow));
  }
  .name {
    font-size: 12.5px; font-weight: 800; letter-spacing: 0.02em;
    background: linear-gradient(90deg, var(--accent), var(--accent-2));
    -webkit-background-clip: text; background-clip: text; color: transparent;
    text-shadow: 0 0 12px color-mix(in srgb, var(--accent) 35%, transparent);
  }
  .dot { width: 6px; height: 6px; border-radius: 50%; background: var(--online); box-shadow: 0 0 6px var(--online); }
  .spacer { flex: 1; height: 100%; }
  .ctrls { display: flex; height: 100%; }
  .wb {
    width: 46px; height: 100%;
    border: none; background: transparent; color: var(--fg-muted);
    display: grid; place-items: center; cursor: pointer;
    transition: background 0.12s ease, color 0.12s ease, box-shadow 0.12s ease;
  }
  .wb svg { width: 12px; height: 12px; fill: none; stroke: currentColor; stroke-width: 1.3; }
  .wb:hover { background: var(--accent-soft); color: var(--accent); box-shadow: inset 0 -2px 0 var(--accent); }
  .wb.close:hover { background: #E81123; color: #fff; box-shadow: 0 0 16px -2px rgba(232, 17, 35, 0.7); }
  @media (max-width: 720px) { :global(html:not(.popout)) .titlebar { display: none; } }
</style>
