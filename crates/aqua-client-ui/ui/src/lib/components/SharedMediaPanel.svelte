<script module lang="ts">
  import { writable } from "svelte/store";
  export const sharedMediaOpen = writable(false);
  export function openSharedMedia() { sharedMediaOpen.set(true); }
  export function closeSharedMedia() { sharedMediaOpen.set(false); }
  export function toggleSharedMedia() { sharedMediaOpen.update((v) => !v); }
</script>

<script lang="ts">
  import { streamSrc } from "../media";
  import { t } from "../i18n";
  import { activeChat } from "../stores";
  import { api, assetUrl } from "../tauri";
  import { openLightbox } from "./Lightbox.svelte";
  import { playTrack } from "../player";
  import type { ChatMessage } from "../types";

  type Tab = "media" | "files" | "voice" | "links";
  let tab = $state<Tab>("media");

  interface Item {
    message_id: string;
    name: string;
    size: number;
    mime: string | null;
    category: string | null;
    saved_at: string | null;
    time: string;
  }
  let items = $state<Item[]>([]);
  let links = $state<{ url: string; time: string }[]>([]);
  let loading = $state(false);
  let scannedFor = "";

  // Rescan whenever the panel opens or the active chat changes.
  $effect(() => {
    const open = $sharedMediaOpen;
    const chat = $activeChat;
    if (open && chat && chat.id !== scannedFor) {
      scannedFor = chat.id;
      void scan(chat.id);
    }
    if (!open) scannedFor = "";
  });

  const URL_RE = /(https?:\/\/[^\s]+)/g;

  async function scan(peerId: string) {
    loading = true;
    const its: Item[] = [];
    const lnks: { url: string; time: string }[] = [];
    try {
      const msgs: ChatMessage[] = await api.getMessages(peerId);
      for (const m of msgs) {
        if (m.kind.type === "file") {
          const c = m.kind.content;
          its.push({ message_id: m.id, name: c.name, size: c.size, mime: c.mime, category: c.category, saved_at: c.saved_at, time: m.time });
        } else if (m.kind.type === "text") {
          const found = m.kind.content.text.match(URL_RE);
          if (found) for (const u of found) lnks.push({ url: u, time: m.time });
        }
      }
    } catch (e) { console.warn("shared-media scan", e); }
    its.reverse(); lnks.reverse(); // newest first
    items = its; links = lnks;
    loading = false;
  }

  // ---- classification ----
  function isImage(e: Item) { const n = e.name.toLowerCase(); return (e.mime ?? "").startsWith("image/") || /\.(png|jpe?g|webp|gif|bmp|svg)$/.test(n); }
  function isVideo(e: Item) { const n = e.name.toLowerCase(); return (e.mime ?? "").startsWith("video/") || /\.(mp4|webm|mov|mkv|avi|m4v)$/.test(n); }
  function isAudio(e: Item) { const n = e.name.toLowerCase(); return e.category === "voice" || (e.mime ?? "").startsWith("audio/") || /\.(mp3|wav|ogg|flac|m4a|opus|aac)$/.test(n); }

  let mediaItems = $derived(items.filter((e) => (isImage(e) || isVideo(e)) && e.category !== "sticker"));
  let voiceItems = $derived(items.filter(isAudio));
  let fileItems  = $derived(items.filter((e) => !isImage(e) && !isVideo(e) && !isAudio(e)));

  // ---- thumbnails (stream from disk via the asset protocol) ----
  function thumb(path: string | null, _mime: string | null): string | null {
    return path ? assetUrl(path) : null;
  }

  function playAudio(e: Item) {
    if (!e.saved_at) return;
    playTrack({ src: assetUrl(e.saved_at), title: e.category === "voice" ? "Voice message" : e.name, sub: $activeChat?.name ?? "" });
  }

  function fmtSize(b: number): string {
    if (b < 1024) return `${b} B`;
    if (b < 1024 * 1024) return `${(b / 1024).toFixed(1)} KB`;
    return `${(b / 1024 / 1024).toFixed(1)} MB`;
  }
  function host(u: string): string { try { return new URL(u).hostname.replace(/^www\./, ""); } catch { return u; } }
</script>

{#if $sharedMediaOpen}
  <aside class="sm" data-back>
    <div class="sm-head">
      <h3>{$t("Shared media")}</h3>
      <button data-x class="sm-close" onclick={closeSharedMedia} title={$t("Close")}>×</button>
    </div>
    <div class="sm-tabs">
      {#each [["media","Media"],["files","Files"],["voice","Voice"],["links","Links"]] as t}
        <button class="sm-tab" class:active={tab === t[0]} onclick={() => (tab = t[0] as Tab)}>{t[1]}</button>
      {/each}
    </div>

    <div class="sm-body">
      {#if loading}
        <div class="sm-status">{$t("Scanning…")}</div>
      {:else if tab === "media"}
        {#if mediaItems.length === 0}
          <div class="sm-status">{$t("No media yet.")}</div>
        {:else}
          <div class="sm-grid">
            {#each mediaItems as e (e.message_id)}
              {@const u = e.saved_at ? thumb(e.saved_at, e.mime) : null}
              <button class="sm-cell" onclick={() => u && openLightbox({ url: u, kind: isVideo(e) ? "video" : "image", name: e.name })}>
                {#if u}
                  {#if isVideo(e)}<video use:streamSrc={u} muted preload="metadata"></video>{:else}<img src={u} alt={e.name} />{/if}
                  {#if isVideo(e)}<span class="sm-playmark">▶</span>{/if}
                {:else}
                  <div class="sm-ph"></div>
                {/if}
              </button>
            {/each}
          </div>
        {/if}
      {:else if tab === "voice"}
        {#if voiceItems.length === 0}
          <div class="sm-status">{$t("No audio yet.")}</div>
        {:else}
          {#each voiceItems as e (e.message_id)}
            <div class="sm-row">
              <button class="sm-row-main" onclick={() => playAudio(e)}>
                <span class="sm-ic">♪</span>
                <span class="sm-row-name" title={e.name}>{e.category === "voice" ? "Voice message" : e.name}</span>
                <span class="sm-row-sub">{fmtSize(e.size)} · {e.time}</span>
              </button>
            </div>
          {/each}
        {/if}
      {:else if tab === "files"}
        {#if fileItems.length === 0}
          <div class="sm-status">{$t("No files yet.")}</div>
        {:else}
          {#each fileItems as e (e.message_id)}
            <div class="sm-row">
              <span class="sm-ic">{(e.name.split(".").pop() ?? "").toUpperCase().slice(0,4) || "·"}</span>
              <div class="sm-row-col">
                <span class="sm-row-name" title={e.name}>{e.name}</span>
                <span class="sm-row-sub">{fmtSize(e.size)} · {e.time}</span>
              </div>
            </div>
          {/each}
        {/if}
      {:else if tab === "links"}
        {#if links.length === 0}
          <div class="sm-status">{$t("No links yet.")}</div>
        {:else}
          {#each links as l (l.url + l.time)}
            <a class="sm-link" href={l.url} target="_blank" rel="noopener noreferrer">
              <span class="sm-link-host">{host(l.url)}</span>
              <span class="sm-link-url">{l.url}</span>
            </a>
          {/each}
        {/if}
      {/if}
    </div>
  </aside>
{/if}

<style>
  .sm {
    width: 320px;
    flex-shrink: 0;
    background: var(--bg-list);
    border-left: 1px solid var(--border);
    display: flex; flex-direction: column;
    min-height: 0;
    animation: sm-in 0.18s ease-out;
  }
  @keyframes sm-in { from { opacity: 0; transform: translateX(10px); } to { opacity: 1; transform: translateX(0); } }
  .sm-head { display: flex; align-items: center; justify-content: space-between; padding: 14px 16px; border-bottom: 1px solid var(--border); }
  .sm-head h3 { margin: 0; font-size: 14px; font-weight: 700; }
  .sm-close { width: 28px; height: 28px; background: transparent; border: 1px solid var(--border); border-radius: calc(8px * var(--rk, 1)); color: var(--fg-dim); font-size: 17px; cursor: pointer; }
  .sm-close:hover { color: var(--fg); background: var(--bg-hover); }
  .sm-tabs { display: flex; gap: 3px; padding: 8px 10px; border-bottom: 1px solid var(--border); }
  .sm-tab { flex: 1; padding: 6px 4px; border-radius: calc(7px * var(--rk, 1)); background: transparent; border: none; color: var(--fg-muted); font-size: 12px; font-weight: 600; cursor: pointer; }
  .sm-tab:hover { color: var(--fg); background: var(--bg-hover); }
  .sm-tab.active { color: var(--accent); background: var(--accent-soft); }
  .sm-body { flex: 1; overflow-y: auto; padding: 8px; }
  .sm-status { padding: 36px 16px; text-align: center; color: var(--fg-dim); font-size: 12.5px; }

  .sm-grid { display: grid; grid-template-columns: repeat(3, 1fr); gap: 4px; }
  .sm-cell { position: relative; aspect-ratio: 1; border: none; padding: 0; border-radius: calc(8px * var(--rk, 1)); overflow: hidden; background: var(--bg-input); cursor: pointer; }
  .sm-cell img, .sm-cell video { width: 100%; height: 100%; object-fit: cover; display: block; }
  .sm-ph { width: 100%; height: 100%; background: var(--bg-input); }
  .sm-playmark { position: absolute; inset: 0; display: grid; place-items: center; color: #fff; font-size: 18px; text-shadow: 0 1px 4px rgba(0,0,0,0.6); }

  .sm-row { display: flex; align-items: center; gap: 10px; padding: 8px; border-radius: calc(9px * var(--rk, 1)); }
  .sm-row:hover { background: var(--bg-hover); }
  .sm-row-main { display: flex; align-items: center; gap: 10px; width: 100%; background: transparent; border: none; cursor: pointer; color: inherit; text-align: left; padding: 0; }
  .sm-ic {
    width: 34px; height: 34px; flex-shrink: 0; border-radius: calc(9px * var(--rk, 1));
    background: var(--bg-input); border: 1px solid var(--border);
    display: grid; place-items: center; font-size: 11px; font-weight: 700; color: var(--accent);
  }
  .sm-row-col { min-width: 0; display: flex; flex-direction: column; }
  .sm-row-name { font-size: 13px; font-weight: 600; color: var(--fg); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sm-row-sub { font-size: 11px; color: var(--fg-dim); }

  .sm-link { display: block; padding: 8px 10px; border-radius: calc(9px * var(--rk, 1)); text-decoration: none; }
  .sm-link:hover { background: var(--bg-hover); }
  .sm-link-host { display: block; font-size: 13px; font-weight: 600; color: var(--accent); }
  .sm-link-url { display: block; font-size: 11px; color: var(--fg-dim); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

  @media (max-width: 960px) { .sm { display: none; } }
</style>
