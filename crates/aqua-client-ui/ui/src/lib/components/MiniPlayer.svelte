<script lang="ts">
  // Telegram's "now playing" strip on phones: ▶ Artist – Title ✕ with a thin
  // progress line. Under the folder tabs in the chat list and under the
  // pinned message in a chat; the <audio> stays in TopPlayer.
  import { t } from "../i18n";
  import { nowPlaying, playerPaused, playerProgress, togglePlayback, expandPlayer, stopTrack } from "../player";

  let { inChat = false }: { inChat?: boolean } = $props();

  let artist = $derived($nowPlaying?.nd?.artist ?? "");
  let title = $derived($nowPlaying?.title ?? "");
</script>

{#if $nowPlaying}
  <div class="mp" class:in-chat={inChat}>
    <button class="mp-play" onclick={togglePlayback} aria-label={$playerPaused ? $t("Play") : $t("Pause")}>
      {#if $playerPaused}
        <svg viewBox="0 0 24 24" fill="currentColor"><path d="M7 4.5v15l13-7.5z"/></svg>
      {:else}
        <svg viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="4.5" width="4.2" height="15" rx="1"/><rect x="13.8" y="4.5" width="4.2" height="15" rx="1"/></svg>
      {/if}
    </button>
    <button class="mp-info" onclick={expandPlayer} title={$t("Open player")}>
      {#if artist}<b>{artist}</b><span class="mp-dash">&nbsp;-&nbsp;</span>{/if}<span class="mp-title">{title}</span>
    </button>
    <button class="mp-x" onclick={stopTrack} aria-label={$t("Stop")} title={$t("Stop")}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="M6 6l12 12M18 6 6 18"/></svg>
    </button>
    <span class="mp-bar" style:width={`${$playerProgress * 100}%`}></span>
  </div>
{/if}

<style>
  /* The menus' look (pop-ups, the account sheet): the shell colour as
     solid as Settings → See-through menus says, frosted, a thin border. */
  .mp {
    position: relative; flex-shrink: 0;
    display: flex; align-items: center; gap: 4px;
    height: 44px; margin: 6px 10px 4px; padding: 0 4px 0 8px;
    border-radius: calc(22px * var(--rk, 1));
    background: color-mix(in srgb, var(--bg-shell) var(--menu-alpha, 82%), transparent);
    backdrop-filter: blur(24px) saturate(1.5); -webkit-backdrop-filter: blur(24px) saturate(1.5);
    border: 1px solid var(--border-strong);
    box-shadow: 0 10px 30px -18px rgba(0, 0, 0, 0.8);
    overflow: hidden;
  }
  .mp.in-chat { margin: 6px 10px 2px; }
  .mp-play, .mp-x {
    width: 34px; height: 34px; flex-shrink: 0; padding: 0; border: none; background: none;
    display: grid; place-items: center; cursor: pointer;
  }
  .mp-play { color: var(--accent); }
  .mp-play svg { width: 20px; height: 20px; }
  .mp-x { color: var(--fg-dim); }
  .mp-x svg { width: 18px; height: 18px; }
  .mp-info {
    flex: 1; min-width: 0; border: none; background: none; padding: 0 4px; cursor: pointer;
    text-align: left; font: inherit; font-size: 15px;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--fg-dim);
  }
  .mp-info b { color: var(--fg); font-weight: 600; }
  .mp-bar {
    position: absolute; left: 0; bottom: 0; height: 2px;
    background: var(--accent); pointer-events: none;
    transition: width 0.25s linear;
  }
</style>
