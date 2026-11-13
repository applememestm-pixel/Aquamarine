<script lang="ts">
  /**
   * Telegram's story archive: every story I've posted — the live ones and
   * the ones that ran out — newest first, as a grid. A tap opens it in the
   * story viewer (pin to profile, delete there). Nothing leaves it unless I
   * delete it: no limit, the 10-live cap sends the oldest here too.
   */
  import { t } from "../i18n";
  import { assetUrl } from "../tauri";
  import { onBack } from "../back";
  import { myStories, storyArchive, pinnedStories, storyArchiveOpen, allMyStories, openStories, storyFile, isLive, type Story } from "../stories";

  const open = $derived($storyArchiveOpen);
  const list = $derived<Story[]>(allMyStories($myStories, $storyArchive));
  const pinned = $derived(new Set($pinnedStories.map((p) => p.from ?? p.id)));
  let thumbs = $state<Record<string, string>>({});
  $effect(() => {
    if (!open) return;
    for (const s of list) {
      if (thumbs[s.id] !== undefined || s.media.kind !== "image") continue;
      thumbs[s.id] = "";
      void storyFile(s).then((p) => (thumbs[s.id] = assetUrl(p))).catch(() => {});
    }
  });
  function close() { storyArchiveOpen.set(false); }
  $effect(() => { if (open) return onBack(close); });
  const day = (at: number) => new Date(at).toLocaleDateString([], { day: "numeric", month: "short" });
  function show(i: number) { openStories([""], "", "archive", i); }
</script>

{#if open}
  <div class="sa" role="dialog" aria-modal="true" aria-label={$t("Story archive")} data-back>
    <header>
      <span class="title">{$t("Story archive")}{#if list.length} · {list.length}{/if}</span>
      <button class="x" data-x onclick={close} aria-label={$t("Close")}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="M6 6l12 12M18 6L6 18"/></svg>
      </button>
    </header>
    {#if list.length}
      <div class="note">{$t("All your stories, the live ones too — only you see this.")}</div>
      <div class="grid">
        {#each list as s, i (s.id)}
          <button class="tile" onclick={() => show(i)} style:background-image={thumbs[s.id] ? `url("${thumbs[s.id]}")` : undefined} aria-label={$t("Story")}>
            {#if s.media.kind === "video"}<span class="play">▶</span>{/if}
            <span class="when">{day(s.at)}</span>
            {#if isLive(s)}<span class="tag live">{$t("Live now")}</span>{:else if pinned.has(s.id)}<span class="tag">📌</span>{/if}
          </button>
        {/each}
      </div>
    {:else}
      <div class="empty">{$t("No stories yet — the ones you post stay here.")}</div>
    {/if}
  </div>
{/if}

<style>
  .sa { position: fixed; inset: 0; z-index: 10090; display: flex; flex-direction: column; background: var(--bg-shell); color: var(--fg);
    padding: env(safe-area-inset-top) 0 env(safe-area-inset-bottom); }
  header { display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: 12px 16px; border-bottom: 1px solid var(--border); }
  .title { font-weight: 700; font-size: 16px; }
  .x { width: 34px; height: 34px; display: grid; place-items: center; border: none; border-radius: calc(10px * var(--rk, 1)); background: transparent; color: var(--fg-muted); cursor: pointer; }
  .x:hover { background: var(--bg-hover); color: var(--fg); }
  .x svg { width: 18px; height: 18px; }
  .note { padding: 10px 16px 4px; font-size: 12.5px; color: var(--fg-muted); }
  .grid { flex: 1; overflow-y: auto; display: grid; grid-template-columns: repeat(auto-fill, minmax(110px, 1fr)); gap: 6px; padding: 10px 16px 16px; align-content: start; }
  .tile { position: relative; aspect-ratio: 9 / 16; border: none; padding: 0; cursor: pointer; border-radius: calc(10px * var(--rk, 1)); overflow: hidden;
    background: linear-gradient(135deg, var(--accent), var(--accent-2)) center / cover; }
  .tile:hover { filter: brightness(1.08); }
  .play { position: absolute; inset: 0; display: grid; place-items: center; color: #fff; font-size: 22px; text-shadow: 0 1px 4px rgba(0,0,0,.6); }
  .when { position: absolute; left: 6px; bottom: 6px; font-size: 11.5px; font-weight: 700; color: #fff; text-shadow: 0 1px 4px rgba(0,0,0,.75); }
  .tag { position: absolute; right: 6px; top: 6px; font-size: 10.5px; font-weight: 800; color: #fff; background: rgba(0,0,0,.45); border-radius: 6px; padding: 1px 5px; }
  .tag.live { background: var(--accent); color: #061015; }
  .empty { margin: auto; padding: 24px; text-align: center; color: var(--fg-muted); font-size: 14px; }
</style>
