<script module lang="ts">
  import type { LinkPreview as Preview } from "../tauri";
  // One request per link for the whole session, however many bubbles show it.
  const inflight = new Map<string, Promise<Preview | null>>();
  // Videos already downloaded (url → local path).
  const videoPaths = new Map<string, string>();
</script>

<script lang="ts">
  /**
   * Telegram-style preview under a message with a link: site, title,
   * description and picture; YouTube with an inline player; links to video
   * files are downloaded and play right in the chat; Telegraph pages get an
   * "Instant View" button that opens them in the in-app reader.
   */
  import { api, assetUrl } from "../tauri";
  import MediaPlayer from "./MediaPlayer.svelte";
  import { showToast } from "./Toast.svelte";
  import { settings } from "../theme";
  import { t } from "../i18n";
  import { openTelegraph, openLink } from "../reader";
  import { openLightbox } from "./Lightbox.svelte";

  interface Props { url: string; compact?: boolean }
  let { url, compact = false }: Props = $props();

  const AUTO_VIDEO_MB = 60;
  const MAX_VIDEO_MB = 500;

  let p = $state<Preview | null>(null);
  let ytPlaying = $state(false);
  let video = $state<"idle" | "loading" | "ready" | "error">("idle");
  let videoPath = $state<string | null>(null);
  let videoErr = $state("");

  function load(u: string): Promise<Preview | null> {
    let pr = inflight.get(u);
    if (!pr) {
      pr = api.linkPreview(u).catch(() => null);
      inflight.set(u, pr);
    }
    return pr;
  }

  $effect(() => {
    const u = url;
    p = null; ytPlaying = false; video = "idle"; videoPath = null;
    if (!$settings.linkPreviews || !u) return;
    let alive = true;
    void load(u).then((r) => {
      if (!alive || !r) return;
      p = r;
      if (r.kind === "video" && r.video_url) {
        const cached = videoPaths.get(r.video_url);
        if (cached) { videoPath = cached; video = "ready"; }
        else if ($settings.autoDownloadMedia && (r.video_size == null || r.video_size <= AUTO_VIDEO_MB * 1024 * 1024)) void fetchVideo();
      }
    });
    return () => { alive = false; };
  });

  async function fetchVideo() {
    if (!p?.video_url || video === "loading") return;
    video = "loading"; videoErr = "";
    try {
      const m = await api.cacheRemoteMedia(p.video_url, MAX_VIDEO_MB);
      videoPaths.set(p.video_url, m.path);
      videoPath = m.path;
      video = "ready";
    } catch (e) {
      videoErr = String(e);
      video = "error";
    }
  }

  function mb(n: number | null): string {
    return n == null ? "" : `${(n / 1024 / 1024).toFixed(n > 100 * 1024 * 1024 ? 0 : 1)} MB`;
  }
  let img = $derived(p?.image_path ? assetUrl(p.image_path) : "");
</script>

{#if p && (p.title || p.description || img || p.kind !== "page")}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="lp" class:compact onclick={(e) => e.stopPropagation()}>
    {#if p.site}<div class="lp-site">{p.kind === "youtube" ? "▶ " : ""}{p.site}</div>{/if}
    {#if p.title}
      <button class="lp-title" onclick={() => (p?.kind === "telegraph" && p.telegraph_path ? openTelegraph(p.telegraph_path, url) : openLink(url))}>{p.title}</button>
    {/if}
    {#if p.description && !compact}<div class="lp-desc">{p.description}</div>{/if}

    {#if p.kind === "youtube" && p.youtube_id}
      <div class="lp-media video16">
        {#if ytPlaying}
          <iframe
            src={`https://www.youtube-nocookie.com/embed/${p.youtube_id}?autoplay=1&rel=0&modestbranding=1`}
            title={p.title ?? "YouTube"}
            allow="autoplay; encrypted-media; picture-in-picture; fullscreen"
            allowfullscreen
            referrerpolicy="strict-origin-when-cross-origin"
          ></iframe>
        {:else}
          <button class="lp-thumb" onclick={() => (ytPlaying = true)} aria-label={$t("Play")}>
            {#if img}<img src={img} alt="" />{/if}
            <span class="play yt"><svg viewBox="0 0 24 24"><path d="M8 5v14l11-7z" fill="currentColor"/></svg></span>
          </button>
        {/if}
      </div>
      {#if ytPlaying}
        <button class="lp-ext" onclick={() => openLink(url)}>{$t("Open on YouTube")} ↗</button>
      {/if}
    {:else if p.kind === "video"}
      <div class="lp-media">
        {#if video === "ready" && videoPath}
          <MediaPlayer src={assetUrl(videoPath)} kind="video" name={p?.title ?? undefined} autoplay />
          <button class="lp-ext" onclick={() => videoPath && openLightbox({ url: assetUrl(videoPath), kind: "video", path: videoPath, name: p?.title ?? undefined })}>{$t("Full screen")} ⤢</button>
        {:else}
          <button class="lp-thumb vid" onclick={fetchVideo} disabled={video === "loading"}>
            {#if img}<img src={img} alt="" />{/if}
            {#if video === "loading"}
              <span class="spin"></span><span class="vlabel">{$t("Loading video…")}</span>
            {:else}
              <span class="play"><svg viewBox="0 0 24 24"><path d="M8 5v14l11-7z" fill="currentColor"/></svg></span>
              <span class="vlabel">{$t("Load video")}{p.video_size ? ` · ${mb(p.video_size)}` : ""}</span>
            {/if}
          </button>
          {#if video === "error"}<div class="lp-err">{$t("Could not load the video")}: {videoErr}</div>{/if}
        {/if}
      </div>
    {:else if p.kind === "telegraph"}
      {#if img && !compact}<button class="lp-img" onclick={() => p?.telegraph_path && openTelegraph(p.telegraph_path, url)}><img src={img} alt="" /></button>{/if}
      <button class="iv" onclick={() => p?.telegraph_path && openTelegraph(p.telegraph_path, url)}>⚡ {$t("Instant View")}</button>
    {:else if img}
      <button class="lp-img" class:small={compact} onclick={() => openLightbox({ url: img, kind: "image", name: p?.title ?? undefined })}><img src={img} alt="" /></button>
    {/if}
  </div>
{/if}

<style>
  .lp {
    margin-top: 6px;
    padding: 6px 10px 8px;
    border-left: 3px solid var(--accent);
    border-radius: 6px;
    background: color-mix(in srgb, var(--accent) 8%, transparent);
    max-width: 420px;
    display: flex; flex-direction: column; gap: 3px;
  }
  .lp-site { font-size: 12.5px; font-weight: 700; color: var(--accent); }
  .lp-title {
    all: unset; cursor: pointer;
    font-size: 13.5px; font-weight: 700; color: var(--fg); line-height: 1.3;
  }
  .lp-title:hover { text-decoration: underline; }
  .lp-desc {
    font-size: 12.5px; color: var(--fg-muted); line-height: 1.4;
    display: -webkit-box; -webkit-line-clamp: 4; line-clamp: 4; -webkit-box-orient: vertical; overflow: hidden;
    white-space: pre-line;
  }
  .lp-media { margin-top: 5px; position: relative; }
  .video16 { aspect-ratio: 16 / 9; overflow: hidden; border-radius: 8px; }
  .video16 > .lp-thumb { position: absolute; inset: 0; min-height: 0; }
  .lp-media iframe {
    width: 100%; height: 100%; border: none; border-radius: 8px; display: block; background: #000;
  }
  .lp-thumb {
    all: unset; position: relative; display: block; cursor: pointer;
    width: 100%; height: 100%; min-height: 120px; border-radius: 8px; overflow: hidden; background: #0b0d14;
  }
  .lp-thumb.vid { aspect-ratio: 16 / 9; display: grid; place-items: center; }
  .lp-thumb img { width: 100%; height: 100%; object-fit: cover; display: block; }
  .play {
    position: absolute; left: 50%; top: 50%; transform: translate(-50%, -50%);
    width: 54px; height: 54px; border-radius: 50%;
    display: grid; place-items: center;
    background: rgba(0, 0, 0, 0.6); color: #fff;
  }
  .play.yt { width: 64px; height: 44px; border-radius: 12px; background: #ff0033; }
  .play svg { width: 26px; height: 26px; }
  .vlabel {
    position: absolute; left: 8px; bottom: 8px;
    padding: 2px 8px; border-radius: 8px; background: rgba(0, 0, 0, 0.6); color: #fff;
    font-size: 11.5px; font-weight: 600;
  }
  .spin {
    position: absolute; left: 50%; top: 50%; margin: -18px 0 0 -18px;
    width: 36px; height: 36px; border-radius: 50%;
    border: 3px solid rgba(255, 255, 255, 0.25); border-top-color: #fff;
    animation: lp-spin 0.8s linear infinite;
  }
  @keyframes lp-spin { to { transform: rotate(360deg); } }
  .lp-img { all: unset; cursor: zoom-in; display: block; margin-top: 5px; border-radius: 8px; overflow: hidden; }
  .lp-img img { width: 100%; max-height: 240px; object-fit: cover; display: block; }
  .lp-img.small img { max-height: 120px; }
  .lp-ext { all: unset; cursor: pointer; margin-top: 4px; font-size: 12px; color: var(--accent); font-weight: 600; }
  .lp-err { font-size: 11.5px; color: var(--danger); margin-top: 4px; }
  .iv {
    margin-top: 6px; align-self: stretch;
    padding: 7px 10px; border-radius: 8px; border: none; cursor: pointer;
    background: color-mix(in srgb, var(--accent) 18%, transparent); color: var(--accent);
    font-weight: 800; font-size: 12.5px; letter-spacing: 0.04em; text-transform: uppercase;
  }
  .iv:hover { background: color-mix(in srgb, var(--accent) 28%, transparent); }
  .compact .lp-desc { display: none; }
</style>
