<script lang="ts">
  import { streamSrc } from "../media";
  import { t } from "../i18n";
  import { onMount } from "svelte";
  import { chats, activeTab, selectChat } from "../stores";
  import { api, assetUrl } from "../tauri";
  import { displayName } from "../contacts";
  import { icons } from "../icons";
  import MediaPlayer from "./MediaPlayer.svelte";
  import { openLightbox, openGallery, type GalleryItem } from "./Lightbox.svelte";
  import { playQueue } from "../player";
  import type { ChatMessage } from "../types";

  // Previews stream from disk via the asset protocol — instant, no IPC.
  function previewFor(path: string | null, _mime: string | null): string | null {
    return path ? assetUrl(path) : null;
  }

  // Which row is "expanded" (showing a player / big thumbnail).
  let expanded = $state<string | null>(null);
  function toggleExpand(id: string) {
    expanded = expanded === id ? null : id;
  }

  interface FileEntry {
    peer_id: string;
    peer_name: string;
    message_id: string;
    time: string;
    name: string;
    size: number;
    mime: string | null;
    category: string | null;
    saved_at: string | null;
    from_me: boolean;
  }

  // Local tab filter: all | images | videos | audio | docs
  type Tab = "all" | "images" | "videos" | "audio" | "docs";
  let tab = $state<Tab>("all");

  let entries = $state<FileEntry[]>([]);
  let loading = $state(true);

  async function refresh() {
    loading = true;
    const out: FileEntry[] = [];
    for (const chat of $chats) {
      try {
        const msgs: ChatMessage[] = await api.getMessages(chat.id);
        for (const m of msgs) {
          if (m.kind.type !== "file") continue;
          const c = m.kind.content;
          out.push({
            peer_id:    chat.id,
            peer_name:  displayName(chat.id, chat.name),
            message_id: m.id,
            time:       m.time,
            name:       c.name,
            size:       c.size,
            mime:       c.mime,
            category:   c.category,
            saved_at:   c.saved_at,
            from_me:    m.from === "me",
          });
        }
      } catch (e) { console.warn("list files for", chat.id, e); }
    }
    // Newest first.
    out.sort((a, b) => (a.time < b.time ? 1 : -1));
    entries = out;
    loading = false;
  }

  function matchTab(e: FileEntry): boolean {
    if (tab === "all") return true;
    const mime = (e.mime ?? "").toLowerCase();
    const name = e.name.toLowerCase();
    switch (tab) {
      case "images": return mime.startsWith("image/") || /\.(png|jpe?g|webp|gif|bmp|svg)$/.test(name);
      case "videos": return mime.startsWith("video/") || /\.(mp4|webm|mov|mkv|avi|m4v)$/.test(name);
      case "audio":  return mime.startsWith("audio/") || /\.(mp3|wav|ogg|flac|m4a|opus|aac)$/.test(name);
      case "docs":   return /\.(pdf|docx?|xlsx?|pptx?|txt|md|rtf|odt|zip|7z|rar)$/.test(name);
    }
  }
  let shown = $derived(entries.filter(matchTab));

  function fmtSize(b: number): string {
    if (b < 1024) return `${b} B`;
    if (b < 1024 * 1024) return `${(b / 1024).toFixed(1)} KB`;
    if (b < 1024 * 1024 * 1024) return `${(b / 1024 / 1024).toFixed(1)} MB`;
    return `${(b / 1024 / 1024 / 1024).toFixed(2)} GB`;
  }

  function iconFor(e: FileEntry): string {
    const mime = (e.mime ?? "").toLowerCase();
    if (e.category === "sticker") return "🎨";
    if (e.category === "gif" || mime === "image/gif") return "🎞️";
    if (mime.startsWith("image/")) return "🖼️";
    if (mime.startsWith("video/")) return "📹";
    if (mime.startsWith("audio/")) return "🎵";
    return "📄";
  }

  function kindOf(e: FileEntry): "audio" | "image" | "video" | "doc" {
    const mime = (e.mime ?? "").toLowerCase(), name = e.name.toLowerCase();
    if (e.category === "voice" || mime.startsWith("audio/") || /\.(mp3|wav|ogg|flac|m4a|opus|aac)$/.test(name)) return "audio";
    if (mime.startsWith("image/") || /\.(png|jpe?g|webp|gif|bmp|svg)$/.test(name)) return "image";
    if (mime.startsWith("video/") || /\.(mp4|webm|mov|mkv|avi|m4v)$/.test(name)) return "video";
    return "doc";
  }
  /** Click on a file: photos / videos open the gallery over everything in
   *  the current tab, audio plays as a queue, documents open their chat. */
  function openEntry(e: FileEntry) {
    const k = kindOf(e);
    if (!e.saved_at || k === "doc") { openChat(e.peer_id); return; }
    if (k === "audio") {
      const list = shown.filter((x) => x.saved_at && kindOf(x) === "audio");
      playQueue(list.map((x) => ({ src: assetUrl(x.saved_at!), title: x.name, sub: x.peer_name })), Math.max(0, list.indexOf(e)));
      return;
    }
    const list = shown.filter((x) => x.saved_at && (kindOf(x) === "image" || kindOf(x) === "video"));
    const items: GalleryItem[] = list.map((x) => ({
      url: assetUrl(x.saved_at!), kind: kindOf(x) === "video" ? "video" : "image", name: x.name,
      path: x.saved_at, size: x.size, from: x.from_me ? $t("You") : x.peer_name, time: x.time,
    }));
    openGallery(items, Math.max(0, list.indexOf(e)));
  }

  function openChat(peerId: string) {
    activeTab.set("chats");
    void selectChat(peerId);
  }

  onMount(() => { void refresh(); });
</script>

<aside class="panel">
  <div class="head">
    <h2>{$t("Files")}</h2>
    <button class="refresh" onclick={refresh} title={$t("Rescan")}>↻</button>
  </div>

  <div class="tabs">
    {#each [
      { id: "all",    label: "All" },
      { id: "images", label: "Images" },
      { id: "videos", label: "Videos" },
      { id: "audio",  label: "Audio" },
      { id: "docs",   label: "Docs" },
    ] as tb}
      <button
        class="tab"
        class:active={tab === tb.id}
        onclick={() => (tab = tb.id as Tab)}
      >{$t(tb.label)}</button>
    {/each}
  </div>

  <div class="list">
    {#if loading}
      <div class="status">{$t("Scanning all chats…")}</div>
    {:else if shown.length === 0}
      <div class="empty">
        <div class="empty-icon">{@html icons.files}</div>
        <div class="empty-title">{$t("Nothing yet")}</div>
        <div class="empty-sub">{$t("Files sent and received across your chats will appear here.")}</div>
      </div>
    {:else}
      {#each shown as e (`${e.peer_id}:${e.message_id}`)}
        {@const rowId = `${e.peer_id}:${e.message_id}`}
        {@const mime  = (e.mime ?? "").toLowerCase()}
        {@const name  = e.name.toLowerCase()}
        {@const isAudio = e.category === "voice" || mime.startsWith("audio/") || /\.(mp3|wav|ogg|flac|m4a|opus|aac|webm)$/.test(name)}
        {@const isImage = mime.startsWith("image/") || /\.(png|jpe?g|webp|gif|bmp|svg)$/.test(name)}
        {@const isVideo = mime.startsWith("video/") || /\.(mp4|webm|mov|mkv|avi|m4v)$/.test(name)}
        {@const previewable = (isAudio || isImage || isVideo) && !!e.saved_at}
        {@const previewMime = e.mime ?? (isAudio ? "audio/webm" : isImage ? "image/jpeg" : "video/mp4")}
        {@const url = previewable ? previewFor(e.saved_at, previewMime) : null}
        <div class="row-wrap">
          <div class="row">
            <button class="main" onclick={() => openEntry(e)}>
              <div class="ic" class:thumb={!!url && (isImage || isVideo)}>
                {#if url && isImage}
                  <img src={url} alt="" loading="lazy" />
                {:else if url && isVideo}
                  <video use:streamSrc={url + "#t=0.5"} muted preload="metadata"></video>
                  <span class="vplay"><svg viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg></span>
                {:else}
                  {iconFor(e)}
                {/if}
              </div>
              <div class="meta">
                <div class="r1">
                  <span class="name" title={e.name}>{e.name}</span>
                  <span class="size">{fmtSize(e.size)}</span>
                </div>
                <div class="r2">
                  <span class="peer">{e.from_me ? "you → " : ""}{e.peer_name}</span>
                  <span class="dot">·</span>
                  <span class="when">{e.time}</span>

                </div>
              </div>
            </button>
            {#if previewable && (isVideo || isImage)}
              <button class="goto" onclick={() => toggleExpand(rowId)} title={$t("Preview")}>{expanded === rowId ? "▾" : "▸"}</button>
            {/if}
            <button class="goto" onclick={() => openChat(e.peer_id)} title={$t("Open chat")}>↗</button>
          </div>
          {#if expanded === rowId && previewable}
            <div class="preview">
              {#if url}
                {#if isAudio}
                  <button class="bg-play" onclick={() => openEntry(e)} title={$t("Play in background")}>
                    <svg viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg>
                    {$t("Play in background")}
                  </button>
                {:else if isImage}
                  <button class="img-btn" onclick={() => openLightbox({ url, kind: "image", name: e.name })}>
                    <img class="img" src={url} alt={e.name} />
                  </button>
                {:else if isVideo}
                  <MediaPlayer src={url} kind="video" name={e.name} onExpand={() => openLightbox({ url, kind: "video", name: e.name })} />
                {/if}
              {:else}
                <div class="loading">{$t("Loading preview…")}</div>
              {/if}
            </div>
          {/if}
        </div>
      {/each}
    {/if}
  </div>
</aside>

<style>
  .panel {
    background: var(--bg-list);
    border-right: 1px solid var(--border);
    display: flex; flex-direction: column;
    min-width: 0;
  }
  .head {
    padding: 14px 18px 10px;
    display: flex; align-items: center; justify-content: space-between;
  }
  .head h2 { margin: 0; font-size: 15.5px; font-weight: 700; letter-spacing: -0.01em; }
  .refresh {
    width: 28px; height: 28px;
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 8px;
    color: var(--fg-muted);
    cursor: pointer;
    font-size: 15px;
  }
  .refresh:hover { color: var(--accent); border-color: var(--accent-soft); }

  .tabs {
    display: flex; gap: 4px;
    padding: 0 14px 8px;
    overflow-x: auto;
    border-bottom: 1px solid var(--border);
  }
  .tab {
    padding: 5px 11px;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 7px;
    color: var(--fg-muted);
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    flex-shrink: 0;
  }
  .tab:hover { color: var(--fg); background: var(--bg-hover); }
  .tab.active { color: var(--accent); background: var(--accent-soft); border-color: color-mix(in srgb, var(--accent) 25%, transparent); }

  .list { flex: 1; overflow-y: auto; padding: 6px; }
  .status, .empty { padding: 40px 22px; text-align: center; color: var(--fg-dim); font-size: 12.5px; }
  .empty { padding-top: 60px; }
  .empty-icon {
    display: inline-grid; place-items: center;
    width: 56px; height: 56px;
    border-radius: 50%;
    background: var(--accent-soft);
    color: var(--accent);
    margin-bottom: 14px;
  }
  .empty-icon :global(svg) { width: 24px; height: 24px; }
  .empty-title { font-size: 14px; font-weight: 700; color: var(--fg); margin-bottom: 4px; }
  .empty-sub { line-height: 1.5; }

  .row-wrap {
    border-radius: 10px;
    margin-bottom: 2px;
    overflow: hidden;
  }
  .row-wrap:hover { background: var(--bg-hover); }
  .row {
    width: 100%;
    display: grid;
    grid-template-columns: 1fr 32px;
    align-items: center;
    gap: 4px;
  }
  .main {
    display: grid;
    grid-template-columns: 36px 1fr;
    gap: 11px;
    align-items: center;
    padding: 9px 10px;
    background: transparent;
    border: none;
    border-radius: 10px;
    text-align: left;
    color: inherit;
    cursor: pointer;
    width: 100%;
    min-width: 0;
  }
  .goto {
    width: 28px; height: 28px;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 7px;
    color: var(--fg-dim);
    font-size: 13px;
    cursor: pointer;
    margin-right: 8px;
  }
  .goto:hover { color: var(--accent); border-color: var(--accent-soft); background: var(--accent-soft); }
  .chev {
    display: inline-block;
    transition: transform 0.15s ease;
    color: var(--fg-dim);
  }
  .chev.open { transform: rotate(90deg); color: var(--accent); }
  .preview {
    padding: 6px 12px 12px 56px;
    display: flex; flex-direction: column; gap: 6px;
    animation: slide-in 0.18s ease-out;
  }
  @keyframes slide-in {
    from { opacity: 0; transform: translateY(-2px); }
    to   { opacity: 1; transform: translateY(0); }
  }
  .bg-play {
    display: inline-flex; align-items: center; gap: 6px; align-self: flex-start;
    padding: 7px 14px 7px 11px; border-radius: 100px; border: none;
    background: linear-gradient(135deg, var(--accent), var(--accent-2));
    color: #06070C; font-weight: 700; font-size: 12.5px; cursor: pointer;
  }
  .bg-play svg { width: 15px; height: 15px; }
  .bg-play:hover { filter: brightness(1.07); }
  .img-btn { border: none; padding: 0; background: transparent; cursor: zoom-in; align-self: flex-start; border-radius: 10px; overflow: hidden; }
  .img { max-width: 100%; max-height: 260px; border-radius: 10px; background: #0a0d14; display: block; }
  .loading { font-size: 12px; color: var(--fg-dim); font-style: italic; padding: 10px 0; }
  .ic.thumb { position: relative; width: 52px; height: 52px; padding: 0; overflow: hidden; background: #000; }
  .ic.thumb img, .ic.thumb video { width: 100%; height: 100%; object-fit: cover; display: block; }
  .vplay { position: absolute; inset: 0; display: grid; place-items: center; color: #fff; background: rgba(0,0,0,0.28); }
  .vplay svg { width: 18px; height: 18px; filter: drop-shadow(0 1px 3px rgba(0,0,0,0.6)); }
  .ic {
    width: 36px; height: 36px;
    border-radius: 10px;
    background: var(--bg-input);
    display: grid; place-items: center;
    font-size: 17px;
    border: 1px solid var(--border);
  }
  .meta { min-width: 0; }
  .r1 { display: flex; justify-content: space-between; gap: 10px; }
  .name {
    font-weight: 600;
    font-size: 13.5px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .size { font-size: 11.5px; color: var(--fg-dim); font-variant-numeric: tabular-nums; flex-shrink: 0; }
  .r2 { display: flex; align-items: center; gap: 6px; font-size: 11.5px; color: var(--fg-muted); margin-top: 2px; }
  .peer { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .dot { color: var(--fg-dim); }
  .when { font-variant-numeric: tabular-nums; flex-shrink: 0; }
</style>
