<script module lang="ts">
  import { writable } from "svelte/store";
  export type SendMode = "media" | "compressed" | "file";
  export interface SendChoice { mode: SendMode; caption: string; edit: boolean; /** Send as a live photo (1.2.0-4). */ live?: import("../liveMotion").LiveStyle }
  interface Ask { paths: string[]; preset?: SendMode; resolve: (v: SendChoice | null) => void }
  const pending = writable<Ask | null>(null);
  const LAST = "aqua.sendMode.v1";
  /**
   * Telegram-style "send files" sheet: previews, and how to send —
   * as photo / video (compressed or original) or as a file (untouched,
   * no preview on either side). Resolves null when cancelled.
   */
  export function askSend(paths: string[], preset?: SendMode): Promise<SendChoice | null> {
    return new Promise((resolve) => pending.set({ paths, preset, resolve }));
  }
  export const IMAGE_RE = /\.(png|jpe?g|webp|bmp|gif|avif|heic)$/i;
  export const VIDEO_RE = /\.(mp4|m4v|webm|mov|mkv|avi|ogv)$/i;
</script>

<script lang="ts">
  import { t, tr } from "../i18n";
  import { api, assetUrl } from "../tauri";
  import { fmtSize } from "../media";
  import { LIVE_STYLES, type LiveStyle } from "../liveMotion";

  const nameOf = (p: string) => p.split(/[\\/]/).pop() ?? p;
  let mode = $state<SendMode>("media");
  let compressVideo = $state(true);
  let caption = $state("");
  /** Live photo: off, or how it plays. Offered for one short clip or a Motion Photo. */
  let live = $state<LiveStyle | null>(null);
  let canLive = $state(false);
  let thumbs = $state<Record<string, string>>({});
  let sizes = $state<Record<string, number>>({});

  let paths = $derived($pending?.paths ?? []);
  /** Phone picks (content://…) have no extension: their type is sniffed (picked_preview). */
  let kinds = $state<Record<string, string>>({});
  const isImg = (p: string) => IMAGE_RE.test(p) || kinds[p] === "image";
  const isVid = (p: string) => VIDEO_RE.test(p) || kinds[p] === "video";
  let hasImages = $derived(paths.some(isImg));
  let hasVideos = $derived(paths.some(isVid));
  let total = $derived(Object.values(sizes).reduce((a, b) => a + b, 0));

  $effect(() => {
    const list = paths;
    if (!list.length) return;
    try {
      const last = JSON.parse(localStorage.getItem(LAST) ?? "null");
      if (last) { mode = last.mode === "file" ? "file" : "media"; compressVideo = last.compressVideo !== false; }
    } catch { /* defaults */ }
    if ($pending?.preset) mode = $pending.preset === "file" ? "file" : "media";
    caption = "";
    live = null;
    canLive = false;
    if (list.length === 1 && VIDEO_RE.test(list[0])) {
      void api.probeMediaFile(list[0], "video").then((r) => { canLive = (r?.duration ?? 0) <= 15; }).catch(() => {});
    } else if (list.length === 1 && /\.(jpe?g|heic)$/i.test(list[0])) {
      void api.hasMotionClip(list[0]).then((y) => { canLive = y; }).catch(() => {});
    }
    thumbs = {};
    sizes = {};
    kinds = {};
    for (const p of list) {
      if (/^(content|file):\/\//.test(p)) {
        void api.pickedPreview(p).then((r) => {
          kinds = { ...kinds, [p]: r.kind };
          if (r.url) thumbs = { ...thumbs, [p]: r.url };
        }).catch(() => {});
        continue;
      }
      if (IMAGE_RE.test(p)) {
        void api.allowMediaPreview(p).then(() => { thumbs = { ...thumbs, [p]: assetUrl(p) }; }).catch(() => {});
      }
      void api.probeMediaFile(p, VIDEO_RE.test(p) ? "video" : "size").then((r) => {
        sizes = { ...sizes, [p]: r?.size ?? 0 };
        if (r?.poster) thumbs = { ...thumbs, [p]: r.poster };
      }).catch(() => {});
    }
  });

  function done(send: boolean, edit = false) {
    const a = $pending;
    if (!a) return;
    pending.set(null);
    if (!send) { a.resolve(null); return; }
    try { localStorage.setItem(LAST, JSON.stringify({ mode, compressVideo })); } catch { /* ignore */ }
    const m: SendMode = mode === "file" ? "file" : hasVideos && compressVideo ? "compressed" : "media";
    a.resolve({ mode: m, caption: caption.trim(), edit, live: canLive && live && mode !== "file" ? live : undefined });
  }
  function key(e: KeyboardEvent) {
    if (!$pending) return;
    if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); done(false); }
    else if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); e.stopPropagation(); done(true); }
  }
</script>

<svelte:window onkeydowncapture={key} />

{#if $pending}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="sd-back" onclick={() => done(false)}>
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div class="sd" role="dialog" aria-modal="true" tabindex="-1" aria-label={$t("Send files")} onclick={(e) => e.stopPropagation()}>
      <header>
        <h3>{paths.length === 1 ? $t("Send a file") : tr("Send {n} files", { n: paths.length })}</h3>
        <span class="sub">{fmtSize(total)}</span>
      </header>

      <div class="grid" class:one={paths.length === 1}>
        {#each paths as p (p)}
          <div class="it" class:file={mode === "file"}>
            {#if mode !== "file" && thumbs[p]}
              <img src={thumbs[p]} alt="" />
              {#if isVid(p)}<span class="vmark">▶</span>{/if}
            {:else}
              <span class="ficon">{isVid(p) ? "🎬" : isImg(p) ? "🖼" : "📄"}</span>
            {/if}
            <span class="nm" title={nameOf(p)}>{nameOf(p)}</span>
            {#if sizes[p]}<span class="sz">{fmtSize(sizes[p])}</span>{/if}
          </div>
        {/each}
      </div>

      {#if hasImages || hasVideos}
        <div class="modes" role="radiogroup" aria-label={$t("Send as")}>
          <button role="radio" aria-checked={mode === "media"} class:on={mode === "media"} onclick={() => (mode = "media")}>
            <b>{$t(hasVideos && !hasImages ? "As video" : hasImages && !hasVideos ? "As photo" : "As photo / video")}</b>
            <span>{$t("Shown in the chat, plays inline")}</span>
          </button>
          <button role="radio" aria-checked={mode === "file"} class:on={mode === "file"} onclick={() => (mode = "file")}>
            <b>{$t("As file")}</b>
            <span>{$t("Original quality, no preview")}</span>
          </button>
        </div>
        {#if mode === "media" && hasVideos}
          <label class="chk">
            <input type="checkbox" bind:checked={compressVideo} />
            <span>{$t("Compress video (720p, much smaller; needs ffmpeg)")}</span>
          </label>
        {/if}
      {/if}

      {#if canLive && mode !== "file"}
        <div class="live-row">
          <span class="live-lbl">◎ {$t("Live photo")}</span>
          <button class:on={live === null} onclick={() => (live = null)}>{$t("Off")}</button>
          {#each LIVE_STYLES as st (st.id)}
            <button class:on={live === st.id} title={$t(st.label)} onclick={() => (live = st.id)}>{$t(st.label.split(" — ")[0])}</button>
          {/each}
        </div>
      {/if}

      <input class="cap" placeholder={$t("Add a caption…")} bind:value={caption} />

      <div class="row">
        {#if paths.length === 1 && IMAGE_RE.test(paths[0]) && mode === "media"}
          <button class="btn ghost" onclick={() => done(true, true)}>{$t("Edit")}</button>
        {/if}
        <span class="grow"></span>
        <button class="btn" onclick={() => done(false)}>{$t("Cancel")}</button>
        <button class="btn ok" onclick={() => done(true)}>{$t("Send")}</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .live-row { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; font-size: 12.5px; }
  .live-lbl { font-weight: 700; color: var(--fg-muted); margin-right: 4px; }
  .live-row button { border: 1px solid var(--border); background: var(--bg-input); color: var(--fg); padding: 5px 10px; border-radius: calc(9px * var(--rk, 1)); font: inherit; cursor: pointer; }
  .live-row button.on { border-color: var(--accent); color: var(--accent); background: var(--accent-soft); }
  .sd-back { position: fixed; inset: 0; z-index: 10090; display: grid; place-items: center; background: rgba(0, 0, 0, 0.55); }
  /* Phones: a sheet from the bottom, like Telegram — not a box in the middle of the chat. */
  :global(html.phone) .sd-back { place-items: end stretch; }
  :global(html.phone) .sd { width: 100%; max-height: 92vh; border-radius: calc(18px * var(--rk, 1)) calc(18px * var(--rk, 1)) 0 0;
    padding-bottom: calc(18px + env(safe-area-inset-bottom, 0px)); animation: sd-up .22s cubic-bezier(.2, .9, .25, 1); }
  @keyframes sd-up { from { transform: translateY(40px); opacity: 0; } }
  .sd {
    width: min(460px, 94vw); max-height: 88vh;
    display: flex; flex-direction: column; gap: 12px;
    padding: 18px;
    border-radius: calc(16px * var(--rk, 1));
    background: var(--bg-shell);
    border: 1px solid var(--border-strong);
    box-shadow: 0 28px 70px rgba(0, 0, 0, 0.55);
  }
  header { display: flex; align-items: baseline; gap: 10px; }
  h3 { margin: 0; font-size: 16px; font-weight: 700; }
  .sub { font-size: 12px; color: var(--fg-dim); }
  .grid {
    display: grid; grid-template-columns: repeat(3, 1fr); gap: 6px;
    overflow-y: auto; max-height: 44vh;
  }
  .grid.one { grid-template-columns: 1fr; }
  .it {
    position: relative; aspect-ratio: 1; border-radius: calc(10px * var(--rk, 1)); overflow: hidden;
    background: var(--bg-input); display: grid; place-items: center;
  }
  .grid.one .it { aspect-ratio: 16 / 10; }
  .it img { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: cover; }
  .it.file { aspect-ratio: auto; display: flex; align-items: center; gap: 10px; padding: 10px; place-items: initial; }
  .grid:has(.file) { grid-template-columns: 1fr; }
  .ficon { font-size: 28px; }
  .it:not(.file) .ficon { font-size: 34px; }
  .vmark {
    position: absolute; width: 38px; height: 38px; border-radius: 50%;
    display: grid; place-items: center; background: rgba(0, 0, 0, 0.55); color: #fff; font-size: 14px;
  }
  .nm {
    font-size: 12.5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }
  .it:not(.file) .nm, .it:not(.file) .sz { display: none; }
  .it.file .nm { flex: 1; min-width: 0; }
  .sz { font-size: 11.5px; color: var(--fg-dim); }
  .modes { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; }
  .modes button {
    display: flex; flex-direction: column; align-items: flex-start; gap: 2px;
    padding: 10px 12px; border-radius: calc(11px * var(--rk, 1));
    border: 1.5px solid var(--border); background: var(--bg-input); color: var(--fg);
    text-align: left; cursor: pointer;
  }
  .modes button b { font-size: 13.5px; }
  .modes button span { font-size: 11.5px; color: var(--fg-dim); }
  .modes button.on { border-color: var(--accent); background: var(--accent-soft); }
  .chk { display: flex; align-items: center; gap: 8px; font-size: 13px; color: var(--fg-muted); cursor: pointer; }
  .chk input { accent-color: var(--accent); width: 16px; height: 16px; }
  .cap {
    height: 38px; padding: 0 12px; border-radius: calc(10px * var(--rk, 1));
    border: 1px solid var(--border); background: var(--bg-input); color: var(--fg); font: inherit; font-size: 14px;
  }
  .cap:focus { outline: none; border-color: var(--accent); }
  .row { display: flex; align-items: center; gap: 8px; }
  .grow { flex: 1; }
  .btn {
    height: 36px; padding: 0 16px; border-radius: calc(10px * var(--rk, 1));
    border: 1px solid var(--border-strong); background: transparent; color: var(--fg);
    font-size: 13.5px; font-weight: 600; cursor: pointer;
  }
  .btn:hover { background: var(--bg-hover); }
  .btn.ok { background: var(--accent); border-color: transparent; color: #06070c; }
  .btn.ghost { border-color: transparent; color: var(--accent); }
</style>
