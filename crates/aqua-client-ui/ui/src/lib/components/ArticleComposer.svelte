<script module lang="ts">
  import { writable, get as getStore } from "svelte/store";

  interface ComposerState {
    peerId: string;
  }
  export const articleState = writable<ComposerState | null>(null);

  export function openArticleComposer(peerId: string) {
    articleState.set({ peerId });
  }
  export function closeArticleComposer() {
    articleState.set(null);
  }
</script>

<script lang="ts">
  import { onMount } from "svelte";
  import { api, bytesToDataUrl } from "../tauri";
  import { loadActiveMessages, chats } from "../stores";
  import { showToast } from "./Toast.svelte";
  import { cropImage } from "./ImageCropper.svelte";
  import { t, tr } from "../i18n";
  import { open as openFileDialog } from "@tauri-apps/plugin-dialog";

  let title = $state("");
  let body = $state("");
  let coverDataUrl = $state<string | null>(null);
  let preview = $state(false);
  let sending = $state(false);

  // Reset state when the composer opens for a new peer.
  let prevPeer = "";
  $effect(() => {
    const s = $articleState;
    if (s && s.peerId !== prevPeer) {
      prevPeer = s.peerId;
      title = "";
      body = "";
      coverDataUrl = null;
      preview = false;
      sending = false;
    } else if (!s) {
      prevPeer = "";
    }
  });

  async function pickCover() {
    try {
      const result = await openFileDialog({
        multiple: false, directory: false, title: "Pick a cover image",
        filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp"] }],
      });
      const path = Array.isArray(result) ? (result[0] ?? null) : result;
      if (!path) return;
      const bytes = await api.readUserImage(path);
      // Frame + shrink the cover (it travels inside the message itself, so
      // a raw multi-megabyte photo would make the article huge).
      const src = URL.createObjectURL(new Blob([new Uint8Array(bytes)]));
      const blob = await cropImage(src, { aspect: 16 / 9, outWidth: 1280, title: $t("Position the cover") });
      URL.revokeObjectURL(src);
      if (!blob) return;
      coverDataUrl = bytesToDataUrl(Array.from(new Uint8Array(await blob.arrayBuffer())), "image/webp");
    } catch (e) {
      showToast(`Could not load cover: ${e}`);
    }
  }
  function removeCover() { coverDataUrl = null; }

  async function send() {
    const s = $articleState;
    if (!s) return;
    if (!title.trim()) { showToast(tr("Give the article a title first")); return; }
    if (!body.trim())  { showToast(tr("Write something first")); return; }
    const chat = $chats.find((c) => c.id === s.peerId);
    if (chat && !chat.live && !s.peerId.startsWith("grp_") && s.peerId !== "__saved__") {
      showToast(tr("This chat isn't connected yet — start a real chat first"));
      return;
    }
    sending = true;
    const payload = JSON.stringify({
      title: title.trim(),
      body: body.trim(),
      cover_data_url: coverDataUrl,
    });
    const envelope = `[article:v1]${payload}`;
    try {
      // Groups and channels fan out to every member; 1:1 uses the session.
      if (s.peerId.startsWith("grp_")) await api.groupSend(s.peerId, envelope);
      else await api.sendMessage(s.peerId, envelope);
      await loadActiveMessages();
      chats.set(await api.listChats());
      closeArticleComposer();
      showToast(tr("Article published"));
    } catch (e) {
      showToast(tr("Could not publish: {e}", { e: String(e) }), 6000);
    } finally {
      sending = false;
    }
  }

  onMount(() => {
    const h = (e: KeyboardEvent) => {
      if (e.key === "Escape") closeArticleComposer();
      else if ((e.metaKey || e.ctrlKey) && e.key === "Enter") void send();
    };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  });

  // Simple markdown-ish rendering for the live preview: headings,
  // bold, italic, inline code, links, paragraph breaks. Not a full
  // parser — just enough to make the preview feel like the real
  // thing before send.
  function renderMarkdown(src: string): string {
    const escape = (s: string) => s.replace(/[&<>]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" }[c] ?? c));
    const lines = escape(src).split(/\r?\n/);
    const out: string[] = [];
    let inList = false;
    for (const raw of lines) {
      const line = raw.trimEnd();
      if (/^#\s/.test(line)) {
        if (inList) { out.push("</ul>"); inList = false; }
        out.push(`<h2>${line.slice(2)}</h2>`);
      } else if (/^##\s/.test(line)) {
        if (inList) { out.push("</ul>"); inList = false; }
        out.push(`<h3>${line.slice(3)}</h3>`);
      } else if (/^-\s/.test(line) || /^\*\s/.test(line)) {
        if (!inList) { out.push("<ul>"); inList = true; }
        out.push(`<li>${inline(line.slice(2))}</li>`);
      } else if (line === "") {
        if (inList) { out.push("</ul>"); inList = false; }
        out.push("");
      } else {
        if (inList) { out.push("</ul>"); inList = false; }
        out.push(`<p>${inline(line)}</p>`);
      }
    }
    if (inList) out.push("</ul>");
    return out.join("\n");
  }
  function inline(s: string): string {
    return s
      .replace(/\*\*(.+?)\*\*/g, "<strong>$1</strong>")
      .replace(/\*(.+?)\*/g, "<em>$1</em>")
      .replace(/`([^`]+)`/g, "<code>$1</code>")
      .replace(/\[([^\]]+)\]\((https?:\/\/[^\)]+)\)/g, `<a href="$2" target="_blank" rel="noopener">$1</a>`);
  }

  let words = $derived(body.trim().split(/\s+/).filter(Boolean).length);
  let readMin = $derived(Math.max(1, Math.round(words / 220)));
</script>

{#if $articleState}
  <div
    class="backdrop"
    role="button"
    tabindex="-1"
    aria-label={$t("Close composer")}
    onclick={(e) => { if ((e.target as HTMLElement).classList.contains("backdrop")) closeArticleComposer(); }}
    onkeydown={(e) => { if (e.key === "Escape") closeArticleComposer(); }}
  >
    <div class="sheet" role="dialog" aria-modal="true" tabindex="-1">
      <div class="head">
        <h2>{$t("Write an article")}</h2>
        <div class="segmented">
          <button class:active={!preview} onclick={() => (preview = false)}>{$t("Edit")}</button>
          <button class:active={preview}  onclick={() => (preview = true)}>{$t("Preview")}</button>
        </div>
        <button class="close" onclick={closeArticleComposer} title={$t("Close (Esc)")}>×</button>
      </div>

      {#if !preview}
        <div class="body">
          <div class="cover-row">
            {#if coverDataUrl}
              <div class="cover" style={`background-image: url(${coverDataUrl});`}>
                <button class="cover-remove" onclick={removeCover} title={$t("Remove cover")}>×</button>
              </div>
            {:else}
              <button class="cover-pick" onclick={pickCover}>+ {$t("Add cover image")}</button>
            {/if}
          </div>
          <input
            class="title-input"
            bind:value={title}
            placeholder={$t("Title")}
            maxlength="140"
          />
          <textarea
            class="body-input"
            bind:value={body}
            placeholder="Write your article in Markdown…&#10;&#10;# Heading&#10;**bold**, *italic*, `code`, [links](https://example.com)&#10;- bulleted lists"
          ></textarea>
          <div class="footnote">
            {words} word{words === 1 ? "" : "s"} · ≈{readMin} min read ·
            <code>**bold**</code> <code>*italic*</code> <code>`code`</code> <code># heading</code>
          </div>
        </div>
      {:else}
        <div class="preview">
          {#if coverDataUrl}
            <div class="pv-cover" style={`background-image: url(${coverDataUrl});`}></div>
          {/if}
          <article class="article-prose">
            <h1>{title || $t("Untitled")}</h1>
            <div class="pv-meta">{words} word{words === 1 ? "" : "s"} · ≈{readMin} min read</div>
            {@html renderMarkdown(body)}
          </article>
        </div>
      {/if}

      <div class="foot">
        <span class="hint">{$t("Ctrl + Enter to publish · Esc to cancel")}</span>
        <div>
          <button class="btn-ghost" onclick={closeArticleComposer} disabled={sending}>{$t("Cancel")}</button>
          <button class="btn-primary" onclick={send} disabled={sending} title={!title.trim() ? $t("Add a title") : !body.trim() ? $t("Write the text") : ""}>
            {sending ? $t("Publishing…") : $t("Publish")}
          </button>
        </div>
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed; inset: 0;
    background: rgba(6, 7, 12, 0.7);
    backdrop-filter: blur(4px);
    display: grid; place-items: center;
    z-index: 125;
    padding: 20px;
  }
  .sheet {
    width: min(680px, 94vw);
    max-height: 94vh;
    display: flex; flex-direction: column;
    background: var(--bg-shell);
    border: 1px solid var(--border);
    border-radius: 16px;
    box-shadow: 0 32px 80px -16px rgba(0,0,0,0.6);
    overflow: hidden;
    animation: slide-up 0.22s ease-out;
  }
  @keyframes slide-up {
    from { opacity: 0; transform: translateY(12px); }
    to   { opacity: 1; transform: translateY(0); }
  }
  .head {
    padding: 12px 18px;
    display: grid;
    grid-template-columns: 1fr auto auto;
    gap: 12px;
    align-items: center;
    border-bottom: 1px solid var(--border);
  }
  .head h2 { margin: 0; font-size: 15.5px; font-weight: 700; }
  .segmented {
    display: flex;
    background: var(--bg-input);
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 3px;
    gap: 2px;
  }
  .segmented button {
    background: transparent; border: none;
    padding: 5px 12px;
    border-radius: 7px;
    color: var(--fg-muted);
    font-weight: 600;
    font-size: 12.5px;
    cursor: pointer;
  }
  .segmented button.active { background: var(--accent-soft); color: var(--accent); }
  .close { width: 30px; height: 30px; background: transparent; border: 1px solid var(--border); border-radius: 8px; color: var(--fg-dim); font-size: 18px; cursor: pointer; }

  .body {
    padding: 18px;
    display: flex; flex-direction: column; gap: 10px;
    overflow-y: auto;
  }
  .cover-row { display: flex; }
  .cover {
    width: 100%;
    aspect-ratio: 16 / 7;
    border-radius: 12px;
    background-size: cover;
    background-position: center;
    position: relative;
  }
  .cover-remove {
    position: absolute; top: 8px; right: 8px;
    width: 28px; height: 28px;
    background: rgba(0,0,0,0.5);
    border: none; border-radius: 50%;
    color: #fff; font-size: 16px; cursor: pointer;
  }
  .cover-pick {
    width: 100%;
    padding: 20px;
    background: var(--bg-input);
    border: 1px dashed var(--border-strong);
    border-radius: 12px;
    color: var(--fg-dim);
    font-size: 13px;
    cursor: pointer;
  }
  .cover-pick:hover { color: var(--accent); border-color: var(--accent); }

  .title-input {
    background: transparent;
    border: none;
    outline: none;
    font-family: inherit;
    font-size: 24px;
    font-weight: 800;
    color: var(--fg);
    padding: 4px 0;
    letter-spacing: -0.01em;
    border-bottom: 1px solid var(--border);
  }
  .title-input:focus { border-bottom-color: var(--accent); }
  .title-input::placeholder { color: var(--fg-dim); }

  .body-input {
    flex: 1;
    min-height: 240px;
    background: var(--bg-input);
    border: 1px solid var(--border);
    border-radius: 10px;
    outline: none;
    color: var(--fg);
    font-family: var(--font-mono);
    font-size: 13.5px;
    line-height: 1.6;
    padding: 12px 14px;
    resize: vertical;
  }
  .body-input:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  .footnote { font-size: 11.5px; color: var(--fg-dim); display: flex; flex-wrap: wrap; gap: 6px; align-items: center; }
  .footnote code { font-family: var(--font-mono); background: var(--bg-input); padding: 2px 6px; border-radius: 4px; color: var(--accent); }

  .preview {
    overflow-y: auto;
    background: var(--bg-shell);
  }
  .pv-cover {
    width: 100%;
    aspect-ratio: 16 / 7;
    background-size: cover;
    background-position: center;
  }
  .article-prose {
    max-width: 640px;
    margin: 0 auto;
    padding: 28px 32px;
    color: var(--fg);
    font-size: 15.5px;
    line-height: 1.7;
  }
  .article-prose h1 { font-size: 28px; font-weight: 800; margin: 0 0 6px; letter-spacing: -0.02em; }
  .article-prose h2 { font-size: 20px; margin: 24px 0 6px; }
  .article-prose h3 { font-size: 16.5px; margin: 20px 0 4px; }
  .article-prose p  { margin: 0 0 12px; }
  .article-prose ul { margin: 0 0 12px; padding-left: 22px; }
  .article-prose li { margin-bottom: 4px; }
  .article-prose code { font-family: var(--font-mono); background: var(--bg-input); padding: 2px 6px; border-radius: 4px; font-size: 0.92em; }
  .article-prose a { color: var(--accent); text-decoration: underline; text-decoration-color: var(--accent-soft); }
  .pv-meta { font-size: 12.5px; color: var(--fg-dim); margin-bottom: 20px; }

  .foot {
    padding: 12px 18px;
    display: flex; justify-content: space-between; align-items: center; gap: 10px;
    border-top: 1px solid var(--border);
    background: var(--bg-list);
  }
  .hint { font-size: 11px; color: var(--fg-dim); }
  .foot > div { display: flex; gap: 10px; }
  .btn-ghost, .btn-primary {
    padding: 8px 18px;
    border-radius: 10px;
    font-size: 13px;
    font-weight: 700;
    cursor: pointer;
    border: none;
  }
  .btn-ghost { background: transparent; border: 1px solid var(--border); color: var(--fg-muted); }
  .btn-ghost:hover:not(:disabled) { color: var(--fg); border-color: var(--border-strong); }
  .btn-primary {
    background: linear-gradient(135deg, var(--accent), var(--accent-2));
    color: #06070C;
    box-shadow: 0 6px 16px -4px var(--accent-glow);
  }
  .btn-primary:hover:not(:disabled) { transform: translateY(-1px); }
  button:disabled { opacity: 0.5; cursor: not-allowed; }
</style>
