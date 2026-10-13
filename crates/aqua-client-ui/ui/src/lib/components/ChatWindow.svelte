<script lang="ts">
  import { tick, onMount, untrack } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import { askConfirm } from "./Confirm.svelte";
  import { askSend, VIDEO_RE, type SendMode } from "./SendDialog.svelte";
  import { shrinkImage } from "../imageTools";
  import { applyThemeFromPath } from "../themeShare";
  import { openFileDialog } from "../dialog";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { icons } from "../icons";
  import { activeChat, activeMessages, drafts, activeId, sendCurrent, loadActiveMessages, info, chats } from "../stores";
  import { settings } from "../theme";
  import { api, assetUrl, sendChatControl } from "../tauri";
  import { openFingerprint } from "./FingerprintModal.svelte";
  import { showToast } from "./Toast.svelte";
  import { startOutgoingCall, callState } from "./CallModal.svelte";
  import { openMediaPicker, closeMediaPicker, pickerState, panelClaimsDrop } from "./MediaPicker.svelte";
  import AnimatedMedia from "./AnimatedMedia.svelte";
  import RichComposer from "./RichComposer.svelte";
  import { openProfile } from "./ProfileModal.svelte";
  import { reactionPack, quickReactions, parseReaction, reactionUrl, loadReactionPack, REACTION_PACK, emojiPacks, reactionSource, loadEmojiPacks } from "../reactionPack";
  import { editAndSend } from "./PhotoEditor.svelte";
  import MediaPlayer from "./MediaPlayer.svelte";
  import { openGallery, type GalleryItem } from "./Lightbox.svelte";
  import { openArticleComposer } from "./ArticleComposer.svelte";
  import { openCircleRecorder } from "./CircleRecorder.svelte";
  import { openPollComposer } from "./PollComposer.svelte";
  import { openForward, type ForwardItem } from "./ForwardPicker.svelte";
  import { playQueue, type Track } from "../player";
  import { toggleSharedMedia } from "./SharedMediaPanel.svelte";
  import { backToList } from "../stores";
  import { events } from "../tauri";
  import { pollStates, castVote, retractVote, applyRemoteVote, closePoll, viewPoll, ME as POLL_ME, type PollView } from "../polls";
  import { peerPresence, setPeerPresence, broadcastPresenceTo, activityVerb, chatOnline, peerStatus, presenceNow } from "../presence";
  import { notePeerProfileVersion, avatars, clans } from "../people";
  import ClanTag from "./ClanTag.svelte";
  import { keepOnScreen } from "../actions";
  import { ghost } from "../ghost";
  import GroupCallPanel from "./GroupCallPanel.svelte";
  import { groupCall, activeGroupCalls, joinGroupCall } from "../groupCall";
  import { postBlock, tagOf, canSendKind, isAdmin, can } from "../groupRules";
  import { t, tr, tn } from "../i18n";
  import { mutedChats, setMuted } from "../chatPrefs";
  import { insertToComposer } from "../composerBus";
  import { groupMap } from "../groups";
  import Markup from "./Markup.svelte";
  import LinkPreview from "./LinkPreview.svelte";
  import Carousel, { type CarouselItem } from "./Carousel.svelte";
  import ArticleMusic from "./ArticleMusic.svelte";
  import { hasMarkup, linksIn, excerptOf, parseBlocks, wordCount } from "../markup";
  import { objectUrl, mediaUrl, streamSrc, gifStill } from "../media";
  import { openArticleReader } from "../reader";
  import { shortcutsOpen } from "../shortcuts";
  import { openGroupPanel } from "./GroupPanel.svelte";
  import { reactions, toggleMyReaction, applyPeerReaction, reactionChips, hashContent, pins, pinMessage, unpinMessage, pinnedId } from "../messageMeta";
  import { openNewChatDialog } from "../newChat";

  import { contacts, getContact } from "../contacts";
  import type { ChatMessage } from "../types";

  // Inline media now streams straight from disk via Tauri's asset
  // protocol — instant, no IPC byte-copy, no base64. (Previously each
  // attachment was read as a Vec<u8> over IPC and base64'd, which was slow
  // and memory-heavy for anything large.)
  function inlineUrl(path: string | null, _mime: string | null): string | null {
    return path ? assetUrl(path) : null;
  }

  // ---- Inline custom emoji ----
  // A `[ce:<packSlug>/<filename>]` token in message text renders as the
  // matching image from the local sticker/emoji library (if present).
  // "slug/filename" -> { url, mime }. Packs imported from Telegram are also
  // reachable by their Telegram short name, and the app-bundled reaction
  // pack by "eltaller47/<file>", so tokens resolve the same for everyone.
  let emojiMap = $state<Record<string, { url: string; mime: string; path?: string }>>({});
  async function loadEmojiMap() {
    try {
      const list = await api.listLocalMedia();
      const map: Record<string, { url: string; mime: string; path?: string }> = {};
      for (const it of $reactionPack.items) map[`${REACTION_PACK}/${it.file}`] = { url: it.url, mime: it.mime };
      for (const pack of list.packs) {
        const slug = pack.id.replace(/^pack:/, "");
        for (const it of pack.items) {
          const fname = it.path.split(/[\\/]/).pop() ?? "";
          if (!fname) continue;
          const v = { url: assetUrl(it.path), mime: it.mime, path: it.path };
          map[`${slug}/${fname}`] = v;
          if (pack.source) map[`${pack.source}/${fname}`] = v;
        }
      }
      emojiMap = map;
    } catch { /* ignore */ }
  }
  $effect(() => { void $reactionPack; void loadEmojiMap(); });
  /** Fallback for a custom emoji we don't have: the emoji in its file name. */
  function ceFallback(key: string): string {
    const f = key.split("/").pop() ?? "";
    const i = f.indexOf("_");
    return i > 0 ? f.slice(0, i) : "▫";
  }
  type RichPart = { t: "text"; v: string } | { t: "emoji"; key: string };
  function richParts(str: string): RichPart[] {
    const out: RichPart[] = [];
    const re = /\[ce:([^\]\s]+)\]/g;
    let last = 0; let m: RegExpExecArray | null;
    while ((m = re.exec(str))) {
      if (m.index > last) out.push({ t: "text", v: str.slice(last, m.index) });
      out.push({ t: "emoji", key: m[1] });
      last = re.lastIndex;
    }
    if (last < str.length) out.push({ t: "text", v: str.slice(last) });
    return out;
  }

  // Returns the effective render kind for a file message.
  /** All viewable photos / videos of the open chat, for the gallery viewer. */
  function chatGallery(): GalleryItem[] {
    const out: GalleryItem[] = [];
    for (const m of rows) {
      if (m.kind.type !== "file" || !m.kind.content.saved_at) continue;
      const c = m.kind.content;
      const rk = renderKind(c);
      if (rk !== "image" && rk !== "video" && rk !== "gif") continue;
      out.push({
        url: assetUrl(c.saved_at!), kind: rk === "video" ? "video" : "image", name: c.name,
        path: c.saved_at, size: c.size ?? undefined, from: m.outgoing ? $t("You") : m.from, time: m.time,
      });
    }
    return out;
  }
  function openInGallery(path: string | null) {
    const items = chatGallery();
    const i = Math.max(0, items.findIndex((x) => x.path === path));
    if (items.length) openGallery(items, i);
  }
  /** Every audio file of the chat as a queue, starting at `path`. */
  function playChatAudio(path: string | null) {
    const tracks: (Track & { path: string })[] = [];
    for (const m of rows) {
      if (m.kind.type !== "file" || !m.kind.content.saved_at) continue;
      if (renderKind(m.kind.content) !== "audio") continue;
      tracks.push({ src: assetUrl(m.kind.content.saved_at), title: m.kind.content.name, sub: $activeChat?.name ?? "", path: m.kind.content.saved_at });
    }
    const i = Math.max(0, tracks.findIndex((x) => x.path === path));
    if (tracks.length) playQueue(tracks, i);
  }

  function renderKind(content: {
    category: string | null;
    mime: string | null;
    name: string;
  }): "sticker" | "gif" | "image" | "video" | "audio" | "voice" | "circle" | "file" {
    const cat = (content.category ?? "").toLowerCase();
    if (cat === "sticker") return "sticker";
    if (cat === "gif") return "gif";
    if (cat === "voice") return "voice";
    if (cat === "circle") return "circle";
    if (cat === "document") return "file"; // "send as file": never inline
    const mime = (content.mime ?? "").toLowerCase();
    const name = content.name.toLowerCase();
    if (mime === "image/gif" || name.endsWith(".gif")) return "gif";
    if (mime.startsWith("image/")) return "image";
    if (mime.startsWith("video/")) return "video";
    if (mime.startsWith("audio/") || /\.(mp3|wav|ogg|flac|m4a|opus|aac)$/.test(name)) return "audio";
    return "file";
  }

  // Auto-download anything small or any sticker/gif, so the picker and
  // the inline bubble get a local path to render from.
  const AUTO_DOWNLOAD_LIMIT = 3 * 1024 * 1024; // 3 MB — covers most voice notes / images
  const AUTO_DOWNLOAD_MEDIA_LIMIT = 25 * 1024 * 1024; // bigger cap for inline media kinds
  const autoTriggered = new Set<string>();

  $effect(() => {
    const chat = $activeChat;
    const msgs = $activeMessages;
    if (!chat) return;
    if (!$settings.autoDownloadMedia) return;
    for (const m of msgs) {
      if (m.kind.type !== "file") continue;
      if (m.from === "me") continue;
      if (m.kind.content.saved_at) continue;
      if (autoTriggered.has(m.id)) continue;
      const kind = renderKind(m.kind.content);
      const size = m.kind.content.size;
      // Media that renders inline (sticker, gif, image, audio, voice,
      // video, circle) auto-downloads up to a generous cap so it plays
      // / shows without a manual click. Plain files only auto-download
      // when small.
      const isInlineMedia = kind !== "file";
      const within = isInlineMedia ? size <= AUTO_DOWNLOAD_MEDIA_LIMIT : size <= AUTO_DOWNLOAD_LIMIT;
      if (within) {
        autoTriggered.add(m.id);
        void autoDownload(chat.id, m.id);
      }
    }
  });

  async function autoDownload(peerId: string, messageId: string) {
    try {
      await api.downloadFile(peerId, messageId);
      await loadActiveMessages();
    } catch (e) {
      // Non-fatal — user can still click "Download & decrypt".
      console.warn("auto-download failed", messageId, e);
    }
  }

  /** Open the info / settings sheet for whatever chat is open. */
  function openChatInfo() {
    const c = $activeChat;
    if (!c || c.id === "__saved__") return;
    if (c.id.startsWith("grp_")) openGroupPanel(c.id);
    else openProfile(c.id);
  }

  function startVoiceCall() {
    const chat = $activeChat;
    if (!chat || !chat.live) {
      showToast(tr("No live session — connect and start a chat first"));
      return;
    }
    if ($callState.kind !== "idle") {
      showToast(tr("Already in a call"));
      return;
    }
    startOutgoingCall(chat.id);
  }

  async function pickAndSendFile(preset?: SendMode) {
    const chat = $activeChat;
    if (!chat) {
      showToast(tr("Pick a chat on the left first"));
      return;
    }
    if (!chat.live) {
      showToast(tr("This isn't a real chat yet — click the pencil icon ✎ next to the search bar to start one with another user"), 5000);
      return;
    }
    if (chat.id.startsWith("grp_") && !canSendKind(activeGroup, myUsername, "media")) {
      showToast(tr("Admins don't allow media in this chat"));
      return;
    }
    let picked: string[] = [];
    try {
      const result = await openFileDialog({
        multiple: true,
        directory: false,
        title: "Attach files",
        filters: [
          { name: "All files", extensions: ["*"] },
          { name: "Images", extensions: ["png", "jpg", "jpeg", "webp", "gif", "bmp", "svg"] },
          { name: "Videos", extensions: ["mp4", "webm", "mov", "mkv", "avi", "m4v"] },
          { name: "Audio",  extensions: ["mp3", "wav", "ogg", "flac", "m4a", "opus", "aac"] },
          { name: "Documents", extensions: ["pdf", "docx", "doc", "xlsx", "xls", "pptx", "ppt", "txt", "md", "rtf", "odt"] },
          { name: "Archives", extensions: ["zip", "7z", "rar", "tar", "gz"] },
        ],
      });
      if (Array.isArray(result)) picked = result;
      else if (result) picked = [result];
    } catch (e) {
      console.error(e);
      showToast(tr("Could not open file picker"));
      return;
    }
    if (picked.length === 0) return;
    await sendFiles(chat.id, picked, preset);
  }

  // Voice-message recorder. Uses MediaRecorder → Blob → base64 →
  // Rust writes the bytes to a temp file → routed through attach_file
  // with category="voice" so the receiver renders an audio bubble
  // labelled "Voice message".
  let recording = $state(false);
  let recordPaused = $state(false);
  let recorder: MediaRecorder | null = null;
  let recordedChunks: Blob[] = [];
  let recordStartMs = 0;
  let pauseStartMs = 0;
  let recordElapsed = $state(0);
  let recordTimer: ReturnType<typeof setInterval> | null = null;

  function togglePauseRecord() {
    if (!recorder || !recording) return;
    if (recordPaused) {
      try { recorder.resume(); } catch { /* ignore */ }
      recordStartMs += Date.now() - pauseStartMs; // exclude paused time
      recordPaused = false;
    } else {
      try { recorder.pause(); } catch { /* ignore */ }
      pauseStartMs = Date.now();
      recordPaused = true;
    }
  }

  function stopRecording() {
    try { recorder?.stop(); } catch { /* ignore */ }
  }
  async function toggleRecord() {
    if (recording) { stopRecording(); return; }
    await startRecording();
  }
  async function startRecording() {
    const chat = $activeChat;
    if (!chat || !chat.live) {
      showToast(tr("Can't record here — start a real chat first"));
      return;
    }
    if (voiceBlocked(chat.id)) return;
    if (recording) return;
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      recordedChunks = [];
      // Prefer opus in webm — tiny files and good quality.
      const mimeCandidates = ["audio/webm;codecs=opus", "audio/webm", "audio/ogg;codecs=opus", "audio/ogg"];
      const mimeType = mimeCandidates.find((m) => MediaRecorder.isTypeSupported(m)) ?? "";
      recorder = new MediaRecorder(stream, mimeType ? { mimeType } : undefined);
      recorder.ondataavailable = (e) => { if (e.data.size > 0) recordedChunks.push(e.data); };
      recorder.onstop = async () => {
        stream.getTracks().forEach((t) => t.stop());
        recording = false;
        recordPaused = false;
        if (recordTimer) { clearInterval(recordTimer); recordTimer = null; }
        if (recordedChunks.length === 0) return;
        await sendVoiceBlob(new Blob(recordedChunks, { type: mimeType || "audio/webm" }));
        recordedChunks = [];
      };
      recorder.start();
      recordStartMs = Date.now();
      recordElapsed = 0;
      recordTimer = setInterval(() => {
        if (!recordPaused) recordElapsed = Math.floor((Date.now() - recordStartMs) / 1000);
      }, 250);
      recording = true;
    } catch (e) {
      showToast(`Mic access denied: ${e}`);
    }
  }

  function cancelRecord() {
    if (!recording || !recorder) return;
    recordedChunks = []; // throw away before stop handler runs
    try { recorder.stop(); } catch { /* ignore */ }
  }

  // ---- Composer: attach popup + one mic/camera button ----
  // Tap the media button to switch mic↔camera; hold to record (release
  // sends a voice note); drag up while holding to "lock" hands-free
  // recording so the pause/delete/send controls stay up.
  let attachMenuOpen = $state(false);
  let mediaMode = $state<"voice" | "circle">("voice");
  let holdTimer: ReturnType<typeof setTimeout> | null = null;
  let holdFired = false;
  let holdStartY = 0;
  let recLocked = false;

  /** The peer's privacy (or the group's rules) forbid voice / video notes. */
  function voiceBlocked(chatId: string): boolean {
    if (!chatId.startsWith("grp_") && $peerPresence[chatId]?.nv) {
      showToast(`@${chatId} doesn't accept voice and video messages from you`);
      return true;
    }
    if (chatId.startsWith("grp_") && !canSendKind(activeGroup, myUsername, "voice")) {
      showToast(tr("Admins don't allow voice messages here"));
      return true;
    }
    return false;
  }

  function mediaDown(e: PointerEvent) {
    if (!$activeChat?.live) return;
    if (voiceBlocked($activeChat.id)) return;
    holdFired = false;
    recLocked = false;
    holdStartY = e.clientY;
    holdTimer = setTimeout(() => {
      holdFired = true;
      if (mediaMode === "voice") void startRecording();
      else if ($activeChat) openCircleRecorder($activeChat.id);
    }, 250);
  }
  function mediaMove(e: PointerEvent) {
    if (holdFired && mediaMode === "voice" && !recLocked && holdStartY - e.clientY > 40) {
      recLocked = true; // slid up → hands-free
    }
  }
  function mediaUp() {
    if (holdTimer) { clearTimeout(holdTimer); holdTimer = null; }
    if (!holdFired) {
      // A tap: switch mode.
      mediaMode = mediaMode === "voice" ? "circle" : "voice";
      return;
    }
    // Hold released: quick voice note sends on release; a locked
    // recording keeps going (controlled by the rec-bar).
    if (mediaMode === "voice" && !recLocked && recording) stopRecording();
  }

  async function sendVoiceBlob(blob: Blob) {
    const chat = $activeChat;
    if (!chat) return;
    const ext = blob.type.includes("ogg") ? "ogg" : "webm";
    const name = `voice-${new Date().toISOString().replace(/[:.]/g, "-")}.${ext}`;
    try {
      const buf = await blob.arrayBuffer();
      const bytes = Array.from(new Uint8Array(buf));
      await api.sendVoiceMessage(chat.id, name, blob.type || "audio/webm", bytes);
      await loadActiveMessages();
    } catch (e) {
      showToast(`Voice send failed: ${e}`);
    }
  }

  function fmtRecord(secs: number): string {
    const m = Math.floor(secs / 60);
    const s = secs % 60;
    return `${m}:${String(s).padStart(2, "0")}`;
  }

  // Send a batch of files sequentially. We don't fire them in parallel
  // because each one goes through the same E2EE session (double-ratchet
  // state must advance in order), and parallel uploads of large blobs
  // would also saturate the WebSocket.
  //
  // Special case: a single image → the PhotoEditor modal opens first.
  // The user can rotate / tweak / "Send original" / "Send edited".
  // "Send original" falls through to the normal attach pipeline.
  async function sendFiles(peerId: string, paths: string[], preset?: SendMode) {
    if (!paths.length) return;
    // Telegram-style sheet: previews + "as photo/video" or "as file".
    const choice = await askSend(paths, preset);
    if (!choice) return;
    if (choice.edit && paths.length === 1 && isImagePath(paths[0])) {
      const edited = await maybeEditThenSend(peerId, paths[0]);
      if (edited) { if (choice.caption) await sendCurrent(choice.caption); return; }
    }
    let ok = 0;
    let fail = 0;
    let lastErr = "";
    for (const p of paths) {
      try {
        if (choice.mode === "file") {
          await api.attachFileAs(peerId, p, "file");
        } else if (isImagePath(p)) {
          await sendCompressedPhoto(peerId, p);
        } else if (VIDEO_RE.test(p) && choice.mode === "compressed") {
          showToast(tr("Compressing {name}…", { name: p.split(/[\\/]/).pop() ?? "" }), 60000);
          try {
            await api.attachFileAs(peerId, p, "compressed");
          } catch (e) {
            // No ffmpeg / odd file: send the original rather than nothing.
            showToast(tr("Could not compress ({e}) — sending the original", { e: String(e) }), 6000);
            await api.attachFile(peerId, p);
          }
        } else {
          await api.attachFile(peerId, p);
        }
        ok++;
      } catch (e) {
        console.error("attach failed for", p, e);
        lastErr = String(e);
        fail++;
      }
    }
    if (choice.caption && ok) await sendCurrent(choice.caption);
    await loadActiveMessages();
    if (fail === 0) {
      showToast(ok === 1 ? tr("File sent") : `${ok} files sent`);
    } else if (ok === 0) {
      showToast(`${tr("Send failed")}: ${lastErr}`, 6000);
    } else {
      showToast(`${ok} sent · ${fail} failed — ${lastErr}`, 6000);
    }
  }

  async function applyThemeFile(path: string | null | undefined) {
    if (!path) return;
    try {
      const name = await applyThemeFromPath(path);
      showToast(tr("Theme “{name}” applied", { name }));
    } catch (e) { showToast(String(e), 6000); }
  }

  /** "As photo": like Telegram, re-encode to ≤ 2560 px (GIFs stay as they are). */
  async function sendCompressedPhoto(peerId: string, path: string) {
    if (/\.gif$/i.test(path)) { await api.attachFile(peerId, path); return; }
    const bytes = await api.readUserImage(path);
    const url = await shrinkImage(new Blob([new Uint8Array(bytes)]), 2560, 0.86);
    const mime = url.slice(5, url.indexOf(";"));
    const bin = atob(url.slice(url.indexOf(",") + 1));
    const out = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
    // Bigger than the original (already small / well compressed)? Send the original.
    if (out.length >= bytes.length) { await api.attachFile(peerId, path); return; }
    const base = (path.split(/[\\/]/).pop() ?? "photo").replace(/\.[^.]+$/, "");
    await api.attachBytes(peerId, `${base}.${mime === "image/webp" ? "webp" : "jpg"}`, mime, Array.from(out));
  }

  function isImagePath(p: string): boolean {
    return /\.(png|jpe?g|webp|bmp|gif)$/i.test(p);
  }

  async function maybeEditThenSend(peerId: string, path: string): Promise<boolean> {
    try {
      const bytes = await api.readUserImage(path);
      const name  = path.split(/[\\/]/).pop() ?? "image.png";
      const lower = path.toLowerCase();
      const mime  = lower.endsWith(".png") ? "image/png"
                  : lower.endsWith(".webp") ? "image/webp"
                  : lower.endsWith(".bmp") ? "image/bmp"
                  : "image/jpeg";
      return await editAndSend({ path, bytes, mime, name, peerId });
    } catch (e) {
      console.warn("photo editor skipped", e);
      return false;
    }
  }

  // Drag-and-drop: Tauri 2 delivers OS file drops through its own
  // webview event (the browser's ondrop gives File objects without a
  // real path). We subscribe once per mount and route the paths
  // through the same batch pipeline as the picker.
  let draggingOver = $state(false);

  onMount(() => {
    void loadEmojiMap();
    queueMicrotask(() => autoresize());
    let unsub: (() => void) | null = null;
    const sub = getCurrentWebview().onDragDropEvent((event) => {
      const t = event.payload.type;
      if (t === "enter" || t === "over") {
        draggingOver = true;
      } else if (t === "leave") {
        draggingOver = false;
      } else if (t === "drop") {
        draggingOver = false;
        if ($panelClaimsDrop) return; // dropped onto the emoji/GIF panel
        const paths = (event.payload as unknown as { paths: string[] }).paths ?? [];
        const chat = $activeChat;
        if (!chat || !chat.live) {
          showToast(tr("No live session — connect and start a chat first"));
          return;
        }
        if (paths.length > 0) void sendFiles(chat.id, paths);
      }
    });
    sub.then((u) => { unsub = u; });

    // Window-level listeners for context menu + drag-select.
    const onDown = () => { if (menu.open) closeMenu(); if (attachMenuOpen) attachMenuOpen = false; };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        if (menu.open) closeMenu();
        else if (selecting) clearSelection();
      } else if (e.key === "Delete" && selecting && selected.size > 0) {
        const el = e.target as HTMLElement | null;
        if (el && (el.isContentEditable || /^(INPUT|TEXTAREA)$/.test(el.tagName))) return;
        e.preventDefault();
        void deleteSelection();
      }
    };
    const onUp = () => { endDrag(); if (holdTimer || holdFired) mediaUp(); };
    window.addEventListener("mousedown", onDown);
    window.addEventListener("keydown", onKey);
    window.addEventListener("pointerup", onUp);

    // Keyboard-control requests from lib/shortcuts.ts.
    const kb: [string, () => void][] = [
      ["aqua:focus-composer", () => { if ($activeChat) composerEl?.focus(); }],
      ["aqua:chat-search", () => { if ($activeChat) openSearch(); }],
      ["aqua:jump-bottom", () => jumpToBottom()],
      ["aqua:call", () => startVoiceCall()],
      ["aqua:toggle-emoji", () => {
        if ($pickerState.open) closeMediaPicker();
        else if ($activeChat?.live) openMediaPicker($activeChat.id);
      }],
    ];
    for (const [n, f] of kb) window.addEventListener(n, f);

    // Incoming poll votes + read receipts from peers.
    let unVote: (() => void) | null = null;
    let unSeen: (() => void) | null = null;
    events.onPollVote((p) => applyRemoteVote(p.from, p.vote))
      .then((u) => { unVote = u; });
    let unClose: (() => void) | null = null;
    events.onPollClose((p) => { if (p.close?.poll) closePoll(p.close.poll, p.from); })
      .then((u) => { unClose = u; });
    events.onSeen((p) => { seenByPeer = { ...seenByPeer, [p.from]: true }; })
      .then((u) => { unSeen = u; });
    let unPres: (() => void) | null = null;
    events.onPresence((p) => { setPeerPresence(p.from, p.presence ?? {}); notePeerProfileVersion(p.from, p.presence?.pv); })
      .then((u) => { unPres = u; });
    let unReact: (() => void) | null = null;
    events.onReaction((p) => { if (p.react?.key && p.react?.emoji) applyPeerReaction(p.react.key, p.react.emoji, !!p.react.add); })
      .then((u) => { unReact = u; });
    let unTyping: (() => void) | null = null;
    events.onTyping((p) => {
      const key = p.gid ?? p.from;
      if (p.gid) typingWho = { ...typingWho, [p.gid]: p.from };
      typingPeers = new Set(typingPeers).add(key);
      if (typingTimers[key]) clearTimeout(typingTimers[key]);
      typingTimers[key] = setTimeout(() => {
        const next = new Set(typingPeers); next.delete(key); typingPeers = next;
      }, 4000);
    }).then((u) => { unTyping = u; });

    return () => {
      if (unTyping) unTyping();
      if (unsub) unsub();
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("pointerup", onUp);
      for (const [n, f] of kb) window.removeEventListener(n, f);
      if (unVote) unVote();
      if (unClose) unClose();
      if (unSeen) unSeen();
      if (unPres) unPres();
      if (unReact) unReact();
    };
  });

  // Copy a locally-stored attachment to a user-chosen location.
  async function saveFileAs(sourcePath: string | null, suggestedName: string) {
    if (!sourcePath) return;
    try {
      const { save } = await import("@tauri-apps/plugin-dialog");
      const dest = await save({ defaultPath: suggestedName });
      if (!dest) return;
      await api.copyFileTo(sourcePath, dest);
      showToast(`Saved to ${dest}`);
    } catch (e) {
      showToast(`Save failed: ${e}`);
    }
  }

  async function downloadAttachment(messageId: string) {
    const chat = $activeChat;
    if (!chat) return;
    try {
      const res = await api.downloadFile(chat.id, messageId);
      await loadActiveMessages();
      showToast(`Saved to ${res.saved_path}`);
    } catch (e) {
      showToast(`Download failed: ${e}`);
    }
  }

  function fmtSize(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
    return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GB`;
  }

  let messagesEl: HTMLDivElement | undefined = $state();
  let composerEl: RichComposer | undefined = $state();
  let text = $state("");

  // Load draft when active chat changes; save on unmount / switch.
  let prevId = "";
  $effect(() => {
    const c = $activeChat;
    if (c && c.id !== prevId) {
      // An in-progress edit is not a draft — don't persist it.
      if (prevId && !editTarget) drafts.update((d) => ({ ...d, [prevId]: text }));
      // Per-chat UI state must not leak into the next chat.
      editTarget = null;
      replyTarget = null;
      searchOpen = false;
      searchQuery = "";
      searchIdx = 0;
      selecting = false;
      selected.clear();
      text = $drafts[c.id] ?? "";
      prevId = c.id;
      queueMicrotask(() => autoresize());
      queueScrollBottom();
    }
  });

  // Scroll to bottom on new messages.
  $effect(() => {
    void $activeMessages.length;
    queueScrollBottom();
  });

  function queueScrollBottom() {
    tick().then(() => {
      if (messagesEl) messagesEl.scrollTop = messagesEl.scrollHeight;
    });
  }

  /** Scroll a long way up → show a floating "jump to bottom" button.
   *  Threshold is "more than one viewport away from the bottom". */
  let scrolledUp = $state(false);
  function onMessagesScroll() {
    if (!messagesEl) return;
    const dist = messagesEl.scrollHeight - messagesEl.scrollTop - messagesEl.clientHeight;
    scrolledUp = dist > messagesEl.clientHeight * 0.9;
  }
  function jumpToBottom() {
    if (!messagesEl) return;
    messagesEl.scrollTo({ top: messagesEl.scrollHeight, behavior: "smooth" });
  }

  /** The rich composer grows on its own (CSS max-height); kept so the
   *  existing call sites stay simple. */
  function autoresize() {}

  async function submit() {
    const t = text.trim();
    if (!t) return;
    const chat = $activeChat;

    // Editing an existing message rather than sending a new one.
    if (editTarget && chat) {
      const et = editTarget;
      editTarget = null;
      text = "";
      queueMicrotask(() => autoresize());
      if (t === et.old) return; // no change
      try {
        await api.editMessage(chat.id, et.old, t);
        if (chat.id !== "__saved__") {
          void sendChatControl(chat.id, "[edit:v1]" + JSON.stringify({ old: et.old, new: t })).catch(() => {});
        }
        await loadActiveMessages();
        chats.set(await api.listChats());
      } catch (e) {
        showToast(`Edit failed: ${e}`);
      }
      return;
    }

    // Short-circuit non-live chats with a clear instruction instead of
    // silently failing on the backend.
    if (chat && !chat.live) {
      showToast(
        "This isn't a real chat yet. Click the big “+ Start new chat” button at the top of the list, or type a username in the search bar and hit the green card that appears.",
        6000,
      );
      return;
    }
    if (chat && chat.id.startsWith("grp_")) {
      const wait = slowModeWait(chat.id);
      if (wait > 0) { showToast(`Slow mode: wait ${wait}s before your next message`); return; }
      if (/https?:\/\/\S/i.test(t) && !canSendKind(activeGroup, myUsername, "link")) {
        showToast(tr("Admins don't allow links in this chat"));
        return;
      }
    }
    const original = text;
    const rt = replyTarget;
    text = "";
    replyTarget = null;
    // Reset the textarea height AFTER the cleared value has hit the DOM,
    // otherwise scrollHeight is still the tall value and the field stays
    // grown until the next reload.
    queueMicrotask(() => autoresize());
    // A fresh outgoing message resets the peer's "seen" state for this chat.
    if (chat) seenByPeer = { ...seenByPeer, [chat.id]: false };
    // Wrap as a quoted reply when replying.
    const payload = rt
      ? "[reply:v1]" + JSON.stringify({ quoted_author: rt.author, quoted_text: rt.text, text: t })
      : t;
    const err = await sendCurrent(payload);
    if (err) {
      // Restore the message + reply target so nothing is lost.
      text = original;
      replyTarget = rt;
      queueMicrotask(() => autoresize());
      showToast(err, 5000);
    } else if (chat?.id.startsWith("grp_")) {
      armSlowMode(chat.id);
    }
  }

  /** Rows the user can reply to (newest last), for Ctrl+↑ / Ctrl+↓. */
  let kbReplyIdx = -1;
  function kbReply(dir: -1 | 1) {
    const list = rows.filter((r) => textOf(r) != null || r.kind.type === "file");
    if (!list.length) return;
    if (kbReplyIdx < 0 || !replyTarget) kbReplyIdx = list.length;
    kbReplyIdx = Math.max(0, Math.min(list.length, kbReplyIdx + dir));
    if (kbReplyIdx >= list.length) { replyTarget = null; kbReplyIdx = -1; return; }
    const row = list[kbReplyIdx];
    replyTarget = { author: authorOf(row), text: snippetOf(row).slice(0, 180) };
    document.querySelector(`[data-msg-id="${CSS.escape(row.id)}"]`)?.scrollIntoView({ block: "nearest" });
  }

  // ---- Article card in the chat ------------------------------------------
  type ArticleContent = Extract<ChatMessage["kind"], { type: "article" }>["content"];
  const cardCache = new WeakMap<object, ReturnType<typeof buildCard>>();
  function buildCard(art: ArticleContent) {
    const images = art.images ?? [];
    const media = art.media ?? [];
    const blocks = parseBlocks(art.body, "article");
    // Slides in reading order: every photo / video the galleries show.
    const seen = new Set<string>();
    const slides: CarouselItem[] = [];
    for (const b of blocks) {
      if (b.t !== "gallery") continue;
      for (const r of b.items) {
        const key = r.kind + r.i;
        if (seen.has(key)) continue;
        seen.add(key);
        if (r.kind === "image" && images[r.i]) slides.push({ kind: "image", src: objectUrl(images[r.i]) });
        else if (r.kind === "video" && media[r.i]?.kind === "video") {
          const mv = media[r.i];
          slides.push({ kind: "video", poster: mv.poster ? objectUrl(mv.poster) : undefined, resolve: () => mediaUrl(mv), label: mv.name.replace(/\.[a-z0-9]{2,5}$/i, "") });
        }
      }
    }
    if (!slides.length) images.forEach((u) => slides.push({ kind: "image", src: objectUrl(u) }));
    const words = wordCount(blocks);
    return {
      slides,
      photos: images.length,
      videos: media.filter((x) => x.kind === "video").length,
      tracks: media.filter((x) => x.kind === "audio"),
      excerpt: excerptOf(art.body),
      readMin: Math.max(1, Math.round(words / 200)),
    };
  }
  function articleCard(art: ArticleContent) {
    let c = cardCache.get(art);
    if (!c) { c = buildCard(art); cardCache.set(art, c); }
    return c;
  }
  function openCardPhotos(slides: CarouselItem[], i: number) {
    const photos = slides.filter((x) => x.kind === "image" && x.src);
    const at = photos.indexOf(slides[i]);
    if (photos.length) openGallery(photos.map((x) => ({ url: x.src!, kind: "image" as const })), Math.max(0, at));
  }

  // ---- "/" commands (Telegram-style) ------------------------------------
  interface SlashCmd { cmd: string; desc: string; run: () => void }
  function replaceText(v: string) { text = v; queueMicrotask(() => composerEl?.focus()); }
  function sendNow(v: string) { text = v; void submit(); }
  let slashCmds = $derived.by((): SlashCmd[] => [
    { cmd: "article", desc: $t("Write an article with photos"), run: () => { text = ""; if ($activeChat) openArticleComposer($activeChat.id); } },
    { cmd: "poll", desc: $t("Create a poll or quiz"), run: () => {
      text = "";
      if (!canSendKind(activeGroup, myUsername, "poll")) { showToast(tr("Only admins can create polls here")); return; }
      openPollComposer();
    } },
    { cmd: "file", desc: $t("Send photos, videos or files"), run: () => { text = ""; void pickAndSendFile(); } },
    { cmd: "gif", desc: $t("Emoji, stickers & GIFs"), run: () => { text = ""; if ($activeChat) openMediaPicker($activeChat.id); } },
    { cmd: "sticker", desc: $t("Emoji, stickers & GIFs"), run: () => { text = ""; if ($activeChat) openMediaPicker($activeChat.id); } },
    { cmd: "video", desc: $t("Record a round video message"), run: () => { text = ""; if ($activeChat) openCircleRecorder($activeChat.id); } },
    { cmd: "call", desc: $t("Voice call"), run: () => { text = ""; if (isGroup && activeGroup) void joinGroupCall(activeGroup.id); else startVoiceCall(); } },
    { cmd: "search", desc: $t("Search in this chat"), run: () => { text = ""; openSearch(); } },
    { cmd: "dice", desc: $t("Roll a die"), run: () => sendNow(`🎲 ${1 + Math.floor(Math.random() * 6)}`) },
    { cmd: "coin", desc: $t("Flip a coin"), run: () => sendNow(`🪙 ${Math.random() < 0.5 ? $t("Heads") : $t("Tails")}`) },
    { cmd: "shrug", desc: "¯\\_(ツ)_/¯", run: () => replaceText("¯\\_(ツ)_/¯ ") },
    { cmd: "tableflip", desc: "(╯°□°)╯︵ ┻━┻", run: () => replaceText("(╯°□°)╯︵ ┻━┻ ") },
    { cmd: "unflip", desc: "┬─┬ノ( º _ ºノ)", run: () => replaceText("┬─┬ノ( º _ ºノ) ") },
    { cmd: "lenny", desc: "( ͡° ͜ʖ ͡°)", run: () => replaceText("( ͡° ͜ʖ ͡°) ") },
    { cmd: "spoiler", desc: $t("Hidden text: ||like this||"), run: () => replaceText("||") },
    { cmd: "help", desc: $t("Keyboard shortcuts and formatting"), run: () => { text = ""; shortcutsOpen.set(true); } },
  ]);
  let slashIdx = $state(0);
  let slashHidden = $state("");
  let slashMatches = $derived.by(() => {
    const m = /^\/([\p{L}\w]*)$/u.exec(text);
    if (!m || $activeChat?.live !== true || slashHidden === text) return [];
    const q = m[1].toLowerCase();
    return slashCmds.filter((c) => c.cmd.startsWith(q));
  });
  $effect(() => { void slashMatches.length; slashIdx = 0; });
  function runSlash(c: SlashCmd) { slashHidden = ""; c.run(); }
  /** Returns true when the key was used by the command popup. */
  function slashKey(e: KeyboardEvent): boolean {
    if (!slashMatches.length) return false;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const n = slashMatches.length;
      slashIdx = (slashIdx + (e.key === "ArrowDown" ? 1 : n - 1)) % n;
      return true;
    }
    if (e.key === "Enter" || e.key === "Tab") {
      e.preventDefault();
      runSlash(slashMatches[Math.min(slashIdx, slashMatches.length - 1)]);
      return true;
    }
    if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); slashHidden = text; return true; }
    return false;
  }

  function onKey(e: KeyboardEvent) {
    if (slashKey(e)) return;
    const ctrl = e.ctrlKey || e.metaKey;
    if (e.key === "ArrowUp" && !ctrl && !e.shiftKey && !e.altKey && text.length === 0 && !editTarget) {
      // Telegram: ↑ in an empty box edits your last message.
      const mine = [...rows].reverse().find((r) => r.outgoing && (r.kind.type === "text" || r.kind.type === "reply"));
      if (mine) { e.preventDefault(); startEdit(mine); }
      return;
    }
    if (ctrl && (e.key === "ArrowUp" || e.key === "ArrowDown")) {
      e.preventDefault();
      kbReply(e.key === "ArrowUp" ? -1 : 1);
      return;
    }
    if (e.key === "Escape") {
      if (editTarget) { e.preventDefault(); e.stopPropagation(); cancelEdit(); return; }
      if (replyTarget) { e.preventDefault(); e.stopPropagation(); cancelReply(); kbReplyIdx = -1; return; }
      composerEl?.blur();
      return;
    }
    if (e.key !== "Enter") return;
    if ($settings.enterToSend) {
      // Enter sends; Shift+Enter is a newline.
      if (!e.shiftKey) { e.preventDefault(); submit(); }
    } else {
      // Enter is a newline; Ctrl/⌘+Enter sends.
      if (e.metaKey || e.ctrlKey) { e.preventDefault(); submit(); }
    }
  }

  async function openPeerFingerprint() {
    const c = $activeChat;
    if (!c) return;
    try {
      const view = await api.verifyFingerprint(c.id);
      openFingerprint(view);
    } catch (e) {
      console.error(e);
    }
  }

  // ---- Message grouping ----

  interface Row extends ChatMessage {
    first: boolean;
    last: boolean;
    outgoing: boolean;
  }
  let myUsername = $derived($info?.username ?? "");
  let isGroup = $derived($activeChat?.id?.startsWith("grp_") ?? false);
  let activeGroup = $derived($activeChat ? $groupMap[$activeChat.id] : undefined);
  // Can the current user post here? (Same rules the backend enforces on
  // receive — groupRules.ts / state.rs may_post.)
  let postBlocked = $derived(postBlock(activeGroup, myUsername));
  let canPost = $derived(postBlocked === null);
  /** Channel / group forbids forwarding, copying and saving. */
  let protectedChat = $derived(!!activeGroup?.settings.protected_content && !isAdmin(activeGroup, myUsername));
  /** Reactions the chat allows (null = any). */
  let allowedReactions = $derived.by((): string[] | null => {
    const g = activeGroup;
    if (!g) return null;
    const mode = g.settings.reactions ?? "all";
    if (mode === "none") return [];
    if (mode === "some") return g.settings.allowed_reactions ?? [];
    return null;
  });
  function filterReactions(list: string[]): string[] {
    const a = allowedReactions;
    return a == null ? list : list.filter((r) => a.includes(r));
  }
  // ---- Slow mode ----
  const lastSentAt: Record<string, number> = {};
  let slowLeft = $state(0);
  let slowTimer: ReturnType<typeof setInterval> | null = null;
  function slowModeWait(chatId: string): number {
    const g = $groupMap[chatId];
    const sec = g?.settings.slow_mode ?? 0;
    if (!g || !sec || isAdmin(g, myUsername)) return 0;
    const at = lastSentAt[chatId] ?? 0;
    return Math.max(0, Math.ceil((at + sec * 1000 - Date.now()) / 1000));
  }
  function armSlowMode(chatId: string) {
    lastSentAt[chatId] = Date.now();
    slowLeft = slowModeWait(chatId);
    if (slowTimer) clearInterval(slowTimer);
    if (slowLeft > 0) {
      slowTimer = setInterval(() => {
        slowLeft = $activeChat ? slowModeWait($activeChat.id) : 0;
        if (slowLeft <= 0 && slowTimer) { clearInterval(slowTimer); slowTimer = null; }
      }, 1000);
    }
  }
  $effect(() => {
    const id = $activeChat?.id;
    untrack(() => { slowLeft = id ? slowModeWait(id) : 0; });
  });
  let rows = $derived(computeRows($activeMessages, myUsername));

  function computeRows(msgs: ChatMessage[], me: string): Row[] {
    const out: Row[] = [];
    let prevFrom: string | null = null;
    let prevTime: string | null = null;
    const mine = (from: string) => from === "me" || (me !== "" && from === me);
    // Telegram-style grouping: a run of messages from the same sender within
    // the same minute sticks together tightly; a sender change OR a new
    // minute starts a fresh block with a little breathing room.
    const sameGroup = (a: ChatMessage, fromPrev: string | null, timePrev: string | null) =>
      a.from === fromPrev && a.time === timePrev;
    for (let i = 0; i < msgs.length; i++) {
      const m = msgs[i];
      if (m.kind.type === "system" || m.kind.type === "date_separator") {
        out.push({ ...m, first: false, last: false, outgoing: false });
        prevFrom = null;
        prevTime = null;
        continue;
      }
      const next = msgs[i + 1];
      const first = !sameGroup(m, prevFrom, prevTime);
      const last =
        !next ||
        next.kind.type === "system" ||
        next.kind.type === "date_separator" ||
        next.from !== m.from ||
        next.time !== m.time;
      out.push({ ...m, first, last, outgoing: mine(m.from) });
      prevFrom = m.from;
      prevTime = m.time;
    }
    return out;
  }

  const SENDER_PAIRS = [
    ["#00E5C7","#7B5CFF"],["#FF7CAC","#FFB067"],["#4ADE80","#22D3EE"],
    ["#C084FC","#F472B6"],["#22D3EE","#6366F1"],["#FFB067","#FF4D6D"],
    ["#F472B6","#FBBF24"],["#60A5FA","#A855F7"],
  ];
  function senderGradient(from: string): string {
    let h = 0;
    for (let i = 0; i < from.length; i++) h = (h * 31 + from.charCodeAt(i)) >>> 0;
    const [a, b] = SENDER_PAIRS[h % SENDER_PAIRS.length];
    return `linear-gradient(135deg, ${a} 0%, ${b} 100%)`;
  }
  function chatGradient(from: string): string {
    if (from === "me" || (myUsername && from === myUsername))
      return "linear-gradient(135deg, #FF7CAC 0%, #FFB067 100%)";
    // In a group each sender gets their own deterministic colour.
    if (isGroup) return senderGradient(from);
    return $activeChat?.gradient ?? "linear-gradient(135deg, #666, #999)";
  }

  // ---- Message selection, context menu, forward (Telegram-style) ----
  let selecting = $state(false);
  // Fine-grained: only rows whose membership changes re-render (a new Set
  // per pointer move re-rendered every message → jerky drag-select).
  const selected = new SvelteSet<string>();
  /** GIFs the user started while "Autoplay GIFs" is off. */
  const gifPlaying = new SvelteSet<string>();
  // Custom (non-Chromium) context menu.
  let menu = $state<{ open: boolean; x: number; y: number; row: Row | null }>({ open: false, x: 0, y: 0, row: null });

  function selectedCount() { return selected.size; }

  function toggleSelect(id: string) {
    if (selected.has(id)) selected.delete(id); else selected.add(id);
    if (selected.size === 0) selecting = false;
  }
  function startSelection(row: Row) {
    selecting = true;
    selected.clear();
    selected.add(row.id);
  }
  function clearSelection() {
    selecting = false;
    selected.clear();
  }

  function onMsgClick(row: Row) {
    // Swallow the click that trails a real drag-select so it doesn't
    // immediately toggle the start row back off.
    if (draggedMoved) { draggedMoved = false; return; }
    if (selecting) toggleSelect(row.id);
  }
  function onMsgContext(e: MouseEvent, row: Row) {
    e.preventDefault();
    e.stopPropagation();
    // Clamp so the menu stays on screen (approx 190x220).
    const x = e.clientX, y = e.clientY; // keepOnScreen pulls it back inside
    menu = { open: true, x, y, row };
  }
  function closeMenu() { menu = { open: false, x: 0, y: 0, row: null }; }

  /** Extract a forwardable payload from a message, or null if it can't
   *  be forwarded (e.g. a media file that was never downloaded). */
  function forwardItemOf(m: ChatMessage): ForwardItem | null {
    const k = m.kind;
    if (k.type === "text") return { kind: "text", text: k.content.text };
    if (k.type === "code") return { kind: "text", text: k.content.code };
    if (k.type === "article") return { kind: "text", text: `${k.content.title}\n\n${k.content.body}` };
    if (k.type === "file") {
      if (k.content.saved_at) return { kind: "file", path: k.content.saved_at, name: k.content.name };
      return null; // not downloaded → can't re-send
    }
    return null;
  }

  function textOf(m: ChatMessage): string | null {
    const k = m.kind;
    if (k.type === "text") return k.content.text;
    if (k.type === "code") return k.content.code;
    if (k.type === "article") return `${k.content.title}\n\n${k.content.body}`;
    if (k.type === "reply") return k.content.text;
    if (k.type === "poll") return k.content.question;
    return null;
  }

  // ---- In-chat search ----
  let searchOpen = $state(false);
  let searchQuery = $state("");
  let searchHits = $derived.by<string[]>(() => {
    const q = searchQuery.trim().toLowerCase();
    if (!q) return [];
    return $activeMessages
      .filter((m) => (textOf(m) ?? "").toLowerCase().includes(q))
      .map((m) => m.id);
  });
  let searchIdx = $state(0);
  // Keep the index in range as hits change.
  $effect(() => {
    if (searchIdx >= searchHits.length) searchIdx = Math.max(0, searchHits.length - 1);
  });
  let searchHitSet = $derived(new Set(searchHits));
  let currentHitId = $derived(searchHits[searchIdx] ?? "");

  function openSearch() {
    searchOpen = true;
    queueMicrotask(() => document.querySelector<HTMLInputElement>("#chat-search-input")?.focus());
  }
  function closeSearch() { searchOpen = false; searchQuery = ""; searchIdx = 0; }
  function scrollToHit() {
    const id = searchHits[searchIdx];
    if (!id || !messagesEl) return;
    messagesEl.querySelector(`[data-id="${id}"]`)?.scrollIntoView({ block: "center", behavior: "smooth" });
  }
  function searchNext() {
    if (searchHits.length === 0) return;
    searchIdx = (searchIdx + 1) % searchHits.length;
    scrollToHit();
  }
  function searchPrev() {
    if (searchHits.length === 0) return;
    searchIdx = (searchIdx - 1 + searchHits.length) % searchHits.length;
    scrollToHit();
  }
  function onSearchKey(e: KeyboardEvent) {
    if (e.key === "Enter") { e.preventDefault(); if (e.shiftKey) searchPrev(); else searchNext(); }
    else if (e.key === "Escape") { e.preventDefault(); closeSearch(); }
  }

  // ---- Quoted replies ----
  let replyTarget = $state<{ author: string; text: string } | null>(null);
  function authorOf(m: Row): string {
    return m.outgoing ? ($info?.username ?? "You") : ($activeChat?.name ?? "Peer");
  }
  /** A short snippet of any message, for the reply preview. */
  function snippetOf(m: ChatMessage): string {
    const k = m.kind;
    if (k.type === "text") return k.content.text;
    if (k.type === "code") return "code";
    if (k.type === "article") return k.content.title;
    if (k.type === "reply") return k.content.text;
    if (k.type === "poll") return `📊 ${k.content.question}`;
    if (k.type === "file") {
      const c = k.content;
      if (c.category === "sticker") return "Sticker";
      if (c.category === "voice") return "Voice message";
      return c.name;
    }
    return "Message";
  }

  async function copyRow(row: Row) {
    const t = textOf(row);
    if (t == null) { showToast(tr("Nothing to copy here")); closeMenu(); return; }
    try { await navigator.clipboard.writeText(t); showToast(tr("Copied")); }
    catch { showToast(tr("Copy blocked")); }
    closeMenu();
  }

  function replyRow(row: Row) {
    replyTarget = { author: authorOf(row), text: snippetOf(row).slice(0, 180) };
    closeMenu();
    queueMicrotask(() => composerEl?.focus());
  }
  function cancelReply() { replyTarget = null; }

  // ---- Edit / delete ----
  let editTarget = $state<{ old: string } | null>(null);
  function startEdit(row: Row) {
    const t = textOf(row);
    if (t == null) { closeMenu(); return; }
    editTarget = { old: t };
    replyTarget = null;
    text = t;
    closeMenu();
    queueMicrotask(() => { autoresize(); composerEl?.focus(); });
  }
  function cancelEdit() { editTarget = null; text = ""; queueMicrotask(() => autoresize()); }

  /** Text messages are deleted on both sides (matched by text); media,
   *  articles, polls… are removed on this device. */
  async function deleteRows(list: Row[], ask = true) {
    closeMenu();
    const chat = $activeChat;
    if (!chat || list.length === 0) return;
    if (ask) {
      const ok = await askConfirm({
        title: list.length === 1 ? tr("Delete this message?") : tr("Delete {n} messages?", { n: list.length }),
        text: chat.id === "__saved__" ? undefined : tr("Your text messages are deleted for everyone; media and articles — on this device."),
        ok: tr("Delete"),
        danger: true,
      });
      if (!ok) return;
    }
    const local: string[] = [];
    let failed = 0;
    for (const row of list) {
      const k = row.kind.type;
      const t = k === "text" || k === "reply" ? textOf(row) : null;
      if (t == null || (!row.outgoing && !chat.id.startsWith("grp_"))) { local.push(row.id); continue; }
      try {
        if (!row.outgoing && chat.id.startsWith("grp_")) {
          await api.deleteGroupMessageOf(chat.id, row.from, t);
          void sendChatControl(chat.id, "[del:v1]" + JSON.stringify({ text: t, author: row.from })).catch(() => {});
        } else {
          await api.deleteMessage(chat.id, t);
          if (chat.id !== "__saved__") void sendChatControl(chat.id, "[del:v1]" + JSON.stringify({ text: t })).catch(() => {});
        }
      } catch { failed++; local.push(row.id); }
    }
    if (local.length) { try { await api.deleteMessagesLocal(chat.id, local); } catch { failed++; } }
    if (failed) showToast(tr("Some messages could not be deleted"));
    await loadActiveMessages();
    chats.set(await api.listChats());
  }
  async function deleteSelection() {
    const list = rows.filter((r) => selected.has(r.id));
    clearSelection();
    await deleteRows(list);
  }

  async function deleteRow(row: Row) {
    closeMenu();
    const t = textOf(row);
    if (t == null || (row.kind.type !== "text" && row.kind.type !== "reply")) { await deleteRows([row]); return; }
    const chat = $activeChat;
    if (!chat) return;
    try {
      if (!row.outgoing && chat.id.startsWith("grp_")) {
        // Admin removing someone else's message.
        await api.deleteGroupMessageOf(chat.id, row.from, t);
        void sendChatControl(chat.id, "[del:v1]" + JSON.stringify({ text: t, author: row.from })).catch(() => {});
      } else {
        await api.deleteMessage(chat.id, t);
        if (chat.id !== "__saved__") {
          void sendChatControl(chat.id, "[del:v1]" + JSON.stringify({ text: t })).catch(() => {});
        }
      }
      await loadActiveMessages();
      chats.set(await api.listChats());
    } catch (e) {
      showToast(`Delete failed: ${e}`);
    }
  }

  // ---- Typing indicator ----
  let typingPeers = $state<Set<string>>(new Set());
  /** Group id → who is typing there. */
  let typingWho = $state<Record<string, string>>({});
  const typingTimers: Record<string, ReturnType<typeof setTimeout>> = {};
  let lastTypingSent = 0;
  function maybeSendTyping() {
    const chat = $activeChat;
    if (!chat || !chat.live || chat.id === "__saved__" || $ghost.noTyping) return;
    const now = Date.now();
    if (now - lastTypingSent < 3000) return;
    lastTypingSent = now;
    void sendChatControl(chat.id, "[typing:v1]{}").catch(() => {});
  }
  function onComposerInput() {
    autoresize();
    if (text.trim().length > 0) maybeSendTyping();
  }

  function forwardRow(row: Row) {
    const it = forwardItemOf(row);
    closeMenu();
    if (!it) { showToast(tr("This message can't be forwarded (not downloaded)")); return; }
    openForward([it]);
  }

  function forwardSelection() {
    const items: ForwardItem[] = [];
    let skipped = 0;
    for (const r of rows) {
      if (selected.has(r.id)) {
        const it = forwardItemOf(r);
        if (it) items.push(it); else skipped++;
      }
    }
    if (items.length === 0) { showToast(tr("Nothing forwardable selected")); return; }
    openForward(items);
    if (skipped) showToast(`${skipped} item(s) can't be forwarded`);
    clearSelection();
  }

  async function copySelection() {
    const parts: string[] = [];
    for (const r of rows) {
      if (selected.has(r.id)) { const t = textOf(r); if (t != null) parts.push(t); }
    }
    if (parts.length === 0) { showToast(tr("No text selected")); return; }
    try { await navigator.clipboard.writeText(parts.join("\n\n")); showToast(`Copied ${parts.length} message(s)`); }
    catch { showToast(tr("Copy blocked")); }
    clearSelection();
  }

  // ---- Telegram-style drag-to-select ----
  // Press beside a message (not on the bubble text/controls) and drag up
  // or down; every message the pointer passes over joins the selection.
  // Scrolling itself is left to the webview — we don't move the viewport.
  let selectableIds = $derived(
    rows.filter((r) => r.kind.type !== "system" && r.kind.type !== "date_separator").map((r) => r.id),
  );
  let dragging = false;       // a real range-drag is in progress
  let armed = false;          // pointer is down in a gutter, not yet dragging
  let dragStartId: string | null = null;
  let armStartY = 0;
  let draggedMoved = false;   // true once a drag actually moved → swallow the click

  function rowIdFromPoint(x: number, y: number): string | null {
    const el = document.elementFromPoint(x, y) as HTMLElement | null;
    const msg = el?.closest(".msg") as HTMLElement | null;
    return msg?.getAttribute("data-id") ?? null;
  }
  let lastLo = -1, lastHi = -1;
  let moveFrame = 0;
  let lastMove: PointerEvent | null = null;
  function applyRange(toId: string) {
    if (dragStartId == null) return;
    const a = selectableIds.indexOf(dragStartId);
    const b = selectableIds.indexOf(toId);
    if (a < 0 || b < 0) return;
    const [lo, hi] = a < b ? [a, b] : [b, a];
    if (lo === lastLo && hi === lastHi) return;
    lastLo = lo; lastHi = hi;
    const want = new Set(selectableIds.slice(lo, hi + 1));
    for (const id of [...selected]) if (!want.has(id)) selected.delete(id);
    for (const id of want) if (!selected.has(id)) selected.add(id);
  }
  function onRowPointerDown(e: PointerEvent, m: Row) {
    if (e.button !== 0) return;
    const t = e.target as HTMLElement;
    // Leave the bubble's own text/links/controls alone so copying text
    // and clicking media still work. Dragging starts in the row gutter.
    if (t.closest(".bubble, a, button, input, textarea, img, video, .media-img-btn")) return;
    // Arm only — selection does NOT start on a click. It begins once the
    // pointer actually drags onto another message (or far enough).
    armed = true;
    dragging = false;
    draggedMoved = false;
    dragStartId = m.id;
    armStartY = e.clientY;
  }
  function onMessagesPointerMove(e: PointerEvent) {
    if (!armed || dragStartId == null) return;
    const id = rowIdFromPoint(e.clientX, e.clientY);
    const movedFar = Math.abs(e.clientY - armStartY) > 8;
    // Begin a real selection only when we reach a different message, or
    // drag clearly up/down past the threshold.
    if (!dragging) {
      if ((id && id !== dragStartId) || movedFar) {
        dragging = true;
        draggedMoved = true;
        selecting = true;
        selected.clear();
        selected.add(dragStartId);
        lastLo = lastHi = -1;
      } else {
        return;
      }
    }
    e.preventDefault();
    // Hit-testing + range update once per frame, not per pointer event.
    lastMove = e;
    if (!moveFrame) moveFrame = requestAnimationFrame(() => {
      moveFrame = 0;
      const ev = lastMove;
      if (!ev || !dragging) return;
      const hit = rowIdFromPoint(ev.clientX, ev.clientY);
      if (hit) applyRange(hit);
    });
  }
  function endDrag() {
    armed = false;
    if (!dragging) { dragStartId = null; return; }
    dragging = false;
    dragStartId = null;
    if (selected.size === 0) selecting = false;
  }

  /** Username whose avatar to show for a message author. */
  function avatarKey(from: string): string {
    if (from === "me") return myUsername ?? "";
    if (isGroup) return from;
    return $activeChat?.id ?? from;
  }

  function firstLetter(from: string): string {
    const initial = (s: string | null | undefined) =>
      (s ?? "").trim().charAt(0).toUpperCase() || "?";
    // Me: my own username (was hardcoded "V" regardless of account).
    if (from === "me" || (myUsername && from === myUsername)) return initial(myUsername);
    // Group: each sender's own initial, not the group's.
    if (isGroup) return initial(from);
    return $activeChat?.letter ?? initial(from);
  }

  // ---- Polls ----
  type PollContent = Extract<ChatMessage["kind"], { type: "poll" }>["content"];
  /** Multiple-choice selections not yet submitted with "Vote". */
  let multiPick = $state<Record<string, number[]>>({});
  function sendVote(pollId: string, options: number[]) {
    const peer = $activeChat?.id;
    if (peer && $activeChat?.live) {
      void sendChatControl(peer, "[pollvote:v1]" + JSON.stringify({ poll: pollId, options, at: Date.now() })).catch(() => {});
    }
  }
  function votePoll(poll: PollContent, option: number) {
    if (poll.multi) {
      const cur = multiPick[poll.id] ?? [];
      multiPick = { ...multiPick, [poll.id]: cur.includes(option) ? cur.filter((x) => x !== option) : [...cur, option] };
      return;
    }
    const next = castVote(poll.id, option, false);
    if (next) sendVote(poll.id, next);
  }
  function submitMulti(poll: PollContent) {
    const pick = multiPick[poll.id] ?? [];
    if (!pick.length) return;
    for (const o of pick) castVote(poll.id, o, true);
    const { [poll.id]: _d, ...rest } = multiPick; multiPick = rest;
    sendVote(poll.id, pick);
  }
  function retractPoll(poll: PollContent) {
    if (retractVote(poll.id)) sendVote(poll.id, []);
  }
  function stopPoll(poll: PollContent) {
    closePoll(poll.id, POLL_ME);
    const peer = $activeChat?.id;
    if (peer && $activeChat?.live) void sendChatControl(peer, "[pollclose:v1]" + JSON.stringify({ poll: poll.id })).catch(() => {});
  }
  function pollKindLabel(poll: PollContent, pv: PollView): string {
    const base = poll.quiz ? (poll.public ? "Quiz" : "Anonymous quiz") : (poll.public ? "Public poll" : "Anonymous poll");
    const parts = [$t(base)];
    if (poll.multi) parts.push($t("multiple answers"));
    if (pv.closed) parts.unshift($t("Final results"));
    else if (poll.close_at) parts.push($t("ends {time}", { time: new Date(poll.close_at).toLocaleString([], { hour: "2-digit", minute: "2-digit", day: "numeric", month: "short" }) }));
    return parts.join(" · ");
  }
  function voterName(v: string): string {
    if (v === POLL_ME) return $t("You");
    if (v.startsWith("__old_")) return "?";
    return getContact(v)?.full_name || v;
  }

  // ---- Reactions (synced via a stable content key) ----
  /** Content that uniquely identifies a message to BOTH peers. */
  function reactionContent(m: ChatMessage): string {
    const k = m.kind;
    if (k.type === "text") return "t:" + k.content.text;
    if (k.type === "code") return "c:" + k.content.code;
    if (k.type === "article") return "a:" + k.content.title + "|" + k.content.body;
    if (k.type === "poll") return "p:" + k.content.id;
    if (k.type === "file") return "f:" + k.content.cid;
    return "x:" + m.id;
  }
  /** `<authorUsername>:<hash>` — same on both sides for one message. */
  function reactionKey(m: Row): string {
    const author = m.outgoing ? ($info?.username ?? "me") : (isGroup ? m.from : ($activeChat?.id ?? "peer"));
    return `${author}:${hashContent(reactionContent(m))}`;
  }
  /** Context menu: show the whole reaction pack instead of the quick 8. */
  let reactAll = $state(false);
  /** Which pack the full reaction picker shows ("" = El Taller). */
  let reactTab = $state("");
  $effect(() => { if (!menu.open) { reactAll = false; reactTab = ""; } });
  let reactTokens = $derived(
    !reactAll ? quickReactions($reactionPack.items)
      : reactTab ? ($emojiPacks.find((p) => p.name === reactTab)?.items.map((i) => i.token) ?? [])
      : $reactionPack.items.map((i) => i.token),
  );

  /** Right-click a reaction: which pack it's from, and add that pack. */
  async function reactionInfo(e: MouseEvent, emoji: string) {
    e.preventDefault();
    e.stopPropagation();
    const src = reactionSource(emoji);
    if (!src) return;
    if (src.local) { showToast(tr("This reaction is from the “{title}” pack — you have it", { title: src.title })); return; }
    const ok = await askConfirm({
      title: tr("“{title}” emoji", { title: src.title }),
      text: tr("This reaction is from the “{title}” pack. Add the whole pack to your emoji?", { title: src.title }),
      ok: tr("Add pack"),
    });
    if (!ok) return;
    try {
      const r = await api.importPackViaServer(src.pack);
      await loadEmojiPacks();
      reactionPack.update((x) => ({ ...x }));
      showToast(tr("Added “{title}” ({n})", { title: r.pack_title, n: r.imported }));
    } catch (err) { showToast(String(err), 7000); }
  }

  function reactTo(row: Row, emoji: string) {
    if (allowedReactions && !allowedReactions.includes(emoji)) { closeMenu(); return; }
    const key = reactionKey(row);
    const added = toggleMyReaction(key, emoji);
    const peer = $activeChat?.id;
    if (peer && $activeChat?.live) {
      void sendChatControl(peer, "[react:v1]" + JSON.stringify({ key, emoji, add: added })).catch(() => {});
    }
    closeMenu();
  }
  function toggleChipReaction(row: Row, emoji: string) {
    const key = reactionKey(row);
    const added = toggleMyReaction(key, emoji);
    const peer = $activeChat?.id;
    if (peer && $activeChat?.live) {
      void sendChatControl(peer, "[react:v1]" + JSON.stringify({ key, emoji, add: added })).catch(() => {});
    }
  }

  // ---- Save to your collection (Telegram's "Add to stickers / Save GIF") ----
  async function saveToCollection(row: Row, target: "stickers" | "gifs") {
    closeMenu();
    if (row.kind.type !== "file" || !row.kind.content.saved_at) return;
    try {
      const n = await api.addMediaFiles(target, target === "stickers" ? "Saved stickers" : null, [row.kind.content.saved_at]);
      showToast(n ? (target === "gifs" ? "Saved to your GIFs" : "Added to “Saved stickers”") : "This file type can't be saved there");
    } catch (e) { showToast(`Could not save: ${e}`); }
  }
  /** Copy the custom emoji used in a message into "My emoji" (only ones we can see). */
  async function saveEmojiFromRow(row: Row) {
    closeMenu();
    const t = textOf(row) ?? "";
    const keys = [...t.matchAll(/\[ce:([^\]\s]+)\]/g)].map((m) => m[1]);
    const paths = keys.map((k) => emojiMap[k]?.url).filter(Boolean) as string[];
    // Asset URLs map back to library files; bundled ones aren't files, skip those.
    const files = keys.map((k) => libraryPathOf(k)).filter(Boolean) as string[];
    if (!files.length) { showToast(paths.length ? "You already have these emoji" : "You don't have these emoji packs yet"); return; }
    try {
      const n = await api.addMediaFiles("emoji", null, files);
      showToast(`Added ${n} emoji to “My emoji”`);
    } catch (e) { showToast(`Could not save: ${e}`); }
  }
  function libraryPathOf(key: string): string | null {
    return emojiMap[key]?.path ?? null;
  }

  // ---- Pins ----
  function pinRow(row: Row) {
    const peer = $activeChat?.id;
    if (peer) pinMessage(peer, row.id);
    closeMenu();
  }
  function unpin() {
    const peer = $activeChat?.id;
    if (peer) unpinMessage(peer);
  }
  let pinned = $derived.by(() => {
    const peer = $activeChat?.id;
    if (!peer) return null;
    void $pins;
    const id = pinnedId(peer);
    if (!id) return null;
    return rows.find((r) => r.id === id) ?? null;
  });
  function scrollToPinned() {
    if (!pinned || !messagesEl) return;
    const el = messagesEl.querySelector(`[data-id="${pinned.id}"]`);
    el?.scrollIntoView({ behavior: "smooth", block: "center" });
  }

  // ---- Read receipts (chat-level "seen") ----
  let seenByPeer = $state<Record<string, boolean>>({});
  let peerSeen = $derived(seenByPeer[$activeChat?.id ?? ""] ?? false);

  // Tell the peer we've seen their messages when we open/scroll a live
  // chat that has incoming messages.
  $effect(() => {
    const chat = $activeChat;
    void $activeMessages.length;
    if (chat?.live && !chat.id.startsWith("grp_") && !$ghost.noReadReceipts && $activeMessages.some((m) => m.from !== "me")) {
      void api.sendControl(chat.id, "[seen:v1]{}");
    }
  });

  // Insert custom-emoji tokens pushed from the emoji picker.
  $effect(() => {
    const tok = $insertToComposer;
    if (tok) untrack(() => {
      // Insert at the caret (Telegram behaviour), shown as a picture.
      if (tok.startsWith("[ce:") && !emojiMap[tok.slice(4, -1)]) void loadEmojiMap();
      if (composerEl) composerEl.insert(tok); else text += tok;
    });
  });

  // Share our presence (status / now-playing) with a chat when we open it.
  let presenceSentFor = "";
  $effect(() => {
    const chat = $activeChat;
    if (chat?.live && chat.id !== presenceSentFor) {
      presenceSentFor = chat.id;
      broadcastPresenceTo(chat.id);
    }
  });
</script>

{#snippet msgFlags(m: ChatMessage)}
  {#if m.meta?.deleted}<span class="mflag del" title={$t("Deleted by the sender — kept on this device")}>{$t("deleted")}</span>{/if}
  {#if m.meta?.edits?.length}<span class="mflag" title={$t("Earlier versions:") + "\n" + m.meta.edits.join("\n— ")}>{$t("edited")}</span>{/if}
{/snippet}

{#snippet statusTick(outgoing: boolean)}
  {#if outgoing}
    <span class="rr" class:read={peerSeen}>{@html peerSeen ? icons.checkDouble : icons.check}</span>
  {/if}
{/snippet}

{#snippet msgBody(str: string)}
  {#if hasMarkup(str)}
    <Markup src={str} resolveEmoji={(k) => emojiMap[k]} compact />
    {#if $settings.linkPreviews}
      {@const link = linksIn(str)[0]}
      {#if link}<LinkPreview url={link} />{/if}
    {/if}
  {:else}
    {@render richText(str)}
  {/if}
{/snippet}

{#snippet richText(str: string)}
  {#each richParts(str) as part}
    {#if part.t === "text"}{part.v}{:else if emojiMap[part.key]}<span class="inline-emoji" title={ceFallback(part.key)}><AnimatedMedia src={emojiMap[part.key].url} mime={emojiMap[part.key].mime} alt={ceFallback(part.key)} play="auto" /></span>{:else}{ceFallback(part.key)}{/if}
  {/each}
{/snippet}

<main class="active-chat">
  {#if !$activeChat}
    <div class="empty">{$t("Pick a chat on the left.")}</div>
  {:else}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <header
      class="chat-header"
      class:clickable={$activeChat.id !== "__saved__"}
      onclick={(e) => {
        // The whole bar opens the chat / channel / profile info, except
        // the action buttons on the right and other real controls.
        const t = e.target as HTMLElement;
        if (t.closest(".chat-actions, .back-btn, .verified-btn, a")) return;
        openChatInfo();
      }}
      title={$activeChat.id === "__saved__" ? "" : isGroup ? "Info & settings" : "Profile & privacy"}
    >
      <button class="back-btn" onclick={() => backToList()} title={$t("Back")} aria-label={$t("Back to chats")}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 18 9 12 15 6"/></svg>
      </button>
      {#if isGroup && activeGroup}
        <button class="avatar sz-40 avatar-btn" style="--gradient: {$activeChat.gradient}" title="Group info & settings">
          {$activeChat.letter}
        </button>
      {:else}
        <div class="avatar sz-40" class:online={chatOnline($activeChat, $peerPresence, $presenceNow)} style="--gradient: {$activeChat.gradient}">
          {$activeChat.letter}
          {#if $avatars[$activeChat.id]}<img class="ava-img" src={$avatars[$activeChat.id]} alt="" />{/if}
        </div>
      {/if}
      <div class="peer-info">
        <h1 class="peer-name">
          {#if isGroup && activeGroup}
            <span class="group-name-btn">{$activeChat.name}</span>
          {:else}
            {$activeChat.name}
            {#if $clans[$activeChat.id]}<span class="hdr-clan"><ClanTag tag={$clans[$activeChat.id]} /></span>{/if}
          {/if}
          {#if $activeChat.verified}
            <button class="verified-btn" onclick={openPeerFingerprint} title={$t("Verify key")}>
              <span class="verified-badge">{@html icons.verified}</span>
            </button>
          {/if}
        </h1>
        {#if $peerPresence[$activeChat.id]?.listening?.title}
          {@const np = $peerPresence[$activeChat.id].listening}
          <a class="now-playing" href={np?.url || undefined} target="_blank" rel="noopener" title={np?.url ? "Open" : ""}>
            <span class="np-note">♪</span>
            <span class="np-text">{np?.title}{#if np?.artist} — {np?.artist}{/if}</span>
          </a>
        {:else if $peerPresence[$activeChat.id]?.status}
          <div class="peer-custom-status">{$peerPresence[$activeChat.id].status}</div>
        {/if}
        <div class="peer-status">
          {#if isGroup && activeGroup}
            {#if typingPeers.has($activeChat.id) && typingWho[$activeChat.id]}
              <span class="typing-ind">{typingWho[$activeChat.id]} {$t("typing…")}</span>
              <span class="sep">·</span>
            {/if}
            <span class="group-sub">
              {$tn(activeGroup.members.length, activeGroup.kind === "channel" ? "subscriber" : "member")}
            </span>
            <span class="sep">·</span>
            <span class="e2ee">{@html icons.lock} encrypted</span>
          {:else}
          {#if typingPeers.has($activeChat.id)}
            <span class="typing-ind">{$t("typing…")}</span>
            <span class="sep">·</span>
          {:else if $peerPresence[$activeChat.id]?.activity?.[0]}
            {@const a = $peerPresence[$activeChat.id].activity![0]}
            <span class="activity-ind" title={a.details ? `${a.details}${a.state ? " — " + a.state : ""}` : a.name}>{activityVerb(a)} <b>{a.name}</b>{#if a.details}: {a.details}{/if}</span>
            <span class="sep">·</span>
          {:else if chatOnline($activeChat, $peerPresence, $presenceNow)}
            {@const st = peerStatus($peerPresence[$activeChat.id], $presenceNow)}
            <span><span class="dot"></span>{$t(st === "idle" ? "away" : st === "dnd" ? "do not disturb" : "online")}</span>
            <span class="sep">·</span>
          {/if}
          {#if $activeChat.live && !$activeChat.has_lock}
            <span class="bot-badge">{$t("🤖 bot · not encrypted")}</span>
          {:else}
            <span class="e2ee">{@html icons.lock} end-to-end encrypted</span>
          {/if}
          {#if $contacts && $activeChat && getContact($activeChat.id)?.phone}
            <span class="sep">·</span>
            <span class="phone" title={$t("Phone from your contact book")}>{getContact($activeChat.id)?.phone}</span>
          {/if}
          {#if $contacts && $activeChat && getContact($activeChat.id)?.full_name}
            <span class="sep">·</span>
            <span class="username" title={$t("Server username")}>@{$activeChat.id}</span>
          {/if}
          {/if}
        </div>
      </div>
      <div class="chat-actions">
        {#if !isGroup}
          <button class="icon-btn" title={$t("Voice call")} onclick={startVoiceCall}>{@html icons.phone}</button>
          <button class="icon-btn" title={$t("Video call (coming soon)")}>{@html icons.video}</button>
        {/if}
        <button class="icon-btn" class:active={searchOpen} title={$t("Search in chat")} onclick={() => (searchOpen ? closeSearch() : openSearch())}>{@html icons.search}</button>
        {#if isGroup && activeGroup}
          {@const gidCall = activeGroup.id}
          {@const chan = activeGroup.kind === "channel"}
          {@const speaker = !chan || isAdmin(activeGroup, myUsername)}
          {#if speaker || $activeGroupCalls[gidCall]}
            <button class="icon-btn" class:active={$groupCall?.gid === gidCall}
              title={chan ? (speaker ? $t("Start a live stream") : $t("Listen to the live stream")) : $t("Voice chat")}
              onclick={() => joinGroupCall(gidCall)}>{@html icons.phone}</button>
          {/if}
          {#if speaker}
            <button class="icon-btn" title={$t("Video chat")} onclick={() => joinGroupCall(gidCall, { video: true })}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="m22 8-6 4 6 4V8Z"/><rect x="2" y="6" width="14" height="12" rx="2"/></svg>
            </button>
          {/if}
          <button class="icon-btn" title={$t("Group settings")} onclick={() => openGroupPanel(activeGroup.id)} aria-label={$t("Group settings")}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/></svg>
          </button>
        {/if}
        <button class="icon-btn" title="Shared media & files" onclick={toggleSharedMedia}>{@html icons.info}</button>
      </div>
    </header>
    {#if isGroup && activeGroup}<GroupCallPanel gid={activeGroup.id} />{/if}

    {#if searchOpen}
      <div class="search-bar">
        <span class="sb-ic">{@html icons.search}</span>
        <input
          id="chat-search-input"
          class="sb-input"
          placeholder={$t("Search in this chat…")}
          bind:value={searchQuery}
          onkeydown={onSearchKey}
        />
        <span class="sb-count">{searchHits.length > 0 ? `${searchIdx + 1}/${searchHits.length}` : (searchQuery.trim() ? "0/0" : "")}</span>
        <button class="sb-btn" title={$t("Previous (Shift+Enter)")} onclick={searchPrev} disabled={searchHits.length === 0}>↑</button>
        <button class="sb-btn" title={$t("Next (Enter)")} onclick={searchNext} disabled={searchHits.length === 0}>↓</button>
        <button class="sb-btn" title={$t("Close (Esc)")} onclick={closeSearch}>×</button>
      </div>
    {/if}

    {#if pinned}
      {@const pv = textOf(pinned) ?? (pinned.kind.type === "poll" ? pinned.kind.content.question : "Message")}
      <div class="pin-bar">
        <button class="pin-main" onclick={scrollToPinned} title={$t("Go to pinned message")}>
          <svg class="pin-ic" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="12" y1="17" x2="12" y2="22"/><path d="M5 17h14l-1.5-5V5a2 2 0 0 0-2-2h-7a2 2 0 0 0-2 2v7z"/></svg>
          <div class="pin-text">
            <div class="pin-title">{$t("Pinned message")}</div>
            <div class="pin-preview">{pv}</div>
          </div>
        </button>
        <button class="pin-x" onclick={unpin} title={$t("Unpin")}>×</button>
      </div>
    {/if}

    {#if selecting}
      <div class="sel-bar">
        <button class="sel-x" onclick={clearSelection} title={$t("Cancel (Esc)")}>×</button>
        <span class="sel-count">{selectedCount()} selected</span>
        <div class="sel-actions">
          {#if !protectedChat}
          <button class="sel-act" onclick={copySelection}>{$t("Copy")}</button>
          <button class="sel-act danger" onclick={() => void deleteSelection()} title={$t("Delete (Del)")}>{$t("Delete")}</button>
          <button class="sel-act primary" onclick={forwardSelection}>{$t("Forward")}</button>
          {/if}
        </div>
      </div>
    {/if}

    <div
      class="messages"
      class:protected={protectedChat}
      class:drag-over={draggingOver}
      class:selecting
      class:dragging
      bind:this={messagesEl}
      onscroll={onMessagesScroll}
      onpointermove={onMessagesPointerMove}
    >
      {#if draggingOver}
        <div class="drop-hint">
          <div class="drop-hint-inner">
            {@html icons.attach}
            <div>{$t("Drop files to send")}</div>
            <div class="drop-sub">{$t("Images, videos, documents — anything")}</div>
          </div>
        </div>
      {/if}
      {#each rows as m (m.id)}
        {#if m.kind.type === "date_separator"}
          <div class="date-sep">{m.kind.content.label}</div>
        {:else if m.kind.type === "system"}
          <div class="sys">{@html icons.lock}<span>{@html m.kind.content.text}</span></div>
        {:else}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <div
            class="msg {m.outgoing ? 'out' : 'in'}"
            data-msg-id={m.id}
            class:first={m.first}
            class:last={m.last}
            class:selected={selected.has(m.id)}
            class:search-hit={searchOpen && searchHitSet.has(m.id)}
            class:search-current={searchOpen && m.id === currentHitId}
            data-id={m.id}
            onclick={() => onMsgClick(m)}
            oncontextmenu={(e) => onMsgContext(e, m)}
            onpointerdown={(e) => onRowPointerDown(e, m)}
          >
            {#if selecting}
              <span class="sel-check" class:on={selected.has(m.id)}>
                {#if selected.has(m.id)}{@html icons.check}{/if}
              </span>
            {/if}
            <div class="avatar sz-34 msg-avatar" style="--gradient: {chatGradient(m.from)}">
              {firstLetter(m.from)}
              {#if $avatars[avatarKey(m.from)]}<img class="ava-img" src={$avatars[avatarKey(m.from)]} alt="" loading="lazy" />{/if}
            </div>
            <div class="msg-col">
              {#if m.first && !m.outgoing && $activeChat}
                {@const mt = isGroup ? tagOf(activeGroup, m.from) : null}
                <div class="msg-header">
                  <span class="msg-author">{isGroup ? m.from : $activeChat.name.split(' ')[0]}</span>
                  {#if $clans[m.from]}<ClanTag tag={$clans[m.from]} />{/if}
                  {#if mt?.tag}<span class="member-tag" class:admin={mt.admin}>{mt.tag}</span>{/if}
                  {#if mt?.title}<span class="admin-title">{mt.title}</span>{/if}
                  <span class="msg-time">{m.time}</span>
                </div>
              {/if}
              {#if m.kind.type === "code"}
                <div class="bubble code"><pre>{m.kind.content.code}</pre></div>
              {:else if m.kind.type === "file"}
                {@const content = m.kind.content}
                {@const rk = renderKind(content)}
                {@const url = inlineUrl(content.saved_at, content.mime)}
                {#if rk === "sticker"}
                  <div class="sticker-wrap">
                    {#if url}
                      <img class="sticker-img" src={url} alt="sticker" />
                    {:else}
                      <div class="sticker-placeholder"></div>
                    {/if}
                    <span class="sticker-meta">
                      {m.time}
                      {@render statusTick(m.outgoing)}
                    </span>
                  </div>
                {:else if rk === "gif" || rk === "image"}
                  <div class="bubble media-bubble">
                    {#if url}
                      {#if rk === "gif" && !$settings.autoplayGifs && !gifPlaying.has(m.id)}
                        <!-- Autoplay off: a still frame until clicked. -->
                        <button class="media-img-btn gif-still" onclick={() => gifPlaying.add(m.id)} title={$t("Play GIF")}>
                          {#await gifStill(url) then still}<img class="media-img" src={still} alt={content.name} />{/await}
                          <span class="gif-badge">GIF ▶</span>
                        </button>
                      {:else}
                        <button class="media-img-btn" onclick={() => openInGallery(content.saved_at)} title={$t("Open")}>
                          <img class="media-img" src={url} alt={content.name} />
                        </button>
                      {/if}
                    {:else}
                      <div class="media-placeholder" style="aspect-ratio: 4/3;"></div>
                    {/if}
                    <div class="media-footer">
                      <span class="media-name" title={content.name}>{content.name}</span>
                      <span class="media-sub">{fmtSize(content.size)}</span>
                      <span class="bubble-meta inline-meta">
                        {m.time}
                        {@render statusTick(m.outgoing)}
                      </span>
                    </div>
                  </div>
                {:else if rk === "circle"}
                  <div class="circle-wrap">
                    {#if url}
                      <!-- svelte-ignore a11y_media_has_caption -->
                      <video class="circle-vid" use:streamSrc={url} loop playsinline autoplay={$settings.autoplayVideos} muted
                        onclick={(e) => { const v = e.currentTarget as HTMLVideoElement; v.muted = !v.muted; }}
                        title={$t("Click to toggle sound")}></video>
                    {:else}
                      <div class="circle-placeholder"></div>
                    {/if}
                    <span class="sticker-meta">
                      {m.time}
                      {@render statusTick(m.outgoing)}
                    </span>
                  </div>
                {:else if rk === "voice" || rk === "audio"}
                  <div class="bubble audio-bubble">
                    {#if rk !== "voice" || !url}<div class="audio-icon">{$t(rk === "voice" ? "🎙" : "🎵")}</div>{/if}
                    <div class="audio-body">
                      {#if url && rk === "voice"}
                        <MediaPlayer src={url} kind="voice" />
                      {:else if url}
                        <button class="bg-play" onclick={() => playChatAudio(content.saved_at)} title={$t("Play in background")}>
                          <svg viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg>
                          {$t("Play")}
                        </button>
                      {:else if !m.outgoing}
                        <button class="file-dl" onclick={() => downloadAttachment(m.id)}>
                          Download &amp; decrypt
                        </button>
                      {:else}
                        <div class="audio-placeholder">{$t("Uploading…")}</div>
                      {/if}
                      <div class="audio-meta">
                        <span class="media-name" title={content.name}>{rk === "voice" ? "Voice message" : content.name}</span>
                        <span class="media-sub">{fmtSize(content.size)}</span>
                        <span class="bubble-meta inline-meta">
                          {m.time}
                          {@render statusTick(m.outgoing)}
                        </span>
                      </div>
                    </div>
                  </div>
                {:else if rk === "video"}
                  <div class="bubble media-bubble">
                    {#if url}
                      <MediaPlayer src={url} kind="video" name={content.name} size={content.size} onExpand={() => openInGallery(content.saved_at)} />
                    {:else if content.saved_at}
                      <div class="media-placeholder" style="aspect-ratio: 16/9;"></div>
                    {:else}
                      <div class="media-placeholder" style="aspect-ratio: 16/9;">
                        {#if !m.outgoing}
                          <button class="file-dl" onclick={() => downloadAttachment(m.id)}>
                            {$t("Download video")}
                          </button>
                        {/if}
                      </div>
                    {/if}
                    <div class="media-footer">
                      <span class="media-name" title={content.name}>{content.name}</span>
                      <span class="media-sub">{fmtSize(content.size)}</span>
                      <span class="bubble-meta inline-meta">
                        {m.time}
                        {@render statusTick(m.outgoing)}
                      </span>
                    </div>
                  </div>
                {:else}
                  {@const ext = (content.name.split(".").pop() ?? "").toUpperCase().slice(0, 4)}
                  <div class="bubble file-bubble">
                    <div class="file-badge">{ext || "FILE"}</div>
                    <div class="file-meta">
                      <div class="file-name" title={content.name}>{content.name}</div>
                      <div class="file-sub">
                        {fmtSize(content.size)}
                        {#if content.mime}· {content.mime}{/if}
                      </div>
                      {#if content.saved_at && /\.aquatheme$/i.test(content.name)}
                        <div class="file-btns">
                          <button class="file-dl primary" onclick={() => applyThemeFile(content.saved_at)}>🎨 {$t("Apply theme")}</button>
                          <button class="file-dl" onclick={() => saveFileAs(content.saved_at, content.name)}>{$t("Save as…")}</button>
                        </div>
                      {:else if content.saved_at}
                        <button class="file-dl" onclick={() => saveFileAs(content.saved_at, content.name)}>
                          {$t("Save as…")}
                        </button>
                      {:else if !m.outgoing}
                        <button class="file-dl" onclick={() => downloadAttachment(m.id)}>
                          Download & decrypt
                        </button>
                      {:else}
                        <div class="file-saved">{$t("Uploaded")}</div>
                      {/if}
                    </div>
                    <span class="bubble-meta">
                      {m.time}
                      {@render statusTick(m.outgoing)}
                    </span>
                  </div>
                {/if}
              {:else if m.kind.type === "article"}
                {@const art = m.kind.content}
                {@const card = articleCard(art)}
                {@const openIt = () => openArticleReader(art, { author: m.outgoing ? $t("You") : m.from, time: m.time })}
                <div class="bubble article-bubble">
                  {#if art.cover_data_url}
                    <button class="art-cover" style={`background-image: url(${objectUrl(art.cover_data_url)});`} onclick={openIt} aria-label={$t("Read article")}></button>
                  {:else if card.slides.length}
                    <div class="art-carousel"><Carousel items={card.slides} aspect={16 / 10} onopen={(i) => openCardPhotos(card.slides, i)} /></div>
                  {/if}
                  <!-- svelte-ignore a11y_click_events_have_key_events -->
                  <!-- svelte-ignore a11y_no_static_element_interactions -->
                  <div class="art-body" onclick={openIt}>
                    <div class="art-tag">📰 {$t("Article")} · {$t("{n} min read", { n: card.readMin })}{card.photos ? ` · ${$t("{n} photos", { n: card.photos })}` : ""}{card.videos ? ` · ${$t("{n} videos", { n: card.videos })}` : ""}</div>
                    <h3 class="art-title">{art.title}</h3>
                    {#if art.subtitle}<div class="art-sub">{art.subtitle}</div>{/if}
                    {#if card.excerpt}<p class="art-excerpt">{card.excerpt}</p>{/if}
                  </div>
                  {#if card.tracks.length}
                    <div class="art-music"><ArticleMusic tracks={card.tracks} title={art.title} cover={art.cover_data_url ? objectUrl(art.cover_data_url) : undefined} compact /></div>
                  {/if}
                  <div class="art-foot">
                    <button class="art-read" onclick={openIt}>⚡ {$t("Read article")}</button>
                    <span class="bubble-meta inline-meta">
                      {m.time}
                      {@render statusTick(m.outgoing)}
                    </span>
                  </div>
                </div>
              {:else if m.kind.type === "poll"}
                {@const poll = m.kind.content}
                {@const pv = viewPoll($pollStates, poll.id, poll.options.length, m.outgoing ? POLL_ME : m.from, poll.close_at)}
                {@const voted = pv.mine.length > 0}
                {@const showRes = voted || pv.closed}
                {@const pick = multiPick[poll.id] ?? []}
                {@const top = Math.max(0, ...pv.counts)}
                <div class="bubble poll-bubble" class:quiz={poll.quiz}>
                  <div class="poll-q">{poll.question}</div>
                  <div class="poll-sub">{pollKindLabel(poll, pv)}</div>
                  <div class="poll-opts">
                    {#each poll.options as opt, i}
                      {@const c = pv.counts[i]}
                      {@const mine = pv.mine.includes(i)}
                      {@const pct = pv.voters > 0 ? Math.round((c / pv.voters) * 100) : 0}
                      {@const right = poll.quiz && showRes && poll.correct === i}
                      {@const wrong = poll.quiz && showRes && mine && poll.correct !== i}
                      <button
                        class="poll-opt"
                        class:mine class:right class:wrong class:results={showRes} class:lead={showRes && c > 0 && c === top}
                        disabled={pv.closed || (showRes && poll.quiz) || (showRes && poll.multi)}
                        onclick={() => votePoll(poll, i)}
                      >
                        <div class="poll-opt-top">
                          {#if showRes}
                            <span class="poll-pct">{pct}%</span>
                          {:else}
                            <span class="poll-mark" class:sq={poll.multi} class:on={pick.includes(i)}></span>
                          {/if}
                          <span class="poll-label">{opt}</span>
                          {#if right}<span class="poll-ic ok">✓</span>{:else if wrong}<span class="poll-ic bad">✕</span>{:else if mine && showRes}<span class="poll-ic">✓</span>{/if}
                        </div>
                        {#if showRes}<div class="poll-bar"><div class="poll-fill" style={`width:${pct}%`}></div></div>{/if}
                        {#if poll.public && showRes && pv.who[i].length}
                          <div class="poll-who">
                            {#each pv.who[i].slice(0, 6) as w (w.voter)}
                              <span class="pw" title={w.at ? `${voterName(w.voter)} · ${new Date(w.at).toLocaleString()}` : voterName(w.voter)}>{voterName(w.voter)}</span>
                            {/each}
                            {#if pv.who[i].length > 6}<span class="pw more">+{pv.who[i].length - 6}</span>{/if}
                          </div>
                        {/if}
                      </button>
                    {/each}
                  </div>
                  {#if poll.quiz && showRes && poll.explanation}
                    <div class="poll-expl"><span>💡</span>{poll.explanation}</div>
                  {/if}
                  {#if poll.multi && !showRes && !pv.closed}
                    <button class="poll-submit" disabled={!pick.length} onclick={() => submitMulti(poll)}>{$t("Vote")}</button>
                  {/if}
                  <div class="poll-foot">
                    <span>{$tn(pv.voters, "vote")}</span>
                    <span class="poll-acts">
                      {#if voted && !pv.closed && !poll.quiz}<button onclick={() => retractPoll(poll)}>{$t("Retract vote")}</button>{/if}
                      {#if m.outgoing && !pv.closed}<button onclick={() => stopPoll(poll)}>{$t("Stop poll")}</button>{/if}
                    </span>
                    <span class="bubble-meta inline-meta">{m.time} {@render statusTick(m.outgoing)}</span>
                  </div>
                </div>
              {:else if m.kind.type === "reply"}
                <div class="bubble">
                  <div class="reply-quote">
                    <span class="rq-author">{m.kind.content.quoted_author}</span>
                    <span class="rq-text">{@render richText(m.kind.content.quoted_text)}</span>
                  </div>
                  {@render msgBody(m.kind.content.text)}
                  <span class="bubble-meta">
                    {@render msgFlags(m)}
                    {m.time}
                    {@render statusTick(m.outgoing)}
                  </span>
                </div>
              {:else if m.kind.type === "text"}
                <div class="bubble">
                  {@render msgBody(m.kind.content.text)}
                  <span class="bubble-meta">
                    {@render msgFlags(m)}
                    {m.time}
                    {@render statusTick(m.outgoing)}
                  </span>
                </div>
              {/if}

              {#if $activeChat}
                {@const chips = reactionChips($reactions[reactionKey(m)])}
                {#if chips.length > 0}
                  <div class="reaction-row">
                    {#each chips as chip (chip.emoji)}
                      {@const ru = reactionUrl(chip.emoji, $reactionPack.items)}
                      {@const src = reactionSource(chip.emoji)}
                      <button class="reaction-chip" class:mine={chip.mine} onclick={() => toggleChipReaction(m, chip.emoji)} oncontextmenu={(e) => reactionInfo(e, chip.emoji)} title={src ? tr("From “{title}” — right-click for the pack", { title: src.title }) : parseReaction(chip.emoji).emoji}>
                        {#if ru}<span class="rc-media"><AnimatedMedia src={ru.url} mime={ru.mime} alt={parseReaction(chip.emoji).emoji} play="auto" /></span>{:else}{parseReaction(chip.emoji).emoji}{/if}{#if chip.count > 1}<span class="rc-count">{chip.count}</span>{/if}
                      </button>
                    {/each}
                  </div>
                {/if}
              {/if}
            </div>
          </div>
        {/if}
      {/each}
    </div>

    {#if scrolledUp}
      <button class="jump-btn" onclick={jumpToBottom} title={$t("Jump to latest")} aria-label={$t("Scroll to bottom")}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
          <polyline points="6 9 12 15 18 9"/>
        </svg>
      </button>
    {/if}

    {#if menu.open && menu.row}
      {@const row = menu.row}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="ctx-menu" use:keepOnScreen style={`left:${menu.x}px; top:${menu.y}px`} role="menu" tabindex="-1" onmousedown={(e) => e.stopPropagation()}>
        <div class="ctx-reactions" class:expanded={reactAll}>
          {#if reactAll && $emojiPacks.length}
            <div class="react-tabs">
              <button class:on={!reactTab} onclick={() => (reactTab = "")}>El Taller</button>
              {#each $emojiPacks as p (p.name)}<button class:on={reactTab === p.name} onclick={() => (reactTab = p.name)} title={p.title}>{p.title}</button>{/each}
            </div>
          {/if}
          {#each filterReactions(reactTokens) as r (r)}
            {@const ru = reactionUrl(r, $reactionPack.items)}
            <button class="ctx-emoji" class:pack={!!ru} onclick={() => reactTo(row, r)} title={`React ${parseReaction(r).emoji}`}>
              {#if ru}<AnimatedMedia src={ru.url} mime={ru.mime} alt={parseReaction(r).emoji} play={reactAll ? "hover" : "auto"} />{:else}{r}{/if}
            </button>
          {/each}
          {#if ($reactionPack.items.length > 8 || $emojiPacks.length) && allowedReactions === null}
            <button class="ctx-emoji more" onclick={() => (reactAll = !reactAll)} title={$t(reactAll ? "Fewer" : "All reactions")}>{$t(reactAll ? "−" : "+")}</button>
          {/if}
        </div>
        <button class="ctx-item" role="menuitem" onclick={() => replyRow(row)}>{$t("Reply")}</button>
        {#if row.kind.type === "file" && row.kind.content.saved_at && !row.outgoing && !protectedChat}
          {@const rk = renderKind(row.kind.content)}
          {#if rk === "sticker"}
            <button class="ctx-item" role="menuitem" onclick={() => saveToCollection(row, "stickers")}>{$t("Add to my stickers")}</button>
          {:else if rk === "gif" || (rk === "video" && (row.kind.content.size ?? 0) < 25 * 1024 * 1024)}
            <button class="ctx-item" role="menuitem" onclick={() => saveToCollection(row, "gifs")}>{$t("Save GIF")}</button>
          {:else if rk === "image"}
            <button class="ctx-item" role="menuitem" onclick={() => saveToCollection(row, "stickers")}>{$t("Add as sticker")}</button>
          {/if}
        {/if}
        {#if textOf(row) != null && /\[ce:[^\]]+\]/.test(textOf(row) ?? "")}
          <button class="ctx-item" role="menuitem" onclick={() => saveEmojiFromRow(row)}>{$t("Add these emoji to my collection")}</button>
        {/if}
        {#if !protectedChat}
        <button class="ctx-item" role="menuitem" onclick={() => forwardRow(row)}>{$t("Forward")}</button>
        {/if}
        {#if textOf(row) != null && !protectedChat}
          <button class="ctx-item" role="menuitem" onclick={() => copyRow(row)}>{$t("Copy text")}</button>
        {/if}
        {#if row.outgoing && (row.kind.type === "text" || row.kind.type === "reply")}
          <button class="ctx-item" role="menuitem" onclick={() => startEdit(row)}>{$t("Edit")}</button>
        {/if}
        {#if row.outgoing || (isGroup && can(activeGroup, myUsername, "delete_messages"))}
          <button class="ctx-item danger" role="menuitem" onclick={() => deleteRow(row)}>{$t("Delete")}</button>
        {/if}
        {#if pinnedId($activeChat?.id ?? "") === row.id}
          <button class="ctx-item" role="menuitem" onclick={() => { unpin(); closeMenu(); }}>{$t("Unpin")}</button>
        {:else}
          <button class="ctx-item" role="menuitem" onclick={() => pinRow(row)}>{$t("Pin")}</button>
        {/if}
        <button class="ctx-item" role="menuitem" onclick={() => { startSelection(row); closeMenu(); }}>{$t("Select")}</button>
      </div>
    {/if}

    {@const liveChat = $activeChat?.live === true}

    {#if !liveChat}
      <div class="not-live-banner">
        <div class="nlb-title">{$t("You can't send messages here")}</div>
        <div class="nlb-sub">
          “{$activeChat.name}” isn't a real conversation — it's just the welcome placeholder.
          Click <b>{$t("+ Start new chat")}</b> {$t("at the top of the sidebar (or type a username in the search bar) to open a real E2EE chat with another user on the server.")}
        </div>
      </div>
    {/if}

    {#if replyTarget}
      <div class="reply-bar">
        <div class="rb-line"></div>
        <div class="rb-body">
          <div class="rb-author">Reply to {replyTarget.author}</div>
          <div class="rb-text">{replyTarget.text}</div>
        </div>
        <button class="rb-x" onclick={cancelReply} title={$t("Cancel reply")}>×</button>
      </div>
    {/if}

    {#if editTarget}
      <div class="reply-bar">
        <div class="rb-line"></div>
        <div class="rb-body">
          <div class="rb-author">{$t("Editing message")}</div>
          <div class="rb-text">{editTarget.old}</div>
        </div>
        <button class="rb-x" onclick={cancelEdit} title={$t("Cancel edit")}>×</button>
      </div>
    {/if}

    {#if recording}
      <div class="rec-bar">
        <button class="rec-trash" onclick={cancelRecord} title={$t("Delete recording")}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"/><path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6"/><path d="M10 11v6M14 11v6"/><path d="M9 6V4a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2"/></svg>
        </button>
        <span class="rec-dot" class:paused={recordPaused}></span>
        <span class="rec-label">{$t(recordPaused ? "Paused" : "Recording")} · {fmtRecord(recordElapsed)}</span>
        <button class="rec-pause" onclick={togglePauseRecord} title={$t(recordPaused ? "Resume" : "Pause")}>
          {#if recordPaused}
            <svg viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg>
          {:else}
            <svg viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="5" width="4" height="14" rx="1"/><rect x="14" y="5" width="4" height="14" rx="1"/></svg>
          {/if}
        </button>
        <button class="rec-send" onclick={toggleRecord} title="Stop &amp; send">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="22" y1="2" x2="11" y2="13"/><polygon points="22 2 15 22 11 13 2 9 22 2"/></svg>
        </button>
      </div>
    {/if}

    {#if !canPost}
      {#if postBlocked === "channel" && $activeChat}
        {@const cid = $activeChat.id}
        <button class="channel-mute" onclick={() => setMuted(cid)}>
          {$t($mutedChats.has(cid) ? "Unmute" : "Mute")}
        </button>
      {:else}
        <div class="channel-locked">
          {@html icons.lock}
          {postBlocked}
        </div>
      {/if}
    {:else}
    <div class="composer" class:disabled={!liveChat}>
      <!-- Attach popup (Media / Article / Poll) -->
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="attach-wrap" onmousedown={(e) => e.stopPropagation()}>
        {#if attachMenuOpen}
          <div class="attach-pop" role="menu" tabindex="-1">
            <button class="ap-item" onclick={() => { attachMenuOpen = false; pickAndSendFile("media"); }}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="3"/><circle cx="9" cy="10" r="2"/><path d="M21 16l-5-5-9 9"/></svg>
              <span>{$t("Photo or video")}</span>
            </button>
            <button class="ap-item" onclick={() => { attachMenuOpen = false; pickAndSendFile("file"); }}>
              {@html icons.attach}<span>{$t("File")}</span>
            </button>
            <button class="ap-item" onclick={() => { attachMenuOpen = false; if ($activeChat) openArticleComposer($activeChat.id); }}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M4 4h12a3 3 0 0 1 3 3v14H7a3 3 0 0 1-3-3V4z"/><path d="M19 7v14"/><line x1="7" y1="9" x2="15" y2="9"/><line x1="7" y1="13" x2="15" y2="13"/></svg>
              <span>{$t("Article")}</span>
            </button>
            <button class="ap-item" onclick={() => { attachMenuOpen = false; if (!canSendKind(activeGroup, myUsername, "poll")) { showToast("Only admins can create polls here"); return; } openPollComposer(); }}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><line x1="6" y1="20" x2="6" y2="12"/><line x1="12" y1="20" x2="12" y2="4"/><line x1="18" y1="20" x2="18" y2="9"/></svg>
              <span>{$t("Poll")}</span>
            </button>
          </div>
        {/if}
        <button
          class="attach-btn"
          class:disabled={!liveChat}
          class:open={attachMenuOpen}
          title={$t(liveChat ? "Attach — media, article, poll" : "Start a real chat first")}
          onclick={() => liveChat && (attachMenuOpen = !attachMenuOpen)}
        >{@html icons.attach}</button>
      </div>
      <button
        class="attach-btn"
        class:disabled={!liveChat}
        title={$t(liveChat ? "Emoji, stickers & GIFs (Ctrl+E)" : "Start a real chat first")}
        data-emoji-anchor
        onclick={() => { if ($pickerState.open) closeMediaPicker(); else if (liveChat && $activeChat) openMediaPicker($activeChat.id); }}
      >{@html icons.sticker}</button>
      <div class="input-wrap">
        {#if slashMatches.length}
          <div class="slash-pop" role="listbox" aria-label={$t("Commands")}>
            {#each slashMatches as c, i (c.cmd)}
              <button class="slash-item" class:on={i === slashIdx} role="option" aria-selected={i === slashIdx}
                onmousedown={(e) => e.preventDefault()} onmouseenter={() => (slashIdx = i)} onclick={() => runSlash(c)}>
                <span class="slash-cmd">/{c.cmd}</span>
                <span class="slash-desc">{c.desc}</span>
              </button>
            {/each}
          </div>
        {/if}
        <RichComposer
          bind:this={composerEl}
          bind:value={text}
          placeholder={liveChat ? `Написать ${$activeChat.name.split(" ")[0]}...` : "Start a real chat first — see the banner above"}
          resolve={(k) => emojiMap[k]}
          oninput={onComposerInput}
          onkeydown={onKey}
        />
      </div>
      {#if text.trim().length === 0 && liveChat}
        <!-- One button: tap switches mic↔camera, hold to record -->
        <button
          class="send-btn media-btn"
          class:recording
          title={$t(mediaMode === "voice" ? "Hold to record voice · tap to switch to video" : "Hold for a round video · tap to switch to voice")}
          onpointerdown={mediaDown}
          onpointermove={mediaMove}
          onpointerup={mediaUp}
        >
          {#if mediaMode === "voice"}
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <path d="M12 1a3 3 0 0 0-3 3v8a3 3 0 0 0 6 0V4a3 3 0 0 0-3-3z"/>
              <path d="M19 10v2a7 7 0 0 1-14 0v-2"/>
              <line x1="12" y1="19" x2="12" y2="23"/>
              <line x1="8" y1="23" x2="16" y2="23"/>
            </svg>
          {:else}
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <path d="M23 7l-7 5 7 5V7z"/>
              <rect x="1" y="5" width="15" height="14" rx="2" ry="2"/>
            </svg>
          {/if}
        </button>
      {:else}
        <button class="send-btn" class:ready={text.trim().length > 0 && slowLeft <= 0} onclick={submit} title={slowLeft > 0 ? `Slow mode: ${slowLeft}s` : "Send"}>
          {#if slowLeft > 0}<span class="slow-left">{slowLeft}</span>{:else}{@html icons.send}{/if}
        </button>
      {/if}
    </div>
    {/if}
    <div class="composer-note">
      {@html icons.lock}
      X3DH · Double Ratchet · your keys never leave this device
      <span class="kbds"><kbd>{$t("Enter")}</kbd> {$t("to send ·")} <kbd>{$t("Shift + Enter")}</kbd> {$t("newline")}</span>
    </div>
  {/if}
</main>

<style>
  .active-chat {
    position: relative; /* anchor the floating jump-to-bottom button */
    display: flex;
    flex-direction: column;
    min-width: 0;
    background:
      radial-gradient(1200px 500px at 80% -20%, color-mix(in srgb, var(--accent) 4%, transparent), transparent),
      radial-gradient(900px 400px at 10% 110%, color-mix(in srgb, var(--accent-2) 4%, transparent), transparent),
      var(--bg-shell);
  }
  .empty {
    place-self: center;
    color: var(--fg-dim);
    padding: 40px;
  }

  /* Header */
  .activity-ind { color: var(--accent); max-width: 320px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .activity-ind b { font-weight: 700; }
  .chat-header.clickable { cursor: pointer; }
  .chat-header.clickable:hover .peer-name { color: var(--accent); }
  .chat-header.clickable:hover { background: color-mix(in srgb, var(--bg-hover) 55%, transparent); }
  .chat-header {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 20px;
    border-bottom: 1px solid var(--border);
    background: rgba(12,15,23,0.7);
    backdrop-filter: blur(12px);
    -webkit-backdrop-filter: blur(12px);
    flex-shrink: 0;
  }
  /* Mobile-only back button (returns to the chat list). */
  .back-btn {
    display: none;
    width: 36px; height: 36px;
    background: transparent; border: none;
    color: var(--fg-muted); cursor: pointer;
    border-radius: 9px; flex-shrink: 0;
    align-items: center; justify-content: center;
  }
  .back-btn:hover { background: var(--bg-hover); color: var(--fg); }
  .back-btn svg { width: 22px; height: 22px; }
  @media (max-width: 720px) { .back-btn { display: inline-flex; } }
  /* Phone header: one line of status, compact action buttons. */
  @media (max-width: 720px) {
    .chat-header { gap: 6px; padding-left: 4px; padding-right: 6px; }
    .peer-status { flex-wrap: nowrap; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
    .chat-header .peer-status .e2ee, .chat-header .peer-status .sep { display: none !important; }
    .chat-actions { gap: 0; }
    .chat-actions .icon-btn { width: 34px; height: 34px; }
    .kbds { display: none; }
  }

  .peer-info { flex: 1; min-width: 0; }
  .peer-name {
    font-size: 15.5px;
    font-weight: 700;
    margin: 0;
    color: var(--fg);
    display: flex;
    align-items: center;
    gap: 5px;
    letter-spacing: -0.01em;
  }
  .verified-btn {
    background: transparent;
    border: none;
    padding: 0;
    cursor: pointer;
    display: inline-flex;
  }
  .verified-badge :global(svg) { width: 14px; height: 14px; color: var(--accent); filter: drop-shadow(0 0 4px var(--accent-glow)); }
  .now-playing {
    display: inline-flex; align-items: center; gap: 6px;
    margin-top: 2px; max-width: 320px;
    font-size: 12px; color: var(--accent); text-decoration: none;
  }
  .np-note { animation: np-bounce 1.4s ease-in-out infinite; }
  @keyframes np-bounce { 0%,100% { transform: translateY(0); } 50% { transform: translateY(-2px); } }
  .np-text { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .now-playing:hover .np-text { text-decoration: underline; }
  .peer-custom-status { margin-top: 2px; font-size: 12px; color: var(--fg-muted); }

  .peer-status {
    font-size: 12.5px;
    color: var(--fg-muted);
    margin-top: 2px;
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .peer-status .dot {
    width: 6px; height: 6px; border-radius: 50%;
    background: var(--online);
    box-shadow: 0 0 6px var(--online);
    display: inline-block;
  }
  .peer-status .sep { color: var(--fg-dim); }
  .peer-status .e2ee {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: var(--accent);
    font-weight: 500;
  }
  .peer-status .e2ee :global(svg) { width: 11px; height: 11px; }

  .avatar-btn { border: none; cursor: pointer; padding: 0; font: inherit; color: #06070C; }
  .group-name-btn { background: none; border: none; padding: 0; font: inherit; color: inherit; cursor: pointer; }
  .group-name-btn:hover { color: var(--accent); }
  .group-sub { background: none; border: none; padding: 0; font: inherit; color: var(--fg-muted); cursor: pointer; }
  .group-sub:hover { color: var(--accent); }
  .channel-locked {
    display: flex; align-items: center; justify-content: center; gap: 8px;
    padding: 16px; margin: 10px 14px; border-radius: 12px;
    background: var(--bg-input); border: 1px solid var(--border);
    color: var(--fg-muted); font-size: 13px;
  }
  .channel-locked :global(svg) { width: 14px; height: 14px; color: var(--fg-dim); }
  .channel-mute {
    margin: 10px 14px; padding: 13px; border-radius: 12px; cursor: pointer;
    background: var(--bg-input); border: 1px solid var(--border);
    color: var(--accent); font: inherit; font-weight: 800; font-size: 14px; letter-spacing: 0.02em;
    transition: background 0.15s;
  }
  .channel-mute:hover { background: color-mix(in srgb, var(--accent) 10%, var(--bg-input)); }
  .messages.protected { user-select: none; -webkit-user-select: none; }
  .member-tag {
    font-size: 10.5px; font-weight: 700; padding: 1px 6px; border-radius: 5px;
    color: var(--fg-muted); background: var(--bg-input); border: 1px solid var(--border);
  }
  .member-tag.admin { color: var(--accent); border-color: color-mix(in srgb, var(--accent) 40%, transparent); background: color-mix(in srgb, var(--accent) 12%, transparent); }
  .admin-title { font-size: 11px; color: var(--fg-dim); font-style: italic; }
  .mflag { font-size: 10.5px; font-style: italic; color: var(--fg-dim); margin-right: 4px; cursor: help; }
  .mflag.del { color: #ff8f8f; }
  .slow-left { font-size: 12px; font-weight: 800; color: var(--fg-muted); font-variant-numeric: tabular-nums; }
  .hdr-clan { margin-left: 6px; display: inline-flex; vertical-align: middle; }

  .chat-actions { display: flex; gap: 4px; }
  .peer-status .bot-badge {
    color: #ffb067;
    font-weight: 600;
  }
  .peer-status .phone {
    font-variant-numeric: tabular-nums;
    color: var(--fg-muted);
  }
  .peer-status .username {
    font-family: var(--font-mono);
    color: var(--fg-dim);
    font-size: 11.5px;
  }
  .icon-btn {
    width: 34px; height: 34px;
    display: grid; place-items: center;
    background: transparent;
    border: none;
    border-radius: 8px;
    color: var(--fg-muted);
    cursor: pointer;
    transition: all 0.12s ease;
  }
  .icon-btn:hover { background: var(--bg-hover); color: var(--fg); }
  .icon-btn :global(svg) { width: 18px; height: 18px; }

  /* In-chat search */
  .icon-btn.active { background: var(--accent-soft); color: var(--accent); }
  .search-bar {
    display: flex; align-items: center; gap: 8px;
    padding: 8px 14px; border-bottom: 1px solid var(--border);
    background: var(--bg-list);
  }
  .search-bar .sb-ic { display: inline-flex; color: var(--fg-dim); }
  .search-bar .sb-ic :global(svg) { width: 15px; height: 15px; }
  .sb-input {
    flex: 1; min-width: 0; background: var(--bg-input); border: 1px solid var(--border);
    border-radius: 9px; color: var(--fg); font: inherit; font-size: 13.5px; padding: 7px 11px; outline: none;
  }
  .sb-input:focus { border-color: var(--accent); }
  .sb-count { font-size: 12px; color: var(--fg-dim); font-variant-numeric: tabular-nums; min-width: 34px; text-align: center; }
  .sb-btn {
    width: 28px; height: 28px; border-radius: 7px; border: 1px solid var(--border);
    background: transparent; color: var(--fg-muted); cursor: pointer; font-size: 15px; line-height: 1;
  }
  .sb-btn:hover:not(:disabled) { color: var(--fg); border-color: var(--border-strong); }
  .sb-btn:disabled { opacity: 0.4; cursor: default; }
  .msg.search-hit .bubble { outline: 2px solid color-mix(in srgb, var(--accent) 45%, transparent); outline-offset: 1px; }
  .msg.search-current .bubble { outline: 2px solid var(--accent); box-shadow: 0 0 0 4px var(--accent-soft); }

  /* Messages */
  .messages {
    flex: 1;
    overflow-y: auto;
    padding: 12px 0 8px;
    display: flex;
    flex-direction: column;
    background: var(--chat-wallpaper, transparent);
    background-size: cover;
    background-position: center;
    background-attachment: local;
  }

  .date-sep {
    align-self: center;
    font-size: 11.5px;
    font-weight: 600;
    color: var(--fg-dim);
    text-transform: uppercase;
    letter-spacing: 0.08em;
    padding: 14px 12px 6px;
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .date-sep::before, .date-sep::after {
    content: '';
    display: block;
    width: 40px; height: 1px;
    background: var(--border);
  }

  .sys {
    align-self: center;
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 6px 14px;
    margin: 12px 20px;
    font-size: 12px;
    color: var(--fg-muted);
    background: color-mix(in srgb, var(--accent) 6%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent) 12%, transparent);
    border-radius: 100px;
  }
  .sys :global(svg) { width: 12px; height: 12px; color: var(--accent); }

  .msg {
    display: grid;
    grid-template-columns: 34px 1fr;
    column-gap: 10px;
    row-gap: 0;
    padding: 0 20px;
    max-width: 100%;
  }
  /* Telegram-style grouping: messages in one block nearly touch (2px);
     a new block (sender change or a new minute) gets real separation. */
  .msg + .msg { margin-top: 2px; }
  .msg.first  { margin-top: 10px; }
  .msg.last   { padding-bottom: 0; }
  /* Every grid child is pinned to ROW 1 with an explicit column. Without
     this, an outgoing row (avatar has an explicit column, bubble doesn't)
     lets the auto-placement cursor push the bubble onto a second row, leaving
     an empty 34px avatar row + row-gap above every outgoing message. */
  .msg > * { grid-row: 1; }
  .msg-avatar { grid-column: 1; align-self: end; }
  .msg-col    { grid-column: 2; }
  /* Telegram: the sender's avatar sits at the BOTTOM of a block (last
     message); own messages carry no avatar and hug the right edge. */
  .msg:not(.last) .msg-avatar { visibility: hidden; }
  .msg.out .msg-avatar { display: none; }
  /* align-items:flex-start — bubbles hug their text (Telegram), instead of
     stretching every incoming message to the max width. */
  .msg-col { min-width: 0; display: flex; flex-direction: column; align-items: flex-start; gap: 2px; }
  .msg-header {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 2px 2px;
  }
  .msg-author { font-size: 13.5px; font-weight: 700; color: var(--fg); }
  .msg-time { font-size: 11px; color: var(--fg-dim); font-variant-numeric: tabular-nums; }

  .bubble {
    background: var(--bg-bubble-in);
    padding: 8px 12px;
    border-radius: 4px var(--bubble-r, 14px) var(--bubble-r, 14px) var(--bubble-r, 14px);
    max-width: 620px;
    word-wrap: break-word;
    overflow-wrap: break-word;
    font-size: 14.5px;
    line-height: 1.45;
    color: var(--fg);
    border: 1px solid var(--border);
    box-shadow: var(--shadow-lift);
    animation: msg-in 0.25s ease-out;
  }
  @keyframes msg-in {
    from { opacity: 0; transform: translateY(4px); }
    to   { opacity: 1; transform: translateY(0); }
  }
  .msg:not(.first) .bubble { border-top-left-radius: var(--bubble-r, 14px); }

  .msg.out { grid-template-columns: 1fr; }
  .msg.out .msg-avatar { grid-column: 2; }
  .msg.out .msg-col { grid-column: 1; align-items: flex-end; }
  .msg.out .bubble {
    background:
      linear-gradient(135deg, color-mix(in srgb, var(--accent) 16%, transparent), color-mix(in srgb, var(--accent-2) 14%, transparent)),
      var(--bg-bubble-in);
    border: 1px solid color-mix(in srgb, var(--accent) 22%, transparent);
    border-radius: var(--bubble-r, 14px) 4px var(--bubble-r, 14px) var(--bubble-r, 14px);
    color: var(--fg);
  }
  .msg.out:not(.first) .bubble { border-top-right-radius: var(--bubble-r, 14px); }

  .bubble-meta {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 10.5px;
    color: var(--fg-dim);
    margin-left: 8px;
    float: right;
    padding-top: 4px;
    padding-left: 8px;
    user-select: none;
    font-variant-numeric: tabular-nums;
  }
  .msg.out .bubble-meta { color: var(--accent); opacity: 0.85; }
  .bubble-meta :global(svg) { width: 12px; height: 12px; }

  .bubble.code {
    font-family: var(--font-mono);
    font-size: 12.5px;
    background: #0A0D14;
    border: 1px solid var(--border-strong);
    padding: 12px 14px;
    color: #C7D6F0;
    max-width: 560px;
    line-height: 1.5;
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent) 6%, transparent), 0 8px 24px -8px rgba(0,0,0,0.5);
  }
  .bubble.code pre { margin: 0; white-space: pre; overflow-x: auto; }
  .msg.out .bubble.code { border-color: color-mix(in srgb, var(--accent) 22%, transparent); }

  /* File bubble */
  .bubble.file-bubble {
    display: grid;
    grid-template-columns: 40px 1fr;
    gap: 12px;
    align-items: center;
    min-width: 240px;
    max-width: 420px;
    padding: 10px 14px;
  }
  .file-badge {
    display: grid; place-items: center;
    width: 40px; height: 40px;
    border-radius: 10px;
    background: linear-gradient(135deg, var(--accent), var(--accent-2));
    color: #06070C;
    font-size: 10px;
    font-weight: 800;
    letter-spacing: 0.02em;
  }
  .file-icon {
    display: grid; place-items: center;
    width: 40px; height: 40px;
    border-radius: 10px;
    background: color-mix(in srgb, var(--accent) 10%, transparent);
    color: var(--accent);
  }
  .file-icon :global(svg) { width: 20px; height: 20px; }
  .msg.out .file-icon { background: rgba(255,255,255,0.08); color: var(--fg); }
  .file-meta { min-width: 0; display: flex; flex-direction: column; gap: 2px; }
  .file-name {
    font-weight: 600;
    font-size: 13.5px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .file-sub {
    font-size: 11.5px;
    color: var(--fg-dim);
    font-variant-numeric: tabular-nums;
  }
  .msg.out .file-sub { color: rgba(255,255,255,0.6); }
  .file-dl {
    margin-top: 4px;
    align-self: flex-start;
    padding: 4px 10px;
    font-size: 11.5px;
    font-weight: 600;
    border-radius: 6px;
    border: 1px solid var(--accent-soft);
    background: var(--accent-soft);
    color: var(--accent);
    cursor: pointer;
    transition: all 0.12s ease;
  }
  .file-dl:hover {
    background: color-mix(in srgb, var(--accent) 22%, transparent);
    box-shadow: 0 0 10px -2px var(--accent-glow);
  }
  .file-saved {
    margin-top: 4px;
    font-size: 11px;
    color: var(--fg-dim);
    font-style: italic;
  }
  .msg.out .file-saved { color: rgba(255,255,255,0.55); }

  /* Drag-and-drop overlay. */
  .messages.drag-over {
    outline: 2px dashed var(--accent);
    outline-offset: -8px;
    background:
      radial-gradient(600px 300px at 50% 50%, color-mix(in srgb, var(--accent) 8%, transparent), transparent),
      var(--bg-shell);
  }
  .drop-hint {
    position: sticky;
    top: 20px;
    align-self: center;
    pointer-events: none;
    z-index: 10;
  }
  .drop-hint-inner {
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent) 30%, transparent);
    color: var(--fg);
    padding: 14px 24px;
    border-radius: 14px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    font-weight: 600;
    font-size: 14px;
    box-shadow: 0 8px 24px -8px rgba(0, 0, 0, 0.5);
    backdrop-filter: blur(10px);
  }
  .drop-hint-inner :global(svg) { width: 22px; height: 22px; color: var(--accent); margin-bottom: 2px; }
  .drop-sub { font-size: 11.5px; color: var(--fg-dim); font-weight: 400; }

  /* Circle video messages — small round video, no bubble. */
  .circle-wrap {
    display: inline-flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
    animation: msg-in 0.25s ease-out;
  }
  .msg.out .circle-wrap { align-items: flex-end; }
  .circle-vid {
    width: 220px; height: 220px;
    border-radius: 50%;
    object-fit: cover;
    background: #000;
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 20%, transparent), 0 10px 24px -6px rgba(0,0,0,0.5);
  }
  .circle-placeholder {
    width: 220px; height: 220px;
    border-radius: 50%;
    background: linear-gradient(90deg, var(--bg-input), var(--bg-hover), var(--bg-input));
    background-size: 200% 100%;
    animation: shimmer 1.4s linear infinite;
  }

  /* Stickers — no bubble, just the image. */
  .sticker-wrap {
    display: inline-flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
    animation: msg-in 0.25s ease-out;
  }
  .msg.out .sticker-wrap { align-items: flex-end; }
  .sticker-img {
    width: var(--sticker-size, 128px);
    height: var(--sticker-size, 128px);
    object-fit: contain;
    display: block;
    filter: drop-shadow(0 4px 10px rgba(0, 0, 0, 0.35));
  }
  .sticker-placeholder {
    width: var(--sticker-size, 128px); height: var(--sticker-size, 128px);
    border-radius: 14px;
    background: linear-gradient(90deg, var(--bg-input), var(--bg-hover), var(--bg-input));
    background-size: 200% 100%;
    animation: shimmer 1.4s linear infinite;
  }
  @keyframes shimmer {
    from { background-position: 200% 0; }
    to   { background-position: -200% 0; }
  }
  .sticker-meta {
    font-size: 10.5px;
    color: var(--fg-dim);
    padding: 0 4px;
    font-variant-numeric: tabular-nums;
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  .msg.out .sticker-meta { color: var(--accent); opacity: 0.85; }
  .gif-still { position: relative; }
  .react-tabs { flex: 1 0 100%; display: flex; gap: 4px; overflow-x: auto; padding: 0 0 6px; scrollbar-width: none; }
  .react-tabs button {
    flex: 0 0 auto; max-width: 140px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    border: 1px solid var(--border); border-radius: 999px; background: transparent; color: var(--fg-muted);
    font-size: 11.5px; font-weight: 600; padding: 3px 9px; cursor: pointer;
  }
  .react-tabs button.on { background: var(--accent-soft); color: var(--accent); border-color: transparent; }
  .file-btns { display: flex; gap: 6px; flex-wrap: wrap; }
  .file-dl.primary { background: var(--accent); color: #06070c; border-color: transparent; }
  .gif-badge {
    position: absolute; left: 50%; top: 50%; transform: translate(-50%, -50%);
    padding: 6px 12px; border-radius: 999px;
    background: rgba(0, 0, 0, 0.6); color: #fff; font-size: 12px; font-weight: 800; letter-spacing: 0.04em;
  }
  .sticker-meta :global(svg) { width: 12px; height: 12px; }

  /* Media bubble — images, GIFs, videos. */
  .bubble.media-bubble {
    padding: 4px;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    gap: 0;
    max-width: var(--media-max, 360px);
    min-width: 180px;
  }
  .media-img-btn {
    border: none; padding: 0; margin: 0; background: transparent;
    cursor: zoom-in; display: block; border-radius: 10px; overflow: hidden;
  }
  .media-img {
    max-width: 100%;
    max-height: 400px;
    width: auto;
    object-fit: contain;
    display: block;
    border-radius: 10px;
    background: #0A0D14;
  }
  .media-video {
    max-width: 100%;
    max-height: 400px;
    width: 100%;
    display: block;
    border-radius: 10px;
    background: #000;
  }
  .media-placeholder {
    width: 100%;
    border-radius: 10px;
    background: linear-gradient(90deg, var(--bg-input), var(--bg-hover), var(--bg-input));
    background-size: 200% 100%;
    animation: shimmer 1.4s linear infinite;
    display: grid; place-items: center;
  }
  .media-footer {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 8px 2px;
    font-size: 11.5px;
    color: var(--fg-dim);
    min-width: 0;
  }
  .msg.out .media-footer { color: rgba(255,255,255,0.6); }
  .media-name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .media-sub { font-variant-numeric: tabular-nums; flex-shrink: 0; }
  .inline-meta { float: none; margin-left: 4px; padding: 0; }

  /* "/" command popup */
  .input-wrap { position: relative; }
  .slash-pop {
    position: absolute; left: -6px; right: -6px; bottom: calc(100% + 10px); z-index: 30;
    max-height: 300px; overflow-y: auto;
    padding: 5px;
    background: var(--bg-surface-2, var(--bg-list));
    border: 1px solid var(--border-strong);
    border-radius: 12px;
    box-shadow: 0 16px 40px -12px rgba(0, 0, 0, 0.55);
  }
  .slash-item {
    width: 100%; display: flex; align-items: baseline; gap: 12px;
    padding: 7px 10px; border: none; border-radius: 8px;
    background: transparent; color: var(--fg); text-align: left; cursor: pointer;
  }
  .slash-item.on { background: var(--accent-soft); }
  .slash-cmd { font-weight: 700; color: var(--accent); font-family: var(--font-mono); font-size: 13px; min-width: 96px; }
  .slash-desc { font-size: 12.5px; color: var(--fg-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }

  /* Article bubble — Telegra.ph-style card; opens the full-screen reader. */
  .bubble.article-bubble {
    padding: 0;
    overflow: hidden;
    width: min(440px, 100%);
    display: block;
  }
  .art-cover {
    all: unset; cursor: pointer; display: block;
    width: 100%;
    aspect-ratio: 16 / 7;
    background-size: cover;
    background-position: center;
    border-bottom: 1px solid var(--border);
  }
  .art-carousel { padding: 6px 6px 0; }
  .art-carousel :global(.stage) { border-radius: 10px; }
  .art-body { padding: 12px 14px 4px; cursor: pointer; }
  .art-tag {
    font-size: 11px; font-weight: 700; color: var(--accent);
    text-transform: uppercase; letter-spacing: 0.06em; margin-bottom: 6px;
  }
  .art-title { margin: 0 0 4px; font-size: 18px; font-weight: 800; letter-spacing: -0.01em; line-height: 1.25; }
  .art-sub { font-size: 13.5px; color: var(--fg-muted); margin-bottom: 4px; }
  .art-excerpt {
    margin: 4px 0 0; font-size: 13px; line-height: 1.5; color: var(--fg-muted);
    display: -webkit-box; -webkit-line-clamp: 4; line-clamp: 4; -webkit-box-orient: vertical; overflow: hidden;
  }
  .art-music { padding: 0 8px; }
  .art-foot { display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: 6px 10px 10px 14px; }
  .art-read {
    padding: 6px 12px; border-radius: 8px; border: none; cursor: pointer;
    background: color-mix(in srgb, var(--accent) 16%, transparent); color: var(--accent);
    font-weight: 800; font-size: 12px; letter-spacing: 0.04em; text-transform: uppercase;
  }
  .art-read:hover { background: color-mix(in srgb, var(--accent) 26%, transparent); }

  /* Voice-recording bar — shown above the composer while mic is live. */
  .rec-bar {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 16px;
    margin: 0 20px 10px;
    background: linear-gradient(135deg, rgba(255, 77, 109, 0.15), rgba(255, 77, 109, 0.08));
    border: 1px solid rgba(255, 77, 109, 0.3);
    border-radius: 12px;
    color: var(--fg);
    font-size: 13px;
    animation: rec-in 0.18s ease-out;
  }
  @keyframes rec-in {
    from { opacity: 0; transform: translateY(4px); }
    to   { opacity: 1; transform: translateY(0); }
  }
  .rec-dot {
    width: 10px; height: 10px;
    border-radius: 50%;
    background: var(--danger);
    box-shadow: 0 0 10px var(--danger);
    animation: rec-pulse 1s ease-in-out infinite;
  }
  @keyframes rec-pulse {
    0%, 100% { opacity: 1; transform: scale(1); }
    50%      { opacity: 0.4; transform: scale(0.7); }
  }
  .rec-dot.paused { animation: none; opacity: 0.5; }
  .rec-label { flex: 1; font-weight: 600; font-variant-numeric: tabular-nums; }
  .rec-trash, .rec-pause, .rec-send {
    width: 34px; height: 34px; flex-shrink: 0;
    display: grid; place-items: center;
    border-radius: 50%; cursor: pointer; border: none;
  }
  .rec-trash svg, .rec-pause svg, .rec-send svg { width: 17px; height: 17px; }
  .rec-trash { background: transparent; border: 1px solid var(--border-strong); color: var(--fg-muted); }
  .rec-trash:hover { color: var(--danger); border-color: var(--danger); }
  .rec-pause { background: var(--bg-input); border: 1px solid var(--border-strong); color: var(--fg); }
  .rec-pause:hover { color: var(--accent); border-color: var(--accent-soft); }
  .rec-send { background: linear-gradient(135deg, var(--accent), var(--accent-2)); color: #06070C; box-shadow: 0 4px 10px -3px var(--accent-glow); }
  .rec-send:hover { transform: scale(1.05); }

  .send-btn.media-btn { color: var(--accent); touch-action: none; }
  .send-btn.media-btn:hover { background: var(--accent-soft); border-color: var(--accent-soft); }
  .send-btn.media-btn.recording {
    color: #06070C;
    background: var(--danger);
    border-color: var(--danger);
    box-shadow: 0 0 12px -2px var(--danger);
    animation: rec-pulse 1s ease-in-out infinite;
  }

  /* Attach popup (Media / Article / Poll) */
  .attach-wrap { position: relative; display: flex; }
  .attach-btn.open { color: var(--accent); background: var(--accent-soft); }
  .attach-pop {
    position: absolute; bottom: calc(100% + 8px); left: 0;
    min-width: 180px;
    background: var(--bg-surface-2);
    border: 1px solid var(--border-strong);
    border-radius: 12px; padding: 5px;
    box-shadow: 0 16px 40px -10px rgba(0,0,0,0.6);
    z-index: 30;
    animation: ap-in 0.12s ease-out;
  }
  @keyframes ap-in { from { opacity: 0; transform: translateY(4px); } to { opacity: 1; transform: translateY(0); } }
  .ap-item {
    display: flex; align-items: center; gap: 10px; width: 100%;
    padding: 9px 12px; border: none; background: transparent;
    color: var(--fg); font-size: 13px; font-weight: 500;
    border-radius: 8px; cursor: pointer; text-align: left;
  }
  .ap-item:hover { background: var(--accent-soft); color: var(--accent); }
  .ap-item :global(svg) { width: 18px; height: 18px; flex-shrink: 0; }

  /* Audio & voice-message bubble. */
  .bubble.audio-bubble {
    display: grid;
    grid-template-columns: 36px 1fr;
    gap: 12px;
    align-items: center;
    min-width: 280px;
    max-width: 380px;
    padding: 10px 14px;
  }
  .audio-icon {
    width: 36px; height: 36px;
    border-radius: 50%;
    background: linear-gradient(135deg, var(--accent) 0%, var(--accent-2) 100%);
    display: grid; place-items: center;
    font-size: 17px;
    color: #06070C;
    box-shadow: 0 4px 10px -3px var(--accent-glow);
  }
  .audio-body { display: flex; flex-direction: column; gap: 6px; min-width: 0; }
  .bg-play {
    display: inline-flex; align-items: center; gap: 6px;
    align-self: flex-start;
    padding: 6px 14px 6px 10px;
    border-radius: 100px;
    border: none;
    background: linear-gradient(135deg, var(--accent), var(--accent-2));
    color: #06070C; font-weight: 700; font-size: 12.5px; cursor: pointer;
  }
  .bg-play svg { width: 15px; height: 15px; }
  .bg-play:hover { filter: brightness(1.07); }
  .audio-player {
    width: 100%;
    height: 32px;
  }
  .audio-placeholder {
    font-size: 11.5px;
    color: var(--fg-dim);
    font-style: italic;
    padding: 6px 0;
  }
  .audio-meta {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 11px;
    color: var(--fg-dim);
    min-width: 0;
  }

  /* Floating "jump to latest" button, shown while the user has
     scrolled a viewport+ away from the bottom of the chat. */
  .jump-btn {
    position: absolute;
    right: 20px;
    bottom: 160px;
    width: 42px; height: 42px;
    display: grid; place-items: center;
    background: var(--bg-surface-2);
    color: var(--accent);
    border: 1px solid var(--border-strong);
    border-radius: 50%;
    cursor: pointer;
    box-shadow: 0 8px 24px -6px rgba(0, 0, 0, 0.45);
    backdrop-filter: blur(8px);
    animation: jump-in 0.18s ease-out;
    z-index: 5;
    transition: transform 0.15s ease, color 0.15s ease, border-color 0.15s ease;
  }
  .jump-btn:hover {
    transform: translateY(-2px);
    color: var(--fg);
    border-color: var(--accent);
    box-shadow: 0 10px 28px -4px var(--accent-glow);
  }
  .jump-btn svg { width: 20px; height: 20px; }

  /* --- Selection bar --- */
  .sel-bar {
    display: flex; align-items: center; gap: 12px;
    padding: 10px 16px;
    background: var(--bg-list);
    border-bottom: 1px solid var(--border);
    z-index: 6;
  }
  .sel-x {
    width: 28px; height: 28px;
    background: transparent; border: 1px solid var(--border);
    border-radius: 8px; color: var(--fg-muted); font-size: 17px; cursor: pointer;
  }
  .sel-x:hover { color: var(--fg); background: var(--bg-hover); }
  .sel-count { flex: 1; font-size: 13.5px; font-weight: 600; color: var(--fg); }
  .sel-actions { display: flex; gap: 6px; }
  .sel-act {
    padding: 6px 14px; border-radius: 8px;
    border: 1px solid var(--border); background: transparent;
    color: var(--fg-muted); font-size: 12.5px; font-weight: 600; cursor: pointer;
  }
  .sel-act:hover { color: var(--fg); border-color: var(--border-strong); }
  .sel-act.primary {
    border-color: var(--accent-soft); background: var(--accent-soft); color: var(--accent);
  }
  .sel-act.danger { color: #ff6b6b; border-color: color-mix(in srgb, #ff6b6b 40%, transparent); }
  .sel-act.danger:hover { background: color-mix(in srgb, #ff6b6b 14%, transparent); color: #ff8787; }
  .sel-act.primary:hover { background: color-mix(in srgb, var(--accent) 22%, transparent); }

  /* --- Per-message selection --- */
  .messages.selecting .msg { cursor: pointer; }
  /* Selection adds a checkbox as a 3rd grid child. Widen the template to a
     real 3-column grid so the bubble never wraps to a new row (which is what
     was blowing the vertical spacing wide open). */
  /* The check sits in the row's left gutter — entering selection mode
     must not re-flow the whole list (that was the jerk). */
  .msg { position: relative; }
  .sel-check { position: absolute; left: 0; top: 50%; transform: translateY(-50%); }
  .msg.selected { background: var(--accent-soft); border-radius: 10px; }
  .sel-check {
    align-self: center;
    width: 20px; height: 20px; flex-shrink: 0;
    border-radius: 50%;
    border: 2px solid var(--border-strong);
    display: grid; place-items: center;
  }
  .sel-check.on { background: var(--accent); border-color: var(--accent); }
  .sel-check :global(svg) { width: 12px; height: 12px; color: #06070C; }

  /* --- Custom (non-Chromium) context menu --- */
  .ctx-menu {
    position: fixed;
    z-index: 120;
    min-width: 170px;
    background: var(--bg-surface-2);
    border: 1px solid var(--border-strong);
    border-radius: 10px;
    padding: 5px;
    box-shadow: 0 16px 40px -10px rgba(0,0,0,0.6);
    animation: ctx-in 0.1s ease-out;
  }
  @keyframes ctx-in { from { opacity: 0; transform: scale(0.97); } to { opacity: 1; transform: scale(1); } }
  .ctx-item {
    display: block; width: 100%; text-align: left;
    padding: 8px 12px; border: none; background: transparent;
    color: var(--fg); font-size: 13px; font-weight: 500;
    border-radius: 7px; cursor: pointer;
  }
  .ctx-item:hover { background: var(--accent-soft); color: var(--accent); }
  .ctx-item.danger { color: #ff8f8f; }
  .ctx-item.danger:hover { background: rgba(255,77,109,0.1); color: #ff6b6b; }
  .typing-ind { color: var(--accent); font-weight: 600; }
  .ctx-reactions {
    display: flex; gap: 2px; padding: 4px 4px 6px; margin-bottom: 4px;
    border-bottom: 1px solid var(--border); flex-wrap: wrap;
  }
  .ctx-emoji {
    width: 30px; height: 30px; border: none; background: transparent;
    border-radius: 8px; font-size: 17px; cursor: pointer; line-height: 1;
  }
  .ctx-emoji:hover { background: var(--bg-hover); transform: scale(1.15); }
  .ctx-emoji.pack { padding: 2px; }
  .ctx-emoji.more { font-size: 18px; color: var(--fg-muted); font-weight: 700; }
  .ctx-reactions.expanded { max-height: 220px; overflow-y: auto; width: 272px; }
  .rc-media { width: 18px; height: 18px; display: inline-block; vertical-align: middle; }

  /* Reaction chips under a bubble */
  /* Quoted reply inside a bubble */
  .reply-quote {
    display: block;
    border-left: 3px solid var(--accent);
    padding: 3px 0 3px 8px; margin-bottom: 5px;
    background: color-mix(in srgb, var(--accent) 8%, transparent);
    border-radius: 0 6px 6px 0; max-width: 100%;
  }
  .rq-author { display: block; font-size: 11.5px; font-weight: 700; color: var(--accent); }
  .rq-text { display: block; font-size: 12.5px; color: var(--fg-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

  .ce-preview {
    position: absolute; left: 0; right: 0; bottom: calc(100% + 6px);
    max-height: 96px; overflow: auto;
    padding: 6px 10px; border-radius: 10px;
    background: var(--bg-surface); border: 1px solid var(--border);
    font-size: 14px; line-height: 1.45; color: var(--fg);
    white-space: pre-wrap; word-break: break-word; pointer-events: none;
    box-shadow: 0 8px 20px -10px rgba(0, 0, 0, 0.6);
  }
  /* Inline custom emoji inside message text */
  :global(.inline-emoji) {
    height: 1.3em;
    width: 1.3em;
    vertical-align: -0.28em;
    object-fit: contain;
    margin: 0 1px;
    display: inline-block;
  }

  /* Reply composer bar */
  .reply-bar {
    display: flex; align-items: center; gap: 10px;
    padding: 8px 16px; margin: 0 20px 8px;
    background: var(--bg-list); border: 1px solid var(--border);
    border-radius: 10px;
  }
  .rb-line { width: 3px; align-self: stretch; background: var(--accent); border-radius: 3px; flex-shrink: 0; }
  .rb-body { flex: 1; min-width: 0; }
  .rb-author { font-size: 11.5px; font-weight: 700; color: var(--accent); }
  .rb-text { font-size: 12.5px; color: var(--fg-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .rb-x { width: 28px; height: 28px; background: transparent; border: 1px solid var(--border); border-radius: 8px; color: var(--fg-dim); font-size: 16px; cursor: pointer; flex-shrink: 0; }
  .rb-x:hover { color: var(--fg); background: var(--bg-hover); }

  .reaction-row { display: flex; gap: 4px; margin-top: 4px; flex-wrap: wrap; }
  .msg.out .reaction-row { justify-content: flex-end; }
  .reaction-chip {
    display: inline-flex; align-items: center; gap: 3px;
    padding: 2px 8px; border-radius: 100px;
    background: var(--bg-input); border: 1px solid var(--border);
    font-size: 13px; cursor: pointer; line-height: 1.3;
  }
  .reaction-chip:hover { border-color: var(--border-strong); }
  .reaction-chip.mine { background: var(--accent-soft); border-color: var(--accent); }
  .rc-count { font-size: 11px; font-weight: 700; color: var(--fg-muted); font-variant-numeric: tabular-nums; }
  .reaction-chip.mine .rc-count { color: var(--accent); }

  /* Read-receipt tick */
  .rr { display: inline-flex; align-items: center; }
  .rr :global(svg) { width: 14px; height: 14px; color: var(--fg-dim); }
  .rr.read :global(svg) { color: var(--accent); }

  /* Pinned-message bar */
  .pin-bar {
    display: flex; align-items: center; gap: 6px;
    padding: 8px 14px; border-bottom: 1px solid var(--border);
    background: var(--bg-list); flex-shrink: 0;
  }
  .pin-main {
    flex: 1; display: flex; align-items: center; gap: 10px;
    background: transparent; border: none; cursor: pointer;
    text-align: left; color: inherit; min-width: 0;
    border-left: 2px solid var(--accent); padding: 2px 0 2px 10px;
  }
  .pin-ic { width: 16px; height: 16px; color: var(--accent); flex-shrink: 0; }
  .pin-text { min-width: 0; }
  .pin-title { font-size: 11px; font-weight: 700; color: var(--accent); }
  .pin-preview { font-size: 12.5px; color: var(--fg-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .pin-x { width: 28px; height: 28px; background: transparent; border: 1px solid var(--border); border-radius: 8px; color: var(--fg-dim); font-size: 16px; cursor: pointer; flex-shrink: 0; }
  .pin-x:hover { color: var(--fg); background: var(--bg-hover); }

  /* Poll bubble */
  .bubble.poll-bubble { display: flex; flex-direction: column; gap: 4px; min-width: 240px; max-width: 360px; }
  .poll-q { font-size: 14.5px; font-weight: 700; color: var(--fg); }
  .poll-sub { font-size: 11px; color: var(--fg-dim); margin-bottom: 6px; }
  .poll-opts { display: flex; flex-direction: column; gap: 7px; }
  .poll-opt {
    background: transparent; border: none; cursor: pointer; text-align: left;
    padding: 2px 0; display: flex; flex-direction: column; gap: 4px; color: inherit;
  }
  .poll-opt-top { display: flex; align-items: center; gap: 8px; font-size: 13px; }
  .poll-mark {
    width: 16px; height: 16px; border-radius: 50%; flex-shrink: 0;
    border: 2px solid var(--border-strong);
  }
  .poll-mark.on { background: var(--accent); border-color: var(--accent); box-shadow: inset 0 0 0 3px var(--bg-bubble-in); }
  .poll-mark.sq { border-radius: 5px; }
  .poll-opt:disabled { cursor: default; }
  .poll-opt.results .poll-pct { min-width: 34px; font-weight: 800; color: var(--fg); }
  .poll-opt.lead .poll-fill { box-shadow: 0 0 10px var(--accent-glow); }
  .poll-opt.right .poll-fill { background: linear-gradient(90deg, #3ddc84, #2bbf6f); }
  .poll-opt.wrong .poll-fill { background: linear-gradient(90deg, #ff6b6b, #ff4d6d); }
  .poll-ic { font-weight: 900; font-size: 12px; color: var(--accent); }
  .poll-ic.ok { color: #3ddc84; }
  .poll-ic.bad { color: #ff6b6b; }
  .poll-who { display: flex; flex-wrap: wrap; gap: 4px; margin-top: 5px; }
  .pw { font-size: 10.5px; padding: 1px 6px; border-radius: 100px; background: var(--bg-input); color: var(--fg-muted); border: 1px solid var(--border); }
  .pw.more { color: var(--fg-dim); }
  .poll-expl { display: flex; gap: 7px; font-size: 12.5px; color: var(--fg-muted); background: var(--bg-input); border-radius: 9px; padding: 8px 10px; margin-top: 8px; line-height: 1.45; }
  .poll-submit {
    margin-top: 8px; border: none; border-radius: 9px; padding: 8px; font: inherit; font-weight: 800; font-size: 13px; cursor: pointer;
    background: linear-gradient(135deg, var(--accent), var(--accent-2)); color: #06070C;
  }
  .poll-submit:disabled { opacity: 0.45; cursor: default; }
  .poll-acts { display: flex; gap: 10px; margin-left: auto; margin-right: 10px; }
  .poll-bubble .poll-foot { flex-wrap: wrap; gap: 6px 12px; }
  .poll-acts button { background: none; border: none; padding: 0; font: inherit; font-size: 11.5px; font-weight: 700; color: var(--accent); cursor: pointer; }
  .poll-acts button:hover { text-decoration: underline; }
  .poll-label { flex: 1; min-width: 0; }
  .poll-opt.mine .poll-label { color: var(--accent); font-weight: 600; }
  .poll-pct { font-variant-numeric: tabular-nums; color: var(--fg-muted); font-size: 12px; }
  .poll-bar { height: 5px; border-radius: 3px; background: var(--border-strong); overflow: hidden; }
  .poll-fill { height: 100%; background: linear-gradient(90deg, var(--accent), var(--accent-2)); border-radius: 3px; transition: width 0.25s ease; }
  .poll-foot { display: flex; justify-content: space-between; align-items: center; margin-top: 8px; font-size: 11.5px; color: var(--fg-dim); }

  /* While drag-selecting, suppress text selection so the drag reads as
     a message range-select, not a text highlight. */
  .messages.dragging :global(.bubble),
  .messages.dragging :global(.msg-col) {
    user-select: none;
    -webkit-user-select: none;
  }
  @keyframes jump-in {
    from { opacity: 0; transform: translateY(6px); }
    to   { opacity: 1; transform: translateY(0); }
  }

  /* Banner shown above the composer when the active chat is a stub
     (e.g. the Welcome placeholder) and messages can't be sent. */
  .not-live-banner {
    margin: 0 20px 10px;
    padding: 12px 16px;
    border-radius: 12px;
    background: linear-gradient(135deg, color-mix(in srgb, var(--accent) 10%, transparent), color-mix(in srgb, var(--accent-2) 8%, transparent));
    border: 1px solid color-mix(in srgb, var(--accent) 30%, transparent);
    color: var(--fg);
  }
  .nlb-title {
    font-size: 13.5px;
    font-weight: 700;
    color: var(--accent);
    margin-bottom: 4px;
  }
  .nlb-sub {
    font-size: 12.5px;
    color: var(--fg-muted);
    line-height: 1.5;
  }
  .nlb-sub :global(b) { color: var(--fg); }

  /* Composer */
  .composer.disabled .composer-input {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .composer {
    display: flex;
    align-items: flex-end;
    gap: 8px;
    padding: 8px 16px 10px;
    border-top: 1px solid var(--border);
    background: rgba(12,15,23,0.7);
    backdrop-filter: blur(12px);
    -webkit-backdrop-filter: blur(12px);
    flex-shrink: 0;
  }
  .attach-btn {
    width: 36px; height: 36px;
    display: grid; place-items: center;
    background: var(--bg-input);
    border: 1px solid var(--border);
    border-radius: 10px;
    color: var(--fg-muted);
    cursor: pointer;
    transition: all 0.12s ease;
    flex-shrink: 0;
  }
  .attach-btn:hover { color: var(--accent); border-color: var(--accent-soft); }
  .attach-btn :global(svg) { width: 18px; height: 18px; }
  /* When there's no live chat, dim the attach + sticker buttons so
     the user understands they can't be used yet. Clicking still fires
     a toast telling them exactly which button to click to fix it. */
  .attach-btn.disabled {
    opacity: 0.4;
    cursor: help;
  }
  .attach-btn.disabled:hover {
    color: var(--fg-dim);
    border-color: var(--border);
  }
  .input-wrap {
    position: relative;
    flex: 1;
    display: flex;
    align-items: flex-end;
    background: var(--bg-input);
    border: 1px solid var(--border);
    border-radius: 12px;
    padding: 2px 4px 2px 12px;
    transition: border-color 0.15s ease, box-shadow 0.15s ease;
    min-width: 0;
  }
  .input-wrap:focus-within {
    border-color: var(--accent-soft);
    box-shadow: 0 0 0 3px var(--accent-soft);
  }
  .composer-input {
    flex: 1;
    background: transparent;
    border: none;
    outline: none;
    color: var(--fg);
    font-family: inherit;
    font-size: 14.5px;
    resize: none;
    /* border-box: autoresize sets height = scrollHeight, which already
       includes padding. With content-box the padding was added twice and
       the field jumped ~12px taller on the first keystroke. */
    box-sizing: border-box;
    padding: 6px 4px;
    max-height: 160px;
    min-height: 32px;
    line-height: 1.35;
    min-width: 0;
  }
  .composer-input::placeholder { color: var(--fg-dim); }

  .send-btn {
    width: 36px; height: 36px;
    display: grid; place-items: center;
    background: var(--bg-input);
    border: 1px solid var(--border);
    border-radius: 10px;
    color: var(--fg-dim);
    cursor: pointer;
    transition: all 0.15s ease;
    flex-shrink: 0;
  }
  .send-btn.ready {
    background: linear-gradient(135deg, var(--accent), var(--accent-2));
    color: #06070C;
    border-color: transparent;
    box-shadow: 0 0 20px -4px var(--accent-glow);
  }
  .send-btn.ready:hover {
    transform: translateY(-1px);
    box-shadow: 0 0 28px -2px var(--accent-glow);
  }
  .send-btn :global(svg) { width: 17px; height: 17px; }

  .composer-note {
    padding: 0 20px 12px;
    font-size: 11px;
    color: var(--fg-dim);
    display: flex;
    align-items: center;
    gap: 6px;
    background: rgba(12,15,23,0.7);
    margin-top: -8px;
    backdrop-filter: blur(12px);
  }
  .composer-note :global(svg) { width: 11px; height: 11px; color: var(--accent); }
  .composer-note kbd {
    font-family: var(--font-mono);
    font-size: 10px;
    background: rgba(255,255,255,0.04);
    padding: 1px 5px;
    border-radius: 3px;
    border: 1px solid var(--border);
    color: var(--fg-muted);
  }
  .kbds { margin-left: auto; }
</style>
