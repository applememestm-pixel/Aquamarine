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
  .mp {
    position: relative; flex-shrink: 0;
    display: flex; align-items: center; gap: 4px;
    height: 42px; margin: 6px 12px 2px; padding: 0 4px 0 8px;
    border-radius: calc(21px * var(--rk, 1));
    background: color-mix(in srgb, var(--bg-input) 88%, transparent);
    backdrop-filter: blur(var(--glass-blur, 12px)); -webkit-backdrop-filter: blur(var(--glass-blur, 12px));
    overflow: hidden;
  }
  /* In a chat: a full-width strip under the pinned message. */
  .mp.in-chat { margin: 0; border-radius: 0; border-bottom: 1px solid var(--border); background: var(--bg-list); }
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
  .mp:not(.in-chat) .mp-bar { display: none; }
</style>
