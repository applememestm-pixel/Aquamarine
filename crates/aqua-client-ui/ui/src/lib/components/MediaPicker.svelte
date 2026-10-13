<script module lang="ts">
  import { writable, get as getStore } from "svelte/store";

  /** Visibility & currently-targeted peer. */
  interface PickerState {
    open: boolean;
    peer: string | null;
  }
  export const pickerState = writable<PickerState>({ open: false, peer: null });
  /** True while an OS file drag hovers the panel — the chat must not
   *  also treat that drop as "send these files". */
  export const panelClaimsDrop = writable(false);

  /** Open the picker for the given peer. */
  export function openMediaPicker(peer: string) {
    pickerState.set({ open: true, peer });
  }

  export function closeMediaPicker() {
    const s = getStore(pickerState);
    pickerState.set({ open: false, peer: s.peer });
    panelClaimsDrop.set(false);
  }
</script>

<script lang="ts">
  import { t, tr } from "../i18n";
  /**
   * Telegram-style Emoji · Stickers · GIFs panel.
   *  - Emoji: recent, custom emoji packs (animated .tgs / .webm too), all
   *    Unicode categories; search in English and Russian.
   *  - Stickers: recent + every pack, pack bar at the bottom.
   *  - GIFs: your saved GIFs (gif / mp4 / webm).
   *  - Drop files onto the panel to add them; import packs from Telegram.
   *  - Keyboard: opens with the search focused; Enter picks the first hit,
   *    Esc clears / closes.
   */
  import { onMount, tick, untrack } from "svelte";
  import type { LocalMediaList, StickerItem, GifItem, StickerPack } from "../types";
  import type { EmojiCat } from "../emojiData";
  import { api, assetUrl } from "../tauri";
  import { showToast } from "./Toast.svelte";
  import { openFileDialog } from "../dialog";
  import { openStickerEditor } from "./StickerEditor.svelte";
  import { insertEmoji } from "../composerBus";
  import { loadActiveMessages } from "../stores";
  import AnimatedMedia from "./AnimatedMedia.svelte";
  import { reactionPack, packOrder, emojiFromFile, REACTION_PACK, loadReactionPack, loadEmojiPacks } from "../reactionPack";

  type Tab = "emoji" | "stickers" | "gifs";
  const TAB_KEY = "aqua.picker.tab";
  let tab = $state<Tab>(((): Tab => { try { return (localStorage.getItem(TAB_KEY) as Tab) || "emoji"; } catch { return "emoji"; } })());
  $effect(() => { try { localStorage.setItem(TAB_KEY, tab); } catch { /* ignore */ } });

  let query = $state("");
  let catalogue = $state<LocalMediaList | null>(null);
  let emojiCats = $state<EmojiCat[] | null>(null);
  let loading = $state(false);
  let panel: HTMLDivElement | undefined = $state();
  let scroller: HTMLDivElement | undefined = $state();
  let searchEl: HTMLInputElement | undefined = $state();
  let pos = $state({ left: 12, bottom: 80 });
  let dropHover = $state(false);
  let menuOpen = $state(false);
  let activeSection = $state("");

  // ---- recents ----
  function loadList(key: string): string[] {
    try { return JSON.parse(localStorage.getItem(key) ?? "[]"); } catch { return []; }
  }
  let recentEmoji = $state<string[]>(loadList("aqua.recent.emoji"));
  let recentStickers = $state<string[]>(loadList("aqua.recent.stickers"));
  function pushRecent(kind: "emoji" | "stickers", v: string) {
    const key = `aqua.recent.${kind}`;
    const list = [v, ...(kind === "emoji" ? recentEmoji : recentStickers).filter((x) => x !== v)].slice(0, kind === "emoji" ? 32 : 20);
    if (kind === "emoji") recentEmoji = list; else recentStickers = list;
    try { localStorage.setItem(key, JSON.stringify(list)); } catch { /* ignore */ }
  }

  // ---- data ----
  async function refreshCatalogue() {
    loading = true;
    try { catalogue = await api.listLocalMedia(); }
    catch (e) { showToast(`Media library: ${e}`); }
    finally { loading = false; }
  }

  $effect(() => {
    if (!$pickerState.open) return;
    untrack(() => {
      void refreshCatalogue();
      if (!emojiCats) void import("../emojiData").then((m) => { emojiCats = m.EMOJI_CATS; });
      if ($reactionPack.source === "none") void loadReactionPack();
      void placeAndFocus();
    });
  });

  async function placeAndFocus() {
    const anchor = document.querySelector("[data-emoji-anchor]")?.getBoundingClientRect();
    const W = 392;
    if (anchor) {
      pos = {
        left: Math.max(8, Math.min(anchor.left - 6, window.innerWidth - W - 8)),
        bottom: Math.max(8, window.innerHeight - anchor.top + 10),
      };
    }
    await tick();
    searchEl?.focus();
  }

  // Unified item shapes.
  interface CItem { kind: "custom"; key: string; token: string; url: string; mime: string; emoji: string; path: string }
  interface UItem { kind: "unicode"; key: string; u: string; words: string }
  type EItem = CItem | UItem;
  interface Section { id: string; title: string; icon: { emoji?: string; item?: CItem | SItem }; items: (EItem | SItem)[] }
  interface SItem { kind: "sticker"; key: string; path: string; url: string; mime: string; emoji: string }

  const slugOf = (p: StickerPack) => p.id.replace(/^pack:/, "");
  const fileOf = (path: string) => path.split(/[\\/]/).pop() ?? "";

  function customFromPack(p: StickerPack): CItem[] {
    // Packs with a Telegram source are addressed by it, so tokens match
    // between people (and with the app-bundled reaction pack).
    const slug = p.source || slugOf(p);
    return packOrder(p.items).map((it) => {
      const file = fileOf(it.path);
      return { kind: "custom", key: `${slug}/${file}`, token: `[ce:${slug}/${file}]`, url: assetUrl(it.path), mime: it.mime, emoji: it.emoji ?? emojiFromFile(file), path: it.path };
    });
  }

  const emojiPacks = $derived.by(() => {
    const packs = (catalogue?.packs ?? []).filter((p) => p.kind === "emoji");
    const out: { id: string; title: string; items: CItem[] }[] = packs.map((p) => ({ id: p.id, title: p.title, items: customFromPack(p) }));
    // App-bundled reaction pack, when it isn't also imported.
    if ($reactionPack.source === "bundled" && !packs.some((p) => p.source === REACTION_PACK)) {
      out.unshift({
        id: "bundled:" + REACTION_PACK, title: "El Taller",
        items: $reactionPack.items.map((r) => ({ kind: "custom", key: `${REACTION_PACK}/${r.file}`, token: `[ce:${REACTION_PACK}/${r.file}]`, url: r.url, mime: r.mime, emoji: r.emoji, path: r.file })),
      });
    }
    return out;
  });
  const stickerPacks = $derived((catalogue?.packs ?? []).filter((p) => p.kind !== "emoji"));

  // emoji char → search words, for matching custom emoji / stickers by meaning
  const wordsByEmoji = $derived.by(() => {
    const m = new Map<string, string>();
    for (const [, , items] of emojiCats ?? []) for (const [u, w] of items) m.set(u.replace(/️/g, ""), w);
    return m;
  });
  const terms = $derived(query.trim().toLowerCase().split(/\s+/).filter(Boolean));
  function matchWords(words: string): boolean {
    return terms.every((t) => words.includes(t));
  }
  function matchEmojiChar(e: string, extra = ""): boolean {
    if (!terms.length) return true;
    const w = (wordsByEmoji.get(e.replace(/️/g, "")) ?? "") + " " + extra.toLowerCase();
    return matchWords(w) || terms.some((t) => e.includes(t));
  }

  const CAT_ICON: Record<string, string> = { people: "😀", nature: "🐻", food: "🍔", activity: "⚽", travel: "🚗", objects: "💡", symbols: "💕", flags: "🏳️" };

  const emojiSections = $derived.by<Section[]>(() => {
    const secs: Section[] = [];
    const allCustom = new Map<string, CItem>();
    for (const p of emojiPacks) for (const it of p.items) allCustom.set(it.token, it);
    if (!terms.length && recentEmoji.length) {
      const items: EItem[] = recentEmoji.map((r) => allCustom.get(r) ?? (r.startsWith("[ce:") ? null : { kind: "unicode", key: "r:" + r, u: r, words: "" })).filter(Boolean) as EItem[];
      if (items.length) secs.push({ id: "recent", title: "Recently used", icon: { emoji: "🕓" }, items });
    }
    for (const p of emojiPacks) {
      const items = p.items.filter((it) => matchEmojiChar(it.emoji, p.title));
      if (items.length) secs.push({ id: p.id, title: p.title, icon: { item: p.items[0] }, items });
    }
    for (const [id, title, list] of emojiCats ?? []) {
      const items: UItem[] = list.filter(([, w]) => !terms.length || matchWords(w)).map(([u, w]) => ({ kind: "unicode", key: id + u, u, words: w }));
      if (items.length) secs.push({ id, title, icon: { emoji: CAT_ICON[id] }, items });
    }
    return secs;
  });

  const stickerSections = $derived.by<Section[]>(() => {
    const secs: Section[] = [];
    const all = new Map<string, SItem>();
    const toS = (it: StickerItem): SItem => ({ kind: "sticker", key: it.path, path: it.path, url: assetUrl(it.path), mime: it.mime, emoji: it.emoji ?? emojiFromFile(fileOf(it.path)) });
    for (const p of stickerPacks) for (const it of p.items) all.set(it.path, toS(it));
    if (!terms.length && recentStickers.length) {
      const items = recentStickers.map((p) => all.get(p)).filter(Boolean) as SItem[];
      if (items.length) secs.push({ id: "recent", title: "Recently used", icon: { emoji: "🕓" }, items });
    }
    for (const p of stickerPacks) {
      const items = packOrder(p.items).map(toS).filter((s) => matchEmojiChar(s.emoji, p.title));
      if (items.length) secs.push({ id: p.id, title: p.title, icon: { item: toS(packOrder(p.items)[0]) }, items });
    }
    return secs;
  });

  const gifs = $derived((catalogue?.gifs ?? []).filter((g) => !terms.length || matchWords(g.name.toLowerCase())));
  const sections = $derived(tab === "emoji" ? emojiSections : tab === "stickers" ? stickerSections : []);

  // ---- actions ----
  function pick(it: EItem | SItem) {
    if (it.kind === "unicode") { insertEmoji(it.u); pushRecent("emoji", it.u); }
    else if (it.kind === "custom") { insertEmoji(it.token); pushRecent("emoji", it.token); }
    else void sendSticker(it);
  }

  async function sendSticker(it: SItem) {
    const s = $pickerState;
    if (!s.peer) return;
    try {
      await api.sendSticker(s.peer, it.path);
      pushRecent("stickers", it.path);
      await loadActiveMessages();
      closeMediaPicker();
    } catch (e) { showToast(`Send failed: ${e}`); }
  }

  async function sendGif(g: GifItem) {
    const s = $pickerState;
    if (!s.peer) return;
    try {
      await api.sendGif(s.peer, g.path);
      await loadActiveMessages();
      closeMediaPicker();
    } catch (e) { showToast(`Send failed: ${e}`); }
  }

  function firstHit() {
    if (tab === "gifs") { if (gifs[0]) void sendGif(gifs[0]); return; }
    const sec = sections.find((s) => s.id !== "recent") ?? sections[0];
    const it = sec?.items[0];
    if (it) pick(it);
  }

  function onSearchKey(e: KeyboardEvent) {
    if (e.key === "Enter") { e.preventDefault(); firstHit(); }
    else if (e.key === "Escape") {
      e.preventDefault(); e.stopPropagation();
      if (query) query = ""; else closeMediaPicker();
    } else if (e.key === "Tab" && !e.shiftKey && !query) {
      e.preventDefault();
      tab = tab === "emoji" ? "stickers" : tab === "stickers" ? "gifs" : "emoji";
    }
  }

  function jumpTo(id: string) {
    const el = scroller?.querySelector<HTMLElement>(`[data-sec="${CSS.escape(id)}"]`);
    if (el && scroller) scroller.scrollTo({ top: el.offsetTop - 4, behavior: "smooth" });
    activeSection = id;
  }
  function onScroll() {
    if (!scroller) return;
    const top = scroller.scrollTop + 8;
    let cur = sections[0]?.id ?? "";
    for (const el of scroller.querySelectorAll<HTMLElement>("[data-sec]")) {
      if (el.offsetTop <= top) cur = el.dataset.sec ?? cur;
    }
    activeSection = cur;
  }
  // New tab / new search → back to the top (but not when "recent" changes).
  $effect(() => {
    void tab; void query;
    untrack(() => { activeSection = sections[0]?.id ?? ""; scroller?.scrollTo({ top: 0 }); });
  });

  // ---- adding things ----
  const targetForTab = (): "emoji" | "stickers" | "gifs" => (tab === "gifs" ? "gifs" : tab === "emoji" ? "emoji" : "stickers");

  async function addPaths(paths: string[]) {
    if (!paths.length) return;
    try {
      const n = await api.addMediaFiles(targetForTab(), null, paths);
      await refreshCatalogue();
      const where = tab === "gifs" ? "GIFs" : tab === "emoji" ? "My emoji" : "My stickers";
      showToast(n ? `Added ${n} file${n === 1 ? "" : "s"} to ${where}` : "Nothing added — use PNG, WEBP, GIF, TGS, WEBM or MP4", 4000);
    } catch (e) { showToast(`Could not add: ${e}`); }
  }

  async function addFiles() {
    menuOpen = false;
    const exts = tab === "gifs" ? ["gif", "mp4", "webm"] : ["png", "webp", "gif", "jpg", "jpeg", "tgs", "webm"];
    try {
      const r = await openFileDialog({ multiple: true, directory: false, title: "Add to your collection", filters: [{ name: "Media", extensions: exts }] });
      const paths = Array.isArray(r) ? r : r ? [r] : [];
      await addPaths(paths as string[]);
    } catch (e) { showToast(`${e}`); }
  }

  async function createSticker() {
    menuOpen = false;
    try {
      const result = await openFileDialog({ multiple: false, directory: false, title: "Pick an image for the sticker", filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp"] }] });
      const path = Array.isArray(result) ? (result[0] ?? null) : result;
      if (!path) return;
      const bytes = await api.readUserImage(path);
      const lower = path.toLowerCase();
      const mime = lower.endsWith(".png") ? "image/png" : lower.endsWith(".webp") ? "image/webp" : "image/jpeg";
      closeMediaPicker();
      openStickerEditor(bytes, mime, path.split(/[\\/]/).pop() ?? "sticker");
    } catch (e) { showToast(`Could not open image: ${e}`); }
  }

  // Telegram import
  const TG_TOKEN_KEY = "aqua.tgBotToken";
  let showImport = $state(false);
  let importing = $state(false);
  let tgToken = $state("");
  let tgPack = $state("");
  try { tgToken = localStorage.getItem(TG_TOKEN_KEY) ?? ""; } catch { /* ignore */ }
  /** The relay can import packs itself (its admin set a bot token). */
  let srv = $state<{ telegram: boolean; packs: { name: string; title: string; type: string; count: number }[] } | null>(null);
  let showTokenField = $derived(!srv?.telegram);
  function openImport(prefill = "") {
    menuOpen = false;
    showImport = true;
    tgPack = prefill;
    void api.serverPacks().then((r) => { srv = r ?? null; }).catch(() => { srv = null; });
  }
  let serverOnly = $derived(
    (srv?.packs ?? []).filter((p) => !(catalogue?.packs ?? []).some((c) => c.source === p.name || c.title === p.title)),
  );
  async function doImport(link?: string) {
    const token = tgToken.trim(), pack = (link ?? tgPack).trim();
    if (!pack) { showToast(tr("Paste a t.me/addstickers/… or t.me/addemoji/… link.")); return; }
    if (!srv?.telegram && !token) { showToast(tr("Enter a Telegram bot token first (from @BotFather).")); return; }
    importing = true;
    try {
      // Through the relay when it can (no token needed here), else directly.
      const r = srv?.telegram ? await api.importPackViaServer(pack) : await api.importTelegramStickers(token, pack);
      if (!srv?.telegram) { try { localStorage.setItem(TG_TOKEN_KEY, token); } catch { /* ignore */ } }
      await refreshCatalogue();
      if (/eltaller47/i.test(pack)) await loadReactionPack();
      void loadEmojiPacks();
      const p = catalogue?.packs.find((x) => x.title === r.pack_title);
      tab = p?.kind === "emoji" ? "emoji" : "stickers";
      showImport = false; tgPack = "";
      showToast(`Imported “${r.pack_title}” — ${r.imported} item${r.imported === 1 ? "" : "s"}`);
      if (p) { await tick(); jumpTo(p.id); }
    } catch (e) { showToast(`Import failed: ${e}`, 5000); }
    finally { importing = false; }
  }

  // ---- outside click / drag & drop ----
  onMount(() => {
    const onDown = (e: MouseEvent) => {
      if (!$pickerState.open || !panel) return;
      const t = e.target as HTMLElement;
      if (panel.contains(t) || t.closest("[data-emoji-anchor]")) return;
      closeMediaPicker();
    };
    window.addEventListener("mousedown", onDown, true);

    let unlisten: (() => void) | null = null;
    import("@tauri-apps/api/webview").then(({ getCurrentWebview }) =>
      getCurrentWebview().onDragDropEvent((ev) => {
        const p = ev.payload as { type: string; position?: { x: number; y: number }; paths?: string[] };
        const inside = () => {
          if (!$pickerState.open || !panel || !p.position) return false;
          const r = panel.getBoundingClientRect(), dpr = window.devicePixelRatio || 1;
          const x = p.position.x / dpr, y = p.position.y / dpr;
          return x >= r.left && x <= r.right && y >= r.top && y <= r.bottom;
        };
        if (p.type === "over" || p.type === "enter") { dropHover = inside(); panelClaimsDrop.set(dropHover); }
        else if (p.type === "leave") { dropHover = false; panelClaimsDrop.set(false); }
        else if (p.type === "drop") {
          const claim = inside();
          dropHover = false;
          if (claim) void addPaths(p.paths ?? []);
          setTimeout(() => panelClaimsDrop.set(false), 0);
        }
      }),
    ).then((u) => { unlisten = u; }).catch(() => {});
    return () => { window.removeEventListener("mousedown", onDown, true); unlisten?.(); };
  });
</script>

{#snippet emojiCell(it: EItem | SItem)}
  {#if it.kind === "unicode"}
    <button class="cell e" title={it.words.split(" ").slice(0, 4).join(" ")} onclick={() => pick(it)}>{it.u}</button>
  {:else if it.kind === "custom"}
    <button class="cell e" title={it.emoji} onclick={() => pick(it)}><AnimatedMedia src={it.url} mime={it.mime} alt={it.emoji} play="hover" /></button>
  {:else}
    <button class="cell s" title={it.emoji} onclick={() => pick(it)}><AnimatedMedia src={it.url} mime={it.mime} alt={it.emoji} play="hover" /></button>
  {/if}
{/snippet}

{#if $pickerState.open}
  <div
    bind:this={panel}
    class="panel"
    class:drop={dropHover}
    style:left="{pos.left}px"
    style:bottom="{pos.bottom}px"
    role="dialog"
    aria-label={$t("Emoji, stickers and GIFs")}
  >
    <div class="top">
      <div class="tabs" role="tablist">
        {#each [["emoji", "Emoji"], ["stickers", "Stickers"], ["gifs", "GIFs"]] as [id, label]}
          <button class="tab" class:active={tab === id} role="tab" aria-selected={tab === id} onclick={() => { tab = id as Tab; searchEl?.focus(); }}>{label}</button>
        {/each}
      </div>
      <div class="more-wrap">
        <button class="icon" title={$t("Add or import")} aria-label={$t("Add or import")} onclick={() => (menuOpen = !menuOpen)}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/></svg>
        </button>
        {#if menuOpen}
          <div class="menu" role="menu">
            <button role="menuitem" onclick={addFiles}>{$t("Add files…")} <span>{$t(tab === "gifs" ? "GIF · MP4" : "PNG · WEBP · TGS")}</span></button>
            <button role="menuitem" onclick={() => openImport("")}>{$t("Import from Telegram…")}</button>
            {#if $reactionPack.source === "none"}
              <button role="menuitem" onclick={() => openImport("https://t.me/addemoji/eltaller47")}>{$t("Get El Taller reactions")}</button>
            {/if}
            {#if tab !== "gifs"}<button role="menuitem" onclick={createSticker}>{$t("Create a sticker…")}</button>{/if}
          </div>
        {/if}
      </div>
    </div>

    <label class="search">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="11" cy="11" r="7"/><line x1="21" y1="21" x2="16.65" y2="16.65"/></svg>
      <input bind:this={searchEl} bind:value={query} onkeydown={onSearchKey}
        placeholder={tab === "emoji" ? "Search emoji" : tab === "stickers" ? "Search stickers" : "Search GIFs"} />
      {#if query}<button class="clear" onclick={() => { query = ""; searchEl?.focus(); }} aria-label={$t("Clear")}>×</button>{/if}
    </label>

    {#if showImport}
      <div class="import">
        <div class="imp-title">{$t("Import from Telegram")}</div>
        {#if showTokenField}
          <input type="password" placeholder={$t("Bot token from @BotFather")} bind:value={tgToken} disabled={importing} />
        {/if}
        <div class="imp-line">
          <input placeholder={$t("t.me/addemoji/… or t.me/addstickers/…")} bind:value={tgPack} disabled={importing}
            onkeydown={(e) => { if (e.key === "Enter") doImport(); }} />
          <button class="imp-go" onclick={() => doImport()} disabled={importing}>{$t(importing ? "…" : "Import")}</button>
        </div>
        {#if serverOnly.length}
          <div class="imp-srv">
            <span class="imp-srv-t">{$t("On this server")}</span>
            {#each serverOnly as p (p.name)}
              <button class="imp-chip" onclick={() => doImport(p.name)} disabled={importing} title={`${p.count}`}>+ {p.title}</button>
            {/each}
          </div>
        {/if}
        <div class="imp-foot">
          <span class="imp-hint">{srv?.telegram ? $t("Just paste the link — your server downloads the pack once for everyone.") : $t("Works for sticker packs and premium emoji packs, no Telegram Premium needed. The token only talks to Telegram.")}</span>
          <button class="imp-close" onclick={() => (showImport = false)}>{$t("Close")}</button>
        </div>
      </div>
    {/if}

    <div class="body" bind:this={scroller} onscroll={onScroll}>
      {#if tab === "gifs"}
        {#if !catalogue && loading}
          <div class="empty">{$t("Loading…")}</div>
        {:else if gifs.length === 0}
          <div class="empty">
            <div class="empty-big">GIF</div>
            {$t(query ? "No GIFs match." : "No saved GIFs yet.")}
            <div class="empty-hint">{$t("Drop GIF / MP4 files here, use")} <b>{$t("+ → Add files")}</b>{$t(", or right-click a GIF in a chat →")} <b>{$t("Save GIF")}</b>.</div>
          </div>
        {:else}
          <div class="gifs">
            {#each gifs as g (g.id)}
              <button class="gif" title={g.name} onclick={() => sendGif(g)}>
                <AnimatedMedia src={assetUrl(g.path)} mime={g.mime ?? "image/gif"} alt={g.name} fit="cover" play="auto" />
              </button>
            {/each}
          </div>
        {/if}
      {:else}
        {#if sections.length === 0}
          <div class="empty">
            {#if query}Nothing found for “{query}”.
            {:else if tab === "stickers"}
              <div class="empty-big">🎨</div>{$t("No sticker packs yet.")}
              <div class="empty-hint">{$t("Import one with")} <b>{$t("+ → Import from Telegram")}</b>{$t(", drop images here, or create your own.")}</div>
            {:else}Loading emoji…{/if}
          </div>
        {/if}
        {#each sections as sec (sec.id)}
          <div class="sec-title" data-sec={sec.id}>{sec.title}</div>
          <div class="grid" class:stickers={tab === "stickers"}>
            {#each sec.items as it (it.key)}{@render emojiCell(it)}{/each}
          </div>
        {/each}
      {/if}
    </div>

    {#if tab !== "gifs" && sections.length > 1 && !query}
      <div class="bar">
        {#each sections as sec (sec.id)}
          <button class="bar-btn" class:active={activeSection === sec.id} title={sec.title} onclick={() => jumpTo(sec.id)}>
            {#if sec.icon.emoji}<span class="bar-emoji">{sec.icon.emoji}</span>
            {:else if sec.icon.item}<AnimatedMedia src={sec.icon.item.url} mime={sec.icon.item.mime} play="never" />{/if}
          </button>
        {/each}
      </div>
    {/if}

    {#if dropHover}
      <div class="drop-veil">Drop to add to {tab === "gifs" ? "GIFs" : tab === "emoji" ? "My emoji" : "My stickers"}</div>
    {/if}
  </div>
{/if}

<style>
  .panel {
    position: fixed; z-index: 95;
    width: 392px; height: min(460px, 70vh);
    display: flex; flex-direction: column;
    background: var(--bg-surface);
    border: 1px solid var(--border-strong);
    border-radius: 14px;
    box-shadow: 0 22px 60px -14px rgba(0, 0, 0, 0.65);
    overflow: hidden;
    animation: pop 0.14s ease-out;
    transform-origin: bottom left;
  }
  @keyframes pop { from { opacity: 0; transform: translateY(8px) scale(0.98); } to { opacity: 1; transform: none; } }
  .panel.drop { border-color: var(--accent); }
  .top { display: flex; align-items: center; justify-content: space-between; padding: 8px 8px 4px 10px; }
  .tabs { display: flex; gap: 2px; }
  .tab {
    padding: 6px 12px; border-radius: 16px; border: none; background: transparent;
    color: var(--fg-muted); font-size: 13.5px; font-weight: 600; cursor: pointer;
  }
  .tab:hover { color: var(--fg); }
  .tab.active { background: var(--accent-soft); color: var(--accent); }
  .more-wrap { position: relative; }
  .icon { width: 30px; height: 30px; border-radius: 8px; border: none; background: transparent; color: var(--fg-muted); cursor: pointer; display: grid; place-items: center; }
  .icon:hover { background: var(--bg-hover); color: var(--fg); }
  .icon svg { width: 18px; height: 18px; }
  .menu {
    position: absolute; right: 0; top: calc(100% + 4px); z-index: 3; min-width: 230px;
    background: var(--bg-surface); border: 1px solid var(--border-strong); border-radius: 10px;
    box-shadow: 0 12px 30px -8px rgba(0, 0, 0, 0.6); padding: 4px;
  }
  .menu button {
    width: 100%; display: flex; justify-content: space-between; gap: 10px; align-items: center;
    background: transparent; border: none; color: var(--fg); text-align: left;
    padding: 8px 10px; border-radius: 6px; font-size: 13px; cursor: pointer;
  }
  .menu button span { color: var(--fg-dim); font-size: 11px; }
  .menu button:hover { background: var(--accent-soft); color: var(--accent); }

  .search {
    display: flex; align-items: center; gap: 8px;
    margin: 4px 10px 6px; padding: 0 10px; height: 34px;
    background: var(--bg-input); border-radius: 17px; color: var(--fg-dim);
  }
  .search svg { width: 15px; height: 15px; flex-shrink: 0; }
  .search input { flex: 1; min-width: 0; background: transparent; border: none; outline: none; color: var(--fg); font-size: 13.5px; font-family: inherit; }
  .clear { background: none; border: none; color: var(--fg-dim); font-size: 18px; cursor: pointer; line-height: 1; }

  .body { flex: 1; overflow-y: auto; padding: 0 6px 8px; position: relative; scrollbar-width: thin; }
  .sec-title {
    position: sticky; top: 0; z-index: 1;
    padding: 8px 6px 4px; font-size: 12.5px; font-weight: 700; color: var(--fg-dim);
    background: linear-gradient(var(--bg-surface) 70%, transparent);
  }
  .grid { display: grid; grid-template-columns: repeat(9, 1fr); gap: 0; }
  .grid.stickers { grid-template-columns: repeat(5, 1fr); gap: 4px; }
  .cell {
    aspect-ratio: 1; border: none; background: transparent; border-radius: 8px; cursor: pointer;
    display: grid; place-items: center; padding: 3px; min-width: 0;
  }
  .cell.e { font-size: 26px; line-height: 1; font-family: "Segoe UI Emoji", "Apple Color Emoji", "Noto Color Emoji", sans-serif; }
  .cell.s { padding: 4px; border-radius: 10px; }
  .cell:hover { background: var(--bg-hover); }
  .cell.s:hover { transform: scale(1.06); background: transparent; }
  .gifs { display: grid; grid-template-columns: repeat(2, 1fr); gap: 4px; padding-top: 4px; }
  .gif { aspect-ratio: 4 / 3; border: none; padding: 0; border-radius: 8px; overflow: hidden; background: var(--bg-input); cursor: pointer; }
  .gif:hover { outline: 2px solid var(--accent); }

  .bar {
    display: flex; gap: 2px; padding: 4px 6px; overflow-x: auto; scrollbar-width: none;
    border-top: 1px solid var(--border); flex-shrink: 0;
  }
  .bar-btn {
    flex-shrink: 0; width: 34px; height: 34px; border: none; border-radius: 8px; background: transparent;
    cursor: pointer; display: grid; place-items: center; padding: 4px; opacity: 0.55; filter: grayscale(0.8);
  }
  .bar-btn:hover { opacity: 0.9; filter: none; background: var(--bg-hover); }
  .bar-btn.active { opacity: 1; filter: none; background: var(--accent-soft); }
  .bar-emoji { font-size: 19px; line-height: 1; }

  .empty { padding: 34px 20px; text-align: center; color: var(--fg-muted); font-size: 13px; }
  .empty-big { font-size: 34px; font-weight: 800; color: var(--fg-dim); margin-bottom: 8px; }
  .empty-hint { margin-top: 8px; font-size: 12px; color: var(--fg-dim); line-height: 1.5; }

  .import { margin: 0 10px 6px; padding: 10px; border-radius: 10px; background: var(--bg-input); display: flex; flex-direction: column; gap: 6px; }
  .imp-title { font-size: 12px; font-weight: 700; color: var(--fg); }
  .import input { background: var(--bg-surface); border: 1px solid var(--border); border-radius: 7px; color: var(--fg); padding: 6px 8px; font-size: 12px; min-width: 0; flex: 1; }
  .import input:focus { outline: none; border-color: var(--accent); }
  .imp-line { display: flex; gap: 6px; }
  .imp-go { border: none; border-radius: 7px; background: var(--accent); color: #06070C; font-weight: 700; font-size: 12px; padding: 0 12px; cursor: pointer; }
  .imp-hint { font-size: 10.5px; color: var(--fg-dim); line-height: 1.4; }
  .import input { height: 32px; box-sizing: border-box; }
  .imp-line { align-items: stretch; }
  .imp-line .imp-go { height: 32px; }
  .imp-foot { display: flex; align-items: center; gap: 8px; }
  .imp-foot .imp-hint { flex: 1; }
  .imp-close { flex: 0 0 auto; height: 26px; padding: 0 10px; border-radius: 7px; border: 1px solid var(--border); background: transparent; color: var(--fg-muted); font-size: 11.5px; font-weight: 600; cursor: pointer; }
  .imp-close:hover { color: var(--fg); background: var(--bg-hover); }
  .imp-srv { display: flex; flex-wrap: wrap; gap: 4px; align-items: center; }
  .imp-srv-t { font-size: 10.5px; font-weight: 700; color: var(--fg-dim); text-transform: uppercase; letter-spacing: 0.04em; margin-right: 2px; }
  .imp-chip { border: 1px solid var(--border); border-radius: 999px; background: transparent; color: var(--fg); font-size: 11.5px; padding: 3px 9px; cursor: pointer; }
  .imp-chip:hover { border-color: var(--accent); color: var(--accent); }

  .drop-veil {
    position: absolute; inset: 0; z-index: 5; display: grid; place-items: center;
    background: color-mix(in srgb, var(--accent) 16%, rgba(0, 0, 0, 0.55));
    color: #fff; font-weight: 800; font-size: 15px; pointer-events: none;
    border: 2px dashed var(--accent); border-radius: 14px;
  }
</style>
