<script module lang="ts">
  import { writable } from "svelte/store";
  /** cid → playable URL, shared by every playlist (chat card + reader). */
  const trackUrls = writable<Record<string, string>>({});
</script>

<script lang="ts">
  /**
   * The music attached to an article — a small playlist. Tracks are
   * downloaded + decrypted on first play and handed to the app-wide player,
   * so the music keeps going while you read, scroll or close the article.
   */
  import type { ArticleMedia } from "../types";
  import { mediaUrl, fmtDuration, fmtSize } from "../media";
  import { playQueue, nowPlaying } from "../player";
  import { showToast } from "./Toast.svelte";
  import { t, tr } from "../i18n";

  interface Props {
    tracks: ArticleMedia[];
    /** Shown as the album in the player. */
    title?: string;
    cover?: string;
    compact?: boolean;
  }
  let { tracks, title = "", cover, compact = false }: Props = $props();

  let loading = $state<number | null>(null);
  async function urlOf(m: ArticleMedia): Promise<string> {
    const have = $trackUrls[m.cid];
    if (have) return have;
    const u = await mediaUrl(m);
    trackUrls.update((x) => ({ ...x, [m.cid]: u }));
    return u;
  }

  const nice = (name: string) => name.replace(/\.[a-z0-9]{2,5}$/i, "").replace(/[_]+/g, " ");

  // The app-wide player's <audio> (TopPlayer): click a playing track to pause.
  const topAudio = () => document.querySelector<HTMLAudioElement>(".top-player audio");
  // "Actually playing" (not just loaded): the animated bars show only then.
  let paused = $state(true);
  $effect(() => {
    const sync = () => { const a = topAudio(); paused = !a || a.paused || a.ended; };
    const on = (e: Event) => { if (e.target === topAudio()) sync(); };
    sync();
    for (const ev of ["play", "playing", "pause", "ended", "emptied"]) document.addEventListener(ev, on, true);
    return () => { for (const ev of ["play", "playing", "pause", "ended", "emptied"]) document.removeEventListener(ev, on, true); };
  });

  async function play(i: number) {
    if (loading != null) return;
    if (playingCid && playingCid === tracks[i]?.cid) {
      const a = topAudio();
      if (a) { if (a.paused) void a.play().catch(() => {}); else a.pause(); return; }
    }
    loading = i;
    try {
      // The clicked track first, then the rest.
      await urlOf(tracks[i]);
      const got = await Promise.allSettled(tracks.map(urlOf));
      const queue = tracks.map((m, k) => ({ m, k, u: got[k].status === "fulfilled" ? (got[k] as PromiseFulfilledResult<string>).value : "" })).filter((x) => x.u);
      const at = Math.max(0, queue.findIndex((x) => x.k === i));
      playQueue(queue.map(({ m, u }) => ({ src: u, title: nice(m.name), sub: title || $t("Article"), cover })), at);
    } catch (e) {
      showToast(tr("Could not load the track: {e}", { e: e instanceof Error ? e.message : String(e) }), 12000);
    } finally {
      loading = null;
    }
  }
  let playingCid = $derived.by(() => {
    const src = $nowPlaying?.src;
    return src ? Object.keys($trackUrls).find((k) => $trackUrls[k] === src) ?? null : null;
  });
  let shown = $derived(compact ? tracks.slice(0, 3) : tracks);
</script>

{#if tracks.length}
  <div class="music" class:compact>
    <div class="m-head">
      <span class="m-title">🎵 {$t("Music")} · {tracks.length}</span>
      <button class="m-all" onclick={(e) => { e.stopPropagation(); void play(0); }} disabled={loading != null}>▶ {$t("Play all")}</button>
    </div>
    {#each shown as tr_, i (tr_.cid)}
      <button class="trk" class:on={playingCid === tr_.cid} onclick={(e) => { e.stopPropagation(); void play(i); }} disabled={loading != null && loading !== i}>
        <span class="tb">
          {#if loading === i}<span class="spin"></span>
          {:else if playingCid === tr_.cid && !paused}<span class="eq"><i></i><i></i><i></i></span>
          {:else if playingCid === tr_.cid}<svg viewBox="0 0 24 24"><path d="M8 5v14l11-7z" fill="currentColor"/></svg>
          {:else}<svg viewBox="0 0 24 24"><path d="M8 5v14l11-7z" fill="currentColor"/></svg>{/if}
        </span>
        <span class="tn">{nice(tr_.name)}</span>
        <span class="td">{fmtDuration(tr_.duration) || fmtSize(tr_.size)}</span>
      </button>
    {/each}
    {#if compact && tracks.length > shown.length}
      <div class="more">+{tracks.length - shown.length}</div>
    {/if}
  </div>
{/if}

<style>
  .music {
    margin: 0.9em 0;
    padding: 8px;
    border: 1px solid var(--border);
    border-radius: calc(12px * var(--rk, 1));
    background: var(--bg-list);
  }
  .music.compact { margin: 6px 0 2px; padding: 6px; }
  .m-head { display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: 2px 4px 6px; }
  .m-title { font-size: 12px; font-weight: 800; letter-spacing: 0.05em; text-transform: uppercase; color: var(--accent); }
  .m-all {
    border: none; border-radius: calc(8px * var(--rk, 1)); padding: 4px 10px; cursor: pointer;
    background: color-mix(in srgb, var(--accent) 16%, transparent); color: var(--accent);
    font-size: 12px; font-weight: 700;
  }
  .trk {
    width: 100%; display: flex; align-items: center; gap: 10px;
    padding: 6px; border: none; border-radius: calc(8px * var(--rk, 1)); background: transparent;
    color: var(--fg); cursor: pointer; text-align: left; font-size: 13.5px;
  }
  .trk:hover { background: var(--bg-hover); }
  .trk.on { background: var(--accent-soft); }
  .trk:disabled { opacity: 0.6; cursor: wait; }
  .tb {
    flex: 0 0 32px; height: 32px; border-radius: 50%;
    display: grid; place-items: center;
    background: linear-gradient(135deg, var(--accent), var(--accent-2)); color: #06070c;
  }
  .tb svg { width: 15px; height: 15px; margin-left: 2px; }
  .tn { flex: 1; min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .td { font-size: 11.5px; color: var(--fg-dim); font-variant-numeric: tabular-nums; }
  .more { font-size: 12px; color: var(--fg-dim); padding: 2px 8px; }
  .spin { width: 14px; height: 14px; border-radius: 50%; border: 2px solid rgba(6, 7, 12, 0.3); border-top-color: #06070c; animation: m-spin 0.8s linear infinite; }
  @keyframes m-spin { to { transform: rotate(360deg); } }
  /* Bars scale with transform, not height: animating height re-ran layout
     for the whole chat every frame (WebKitGTK: ~95% CPU, 20 fps). */
  .eq { display: flex; gap: 2px; align-items: flex-end; height: 12px; contain: strict; width: 13px; }
  .eq i { width: 3px; height: 12px; background: #06070c; border-radius: 1px; transform-origin: 50% 100%; transform: scaleY(0.34); animation: m-eq 0.9s ease-in-out infinite; }
  .eq i:nth-child(2) { animation-delay: -0.3s; }
  .eq i:nth-child(3) { animation-delay: -0.6s; }
  @keyframes m-eq { 0%, 100% { transform: scaleY(0.34); } 50% { transform: scaleY(1); } }
</style>
