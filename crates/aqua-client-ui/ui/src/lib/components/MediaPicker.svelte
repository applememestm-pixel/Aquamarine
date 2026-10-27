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
  import { onBack } from "../back";
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
  import { openStickerEditor, openStickerQueue, stickerLibraryTick } from "../panels/stickerEditor";
  import { openPackManager } from "./PackManager.svelte";
  import { askInput } from "./InputPrompt.svelte";
  import { insertEmoji } from "../composerBus";
  import { loadActiveMessages } from "../stores";
  import AnimatedMedia from "./AnimatedMedia.svelte";
  import { reactionPack, emojiFromFile, REACTION_PACK, loadReactionPack, loadEmojiPacks } from "../reactionPack";

  /** Telegram's paper plane, for packs that came from there. */
  const TG_ICON = `<svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="12" fill="#2AABEE"/><path fill="#fff" d="M5.4 11.8 16.9 7.3c.53-.2 1 .13.83.95l-1.96 9.23c-.14.66-.53.82-1.08.51l-3-2.21-1.45 1.4c-.16.16-.3.29-.6.29l.21-3.05 5.55-5.01c.24-.21-.05-.33-.38-.12L8.2 13.66l-2.95-.92c-.64-.2-.65-.64.14-.94z"/></svg>`;
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
  let fromRight = $state(false);
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
  // A sticker made in the editor shows up without reopening the panel.
  let seenTick = 0;
  $effect(() => { const n = $stickerLibraryTick; if (n !== seenTick) { seenTick = n; untrack(() => void refreshCatalogue()); } });
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
      void checkUpdates();
    });
  });

  // ---- Phones: the panel takes the keyboard's place (Telegram) ----
  // The keyboard's height is learnt from the visual viewport while it's up;
  // opening the panel hides the keyboard and the panel fills the same space,
  // so the composer doesn't jump. Tapping the message field brings the
  // keyboard back and the panel goes.
  const phone = () => matchMedia("(max-width: 720px)").matches;
  const KB_KEY = "aqua.kbH";
  let kbH = $state((() => { try { return Number(localStorage.getItem(KB_KEY)) || 0; } catch { return 0; } })());
  onMount(() => {
    const vv = window.visualViewport;
    if (!vv) return;
    const on = () => {
      const h = Math.round(window.innerHeight - vv.height);
      if (h > 140 && h < window.innerHeight * 0.7) {
        kbH = h;
        try { localStorage.setItem(KB_KEY, String(h)); } catch { /* ignore */ }
      }
    };
    vv.addEventListener("resize", on);
    return () => vv.removeEventListener("resize", on);
  });
  // Back closes the panel (lib/back.ts).
  $effect(() => { if ($pickerState.open && phone()) return onBack(() => closeMediaPicker()); });
  const dockH = $derived(Math.max(330, Math.min(kbH || 0, 480)) || 330);
  $effect(() => {
    const docked = $pickerState.open && phone();
    const r = document.documentElement;
    r.classList.toggle("picker-docked", docked);
    if (docked) r.style.setProperty("--dock-h", `${dockH}px`);
  });
  // The message field gets the finger: keyboard up, panel down.
  onMount(() => {
    const focusIn = (e: FocusEvent) => {
      if (!$pickerState.open || !phone() || !panel) return;
      const t = e.target as HTMLElement;
      if (panel.contains(t)) return;
      if (t.closest("textarea, [contenteditable], input")) closeMediaPicker();
    };
    document.addEventListener("focusin", focusIn);
    return () => document.removeEventListener("focusin", focusIn);
  });

  async function placeAndFocus() {
    if (phone()) {
      // Hide the keyboard; the panel takes its place.
      (document.activeElement as HTMLElement | null)?.blur?.();
      return;
    }
    const anchor = document.querySelector("[data-emoji-anchor]")?.getBoundingClientRect();
    const W = 440;
    if (anchor) {
      // The button sits at the right of the field (like Telegram): the
      // panel grows up and to the left from it.
      const right = anchor.left > window.innerWidth / 2;
      fromRight = right;
      pos = {
        left: Math.max(8, Math.min(right ? anchor.right + 6 - W : anchor.left - 6, window.innerWidth - W - 8)),
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
  interface Section { id: string; title: string; icon: { emoji?: string; item?: CItem | SItem }; items: (EItem | SItem)[]; pack?: StickerPack }
  interface SItem { kind: "sticker"; key: string; path: string; url: string; mime: string; emoji: string }

  const slugOf = (p: StickerPack) => p.id.replace(/^pack:/, "");
  const fileOf = (path: string) => path.split(/[\\/]/).pop() ?? "";

  function customFromPack(p: StickerPack): CItem[] {
    // Packs with a Telegram source are addressed by it, so tokens match
    // between people (and with the app-bundled reaction pack).
    const slug = p.source || slugOf(p);
    return p.items.map((it) => {
      const file = fileOf(it.path);
      return { kind: "custom", key: `${slug}/${file}`, token: `[ce:${slug}/${file}]`, url: assetUrl(it.path), mime: it.mime, emoji: it.emoji ?? emojiFromFile(file), path: it.path };
    });
  }

  const emojiPacks = $derived.by(() => {
    const packs = (catalogue?.packs ?? []).filter((p) => p.kind === "emoji");
    const out: { id: string; title: string; items: CItem[]; pack?: StickerPack }[] = packs.map((p) => ({ id: p.id, title: p.title, items: customFromPack(p), pack: p }));
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
      const icon = (p.pack?.icon && p.items.find((i) => i.path === p.pack!.icon)) || p.items[0];
      if (items.length || (!terms.length && p.pack?.editable)) secs.push({ id: p.id, title: p.title, icon: icon ? { item: icon } : { emoji: "😀" }, items, pack: p.pack });
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
      const items = p.items.map(toS).filter((s) => matchEmojiChar(s.emoji, p.title));
      const ic = p.items.find((i) => i.path === p.icon) ?? p.items[0];
      if (items.length || (!terms.length && p.editable)) secs.push({ id: p.id, title: p.title, icon: ic ? { item: toS(ic) } : { emoji: "🖼" }, items, pack: p });
    }
    return secs;
  });

  const gifs = $derived((catalogue?.gifs ?? []).filter((g) => !terms.length || matchWords(g.name.toLowerCase())));
  /** Keyed lists must never repeat a key (Svelte throws and the picker
   *  stops updating) — drop repeats instead. */
  function uniq<T>(list: T[], key: (x: T) => string): T[] {
    const seen = new Set<string>();
    return list.filter((x) => { const k = key(x); if (seen.has(k)) return false; seen.add(k); return true; });
  }
  const sections = $derived(
    uniq(tab === "emoji" ? emojiSections : tab === "stickers" ? stickerSections : [], (s) => s.id)
      .map((s) => ({ ...s, items: uniq(s.items, (it) => it.key) })),
  );

  // ---- actions ----
  // ---- Telegram-style preview: press and hold a sticker / custom emoji,
  // slide over others to switch, release to close (nothing is sent).
  let preview = $state<CItem | SItem | null>(null);
  let holdTimer: ReturnType<typeof setTimeout> | null = null;
  let suppressClick = false;
  const byKey = $derived.by(() => {
    const m = new Map<string, CItem | SItem>();
    for (const sec of sections) for (const it of sec.items) if (it.kind !== "unicode") m.set(it.key, it);
    return m;
  });
  function holdStart(e: PointerEvent, it: EItem | SItem) {
    if (it.kind === "unicode" || e.button !== 0) return;
    if (holdTimer) clearTimeout(holdTimer);
    holdTimer = setTimeout(() => { preview = it; suppressClick = true; holdTimer = null; }, 320);
  }
  function holdMove(e: PointerEvent) {
    if (!preview) return;
    const cell = (document.elementFromPoint(e.clientX, e.clientY) as HTMLElement | null)?.closest<HTMLElement>("[data-pk]");
    const it = cell ? byKey.get(cell.dataset.pk ?? "") : null;
    if (it && it !== preview) preview = it;
  }
  function holdEnd() {
    if (holdTimer) { clearTimeout(holdTimer); holdTimer = null; }
    if (preview) preview = null;
  }
  function pickGuarded(it: EItem | SItem) {
    if (suppressClick) { suppressClick = false; return; }
    pick(it);
  }

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
      // The panel stays open, as in Telegram: send several in a row.
    } catch (e) { showToast(`Send failed: ${e}`); }
  }

  async function sendGif(g: GifItem) {
    const s = $pickerState;
    if (!s.peer) return;
    try {
      await api.sendGif(s.peer, g.path);
      await loadActiveMessages();
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

  // Each section is its own <section> so its sticky title stays inside it:
  // a title stuck at the top no longer reports the scroll position as its own
  // offset, which made jumps back up land where the list already was.
  let jumping: ReturnType<typeof setTimeout> | null = null;
  function jumpTo(id: string) {
    const el = scroller?.querySelector<HTMLElement>(`[data-sec="${CSS.escape(id)}"]`);
    activeSection = id;
    if (!el || !scroller) return;
    const target = el.offsetTop;
    const from = scroller.scrollTop;
    // Far away: hop most of the way at once, glide the last screen (as in Telegram).
    const screen = scroller.clientHeight;
    if (Math.abs(target - from) > screen * 2) scroller.scrollTop = target + (target > from ? -screen : screen);
    scroller.scrollTo({ top: target, behavior: "smooth" });
    if (jumping) clearTimeout(jumping);
    jumping = setTimeout(() => { jumping = null; }, 600);
  }
  // Keep the highlighted pack icon visible in a long bottom bar.
  let bar = $state<HTMLElement | null>(null);
  $effect(() => {
    const id = activeSection;
    const b = bar?.querySelector<HTMLElement>(`[data-bar="${CSS.escape(id)}"]`);
    b?.scrollIntoView({ block: "nearest", inline: "nearest", behavior: "smooth" });
  });
  function onScroll() {
    if (!scroller || jumping) return;
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

  // ---- Telegram packs follow their originals ----
  let lastCheck = 0;
  async function checkUpdates() {
    if (Date.now() - lastCheck < 15 * 60_000) return;
    lastCheck = Date.now();
    try {
      const done = await api.checkPackUpdates();
      if (done.length) { await refreshCatalogue(); showToast(tr("Updated from Telegram: {list}", { list: done.map((d) => d.title).join(", ") }), 4000); }
    } catch { /* offline / old server */ }
  }
  let syncing = $state("");
  async function syncPack(p: StickerPack) {
    syncing = p.title;
    try {
      const r = await api.syncTelegramPack(p.title);
      await refreshCatalogue();
      showToast(r.changed ? tr("“{pack}” updated: +{a} −{r}", { pack: p.title, a: r.added, r: r.removed }) : tr("“{pack}” is up to date", { pack: p.title }), 3500);
    } catch (e) { showToast(`${e}`, 5000); } finally { syncing = ""; }
  }

  // ---- own packs (the @Stickers bot, see PackManager) ----
  async function quickAdd(p: StickerPack) {
    try {
      const r = await openFileDialog({ multiple: true, directory: false, title: tr("Add to “{pack}”", { pack: p.title }),
        filters: [{ name: tr("Pictures and animations"), extensions: ["png", "jpg", "jpeg", "webp", "gif", "webm", "tgs"] }] });
      const paths = (Array.isArray(r) ? r : r ? [r] : []) as string[];
      if (!paths.length) return;
      const stills = paths.filter((x) => /\.(png|jpe?g|webp)$/i.test(x));
      const anim = paths.filter((x) => !/\.(png|jpe?g|webp)$/i.test(x));
      if (anim.length) { await api.packAddFiles(p.title, anim); await refreshCatalogue(); }
      if (stills.length) { closeMediaPicker(); await openStickerQueue(stills, p.title); }
    } catch (e) { showToast(`${e}`); }
  }
  function manage(p: StickerPack) { closeMediaPicker(); openPackManager(p.title); }
  function newPack(kind: "sticker" | "emoji") {
    menuOpen = false;
    askInput({
      title: tr(kind === "emoji" ? "New emoji pack" : "New sticker pack"),
      description: tr(kind === "emoji" ? "Your own custom emoji: add pictures, give each an emoji, use them in messages and reactions." : "Then add pictures — the editor cuts out the background and adds captions — or GIF / WEBM / TGS as they are."),
      fields: [{ label: tr("Pack name"), placeholder: tr(kind === "emoji" ? "My emoji" : "My stickers") }],
      submitLabel: tr("Create"),
      onSubmit: async ([name]) => {
        const n = name.trim();
        if (!n) return tr("Give the pack a name");
        try { const tt = await api.createStickerPack(n, kind); closeMediaPicker(); openPackManager(tt); stickerLibraryTick.update((x) => x + 1); return null; }
        catch (e) { return String(e); }
      },
    });
  }

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
  /** "loading" until the relay answers, "old" if it has no packs API. */
  let srvState = $state<"loading" | "ok" | "old" | "offline">("loading");
  /** Users never need a token — unless they opt into their own (the relay
   *  isn't set up). Someone who already saved one keeps using it. */
  let ownToken = $state(false);
  try { ownToken = !!localStorage.getItem(TG_TOKEN_KEY); } catch { /* ignore */ }
  let relayCan = $derived(srvState === "ok" && !!srv?.telegram);
  let showTokenField = $derived(srvState !== "loading" && !relayCan && ownToken);
  function openImport(prefill = "") {
    menuOpen = false;
    showImport = true;
    tgPack = prefill;
    srvState = "loading";
    void api.serverPacks()
      .then((r) => { srv = r ?? null; srvState = r ? "ok" : "old"; })
      .catch((e) => { srv = null; srvState = /404|not found/i.test(String(e)) ? "old" : "offline"; });
  }
  let serverOnly = $derived(
    uniq(srv?.packs ?? [], (p) => p.name).filter((p) => !(catalogue?.packs ?? []).some((c) => c.source === p.name || c.title === p.title)),
  );
  async function doImport(link?: string) {
    const token = tgToken.trim(), pack = (link ?? tgPack).trim();
    if (!pack) { showToast(tr("Paste a t.me/addstickers/… or t.me/addemoji/… link.")); return; }
    if (!relayCan && !ownToken) { showToast(tr("Your server can't download packs yet — its admin enters a Telegram bot token once (when the server starts or in the server panel).")); return; }
    if (!relayCan && !token) { showToast(tr("Enter a Telegram bot token first (from @BotFather).")); return; }
    importing = true;
    try {
      // Through the relay when it can (no token needed here), else directly.
      const r = relayCan ? await api.importPackViaServer(pack) : await api.importTelegramStickers(token, pack);
      if (!relayCan) { try { localStorage.setItem(TG_TOKEN_KEY, token); } catch { /* ignore */ } }
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

  // Releasing anywhere ends a press-and-hold preview.
  onMount(() => {
    const up = () => holdEnd();
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", up);
    return () => { window.removeEventListener("pointerup", up); window.removeEventListener("pointercancel", up); };
  });

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
    <button class="cell e" title={it.emoji} data-pk={it.key} onpointerdown={(e) => holdStart(e, it)} onclick={() => pickGuarded(it)}><AnimatedMedia lazy src={it.url} mime={it.mime} alt={it.emoji} play="hover" /></button>
  {:else}
    <button class="cell s" title={it.emoji} data-pk={it.key} onpointerdown={(e) => holdStart(e, it)} onclick={() => pickGuarded(it)}><AnimatedMedia lazy src={it.url} mime={it.mime} alt={it.emoji} play="hover" /></button>
  {/if}
{/snippet}

{#if $pickerState.open}
  <div
    bind:this={panel}
    data-media-picker
    class="panel"
    class:drop={dropHover}
    class:docked={phone()}
    class:from-right={fromRight && !phone()}
    style:left="{pos.left}px"
    style:bottom="{pos.bottom}px"
    role="dialog"
    aria-label={$t("Emoji, stickers and GIFs")}
    onpointermove={holdMove}
  >
    <div class="top">
      <div class="tabs" role="tablist">
        {#each [["emoji", "Emoji"], ["stickers", "Stickers"], ["gifs", "GIFs"]] as [id, label]}
          <button class="tab" class:active={tab === id} role="tab" aria-selected={tab === id} onclick={() => { tab = id as Tab; if (!phone()) searchEl?.focus(); }}>{$t(label)}</button>
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
            {#if tab !== "gifs"}
              <button role="menuitem" onclick={() => newPack("sticker")}>{$t("New sticker pack…")}</button>
              <button role="menuitem" onclick={() => newPack("emoji")}>{$t("New emoji pack…")}</button>
            {/if}
          </div>
        {/if}
      </div>
    </div>

    <label class="search">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="11" cy="11" r="7"/><line x1="21" y1="21" x2="16.65" y2="16.65"/></svg>
      <input bind:this={searchEl} bind:value={query} onkeydown={onSearchKey}
        placeholder={$t(tab === "emoji" ? "Search emoji" : tab === "stickers" ? "Search stickers" : "Search GIFs")} />
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
        {#if importing}
          <div class="imp-busy"><span class="imp-spin"></span>{relayCan ? $t("The server is downloading the pack from Telegram — usually a few seconds, big packs up to a minute.") : $t("Downloading the pack from Telegram…")}</div>
        {/if}
        {#if serverOnly.length}
          <div class="imp-srv">
            <span class="imp-srv-t">{$t("On this server")}</span>
            {#each serverOnly as p (p.name)}
              <button class="imp-chip" onclick={() => doImport(p.name)} disabled={importing} title={`${p.count}`}>+ {p.title}</button>
            {/each}
          </div>
        {/if}
        {#if srvState !== "loading" && !relayCan && !ownToken}
          <div class="imp-note">
            {srvState === "offline"
              ? $t("Not connected to a server — connect first, or use your own bot token.")
              : srvState === "old"
              ? $t("This server is an older version that can't download packs. Update aqua-server — then it asks for a Telegram bot token once at start, and everyone just pastes links.")
              : $t("Your server can't download packs yet: its admin enters a Telegram bot token once — when the server starts, or in the server panel → “Stickers & emoji from Telegram”.")}
            <button class="imp-own" onclick={() => (ownToken = true)}>{$t("Use my own bot token instead")}</button>
          </div>
        {/if}
        <div class="imp-foot">
          <span class="imp-hint">{relayCan ? $t("Just paste the link — your server downloads the pack once for everyone.") : showTokenField ? $t("Works for sticker packs and premium emoji packs, no Telegram Premium needed. The token only talks to Telegram.") : ""}</span>
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
          <section class="sec" data-sec={sec.id}>
          <div class="sec-title">
            {#if sec.pack?.source}<span class="tg" title={$t("From Telegram — read-only")}>{@html TG_ICON}</span>{/if}
            <span class="sec-name">{sec.id === "recent" ? $t(sec.title) : sec.title}</span>
            {#if sec.pack?.source}
              <button class="sec-act" class:spin={syncing === sec.pack.title} onclick={() => syncPack(sec.pack!)} title={$t("Check for new stickers in Telegram")} aria-label={$t("Update")} disabled={!!syncing}>↻</button>
            {:else if sec.pack?.editable}
              <button class="sec-act" onclick={() => quickAdd(sec.pack!)} title={$t("Add to this pack")} aria-label={$t("Add")}>＋</button>
              <button class="sec-act" onclick={() => manage(sec.pack!)} title={$t("Edit pack: emoji, order, icon, delete")} aria-label={$t("Edit pack")}>✎</button>
            {/if}
          </div>
          <div class="grid" class:stickers={tab === "stickers"}>
            {#each sec.items as it (it.key)}{@render emojiCell(it)}{/each}
            {#if sec.pack?.editable && !sec.items.length}
              <button class="cell add-cell" class:s={tab === "stickers"} onclick={() => quickAdd(sec.pack!)}>＋</button>
            {/if}
          </div>
          </section>
        {/each}
      {/if}
    </div>

    {#if tab !== "gifs" && sections.length > 1 && !query}
      <div class="bar" bind:this={bar}>
        {#each sections as sec (sec.id)}
          <button class="bar-btn" data-bar={sec.id} class:active={activeSection === sec.id} title={sec.title} onclick={() => jumpTo(sec.id)}>
            {#if sec.icon.emoji}<span class="bar-emoji">{sec.icon.emoji}</span>
            {:else if sec.icon.item}<AnimatedMedia src={sec.icon.item.url} mime={sec.icon.item.mime} play="never" />{/if}
          </button>
        {/each}
      </div>
    {/if}

    {#if preview}
      <div class="pv" class:emoji={preview.kind === "custom"} aria-hidden="true">
        <div class="pv-media"><AnimatedMedia src={preview.url} mime={preview.mime} alt={preview.emoji} play="auto" /></div>
        <div class="pv-emoji">{preview.emoji}</div>
      </div>
    {/if}

    {#if dropHover}
      <div class="drop-veil">Drop to add to {tab === "gifs" ? "GIFs" : tab === "emoji" ? "My emoji" : "My stickers"}</div>
    {/if}
  </div>
{/if}

<style>
  .pv {
    position: absolute; inset: 0; z-index: 5; pointer-events: none;
    display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 10px;
    background: color-mix(in srgb, var(--bg-surface) 72%, transparent);
    backdrop-filter: blur(6px);
    border-radius: inherit;
    animation: pv-in 0.14s ease-out;
  }
  .pv-media { width: 220px; height: 220px; }
  .pv.emoji .pv-media { width: 128px; height: 128px; }
  .pv-emoji { font-size: 28px; line-height: 1; font-family: "Segoe UI Emoji", "Apple Color Emoji", "Noto Color Emoji", sans-serif; }
  @keyframes pv-in { from { opacity: 0; transform: scale(0.92); } to { opacity: 1; transform: none; } }

  .panel {
    position: fixed; z-index: 95;
    width: 440px; height: min(560px, 74vh);
    display: flex; flex-direction: column;
    background: var(--bg-surface);
    border: 1px solid var(--border-strong);
    border-radius: calc(14px * var(--rk, 1));
    box-shadow: 0 22px 60px -14px rgba(0, 0, 0, 0.65);
    overflow: hidden;
    animation: pop 0.24s cubic-bezier(.2, .9, .25, 1);
    transform-origin: bottom left;
  }
  .panel.from-right { transform-origin: bottom right; }
  /* Unfolds from the emoji button, like Telegram's panel. */
  @keyframes pop {
    from { opacity: 0; transform: translateY(10px) scale(0.86); clip-path: inset(35% 0 0 0 round 14px); }
    to { opacity: 1; transform: none; clip-path: inset(0 0 0 0 round 14px); }
  }
  .panel.drop { border-color: var(--accent); }
  .top { display: flex; align-items: center; justify-content: space-between; padding: 4px 8px 0 10px; border-bottom: 1px solid var(--border); }
  /* Telegram-like: plain tabs across the top, the open one underlined. */
  .tabs { display: flex; gap: 2px; flex: 1; justify-content: space-around; }
  .tab {
    position: relative; padding: 10px 14px 11px; border: none; background: transparent;
    color: var(--fg-muted); font-size: 14px; font-weight: 600; cursor: pointer; transition: color .15s;
  }
  .tab::after { content: ""; position: absolute; left: 10px; right: 10px; bottom: -1px; height: 3px; border-radius: 3px 3px 0 0; background: var(--accent); transform: scaleX(0); transition: transform .2s cubic-bezier(.2,.9,.3,1); }
  .tab:hover { color: var(--fg); }
  .tab.active { color: var(--accent); }
  .tab.active::after { transform: scaleX(1); }
  .more-wrap { position: relative; }
  .icon { width: 30px; height: 30px; border-radius: calc(8px * var(--rk, 1)); border: none; background: transparent; color: var(--fg-muted); cursor: pointer; display: grid; place-items: center; }
  .icon:hover { background: var(--bg-hover); color: var(--fg); }
  .icon svg { width: 18px; height: 18px; }
  .menu {
    position: absolute; right: 0; top: calc(100% + 4px); z-index: 3; min-width: 230px;
    background: var(--bg-surface); border: 1px solid var(--border-strong); border-radius: calc(10px * var(--rk, 1));
    box-shadow: 0 12px 30px -8px rgba(0, 0, 0, 0.6); padding: 4px;
  }
  .menu button {
    width: 100%; display: flex; justify-content: space-between; gap: 10px; align-items: center;
    background: transparent; border: none; color: var(--fg); text-align: left;
    padding: 8px 10px; border-radius: calc(6px * var(--rk, 1)); font-size: 13px; cursor: pointer;
  }
  .menu button span { color: var(--fg-dim); font-size: 11px; }
  .menu button:hover { background: var(--accent-soft); color: var(--accent); }

  .search {
    display: flex; align-items: center; gap: 8px;
    margin: 8px 10px 6px; padding: 0 12px; height: 36px;
    background: var(--bg-input); border-radius: calc(17px * var(--rk, 1)); color: var(--fg-dim);
  }
  .search svg { width: 15px; height: 15px; flex-shrink: 0; }
  .search input { flex: 1; min-width: 0; background: transparent; border: none; outline: none; color: var(--fg); font-size: 13.5px; font-family: inherit; }
  .clear { background: none; border: none; color: var(--fg-dim); font-size: 18px; cursor: pointer; line-height: 1; }

  .body { flex: 1; overflow-y: auto; padding: 0 6px 8px; position: relative; scrollbar-width: thin; }
  .sec-title { display: flex; align-items: center; gap: 6px; }
  .sec-name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .tg { width: 14px; height: 14px; flex-shrink: 0; display: inline-grid; }
  .tg :global(svg) { width: 14px; height: 14px; }
  .sec-act { width: 22px; height: 22px; flex-shrink: 0; border-radius: calc(6px * var(--rk, 1)); border: none; background: var(--bg-input); color: var(--fg-muted); cursor: pointer; font-size: 13px; line-height: 1; display: grid; place-items: center; }
  .sec-act:hover:not(:disabled) { background: var(--bg-input); color: var(--accent); }
  .sec-act.spin { animation: sec-spin .9s linear infinite; }
  @keyframes sec-spin { to { transform: rotate(360deg); } }
  .add-cell { display: grid; place-items: center; font-size: 22px; color: var(--fg-dim); border: 2px dashed var(--border-strong) !important; border-radius: calc(10px * var(--rk, 1)); }
  .add-cell:hover { color: var(--accent); border-color: var(--accent) !important; }
  .sec-title {
    position: sticky; top: 0; z-index: 1;
    padding: 8px 6px 4px; font-size: 12.5px; font-weight: 700; color: var(--fg-dim);
    background: linear-gradient(var(--bg-surface) 70%, transparent);
  }
  .grid { display: grid; grid-template-columns: repeat(9, 1fr); gap: 0; }
  .grid.stickers { grid-template-columns: repeat(5, 1fr); gap: 6px; padding: 0 2px; }
  .cell {
    aspect-ratio: 1; border: none; background: transparent; border-radius: calc(8px * var(--rk, 1)); cursor: pointer;
    display: grid; place-items: center; padding: 3px; min-width: 0;
  }
  .cell.e { font-size: 26px; line-height: 1; font-family: "Segoe UI Emoji", "Apple Color Emoji", "Noto Color Emoji", sans-serif; }
  .cell.s { padding: 4px; border-radius: calc(10px * var(--rk, 1)); }
  .cell:hover { background: var(--bg-hover); }
  .cell.s:hover { transform: scale(1.06); background: transparent; }
  .gifs { display: grid; grid-template-columns: repeat(2, 1fr); gap: 4px; padding-top: 4px; }
  .gif { aspect-ratio: 4 / 3; border: none; padding: 0; border-radius: calc(8px * var(--rk, 1)); overflow: hidden; background: var(--bg-input); cursor: pointer; }
  .gif:hover { outline: 2px solid var(--accent); }

  .bar {
    display: flex; gap: 2px; padding: 4px 6px; overflow-x: auto; scrollbar-width: none;
    border-top: 1px solid var(--border); flex-shrink: 0;
  }
  .bar-btn {
    flex-shrink: 0; width: 38px; height: 38px; border: none; border-radius: calc(8px * var(--rk, 1)); background: transparent;
    cursor: pointer; display: grid; place-items: center; padding: 4px; opacity: 0.55; filter: grayscale(0.8);
  }
  .bar-btn:hover { opacity: 0.9; filter: none; background: var(--bg-hover); }
  .bar-btn.active { opacity: 1; filter: none; background: var(--accent-soft); }
  .bar-emoji { font-size: 19px; line-height: 1; }

  .empty { padding: 34px 20px; text-align: center; color: var(--fg-muted); font-size: 13px; }
  .empty-big { font-size: 34px; font-weight: 800; color: var(--fg-dim); margin-bottom: 8px; }
  .empty-hint { margin-top: 8px; font-size: 12px; color: var(--fg-dim); line-height: 1.5; }

  .import { margin: 0 10px 6px; padding: 10px; border-radius: calc(10px * var(--rk, 1)); background: var(--bg-input); display: flex; flex-direction: column; gap: 6px; }
  .imp-title { font-size: 12px; font-weight: 700; color: var(--fg); }
  .import input { background: var(--bg-surface); border: 1px solid var(--border); border-radius: calc(7px * var(--rk, 1)); color: var(--fg); padding: 6px 8px; font-size: 12px; min-width: 0; flex: 1; }
  .import input:focus { outline: none; border-color: var(--accent); }
  .imp-line { display: flex; gap: 6px; }
  .imp-go { border: none; border-radius: calc(7px * var(--rk, 1)); background: var(--accent); color: #06070C; font-weight: 700; font-size: 12px; padding: 0 12px; cursor: pointer; }
  .imp-hint { font-size: 10.5px; color: var(--fg-dim); line-height: 1.4; }
  .import input { height: 32px; box-sizing: border-box; }
  .imp-line { align-items: stretch; }
  .imp-line .imp-go { height: 32px; }
  .imp-foot { display: flex; align-items: center; gap: 8px; }
  .imp-foot .imp-hint { flex: 1; }
  .imp-close { flex: 0 0 auto; height: 26px; padding: 0 10px; border-radius: calc(7px * var(--rk, 1)); border: 1px solid var(--border); background: transparent; color: var(--fg-muted); font-size: 11.5px; font-weight: 600; cursor: pointer; }
  .imp-close:hover { color: var(--fg); background: var(--bg-hover); }
  .imp-busy { display: flex; align-items: center; gap: 8px; font-size: 11.5px; color: var(--fg-muted); line-height: 1.4; }
  .imp-spin {
    width: 12px; height: 12px; flex-shrink: 0; border-radius: 50%;
    border: 2px solid color-mix(in srgb, var(--accent) 25%, transparent); border-top-color: var(--accent);
    animation: imp-spin 0.8s linear infinite;
  }
  @keyframes imp-spin { to { transform: rotate(360deg); } }
  .imp-note {
    font-size: 11.5px; line-height: 1.45; color: var(--fg-muted);
    padding: 8px 10px; border-radius: calc(8px * var(--rk, 1));
    background: color-mix(in srgb, var(--accent) 7%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent) 22%, transparent);
  }
  .imp-own {
    display: block; margin-top: 4px; padding: 0; border: none; background: none;
    color: var(--accent); font-size: 11.5px; font-weight: 600; cursor: pointer;
  }
  .imp-own:hover { text-decoration: underline; }
  .imp-srv { display: flex; flex-wrap: wrap; gap: 4px; align-items: center; }
  .imp-srv-t { font-size: 10.5px; font-weight: 700; color: var(--fg-dim); text-transform: uppercase; letter-spacing: 0.04em; margin-right: 2px; }
  .imp-chip { border: 1px solid var(--border); border-radius: 999px; background: transparent; color: var(--fg); font-size: 11.5px; padding: 3px 9px; cursor: pointer; }
  .imp-chip:hover { border-color: var(--accent); color: var(--accent); }

  .drop-veil {
    position: absolute; inset: 0; z-index: 5; display: grid; place-items: center;
    background: color-mix(in srgb, var(--accent) 16%, rgba(0, 0, 0, 0.55));
    color: #fff; font-weight: 800; font-size: 15px; pointer-events: none;
    border: 2px dashed var(--accent); border-radius: calc(14px * var(--rk, 1));
  }

  /* ---- Phones: docked where the keyboard was, tabs as a pill below ---- */
  @media (max-width: 720px) {
    .panel.docked {
      left: 0 !important; right: 0; bottom: 0 !important; width: auto;
      height: calc(var(--dock-h, 300px) + env(safe-area-inset-bottom, 0px));
      padding-bottom: env(safe-area-inset-bottom, 0px);
      border-radius: 20px 20px 0 0; border-left: none; border-right: none; border-bottom: none;
      animation: dock-up .2s cubic-bezier(.2,.8,.2,1);
      box-shadow: 0 -10px 30px -18px rgba(0,0,0,.7);
    }
    /* Phone, like Telegram: search, then the packs, then a big grid; the
       tabs are a slim strip at the bottom (it was a tall pill that ate the
       grid and overlapped the pack bar). */
    .panel.docked .search { order: 1; margin: 8px 10px 4px; height: 34px; }
    .panel.docked .bar { order: 2; border-top: none; border-bottom: 1px solid var(--border); padding: 2px 6px 4px; }
    .panel.docked .bar-btn { width: 34px; height: 34px; }
    .panel.docked .body { order: 3; }
    .panel.docked .top { order: 9; justify-content: center; position: relative; padding: 0 44px 2px; border-bottom: none; border-top: 1px solid var(--border); }
    .panel.docked .tabs { justify-content: space-around; }
    .panel.docked .tab { padding: 8px 10px 9px; font-size: 13.5px; }
    .panel.docked .grid { grid-template-columns: repeat(8, 1fr); }
    .panel.docked .grid.stickers { grid-template-columns: repeat(4, 1fr); }
    .panel.docked .more-wrap { position: absolute; right: 10px; }
    .panel.docked .menu { top: auto; bottom: calc(100% + 6px); }
  }
  @keyframes dock-up { from { transform: translateY(40%); opacity: .4; } }
</style>
