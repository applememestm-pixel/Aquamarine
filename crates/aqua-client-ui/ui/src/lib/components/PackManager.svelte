<script module lang="ts">
  import { writable } from "svelte/store";
  /** Title of the pack being managed (null = closed). */
  export const packManager = writable<string | null>(null);
  export function openPackManager(title: string) { packManager.set(title); }
  export function closePackManager() { packManager.set(null); }
</script>

<script lang="ts">
  /**
   * The @Stickers bot, as a window. For packs made here:
   *   add stickers (through the editor, or files as they are — GIF, WEBM,
   *   TGS too), set each sticker's emoji, drag to reorder, pick the pack
   *   icon, rename, delete stickers or the whole pack.
   * Telegram packs never reach this window (they are read-only).
   */
  import { t, tr } from "../i18n";
  import { api, assetUrl } from "../tauri";
  import type { StickerPack, StickerItem } from "../types";
  import { showToast } from "./Toast.svelte";
  import { askConfirm } from "./Confirm.svelte";
  import { openFileDialog } from "../dialog";
  import { openStickerQueue, openStickerEditor, stickerLibraryTick } from "./StickerEditor.svelte";
  import AnimatedMedia from "./AnimatedMedia.svelte";
  import { emojiFromFile } from "../reactionPack";

  const QUICK = ["😀", "😂", "🥰", "😍", "😎", "🤔", "😭", "😡", "👍", "👎", "👏", "🙏", "❤️", "🔥", "✨", "🎉", "💀", "🤡", "😴", "🤯", "🥺", "😱", "🤝", "👋"];
  const LIMIT = { sticker: 120, emoji: 200 };

  let pack = $state<StickerPack | null>(null);
  let items = $state<StickerItem[]>([]);
  let sel = $state<string | null>(null);   // path
  let title = $state("");
  let busy = $state(false);
  let emojiInput = $state("");

  const isEmoji = $derived(pack?.kind === "emoji");
  const selected = $derived(items.find((i) => i.path === sel) ?? null);
  const fileOf = (p: string) => p.split(/[\\/]/).pop() ?? "";
  const emojiOf = (it: StickerItem) => it.emoji ?? emojiFromFile(fileOf(it.path));

  async function load() {
    const want = $packManager;
    if (!want) return;
    try {
      const lib = await api.listLocalMedia();
      const p = lib.packs.find((x) => x.title === want) ?? null;
      if (!p || p.source || p.editable === false) { if (!p) closePackManager(); pack = null; return; }
      pack = p;
      items = p.items;
      if (sel && !items.some((i) => i.path === sel)) sel = null;
      title = p.title;
    } catch (e) { showToast(String(e)); }
  }
  let seen = -1;
  $effect(() => { const n = $stickerLibraryTick; const open = $packManager; if (open && n !== seen) { seen = n; void load(); } });
  $effect(() => { if ($packManager) { seen = -1; sel = null; void load(); } });
  $effect(() => { emojiInput = selected ? emojiOf(selected) : ""; });

  function changed() { stickerLibraryTick.update((n) => n + 1); }

  // ---- adding ----
  const STATIC = /\.(png|jpe?g|webp)$/i;
  async function pickFiles(): Promise<string[]> {
    const r = await openFileDialog({
      multiple: true, directory: false, title: tr("Add to “{pack}”", { pack: pack?.title ?? "" }),
      filters: [{ name: tr("Pictures and animations"), extensions: ["png", "jpg", "jpeg", "webp", "gif", "webm", "tgs"] }],
    });
    return (Array.isArray(r) ? r : r ? [r] : []) as string[];
  }
  function roomLeft(n: number): boolean {
    const max = LIMIT[isEmoji ? "emoji" : "sticker"];
    if (items.length + n > max) { showToast(tr("A pack holds up to {n} items", { n: max })); return false; }
    return true;
  }
  /** Pictures go through the editor one by one; animations are added as they are. */
  async function addWithEditor() {
    if (!pack) return;
    const paths = await pickFiles();
    if (!paths.length || !roomLeft(paths.length)) return;
    const stills = paths.filter((p) => STATIC.test(p));
    const anim = paths.filter((p) => !STATIC.test(p));
    if (anim.length) { await api.packAddFiles(pack.title, anim); changed(); }
    if (stills.length) await openStickerQueue(stills, pack.title);
  }
  async function addAsIs() {
    if (!pack) return;
    const paths = await pickFiles();
    if (!paths.length || !roomLeft(paths.length)) return;
    busy = true;
    try {
      const got = await api.packAddFiles(pack.title, paths);
      showToast(tr("{n} added", { n: got.length }));
      changed();
    } catch (e) { showToast(String(e)); } finally { busy = false; }
  }
  function blank() { if (pack && roomLeft(1)) openStickerEditor([], "", "text", pack.title); }

  // ---- one sticker ----
  async function setEmoji(e: string) {
    if (!selected || !e.trim()) return;
    try {
      const np = await api.stickerSetEmoji(selected.path, e.trim());
      sel = np;
      changed();
    } catch (err) { showToast(String(err)); }
  }
  async function delSticker() {
    if (!selected) return;
    try { await api.stickerDelete(selected.path); sel = null; changed(); } catch (e) { showToast(String(e)); }
  }
  async function makeIcon() {
    if (!selected || !pack) return;
    try { await api.packSetIcon(pack.title, fileOf(selected.path)); showToast(tr("Pack icon set")); changed(); } catch (e) { showToast(String(e)); }
  }
  async function move(d: number) {
    if (!selected || !pack) return;
    const i = items.findIndex((x) => x.path === selected.path), j = i + d;
    if (i < 0 || j < 0 || j >= items.length) return;
    const a = [...items]; [a[i], a[j]] = [a[j], a[i]];
    items = a;
    await saveOrder();
  }
  async function saveOrder() {
    if (!pack) return;
    try { await api.packSetOrder(pack.title, items.map((x) => fileOf(x.path))); changed(); } catch (e) { showToast(String(e)); }
  }

  // ---- drag to reorder ----
  let drag = $state<{ path: string; x: number; y: number; moved: boolean } | null>(null);
  function down(e: PointerEvent, it: StickerItem) {
    if (e.button !== 0) return;
    drag = { path: it.path, x: e.clientX, y: e.clientY, moved: false };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }
  function over(e: PointerEvent) {
    if (!drag) return;
    if (!drag.moved && Math.hypot(e.clientX - drag.x, e.clientY - drag.y) < 6) return;
    drag.moved = true;
    const el = (document.elementFromPoint(e.clientX, e.clientY) as HTMLElement | null)?.closest<HTMLElement>("[data-path]");
    const to = el?.dataset.path;
    if (!to || to === drag.path) return;
    const a = [...items];
    const i = a.findIndex((x) => x.path === drag!.path), j = a.findIndex((x) => x.path === to);
    if (i < 0 || j < 0) return;
    const [m] = a.splice(i, 1);
    a.splice(j, 0, m);
    items = a;
  }
  function up(it: StickerItem) {
    const d = drag;
    drag = null;
    if (!d) return;
    if (d.moved) void saveOrder();
    else sel = sel === it.path ? null : it.path;
  }

  // ---- pack ----
  async function rename() {
    if (!pack) return;
    const n = title.trim();
    if (!n || n === pack.title) { title = pack.title; return; }
    try { await api.renameStickerPack(pack.title, n); packManager.set(n); changed(); showToast(tr("Renamed")); }
    catch (e) { title = pack.title; showToast(String(e)); }
  }
  async function delPack() {
    if (!pack) return;
    if (!(await askConfirm({ title: tr("Delete pack?"), text: tr("“{pack}” and all its stickers are removed from this device.", { pack: pack.title }), ok: tr("Delete"), danger: true }))) return;
    try { await api.deleteStickerPack(pack.title); closePackManager(); changed(); } catch (e) { showToast(String(e)); }
  }

  function onKey(e: KeyboardEvent) {
    if (!$packManager) return;
    if (e.key === "Escape" && !e.defaultPrevented) { e.preventDefault(); if (sel) sel = null; else closePackManager(); }
    else if ((e.key === "Delete") && selected && (e.target as HTMLElement)?.tagName !== "INPUT") void delSticker();
    else if (selected && (e.key === "ArrowLeft" || e.key === "ArrowRight") && (e.target as HTMLElement)?.tagName !== "INPUT") void move(e.key === "ArrowLeft" ? -1 : 1);
  }
</script>

<svelte:window onkeydown={onKey} />

{#if $packManager && pack}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="pm-veil" onclick={(e) => { if (e.target === e.currentTarget) closePackManager(); }}>
    <div class="pm" role="dialog" aria-label={$t("Pack")}>
      <header class="pm-head">
        <div class="pm-ico">{#if pack.icon}<AnimatedMedia src={assetUrl(pack.icon)} mime={items.find((i) => i.path === pack!.icon)?.mime ?? "image/webp"} play="never" />{:else}<span>{isEmoji ? "😀" : "🖼"}</span>{/if}</div>
        <div class="pm-title">
          <input bind:value={title} maxlength="64" onblur={rename} onkeydown={(e) => { if (e.key === "Enter") (e.target as HTMLInputElement).blur(); }} aria-label={$t("Pack name")} />
          <span class="pm-sub">{isEmoji ? $t("Emoji pack") : $t("Sticker pack")} · {items.length}/{LIMIT[isEmoji ? "emoji" : "sticker"]}</span>
        </div>
        <button class="pm-x" onclick={closePackManager} aria-label={$t("Close")}>×</button>
      </header>

      <div class="pm-tools">
        <button class="pm-btn primary" onclick={addWithEditor} disabled={busy}>＋ {$t(isEmoji ? "Add emoji" : "Add stickers")}</button>
        <button class="pm-btn" onclick={addAsIs} disabled={busy} title={$t("Pictures, GIF, WEBM or TGS without the editor")}>{$t("Add files as is")}</button>
        <button class="pm-btn" onclick={blank} title={$t("A sticker from text and drawing")}>Aa {$t("Text")}</button>
        <span class="grow"></span>
        <button class="pm-btn danger" onclick={delPack}>{$t("Delete pack")}</button>
      </div>

      {#if items.length === 0}
        <div class="pm-empty">
          <div class="pm-empty-big">{isEmoji ? "😀" : "🖼"}</div>
          <div>{$t(isEmoji ? "No emoji yet. Add pictures — square ones look best (100×100)." : "No stickers yet. Add pictures: the editor cuts out the background, adds a caption and the emoji.")}</div>
        </div>
      {:else}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div class="pm-grid" class:emoji={isEmoji} onpointermove={over}>
          {#each items as it (it.path)}
            <div class="pm-cell" class:sel={sel === it.path} class:dragging={drag?.moved && drag.path === it.path} class:icon={pack.icon === it.path}
                 data-path={it.path} role="button" tabindex="0"
                 onpointerdown={(e) => down(e, it)} onpointerup={() => up(it)}
                 onkeydown={(e) => { if (e.key === "Enter") sel = it.path; }}>
              <AnimatedMedia src={assetUrl(it.path)} mime={it.mime} alt={emojiOf(it)} play="hover" lazy />
              {#if emojiOf(it)}<span class="pm-em">{emojiOf(it)}</span>{/if}
            </div>
          {/each}
          <button class="pm-cell add" onclick={addWithEditor} aria-label={$t("Add")}>＋</button>
        </div>
      {/if}

      {#if selected}
        <div class="pm-sel">
          <div class="pm-pv"><AnimatedMedia src={assetUrl(selected.path)} mime={selected.mime} alt={emojiOf(selected)} play="auto" /></div>
          <div class="pm-side">
            <div class="pm-lbl">{$t("Emoji for this sticker")}</div>
            <div class="pm-emrow">
              <input class="pm-eminput" bind:value={emojiInput} maxlength="8" onkeydown={(e) => { if (e.key === "Enter") void setEmoji(emojiInput); }} aria-label={$t("Emoji")} />
              <button class="pm-btn" onclick={() => setEmoji(emojiInput)} disabled={!emojiInput.trim() || emojiInput.trim() === emojiOf(selected)}>{$t("Save")}</button>
            </div>
            <div class="pm-quick">
              {#each QUICK as q (q)}<button class:on={emojiOf(selected) === q} onclick={() => setEmoji(q)}>{q}</button>{/each}
            </div>
            <div class="pm-acts">
              <button class="pm-btn" onclick={() => move(-1)} title="←">←</button>
              <button class="pm-btn" onclick={() => move(1)} title="→">→</button>
              <button class="pm-btn" onclick={makeIcon} disabled={pack.icon === selected.path}>{$t("Make pack icon")}</button>
              <span class="grow"></span>
              <button class="pm-btn danger" onclick={delSticker}>{$t("Delete")}</button>
            </div>
          </div>
        </div>
      {:else}
        <div class="pm-hint">{$t("Click a sticker to change its emoji, make it the pack icon or delete it. Drag to reorder.")}</div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .pm-veil { position: fixed; inset: 0; z-index: 265; background: rgba(4, 6, 12, .55); display: grid; place-items: center; animation: pm-fade .16s ease-out; }
  @keyframes pm-fade { from { opacity: 0; } }
  .pm { width: min(620px, calc(100vw - 32px)); max-height: calc(100vh - 48px); display: flex; flex-direction: column; gap: 10px; padding: 14px;
    border-radius: 18px; background: var(--bg-shell); border: 1px solid var(--border-strong); box-shadow: 0 30px 80px -20px rgba(0,0,0,.75);
    animation: pm-in .22s cubic-bezier(.2,.9,.25,1.1); }
  @keyframes pm-in { from { opacity: 0; transform: translateY(8px) scale(.98); } }
  .pm-head { display: flex; align-items: center; gap: 12px; }
  .pm-ico { width: 48px; height: 48px; flex-shrink: 0; border-radius: 12px; background: var(--bg-input); display: grid; place-items: center; font-size: 24px; overflow: hidden; }
  .pm-title { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 2px; }
  .pm-title input { background: transparent; border: 1px solid transparent; border-radius: 8px; color: var(--fg); font: inherit; font-size: 16px; font-weight: 800; padding: 4px 6px; outline: none; }
  .pm-title input:hover { border-color: var(--border); }
  .pm-title input:focus { border-color: var(--accent); background: var(--bg-input); }
  .pm-sub { font-size: 12px; color: var(--fg-dim); padding-left: 7px; }
  .pm-x { width: 32px; height: 32px; border-radius: 9px; border: none; background: transparent; color: var(--fg-muted); font-size: 22px; cursor: pointer; }
  .pm-x:hover { background: var(--bg-input); color: var(--fg); }
  .pm-tools, .pm-acts, .pm-emrow { display: flex; gap: 6px; align-items: center; flex-wrap: wrap; }
  .grow { flex: 1; }
  .pm-btn { border: 1px solid var(--border); background: var(--bg-input); color: var(--fg); border-radius: 9px; padding: 7px 11px; font: inherit; font-size: 12.5px; font-weight: 700; cursor: pointer; }
  .pm-btn:hover:not(:disabled) { border-color: var(--border-strong); }
  .pm-btn:disabled { opacity: .45; cursor: default; }
  .pm-btn.primary { background: var(--accent); border-color: var(--accent); color: #06070c; }
  .pm-btn.danger { color: #ff6b6b; }
  .pm-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(84px, 1fr)); gap: 6px; overflow-y: auto; min-height: 120px; max-height: 46vh; padding: 2px; touch-action: none; }
  .pm-grid.emoji { grid-template-columns: repeat(auto-fill, minmax(56px, 1fr)); }
  .pm-cell { position: relative; aspect-ratio: 1; border-radius: 12px; border: 2px solid transparent; background: var(--bg-input); padding: 6px; cursor: grab; user-select: none; transition: border-color .12s, transform .12s; }
  .pm-cell:hover { border-color: var(--border-strong); }
  .pm-cell.sel { border-color: var(--accent); background: var(--accent-soft); }
  .pm-cell.dragging { opacity: .5; transform: scale(.94); cursor: grabbing; }
  .pm-cell.icon::after { content: "★"; position: absolute; top: 3px; left: 5px; font-size: 11px; color: var(--accent); }
  .pm-cell.add { display: grid; place-items: center; font-size: 26px; color: var(--fg-dim); border: 2px dashed var(--border-strong); background: transparent; cursor: pointer; }
  .pm-cell.add:hover { color: var(--accent); border-color: var(--accent); }
  .pm-em { position: absolute; right: 4px; bottom: 2px; font-size: 14px; filter: drop-shadow(0 1px 1px rgba(0,0,0,.6)); }
  .pm-empty { text-align: center; color: var(--fg-dim); font-size: 13px; padding: 26px 20px; border: 2px dashed var(--border); border-radius: 14px; }
  .pm-empty-big { font-size: 40px; margin-bottom: 8px; }
  .pm-sel { display: flex; gap: 14px; padding: 10px; border-radius: 14px; background: var(--bg-surface); animation: pm-in .18s ease-out; }
  .pm-pv { width: 132px; height: 132px; flex-shrink: 0; }
  .pm-side { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 8px; }
  .pm-lbl { font-size: 11px; font-weight: 800; letter-spacing: .05em; text-transform: uppercase; color: var(--fg-dim); }
  .pm-eminput { width: 90px; background: var(--bg-input); border: 1px solid var(--border); border-radius: 9px; color: var(--fg); font-size: 20px; text-align: center; padding: 3px 6px; outline: none; }
  .pm-eminput:focus { border-color: var(--accent); }
  .pm-quick { display: grid; grid-template-columns: repeat(12, 1fr); gap: 2px; }
  .pm-quick button { border: 1px solid transparent; background: transparent; border-radius: 8px; font-size: 18px; padding: 2px 0; cursor: pointer; }
  .pm-quick button:hover { background: var(--bg-input); }
  .pm-quick button.on { border-color: var(--accent); background: var(--accent-soft); }
  .pm-hint { font-size: 12px; color: var(--fg-dim); text-align: center; padding: 4px 0 2px; }
</style>
