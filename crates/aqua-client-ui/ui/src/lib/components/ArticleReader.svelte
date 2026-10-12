<script lang="ts">
  /**
   * Full-screen reader ("Instant View") for Aquamarine articles and
   * Telegraph pages (telegra.ph links open here instead of the browser).
   */
  import { onMount } from "svelte";
  import { reader, closeReader, openLink } from "../reader";
  import { api, assetUrl, type TelegraphPage } from "../tauri";
  import { parseBlocks, telegraphToBlocks, wordCount, type Block } from "../markup";
  import { t } from "../i18n";
  import Markup from "./Markup.svelte";
  import { gallery } from "./Lightbox.svelte";
  import { get as getStore } from "svelte/store";

  let blocks = $state<Block[]>([]);
  let images = $state<string[]>([]);
  let title = $state("");
  let subtitle = $state("");
  let cover = $state("");
  let author = $state("");
  let authorUrl = $state("");
  let when = $state("");
  let views = $state<number | null>(null);
  let extUrl = $state("");
  let loading = $state(false);
  let error = $state("");
  let progress = $state(0);
  let scale = $state(loadScale());
  let scroller: HTMLDivElement | undefined = $state();

  function loadScale(): number {
    try { const v = parseFloat(localStorage.getItem("aqua.reader.scale") ?? ""); if (v >= 0.8 && v <= 1.5) return v; } catch { /* ignore */ }
    return 1;
  }
  function setScale(d: number) {
    scale = Math.max(0.8, Math.min(1.5, Math.round((scale + d) * 10) / 10));
    try { localStorage.setItem("aqua.reader.scale", String(scale)); } catch { /* ignore */ }
  }

  let token = 0;
  $effect(() => {
    const st = $reader;
    const my = ++token;
    blocks = []; images = []; error = ""; progress = 0; views = null;
    title = subtitle = cover = author = authorUrl = when = extUrl = "";
    if (scroller) scroller.scrollTop = 0;
    if (!st) return;
    if (st.kind === "article") {
      const a = st.article;
      title = a.title; subtitle = a.subtitle ?? ""; cover = a.cover_data_url ?? "";
      author = st.author ?? ""; when = st.time ?? "";
      images = a.images ?? [];
      blocks = parseBlocks(a.body, "article");
      return;
    }
    extUrl = st.url;
    loading = true;
    api.telegraphPage(st.path).then((page: TelegraphPage) => {
      if (my !== token) return;
      title = page.title;
      subtitle = "";
      author = page.author_name ?? "";
      authorUrl = page.author_url ?? "";
      views = page.views ?? null;
      const urls: string[] = [];
      blocks = telegraphToBlocks(page.content ?? [], urls);
      images = urls.map(() => "");
      void resolveImages(urls, my);
    }).catch((e) => {
      if (my === token) error = String(e);
    }).finally(() => { if (my === token) loading = false; });
  });

  /** Remote pictures are cached by the Rust side, then shown from disk. */
  async function resolveImages(urls: string[], my: number) {
    let next = 0;
    const worker = async () => {
      while (next < urls.length) {
        const i = next++;
        try {
          const m = await api.cacheRemoteMedia(urls[i], 25);
          if (my !== token) return;
          images[i] = assetUrl(m.path);
        } catch { /* leave the placeholder */ }
      }
    };
    await Promise.all([worker(), worker(), worker()]);
  }

  let words = $derived(wordCount(blocks));
  let readMin = $derived(Math.max(1, Math.round(words / 200)));
  let photos = $derived(blocks.reduce((n, b) => n + (b.t === "gallery" ? b.idx.length : 0), 0));

  function onScroll() {
    if (!scroller) return;
    const max = scroller.scrollHeight - scroller.clientHeight;
    progress = max > 0 ? Math.min(1, scroller.scrollTop / max) : 1;
  }

  onMount(() => {
    const h = (e: KeyboardEvent) => {
      if (!$reader || getStore(gallery)) return;
      if (e.key === "Escape") { e.preventDefault(); e.stopImmediatePropagation(); closeReader(); }
      else if ((e.ctrlKey || e.metaKey) && (e.key === "=" || e.key === "+")) { e.preventDefault(); setScale(0.1); }
      else if ((e.ctrlKey || e.metaKey) && e.key === "-") { e.preventDefault(); setScale(-0.1); }
    };
    window.addEventListener("keydown", h, true);
    return () => window.removeEventListener("keydown", h, true);
  });
</script>

{#if $reader}
  <div class="reader" role="dialog" aria-modal="true" aria-label={title || $t("Article")}>
    <div class="bar">
      <button class="icon" onclick={closeReader} title={$t("Close (Esc)")} aria-label={$t("Close")}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 18 9 12 15 6"/></svg>
      </button>
      <div class="bar-title">
        <span class="iv-tag">⚡ {$reader.kind === "telegraph" ? "Telegraph" : $t("Article")}</span>
        <span class="bar-name">{title}</span>
      </div>
      <button class="icon txt" onclick={() => setScale(-0.1)} title="Ctrl −">A−</button>
      <button class="icon txt" onclick={() => setScale(0.1)} title="Ctrl +">A+</button>
      {#if extUrl}
        <button class="icon" onclick={() => api.openExternal(extUrl).catch(() => {})} title={$t("Open in browser")} aria-label={$t("Open in browser")}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/><polyline points="15 3 21 3 21 9"/><line x1="10" y1="14" x2="21" y2="3"/></svg>
        </button>
      {/if}
      <div class="progress" style={`transform: scaleX(${progress});`}></div>
    </div>

    <div class="scroll" bind:this={scroller} onscroll={onScroll}>
      {#if loading}
        <div class="state"><span class="spin"></span>{$t("Loading…")}</div>
      {:else if error}
        <div class="state err">
          <p>{$t("Could not open this page")}: {error}</p>
          {#if extUrl}<button class="btn" onclick={() => api.openExternal(extUrl).catch(() => {})}>{$t("Open in browser")}</button>{/if}
        </div>
      {:else}
        {#if cover}<div class="cover" style={`background-image: url(${cover});`}></div>{/if}
        <article class="page" style={`font-size: ${17 * scale}px;`}>
          <h1 class="title">{title}</h1>
          {#if subtitle}<p class="sub">{subtitle}</p>{/if}
          <div class="meta">
            {#if author}
              {#if authorUrl}<button class="author" onclick={() => openLink(authorUrl)}>{author}</button>{:else}<span class="author-plain">{author}</span>{/if}
              <span class="sep">·</span>
            {/if}
            {#if when}<span>{when}</span><span class="sep">·</span>{/if}
            <span>{$t("{n} min read", { n: readMin })}</span>
            {#if photos > 0}<span class="sep">·</span><span>{$t("{n} photos", { n: photos })}</span>{/if}
            {#if views != null}<span class="sep">·</span><span>👁 {views}</span>{/if}
          </div>
          <Markup {blocks} {images} mode="article" />
        </article>
        <div class="end">
          <button class="btn ghost" onclick={closeReader}>{$t("Back to chat")}</button>
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .reader {
    position: fixed; inset: var(--tb-h, 0px) 0 0 0; z-index: 140;
    display: flex; flex-direction: column;
    background: var(--bg-shell);
    animation: rd-in 0.18s ease-out;
  }
  @keyframes rd-in { from { opacity: 0; transform: translateY(10px); } to { opacity: 1; transform: none; } }
  .bar {
    position: relative;
    display: flex; align-items: center; gap: 6px;
    padding: 8px 12px;
    border-bottom: 1px solid var(--border);
    background: var(--bg-list);
  }
  .bar-title { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .iv-tag { font-size: 10.5px; font-weight: 800; letter-spacing: 0.08em; text-transform: uppercase; color: var(--accent); }
  .bar-name { font-weight: 700; font-size: 14px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .icon {
    height: 34px; min-width: 34px; padding: 0 8px;
    display: grid; place-items: center;
    border: 1px solid var(--border); border-radius: 9px;
    background: transparent; color: var(--fg-muted); cursor: pointer;
  }
  .icon:hover { color: var(--fg); border-color: var(--border-strong); }
  .icon svg { width: 17px; height: 17px; }
  .icon.txt { font-weight: 800; font-size: 12.5px; }
  .progress {
    position: absolute; left: 0; right: 0; bottom: -1px; height: 2px;
    background: linear-gradient(90deg, var(--accent), var(--accent-2));
    transform-origin: left; transition: transform 0.08s linear;
  }
  .scroll { flex: 1; overflow-y: auto; }
  .cover { width: 100%; max-width: 980px; margin: 0 auto; aspect-ratio: 16 / 7; background-size: cover; background-position: center; }
  .page { max-width: 700px; margin: 0 auto; padding: 28px 24px 12px; line-height: 1.65; color: var(--fg); }
  .title { font-size: 2em; line-height: 1.15; font-weight: 800; letter-spacing: -0.02em; margin: 0 0 0.3em; }
  .sub { font-size: 1.15em; color: var(--fg-muted); margin: 0 0 0.6em; line-height: 1.4; }
  .meta { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; font-size: 0.8em; color: var(--fg-dim); margin-bottom: 1.6em; }
  .sep { opacity: 0.6; }
  .author { all: unset; cursor: pointer; color: var(--accent); font-weight: 700; }
  .author:hover { text-decoration: underline; }
  .author-plain { font-weight: 700; color: var(--fg-muted); }
  .end { display: flex; justify-content: center; padding: 10px 0 40px; }
  .state { display: grid; place-items: center; gap: 12px; padding: 80px 20px; color: var(--fg-dim); text-align: center; }
  .state.err { color: var(--danger); }
  .btn {
    padding: 9px 18px; border-radius: 10px; border: none; cursor: pointer; font-weight: 700;
    background: linear-gradient(135deg, var(--accent), var(--accent-2)); color: #06070c;
  }
  .btn.ghost { background: transparent; border: 1px solid var(--border); color: var(--fg-muted); }
  .spin {
    width: 30px; height: 30px; border-radius: 50%;
    border: 3px solid var(--border-strong); border-top-color: var(--accent);
    animation: rd-spin 0.8s linear infinite;
  }
  @keyframes rd-spin { to { transform: rotate(360deg); } }
</style>
