<script lang="ts">
  /**
   * Stories pinned to a profile (Telegram's "Posts" row): small 9:16 tiles,
   * a tap opens them in the story viewer. Mine also lead to the archive.
   */
  import { t, tr } from "../i18n";
  import { api, assetUrl } from "../tauri";
  import { peerPresence } from "../presence";
  import { pinnedStories, saneStories, storyArchive, openStories, storyFile, storiesOff, type Story } from "../stories";
  import { showToast } from "./Toast.svelte";

  let { user, onopen }: { user: string; onopen?: () => void } = $props();
  const me = $derived(user === "");
  const list = $derived<Story[]>($storiesOff ? [] : me ? $pinnedStories.slice().reverse() : saneStories($peerPresence[user]?.sp, Date.now(), true).reverse());
  let thumbs = $state<Record<string, string>>({});
  $effect(() => {
    for (const s of list) {
      if (thumbs[s.id] !== undefined || s.media.kind !== "image") continue;
      thumbs[s.id] = "";
      void (me ? storyFile(s) : api.fetchAttachment(s.media)).then((p) => (thumbs[s.id] = assetUrl(p))).catch(() => {});
    }
  });
  function open(i: number) { onopen?.(); openStories([user], user, "pinned", i); }
  function archive() {
    if (!$storyArchive.length) { showToast(tr("The archive is empty — stories land here when they run out")); return; }
    onopen?.();
    openStories([""], "", "archive");
  }
</script>

{#if list.length || (me && !$storiesOff)}
  <section class="st-tiles">
    <div class="head">
      <span>{$t("Stories")}</span>
      {#if me}<button class="arch" onclick={archive}>{$t("Story archive")}{#if $storyArchive.length} · {$storyArchive.length}{/if}</button>{/if}
    </div>
    {#if list.length}
      <div class="row">
        {#each list as s, i (s.id)}
          <button class="tile" onclick={() => open(i)} style:background-image={thumbs[s.id] ? `url("${thumbs[s.id]}")` : undefined} aria-label={$t("Story")}>
            {#if s.media.kind === "video"}<span class="play">▶</span>{/if}
          </button>
        {/each}
      </div>
    {:else}
      <div class="empty">{$t("Pin a story from its menu (⋮) or the archive — it stays in your profile.")}</div>
    {/if}
  </section>
{/if}

<style>
  .st-tiles { padding: 4px 0 8px; }
  .head { display: flex; align-items: center; justify-content: space-between; gap: 8px; margin-bottom: 8px;
    font-size: 12px; font-weight: 700; text-transform: uppercase; letter-spacing: .04em; color: var(--fg-muted); }
  .arch { border: none; background: transparent; color: var(--accent); font: 600 12.5px var(--font-ui, inherit); cursor: pointer; text-transform: none; letter-spacing: 0; padding: 2px 0; }
  .row { display: flex; gap: 6px; overflow-x: auto; scrollbar-width: none; }
  .row::-webkit-scrollbar { display: none; }
  .tile { position: relative; flex-shrink: 0; width: 74px; aspect-ratio: 9 / 16; border: none; padding: 0; border-radius: calc(10px * var(--rk, 1)); cursor: pointer;
    background: linear-gradient(135deg, var(--accent), var(--accent-2)) center / cover; }
  .tile:hover { filter: brightness(1.08); }
  .play { position: absolute; inset: 0; display: grid; place-items: center; color: #fff; font-size: 18px; text-shadow: 0 1px 4px rgba(0,0,0,.6); }
  .empty { font-size: 13px; color: var(--fg-muted); }
</style>
