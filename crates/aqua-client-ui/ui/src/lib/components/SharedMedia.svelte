<script lang="ts">
  import { streamSrc } from "../media";
  import { t, tr, lang } from "../i18n";
  import { api, assetUrl } from "../tauri";
  import { openGallery, type GalleryItem } from "./Lightbox.svelte";
  import { playQueue } from "../player";
  import { showToast } from "./Toast.svelte";
  import type { ChatMessage } from "../types";

  /** One chat's photos, videos, files, music, voice, GIFs and links — in the
   *  side panel and in a person's profile. Telegram's shared media: tabs
   *  with an underline (only the ones that have something), a tight grid,
   *  pinch (or Ctrl + wheel) to change how many columns. */
  let { peerId, title = "" }: { peerId: string; title?: string } = $props();
  type Tab = "media" | "files" | "links" | "music" | "voice" | "gifs";
  let tab = $state<Tab>("media");

  interface Item {
    message_id: string;
    name: string;
    size: number;
    mime: string | null;
    category: string | null;
    saved_at: string | null;
    time: string;
    at: number | null;
    mine: boolean;
  }
  let items = $state<Item[]>([]);
  let links = $state<{ url: string; time: string; at: number | null; text: string }[]>([]);
  let loading = $state(false);
  let scannedFor = "";

  $effect(() => {
    const id = peerId;
    if (id && id !== scannedFor) { scannedFor = id; void scan(id); }
  });

  const URL_RE = /(https?:\/\/[^\s]+)/g;

  async function scan(peerId: string) {
    loading = true;
    const its: Item[] = [];
    const lnks: typeof links = [];
    try {
      const msgs: ChatMessage[] = await api.getMessages(peerId);
      for (const m of msgs) {
        if (m.meta?.deleted) continue;
        if (m.kind.type === "file") {
          const c = m.kind.content;
          its.push({ message_id: m.id, name: c.name, size: c.size, mime: c.mime, category: c.category, saved_at: c.saved_at, time: m.time, at: m.at ?? null, mine: m.from !== peerId });
        } else if (m.kind.type === "text") {
          const text = m.kind.content.text;
          const found = text.match(URL_RE);
          if (found) for (const u of found) lnks.push({ url: u.replace(/[),.;!?]+$/, ""), time: m.time, at: m.at ?? null, text: text.replace(u, "").trim() });
        }
      }
    } catch (e) { console.warn("shared-media scan", e); }
    its.reverse(); lnks.reverse(); // newest first
    items = its; links = lnks;
    loading = false;
  }

  // ---- classification ----
  const low = (e: Item) => e.name.toLowerCase();
  const isGif = (e: Item) => e.mime === "image/gif" || low(e).endsWith(".gif") || e.category === "gif";
  const isImage = (e: Item) => (e.mime ?? "").startsWith("image/") || /\.(png|jpe?g|webp|gif|bmp|svg|heic)$/.test(low(e));
  const isVideo = (e: Item) => (e.mime ?? "").startsWith("video/") || /\.(mp4|webm|mov|mkv|avi|m4v)$/.test(low(e));
  const isVoice = (e: Item) => e.category === "voice" || e.category === "circle";
  const isAudio = (e: Item) => (e.mime ?? "").startsWith("audio/") || /\.(mp3|wav|ogg|flac|m4a|opus|aac)$/.test(low(e));

  const visual = (e: Item) => e.category !== "sticker" && !isVoice(e);
  let mediaItems = $derived(items.filter((e) => visual(e) && !isGif(e) && (isImage(e) || isVideo(e))));
  let gifItems = $derived(items.filter((e) => visual(e) && isGif(e)));
  let voiceItems = $derived(items.filter(isVoice));
  let musicItems = $derived(items.filter((e) => !isVoice(e) && isAudio(e)));
  let fileItems = $derived(items.filter((e) => e.category !== "sticker" && !isImage(e) && !isVideo(e) && !isAudio(e) && !isVoice(e)));

  const TABS: [Tab, string][] = [["media", "Media"], ["files", "Files"], ["links", "Links"], ["music", "Music"], ["voice", "Voice"], ["gifs", "GIFs"]];
  const count = (k: Tab) => k === "media" ? mediaItems.length : k === "files" ? fileItems.length : k === "links" ? links.length : k === "music" ? musicItems.length : k === "voice" ? voiceItems.length : gifItems.length;
  // Telegram shows only the tabs that have something (Media always).
  let tabs = $derived(TABS.filter(([k]) => k === "media" || count(k) > 0));
  $effect(() => { if (!loading && !tabs.some(([k]) => k === tab)) tab = "media"; });

  // ---- grid columns: pinch / Ctrl + wheel, remembered ----
  const COLS = [2, 3, 4, 5, 6, 9];
  const COLS_KEY = "aqua.sharedCols.v1";
  let cols = $state(3);
  try { const c = +(localStorage.getItem(COLS_KEY) ?? 0); if (COLS.includes(c)) cols = c; } catch { /* none */ }
  /** Live pinch scale while two fingers are down (the grid follows them). */
  let pinchScale = $state(1);
  let pinchFrom = 0;
  function setCols(dir: 1 | -1) {
    const i = COLS.indexOf(cols);
    const n = COLS[Math.max(0, Math.min(COLS.length - 1, i + dir))];
    if (n === cols) return;
    const apply = () => { cols = n; try { localStorage.setItem(COLS_KEY, String(n)); } catch { /* none */ } };
    const d = document as Document & { startViewTransition?: (f: () => void) => unknown };
    // The cells glide to their new places where the WebView can.
    if (d.startViewTransition && !matchMedia("(prefers-reduced-motion: reduce)").matches) d.startViewTransition(apply); else apply();
  }
  const dist = (e: TouchEvent) => Math.hypot(e.touches[0].clientX - e.touches[1].clientX, e.touches[0].clientY - e.touches[1].clientY);
  function gridTouchStart(e: TouchEvent) { if (e.touches.length === 2) { pinchFrom = dist(e); pinchScale = 1; } }
  function gridTouchMove(e: TouchEvent) {
    if (e.touches.length !== 2 || !pinchFrom) return;
    e.preventDefault();
    pinchScale = Math.max(0.6, Math.min(1.6, dist(e) / pinchFrom));
  }
  function gridTouchEnd(e: TouchEvent) {
    if (!pinchFrom || e.touches.length >= 2) return;
    const s = pinchScale;
    pinchFrom = 0; pinchScale = 1;
    // Fingers apart — bigger cells (fewer columns), together — more.
    if (s > 1.12) setCols(-1); else if (s < 0.88) setCols(1);
  }
  function gridWheel(e: WheelEvent) {
    if (!e.ctrlKey) return;
    e.preventDefault();
    setCols(e.deltaY < 0 ? -1 : 1);
  }
  function pinch(node: HTMLElement) {
    // Non-passive: two-finger moves must not zoom/scroll the page.
    node.addEventListener("touchmove", gridTouchMove, { passive: false });
    node.addEventListener("wheel", gridWheel, { passive: false });
    return { destroy() { node.removeEventListener("touchmove", gridTouchMove); node.removeEventListener("wheel", gridWheel); } };
  }

  // ---- opening ----
  let busy = $state("");
  async function ensure(e: Item): Promise<string | null> {
    if (e.saved_at) return e.saved_at;
    busy = e.message_id;
    try {
      const r = await api.downloadFile(peerId, e.message_id);
      e.saved_at = r.saved_path;
      items = [...items];
      return e.saved_at;
    } catch (err) { showToast(tr("Could not download: {e}", { e: String(err) })); return null; }
    finally { busy = ""; }
  }
  async function openVisual(list: Item[], e: Item) {
    if (!(await ensure(e))) return;
    const ready = list.filter((x) => x.saved_at);
    const gi: GalleryItem[] = ready.map((x) => ({ url: assetUrl(x.saved_at!), kind: isVideo(x) ? "video" : "image", name: x.name, path: x.saved_at, size: x.size, from: x.mine ? tr("You") : title, time: x.time }));
    openGallery(gi, Math.max(0, ready.indexOf(e)));
  }
  async function openFile(e: Item) {
    const p = await ensure(e);
    if (p) api.openSavedFile(p).catch((err) => showToast(String(err)));
  }
  async function playList(list: Item[], e: Item) {
    if (!(await ensure(e))) return;
    const ready = list.filter((x) => x.saved_at);
    playQueue(ready.map((x) => ({ src: assetUrl(x.saved_at!), title: isVoice(x) ? tr("Voice message") : x.name.replace(/\.[^.]+$/, ""), sub: x.mine ? tr("You") : title })), Math.max(0, ready.indexOf(e)));
  }

  // ---- looks ----
  function fmtSize(b: number): string {
    if (b < 1024) return `${b} B`;
    if (b < 1024 * 1024) return `${(b / 1024).toFixed(1)} KB`;
    return `${(b / 1024 / 1024).toFixed(1)} MB`;
  }
  function when(at: number | null, time: string): string {
    if (!at) return time;
    const d = new Date(at * 1000);
    return d.toLocaleDateString($lang, { day: "numeric", month: "short", year: d.getFullYear() === new Date().getFullYear() ? undefined : "numeric" }) + ", " + time;
  }
  function month(at: number | null): string {
    if (!at) return "";
    const d = new Date(at * 1000);
    const s = d.toLocaleDateString($lang, { month: "long", year: "numeric" });
    return s.charAt(0).toUpperCase() + s.slice(1);
  }
  /** Month groups for the grid (Telegram's floating month). */
  function byMonth(list: Item[]): { m: string; list: Item[] }[] {
    const out: { m: string; list: Item[] }[] = [];
    for (const e of list) {
      const m = month(e.at);
      const last = out[out.length - 1];
      if (last && last.m === m) last.list.push(e); else out.push({ m, list: [e] });
    }
    return out;
  }
  const EXT_COLORS: [RegExp, string][] = [
    [/^(pdf)$/, "#E5534B"], [/^(docx?|odt|rtf|txt|md)$/, "#3D8BFD"], [/^(xlsx?|ods|csv)$/, "#2EA44F"],
    [/^(pptx?|odp|key)$/, "#F08A24"], [/^(zip|rar|7z|tar|gz|xz)$/, "#8E6CEF"], [/^(apk|exe|msi|deb|rpm|dmg)$/, "#22B8CF"],
  ];
  const ext = (e: Item) => (e.name.includes(".") ? e.name.split(".").pop()! : "").toLowerCase().slice(0, 4);
  const extColor = (e: Item) => EXT_COLORS.find(([r]) => r.test(ext(e)))?.[1] ?? "var(--accent)";
  function host(u: string): string { try { return new URL(u).hostname.replace(/^www\./, ""); } catch { return u; } }
</script>

{#snippet grid(list: Item[])}
  <div class="sm-gridwrap" use:pinch ontouchstart={gridTouchStart} ontouchend={gridTouchEnd} ontouchcancel={gridTouchEnd}>
    {#each byMonth(list) as g (g.m)}
      {#if g.m && list.length > cols * 2}<div class="sm-month">{g.m}</div>{/if}
      <div class="sm-grid" style:--cols={cols} style:transform={pinchScale !== 1 ? `scale(${pinchScale})` : undefined}>
        {#each g.list as e (e.message_id)}
          {@const u = e.saved_at ? assetUrl(e.saved_at) : null}
          <button class="sm-cell" style:view-transition-name={"sm" + e.message_id.replace(/[^a-zA-Z0-9_-]/g, "")} onclick={() => void openVisual(list, e)} aria-label={e.name}>
            {#if u}
              {#if isVideo(e)}<video use:streamSrc={u} muted preload="metadata" playsinline></video>{:else}<img src={u} alt="" loading="lazy" decoding="async" />{/if}
            {:else}
              <span class="sm-ph">{#if busy === e.message_id}<span class="sm-spin"></span>{:else}<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 4v12m-5-5 5 5 5-5M5 20h14"/></svg>{/if}</span>
            {/if}
            {#if isVideo(e)}<span class="sm-badge"><svg viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg>{fmtSize(e.size)}</span>{/if}
            {#if isGif(e)}<span class="sm-badge">GIF</span>{/if}
          </button>
        {/each}
      </div>
    {/each}
  </div>
{/snippet}

{#snippet rowAudio(list: Item[], e: Item)}
  <button class="sm-row" onclick={() => void playList(list, e)}>
    <span class="sm-play" class:voice={isVoice(e)}>
      {#if busy === e.message_id}<span class="sm-spin"></span>{:else}<svg viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg>{/if}
    </span>
    <span class="sm-row-col">
      <span class="sm-row-name">{isVoice(e) ? (e.mine ? $t("You") : (title || $t("Voice message"))) : e.name.replace(/\.[^.]+$/, "")}</span>
      <span class="sm-row-sub">{isVoice(e) ? $t(e.category === "circle" ? "Video message" : "Voice message") + " · " : ""}{fmtSize(e.size)} · {when(e.at, e.time)}</span>
    </span>
  </button>
{/snippet}

<div class="sm-media">
  <div class="sm-tabs" role="tablist">
    {#each tabs as [k, label] (k)}
      <button class="sm-tab" role="tab" class:active={tab === k} aria-selected={tab === k} onclick={() => (tab = k)}>{$t(label)}</button>
    {/each}
  </div>

  <div class="sm-body">
    {#if loading}
      <div class="sm-status">{$t("Scanning…")}</div>
    {:else if tab === "media"}
      {#if mediaItems.length === 0}<div class="sm-status">{$t("No media yet.")}</div>
      {:else}{@render grid(mediaItems)}<div class="sm-hint">{$t("Pinch to change the size")}</div>{/if}
    {:else if tab === "gifs"}
      {@render grid(gifItems)}
    {:else if tab === "voice"}
      {#each voiceItems as e (e.message_id)}{@render rowAudio(voiceItems, e)}{/each}
    {:else if tab === "music"}
      {#each musicItems as e (e.message_id)}{@render rowAudio(musicItems, e)}{/each}
    {:else if tab === "files"}
      {#each fileItems as e (e.message_id)}
        <button class="sm-row" onclick={() => void openFile(e)}>
          <span class="sm-ext" style:background={extColor(e)}>{#if busy === e.message_id}<span class="sm-spin"></span>{:else}{ext(e) || "·"}{/if}</span>
          <span class="sm-row-col">
            <span class="sm-row-name" title={e.name}>{e.name}</span>
            <span class="sm-row-sub">{fmtSize(e.size)} · {when(e.at, e.time)}</span>
          </span>
        </button>
      {/each}
    {:else if tab === "links"}
      {#each links as l, i (l.url + i)}
        <a class="sm-row" href={l.url} target="_blank" rel="noopener noreferrer">
          <span class="sm-ext link">{host(l.url).charAt(0).toUpperCase()}</span>
          <span class="sm-row-col">
            <span class="sm-row-name">{host(l.url)}</span>
            {#if l.text}<span class="sm-row-txt">{l.text}</span>{/if}
            <span class="sm-row-url">{l.url}</span>
          </span>
        </a>
      {/each}
    {/if}
  </div>
</div>

<style>
  .sm-media { display: flex; flex-direction: column; min-height: 0; flex: 1; }
  /* Telegram: plain words, the chosen one underlined in the accent. */
  .sm-tabs { display: flex; gap: 2px; padding: 0 6px; border-bottom: 1px solid var(--border); overflow-x: auto; scrollbar-width: none; flex-shrink: 0; }
  .sm-tabs::-webkit-scrollbar { display: none; }
  .sm-tab { position: relative; flex: 0 0 auto; padding: 13px 12px 12px; background: transparent; border: none; color: var(--fg-muted); font: 600 14px var(--font-ui, inherit); cursor: pointer; white-space: nowrap; transition: color .15s; }
  .sm-tab:hover { color: var(--fg); }
  .sm-tab.active { color: var(--accent); }
  .sm-tab::after { content: ""; position: absolute; left: 8px; right: 8px; bottom: 0; height: 3px; border-radius: 3px 3px 0 0; background: var(--accent); transform: scaleX(0); transition: transform .2s cubic-bezier(.2,.9,.25,1); }
  .sm-tab.active::after { transform: scaleX(1); }
  .sm-body { flex: 1; overflow-y: auto; padding: 2px 0 8px; }
  .sm-status { padding: 40px 16px; text-align: center; color: var(--fg-dim); font-size: 13.5px; }
  .sm-hint { padding: 10px 16px 4px; text-align: center; color: var(--fg-dim); font-size: 11.5px; }
  :global(html:not(.phone)) .sm-hint { display: none; }

  .sm-gridwrap { touch-action: pan-y; }
  .sm-month { position: sticky; top: 4px; z-index: 2; width: fit-content; margin: 6px auto; padding: 3px 10px; border-radius: 999px; font-size: 12.5px; font-weight: 600;
    color: #fff; background: rgba(0, 0, 0, .45); backdrop-filter: blur(8px); }
  .sm-grid { display: grid; grid-template-columns: repeat(var(--cols, 3), minmax(0, 1fr)); gap: 2px; transform-origin: 50% 0; transition: transform .12s ease-out; }
  .sm-cell { position: relative; aspect-ratio: 1; border: none; padding: 0; overflow: hidden; background: var(--bg-input); cursor: pointer; }
  .sm-cell img, .sm-cell video { width: 100%; height: 100%; object-fit: cover; display: block; pointer-events: none; }
  .sm-cell:active img, .sm-cell:active video { filter: brightness(.85); }
  .sm-ph { width: 100%; height: 100%; display: grid; place-items: center; color: var(--fg-dim); }
  .sm-ph svg { width: 22px; height: 22px; }
  .sm-badge { position: absolute; left: 4px; bottom: 4px; display: flex; align-items: center; gap: 2px; padding: 1px 6px 1px 4px; border-radius: 999px;
    background: rgba(0, 0, 0, .55); color: #fff; font-size: 11px; font-weight: 600; }
  .sm-badge svg { width: 11px; height: 11px; }

  .sm-row { display: flex; align-items: center; gap: 12px; width: 100%; box-sizing: border-box; padding: 8px 14px; background: transparent; border: none; color: inherit; text-align: left; font: inherit; cursor: pointer; text-decoration: none; }
  .sm-row:hover { background: var(--bg-hover); }
  .sm-play { width: 44px; height: 44px; flex-shrink: 0; border-radius: 50%; display: grid; place-items: center; background: var(--accent); color: var(--accent-fg, #fff); }
  .sm-play svg { width: 20px; height: 20px; margin-left: 2px; }
  .sm-ext { width: 44px; height: 44px; flex-shrink: 0; border-radius: calc(10px * var(--rk, 1)); display: grid; place-items: center; color: #fff;
    font-size: 12px; font-weight: 800; text-transform: uppercase; letter-spacing: .02em; }
  .sm-ext.link { background: color-mix(in srgb, var(--accent) 22%, transparent); color: var(--accent); font-size: 18px; }
  .sm-row-col { min-width: 0; flex: 1; display: flex; flex-direction: column; gap: 2px; padding: 2px 0; border-bottom: 1px solid var(--border); padding-bottom: 8px; }
  .sm-row-name { font-size: 14.5px; font-weight: 600; color: var(--fg); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sm-row-sub { font-size: 12.5px; color: var(--fg-dim); }
  .sm-row-txt { font-size: 13px; color: var(--fg-muted); overflow: hidden; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; }
  .sm-row-url { font-size: 12.5px; color: var(--accent); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sm-spin { width: 18px; height: 18px; border-radius: 50%; border: 2px solid currentColor; border-right-color: transparent; animation: sm-spin .8s linear infinite; }
  @keyframes sm-spin { to { transform: rotate(360deg); } }
</style>
