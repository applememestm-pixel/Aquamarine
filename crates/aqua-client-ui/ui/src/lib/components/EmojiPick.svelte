<script lang="ts">
  /**
   * Pick any emoji — every Unicode one (with search), or anything from the
   * emoji and sticker packs on this device, like Telegram's status picker.
   * `plain` = Unicode only (the banner pattern is drawn from text).
   */
  import { onMount, tick } from "svelte";
  import AnimatedMedia from "./AnimatedMedia.svelte";
  import { t } from "../i18n";
  import { reactionPack, emojiPacks, loadEmojiPacks, loadReactionPack, REACTION_PACK } from "../reactionPack";
  import type { EmojiCat } from "../emojiData";

  let { value = "", plain = false, onpick, onclose }: { value?: string; plain?: boolean; onpick: (token: string) => void; onclose: () => void } = $props();

  let cats = $state<EmojiCat[] | null>(null);
  let q = $state("");
  let tab = $state("emoji");
  let box = $state<HTMLDivElement | null>(null);
  let input = $state<HTMLInputElement | null>(null);

  /** Above everything (a scrolling column would clip it), next to whatever opened it. */
  function place() {
    const host = anchor ?? box?.parentElement;
    if (!box || !host) return;
    const r = host.getBoundingClientRect();
    const w = Math.min(340, window.innerWidth - 16), h = box.offsetHeight || 380;
    let left = Math.min(Math.max(8, r.left), window.innerWidth - w - 8);
    let top = r.bottom + 6;
    if (top + h > window.innerHeight - 8) top = Math.max(8, r.top - h - 6);
    box.style.left = left + "px"; box.style.top = top + "px"; box.style.width = w + "px";
  }
  let anchor: HTMLElement | null = null;
  onMount(() => {
    anchor = box?.parentElement ?? null;
    if (box) document.body.appendChild(box);
    place();
    requestAnimationFrame(place);
    window.addEventListener("resize", place);
    void import("../emojiData").then((m) => (cats = m.EMOJI_CATS));
    if (!plain) { void loadEmojiPacks(); if ($reactionPack.source === "none") void loadReactionPack(); }
    void tick().then(() => input?.focus());
    const down = (e: PointerEvent) => { if (box && !box.contains(e.target as Node)) onclose(); };
    const key = (e: KeyboardEvent) => { if (e.key === "Escape") { e.stopPropagation(); onclose(); } };
    // Next frame: the click that opened the picker mustn't close it.
    const id = requestAnimationFrame(() => document.addEventListener("pointerdown", down, true));
    document.addEventListener("keydown", key, true);
    return () => { window.removeEventListener("resize", place); box?.remove(); cancelAnimationFrame(id); document.removeEventListener("pointerdown", down, true); document.removeEventListener("keydown", key, true); };
  });

  type PackView = { id: string; title: string; items: { token: string; url: string; mime: string; emoji: string }[] };
  const packs = $derived.by((): PackView[] => {
    if (plain) return [];
    const out: PackView[] = [];
    if ($reactionPack.items.length) out.push({ id: REACTION_PACK, title: "El Taller", items: $reactionPack.items });
    for (const p of $emojiPacks) out.push({ id: p.name, title: p.title, items: p.items });
    return out;
  });
  const needle = $derived(q.trim().toLowerCase());
  const unicodeHits = $derived.by(() => {
    if (!cats || !needle) return [];
    const out: string[] = [];
    for (const [, , items] of cats) for (const [e, words] of items) if (words.includes(needle) && out.length < 160) out.push(e);
    return out;
  });
  const packHits = $derived.by(() => {
    if (!needle) return [];
    const out: PackView["items"] = [];
    for (const p of packs) for (const it of p.items) if (it.emoji && (it.emoji.includes(needle) || p.title.toLowerCase().includes(needle)) && out.length < 120) out.push(it);
    return out;
  });
  const curPack = $derived(packs.find((p) => p.id === tab) ?? null);
  function pick(tok: string) { onpick(tok); onclose(); }
</script>

<div class="ep" bind:this={box} role="dialog" aria-label={$t("Choose an emoji")}>
  <input class="ep-q" bind:this={input} bind:value={q} placeholder={$t("Search emoji")} />
  {#if !needle && packs.length}
    <div class="ep-tabs">
      <button class:on={tab === "emoji"} onclick={() => (tab = "emoji")} title={$t("Emoji")}>😀</button>
      {#each packs as p (p.id)}
        {@const first = p.items[0]}
        <button class:on={tab === p.id} onclick={() => (tab = p.id)} title={p.title}>
          {#if first}<AnimatedMedia emoji src={first.url} mime={first.mime} alt={p.title} play="hover" />{:else}{p.title.slice(0, 2)}{/if}
        </button>
      {/each}
    </div>
  {/if}
  <div class="ep-grid">
    {#if needle}
      {#each unicodeHits as e (e)}<button class="u" class:on={value === e} onclick={() => pick(e)}>{e}</button>{/each}
      {#each packHits as it (it.token)}<button class:on={value === it.token} onclick={() => pick(it.token)} title={it.emoji}><AnimatedMedia emoji src={it.url} mime={it.mime} alt={it.emoji} play="hover" /></button>{/each}
      {#if !unicodeHits.length && !packHits.length}<div class="ep-empty">{$t("Nothing found")}</div>{/if}
    {:else if curPack}
      <div class="ep-title">{curPack.title}</div>
      {#each curPack.items as it (it.token)}<button class:on={value === it.token} onclick={() => pick(it.token)} title={it.emoji}><AnimatedMedia emoji src={it.url} mime={it.mime} alt={it.emoji} play="hover" lazy /></button>{/each}
    {:else if cats}
      {#each cats as [id, title, items] (id)}
        <div class="ep-title">{$t(title)}</div>
        {#each items as [e] (e)}<button class="u" class:on={value === e} onclick={() => pick(e)}>{e}</button>{/each}
      {/each}
    {:else}
      <div class="ep-empty">…</div>
    {/if}
  </div>
</div>

<style>
  .ep { position: fixed; z-index: 300; top: 0; left: 0; width: min(340px, 86vw); padding: 10px;
    background: var(--bg-shell); color: var(--fg); border: 1px solid var(--border-strong); border-radius: calc(14px * var(--rk, 1));
    box-shadow: 0 22px 50px -14px rgba(0, 0, 0, .75); display: flex; flex-direction: column; gap: 8px; animation: ep-in .18s cubic-bezier(.2,.9,.25,1.1); }
  @keyframes ep-in { from { opacity: 0; transform: translateY(-4px) scale(.98); } }
  .ep-q { background: var(--bg-input); border: 1px solid var(--border); border-radius: calc(9px * var(--rk, 1)); color: var(--fg); font: inherit; font-size: 13px; padding: 8px 10px; outline: none; }
  .ep-q:focus { border-color: var(--accent); }
  .ep-tabs { display: flex; gap: 4px; overflow-x: auto; padding-bottom: 2px; scrollbar-width: thin; }
  .ep-tabs button { flex: 0 0 34px; height: 34px; padding: 4px; border: 1px solid transparent; border-radius: calc(9px * var(--rk, 1)); background: var(--bg-input); cursor: pointer; font-size: 18px; color: var(--fg); display: grid; place-items: center; }
  .ep-tabs button.on { border-color: var(--accent); background: var(--accent-soft); }
  .ep-tabs :global(img), .ep-tabs :global(video), .ep-tabs :global(canvas) { width: 24px; height: 24px; object-fit: contain; }
  .ep-grid { display: grid; grid-template-columns: repeat(8, 1fr); gap: 2px; max-height: 280px; overflow-y: auto; align-content: start; scrollbar-width: thin; }
  .ep-grid button { aspect-ratio: 1; border: 1px solid transparent; background: none; border-radius: calc(8px * var(--rk, 1)); cursor: pointer; padding: 3px; display: grid; place-items: center; color: var(--fg); }
  .ep-grid button.u { font-size: 22px; line-height: 1; font-family: "Noto Color Emoji", "Apple Color Emoji", "Segoe UI Emoji", sans-serif; }
  .ep-grid button:hover { background: var(--bg-hover, rgba(255,255,255,.06)); }
  .ep-grid button.on { border-color: var(--accent); background: var(--accent-soft); }
  .ep-grid :global(img), .ep-grid :global(video), .ep-grid :global(canvas) { width: 100%; height: 100%; object-fit: contain; }
  .ep-title { grid-column: 1 / -1; font-size: 11px; font-weight: 700; letter-spacing: .06em; text-transform: uppercase; color: var(--fg-dim); padding: 6px 2px 2px; }
  .ep-empty { grid-column: 1 / -1; text-align: center; color: var(--fg-dim); font-size: 12.5px; padding: 18px 0; }
</style>
