<script module lang="ts">
  import { writable } from "svelte/store";
  /** Channel whose statistics are open (null = closed). */
  export const statsFor = writable<string | null>(null);
  export interface StatPost { key: string; id: string; text: string; time: string; reactions: number; comments: number }
</script>

<script lang="ts">
  import { sideResize } from "../sideResize";
  /** Channel statistics for admins: views (who and when, for a month),
   *  reactions and comments per post, and how a post was read over time. */
  import { t, tr, tn, lang } from "../i18n";
  import { channelStats, viewCounts, DETAIL_DAYS } from "../channelStats";
  import { avatars } from "../people";
  import { icons } from "../icons";
  import { IS_MOBILE } from "../platform";
  const CLOSE = `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="M6 6l12 12M18 6 6 18"/></svg>`;

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
  /** A plain line chart (Telegram's statistics): grid, axis numbers, a line
   *  with a light area under it — no gradients, no glow. */
  const W = 320, H = 130, PL = 30, PB = 18, PT = 8, PR = 8;
  function chart(vals: number[]) {
    const max = Math.max(1, ...vals);
    const n = Math.max(1, vals.length - 1);
    const x = (i: number) => PL + (i / n) * (W - PL - PR);
    const y = (v: number) => PT + (1 - v / max) * (H - PT - PB);
    const pts = vals.map((v, i) => `${x(i).toFixed(1)},${y(v).toFixed(1)}`);
    const line = pts.length > 1 ? "M" + pts.join("L") : `M${PL},${y(vals[0] ?? 0)}L${W - PR},${y(vals[0] ?? 0)}`;
    const area = `${line}L${x(vals.length - 1)},${H - PB}L${PL},${H - PB}Z`;
    const ticks = [0, Math.round(max / 2), max].map((v) => ({ v, y: y(v) }));
    return { line, area, ticks, x, y, max };
  }
  const when = (ms: number) => new Date(ms).toLocaleString(loc(), { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" });
</script>

<aside class="cs" class:phone={IS_MOBILE} use:sideResize={"channelstats"} data-back aria-label={$t("Channel statistics")}>
  <header class="cs-head">
    {#if cur}<button data-x data-keep-x class="cs-x" onclick={() => (sel = null)} aria-label={$t("Back")}>{@html icons.back}</button>
    {:else if IS_MOBILE}<button data-x data-keep-x class="cs-x" onclick={() => statsFor.set(null)} aria-label={$t("Back")}>{@html icons.back}</button>{/if}
    <b>{cur ? $t("Post") : $t("Statistics")}</b>
    {#if !IS_MOBILE}<button data-x data-keep-x class="cs-x" onclick={() => statsFor.set(null)} aria-label={$t("Close")}>{@html CLOSE}</button>{/if}
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
      {#if rows.length > 1}
        {@const vals = [...rows].reverse().map((r) => r.views)}
        {@const c = chart(vals)}
        <div class="cs-lbl">{$t("Views per post")}</div>
        <svg class="cs-svg" viewBox="0 0 {W} {H}" role="img" aria-label={$t("Views per post")}>
          {#each c.ticks as tk (tk.v)}<line class="g" x1={PL} x2={W - PR} y1={tk.y} y2={tk.y} /><text class="ax" x={PL - 5} y={tk.y + 3} text-anchor="end">{tk.v}</text>{/each}
          <path class="ar" d={c.area} /><path class="ln" d={c.line} />
          <text class="ax" x={PL} y={H - 4}>{[...rows].reverse()[0]?.time ?? ""}</text>
          <text class="ax" x={W - PR} y={H - 4} text-anchor="end">{rows[0]?.time ?? ""}</text>
        </svg>
      {/if}
      <div class="cs-lbl">{$t("Posts")}</div>
      {#if !rows.length}<div class="cs-empty">{$t("No posts yet.")}</div>{/if}
      {#each rows as r (r.key)}
        <button class="cs-post" onclick={() => (sel = r.key)}>
          <span class="cs-pt">{r.text || "…"}</span>
          <span class="cs-pm">{$tn(r.views, "view")} · {$tn(r.reactions, "reaction")} · {$tn(r.comments, "comment")} · {r.time}</span>
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
        {@const cum = cur.st.h.reduce((a: number[], n: number) => (a.push((a[a.length - 1] ?? 0) + n), a), [])}
        {@const c = chart(cum)}
        <svg class="cs-svg" viewBox="0 0 {W} {H}" role="img" aria-label={$t("Views per hour")}>
          {#each c.ticks as tk (tk.v)}<line class="g" x1={PL} x2={W - PR} y1={tk.y} y2={tk.y} /><text class="ax" x={PL - 5} y={tk.y + 3} text-anchor="end">{tk.v}</text>{/each}
          {#each cur.st.h as n, i (i)}<rect class="br" x={c.x(i) - 1.5} width="3" y={H - PB - (n / peak) * (H - PT - PB) * 0.35} height={(n / peak) * (H - PT - PB) * 0.35}><title>{tr("Hour {h}: {n}", { h: i + 1, n })}</title></rect>{/each}
          <path class="ar" d={c.area} /><path class="ln" d={c.line} />
          <text class="ax" x={PL} y={H - 4}>1 {$t("h")}</text>
          <text class="ax" x={W - PR} y={H - 4} text-anchor="end">{cur.st.h.length} {$t("h")}</text>
        </svg>
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
  /* Phones: a screen of its own (Telegram's statistics), not a strip. */
  .cs.phone { position: fixed; inset: 0; width: auto; z-index: 300; border: none; box-shadow: none;
    padding: var(--sat, env(safe-area-inset-top, 0px)) 0 var(--sab, env(safe-area-inset-bottom, 0px)); animation: cs-in-ph 0.22s cubic-bezier(.2,.9,.3,1); }
  @keyframes cs-in { from { transform: translateX(24px); opacity: 0; } }
  @keyframes cs-in-ph { from { transform: translateX(40%); opacity: 0; } }
  .cs-head { display: flex; align-items: center; gap: 6px; min-height: 56px; padding: 0 8px 0 6px; border-bottom: 1px solid var(--border); background: var(--bg-list); }
  .cs-head b { flex: 1; font-size: 17px; font-weight: 600; padding-left: 8px; }
  .cs-x { width: 44px; height: 44px; flex-shrink: 0; display: grid; place-items: center; border-radius: 50%; border: none; background: transparent; color: var(--fg); cursor: pointer; }
  .cs-x :global(svg) { width: 22px; height: 22px; }
  .cs-x:hover { background: color-mix(in srgb, var(--fg) 8%, transparent); }
  .cs-body { flex: 1; overflow-y: auto; padding: 12px; display: flex; flex-direction: column; gap: 10px; }
  /* Telegram's overview: one card, numbers in two columns. */
  .cs-tiles { display: grid; grid-template-columns: repeat(2, 1fr); gap: 14px 12px; padding: 14px 16px; border-radius: calc(16px * var(--rk, 1)); background: var(--bg-surface); border: 1px solid var(--border); }
  .cs-tiles.three { grid-template-columns: repeat(3, 1fr); }
  .cs-tile { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .cs-tile b { font-size: 21px; font-weight: 600; color: var(--fg); font-variant-numeric: tabular-nums; }
  .cs-tile span { font-size: 13px; color: var(--fg-muted); }
  .cs-lbl { margin: 10px 4px 0; font-size: 14px; font-weight: 600; color: var(--accent); }
  .cs-empty { font-size: 14px; color: var(--fg-dim); padding: 0 4px; }
  .cs-post { display: flex; flex-direction: column; gap: 5px; padding: 12px 14px; border-radius: calc(14px * var(--rk, 1)); border: 1px solid var(--border); background: var(--bg-surface); color: var(--fg); font: inherit; text-align: left; cursor: pointer; }
  .cs-post:hover { border-color: color-mix(in srgb, var(--accent) 50%, var(--border)); }
  .cs-pt { font-size: 15px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .cs-pm { font-size: 12.5px; color: var(--fg-dim); font-variant-numeric: tabular-nums; }
  .cs-bar { height: 4px; border-radius: 2px; background: color-mix(in srgb, var(--fg) 8%, transparent); overflow: hidden; }
  .cs-bar i { display: block; height: 100%; background: var(--accent); border-radius: 2px; }
  .cs-note { margin: 8px 4px; font-size: 12.5px; color: var(--fg-dim); line-height: 1.5; }
  .cs-root { padding: 12px 14px; border-radius: calc(14px * var(--rk, 1)); background: var(--bg-surface); border: 1px solid var(--border); border-left: 3px solid var(--accent); font-size: 15px; line-height: 1.45; max-height: 140px; overflow: hidden; }
  .cs-svg { width: 100%; height: auto; display: block; padding: 8px 0; border-radius: calc(16px * var(--rk, 1)); background: var(--bg-surface); border: 1px solid var(--border); box-sizing: border-box; }
  .cs-svg .g { stroke: var(--border); stroke-width: 1; }
  .cs-svg .ax { fill: var(--fg-dim); font-size: 9px; font-variant-numeric: tabular-nums; }
  .cs-svg .ln { fill: none; stroke: var(--accent); stroke-width: 2; stroke-linejoin: round; stroke-linecap: round; }
  .cs-svg .ar { fill: var(--accent); opacity: 0.12; stroke: none; }
  .cs-svg .br { fill: var(--accent); opacity: 0.35; }
  .cs-viewer { display: flex; align-items: center; gap: 12px; padding: 8px 4px; border-bottom: 1px solid var(--border); }
  .cs-av { width: 40px; height: 40px; border-radius: 50%; flex-shrink: 0; display: grid; place-items: center; font-size: 16px; font-weight: 700; color: var(--bg-shell); background: linear-gradient(135deg, var(--accent), var(--accent-2)); background-size: cover; background-position: center; }
  .cs-vn { flex: 1; font-size: 15px; }
  .cs-vt { font-size: 13px; color: var(--fg-dim); }
</style>
