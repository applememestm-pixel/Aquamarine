<script lang="ts">
  /**
   * Renders formatted text (lib/markup.ts) — chat messages, articles and
   * Telegraph pages. Plain Svelte elements only (no {@html}).
   */
  import { parseBlocks, type Block, type Inline, type Mode } from "../markup";
  import { openLink } from "../reader";
  import { openProfile } from "./ProfileModal.svelte";
  import { openGallery } from "./Lightbox.svelte";
  import { showToast } from "./Toast.svelte";
  import { tr } from "../i18n";
  import AnimatedMedia from "./AnimatedMedia.svelte";
  import Carousel from "./Carousel.svelte";
  import LinkPreview from "./LinkPreview.svelte";

  interface Props {
    /** Raw text, or pre-parsed blocks. */
    src?: string;
    blocks?: Block[];
    mode?: Mode;
    /** Display URLs for gallery blocks (indexes from the article). */
    images?: string[];
    /** Custom emoji lookup ("pack/file" → media). */
    resolveEmoji?: (key: string) => { url: string; mime: string } | undefined;
    /** Small bubbles: tighter spacing, no thumbnails under carousels. */
    compact?: boolean;
  }
  let { src = "", blocks, mode = "message", images = [], resolveEmoji, compact = false }: Props = $props();

  let tree = $derived(blocks ?? parseBlocks(src, mode));

  function emojiFallback(key: string): string {
    const f = key.split("/").pop() ?? "";
    const i = f.indexOf("_");
    return i > 0 ? f.slice(0, i) : "▫";
  }
  function click(e: MouseEvent, href: string) {
    e.preventDefault();
    e.stopPropagation();
    void openLink(href);
  }
  function reveal(e: Event) {
    e.stopPropagation();
    (e.currentTarget as HTMLElement).classList.add("shown");
  }
  async function copyPre(text: string) {
    try { await navigator.clipboard.writeText(text); showToast(tr("Copied")); } catch { /* ignore */ }
  }
  function openPhotos(idx: number[], i: number) {
    const items = idx.map((k) => images[k]).filter(Boolean).map((url) => ({ url, kind: "image" as const }));
    if (items.length) openGallery(items, Math.min(i, items.length - 1));
  }
</script>

{#snippet inl(nodes: Inline[])}
  {#each nodes as n, k (k)}
    {#if n.t === "text"}{n.v}{:else if n.t === "br"}<br />{:else if n.t === "emoji"}
      {@const m = resolveEmoji?.(n.key)}
      {#if m}<span class="mk-emoji" title={emojiFallback(n.key)}><AnimatedMedia src={m.url} mime={m.mime} alt={emojiFallback(n.key)} play="auto" /></span>{:else}{emojiFallback(n.key)}{/if}
    {:else if n.t === "code"}<code class="mk-code">{n.v}</code>{:else if n.t === "b"}<strong>{@render inl(n.c)}</strong>{:else if n.t === "i"}<em>{@render inl(n.c)}</em>{:else if n.t === "u"}<u>{@render inl(n.c)}</u>{:else if n.t === "s"}<s>{@render inl(n.c)}</s>{:else if n.t === "spoiler"}<!-- svelte-ignore a11y_click_events_have_key_events --><span class="mk-spoiler" role="button" tabindex="0" title={tr("Spoiler — click to show")} onclick={reveal}>{@render inl(n.c)}</span>{:else if n.t === "link"}<a class="mk-link" href={n.href} title={n.href} onclick={(e) => click(e, n.href)}>{@render inl(n.c)}</a>{:else if n.t === "mention"}<button class="mk-mention" onclick={(e) => { e.stopPropagation(); openProfile(n.v); }}>@{n.v}</button>{:else if n.t === "hashtag"}<span class="mk-tag">#{n.v}</span>{/if}
  {/each}
{/snippet}

{#snippet blocksOf(list: Block[])}
  {#each list as b, k (k)}
    {#if b.t === "p"}
      <p class="mk-p">{@render inl(b.c)}</p>
    {:else if b.t === "h1"}
      <h3 class="mk-h1">{@render inl(b.c)}</h3>
    {:else if b.t === "h2"}
      <h4 class="mk-h2">{@render inl(b.c)}</h4>
    {:else if b.t === "quote"}
      <blockquote class="mk-quote" class:pull={b.pull}>{@render blocksOf(b.c)}</blockquote>
    {:else if b.t === "pre"}
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <pre class="mk-pre" title={tr("Click to copy")} onclick={(e) => { e.stopPropagation(); void copyPre(b.v); }}>{#if b.lang}<span class="mk-lang">{b.lang}</span>{/if}<code>{b.v}</code></pre>
    {:else if b.t === "ul"}
      <ul class="mk-list">{#each b.items as it, j (j)}<li>{@render inl(it)}</li>{/each}</ul>
    {:else if b.t === "ol"}
      <ol class="mk-list" start={b.start ?? 1}>{#each b.items as it, j (j)}<li>{@render inl(it)}</li>{/each}</ol>
    {:else if b.t === "hr"}
      <hr class="mk-hr" />
    {:else if b.t === "gallery"}
      <figure class="mk-fig">
        <Carousel srcs={b.idx.map((i) => images[i] ?? "")} aspect={compact ? 16 / 10 : 4 / 3} thumbs={!compact && b.idx.length > 1} onopen={(i) => openPhotos(b.idx, i)} />
        {#if b.caption}<figcaption>{@render inl(b.caption)}</figcaption>{/if}
      </figure>
    {:else if b.t === "video" || b.t === "embed"}
      <figure class="mk-fig">
        <LinkPreview url={b.url} />
        {#if b.caption}<figcaption>{@render inl(b.caption)}</figcaption>{/if}
      </figure>
    {/if}
  {/each}
{/snippet}

<div class="mk" class:compact class:msg={mode === "message"} class:article={mode === "article"}>{@render blocksOf(tree)}</div>

<style>
  .mk { min-width: 0; overflow-wrap: anywhere; }
  /* Chat bubbles: text flows inline so the time stamp floats after it. */
  .mk.msg { display: inline; }
  .mk.msg .mk-p { display: inline; }
  .mk-p { margin: 0; white-space: pre-wrap; }
  .article .mk-p { margin: 0 0 0.85em; white-space: normal; }
  .mk-h1 { font-size: 1.3em; font-weight: 800; margin: 1.1em 0 0.4em; letter-spacing: -0.01em; line-height: 1.25; }
  .mk-h2 { font-size: 1.1em; font-weight: 700; margin: 1em 0 0.35em; color: var(--fg-muted); }
  .mk > :first-child { margin-top: 0; }
  .mk-quote {
    margin: 4px 0;
    padding: 3px 10px;
    border-left: 3px solid var(--accent);
    border-radius: 4px;
    background: color-mix(in srgb, var(--accent) 9%, transparent);
  }
  .article .mk-quote { margin: 0.9em 0; padding: 8px 14px; }
  .mk-quote > :global(:last-child) { margin-bottom: 0; }
  .mk-quote.pull {
    border-left: none; background: none; text-align: center;
    font-size: 1.15em; font-style: italic; color: var(--fg-muted);
    padding: 0.6em 1.5em;
  }
  .mk-pre {
    position: relative;
    margin: 4px 0;
    padding: 8px 10px;
    background: var(--bg-input);
    border: 1px solid var(--border);
    border-radius: 8px;
    font-family: var(--font-mono);
    font-size: 0.88em;
    line-height: 1.5;
    white-space: pre-wrap;
    overflow-x: auto;
    cursor: copy;
  }
  .article .mk-pre { margin: 0.9em 0; }
  .mk-lang { display: block; font-size: 0.8em; color: var(--accent); font-weight: 700; margin-bottom: 4px; text-transform: lowercase; }
  .mk-code {
    font-family: var(--font-mono);
    font-size: 0.9em;
    padding: 1px 5px;
    border-radius: 4px;
    background: var(--bg-input);
    color: var(--accent);
  }
  .mk-list { margin: 0.2em 0 0.85em; padding-left: 1.4em; }
  .mk-list li { margin-bottom: 0.25em; }
  .mk-hr { border: none; height: 1px; background: var(--border-strong); margin: 1.4em 15%; }
  .mk-fig { margin: 0.8em 0; }
  .compact .mk-fig { margin: 6px 0; }
  .mk-fig figcaption { font-size: 0.86em; color: var(--fg-dim); text-align: center; margin-top: 6px; }
  .mk-link { color: var(--accent); text-decoration: underline; text-decoration-color: color-mix(in srgb, var(--accent) 45%, transparent); cursor: pointer; word-break: break-word; }
  .mk-link:hover { text-decoration-color: var(--accent); }
  .mk-mention { all: unset; color: var(--accent); cursor: pointer; font-weight: 600; }
  .mk-mention:hover { text-decoration: underline; }
  .mk-tag { color: var(--accent); }
  .mk-spoiler {
    border-radius: 4px;
    background: var(--fg-dim);
    color: transparent;
    cursor: pointer;
    transition: background 0.2s, color 0.2s;
    user-select: none;
  }
  .mk-spoiler :global(*) { visibility: hidden; }
  .mk-spoiler:global(.shown) { background: color-mix(in srgb, var(--fg) 8%, transparent); color: inherit; user-select: text; cursor: text; }
  .mk-spoiler:global(.shown) :global(*) { visibility: visible; }
  .mk-emoji { display: inline-block; width: 1.35em; height: 1.35em; vertical-align: -0.3em; margin: 0 1px; }
  .mk-emoji :global(img), .mk-emoji :global(video) { width: 100%; height: 100%; object-fit: contain; }
  strong { font-weight: 700; }
</style>
