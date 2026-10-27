<script module lang="ts">
  import { writable } from "svelte/store";
  /** Channel whose statistics are open (null = closed). */
  export const statsFor = writable<string | null>(null);
  export interface StatPost { key: string; id: string; text: string; time: string; reactions: number; comments: number }
</script>

<script lang="ts">
  /** Channel statistics for admins: views (who and when, for a month),
   *  reactions and comments per post, and how a post was read over time. */
  import { t, tr, tn, lang } from "../i18n";
  import { channelStats, viewCounts, DETAIL_DAYS } from "../channelStats";
  import { avatars } from "../people";

  let { gid, posts, subscribers }: { gid: string; posts: StatPost[]; subscribers: number } = $props();

  let stats = $derived($channelStats[gid] ?? {});
  let rows = $derived(posts.map((p) => ({ ...p, views: Math.max(stats[p.key]?.n ?? 0, $viewCounts[gid]?.[p.key] ?? 0), st: stats[p.key] })).reverse());
  let total = $derived(rows.reduce((a, r) => a + r.views, 0));
  let reacts = $derived(rows.reduce((a, r) => a + r.reactions, 0));
  let comms = $derived(rows.reduce((a, r) => a + r.comments, 0));
  let avg = $derived(rows.length ? Math.round(total / rows.length) : 0);
  let reach = $derived(subscribers > 1 ? Math.round((avg / (subscribers - 1)) * 100) : 0);
  let sel = $state<string | null>(null);
  let cur = $derived(rows.find((r) => r.key === sel) ?? null);
  let peak = $derived(cur?.st ? Math.max(1, ...cur.st.h) : 1);
  const loc = () => ($lang === "ru" ? "ru-RU" : undefined);
  const when = (ms: number) => new Date(ms).toLocaleString(loc(), { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" });
</script>

<aside class="cs" data-back aria-label={$t("Channel statistics")}>
  <header class="cs-head">
    {#if cur}<button class="cs-back" onclick={() => (sel = null)} aria-label={$t("Back")}>‹</button>{/if}
    <b>📊 {cur ? $t("Post") : $t("Statistics")}</b>
    <button data-x class="cs-x" onclick={() => statsFor.set(null)} aria-label={$t("Close")}>✕</button>
  </header>
  <div class="cs-body">
    {#if !cur}
      <div class="cs-tiles">
        <div class="cs-tile"><b>{subscribers}</b><span>{$t("Subscribers")}</span></div>
        <div class="cs-tile"><b>{avg}</b><span>{$t("Views per post")}</span></div>
        <div class="cs-tile"><b>{reach}%</b><span>{$t("Reach")}</span></div>
        <div class="cs-tile"><b>{reacts}</b><span>{$t("Reactions")}</span></div>
        <div class="cs-tile"><b>{comms}</b><span>{$t("Comments")}</span></div>
        <div class="cs-tile"><b>{total}</b><span>{$t("Views in total")}</span></div>
      </div>
      <div class="cs-lbl">{$t("Posts")}</div>
      {#if !rows.length}<div class="cs-empty">{$t("No posts yet.")}</div>{/if}
      {#each rows as r (r.key)}
        <button class="cs-post" onclick={() => (sel = r.key)}>
          <span class="cs-pt">{r.text || "…"}</span>
          <span class="cs-pm">👁 {r.views} · ❤ {r.reactions} · 💬 {r.comments} · {r.time}</span>
          <span class="cs-bar"><i style:width="{subscribers > 1 ? Math.min(100, (r.views / (subscribers - 1)) * 100) : 0}%"></i></span>
        </button>
      {/each}
      <div class="cs-note">{tr("Who read a post and when is kept for {d} days after it was first read, then only the counters stay. It's all counted on the admins' devices — servers never learn who read what.", { d: DETAIL_DAYS })}</div>
    {:else}
      <div class="cs-root">{cur.text || "…"}</div>
      <div class="cs-tiles three">
        <div class="cs-tile"><b>{cur.views}</b><span>{$t("Views")}</span></div>
        <div class="cs-tile"><b>{cur.reactions}</b><span>{$t("Reactions")}</span></div>
        <div class="cs-tile"><b>{cur.comments}</b><span>{$t("Comments")}</span></div>
      </div>
      {#if cur.st?.h?.length}
        <div class="cs-lbl">{$t("Views in the first 48 hours")}</div>
        <div class="cs-chart" role="img" aria-label={$t("Views per hour")}>
          {#each cur.st.h as n, i (i)}<i style:height="{(n / peak) * 100}%" title={tr("Hour {h}: {n}", { h: i + 1, n })}></i>{/each}
        </div>
      {/if}
      <div class="cs-lbl">{$t("Who viewed")}</div>
      {#if cur.st?.v}
        {#each Object.entries(cur.st.v).sort((a, b) => a[1] - b[1]) as [u, at] (u)}
          <div class="cs-viewer">
            <span class="cs-av" style:background-image={$avatars[u] ? `url("${$avatars[u]}")` : undefined}>{$avatars[u] ? "" : u.slice(0, 1).toUpperCase()}</span>
            <span class="cs-vn">@{u}</span>
            <span class="cs-vt">{when(at)}</span>
          </div>
        {/each}
        {#if !Object.keys(cur.st.v).length}<div class="cs-empty">{$t("Nobody yet.")}</div>{/if}
      {:else}
        <div class="cs-empty">{cur.st ? tr("The details are older than {d} days — only the counter is kept.", { d: DETAIL_DAYS }) : $t("Nobody yet.")}</div>
      {/if}
    {/if}
  </div>
</aside>

<style>
  .cs { position: absolute; top: 0; right: 0; bottom: 0; width: min(380px, 100%); z-index: 26; display: flex; flex-direction: column; background: var(--bg-shell); border-left: 1px solid var(--border-strong); box-shadow: -16px 0 40px -22px rgba(0,0,0,0.6); animation: cs-in 0.2s cubic-bezier(.2,.9,.3,1); }
  @keyframes cs-in { from { transform: translateX(24px); opacity: 0; } }
  .cs-head { display: flex; align-items: center; gap: 8px; padding: 14px 12px 12px 16px; border-bottom: 1px solid var(--border); }
  .cs-head b { flex: 1; font-size: 15px; }
  .cs-back, .cs-x { width: 28px; height: 28px; border-radius: 50%; border: none; background: var(--bg-hover); color: var(--fg-muted); cursor: pointer; font-size: 15px; }
  .cs-body { flex: 1; overflow-y: auto; padding: 14px; display: flex; flex-direction: column; gap: 10px; }
  .cs-tiles { display: grid; grid-template-columns: repeat(2, 1fr); gap: 8px; }
  .cs-tiles.three { grid-template-columns: repeat(3, 1fr); }
  .cs-tile { padding: 10px 12px; border-radius: calc(12px * var(--rk, 1)); background: var(--bg-input); display: flex; flex-direction: column; gap: 2px; }
  .cs-tile b { font-size: 20px; color: var(--accent); font-variant-numeric: tabular-nums; }
  .cs-tile span { font-size: 11.5px; color: var(--fg-muted); }
  .cs-lbl { margin-top: 6px; font-size: 11.5px; font-weight: 800; color: var(--fg-muted); text-transform: uppercase; letter-spacing: 0.04em; }
  .cs-empty { font-size: 12.5px; color: var(--fg-dim); }
  .cs-post { display: flex; flex-direction: column; gap: 4px; padding: 10px 12px; border-radius: calc(11px * var(--rk, 1)); border: 1px solid var(--border); background: var(--bg-input); color: var(--fg); font: inherit; text-align: left; cursor: pointer; }
  .cs-post:hover { border-color: var(--accent); }
  .cs-pt { font-size: 13px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .cs-pm { font-size: 11.5px; color: var(--fg-dim); font-variant-numeric: tabular-nums; }
  .cs-bar { height: 3px; border-radius: 2px; background: var(--bg-hover); overflow: hidden; }
  .cs-bar i { display: block; height: 100%; background: linear-gradient(90deg, var(--accent), var(--accent-2)); }
  .cs-note { margin-top: 8px; font-size: 11.5px; color: var(--fg-dim); line-height: 1.5; }
  .cs-root { padding: 10px 12px; border-radius: calc(12px * var(--rk, 1)); background: var(--bg-input); border-left: 3px solid var(--accent); font-size: 13.5px; line-height: 1.45; max-height: 120px; overflow: hidden; }
  .cs-chart { height: 90px; display: flex; align-items: flex-end; gap: 2px; padding: 6px; border-radius: calc(10px * var(--rk, 1)); background: var(--bg-input); }
  .cs-chart i { flex: 1; min-height: 2px; border-radius: 2px 2px 0 0; background: linear-gradient(180deg, var(--accent), var(--accent-2)); }
  .cs-viewer { display: flex; align-items: center; gap: 9px; padding: 4px 2px; }
  .cs-av { width: 26px; height: 26px; border-radius: 50%; display: grid; place-items: center; font-size: 11px; font-weight: 800; color: #06070C; background: linear-gradient(135deg, var(--accent), var(--accent-2)); background-size: cover; background-position: center; }
  .cs-vn { flex: 1; font-size: 13px; }
  .cs-vt { font-size: 11.5px; color: var(--fg-dim); }
</style>
