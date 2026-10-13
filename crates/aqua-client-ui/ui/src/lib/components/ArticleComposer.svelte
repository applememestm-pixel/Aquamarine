<script module lang="ts">
  import { writable } from "svelte/store";

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
  /**
   * Article editor — Telegraph-style: title / subtitle / cover, a formatting
   * toolbar (and the same shortcuts as Telegram), photo carousels, live
   * preview, and a per-chat draft that survives closing the window.
   */
  import { onMount, tick } from "svelte";
  import { api, bytesToDataUrl } from "../tauri";
  import type { ArticleMedia } from "../types";
  import { loadActiveMessages, chats } from "../stores";
  import { showToast } from "./Toast.svelte";
  import { cropImage } from "./ImageCropper.svelte";
  import { askInput, promptOpen } from "./InputPrompt.svelte";
  import { gallery } from "./Lightbox.svelte";
  import { get as getStore } from "svelte/store";
  import { t, tr } from "../i18n";
  import { openFileDialog } from "../dialog";
  import { parseBlocks, wordCount, parseGalleryRefs, galleryLine, GALLERY_LINE, type GalleryRef } from "../markup";
  import { shrinkImage, dataUrlBytes, probeMedia } from "../imageTools";
  import { fmtDuration, fmtSize, streamUrl } from "../media";
  import Markup from "./Markup.svelte";

  const MAX_PHOTOS = 40;
  const WARN_BYTES = 10 * 1024 * 1024;
  const MAX_BYTES = 24 * 1024 * 1024;
  const GALLERY = GALLERY_LINE;
  const MAX_MEDIA = 30;
  const VIDEO_EXT = ["mp4", "m4v", "webm", "mov", "mkv"];
  const AUDIO_EXT = ["mp3", "m4a", "aac", "ogg", "oga", "opus", "flac", "wav", "weba"];
  const IMAGE_EXT = ["png", "jpg", "jpeg", "webp", "gif", "bmp", "avif"];

  /** A video / track in the composer: a local file until publish uploads it. */
  interface PendingMedia {
    kind: "video" | "audio";
    name: string;
    path?: string;
    poster?: string;
    duration?: number;
    /** Set once uploaded (or when restored from a draft). */
    ref?: ArticleMedia;
  }

  let title = $state("");
  let subtitle = $state("");
  let body = $state("");
  let coverDataUrl = $state<string | null>(null);
  let images = $state<string[]>([]);
  let media = $state<PendingMedia[]>([]);
  let uploading = $state("");
  let preview = $state(false);
  let sending = $state(false);
  let adding = $state(0);
  let ta: HTMLTextAreaElement | undefined = $state();

  // ---------------------------------------------------------------- drafts
  const draftKey = (peer: string) => `aqua.articleDraft.${peer}`;
  let prevPeer = "";
  $effect(() => {
    const s = $articleState;
    if (s && s.peerId !== prevPeer) {
      prevPeer = s.peerId;
      preview = false; sending = false;
      title = ""; subtitle = ""; body = ""; coverDataUrl = null; images = []; media = [];
      try {
        const d = JSON.parse(localStorage.getItem(draftKey(s.peerId)) ?? "null");
        if (d) {
          title = d.title ?? ""; subtitle = d.subtitle ?? ""; body = d.body ?? "";
          coverDataUrl = d.cover ?? null; images = Array.isArray(d.images) ? d.images : [];
          media = Array.isArray(d.media) ? d.media : [];
          for (const m of media) if (m.path) void localUrl(m.path).catch(() => {});
        }
      } catch { /* no draft */ }
      void tick().then(() => ta?.focus());
    } else if (!s) {
      prevPeer = "";
    }
  });
  let saveTimer: ReturnType<typeof setTimeout> | null = null;
  $effect(() => {
    const s = $articleState;
    const snap = { title, subtitle, body, cover: coverDataUrl, images, media };
    if (!s) return;
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      const empty = !snap.title && !snap.subtitle && !snap.body && !snap.cover && !snap.images.length && !snap.media.length;
      try {
        if (empty) localStorage.removeItem(draftKey(s.peerId));
        else localStorage.setItem(draftKey(s.peerId), JSON.stringify(snap));
      } catch {
        // Photos can exceed the storage quota: keep the text at least.
        try { localStorage.setItem(draftKey(s.peerId), JSON.stringify({ ...snap, images: [], cover: null })); } catch { /* ignore */ }
      }
    }, 500);
  });
  function dropDraft(peer: string) { try { localStorage.removeItem(draftKey(peer)); } catch { /* ignore */ } }

  // ---------------------------------------------------------------- editing
  /** Replace the selection, keeping the textarea's undo history. */
  function replaceSel(text: string, selectFrom?: number, selectTo?: number) {
    if (!ta) return;
    ta.focus();
    const start = ta.selectionStart;
    const ok = document.execCommand?.("insertText", false, text);
    if (!ok) {
      ta.setRangeText(text, ta.selectionStart, ta.selectionEnd, "end");
      body = ta.value;
    }
    if (selectFrom != null) ta.setSelectionRange(start + selectFrom, start + (selectTo ?? selectFrom));
  }

  function wrap(before: string, after = before, placeholder = "") {
    if (!ta) return;
    const sel = ta.value.slice(ta.selectionStart, ta.selectionEnd);
    // Toggle off when the selection is already wrapped.
    const s = ta.selectionStart, e = ta.selectionEnd;
    if (sel && ta.value.slice(s - before.length, s) === before && ta.value.slice(e, e + after.length) === after) {
      ta.setSelectionRange(s - before.length, e + after.length);
      replaceSel(sel, 0, sel.length);
      return;
    }
    if (sel && /^\s|\s$/.test(sel)) {
      // "word " → "**word** " (markers next to spaces don't parse).
      const lead = sel.match(/^\s*/)?.[0] ?? "";
      const trail = sel.match(/\s*$/)?.[0] ?? "";
      const core = sel.slice(lead.length, sel.length - trail.length);
      if (!core) return;
      replaceSel(lead + before + core + after + trail, lead.length + before.length, lead.length + before.length + core.length);
      return;
    }
    const inner = sel || placeholder;
    replaceSel(before + inner + after, before.length, before.length + inner.length);
  }

  /** Select whole lines touched by the selection. */
  function selectLines() {
    if (!ta) return;
    const v = ta.value;
    const s = v.lastIndexOf("\n", ta.selectionStart - 1) + 1;
    let e = v.indexOf("\n", ta.selectionEnd);
    if (e < 0) e = v.length;
    ta.setSelectionRange(s, e);
  }

  function prefixLines(make: (i: number) => string, strip: RegExp) {
    if (!ta) return;
    selectLines();
    const lines = ta.value.slice(ta.selectionStart, ta.selectionEnd).split("\n");
    const all = lines.every((l) => strip.test(l));
    const out = lines.map((l, i) => (all ? l.replace(strip, "") : make(i) + l.replace(strip, ""))).join("\n");
    replaceSel(out, 0, out.length);
  }

  /** Put a block on its own line(s). */
  function insertBlock(text: string) {
    if (!ta) return;
    const v = ta.value;
    const s = ta.selectionStart;
    const needNlBefore = s > 0 && v[s - 1] !== "\n";
    const needNlAfter = ta.selectionEnd < v.length && v[ta.selectionEnd] !== "\n";
    replaceSel((needNlBefore ? "\n" : "") + text + (needNlAfter ? "\n" : "\n"));
  }

  function addLink() {
    if (!ta) return;
    const s = ta.selectionStart, e = ta.selectionEnd;
    const sel = ta.value.slice(s, e);
    askInput({
      title: tr("Add a link"),
      fields: [
        { label: tr("Text"), value: sel, placeholder: tr("Link text") },
        { label: "URL", placeholder: "https://", monospace: true },
      ],
      submitLabel: tr("Insert"),
      onSubmit: async ([text, url]) => {
        let u = url.trim();
        if (!u) return tr("Enter a link");
        if (!/^(https?:|mailto:)/i.test(u)) u = "https://" + u;
        const label = text.trim() || u;
        ta?.focus();
        ta?.setSelectionRange(s, e);
        replaceSel(`[${label}](${u})`);
        return null;
      },
    });
  }

  type Tool = { key: string; label: string; title: string; run: () => void; sep?: boolean };
  const tools: Tool[] = [
    { key: "b", label: "B", title: "Bold (Ctrl+B)", run: () => wrap("**", "**", tr("bold")) },
    { key: "i", label: "I", title: "Italic (Ctrl+I)", run: () => wrap("__", "__", tr("italic")) },
    { key: "u", label: "U", title: "Underline (Ctrl+U)", run: () => wrap("++", "++", tr("underline")) },
    { key: "s", label: "S", title: "Strikethrough (Ctrl+Shift+X)", run: () => wrap("~~", "~~", tr("strikethrough")) },
    { key: "sp", label: "▒", title: "Spoiler (Ctrl+Shift+P)", run: () => wrap("||", "||", tr("spoiler")) },
    { key: "c", label: "</>", title: "Monospace (Ctrl+Shift+M)", run: () => wrap("`", "`", "code") },
    { key: "a", label: "🔗", title: "Link (Ctrl+K)", run: addLink, sep: true },
    { key: "h1", label: "H1", title: "Heading", run: () => prefixLines(() => "# ", /^#{1,3}\s+/) },
    { key: "h2", label: "H2", title: "Subheading", run: () => prefixLines(() => "## ", /^#{1,3}\s+/) },
    { key: "q", label: "❝", title: "Quote", run: () => prefixLines(() => "> ", /^>\s?/) },
    { key: "pq", label: "“ ”", title: "Pull quote", run: () => prefixLines(() => ">> ", /^>>\s?/) },
    { key: "ul", label: "•", title: "Bulleted list", run: () => prefixLines(() => "- ", /^[-*•]\s+/) },
    { key: "ol", label: "1.", title: "Numbered list", run: () => prefixLines((i) => `${i + 1}. `, /^\d+[.)]\s+/) },
    { key: "pre", label: "{ }", title: "Code block", run: () => { const sel = ta ? ta.value.slice(ta.selectionStart, ta.selectionEnd) : ""; insertBlock("```\n" + (sel || "code") + "\n```"); } },
    { key: "hr", label: "—", title: "Divider", run: () => insertBlock("---") },
  ];

  function onKey(e: KeyboardEvent) {
    const ctrl = e.ctrlKey || e.metaKey;
    if (!ctrl) return;
    const k = e.key.toLowerCase();
    const fire = (key: string) => { e.preventDefault(); tools.find((x) => x.key === key)?.run(); };
    if (k === "enter") { e.preventDefault(); void send(); }
    else if (!e.shiftKey && k === "b") fire("b");
    else if (!e.shiftKey && k === "i") fire("i");
    else if (!e.shiftKey && k === "u") fire("u");
    else if (!e.shiftKey && k === "k") fire("a");
    else if (e.shiftKey && k === "x") fire("s");
    else if (e.shiftKey && k === "p") fire("sp");
    else if (e.shiftKey && k === "m") fire("c");
  }

  // ---------------------------------------------------------------- photos
  async function addBlobs(blobs: Blob[]): Promise<number[]> {
    const room = MAX_PHOTOS - images.length;
    if (room <= 0) { showToast(tr("Up to {n} photos per article", { n: MAX_PHOTOS })); return []; }
    if (blobs.length > room) showToast(tr("Up to {n} photos per article", { n: MAX_PHOTOS }));
    const added: number[] = [];
    adding += Math.min(room, blobs.length);
    for (const b of blobs.slice(0, room)) {
      try {
        const url = await shrinkImage(b);
        images = [...images, url];
        added.push(images.length - 1);
      } catch (e) {
        showToast(tr("Could not add a photo: {e}", { e: String(e) }));
      } finally {
        adding--;
      }
    }
    return added;
  }

  const extOf = (p: string) => (p.split(/[\\/]/).pop() ?? "").split(".").pop()?.toLowerCase() ?? "";
  const nameOf = (p: string) => p.split(/[\\/]/).pop() ?? p;

  /** Streaming URLs of local files (path → URL), for previews. */
  let localUrls = $state<Record<string, string>>({});
  async function localUrl(path: string): Promise<string> {
    const u = await streamUrl(path);
    if (localUrls[path] !== u) localUrls = { ...localUrls, [path]: u };
    return u;
  }

  /**
   * Poster + duration of a picked file. ffmpeg (in Rust, another process)
   * first. On Linux the webview is never asked: WebKitGTK's GStreamer can
   * hang the whole window grabbing a video frame (NVIDIA decoders).
   */
  async function probeLocal(path: string, kind: "video" | "audio"): Promise<{ poster?: string; duration?: number }> {
    const native = await api.probeMediaFile(path, kind).catch(() => null);
    if (native && (native.poster || kind === "audio") && native.duration) return { poster: native.poster ?? undefined, duration: native.duration };
    if (document.documentElement.classList.contains("wk-lowfx")) {
      return { poster: native?.poster ?? undefined, duration: native?.duration ?? undefined };
    }
    return probeMedia(await localUrl(path), kind);
  }

  /** Videos / tracks picked from disk → pending media (uploaded on publish). */
  async function addMediaFiles(paths: string[], kind: "video" | "audio"): Promise<number[]> {
    const room = MAX_MEDIA - media.length;
    if (room <= 0) { showToast(tr("Up to {n} videos and tracks per article", { n: MAX_MEDIA })); return []; }
    const out: number[] = [];
    for (const path of paths.slice(0, room)) {
      // Over the attachment limit: say so now, not after "Publish".
      const pre = await api.probeMediaFile(path, "size").catch(() => null);
      if (pre?.too_big) {
        showToast(tr("{name} is too big: {size} (limit 200 MB)", { name: nameOf(path), size: fmtSize(pre.size) }), 6000);
        continue;
      }
      const item: PendingMedia = { kind, name: nameOf(path), path };
      media = [...media, item];
      const i = media.length - 1;
      out.push(i);
      // Poster + duration in the background. Streamed over the loopback media
      // server: through the asset protocol the webview pulled the whole file
      // in at once and the window froze on big videos.
      void probeLocal(path, kind).then((r) => {
        const cur = media[i];
        if (cur && cur.path === path) media[i] = { ...cur, poster: r.poster, duration: r.duration };
      }).catch(() => {});
    }
    return out;
  }

  /** Photos and videos in one go — they become one carousel, in pick order. */
  async function pickPhotos() {
    try {
      const res = await openFileDialog({
        multiple: true, directory: false, title: tr("Pick photos and videos for a carousel"),
        filters: [{ name: tr("Photos and videos"), extensions: [...IMAGE_EXT, ...VIDEO_EXT] }],
      });
      const paths = Array.isArray(res) ? res : res ? [res] : [];
      if (!paths.length) return;
      const refs: GalleryRef[] = [];
      for (const p of paths) {
        if (VIDEO_EXT.includes(extOf(p))) {
          for (const i of await addMediaFiles([p], "video")) refs.push({ kind: "video", i });
        } else {
          const bytes = await api.readUserImage(p);
          for (const i of await addBlobs([new Blob([new Uint8Array(bytes)])])) refs.push({ kind: "image", i });
        }
      }
      if (refs.length) insertBlock(galleryLine(refs));
    } catch (e) {
      showToast(tr("Could not add a photo: {e}", { e: String(e) }));
    }
  }

  async function pickMusic() {
    try {
      const res = await openFileDialog({
        multiple: true, directory: false, title: tr("Pick music for the article"),
        filters: [{ name: tr("Music"), extensions: AUDIO_EXT }],
      });
      const paths = Array.isArray(res) ? res : res ? [res] : [];
      if (paths.length) await addMediaFiles(paths, "audio");
    } catch (e) {
      showToast(String(e));
    }
  }

  async function onPaste(e: ClipboardEvent) {
    const files = [...(e.clipboardData?.files ?? [])].filter((f) => f.type.startsWith("image/"));
    if (!files.length) return;
    e.preventDefault();
    const idx = await addBlobs(files);
    if (idx.length) insertBlock(`[[gallery:${idx.join(",")}]]`);
  }

  function rewriteGalleries(map: (refs: GalleryRef[]) => GalleryRef[]) {
    body = body.split("\n").flatMap((line) => {
      const m = GALLERY.exec(line.trim());
      if (!m) return [line];
      const refs = map(parseGalleryRefs(m[1]));
      return refs.length ? [galleryLine(refs, m[2] ?? null)] : [];
    }).join("\n");
  }
  function removeImage(r: number) {
    images = images.filter((_, i) => i !== r);
    rewriteGalleries((refs) => refs
      .filter((x) => !(x.kind === "image" && x.i === r))
      .map((x) => (x.kind === "image" && x.i > r ? { ...x, i: x.i - 1 } : x)));
  }
  function moveImage(r: number, d: -1 | 1) {
    const to = r + d;
    if (to < 0 || to >= images.length) return;
    const next = [...images];
    [next[r], next[to]] = [next[to], next[r]];
    images = next;
    rewriteGalleries((refs) => refs.map((x) => (x.kind !== "image" ? x : x.i === r ? { ...x, i: to } : x.i === to ? { ...x, i: r } : x)));
  }
  function removeMedia(r: number) {
    media = media.filter((_, i) => i !== r);
    rewriteGalleries((refs) => refs
      .filter((x) => !(x.kind === "video" && x.i === r))
      .map((x) => (x.kind === "video" && x.i > r ? { ...x, i: x.i - 1 } : x)));
  }

  async function pickCover() {
    try {
      const result = await openFileDialog({
        multiple: false, directory: false, title: tr("Pick a cover image"),
        filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp"] }],
      });
      const path = Array.isArray(result) ? (result[0] ?? null) : result;
      if (!path) return;
      const bytes = await api.readUserImage(path);
      const src = URL.createObjectURL(new Blob([new Uint8Array(bytes)]));
      const blob = await cropImage(src, { aspect: 16 / 9, outWidth: 1280, title: $t("Position the cover") });
      URL.revokeObjectURL(src);
      if (!blob) return;
      coverDataUrl = bytesToDataUrl(Array.from(new Uint8Array(await blob.arrayBuffer())), blob.type || "image/webp");
    } catch (e) {
      showToast(`Could not load cover: ${e}`);
    }
  }

  // ---------------------------------------------------------------- publish
  let usedRefs = $derived.by(() => {
    const s = new Set<string>();
    for (const line of body.split("\n")) {
      const m = GALLERY.exec(line.trim());
      if (m) for (const r of parseGalleryRefs(m[1])) s.add(r.kind + r.i);
    }
    return s;
  });
  let used = $derived(new Set([...usedRefs].filter((k) => k.startsWith("image")).map((k) => parseInt(k.slice(5), 10))));
  /** What the preview shows for media that isn't uploaded yet. */
  let previewMedia = $derived(media.map((m): ArticleMedia => m.ref ?? {
    kind: m.kind, cid: "local-preview", key_b64: "", nonce_b64: "", name: m.name, size: 0, mime: null, poster: m.poster, duration: m.duration,
  }));
  let previewLocal = $derived(Object.fromEntries(media.map((m, i) => [i, m.path ? localUrls[m.path] ?? "" : ""]).filter(([, u]) => u)));
  let bytes = $derived(images.reduce((n, u) => n + dataUrlBytes(u), 0) + (coverDataUrl ? dataUrlBytes(coverDataUrl) : 0) + body.length * 2);
  let blocks = $derived(parseBlocks(body, "article"));
  let words = $derived(wordCount(blocks));
  let readMin = $derived(Math.max(1, Math.round(words / 200)));

  function mb(n: number) { return `${(n / 1024 / 1024).toFixed(1)} MB`; }

  async function send() {
    const s = $articleState;
    if (!s || sending) return;
    if (!title.trim()) { showToast(tr("Give the article a title first")); return; }
    if (!body.trim() && images.length === 0) { showToast(tr("Write something first")); return; }
    if (bytes > MAX_BYTES) { showToast(tr("The article is too big ({size}) — remove some photos", { size: mb(bytes) }), 6000); return; }
    const chat = $chats.find((c) => c.id === s.peerId);
    if (chat && !chat.live && !s.peerId.startsWith("grp_") && s.peerId !== "__saved__") {
      showToast(tr("This chat isn't connected yet — start a real chat first"));
      return;
    }
    // Photos / videos that aren't in any carousel go at the end, so nothing is lost.
    const orphans: GalleryRef[] = [
      ...images.map((_, i) => ({ kind: "image" as const, i })).filter((r) => !usedRefs.has("image" + r.i)),
      ...media.map((m, i) => ({ kind: m.kind, i })).filter((r) => r.kind === "video" && !usedRefs.has("video" + r.i)) as GalleryRef[],
    ];
    const finalBody = body.trim() + (orphans.length ? `\n\n${galleryLine(orphans)}` : "");
    sending = true;
    // Videos and music go up as encrypted files first; the article only
    // carries their keys.
    const refs: ArticleMedia[] = [];
    try {
      for (let i = 0; i < media.length; i++) {
        const m = media[i];
        if (!m.ref) {
          uploading = tr("Uploading {what} {i} of {n}…", { what: m.kind === "video" ? tr("video") : tr("track"), i: i + 1, n: media.length });
          if (!m.path) throw new Error(tr("{name}: the file is missing", { name: m.name }));
          const ref = await api.uploadAttachment(s.peerId, m.path);
          media[i] = { ...m, ref: { ...ref, poster: m.poster ?? null, duration: m.duration ?? null } };
        }
        refs.push(media[i].ref!);
      }
    } catch (e) {
      uploading = "";
      sending = false;
      showToast(tr("Could not upload: {e}", { e: String(e) }), 7000);
      return;
    }
    uploading = "";
    const payload = JSON.stringify({
      title: title.trim(),
      subtitle: subtitle.trim() || null,
      body: finalBody,
      cover_data_url: coverDataUrl,
      images,
      media: refs,
    });
    const envelope = `[article:v1]${payload}`;
    try {
      if (s.peerId.startsWith("grp_")) await api.groupSend(s.peerId, envelope);
      else await api.sendMessage(s.peerId, envelope);
      dropDraft(s.peerId);
      title = ""; subtitle = ""; body = ""; coverDataUrl = null; images = []; media = [];
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
    // Capture phase: decide before a nested dialog closes itself on the same Esc.
    const h = (e: KeyboardEvent) => {
      if (!$articleState || e.key !== "Escape") return;
      if (getStore(promptOpen) || getStore(gallery) || document.querySelector('div.backdrop[role="presentation"]')) return;
      closeArticleComposer();
    };
    window.addEventListener("keydown", h, true);
    return () => window.removeEventListener("keydown", h, true);
  });
</script>

{#if $articleState}
  <div
    class="backdrop"
    role="button"
    tabindex="-1"
    aria-label={$t("Close composer")}
    onclick={(e) => { if ((e.target as HTMLElement).classList.contains("backdrop")) closeArticleComposer(); }}
    onkeydown={() => {}}
  >
    <div class="sheet" role="dialog" aria-modal="true" tabindex="-1">
      <div class="head">
        <h2>{$t("Write an article")}</h2>
        <div class="segmented">
          <button class:active={!preview} onclick={() => (preview = false)}>{$t("Edit")}</button>
          <button class:active={preview}  onclick={() => (preview = true)}>{$t("Preview")}</button>
        </div>
        <button class="close" onclick={closeArticleComposer} title={$t("Close (Esc) — the draft is kept")}>×</button>
      </div>

      {#if !preview}
        <div class="body">
          <div class="cover-row">
            {#if coverDataUrl}
              <div class="cover" style={`background-image: url(${coverDataUrl});`}>
                <button class="cover-remove" onclick={() => (coverDataUrl = null)} title={$t("Remove cover")}>×</button>
                <button class="cover-change" onclick={pickCover}>{$t("Change")}</button>
              </div>
            {:else}
              <button class="cover-pick" onclick={pickCover}>+ {$t("Add cover image")}</button>
            {/if}
          </div>
          <input class="title-input" bind:value={title} placeholder={$t("Title")} maxlength="140" />
          <input class="sub-input" bind:value={subtitle} placeholder={$t("Subtitle (optional)")} maxlength="240" />

          <div class="toolbar" role="toolbar" aria-label={$t("Formatting")}>
            {#each tools as tl (tl.key)}
              {#if tl.sep}<span class="tsep"></span>{/if}
              <button class="tool t-{tl.key}" title={$t(tl.title)} onmousedown={(e) => e.preventDefault()} onclick={tl.run}>{tl.label}</button>
            {/each}
            <span class="tsep"></span>
            <button class="tool photos" title={$t("Add photos and videos — they become a swipeable carousel")} onmousedown={(e) => e.preventDefault()} onclick={pickPhotos}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="5" width="18" height="14" rx="2"/><circle cx="8.5" cy="10" r="1.5"/><path d="m21 15-5-5L5 19"/></svg>
              {$t("Photo / video")}
            </button>
            <button class="tool photos" title={$t("Attach music — it plays while people read")} onmousedown={(e) => e.preventDefault()} onclick={pickMusic}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/></svg>
              {$t("Music")}
            </button>
          </div>

          <textarea
            bind:this={ta}
            class="body-input"
            bind:value={body}
            onkeydown={onKey}
            onpaste={onPaste}
            placeholder={$t("Tell your story… Select text and use the toolbar, or type **bold**, __italic__, > quote, # heading. Paste or add photos to make a carousel.")}
          ></textarea>

          {#if images.length || adding || media.length}
            <div class="tray">
              <div class="tray-head">
                <span>{$t("Attachments")} · {images.length + media.length}{adding ? ` (+${adding}…)` : ""}</span>
                <span class="tray-hint">{$t("Click a photo to put it into the text")}</span>
              </div>
              <div class="tray-list">
                {#each images as src, i (src.length + ":" + i)}
                  <div class="ph" class:orphan={!used.has(i)}>
                    <button class="ph-img" onclick={() => insertBlock(`[[gallery:${i}]]`)} title={$t("Insert into the text")}><img {src} alt="" /></button>
                    <span class="ph-n">{i}</span>
                    <button class="ph-x" onclick={() => removeImage(i)} title={$t("Remove")}>×</button>
                    <div class="ph-move">
                      <button onclick={() => moveImage(i, -1)} disabled={i === 0} aria-label="←">‹</button>
                      <button onclick={() => moveImage(i, 1)} disabled={i === images.length - 1} aria-label="→">›</button>
                    </div>
                  </div>
                {/each}
                {#each Array(adding) as _, k (k)}<div class="ph loading"></div>{/each}
                {#each media as m, i (m.name + ":" + i)}
                  {#if m.kind === "video"}
                    <div class="ph vid" class:orphan={!usedRefs.has("video" + i)}>
                      <button class="ph-img" onclick={() => insertBlock(galleryLine([{ kind: "video", i }]))} title={$t("Insert into the text")}>
                        {#if m.poster}<img src={m.poster} alt="" />{:else}<span class="ph-icon">🎬</span>{/if}
                        <span class="ph-play">▶</span>
                      </button>
                      <span class="ph-n">v{i}{m.duration ? " · " + fmtDuration(m.duration) : ""}</span>
                      <button class="ph-x" onclick={() => removeMedia(i)} title={$t("Remove")}>×</button>
                    </div>
                  {:else}
                    <div class="ph aud" title={m.name}>
                      <span class="ph-icon">🎵</span>
                      <span class="aud-name">{m.name}</span>
                      <button class="ph-x" onclick={() => removeMedia(i)} title={$t("Remove")}>×</button>
                    </div>
                  {/if}
                {/each}
              </div>
            </div>
          {/if}
        </div>
      {:else}
        <div class="preview">
          {#if coverDataUrl}<div class="pv-cover" style={`background-image: url(${coverDataUrl});`}></div>{/if}
          <article class="article-prose">
            <h1>{title || $t("Untitled")}</h1>
            {#if subtitle}<p class="pv-sub">{subtitle}</p>{/if}
            <div class="pv-meta">{$t("{n} min read", { n: readMin })}{images.length ? ` · ${$t("{n} photos", { n: images.length })}` : ""}</div>
            {#if media.some((m) => m.kind === "audio")}
              <div class="pv-music">🎵 {media.filter((m) => m.kind === "audio").map((m) => m.name).join(" · ")}</div>
            {/if}
            <Markup {blocks} {images} media={previewMedia} localMedia={previewLocal} mode="article" />
          </article>
        </div>
      {/if}

      <div class="foot">
        <span class="hint" class:warn={bytes > WARN_BYTES}>
          {#if uploading}<b class="up">{uploading}</b>{:else}{$t("{n} words", { n: words })} · ≈{readMin} {$t("min")} · {mb(bytes)}{#if media.length} · 🎬🎵 {media.length}{/if}{/if}
          <span class="kb">· Ctrl+Enter — {$t("publish")}</span>
        </span>
        <div>
          <button class="btn-ghost" onclick={closeArticleComposer} disabled={sending}>{$t("Cancel")}</button>
          <button class="btn-primary" onclick={send} disabled={sending || adding > 0}>
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
    padding: calc(var(--tb-h, 0px) + 12px) 20px 20px;
  }
  .sheet {
    width: min(760px, 96vw);
    height: min(900px, calc(100vh - var(--tb-h, 0px) - 32px));
    display: flex; flex-direction: column;
    background: var(--bg-shell);
    border: 1px solid var(--border);
    border-radius: 16px;
    box-shadow: 0 32px 80px -16px rgba(0,0,0,0.6);
    overflow: hidden;
    animation: slide-up 0.22s ease-out;
  }
  @keyframes slide-up { from { opacity: 0; transform: translateY(12px); } to { opacity: 1; transform: none; } }
  .head {
    padding: 12px 18px;
    display: grid; grid-template-columns: 1fr auto auto; gap: 12px; align-items: center;
    border-bottom: 1px solid var(--border);
  }
  .head h2 { margin: 0; font-size: 15.5px; font-weight: 700; }
  .segmented { display: flex; background: var(--bg-input); border: 1px solid var(--border); border-radius: 10px; padding: 3px; gap: 2px; }
  .segmented button { background: transparent; border: none; padding: 5px 12px; border-radius: 7px; color: var(--fg-muted); font-weight: 600; font-size: 12.5px; cursor: pointer; }
  .segmented button.active { background: var(--accent-soft); color: var(--accent); }
  .close { width: 30px; height: 30px; background: transparent; border: 1px solid var(--border); border-radius: 8px; color: var(--fg-dim); font-size: 18px; cursor: pointer; }

  .body { flex: 1; min-height: 0; padding: 16px 18px; display: flex; flex-direction: column; gap: 8px; overflow-y: auto; }
  .cover-row { display: flex; }
  .cover { width: 100%; aspect-ratio: 16 / 6; border-radius: 12px; background-size: cover; background-position: center; position: relative; flex-shrink: 0; }
  .cover-remove { position: absolute; top: 8px; right: 8px; width: 28px; height: 28px; background: rgba(0,0,0,0.55); border: none; border-radius: 50%; color: #fff; font-size: 16px; cursor: pointer; }
  .cover-change { position: absolute; bottom: 8px; right: 8px; padding: 4px 10px; background: rgba(0,0,0,0.55); border: none; border-radius: 8px; color: #fff; font-size: 12px; font-weight: 600; cursor: pointer; }
  .cover-pick { width: 100%; padding: 14px; background: var(--bg-input); border: 1px dashed var(--border-strong); border-radius: 12px; color: var(--fg-dim); font-size: 13px; cursor: pointer; }
  .cover-pick:hover { color: var(--accent); border-color: var(--accent); }

  .title-input, .sub-input { background: transparent; border: none; outline: none; font-family: inherit; color: var(--fg); padding: 4px 0; }
  .title-input { font-size: 24px; font-weight: 800; letter-spacing: -0.01em; }
  .sub-input { font-size: 15px; color: var(--fg-muted); border-bottom: 1px solid var(--border); padding-bottom: 8px; }
  .title-input::placeholder, .sub-input::placeholder { color: var(--fg-dim); }
  .sub-input:focus { border-bottom-color: var(--accent); }

  .toolbar {
    position: sticky; top: -16px; z-index: 2;
    display: flex; flex-wrap: wrap; align-items: center; gap: 3px;
    padding: 5px; border-radius: 10px;
    background: var(--bg-list); border: 1px solid var(--border);
  }
  .tool {
    min-width: 30px; height: 28px; padding: 0 7px;
    display: inline-flex; align-items: center; justify-content: center; gap: 5px;
    border: none; border-radius: 7px; background: transparent;
    color: var(--fg-muted); font-size: 13px; font-weight: 700; cursor: pointer;
  }
  .tool:hover { background: var(--bg-hover); color: var(--fg); }
  .tool svg { width: 16px; height: 16px; }
  .t-i { font-style: italic; font-family: Georgia, serif; }
  .t-u { text-decoration: underline; }
  .t-s { text-decoration: line-through; }
  .t-c, .t-pre { font-family: var(--font-mono); font-size: 11.5px; }
  .tool.photos { color: var(--accent); }
  .tsep { width: 1px; height: 18px; background: var(--border-strong); margin: 0 3px; }

  .body-input {
    flex: 1; min-height: 260px;
    background: var(--bg-input); border: 1px solid var(--border); border-radius: 10px; outline: none;
    color: var(--fg); font-family: inherit; font-size: 14.5px; line-height: 1.6;
    padding: 12px 14px; resize: none;
  }
  .body-input:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }

  .tray { border: 1px solid var(--border); border-radius: 10px; padding: 8px; background: var(--bg-list); }
  .tray-head { display: flex; justify-content: space-between; gap: 8px; font-size: 12px; font-weight: 700; color: var(--fg-muted); margin-bottom: 6px; }
  .tray-hint { font-weight: 500; color: var(--fg-dim); }
  .tray-list { display: flex; gap: 8px; overflow-x: auto; padding-bottom: 2px; }
  .ph { position: relative; flex: 0 0 96px; height: 72px; border-radius: 8px; overflow: hidden; background: var(--bg-input); }
  .ph.orphan { outline: 2px dashed var(--border-strong); outline-offset: -2px; }
  .ph.loading { background: linear-gradient(90deg, var(--bg-input), var(--bg-hover), var(--bg-input)); }
  .ph-img { all: unset; cursor: pointer; display: block; width: 100%; height: 100%; }
  .ph-img img { width: 100%; height: 100%; object-fit: cover; display: block; }
  .ph-n { position: absolute; left: 4px; top: 4px; padding: 0 5px; border-radius: 6px; background: rgba(0,0,0,0.6); color: #fff; font-size: 10.5px; font-weight: 700; }
  .ph-x { position: absolute; right: 3px; top: 3px; width: 20px; height: 20px; border-radius: 50%; border: none; background: rgba(0,0,0,0.6); color: #fff; cursor: pointer; font-size: 13px; line-height: 1; }
  .ph.vid .ph-play { position: absolute; inset: 0; display: grid; place-items: center; color: #fff; font-size: 18px; text-shadow: 0 0 6px #000; pointer-events: none; }
  .ph-icon { display: grid; place-items: center; width: 100%; height: 100%; font-size: 26px; }
  .ph.aud { flex-basis: 150px; display: flex; align-items: center; gap: 6px; padding: 0 26px 0 8px; }
  .ph.aud .ph-icon { width: auto; height: auto; font-size: 20px; }
  .aud-name { font-size: 11.5px; color: var(--fg-muted); overflow: hidden; display: -webkit-box; -webkit-line-clamp: 3; line-clamp: 3; -webkit-box-orient: vertical; word-break: break-all; }
  .pv-music { font-size: 13px; color: var(--fg-muted); padding: 8px 12px; border: 1px solid var(--border); border-radius: 10px; margin-bottom: 14px; }
  .up { color: var(--accent); font-weight: 700; }
  .ph-move { position: absolute; left: 0; right: 0; bottom: 0; display: flex; justify-content: space-between; opacity: 0; transition: opacity 0.15s; }
  .ph:hover .ph-move { opacity: 1; }
  .ph-move button { border: none; background: rgba(0,0,0,0.6); color: #fff; width: 22px; height: 20px; cursor: pointer; font-weight: 700; }
  .ph-move button:disabled { opacity: 0.3; cursor: default; }

  .preview { flex: 1; overflow-y: auto; background: var(--bg-shell); }
  .pv-cover { width: 100%; aspect-ratio: 16 / 7; background-size: cover; background-position: center; }
  .article-prose { max-width: 660px; margin: 0 auto; padding: 26px 28px; color: var(--fg); font-size: 16px; line-height: 1.65; }
  .article-prose h1 { font-size: 28px; font-weight: 800; margin: 0 0 6px; letter-spacing: -0.02em; }
  .pv-sub { font-size: 17px; color: var(--fg-muted); margin: 0 0 6px; }
  .pv-meta { font-size: 12.5px; color: var(--fg-dim); margin-bottom: 20px; }

  .foot { padding: 12px 18px; display: flex; justify-content: space-between; align-items: center; gap: 10px; border-top: 1px solid var(--border); background: var(--bg-list); }
  .hint { font-size: 11.5px; color: var(--fg-dim); }
  .hint.warn { color: var(--warning, #f5a524); }
  .foot > div { display: flex; gap: 10px; }
  .btn-ghost, .btn-primary { padding: 8px 18px; border-radius: 10px; font-size: 13px; font-weight: 700; cursor: pointer; border: none; }
  .btn-ghost { background: transparent; border: 1px solid var(--border); color: var(--fg-muted); }
  .btn-ghost:hover:not(:disabled) { color: var(--fg); border-color: var(--border-strong); }
  .btn-primary { background: linear-gradient(135deg, var(--accent), var(--accent-2)); color: #06070C; box-shadow: 0 6px 16px -4px var(--accent-glow); }
  .btn-primary:hover:not(:disabled) { transform: translateY(-1px); }
  button:disabled { opacity: 0.5; cursor: not-allowed; }
  @media (max-width: 640px) { .kb { display: none; } }
</style>
