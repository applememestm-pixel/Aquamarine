<script lang="ts">
  /** Phone: tapping the "now playing" strip opens this sheet (Telegram's
   *  music list): the queue, the playing track with its progress and
   *  controls, a menu of repeat / shuffle / reverse; ↗ opens the full player. */
  import { t } from "../i18n";
  import {
    nowPlaying, queue, queueIndex, playerPaused, playerProgress, togglePlayback,
    nextTrack, prevTrack, expandPlayer, repeatAll, repeatOne, shuffleQueue, reverseQueue, playAt, type Track,
  } from "../player";

  let { onclose }: { onclose: () => void } = $props();
  let menu = $state(false);

  const list = $derived<Track[]>($queue.length ? $queue : $nowPlaying ? [$nowPlaying] : []);
  const cur = $derived($queue.length ? $queueIndex : 0);
  const artistOf = (x: Track) => x.nd?.artist || x.sub || "";
  const mode = $derived($repeatOne ? "one" : $repeatAll ? "all" : "off");

  function setMode(m: "one" | "all" | "shuffle" | "reverse") {
    menu = false;
    if (m === "one") { repeatOne.update((v) => !v); repeatAll.set(false); }
    else if (m === "all") { repeatAll.update((v) => !v); repeatOne.set(false); }
    else if (m === "shuffle") shuffleQueue();
    else reverseQueue();
  }
  function full() { onclose(); expandPlayer(); }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="qs-veil" data-x onclick={onclose}></div>
<div class="qs" role="dialog" aria-label={$t("Queue")} data-back>
  <div class="qs-grip" aria-hidden="true"></div>
  <div class="qs-list">
    {#each list as tr, i (i + tr.src)}
      <button class="qs-row" class:on={i === cur} onclick={() => ($queue.length ? playAt(i) : togglePlayback())}>
        <span class="qs-art" style:background-image={tr.cover ? `url("${tr.cover}")` : undefined}>
          {#if i === cur}
            <span class="qs-state">{#if $playerPaused}▶{:else}<i></i><i></i><i></i>{/if}</span>
          {:else if !tr.cover}♪{/if}
        </span>
        <span class="qs-txt"><b>{tr.title}</b>{#if artistOf(tr)}<small>{artistOf(tr)}</small>{/if}</span>
      </button>
    {/each}
  </div>

  {#if $nowPlaying}
    <div class="qs-now">
      <div class="qs-line"><span style:width={`${$playerProgress * 100}%`}></span></div>
      <div class="qs-ctl">
        <div class="qs-modewrap">
          <button class="qs-btn" class:act={mode !== "off"} onclick={() => (menu = !menu)} aria-label={$t("Order")}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M17 2l3 3-3 3"/><path d="M3 11v-1a4 4 0 0 1 4-4h13"/><path d="M7 22l-3-3 3-3"/><path d="M21 13v1a4 4 0 0 1-4 4H4"/>{#if mode === "one"}<text x="12" y="15" font-size="7" text-anchor="middle" fill="currentColor" stroke="none">1</text>{/if}</svg>
          </button>
          {#if menu}
            <div class="qs-menu" role="menu">
              <button role="menuitem" class:on={mode === "one"} onclick={() => setMode("one")}>{$t("Repeat track")}</button>
              <button role="menuitem" class:on={mode === "all"} onclick={() => setMode("all")}>{$t("Repeat list")}</button>
              <button role="menuitem" onclick={() => setMode("shuffle")} disabled={$queue.length < 2}>{$t("Shuffle list")}</button>
              <button role="menuitem" onclick={() => setMode("reverse")} disabled={$queue.length < 2}>{$t("Reverse order")}</button>
            </div>
          {/if}
        </div>
        <button class="qs-btn" onclick={() => prevTrack()} disabled={$queue.length < 2} aria-label={$t("Previous")}>
          <svg viewBox="0 0 24 24" fill="currentColor"><path d="M6 5h2v14H6zM20 5v14L9 12z"/></svg>
        </button>
        <button class="qs-btn big" onclick={togglePlayback} aria-label={$playerPaused ? $t("Play") : $t("Pause")}>
          {#if $playerPaused}<svg viewBox="0 0 24 24" fill="currentColor"><path d="M7 4.5v15l13-7.5z"/></svg>
          {:else}<svg viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="4.5" width="4.2" height="15" rx="1"/><rect x="13.8" y="4.5" width="4.2" height="15" rx="1"/></svg>{/if}
        </button>
        <button class="qs-btn" onclick={() => nextTrack()} disabled={$queue.length < 2} aria-label={$t("Next")}>
          <svg viewBox="0 0 24 24" fill="currentColor"><path d="M16 5h2v14h-2zM4 5v14l11-7z"/></svg>
        </button>
        <button class="qs-btn" onclick={full} aria-label={$t("Open player")}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M15 3h6v6M9 21H3v-6M21 3l-7 7M3 21l7-7"/></svg>
        </button>
      </div>
    </div>
  {/if}
</div>

<style>
  .qs-veil { position: fixed; inset: 0; z-index: 140; background: rgba(0, 0, 0, 0.35); animation: qs-fade .18s ease-out; }
  .qs {
    position: fixed; left: 0; right: 0; bottom: 0; z-index: 141;
    max-height: 78vh; display: flex; flex-direction: column;
    background: var(--bg-surface); border-top: 1px solid var(--border-strong);
    border-radius: calc(20px * var(--rk, 1)) calc(20px * var(--rk, 1)) 0 0;
    box-shadow: 0 -18px 40px -20px rgba(0, 0, 0, 0.7);
    padding-bottom: env(safe-area-inset-bottom, 0px);
    animation: qs-up .24s cubic-bezier(.2, .9, .25, 1);
  }
  .qs-grip { width: 40px; height: 4px; border-radius: 2px; background: var(--fg-dim); opacity: .5; margin: 8px auto 4px; flex-shrink: 0; }
  .qs-list { overflow-y: auto; overscroll-behavior: contain; padding: 4px 0; }
  .qs-row { display: flex; align-items: center; gap: 14px; width: 100%; padding: 8px 16px; border: none; background: none; color: var(--fg); font: inherit; text-align: left; cursor: pointer; }
  .qs-row.on { background: var(--accent-soft); }
  .qs-art {
    width: 48px; height: 48px; flex-shrink: 0; border-radius: 50%;
    background: var(--bg-input) center / cover no-repeat; display: grid; place-items: center; color: var(--fg-dim); font-size: 18px; position: relative; overflow: hidden;
  }
  .qs-state { position: absolute; inset: 0; display: flex; align-items: center; justify-content: center; gap: 3px; background: color-mix(in srgb, var(--bg-void) 45%, transparent); color: var(--fg); font-size: 16px; }
  .qs-state i { width: 3px; height: 14px; background: var(--accent); border-radius: 2px; animation: qs-eq 0.9s ease-in-out infinite; }
  .qs-state i:nth-child(2) { animation-delay: .15s; }
  .qs-state i:nth-child(3) { animation-delay: .3s; }
  .qs-txt { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .qs-txt b { font-weight: 600; font-size: 15px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .qs-txt small { color: var(--fg-muted); font-size: 13px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .qs-now { border-top: 1px solid var(--border); flex-shrink: 0; }
  .qs-line { height: 3px; background: var(--bg-input); }
  .qs-line span { display: block; height: 100%; background: var(--accent); transition: width .25s linear; }
  .qs-ctl { display: flex; align-items: center; justify-content: space-around; padding: 8px 10px 10px; }
  .qs-btn { width: 44px; height: 44px; border: none; background: none; color: var(--fg); display: grid; place-items: center; border-radius: 50%; cursor: pointer; }
  .qs-btn svg { width: 22px; height: 22px; }
  .qs-btn.big { width: 54px; height: 54px; background: var(--accent); color: var(--bg-void); }
  .qs-btn.big svg { width: 26px; height: 26px; }
  .qs-btn.act { color: var(--accent); }
  .qs-btn:disabled { opacity: .35; }
  .qs-modewrap { position: relative; }
  .qs-menu {
    position: absolute; left: 0; bottom: calc(100% + 8px); min-width: 210px; padding: 4px 0;
    background: var(--bg-surface-2, var(--bg-surface)); border: 1px solid var(--border-strong);
    border-radius: calc(14px * var(--rk, 1)); box-shadow: 0 14px 36px -12px rgba(0, 0, 0, 0.6);
  }
  .qs-menu button { display: block; width: 100%; padding: 12px 16px; border: none; background: none; color: var(--fg); font: inherit; font-size: 15px; text-align: left; cursor: pointer; }
  .qs-menu button.on { color: var(--accent); font-weight: 600; }
  .qs-menu button:disabled { opacity: .4; }
  @keyframes qs-up { from { transform: translateY(100%); } }
  @keyframes qs-fade { from { opacity: 0; } }
  @keyframes qs-eq { 0%, 100% { transform: scaleY(.35); } 50% { transform: scaleY(1); } }
</style>
