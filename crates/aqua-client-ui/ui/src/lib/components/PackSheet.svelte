<script module lang="ts">
  import { writable } from "svelte/store";
  /** Tap a sticker in a chat → its whole pack (Telegram-style sheet). */
  export const packSheet = writable<{ ref: string; from?: string; path?: string | null } | null>(null);
  export function openPackSheet(ref: string, from?: string, path?: string | null) {
    packSheet.set({ ref, from, path });
  }
  // Hold a sticker: it shows big until the finger lets go (or a tap).
  type BigItem = { src: Promise<string> | string; mime?: string | null; emoji?: string };
  let big = $state<BigItem | null>(null);
  let held = false;
  let holdT: ReturnType<typeof setTimeout> | null = null;
  function holdStart(e: PointerEvent, it: BigItem) {
    if (e.button !== 0 && e.pointerType === "mouse") return;
    holdT = setTimeout(() => { held = true; big = it; navigator.vibrate?.(12); }, 380);
  }
  function holdEnd() {
    if (holdT) { clearTimeout(holdT); holdT = null; }
    if (big && held) setTimeout(() => (big = null), 0);
  }
</script>

<script lang="ts">
  import { t, tr } from "../i18n";
  import { api, assetUrl } from "../tauri";
  import { activeId, loadActiveMessages } from "../stores";
  import { showToast } from "./Toast.svelte";
  import { stickerLibraryTick } from "../panels/stickerEditor";
  import { openPackManager } from "./PackManager.svelte";
  import { openSettings } from "../panels/settings";
  import AnimatedMedia from "./AnimatedMedia.svelte";

  interface Item { key: string; src: Promise<string> | string; mime: string; emoji: string; path?: string }
  type Preview = Awaited<ReturnType<typeof api.packPreview>>;

  let preview = $state<Preview | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);
  let items = $state<Item[]>([]);

  $effect(() => {
    const s = $packSheet;
    preview = null; error = null; items = [];
    if (!s) return;
    void load(s.ref);
  });

  async function load(ref: string) {
    try {
      const p = await api.packPreview(ref);
      if ($packSheet?.ref !== ref) return;
      preview = p;
      if (p.local) {
        items = p.local.items.map((i) => ({ key: i.path, src: assetUrl(i.path), mime: i.mime, emoji: i.emoji ?? "", path: i.path }));
      } else if (p.server) {
        const name = p.server.name as string;
        items = (p.server.files as { file: string; emoji: string; mime: string }[]).map((f) => ({
          key: f.file, mime: f.mime, emoji: f.emoji,
          src: api.packFilePath(name, f.file).then((path) => assetUrl(path)),
        }));
      }
    } catch (e) {
      error = String(e);
    }
  }

  function close() { packSheet.set(null); }

  async function add() {
    if (!preview?.server) return;
    busy = true;
    try {
      const r = await api.importPackViaServer(preview.server.name as string);
      stickerLibraryTick.update((n) => n + 1);
      showToast(tr("Pack “{t}” added: {n} stickers", { t: r.pack_title || preview.title, n: r.imported || items.length }));
      await load(preview.ref);
    } catch (e) { showToast(String(e)); }
    finally { busy = false; }
  }

  async function send(it: Item) {
    const peer = $activeId;
    if (!it.path) { showToast(tr("Add the pack first")); return; }
    if (!peer) return;
    try {
      await api.sendSticker(peer, it.path);
      await loadActiveMessages();
      close();
    } catch (e) { showToast(String(e)); }
  }

  async function saveThis() {
    const s = $packSheet;
    if (!s?.path) return;
    try {
      await api.addMediaFiles("stickers", null, [s.path]);
      stickerLibraryTick.update((n) => n + 1);
      showToast(tr("Sticker added to “My stickers”"));
      close();
    } catch (e) { showToast(String(e)); }
  }
</script>

<svelte:window onkeydown={(e) => { if ($packSheet && e.key === "Escape") close(); }} />

{#if $packSheet}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="ps-veil" onclick={(e) => { if (e.target === e.currentTarget) close(); }}>
    <div class="ps" role="dialog" aria-modal="true" aria-label={$t("Sticker pack")}>
      <header class="ps-head">
        <div class="ps-title">
          <b>{preview?.title ?? "…"}</b>
          {#if preview}
            <span class="ps-sub">
              {preview.kind === "emoji" ? $t("Emoji pack") : $t("Sticker pack")}
              {#if items.length} · {items.length}{/if}
              {#if preview.ref.startsWith("tg:")} · Telegram{/if}
            </span>
          {/if}
        </div>
        <button data-x class="ps-x" onclick={close} aria-label={$t("Close")}>✕</button>
      </header>

      {#if error}
        <div class="ps-note">{error}</div>
      {:else if !preview}
        <div class="ps-grid">
          {#each Array(12) as _, i (i)}<div class="ps-cell skel"></div>{/each}
        </div>
      {:else if !items.length}
        <div class="ps-note">
          {$packSheet.from ? tr("This sticker is from @{u}'s own pack. Only the stickers they send can be seen.", { u: $packSheet.from }) : $t("This pack isn't available.")}
        </div>
      {:else}
        <div class="ps-grid">
          {#each items as it (it.key)}
            <button class="ps-cell" class:disabled={!it.path} title={it.emoji}
              onclick={() => { if (held) { held = false; return; } send(it); }}
              onpointerdown={(e) => holdStart(e, it)} onpointerup={holdEnd} onpointerleave={holdEnd} onpointercancel={holdEnd}
              oncontextmenu={(e) => e.preventDefault()}>
              {#await it.src}
                <span class="skel"></span>
              {:then src}
                <AnimatedMedia lazy {src} mime={it.mime} alt={it.emoji} play="hover" />
              {:catch}
                <span class="ps-miss">{it.emoji || "?"}</span>
              {/await}
            </button>
          {/each}
        </div>
      {/if}

      {#if big}
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
        <div class="ps-big" onclick={() => (big = null)}>
          {#await big.src then src}<div class="ps-big-media"><AnimatedMedia {src} mime={big.mime} alt={big.emoji} play="auto" /></div>{/await}
          {#if big.emoji}<span class="ps-big-emoji">{big.emoji}</span>{/if}
        </div>
      {/if}

      <footer class="ps-foot">
        {#if preview?.installed}
          <span class="ps-ok">✓ {$t("In your stickers")}</span>
          <button class="ps-btn ghost" onclick={() => { const tl = preview?.local?.title; close(); if (tl && !preview?.ref.startsWith("tg:")) openPackManager(tl); else openSettings("stickers"); }}>{$t("Manage packs")}</button>
        {:else if preview?.server}
          <button class="ps-btn primary" disabled={busy} onclick={add}>{busy ? "…" : tr("Add {n} stickers", { n: items.length })}</button>
        {:else if preview && $packSheet.path}
          <button class="ps-btn primary" onclick={saveThis}>{$t("Add this sticker")}</button>
        {/if}
      </footer>
    </div>
  </div>
{/if}

<style>
  .ps-veil { position: fixed; inset: 0; z-index: 900; display: grid; place-items: center; background: rgba(4, 6, 12, 0.5); animation: ps-fade 0.14s ease-out; }
  .ps {
    width: min(560px, calc(100vw - 24px)); max-height: min(640px, calc(100dvh - 24px));
    display: flex; flex-direction: column; background: var(--bg-shell); border: 1px solid var(--border-strong);
    border-radius: calc(18px * var(--rk, 1)); overflow: hidden; position: relative; animation: ps-pop 0.18s cubic-bezier(.2,.9,.3,1.2);
  }
  .ps-head { display: flex; align-items: center; gap: 10px; padding: 14px 16px 10px; border-bottom: 1px solid var(--border); }
  .ps-title { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 2px; }
  .ps-title b { font-size: 16px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .ps-sub { font-size: 12px; color: var(--fg-dim); }
  .ps-x { width: 30px; height: 30px; border-radius: 50%; border: none; background: var(--bg-hover); color: var(--fg-muted); cursor: pointer; }
  .ps-x:hover { color: var(--fg); }
  /* Five in a row, like Telegram; hold one to see it big. */
  .ps-grid { display: grid; grid-template-columns: repeat(5, 1fr); gap: 4px; padding: 10px; overflow-y: auto; }
  .ps-big { position: absolute; inset: 0; z-index: 5; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 10px;
    background: color-mix(in srgb, var(--bg-shell) 70%, transparent); backdrop-filter: blur(10px); animation: ps-big-in .18s cubic-bezier(.2,.9,.3,1.2); }
  .ps-big-media { width: min(72%, 320px); aspect-ratio: 1; }
  .ps-big-emoji { font-size: 30px; }
  @keyframes ps-big-in { from { opacity: 0; transform: scale(.9); } }
  .ps-cell { aspect-ratio: 1; border: none; background: transparent; border-radius: calc(12px * var(--rk, 1)); padding: 6px; cursor: pointer; display: grid; place-items: center; transition: background 0.12s, transform 0.12s; }
  .ps-cell:hover { background: var(--bg-hover); transform: scale(1.04); }
  .ps-cell.disabled { cursor: default; }
  .ps-cell :global(img), .ps-cell :global(video), .ps-cell :global(canvas) { max-width: 100%; max-height: 100%; object-fit: contain; }
  .skel { width: 100%; height: 100%; border-radius: calc(12px * var(--rk, 1)); background: linear-gradient(90deg, var(--bg-hover), color-mix(in srgb, var(--bg-hover) 40%, transparent), var(--bg-hover)); background-size: 200% 100%; animation: ps-shimmer 1.2s linear infinite; }
  .ps-cell.skel { padding: 0; }
  .ps-miss { font-size: 28px; }
  .ps-note { padding: 22px 18px; color: var(--fg-muted); font-size: 14px; line-height: 1.5; }
  .ps-foot { display: flex; align-items: center; justify-content: flex-end; gap: 10px; padding: 12px 16px; border-top: 1px solid var(--border); min-height: 24px; }
  .ps-ok { margin-right: auto; color: var(--accent); font-size: 13px; font-weight: 600; }
  .ps-btn { border-radius: calc(10px * var(--rk, 1)); padding: 9px 16px; font-weight: 600; font-size: 14px; cursor: pointer; border: 1px solid var(--border-strong); background: transparent; color: var(--fg); }
  .ps-btn.primary { flex: 1; border: none; color: #061015; background: linear-gradient(135deg, var(--accent), var(--accent-2)); }
  .ps-btn:disabled { opacity: 0.6; cursor: default; }
  @keyframes ps-fade { from { opacity: 0; } }
  @keyframes ps-pop { from { opacity: 0; transform: translateY(12px) scale(0.97); } }
  @keyframes ps-shimmer { to { background-position: -200% 0; } }
  :global(html.wk-lowfx) .skel { animation: none; }
</style>
