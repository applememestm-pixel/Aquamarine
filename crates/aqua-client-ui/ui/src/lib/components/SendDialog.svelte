<script module lang="ts">
  import { writable } from "svelte/store";
  export type SendMode = "media" | "compressed" | "file";
  export interface SendChoice { mode: SendMode; caption: string; edit: boolean; /** The files, with any added in the sheet ("+"). */ paths: string[]; /** Send as a live photo (1.2.0-4). */ live?: import("../liveMotion").LiveStyle; /** View once (no saving, no forwarding, gone after one look). */ once?: boolean }
  interface Ask { paths: string[]; preset?: SendMode; caption?: string; resolve: (v: SendChoice | null) => void }
  const pending = writable<Ask | null>(null);
  const LAST = "aqua.sendMode.v1";
  /**
   * Telegram-style "send files" sheet: big previews (✕ takes one out, ✎
   * edits a photo), a "compress" switch (off: as files, untouched), and a
   * caption field with the send button like the chat's own. The text
   * already typed in the chat comes along as the caption. Resolves null
   * when cancelled.
   */
  export function askSend(paths: string[], preset?: SendMode, caption?: string): Promise<SendChoice | null> {
    return new Promise((resolve) => pending.set({ paths, preset, caption, resolve }));
  }
  export const IMAGE_RE = /\.(png|jpe?g|webp|bmp|gif|avif|heic)$/i;
  export const VIDEO_RE = /\.(mp4|m4v|webm|mov|mkv|avi|ogv)$/i;
</script>

<script lang="ts">
  import { untrack } from "svelte";
  import { t, tr } from "../i18n";
  import { api, assetUrl } from "../tauri";
  import { fmtSize } from "../media";
  import { icons } from "../icons";
  import { IS_MOBILE } from "../platform";
  import { LIVE_STYLES, type LiveStyle } from "../liveMotion";

  const nameOf = (p: string) => p.split(/[\\/]/).pop() ?? p;
  let mode = $state<SendMode>("media");
  let compressVideo = $state(true);
  let caption = $state("");
  /** Live photo: off, or how it plays. Offered for one short clip or a Motion Photo. */
  let live = $state<LiveStyle | null>(null);
  let canLive = $state(false);
  let once = $state(false);
  let thumbs = $state<Record<string, string>>({});
  let sizes = $state<Record<string, number>>({});
  let capEl = $state<HTMLTextAreaElement | null>(null);

  /** Files added with "+" in the sheet, on top of the ones it opened with. */
  let extra = $state<string[]>([]);
  /** Taken out with ✕. */
  let removed = $state<string[]>([]);
  let paths = $derived([...($pending?.paths ?? []), ...extra].filter((p) => !removed.includes(p)));
  $effect(() => { void $pending; extra = []; removed = []; });
  async function addMore() {
    try {
      const { openFileDialog } = await import("../dialog");
      const r = await openFileDialog({ multiple: true });
      const list = (Array.isArray(r) ? r : r ? [r] : []).map(String).filter((p) => !paths.includes(p));
      if (list.length) { removed = removed.filter((p) => !list.includes(p)); extra = [...extra, ...list.filter((p) => !extra.includes(p))]; }
    } catch { /* cancelled */ }
  }
  function drop(p: string) {
    removed = [...removed, p];
    if (!paths.length) done(false);
  }
  const AUDIO_RE = /\.(mp3|wav|ogg|oga|flac|m4a|opus|aac|wma)$/i;
  /** Phone picks (content://…) have no extension: their type is sniffed (picked_preview). */
  let kinds = $state<Record<string, string>>({});
  const isImg = (p: string) => IMAGE_RE.test(p) || kinds[p] === "image";
  const isVid = (p: string) => VIDEO_RE.test(p) || kinds[p] === "video";
  let hasImages = $derived(paths.some(isImg));
  let hasVideos = $derived(paths.some(isVid));
  let total = $derived(paths.reduce((a, p) => a + (sizes[p] ?? 0), 0));
  /** View once: one photo or video, sent as media. */
  let canOnce = $derived(paths.length === 1 && (isImg(paths[0]) || isVid(paths[0])) && mode !== "file");
  let allMedia = $derived(paths.length > 0 && paths.every((p) => isImg(p) || isVid(p) || kinds[p] === ""));

  // New files: settings, caption and previews once per opened sheet.
  $effect(() => {
    const a = $pending;
    if (!a) return;
    try {
      const last = JSON.parse(localStorage.getItem(LAST) ?? "null");
      if (last) { mode = last.mode === "file" ? "file" : "media"; compressVideo = last.compressVideo !== false; }
    } catch { /* defaults */ }
    if (a.preset) mode = a.preset === "file" ? "file" : "media";
    caption = a.caption ?? "";
    once = false;
    live = null;
    canLive = false;
    thumbs = {};
    sizes = {};
    kinds = {};
    if (!IS_MOBILE) setTimeout(() => { capEl?.focus(); const n = capEl?.value.length ?? 0; capEl?.setSelectionRange(n, n); grow(); }, 30);
    else setTimeout(grow, 30);
  });
  $effect(() => {
    const list = paths;
    if (!$pending || !list.length) return;
    untrack(() => load(list));
  });
  function load(list: string[]) {
    canLive = false;
    if (list.length === 1 && VIDEO_RE.test(list[0])) {
      void api.probeMediaFile(list[0], "video").then((r) => { canLive = (r?.duration ?? 0) <= 15; }).catch(() => {});
    } else if (list.length === 1 && /\.(jpe?g|heic)$/i.test(list[0])) {
      void api.hasMotionClip(list[0]).then((y) => { canLive = y; }).catch(() => {});
    }
    for (const p of list) {
      if (p in sizes || p in kinds || p in thumbs) continue;
      if (/^(content|file):\/\//.test(p)) {
        kinds = { ...kinds, [p]: "" };
        void api.pickedPreview(p).then((r) => {
          kinds = { ...kinds, [p]: r.kind };
          if (r.url) thumbs = { ...thumbs, [p]: r.url };
        }).catch(() => {});
        continue;
      }
      sizes = { ...sizes, [p]: 0 };
      if (IMAGE_RE.test(p)) {
        void api.allowMediaPreview(p).then(() => { thumbs = { ...thumbs, [p]: assetUrl(p) }; }).catch(() => {});
      }
      void api.probeMediaFile(p, VIDEO_RE.test(p) ? "video" : "size").then((r) => {
        sizes = { ...sizes, [p]: r?.size ?? 0 };
        if (r?.poster) thumbs = { ...thumbs, [p]: r.poster };
      }).catch(() => {});
    }
  }

  function grow() {
    const el = capEl;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 140)}px`;
  }

  function done(send: boolean, edit = false) {
    const a = $pending;
    if (!a) return;
    // `paths` (and what hangs on it) is derived from `$pending`: read it all
    // before closing, or the list is already empty and the send turns into a
    // silent cancel (the sheet closed, nothing went out).
    const list = [...paths];
    const m: SendMode = mode === "file" ? "file" : hasVideos && compressVideo ? "compressed" : "media";
    const one = once && canOnce;
    const lv = !one && canLive && live && mode !== "file" ? live : undefined;
    pending.set(null);
    if (!send || !list.length) { a.resolve(null); return; }
    try { localStorage.setItem(LAST, JSON.stringify({ mode, compressVideo })); } catch { /* ignore */ }
    a.resolve({ mode: m, caption: caption.trim(), edit: edit && !one, paths: list, live: lv, once: one || undefined });
  }
  function key(e: KeyboardEvent) {
    if (!$pending) return;
    if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); done(false); }
    else if (e.key === "Enter" && !e.shiftKey && !e.isComposing && !IS_MOBILE) { e.preventDefault(); e.stopPropagation(); done(true); }
  }
  const asMedia = () => $t(hasVideos && !hasImages ? "As video" : hasImages && !hasVideos ? "As photo" : "As photo / video");
</script>

<svelte:window onkeydowncapture={key} />

{#if $pending}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="sd-back" onclick={() => done(false)}>
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div class="sd" role="dialog" aria-modal="true" tabindex="-1" aria-label={$t("Send files")} onclick={(e) => e.stopPropagation()} onkeydown={key}>
      <header>
        <button class="x" data-x data-keep-x onclick={() => done(false)} aria-label={$t("Close")}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M6 6l12 12M18 6 6 18"/></svg>
        </button>
        <h3>{paths.length === 1 ? $t(mode !== "file" && isImg(paths[0]) ? "Send a photo" : mode !== "file" && isVid(paths[0]) ? "Send a video" : "Send a file") : tr("Send {n} files", { n: paths.length })}</h3>
        {#if total}<span class="sub">{fmtSize(total)}</span>{/if}
      </header>

      {#if mode !== "file" && allMedia}
        <div class="grid" class:one={paths.length === 1} class:two={paths.length === 2 || paths.length === 4}>
          {#each paths as p (p)}
            <div class="it">
              {#if thumbs[p]}
                <img src={thumbs[p]} alt="" />
              {:else}
                <span class="ficon">{isVid(p) ? "🎬" : "🖼"}</span>
              {/if}
              {#if isVid(p)}<span class="vmark"><svg viewBox="0 0 24 24" fill="currentColor"><path d="M8 5.5v13l11-6.5z"/></svg></span>{/if}
              <div class="tools">
                {#if paths.length === 1 && IMAGE_RE.test(p)}
                  <button onclick={() => done(true, true)} title={$t("Edit")} aria-label={$t("Edit")}>
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 20h9"/><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4Z"/></svg>
                  </button>
                {/if}
                {#if paths.length > 1}
                  <button onclick={() => drop(p)} title={$t("Remove")} aria-label={$t("Remove")}>
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M6 6l12 12M18 6 6 18"/></svg>
                  </button>
                {/if}
              </div>
            </div>
          {/each}
        </div>
      {:else}
        <div class="files">
          {#each paths as p (p)}
            <div class="fr">
              <span class="fthumb">
                {#if thumbs[p] && (isImg(p) || isVid(p))}<img src={thumbs[p]} alt="" />{:else}{isVid(p) ? "🎬" : isImg(p) ? "🖼" : AUDIO_RE.test(p) ? "🎵" : "📄"}{/if}
              </span>
              <span class="fmeta"><b title={nameOf(p)}>{nameOf(p)}</b>{#if sizes[p]}<small>{fmtSize(sizes[p])}</small>{/if}</span>
              {#if paths.length > 1}
                <button class="fx" onclick={() => drop(p)} title={$t("Remove")} aria-label={$t("Remove")}>
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M6 6l12 12M18 6 6 18"/></svg>
                </button>
              {/if}
            </div>
          {/each}
        </div>
      {/if}

      {#if hasImages || hasVideos}
        <div class="opts">
          <label class="sw">
            <input type="checkbox" checked={mode !== "file"} onchange={(e) => (mode = (e.currentTarget as HTMLInputElement).checked ? "media" : "file")} />
            <span class="knob"></span>
            <span class="lbl"><b>{asMedia()}</b><small>{$t(mode === "file" ? "Off — as files, original quality, no preview" : "Shown in the chat, plays inline")}</small></span>
          </label>
          {#if canOnce}
            <label class="sw">
              <input type="checkbox" bind:checked={once} />
              <span class="knob"></span>
              <span class="lbl"><b>① {$t("View once")}</b><small>{$t("Opens one time; no saving, forwarding or screenshots")}</small></span>
            </label>
          {/if}
          {#if mode === "media" && hasVideos && !once}
            <label class="sw">
              <input type="checkbox" bind:checked={compressVideo} />
              <span class="knob"></span>
              <span class="lbl"><b>{$t("Compress video")}</b><small>{$t("720p, much smaller; needs ffmpeg")}</small></span>
            </label>
          {/if}
        </div>
      {/if}

      {#if canLive && mode !== "file" && !once}
        <div class="live-row">
          <span class="live-lbl">◎ {$t("Live photo")}</span>
          <button class:on={live === null} onclick={() => (live = null)}>{$t("Off")}</button>
          {#each LIVE_STYLES as st (st.id)}
            <button class:on={live === st.id} title={$t(st.label)} onclick={() => (live = st.id)}>{$t(st.label.split(" — ")[0])}</button>
          {/each}
        </div>
      {/if}

      <div class="capbar">
        <button class="add" onclick={addMore} title={$t("Add files")} aria-label={$t("Add files")}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg>
        </button>
        <!-- Enter sends right from the caption too: the window-level handler
             alone didn't always get the key (the dialog stayed open). -->
        <textarea bind:this={capEl} class="cap" rows="1" placeholder={$t("Add a caption…")} bind:value={caption} oninput={grow} onkeydown={key}></textarea>
        <button class="go" onclick={() => done(true)} title={$t("Send")} aria-label={$t("Send")}>
          {@html icons.send}
          {#if paths.length > 1}<span class="cnt">{paths.length}</span>{/if}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .live-row { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; font-size: 12.5px; padding: 0 2px; }
  .live-lbl { font-weight: 700; color: var(--fg-muted); margin-right: 4px; }
  .live-row button { border: 1px solid var(--border); background: var(--bg-input); color: var(--fg); padding: 5px 10px; border-radius: calc(9px * var(--rk, 1)); font: inherit; cursor: pointer; }
  .live-row button.on { border-color: var(--accent); color: var(--accent); background: var(--accent-soft); }
  .sd-back { position: fixed; inset: 0; z-index: 10090; display: grid; place-items: center; background: rgba(0, 0, 0, 0.55); animation: sd-fade .16s ease; }
  @keyframes sd-fade { from { opacity: 0; } }
  .sd {
    width: min(440px, 94vw); max-height: 90vh;
    display: flex; flex-direction: column; gap: 10px;
    padding: 12px;
    border-radius: calc(16px * var(--rk, 1));
    background: var(--bg-shell);
    border: 1px solid var(--border-strong);
    box-shadow: 0 28px 70px rgba(0, 0, 0, 0.55);
    animation: sd-pop .2s cubic-bezier(.2, .9, .25, 1.1);
  }
  @keyframes sd-pop { from { transform: scale(.96); opacity: 0; } }
  /* Phones: a sheet from the bottom, like Telegram — not a box in the middle of the chat. */
  :global(html.phone) .sd-back { place-items: end stretch; }
  :global(html.phone) .sd { width: 100%; box-sizing: border-box; max-height: 94dvh; border-radius: calc(18px * var(--rk, 1)) calc(18px * var(--rk, 1)) 0 0; border-bottom: none;
    padding-bottom: calc(10px + env(safe-area-inset-bottom, 0px)); animation: sd-up .22s cubic-bezier(.2, .9, .25, 1); }
  @keyframes sd-up { from { transform: translateY(40px); opacity: 0; } }
  header { display: flex; align-items: center; gap: 8px; min-height: 34px; }
  .x { width: 34px; height: 34px; flex-shrink: 0; border: none; border-radius: 50%; background: transparent; color: var(--fg-muted); display: grid; place-items: center; cursor: pointer; }
  .x:hover { background: var(--bg-hover); color: var(--fg); }
  .x svg { width: 18px; height: 18px; }
  h3 { margin: 0; font-size: 16px; font-weight: 700; flex: 1; min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .sub { font-size: 12px; color: var(--fg-dim); font-variant-numeric: tabular-nums; padding-right: 6px; }

  .grid { display: grid; grid-template-columns: repeat(3, 1fr); gap: 3px; overflow-y: auto; max-height: 56vh; border-radius: calc(12px * var(--rk, 1)); flex-shrink: 1; min-height: 0; }
  .grid.two { grid-template-columns: repeat(2, 1fr); }
  .grid.one { grid-template-columns: 1fr; }
  .it { position: relative; aspect-ratio: 1; overflow: hidden; background: var(--bg-input); display: grid; place-items: center; }
  .it img { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: cover; }
  /* One photo: the whole picture, as big as fits (not a cropped strip). */
  .grid.one .it { aspect-ratio: auto; min-height: 160px; max-height: 56vh; }
  .grid.one .it img { position: static; display: block; width: 100%; height: auto; max-height: 56vh; object-fit: contain; background: #000; }
  .ficon { font-size: 34px; }
  .vmark { position: absolute; left: 50%; top: 50%; transform: translate(-50%, -50%); width: 44px; height: 44px; border-radius: 50%;
    display: grid; place-items: center; background: rgba(0, 0, 0, 0.5); color: #fff; pointer-events: none; }
  .vmark svg { width: 20px; height: 20px; margin-left: 2px; }
  .tools { position: absolute; top: 6px; right: 6px; display: flex; gap: 5px; }
  .tools button { width: 30px; height: 30px; border-radius: 50%; border: none; display: grid; place-items: center; cursor: pointer;
    color: #fff; background: rgba(0, 0, 0, 0.5); backdrop-filter: blur(6px); -webkit-backdrop-filter: blur(6px); transition: background .15s, transform .15s; }
  .tools button:hover { background: rgba(0, 0, 0, 0.72); transform: scale(1.06); }
  .tools svg { width: 16px; height: 16px; }

  .files { display: flex; flex-direction: column; gap: 2px; overflow-y: auto; max-height: 50vh; }
  .fr { display: flex; align-items: center; gap: 12px; padding: 6px 4px 6px 6px; border-radius: calc(10px * var(--rk, 1)); }
  .fr:hover { background: var(--bg-hover); }
  .fthumb { width: 46px; height: 46px; flex-shrink: 0; border-radius: calc(10px * var(--rk, 1)); overflow: hidden; display: grid; place-items: center; font-size: 22px;
    background: var(--accent-soft); }
  .fthumb img { width: 100%; height: 100%; object-fit: cover; }
  .fmeta { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 2px; }
  .fmeta b { font-size: 14px; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .fmeta small { font-size: 12px; color: var(--fg-dim); }
  .fx { width: 30px; height: 30px; border: none; border-radius: 50%; background: transparent; color: var(--fg-dim); display: grid; place-items: center; cursor: pointer; }
  .fx:hover { background: var(--bg-hover); color: var(--fg); }
  .fx svg { width: 15px; height: 15px; }

  .opts { display: flex; flex-direction: column; gap: 2px; }
  .sw { display: flex; align-items: center; gap: 12px; padding: 7px 6px; border-radius: calc(10px * var(--rk, 1)); cursor: pointer; }
  .sw:hover { background: var(--bg-hover); }
  .sw input { position: absolute; opacity: 0; pointer-events: none; }
  .knob { position: relative; width: 36px; height: 20px; flex-shrink: 0; border-radius: 10px; background: var(--border-strong); transition: background .18s; }
  .knob::after { content: ""; position: absolute; top: 2px; left: 2px; width: 16px; height: 16px; border-radius: 50%; background: #fff; transition: transform .18s cubic-bezier(.3,.9,.3,1.2); box-shadow: 0 1px 3px rgba(0,0,0,.35); }
  .sw input:checked + .knob { background: var(--accent); }
  .sw input:checked + .knob::after { transform: translateX(16px); }
  .sw input:focus-visible + .knob { box-shadow: 0 0 0 3px var(--accent-soft); }
  .lbl { display: flex; flex-direction: column; gap: 1px; min-width: 0; }
  .lbl b { font-size: 13.5px; font-weight: 600; }
  .lbl small { font-size: 11.5px; color: var(--fg-dim); }

  /* The caption: the chat's own message field, with its send button. */
  .capbar { display: flex; align-items: flex-end; gap: 6px; }
  .add { width: 40px; height: 40px; flex-shrink: 0; border: none; border-radius: 50%; background: transparent; color: var(--fg-muted); display: grid; place-items: center; cursor: pointer; transition: color .15s, background .15s; }
  .add:hover { color: var(--accent); background: var(--accent-soft); }
  .add svg { width: 22px; height: 22px; }
  .cap {
    flex: 1; min-width: 0; min-height: 40px; max-height: 140px; box-sizing: border-box; resize: none;
    padding: 10px 14px; line-height: 1.35; border-radius: calc(20px * var(--rk, 1));
    border: 1px solid var(--border); background: var(--bg-input); color: var(--fg); font: inherit; font-size: 14.5px;
    transition: border-color .15s, box-shadow .15s;
  }
  .cap:focus { outline: none; border-color: var(--accent-soft); box-shadow: 0 0 0 3px var(--accent-soft); }
  .go { position: relative; width: 40px; height: 40px; flex-shrink: 0; border: none; border-radius: 50%; display: grid; place-items: center; cursor: pointer;
    color: #06070c; background: var(--accent); box-shadow: 0 6px 16px -8px var(--accent-glow, var(--accent)); transition: transform .15s ease, filter .15s ease; }
  .go:hover { filter: brightness(1.08); transform: translateX(1px); }
  .go:active { transform: scale(.92); }
  .go :global(svg) { width: 20px; height: 20px; margin-left: 2px; }
  .cnt { position: absolute; top: -4px; right: -4px; min-width: 18px; height: 18px; padding: 0 5px; box-sizing: border-box; border-radius: 9px;
    background: var(--bg-shell); color: var(--accent); border: 1.5px solid var(--accent); font: 800 11px/15px var(--font-ui, inherit); text-align: center; }
</style>
