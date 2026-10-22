<script lang="ts">
  import { tick, onMount, untrack } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import { askConfirm, askConfirmCheck } from "./Confirm.svelte";
  import { askSend, VIDEO_RE, type SendMode } from "./SendDialog.svelte";
  import { shrinkImage } from "../imageTools";
  import { applyThemeFromPath } from "../themeShare";
  import { openFileDialog } from "../dialog";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { icons } from "../icons";
  import { activeChat, activeMessages, drafts, activeId, sendCurrent, loadActiveMessages, info, chats } from "../stores";
  import { meshStatus, meshSyncNow } from "../mesh";
  import Welcome from "./Welcome.svelte";
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
  import { editAndSend } from "../panels/photoEditor";
  import MediaPlayer from "./MediaPlayer.svelte";
  import { openGallery, type GalleryItem } from "./Lightbox.svelte";
  import { openArticleComposer } from "../panels/articleComposer";
  import { openCircleRecorder } from "./CircleRecorder.svelte";
  import { openPollComposer } from "./PollComposer.svelte";
  import { openForward, type ForwardItem } from "./ForwardPicker.svelte";
  import { playQueue, type Track } from "../player";
  import { toggleSharedMedia } from "./SharedMediaPanel.svelte";
  import { backToList } from "../stores";
  import { events } from "../tauri";
  import { pollStates, castVote, retractVote, applyRemoteVote, closePoll, viewPoll, ME as POLL_ME, type PollView } from "../polls";
  import { peerPresence, setPeerPresence, broadcastPresenceTo, activityVerb, chatOnline, peerStatus, presenceNow } from "../presence";
  import { notePeerProfileVersion, avatars, clans, peerCards } from "../people";
  import { profile as myProfile } from "../profile";
  import { isPublic } from "../privacy";
  import { fwdHeader, type ForwardAuthor } from "./ForwardPicker.svelte";
  import ClanTag from "./ClanTag.svelte";
  import { keepOnScreen } from "../actions";
  import { openPackSheet } from "./PackSheet.svelte";
  import { ghost, noReceiptChats, setReceipts } from "../ghost";
  import { TTL_PRESETS, ttlLabel, ttlShort, ttlNote } from "../ttl";
  import { parseBotText, type BotKb, type KbButton } from "../botkb";
  import { openMiniApp } from "./MiniApp.svelte";
  import { parseSpecial, newId as plannerId } from "../planner";
  import { parseDoc, parseBoard, newWsId } from "../workspace";
  import DocCard, { docEditor } from "./DocCard.svelte";
  import BoardCard from "./BoardCard.svelte";
  import CapsuleCard from "./CapsuleCard.svelte";
  import { capsuleComposer } from "./CapsuleComposer.svelte";
  import { parseCapsule } from "../capsules";
  import { btSheet } from "../bluetooth";
  import { chatLocks, unlocked, unlockChat, noteLeft } from "../chatLocks";
  import ThreadPanel, { type ThreadItem } from "./ThreadPanel.svelte";
  import { noteView, viewCounts, channelStats } from "../channelStats";
  import ChannelStats, { statsFor, type StatPost } from "./ChannelStats.svelte";
  import EventCard from "./EventCard.svelte";
  import TasksCard from "./TasksCard.svelte";
  import { openPlanner } from "./PlannerComposer.svelte";
  import { calendarFor } from "./ChatCalendar.svelte";
  import GroupCallPanel from "./GroupCallPanel.svelte";
  import { groupCall, activeGroupCalls, joinGroupCall } from "../groupCall";
  import { postBlock, tagOf, canSendKind, isAdmin, can, roomsOf, setOpenRoom } from "../groupRules";
  import { roleColor, hasPerm, rolesOn } from "../roles";
  import type { Room } from "../types";
  import { t, tr, tn } from "../i18n";
  import { mutedChats, setMuted } from "../chatPrefs";
  import { insertToComposer } from "../composerBus";
  import { startRecorder, AUDIO_TYPES } from "../recorder";
  import { groupMap, refreshGroups } from "../groups";
  import Markup from "./Markup.svelte";
  import LinkPreview from "./LinkPreview.svelte";
  import Carousel, { type CarouselItem } from "./Carousel.svelte";
  import ArticleMusic from "./ArticleMusic.svelte";
  import { hasMarkup, linksIn, excerptOf, parseBlocks, wordCount } from "../markup";
  import { objectUrl, mediaUrl, streamSrc, gifStill } from "../media";
  import { openArticleReader } from "../reader";
  import { shortcutsOpen } from "../shortcuts";
  import { openGroupPanel } from "../panels/groupPanel";
  import { reactions, toggleMyReaction, applyPeerReaction, reactionChips, hashContent, pins, pinMessage, unpinMessage, pinnedId } from "../messageMeta";
  import { openNewChatDialog } from "../newChat";

  import { contacts, getContact } from "../contacts";
  import type { ChatMessage, Group } from "../types";

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

  function startVoiceCall(video = false) {
    const chat = $activeChat;
    if (!chat || !chat.live) {
      showToast(tr("No live session — connect and start a chat first"));
      return;
    }
    if ($callState.kind !== "idle") {
      showToast(tr("Already in a call"));
      return;
    }
    startOutgoingCall(chat.id, video);
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
      // Prefer opus in webm — tiny files and good quality; whatever starts.
      let st;
      try { st = startRecorder(stream, AUDIO_TYPES, (b) => recordedChunks.push(b), 1000); }
      catch (e) { stream.getTracks().forEach((t) => t.stop()); throw e; }
      recorder = st.recorder;
      const mimeType = st.mime;
      recorder.onstop = async () => {
        stream.getTracks().forEach((t) => t.stop());
        recording = false;
        recordPaused = false;
        if (recordTimer) { clearInterval(recordTimer); recordTimer = null; }
        if (recordedChunks.length === 0) return;
        await sendVoiceBlob(new Blob(recordedChunks, { type: mimeType || "audio/webm" }));
        recordedChunks = [];
      };
      recordStartMs = Date.now();
      recordElapsed = 0;
      recordTimer = setInterval(() => {
        if (!recordPaused) recordElapsed = Math.floor((Date.now() - recordStartMs) / 1000);
      }, 250);
      recording = true;
    } catch (e) {
      showToast(`${tr("Voice recording failed")}: ${e instanceof Error ? e.message : e}`, 8000);
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
    const ext = blob.type.includes("ogg") ? "ogg" : blob.type.includes("mp4") ? "m4a" : blob.type.includes("aac") ? "aac" : "webm";
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
  // Switching chats: the new conversation glides in instead of snapping.
  let lastShownChat = "";
  $effect(() => {
    const id = $activeId ?? "";
    const el = messagesEl;
    if (!el || !id || id === lastShownChat) return;
    const first = !lastShownChat;
    lastShownChat = id;
    if (first || matchMedia("(prefers-reduced-motion: reduce)").matches || document.documentElement.classList.contains("soft-render")) return;
    el.animate([{ opacity: 0, transform: "translateY(10px)" }, { opacity: 1, transform: "none" }], { duration: 240, easing: "cubic-bezier(.2,.8,.2,1)" });
  });
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
    if (messagesEl.scrollTop < 600) void showOlder();
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
    void ensureShown(row.id).then(() =>
      document.querySelector(`[data-msg-id="${CSS.escape(row.id)}"]`)?.scrollIntoView({ block: "nearest" }));
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
    ...(isGroup && activeGroup ? [
      { cmd: "rules", desc: $t("Post this chat's rules"), run: () => activeGroup?.rules ? sendNow(`📜 ${tr("Rules")}\n${activeGroup.rules}`) : (text = "", showToast(tr("No rules set — admins add them in Edit"))) },
      { cmd: "top", desc: $t("Top chatters this week"), run: () => sendNow(topChatters()) },
      { cmd: "pick", desc: $t("Pick a random member"), run: () => { const u = pickOne(activeGroup!.members.map((m) => m.username)); if (u) sendNow(`🎯 ${tr("The chosen one")}: @${u}`); } },
      { cmd: "ship", desc: $t("Ship two random members"), run: () => {
        const ms = activeGroup!.members.map((m) => m.username);
        if (ms.length < 2) { text = ""; showToast(tr("Need at least two members")); return; }
        const a = pickOne(ms)!; const b = pickOne(ms.filter((x) => x !== a))!;
        sendNow(`💘 @${a} + @${b} = ${Math.floor(Math.random() * 101)}%`);
      } },
    ] : []),
    { cmd: "8ball", desc: $t("Ask the magic 8-ball"), run: () => sendNow(`🎱 ${tr(pickOne(EIGHT_BALL)!)}`) },
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

  // ---- Channel views (subscribers report to admins; see channelStats.ts) ----
  let isChannel = $derived(activeGroup?.kind === "channel");
  let amChanAdmin = $derived(!!activeGroup && isChannel && isAdmin(activeGroup, myUsername));
  $effect(() => {
    const el = messagesEl, gid = $activeChat?.id;
    void shownRows.length;
    if (!el || !gid || !isChannel || amChanAdmin || lockedNow) return;
    const io = new IntersectionObserver((entries) => {
      for (const e of entries) {
        const k = (e.target as HTMLElement).dataset.vkey;
        if (e.isIntersecting && k) { noteView(gid, k); io.unobserve(e.target); }
      }
    }, { root: el, threshold: 0.6 });
    void tick().then(() => el.querySelectorAll("[data-vkey]").forEach((n) => io.observe(n)));
    return () => io.disconnect();
  });

  let statPosts = $derived.by<StatPost[]>(() => {
    if (!amChanAdmin || $statsFor !== $activeChat?.id) return [];
    return rows.filter((r) => r.kind.type !== "system" && r.kind.type !== "date_separator").map((r) => {
      const key = reactionKey(r);
      const rc = $reactions[key];
      return { key, id: r.id, text: snippetOf(r).replace(/\s+/g, " ").slice(0, 120), time: r.time, reactions: rc ? rc.mine.length + Object.values(rc.peer).reduce((a, b) => a + b, 0) : 0, comments: commentCount.get(key) ?? 0 };
    });
  });

  // ---- Threads: comments under posts, replies gathered under a message ----
  interface Cmt { id: string; from: string; time: string; to: string; t: string; re: { a: string; t: string } | null; outgoing: boolean }
  let comments = $derived.by<Cmt[]>(() => {
    const out: Cmt[] = [];
    for (const m of roomMessages) {
      if (!isComment(m) || m.kind.type !== "text") continue;
      try {
        const v = JSON.parse(m.kind.content.text.slice(8));
        if (typeof v?.to !== "string" || typeof v?.t !== "string") continue;
        const re = v.re && typeof v.re.a === "string" && typeof v.re.t === "string" ? { a: v.re.a.slice(0, 60), t: v.re.t.slice(0, 160) } : null;
        const mine = m.from === "me" || m.from === myUsername;
        out.push({ id: m.id, from: mine ? myUsername : m.from, time: m.time, to: v.to, t: v.t.slice(0, 4000), re, outgoing: mine });
      } catch { /* not a comment */ }
    }
    return out;
  });
  let commentCount = $derived.by(() => { const c = new Map<string, number>(); for (const x of comments) c.set(x.to, (c.get(x.to) ?? 0) + 1); return c; });
  let replyCount = $derived.by(() => {
    const c = new Map<string, number>();
    for (const r of rows) if (r.kind.type === "reply") c.set(r.kind.content.quoted_text, (c.get(r.kind.content.quoted_text) ?? 0) + 1);
    return c;
  });
  const quoteOf = (m: ChatMessage) => snippetOf(m).slice(0, 180);
  let thread = $state<{ kind: "comments" | "replies"; root: Row } | null>(null);
  $effect(() => { void $activeChat?.id; thread = null; });
  let threadItems = $derived.by<ThreadItem[]>(() => {
    const th = thread;
    if (!th) return [];
    if (th.kind === "comments") {
      const key = reactionKey(th.root);
      return comments.filter((c) => c.to === key).map((c) => ({ id: c.id, author: c.from, time: c.time, text: c.t, outgoing: c.outgoing, re: c.re, depth: c.re ? 1 : 0 }));
    }
    // Replies, and replies to those (Telegram's "replies" view).
    const out: ThreadItem[] = [];
    let level = [quoteOf(th.root)];
    const seen = new Set<string>();
    for (let depth = 0; depth < 4 && level.length; depth++) {
      const next: string[] = [];
      for (const r of rows) {
        if (r.kind.type !== "reply" || seen.has(r.id) || !level.includes(r.kind.content.quoted_text)) continue;
        seen.add(r.id);
        out.push({ id: r.id, author: r.outgoing ? myUsername : (isGroup ? r.from : ($activeChat?.name ?? r.from)), time: r.time, text: r.kind.content.text, outgoing: r.outgoing, re: depth ? { a: r.kind.content.quoted_author, t: r.kind.content.quoted_text } : null, depth });
        next.push(quoteOf(r));
      }
      level = next;
    }
    return out;
  });
  let commentsAllowed = $derived(!!activeGroup && activeGroup.settings.comments !== false);
  function openThread(row: Row, kind?: "comments" | "replies") {
    closeMenu();
    const k = kind ?? (activeGroup?.kind === "channel" ? "comments" : "replies");
    thread = { kind: k, root: row };
  }
  async function sendThread(text: string, re: { a: string; t: string } | null) {
    const th = thread, chat = $activeChat;
    if (!th || !chat) return;
    const body = th.kind === "comments"
      ? "[cmt:v1]" + JSON.stringify({ to: reactionKey(th.root), t: text, ...(re ? { re } : {}) })
      : "[reply:v1]" + JSON.stringify({ quoted_author: re?.a ?? authorOf(th.root), quoted_text: re ? re.t.slice(0, 180) : quoteOf(th.root), text });
    try {
      if (chat.id.startsWith("grp_")) await api.groupSend(chat.id, body);
      else await api.sendMessage(chat.id, body);
      await loadActiveMessages();
    } catch (e) { showToast(String(e)); }
  }

  // ---- Chat password (only for me) ----
  let lockedNow = $derived(!!$activeChat && !!$chatLocks[$activeChat.id] && !$unlocked[$activeChat.id]);
  let lockPass = $state("");
  let lockErr = $state(false);
  let lockBusy = $state(false);
  let prevChat = "";
  $effect(() => {
    const id = $activeChat?.id ?? "";
    untrack(() => {
      if (prevChat && prevChat !== id) noteLeft(prevChat);
      prevChat = id;
      lockPass = ""; lockErr = false;
      if (id && $unlocked[id]) unlocked.update((u) => ({ ...u, [id]: Date.now() }));
    });
  });
  async function tryUnlock() {
    if (!$activeChat || lockBusy) return;
    lockBusy = true;
    const ok = await unlockChat($activeChat.id, lockPass);
    lockBusy = false;
    lockErr = !ok;
    if (ok) lockPass = "";
  }

  /** This chat is in its own window (calls stay in the main one). */
  const POPOUT = typeof document !== "undefined" && document.documentElement.classList.contains("popout");

  // ---- Bots: keyboards, mini apps, menu button ----
  let isBotChat = $derived(!!$activeChat && $activeChat.live && !$activeChat.has_lock && !$activeChat.id.startsWith("grp_"));
  let kbBusy = $state<string | null>(null);
  /** The reply keyboard the bot left open (newest bot message that set or removed one). */
  let replyKb = $derived.by<BotKb | null>(() => {
    if (!isBotChat) return null;
    const msgs = $activeMessages;
    let mineAfter = false;
    for (let i = msgs.length - 1; i >= 0; i--) {
      const m = msgs[i];
      if (m.from === "me") { mineAfter = true; continue; }
      if (m.kind.type !== "text" || !m.kind.content.text.startsWith("[kb:v1]")) continue;
      const { kb } = parseBotText(m.kind.content.text);
      if (!kb || kb.kind === "inline") continue;
      if (kb.kind === "remove") return null;
      if (kb.once && mineAfter) return null;
      return kb;
    }
    return null;
  });
  let replyKbHidden = $state(false);
  $effect(() => { void replyKb; replyKbHidden = false; });
  let botMenu = $state<{ text: string; url: string } | null>(null);
  $effect(() => {
    const id = $activeChat?.id;
    botMenu = null;
    if (!id || !isBotChat) return;
    api.webappMenu(id).then((m) => {
      if ($activeChat?.id === id && m?.type === "web_app" && m.web_app?.url) botMenu = { text: m.text || tr("Open"), url: m.web_app.url };
    }).catch(() => {});
  });
  async function pressKb(b: KbButton, kb: BotKb) {
    const chat = $activeChat;
    if (!chat) return;
    if (b.url) { void api.openExternal(b.url).catch(() => {}); return; }
    if (b.wa) { openMiniApp({ bot: chat.id, url: b.wa, buttonText: b.t, from: kb.kind === "reply" ? "reply" : "inline" }); return; }
    if (b.contact || b.location) { showToast(tr("Sharing a contact or location with bots isn't supported yet")); return; }
    if (b.q !== undefined) { composerEl?.insert(`@${chat.id} ${b.q}`); return; }
    if (b.cb !== undefined && kb.kind === "inline") {
      const key = `${kb.m}:${b.cb}`;
      kbBusy = key;
      try { await api.botSendRaw(chat.id, "[cb:v1]" + JSON.stringify({ m: kb.m, d: b.cb })); }
      catch (e) { showToast(String(e)); }
      finally { setTimeout(() => { if (kbBusy === key) kbBusy = null; }, 900); }
      return;
    }
    // Reply keyboard: the label is an ordinary message, like in Telegram.
    try {
      await api.sendMessage(chat.id, b.t);
      await loadActiveMessages();
      chats.set(await api.listChats());
    } catch (e) { showToast(String(e)); }
  }

  // ---- Auto-delete timer + read receipts of this chat (header ⏱ menu) ----
  let chatTtl = $state(0);
  let ttlMenu = $state(false);
  // ---- Header: one call button (audio / video), one tools menu ----
  let callMenu = $state(false);
  let toolsMenu = $state(false);
  const DESKTOP_APP = typeof navigator !== "undefined" && !/android|iphone|ipad/i.test(navigator.userAgent);
  const IC = {
    phone: icons.phone,
    video: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="m22 8-6 4 6 4V8Z"/><rect x="2" y="6" width="14" height="12" rx="2"/></svg>',
    tools: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="7.5" height="7.5" rx="2"/><rect x="13.5" y="3" width="7.5" height="7.5" rx="2"/><rect x="3" y="13.5" width="7.5" height="7.5" rx="2"/><circle cx="17.25" cy="17.25" r="3.75"/></svg>',
    cal: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="5" width="18" height="16" rx="3"/><line x1="3" y1="10" x2="21" y2="10"/><line x1="8" y1="3" x2="8" y2="7"/><line x1="16" y1="3" x2="16" y2="7"/></svg>',
    ttl: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="13" r="8"/><path d="M12 9v4l2.5 2"/><path d="M9.5 2.5h5"/><path d="M12 2.5V5"/></svg>',
    stats: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><line x1="6" y1="20" x2="6" y2="12"/><line x1="12" y1="20" x2="12" y2="4"/><line x1="18" y1="20" x2="18" y2="9"/></svg>',
    win: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="5" width="14" height="14" rx="2"/><path d="M14 3h7v7"/><path d="M21 3l-8 8"/></svg>',
  };
  type HdrItem = { id: string; label: string; sub?: string; icon: string; on?: boolean; run: () => void };
  let callChoices = $derived.by((): HdrItem[] => {
    const c = $activeChat;
    if (!c || POPOUT) return [];
    if (isGroup && activeGroup) {
      const gid = activeGroup.id;
      const chan = activeGroup.kind === "channel";
      const speaker = !chan || isAdmin(activeGroup, myUsername);
      const out: HdrItem[] = [];
      if (speaker || $activeGroupCalls[gid]) out.push({ id: "voice", label: chan ? (speaker ? tr("Start a live stream") : tr("Listen to the live stream")) : tr("Voice chat"), icon: IC.phone, on: $groupCall?.gid === gid, run: () => void joinGroupCall(gid) });
      if (speaker) out.push({ id: "video", label: tr("Video chat"), icon: IC.video, run: () => void joinGroupCall(gid, { video: true }) });
      return out;
    }
    if (c.id === "__saved__" || isBotChat) return [];
    return [
      { id: "audio", label: tr("Audio call"), icon: IC.phone, run: () => startVoiceCall(false) },
      { id: "video", label: tr("Video call"), icon: IC.video, run: () => startVoiceCall(true) },
    ];
  });
  let toolItems = $derived.by((): HdrItem[] => {
    const c = $activeChat;
    if (!c) return [];
    const out: HdrItem[] = [];
    if (c.id !== "__saved__" && !isBotChat) out.push({ id: "cal", label: tr("Calendar"), sub: tr("Events and reminders of this chat"), icon: IC.cal, on: $calendarFor === c.id, run: () => calendarFor.set($calendarFor === c.id ? null : c.id) });
    if (ttlEditable || chatTtl) out.push({ id: "ttl", label: tr("Auto-delete & read receipts"), sub: chatTtl ? tr("Auto-delete: {t}", { t: ttlLabel(chatTtl) }) : tr("Off"), icon: IC.ttl, on: chatTtl > 0, run: () => (ttlMenu = true) });
    if (amChanAdmin) out.push({ id: "stats", label: tr("Channel statistics"), sub: tr("Views, reactions, comments"), icon: IC.stats, on: $statsFor === c.id, run: () => statsFor.set($statsFor === c.id ? null : c.id) });
    if (DESKTOP_APP && !POPOUT) out.push({ id: "win", label: tr("Open in a new window"), sub: "Ctrl + " + tr("click"), icon: IC.win, run: () => void api.openChatWindow(c.id, c.name).catch((e) => showToast(String(e))) });
    return out;
  });
  $effect(() => { void $activeChat?.id; callMenu = false; toolsMenu = false; });
  $effect(() => {
    const id = $activeChat?.id;
    void $chats;
    ttlMenu = false;
    if (!id) { chatTtl = 0; return; }
    const g = $groupMap[id];
    if (g) { chatTtl = g.settings.ttl ?? 0; return; }
    api.getChatTtl(id).then((v) => { if ($activeChat?.id === id) chatTtl = v; }).catch(() => { chatTtl = 0; });
  });
  /** May I change this chat's timer? (1:1 with a live session; group admins) */
  let ttlEditable = $derived(
    !!$activeChat && $activeChat.id !== "__saved__" &&
    (activeGroup ? isAdmin(activeGroup, myUsername) : $activeChat.live && $activeChat.has_lock),
  );
  async function setTtl(secs: number) {
    const chat = $activeChat;
    ttlMenu = false;
    if (!chat || secs === chatTtl) return;
    try {
      if (activeGroup) {
        const g = JSON.parse(JSON.stringify(activeGroup));
        g.settings.ttl = secs;
        g.log = [...(g.log ?? []), { at: new Date().toLocaleString(), by: myUsername, text: secs ? `set auto-delete to ${ttlLabel(secs)}` : "turned off auto-delete" }].slice(-200);
        await api.updateGroup(g);
        await refreshGroups();
      } else {
        await api.setChatTtl(chat.id, secs);
        await loadActiveMessages();
      }
      chatTtl = secs;
    } catch (e) { showToast(String(e)); }
  }

  // ---- server rules / welcome note (once per change) ---------------------
  const rulesKey = (g: Group) => `aqua.rulesSeen.${g.id}`;
  const rulesHash = (g: Group) => { let h = 0; for (const c of `${g.rules ?? ""}|${g.welcome ?? ""}`) h = (h * 31 + c.charCodeAt(0)) | 0; return String(h); };
  let rulesTick = $state(0);
  let rulesNote = $derived.by(() => {
    void rulesTick;
    const g = activeGroup;
    if (!g || g.kind === "channel" || (!g.rules && !g.welcome)) return null;
    try { if (localStorage.getItem(rulesKey(g)) === rulesHash(g)) return null; } catch { /* storage off: show */ }
    return { rules: g.rules ?? "", welcome: g.owner === myUsername ? "" : g.welcome ?? "" };
  });
  function dismissRules() {
    if (activeGroup) try { localStorage.setItem(rulesKey(activeGroup), rulesHash(activeGroup)); } catch { /* ignore */ }
    rulesTick++;
  }
  // ---- server fun -------------------------------------------------------
  function pickOne<T>(a: T[]): T | undefined { return a[Math.floor(Math.random() * a.length)]; }
  function topChatters(): string {
    const week = Date.now() - 7 * 864e5;
    const n: Record<string, number> = {};
    for (const m of $activeMessages) { const at = Date.parse(m.time); if (at >= week && m.from) n[m.from] = (n[m.from] ?? 0) + 1; }
    const top = Object.entries(n).sort((a, b) => b[1] - a[1]).slice(0, 5);
    if (!top.length) return tr("🏆 Nobody has said anything this week");
    return `🏆 ${tr("Top chatters this week")}\n` + top.map(([u, c], i) => `${["🥇", "🥈", "🥉", "4.", "5."][i]} @${u} — ${c}`).join("\n");
  }
  const EIGHT_BALL = ["It is certain", "Without a doubt", "Yes — definitely", "Most likely", "Signs point to yes",
    "Reply hazy, try again", "Ask again later", "Cannot predict now", "Don't count on it", "My sources say no", "Very doubtful"];
  // Can the current user post here? (Same rules the backend enforces on
  // receive — groupRules.ts / state.rs may_post.)
  // ---- supergroup rooms (Discord-style channels) ---------------------------
  const ROOM_KEY = "aqua.rooms.active.v1", SEEN_KEY = "aqua.rooms.seen.v1";
  const readJson = <T,>(k: string, d: T): T => { try { return JSON.parse(localStorage.getItem(k) ?? "") as T; } catch { return d; } };
  let roomPick = $state<Record<string, string>>(readJson(ROOM_KEY, {}));
  let roomSeen = $state<Record<string, Record<string, number>>>(readJson(SEEN_KEY, {}));
  let rooms = $derived(roomsOf(activeGroup));
  let textRooms = $derived(rooms.filter((r) => r.kind === "text" && (!rolesOn(activeGroup) || hasPerm(activeGroup, myUsername, "view_room", r))));
  let activeRoom = $derived.by((): Room | null => {
    if (!activeGroup || !textRooms.length) return null;
    return textRooms.find((r) => r.id === roomPick[activeGroup.id]) ?? textRooms[0];
  });
  const roomOfMsg = (m: ChatMessage) => m.meta?.room && textRooms.some((r) => r.id === m.meta!.room) ? m.meta.room : textRooms[0]?.id;
  let roomCounts = $derived.by(() => {
    const c: Record<string, number> = {};
    if (textRooms.length) for (const m of $activeMessages) { const r = roomOfMsg(m); if (r) c[r] = (c[r] ?? 0) + 1; }
    return c;
  });
  let roomMessages = $derived(activeRoom ? $activeMessages.filter((m) => roomOfMsg(m) === activeRoom!.id) : $activeMessages);
  // Tell the backend where sends go, and mark the open room as read.
  $effect(() => {
    const g = activeGroup, r = activeRoom;
    if (!g) return;
    setOpenRoom(g.id, r);
    void api.setGroupRoom(g.id, r?.id ?? "").catch(() => {});
    if (r) {
      const n = roomCounts[r.id] ?? 0;
      if ((roomSeen[g.id]?.[r.id] ?? -1) !== n) {
        roomSeen = { ...roomSeen, [g.id]: { ...(roomSeen[g.id] ?? {}), [r.id]: n } };
        try { localStorage.setItem(SEEN_KEY, JSON.stringify(roomSeen)); } catch { /* ignore */ }
      }
    }
  });
  // ---- voice room full screen (Discord) ----
  let voiceView = $state<Record<string, string>>({});   // gid → voice room id
  let voiceChat = $state(false);
  let headerH = $state(64);
  let headerEl = $state<HTMLElement | null>(null);
  // Measured only for the voice view (a size binding here kept a
  // ResizeObserver busy on every repaint of the chat).
  $effect(() => { if (fullVoice && headerEl) headerH = headerEl.offsetHeight; });
  let inMyCall = $derived(!!activeGroup && $groupCall?.gid === activeGroup.id);
  let fullVoice = $derived(!!activeGroup && !!voiceView[activeGroup.id] && inMyCall);
  let voiceRoomName = $derived(activeGroup ? rooms.find((r) => r.id === voiceView[activeGroup.id])?.name ?? "" : "");
  // The call ended → back to the chat.
  let hadCall = false;
  $effect(() => {
    const on = !!$groupCall;
    if (hadCall && !on) voiceView = {};
    hadCall = on;
  });
  function openVoiceFull() { if (activeGroup) voiceView = { ...voiceView, [activeGroup.id]: rooms.find((r) => r.kind === "voice")?.id ?? "voice" }; }

  function pickRoom(r: Room) {
    if (!activeGroup) return;
    if (r.kind === "voice") {
      if (rolesOn(activeGroup) && !hasPerm(activeGroup, myUsername, "connect", r)) { showToast(tr("You can't join this voice room")); return; }
      voiceView = { ...voiceView, [activeGroup.id]: r.id };
      voiceChat = false;
      if (!inMyCall) void joinGroupCall(activeGroup.id);
      return;
    }
    if (voiceView[activeGroup.id]) { const { [activeGroup.id]: _, ...rest } = voiceView; voiceView = rest; }
    roomPick = { ...roomPick, [activeGroup.id]: r.id };
    try { localStorage.setItem(ROOM_KEY, JSON.stringify(roomPick)); } catch { /* ignore */ }
  }
  const MUTE_KEY = "aqua.rooms.muted.v1";
  let roomMuted = $state<Record<string, boolean>>(readJson(MUTE_KEY, {}));
  const isRoomMuted = (r: Room) => !!activeGroup && !!roomMuted[`${activeGroup.id}/${r.id}`];
  const roomUnread = (r: Room) => activeGroup && !isRoomMuted(r) ? Math.max(0, (roomCounts[r.id] ?? 0) - (roomSeen[activeGroup.id]?.[r.id] ?? roomCounts[r.id] ?? 0)) : 0;
  let roomsOpen = $state(false);
  // ---- room context menu (right click) ----
  let roomMenu = $state<{ r: Room; x: number; y: number } | null>(null);
  const canEditRooms = $derived(!!activeGroup && (can(activeGroup, myUsername, "change_info") || hasPerm(activeGroup, myUsername, "manage_rooms")));
  function openRoomMenu(e: MouseEvent, r: Room) {
    e.preventDefault();
    const W = 220, H = 230;
    roomMenu = { r, x: Math.min(e.clientX, window.innerWidth - W - 8), y: Math.min(e.clientY, window.innerHeight - H - 8) };
  }
  function markRoomRead(r: Room) {
    if (!activeGroup) return;
    roomSeen = { ...roomSeen, [activeGroup.id]: { ...(roomSeen[activeGroup.id] ?? {}), [r.id]: roomCounts[r.id] ?? 0 } };
    try { localStorage.setItem(SEEN_KEY, JSON.stringify(roomSeen)); } catch { /* ignore */ }
  }
  function toggleRoomMute(r: Room) {
    if (!activeGroup) return;
    const k = `${activeGroup.id}/${r.id}`;
    const { [k]: was, ...rest } = roomMuted;
    roomMuted = was ? rest : { ...rest, [k]: true };
    try { localStorage.setItem(MUTE_KEY, JSON.stringify(roomMuted)); } catch { /* ignore */ }
    showToast(was ? tr("Room unmuted") : tr("Room muted"));
  }
  async function deleteRoomQuick(r: Room) {
    const g = activeGroup;
    if (!g) return;
    if ((g.rooms ?? []).filter((x) => x.kind === "text").length < 2 && r.kind === "text") { showToast(tr("Keep at least one text room")); return; }
    if (!(await askConfirm({ title: tr("Delete room?"), text: tr("“{room}” disappears for everyone. Its messages move to the first room.", { room: r.name }), ok: tr("Delete"), danger: true }))) return;
    const next = JSON.parse(JSON.stringify(g)) as typeof g;
    next.rooms = (next.rooms ?? []).filter((x) => x.id !== r.id);
    next.log = [...(next.log ?? []), { at: new Date().toLocaleString(), by: myUsername, text: `deleted room ${r.name}` }].slice(-200);
    try { await api.updateGroup(next); await refreshGroups(); } catch (e) { showToast(`Couldn't save: ${e}`, 5000); }
  }
  $effect(() => {
    if (!roomMenu) return;
    const close = (e: Event) => { if (!(e.target as HTMLElement)?.closest?.(".room-menu")) roomMenu = null; };
    const key = (e: KeyboardEvent) => { if (e.key === "Escape") { e.stopPropagation(); roomMenu = null; } };
    document.addEventListener("pointerdown", close, true);
    document.addEventListener("keydown", key, true);
    window.addEventListener("blur", close);
    return () => { document.removeEventListener("pointerdown", close, true); document.removeEventListener("keydown", key, true); window.removeEventListener("blur", close); };
  });

  let postBlocked = $derived(
    activeRoom?.readonly && !isAdmin(activeGroup, myUsername)
      ? tr("Only admins write in #{room}", { room: activeRoom.name })
      : postBlock(activeGroup, myUsername, activeRoom));
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
  // Comments ([cmt:v1]) live in threads under their message, not in the flow.
  const isComment = (m: ChatMessage) => m.kind.type === "text" && m.kind.content.text.startsWith("[cmt:v1]");
  let flowMessages = $derived(roomMessages.some(isComment) ? roomMessages.filter((m) => !isComment(m)) : roomMessages);
  let rows = $derived(computeRows(flowMessages, myUsername));

  // ---- Windowed rendering: only the newest WINDOW_STEP rows are in the
  // DOM; scrolling near the top reveals the next page (scroll position
  // kept). Long chats stay as light as short ones.
  // A chat opens with the newest WINDOW_FIRST rows (a few screens) — the
  // first layout is the expensive part of opening a chat in WebKitGTK.
  const WINDOW_FIRST = 40;
  const WINDOW_STEP = 80;
  let shownCount = $state(WINDOW_FIRST);
  let shownRows = $derived(rows.length > shownCount ? rows.slice(rows.length - shownCount) : rows);
  $effect(() => {
    void $activeChat?.id;
    untrack(() => { shownCount = WINDOW_FIRST; });
  });
  let growing = false;
  async function showOlder() {
    if (growing || !messagesEl || shownCount >= rows.length) return;
    growing = true;
    const h0 = messagesEl.scrollHeight, top0 = messagesEl.scrollTop;
    shownCount += WINDOW_STEP;
    await tick();
    if (messagesEl) messagesEl.scrollTop = top0 + (messagesEl.scrollHeight - h0);
    growing = false;
  }
  /** Make sure a row is rendered before scrolling to it. */
  async function ensureShown(id: string) {
    const idx = rows.findIndex((r) => r.id === id);
    if (idx < 0) return;
    const need = rows.length - idx + 20;
    if (need > shownCount) { shownCount = need; await tick(); }
  }

  // Row objects are reused while their message and grouping are unchanged,
  // so the keyed list only touches rows that actually changed.
  let rowCache = new WeakMap<ChatMessage, Row>();
  function rowOf(m: ChatMessage, first: boolean, last: boolean, outgoing: boolean): Row {
    const c = rowCache.get(m);
    if (c && c.first === first && c.last === last && c.outgoing === outgoing) return c;
    const r = { ...m, first, last, outgoing } as Row;
    rowCache.set(m, r);
    return r;
  }

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
    const ids = new Set<string>();
    for (let i = 0; i < msgs.length; i++) {
      const m = msgs[i];
      // A repeated id would break the keyed list for the whole chat.
      if (ids.has(m.id)) continue;
      ids.add(m.id);
      if (m.kind.type === "system" || m.kind.type === "date_separator") {
        out.push(rowOf(m, false, false, false));
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
      out.push(rowOf(m, first, last, mine(m.from)));
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
  let menu = $state<{ open: boolean; x: number; y: number; row: Row | null; touch?: boolean }>({ open: false, x: 0, y: 0, row: null });

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

  // ---- Touch, as in Telegram: a tap opens the menu with reactions at once;
  // a long press on the words selects text (the system's handles), a long
  // press beside them selects the whole message.
  let lastPointerType = "mouse";
  let tap: { x: number; y: number; t: number; menuOpen: boolean } | null = null;
  function notePointer(e: PointerEvent) {
    lastPointerType = e.pointerType || "mouse";
    tap = { x: e.clientX, y: e.clientY, t: performance.now(), menuOpen: menu.open };
  }
  /** Things inside a bubble that do something on their own when tapped. */
  const TAP_OWN = "a, button, input, textarea, select, video, audio, canvas, label, summary, [role=button], [contenteditable], .media-img-btn, .reaction-chip, .spoiler:not(.revealed)";
  /** Is (x, y) over actual characters (not the padding beside them)? */
  function pointOnText(x: number, y: number): boolean {
    const el = document.elementFromPoint(x, y);
    if (!el || !el.closest(".bubble") || el.closest(".bubble-meta")) return false;
    const walk = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
    const range = document.createRange();
    for (let n = walk.nextNode(); n; n = walk.nextNode()) {
      if (!n.textContent?.trim()) continue;
      range.selectNodeContents(n);
      for (const b of range.getClientRects()) {
        if (x >= b.left - 2 && x <= b.right + 2 && y >= b.top - 2 && y <= b.bottom + 2) return true;
      }
    }
    return false;
  }
  function openTouchMenu(row: Row, x: number, y: number) {
    menu = { open: true, x, y, row, touch: true };
  }

  function onMsgClick(row: Row, e?: MouseEvent) {
    // Swallow the click that trails a real drag-select so it doesn't
    // immediately toggle the start row back off.
    if (draggedMoved) { draggedMoved = false; return; }
    if (selecting) { toggleSelect(row.id); return; }
    const t0 = tap; tap = null;
    if (!e || lastPointerType === "mouse" || !t0) return;
    if (t0.menuOpen) return;                       // that tap only closed the menu
    if (performance.now() - t0.t > 450 || Math.hypot(e.clientX - t0.x, e.clientY - t0.y) > 12) return;
    if ((e.target as HTMLElement).closest(TAP_OWN)) return;
    const sel = getSelection();
    if (sel && !sel.isCollapsed && sel.toString().trim()) return;   // tapping off a text selection
    openTouchMenu(row, e.clientX, e.clientY);
  }
  function onMsgContext(e: MouseEvent, row: Row) {
    if (lastPointerType === "touch") {
      const canSelect = !protectedChat && !selecting && pointOnText(e.clientX, e.clientY);
      if (canSelect) { closeMenu(); return; }      // let the system select the words
      e.preventDefault();
      e.stopPropagation();
      closeMenu();
      if (selecting) toggleSelect(row.id); else startSelection(row);
      try { navigator.vibrate?.(12); } catch { /* no vibration */ }
      return;
    }
    e.preventDefault();
    e.stopPropagation();
    // Clamp so the menu stays on screen (approx 190x220).
    const x = e.clientX, y = e.clientY; // keepOnScreen pulls it back inside
    menu = { open: true, x, y, row };
  }
  function closeMenu() { menu = { open: false, x: 0, y: 0, row: null }; }
  /** Touch screens: a long press opens the menu, so text is selected from
   *  here (Telegram's "Select text"); the system handles then adjust it. */
  function selectRowText(row: Row) {
    closeMenu();
    const bubble = messagesEl?.querySelector<HTMLElement>(`[data-id="${CSS.escape(String(row.id))}"] .bubble`);
    if (!bubble) return;
    const meta = bubble.querySelector(":scope > .bubble-meta");
    requestAnimationFrame(() => {
      const r = document.createRange();
      r.selectNodeContents(bubble);
      if (meta) r.setEndBefore(meta);   // the text, not the time / ticks
      const sel = getSelection();
      sel?.removeAllRanges();
      sel?.addRange(r);
    });
  }

  /** Extract a forwardable payload from a message, or null if it can't
   *  be forwarded (e.g. a media file that was never downloaded). */
  /** Who wrote `m`, for the "Forwarded from" header. A re-forward keeps the
   *  original author; a link to the account only if they allow it. */
  function fwdAuthorOf(m: ChatMessage): ForwardAuthor | undefined {
    const f = m.meta?.fwd;
    if (f?.name) return { from: f.from, name: f.name, at: f.at, chat: f.chat };
    const from = m.from === "me" ? myUsername : m.from;
    if (!from || from === "system") return undefined;
    const mine = from === myUsername;
    const card = $peerCards[from];
    const name = mine
      ? ($myProfile.display_name && $myProfile.display_name !== "You" ? $myProfile.display_name : myUsername)
      : (card?.display_name || (!isGroup ? $activeChat?.name : "") || from);
    const link = mine ? isPublic("forwards") : (card?.extra?.fwd_link !== false || $peerPresence[from]?.fl === true);
    const chan = isGroup && activeGroup ? activeGroup.title : undefined;
    return { from: link ? from : undefined, name, at: m.time, chat: chan };
  }

  function forwardItemOf(m: ChatMessage): ForwardItem | null {
    const k = m.kind;
    const author = fwdAuthorOf(m);
    if (k.type === "text") return { kind: "text", text: k.content.text, author };
    if (k.type === "code") return { kind: "text", text: "```" + (k.content.language || "") + "\n" + k.content.code + "\n```", author };
    if (k.type === "article") return { kind: "text", text: `${k.content.title}\n\n${k.content.body}`, author };
    if (k.type === "reply") return { kind: "text", text: k.content.text, author };
    if (k.type === "file") {
      if (k.content.saved_at) return { kind: "file", path: k.content.saved_at, name: k.content.name, author };
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
  /** Search by reaction: only reactions that occur in this chat are offered
   *  (computed while the search bar is open, from the local reaction map). */
  let searchReact = $state("");
  let chatReactions = $derived.by<{ emoji: string; n: number }[]>(() => {
    if (!searchOpen) return [];
    const count = new Map<string, number>();
    for (const m of rows) {
      if (m.kind.type === "system" || m.kind.type === "date_separator") continue;
      const r = $reactions[reactionKey(m)];
      if (!r) continue;
      for (const e of new Set([...r.mine, ...Object.keys(r.peer)])) count.set(e, (count.get(e) ?? 0) + 1);
    }
    return [...count.entries()].sort((a, b) => b[1] - a[1]).slice(0, 12).map(([emoji, n]) => ({ emoji, n }));
  });
  let searchHits = $derived.by<string[]>(() => {
    const q = searchQuery.trim().toLowerCase();
    const re = searchReact;
    if (!q && !re) return [];
    return rows
      .filter((m) => m.kind.type !== "system" && m.kind.type !== "date_separator")
      .filter((m) => !q || (textOf(m) ?? "").toLowerCase().includes(q))
      .filter((m) => { if (!re) return true; const r = $reactions[reactionKey(m)]; return !!r && (r.mine.includes(re) || !!r.peer[re]); })
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
  function closeSearch() { searchOpen = false; searchQuery = ""; searchReact = ""; searchIdx = 0; }
  async function scrollToHit() {
    const id = searchHits[searchIdx];
    if (!id || !messagesEl) return;
    await ensureShown(id);
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
    return m.outgoing ? ($info?.username ?? "You") : isGroup ? m.from : ($activeChat?.name ?? "Peer");
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
    // "Delete for everyone" is possible for your own text messages (and,
    // in groups, for others' when you may moderate); the rest is local.
    const canAll = chat.id !== "__saved__" && list.some((r) => (r.kind.type === "text" || r.kind.type === "reply") && (r.outgoing || chat.id.startsWith("grp_")));
    let forAll = canAll;
    if (ask) {
      const title = list.length === 1 ? tr("Delete this message?") : tr("Delete {n} messages?", { n: list.length });
      if (canAll) {
        const r = await askConfirmCheck({
          title,
          text: tr("Media and articles are removed on this device only."),
          check: chat.id.startsWith("grp_") ? tr("Also delete for all members") : tr("Also delete for {name}", { name: chat.name }),
          checked: true,
          ok: tr("Delete"),
          danger: true,
        });
        if (!r.ok) return;
        forAll = r.checked;
      } else {
        const ok = await askConfirm({ title, ok: tr("Delete"), danger: true });
        if (!ok) return;
      }
    }
    if (!forAll) {
      try { await api.deleteMessagesLocal(chat.id, list.map((r) => r.id)); } catch { showToast(tr("Some messages could not be deleted")); }
      await loadActiveMessages();
      chats.set(await api.listChats());
      return;
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

  // ---- Workspace: documents and boards, also made from selected messages ----
  async function postToChat(body: string) {
    const chat = $activeChat;
    if (!chat) return;
    try {
      if (chat.id.startsWith("grp_")) await api.groupSend(chat.id, body);
      else await api.sendMessage(chat.id, body);
      await loadActiveMessages();
      chats.set(await api.listChats());
    } catch (e) { showToast(String(e)); }
  }
  async function newDoc(title = tr("Notes"), body = "") {
    const doc = { id: newWsId(), title, body: body.slice(0, 20000) };
    await postToChat("[doc:v1]" + JSON.stringify(doc));
    if ($activeChat) docEditor.set({ chat: $activeChat.id, doc });
  }
  async function newBoard(title = tr("Board"), texts: string[] = []) {
    const b = { id: newWsId(), title, cols: [tr("To do"), tr("Doing"), tr("Done")], cards: texts.slice(0, 200).map((t, i) => ({ id: String(i + 1), t: t.slice(0, 500), c: 0 })) };
    await postToChat("[board:v1]" + JSON.stringify(b));
  }
  /** One tap: the selected messages become a checklist, a document or a board. */
  async function selectionTo(kind: "tasks" | "doc" | "board") {
    const picked = rows.filter((r) => selected.has(r.id));
    const texts = picked.map((r) => (textOf(r) ?? "").replace(/^\[\w+:v1\]\{.*?\}\n?/, "").trim()).filter(Boolean);
    if (!texts.length) { showToast(tr("Nothing with text selected")); return; }
    clearSelection();
    if (kind === "tasks") {
      const tl = { id: plannerId(), title: tr("Tasks from the chat"), items: texts.slice(0, 50).map((t, i) => ({ id: String(i + 1), t: t.slice(0, 300) })), need: { mode: "all", n: 1 }, seq: false, open: true };
      await postToChat("[tasks:v1]" + JSON.stringify(tl));
    } else if (kind === "doc") {
      const lines = picked.map((r) => { const t = (textOf(r) ?? "").trim(); return t ? `**${r.outgoing ? myUsername : (isGroup ? r.from : ($activeChat?.name ?? r.from))}** (${r.time}): ${t}` : ""; }).filter(Boolean);
      await newDoc(tr("Notes from the chat"), lines.join("\n\n"));
    } else {
      await newBoard(tr("Board from the chat"), texts);
    }
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

  /** The private "save to Saved Messages" reaction (⭐ by default). */
  let favEmoji = $derived($settings.favReaction ?? "⭐");
  async function saveToFavorites(row: Row, on: boolean) {
    if (!on) { showToast(tr("Removed the mark — the copy stays in Saved Messages")); return; }
    const it = forwardItemOf(row);
    if (!it) { showToast(tr("This message can't be saved (not downloaded)")); return; }
    const h = fwdHeader(it.author);
    try {
      if (it.kind === "text") await api.sendMessage("__saved__", h ? `[fwd:v1]${h}\n${it.text}` : it.text);
      else if (h) await api.forwardFile("__saved__", it.path, h);
      else await api.attachFile("__saved__", it.path);
      chats.set(await api.listChats());
      showToast(tr("Saved to Saved Messages"));
    } catch (e) { showToast(String(e)); }
  }

  function reactTo(row: Row, emoji: string) {
    if (favEmoji && emoji === favEmoji && $activeChat?.id !== "__saved__") {
      // Private: marks the message here and copies it to Saved Messages; the
      // other side never sees this reaction.
      const added = toggleMyReaction(reactionKey(row), emoji);
      void saveToFavorites(row, added);
      closeMenu();
      return;
    }
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
    if (favEmoji && emoji === favEmoji) { void saveToFavorites(row, added); return; }
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
  // Jump to a message (calendar, search results elsewhere…).
  onMount(() => {
    const onJump = async (e: Event) => {
      const id = (e as CustomEvent<string>).detail;
      if (!id || !messagesEl) return;
      await ensureShown(id);
      const el = messagesEl.querySelector(`[data-id="${id}"]`) as HTMLElement | null;
      el?.scrollIntoView({ behavior: "smooth", block: "center" });
      el?.classList.add("flash");
      setTimeout(() => el?.classList.remove("flash"), 1400);
    };
    window.addEventListener("aqua:jump", onJump);
    return () => window.removeEventListener("aqua:jump", onJump);
  });

  async function scrollToPinned() {
    if (!pinned || !messagesEl) return;
    await ensureShown(pinned.id);
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
    if (chat?.live && !chat.id.startsWith("grp_") && !$ghost.noReadReceipts && !$noReceiptChats.has(chat.id) && !lockedNow && $activeMessages.some((m) => m.from !== "me")) {
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

{#snippet roomIcon(r: Room)}
  {#if r.emoji}<span class="room-ic">{r.emoji}</span>
  {:else if r.color}<span class="room-ic tint" style:--tint={r.color}>{r.name.charAt(0).toUpperCase()}</span>
  {:else}<span class="room-ic">{r.kind === "voice" ? "🔊" : "#"}</span>{/if}
{/snippet}

<main class="active-chat" class:with-rooms={rooms.length > 0} class:voice-on={fullVoice} class:voice-chat={fullVoice && voiceChat}>
  {#if !$activeChat}
    <Welcome />
  {:else}
    {#if $statsFor === $activeChat.id && amChanAdmin && activeGroup}
      <ChannelStats gid={$activeChat.id} subscribers={activeGroup.members.length} posts={statPosts} />
    {/if}
    {#if thread && !lockedNow}
      {@const th = thread}
      <ThreadPanel
        title={th.kind === "comments" ? $t("Comments") : $t("Thread")}
        rootAuthor={th.root.outgoing ? $t("You") : (isGroup ? th.root.from : $activeChat.name)}
        rootText={snippetOf(th.root)}
        rootTime={th.root.time}
        items={threadItems}
        canWrite={$activeChat.live && (th.kind === "replies" ? canPost : commentsAllowed)}
        onSend={sendThread}
        onClose={() => (thread = null)}
        onJump={th.kind === "replies" ? (id) => window.dispatchEvent(new CustomEvent("aqua:jump", { detail: id })) : undefined}
      />
    {/if}
    {#if lockedNow}
      <div class="lock-gate">
        <div class="lg-card">
          <div class="lg-ic">{@html icons.lock}</div>
          <b>{$t("This chat is locked")}</b>
          <span class="lg-sub">{$t("Enter its password to open it.")}</span>
          <!-- svelte-ignore a11y_autofocus -->
          <input class="lg-in" class:err={lockErr} type="password" autocomplete="off" autofocus placeholder={$t("Password")}
            bind:value={lockPass} oninput={() => (lockErr = false)} onkeydown={(e) => { if (e.key === "Enter") void tryUnlock(); }} />
          {#if lockErr}<span class="lg-err">{$t("Wrong password")}</span>{/if}
          {#if $chatLocks[$activeChat.id]?.hint}<span class="lg-hint">{$t("Hint")}: {$chatLocks[$activeChat.id].hint}</span>{/if}
          <button class="lg-btn" onclick={tryUnlock} disabled={lockBusy || !lockPass}>{lockBusy ? "…" : $t("Unlock")}</button>
        </div>
      </div>
    {/if}
    {#if rooms.length && activeGroup}
      {#if roomsOpen}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div class="rooms-veil" onclick={() => (roomsOpen = false)}></div>
      {/if}
      <nav class="rooms" class:open={roomsOpen} aria-label={$t("Rooms")}>
        <button class="rooms-head" onclick={() => openGroupPanel(activeGroup!.id)} title={$t("Group settings")}>
          {#if $avatars[activeGroup.id]}<img src={$avatars[activeGroup.id]} alt="" />{:else}<span class="rh-ava" style:background={activeGroup.gradient}>{activeGroup.title.charAt(0).toUpperCase()}</span>{/if}
          <span class="rh-name">{activeGroup.title}</span>
          <span class="rh-chev">⌄</span>
        </button>
        <div class="rooms-list">
          <div class="rooms-cat">{$t("Text rooms")}</div>
          {#each textRooms as r (r.id)}
            {@const n = roomUnread(r)}
            <button class="room" class:active={activeRoom?.id === r.id} class:unread={n > 0} class:muted={isRoomMuted(r)} onclick={() => { pickRoom(r); roomsOpen = false; }} oncontextmenu={(e) => openRoomMenu(e, r)} title={r.topic ?? ""}>
              {@render roomIcon(r)}<span class="room-name">{r.name}</span>
              {#if r.readonly}<span class="room-lock" title={$t("Only admins write here")}>🔒</span>{/if}
              {#if n > 0}<span class="room-badge">{n > 99 ? "99+" : n}</span>{/if}
            </button>
          {/each}
          {#if rooms.some((r) => r.kind === "voice")}
            <div class="rooms-cat">{$t("Voice rooms")}</div>
            {#each rooms.filter((r) => r.kind === "voice") as r (r.id)}
              {@const live = $activeGroupCalls[activeGroup.id]}
              <button class="room voice" class:active={fullVoice && voiceView[activeGroup.id] === r.id} onclick={() => { pickRoom(r); roomsOpen = false; }} oncontextmenu={(e) => openRoomMenu(e, r)} title={r.topic ?? ""}>
                <span class="room-ic">{r.emoji ?? "🔊"}</span><span class="room-name">{r.name}</span>
              </button>
              {#if live?.users?.length}
                <div class="room-people">
                  {#each live.users.slice(0, 8) as u (u)}<span class="rp">{#if $avatars[u]}<img src={$avatars[u]} alt="" />{:else}<b class="rp-l">{u.charAt(0).toUpperCase()}</b>{/if}<i>{u}</i></span>{/each}
                </div>
              {/if}
            {/each}
          {/if}
        </div>
      </nav>
      {#each roomMenu ? [roomMenu] : [] as rm (rm)}
        {@const r = rm.r}
        <div class="room-menu" role="menu" style:left="{rm.x}px" style:top="{rm.y}px">
          <div class="rm-head">{@render roomIcon(r)}<b>{r.name}</b></div>
          {#if r.kind === "text"}
            <button role="menuitem" onclick={() => { markRoomRead(r); roomMenu = null; }}>✓ {$t("Mark as read")}</button>
          {/if}
          <button role="menuitem" onclick={() => { toggleRoomMute(r); roomMenu = null; }}>{isRoomMuted(r) ? "🔔" : "🔕"} {$t(isRoomMuted(r) ? "Unmute room" : "Mute room")}</button>
          <button role="menuitem" onclick={() => { void navigator.clipboard?.writeText(r.name); roomMenu = null; showToast(tr("Copied")); }}>⧉ {$t("Copy name")}</button>
          {#if canEditRooms}
            <div class="rm-sep"></div>
            <button role="menuitem" onclick={() => { const g = activeGroup!.id; roomMenu = null; openGroupPanel(g, { page: "room-edit", room: r.id }); }}>⚙ {$t("Room settings")}</button>
            {#if rolesOn(activeGroup)}
              <button role="menuitem" onclick={() => { const g = activeGroup!.id; roomMenu = null; openGroupPanel(g, { page: "room-perms", room: r.id }); }}>🛡 {$t("Room permissions")}</button>
            {/if}
            <button role="menuitem" class="danger" onclick={() => { roomMenu = null; void deleteRoomQuick(r); }}>🗑 {$t("Delete room")}</button>
          {/if}
        </div>
      {/each}
    {/if}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <header
      bind:this={headerEl}
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
          {#if $avatars[$activeChat.id]}<img class="ava-img" src={$avatars[$activeChat.id]} alt="" />{/if}
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
            {#if activeRoom}
              <button class="room-switch" onclick={(e) => { e.stopPropagation(); roomsOpen = !roomsOpen; }} oncontextmenu={(e) => { e.stopPropagation(); openRoomMenu(e, activeRoom!); }} title={activeRoom.topic ?? $t("Rooms")}>{activeRoom.emoji ?? "#"} {activeRoom.name}</button>
            {/if}
            <span class="group-name-btn" class:dim={!!activeRoom}>{$activeChat.name}</span>
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
            {#if activeRoom}<span class="grp-title-m">{$activeChat.name}</span>{/if}
            {#if activeRoom?.topic}
              <span class="room-topic" title={activeRoom.topic}>{activeRoom.topic}</span>
              <span class="sep">·</span>
            {/if}
            <span class="group-sub">
              {$tn(activeGroup.members.length, activeGroup.kind === "channel" ? "subscriber" : "member")}
            </span>
            {#if activeGroup.settings.ends_at}
              {@const left = activeGroup.settings.ends_at - Date.now()}
              <span class="sep">·</span>
              <span class="event-room" class:ended={left <= 0}>⏳ {left <= 0 ? $t("event over") : left < 3_600_000 ? tr("ends in {n} min", { n: Math.max(1, Math.round(left / 60_000)) }) : left < 86_400_000 ? tr("ends in {n} h", { n: Math.round(left / 3_600_000) }) : tr("ends in {n} d", { n: Math.round(left / 86_400_000) })}</span>
            {/if}
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
        <button class="icon-btn" class:active={searchOpen} title={$t("Search in chat")} onclick={() => (searchOpen ? closeSearch() : openSearch())}>{@html icons.search}</button>
        {#if callChoices.length}
          <div class="ttl-wrap hdr-menu">
            <button class="icon-btn call-btn" class:active={callMenu} class:live={callChoices.some((c) => c.on)} title={$t("Call")} aria-haspopup="menu" aria-expanded={callMenu}
              onclick={() => { callMenu = !callMenu; toolsMenu = false; ttlMenu = false; }}>{@html icons.phone}<span class="hm-caret"></span></button>
            {#if callMenu}
              <div class="ttl-pop hm-pop" role="menu" tabindex="-1">
                {#each callChoices as c (c.id)}
                  <button class="hm-item" class:on={c.on} role="menuitem" onclick={() => { callMenu = false; c.run(); }}><span class="hm-ic">{@html c.icon}</span><span class="hm-tx"><b>{c.label}</b>{#if c.sub}<small>{c.sub}</small>{/if}</span></button>
                {/each}
              </div>
            {/if}
          </div>
        {/if}
        {#if toolItems.length}
          {@const cid = $activeChat.id}
          <div class="ttl-wrap hdr-menu">
            <button class="icon-btn ttl-btn" class:active={toolsMenu || ttlMenu} class:on={chatTtl > 0} title={$t("Tools")} aria-haspopup="menu" aria-expanded={toolsMenu}
              onclick={() => { if (ttlMenu) { ttlMenu = false; return; } toolsMenu = !toolsMenu; callMenu = false; }}>
              {@html IC.tools}
              {#if chatTtl}<span class="ttl-badge">{ttlShort(chatTtl)}</span>{/if}
            </button>
            {#if toolsMenu}
              <div class="ttl-pop hm-pop" role="menu" tabindex="-1">
                {#each toolItems as it (it.id)}
                  <button class="hm-item" class:on={it.on} role="menuitem" onclick={() => { toolsMenu = false; it.run(); }}><span class="hm-ic">{@html it.icon}</span><span class="hm-tx"><b>{it.label}</b>{#if it.sub}<small>{it.sub}</small>{/if}</span></button>
                {/each}
              </div>
            {/if}
            {#if ttlMenu}
              <div class="ttl-pop" role="menu" tabindex="-1">
                <div class="ttl-h">{$t("Auto-delete messages")}</div>
                <div class="ttl-opts">
                  {#each TTL_PRESETS as v (v)}
                    <button role="menuitemradio" aria-checked={chatTtl === v} class:on={chatTtl === v} disabled={!ttlEditable} onclick={() => setTtl(v)}>{ttlLabel(v)}</button>
                  {/each}
                </div>
                <div class="ttl-note">{ttlEditable ? $t("New messages disappear for everyone in the chat after this time. Messages already here stay.") : $t("Only admins can change the timer.")}</div>
                {#if !isGroup}
                  <div class="ttl-sep"></div>
                  <button class="ttl-toggle" role="menuitemcheckbox" aria-checked={!$noReceiptChats.has(cid)} onclick={() => setReceipts(cid, $noReceiptChats.has(cid))}>
                    <span class="ttl-tt"><b>{$t("Read receipts")}</b><span>{$noReceiptChats.has(cid) ? $t("Off — they won't see when you've read their messages") : $t("On — they see ✓✓ when you open the chat")}</span></span>
                    <span class="ttl-sw" class:on={!$noReceiptChats.has(cid) && !$ghost.noReadReceipts}></span>
                  </button>
                  {#if $ghost.noReadReceipts}<div class="ttl-note">{$t("Ghost mode is on: no read receipts are sent anywhere.")}</div>{/if}
                {/if}
              </div>
            {/if}
          </div>
        {/if}
        <button class="icon-btn" title={$t("Shared media & files")} onclick={toggleSharedMedia}>{@html icons.info}</button>
      </div>
    </header>
    {#if isGroup && activeGroup && !fullVoice}<GroupCallPanel gid={activeGroup.id} onExpand={rooms.length ? openVoiceFull : undefined} />{/if}
    {#if fullVoice && activeGroup}
      <div class="voice-full" class:with-chat={voiceChat} style:top="{headerH}px">
        <GroupCallPanel gid={activeGroup.id} mode="full" roomName={voiceRoomName} chatOpen={voiceChat} onToggleChat={() => (voiceChat = !voiceChat)} />
      </div>
    {/if}

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
      {#if chatReactions.length}
        <div class="sb-reacts" role="toolbar" aria-label={$t("Find by reaction")}>
          <span class="sb-rl">{$t("By reaction")}</span>
          {#each chatReactions as r (r.emoji)}
            {@const ru = reactionUrl(r.emoji, $reactionPack.items)}
            <button class="sb-react" class:on={searchReact === r.emoji} title={parseReaction(r.emoji).emoji}
              onclick={() => { searchReact = searchReact === r.emoji ? "" : r.emoji; searchIdx = Math.max(0, searchHits.length - 1); void tick().then(scrollToHit); }}>
              {#if ru}<span class="sbr-media"><AnimatedMedia src={ru.url} mime={ru.mime} alt={parseReaction(r.emoji).emoji} play="hover" /></span>{:else}{parseReaction(r.emoji).emoji}{/if}
              <i>{r.n}</i>
            </button>
          {/each}
        </div>
      {/if}
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

    {#if rulesNote}
      <div class="rules-bar" style:--acc={activeGroup?.accent ?? "var(--accent)"}>
        <div class="rb-text">
          {#if rulesNote.welcome}<div class="rb-welcome">👋 {rulesNote.welcome}</div>{/if}
          {#if rulesNote.rules}<details><summary>📜 {$t("Rules of this chat")}</summary><p>{rulesNote.rules}</p></details>{/if}
        </div>
        <button class="rb-ok" onclick={dismissRules}>{$t("Got it")}</button>
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
          <button class="sel-act" onclick={() => void selectionTo("tasks")} title={$t("Make a checklist from these messages")}>☑️ {$t("Tasks")}</button>
          <button class="sel-act" onclick={() => void selectionTo("doc")} title={$t("Make a shared document from these messages")}>📄 {$t("Doc")}</button>
          <button class="sel-act" onclick={() => void selectionTo("board")} title={$t("Make a board: one card per message")}>🗂 {$t("Board")}</button>
          <button class="sel-act primary" onclick={forwardSelection}>{$t("Forward")}</button>
          {/if}
        </div>
      </div>
    {/if}

    <div class="msg-stage">
    <div class="chat-wall" aria-hidden="true"></div>
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
      {#each shownRows as m (m.id)}
        {#if m.kind.type === "date_separator"}
          <div class="date-sep">{m.kind.content.label}</div>
        {:else if m.kind.type === "system"}
          {@const tn2 = ttlNote(m.kind.content.text)}
          <div class="sys" class:ttl={!!tn2}>{@html icons.lock}<span>{@html tn2 ?? m.kind.content.text}</span></div>
        {:else}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <div
            class="msg {m.outgoing ? 'out' : 'in'}"
            data-msg-id={m.id}
            data-vkey={isChannel ? reactionKey(m) : undefined}
            class:first={m.first}
            class:last={m.last}
            class:selected={selected.has(m.id)}
            class:search-hit={searchOpen && searchHitSet.has(m.id)}
            class:search-current={searchOpen && m.id === currentHitId}
            data-id={m.id}
            class:menu-target={menu.open && menu.touch && menu.row?.id === m.id}
            onclick={(e) => onMsgClick(m, e)}
            oncontextmenu={(e) => onMsgContext(e, m)}
            onpointerdown={(e) => { notePointer(e); onRowPointerDown(e, m); }}
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
                  <span class="msg-author" style:color={isGroup ? roleColor(activeGroup, m.from) || null : null}>{isGroup ? m.from : $activeChat.name.split(' ')[0]}</span>
                  {#if $clans[m.from]}<ClanTag tag={$clans[m.from]} />{/if}
                  {#if mt?.tag}<span class="member-tag" class:admin={mt.admin}>{mt.tag}</span>{/if}
                  {#if mt?.title}<span class="admin-title">{mt.title}</span>{/if}
                  <span class="msg-time">{m.time}</span>
                </div>
              {/if}
              {#if m.meta?.fwd?.name}
                {@const fw = m.meta.fwd}
                <button class="fwd-tag" class:out={m.outgoing} disabled={!fw.from} onclick={() => fw.from && openProfile(fw.from)}
                  title={fw.from ? `@${fw.from}${fw.at ? " · " + fw.at : ""}` : $t("The author doesn't allow linking to their account")}>
                  <span class="fwd-av" style:background-image={fw.from && $avatars[fw.from] ? `url("${$avatars[fw.from]}")` : undefined}>{fw.from && $avatars[fw.from] ? "" : fw.name.slice(0, 1).toUpperCase()}</span>
                  <span class="fwd-txt"><i>{$t("Forwarded from")}</i> <b>{fw.name}</b>{#if fw.chat}<span class="fwd-chat"> · {fw.chat}</span>{/if}</span>
                </button>
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
                      <button class="sticker-btn" title={$t("Show the pack")} onclick={() => openPackSheet(content.pack ?? "own:-", m.outgoing ? undefined : m.from, content.saved_at)}>
                        <img class="sticker-img" src={url} alt="sticker" />
                      </button>
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
              {:else if m.kind.type === "text" && m.kind.content.text.startsWith("[doc:v1]") && parseDoc(m.kind.content.text) && $activeChat}
                <DocCard doc={parseDoc(m.kind.content.text)!} chat={$activeChat.id} outgoing={m.outgoing} />
                <span class="card-meta">{m.time}</span>
              {:else if m.kind.type === "text" && m.kind.content.text.startsWith("[capsule:v1]") && parseCapsule(m.kind.content.text)}
                <CapsuleCard c={parseCapsule(m.kind.content.text)!} outgoing={m.outgoing} />
                <span class="card-meta">{m.time}</span>
              {:else if m.kind.type === "text" && m.kind.content.text.startsWith("[board:v1]") && parseBoard(m.kind.content.text) && $activeChat}
                <BoardCard board={parseBoard(m.kind.content.text)!} chat={$activeChat.id} outgoing={m.outgoing} />
                <span class="card-meta">{m.time}</span>
              {:else if m.kind.type === "text" && (m.kind.content.text.startsWith("[event:v1]") || m.kind.content.text.startsWith("[tasks:v1]")) && parseSpecial(m.kind.content.text) && $activeChat}
                {@const sp = parseSpecial(m.kind.content.text)!}
                {#if sp.kind === "event"}
                  <EventCard ev={sp.ev} chat={$activeChat.id} outgoing={m.outgoing} />
                {:else}
                  <TasksCard tl={sp.tl} chat={$activeChat.id} members={isGroup && activeGroup ? activeGroup.members.length : 2} outgoing={m.outgoing} />
                {/if}
                <span class="card-meta">{m.time}</span>
              {:else if m.kind.type === "text" && isBotChat && m.kind.content.text.startsWith("[kb:v1]")}
                {@const pb = parseBotText(m.kind.content.text)}
                <div class="bubble">
                  {@render msgBody(pb.body)}
                  <span class="bubble-meta">
                    {@render msgFlags(m)}
                    {m.time}
                  </span>
                </div>
                {#if pb.kb?.kind === "inline" && pb.kb.rows?.length}
                  {@const kb = pb.kb}
                  <div class="ikb">
                    {#each kb.rows ?? [] as row, ri (ri)}
                      <div class="ikb-row">
                        {#each row as b, bi (bi)}
                          <button class="ikb-btn" class:busy={kbBusy === `${kb.m}:${b.cb}`} onclick={() => pressKb(b, kb)} title={b.url ?? b.wa ?? ""}>
                            <span>{b.t}</span>
                            {#if b.url}<i class="ikb-ic">↗</i>{:else if b.wa}<i class="ikb-ic">◫</i>{/if}
                          </button>
                        {/each}
                      </div>
                    {/each}
                  </div>
                {/if}
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

              {#if (isGroup && commentsAllowed && (activeGroup?.kind === "channel" || commentCount.get(reactionKey(m)))) || replyCount.get(quoteOf(m)) || isChannel}
                {@const nc = isGroup ? (commentCount.get(reactionKey(m)) ?? 0) : 0}
                {@const nr = replyCount.get(quoteOf(m)) ?? 0}
                {@const vk = reactionKey(m)}
                <div class="post-foot" class:out={m.outgoing}>
                  {#if (isGroup && commentsAllowed && (activeGroup?.kind === "channel" || nc)) || nr}
                    <button class="thread-chip" onclick={() => openThread(m, nc || activeGroup?.kind === "channel" ? "comments" : "replies")}>
                      {#if nc || activeGroup?.kind === "channel"}💬 {nc ? $tn(nc, "comment") : $t("Comment")}{:else}↩ {$tn(nr, "reply")}{/if}
                      <span class="tc-arrow">›</span>
                    </button>
                  {/if}
                  {#if isChannel}
                    {@const n = Math.max($viewCounts[$activeChat?.id ?? ""]?.[vk] ?? 0, $channelStats[$activeChat?.id ?? ""]?.[vk]?.n ?? 0)}
                    <span class="views-chip" title={$t("Views")}>👁 {n}</span>
                  {/if}
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
      {#if menu.touch}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div class="ctx-veil" onclick={closeMenu}></div>
      {/if}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="ctx-menu" class:touch={menu.touch} use:keepOnScreen style={`left:${menu.x}px; top:${menu.y}px`} role="menu" tabindex="-1" onmousedown={(e) => e.stopPropagation()}>
        <div class="ctx-reactions" class:expanded={reactAll}>
          {#if reactAll && $emojiPacks.length}
            <div class="react-tabs">
              <button class:on={!reactTab} onclick={() => (reactTab = "")}>El Taller</button>
              {#each $emojiPacks.filter((p, i, all) => all.findIndex((q) => q.name === p.name) === i) as p (p.name)}<button class:on={reactTab === p.name} onclick={() => (reactTab = p.name)} title={p.title}>{p.title}</button>{/each}
            </div>
          {/if}
          {#if favEmoji && !reactAll && $activeChat?.id !== "__saved__"}
            <button class="ctx-emoji fav" onclick={() => reactTo(row, favEmoji)} title={$t("Save to Saved Messages (only you see this mark)")}>{favEmoji}</button>
          {/if}
          {#each [...new Set(filterReactions(reactTokens))].filter((r) => r !== favEmoji) as r (r)}
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
        <button class="ctx-item" role="menuitem" onclick={() => openThread(row)}>{$t(isGroup && commentsAllowed ? "Comments & thread" : "Thread")}</button>
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
        {#if DESKTOP_APP && !protectedChat && row.kind.type === "file" && row.kind.content.saved_at}
          {@const p = row.kind.content.saved_at}
          <button class="ctx-item" role="menuitem" onclick={() => { closeMenu(); btSheet.set([p]); }}>{$t("Send via Bluetooth")}</button>
        {/if}
        {#if textOf(row) != null && !protectedChat}
          <button class="ctx-item" role="menuitem" onclick={() => copyRow(row)}>{$t("Copy text")}</button>
          <button class="ctx-item" role="menuitem" onclick={() => selectRowText(row)}>{$t("Select text")}</button>
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
    {#if replyKb && !replyKbHidden}
      <div class="rkb" class:many={(replyKb.rows?.length ?? 0) > 3}>
        {#each replyKb.rows ?? [] as row, ri (ri)}
          <div class="rkb-row">
            {#each row as b, bi (bi)}
              <button class="rkb-btn" onclick={() => pressKb(b, replyKb!)}>{b.t}{#if b.wa}<i class="ikb-ic">◫</i>{/if}</button>
            {/each}
          </div>
        {/each}
      </div>
    {/if}
    {#if $activeChat && !$info?.connected && ($meshStatus?.per_chat?.[$activeChat.id] ?? 0) > 0}
      {@const n = $meshStatus?.per_chat?.[$activeChat.id] ?? 0}
      <div class="mesh-bar">
        <span class="mesh-ic"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M7 7l10 10-5 5V2l5 5L7 17"/></svg></span>
        <span class="mesh-tx">{$t("No server — {n} waiting. They go via Bluetooth when {name} is nearby, or via the server once it's back.", { n, name: $activeChat.name.split(" ")[0] })}</span>
        {#if $meshStatus?.supported}
          <button class="mesh-go" disabled={$meshStatus?.syncing} onclick={async () => { const e = await meshSyncNow(); if (e) showToast(tr(e.replace(/^Error: /, ""))); }}>{$t($meshStatus?.syncing ? "Searching…" : "Find nearby")}</button>
        {/if}
      </div>
    {/if}
    <div class="composer" class:disabled={!liveChat}>
      {#if botMenu}
        <button class="bot-menu-btn" onclick={() => { if ($activeChat && botMenu) openMiniApp({ bot: $activeChat.id, url: botMenu.url, from: "menu" }); }} title={botMenu.url}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><line x1="4" y1="7" x2="20" y2="7"/><line x1="4" y1="12" x2="20" y2="12"/><line x1="4" y1="17" x2="14" y2="17"/></svg>
          <span>{botMenu.text}</span>
        </button>
      {/if}
      {#if replyKb}
        <button class="icon-btn rkb-toggle" class:on={!replyKbHidden} onclick={() => (replyKbHidden = !replyKbHidden)} title={$t("Bot keyboard")} aria-label={$t("Bot keyboard")}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="6" width="20" height="12" rx="2"/><line x1="6" y1="10" x2="6.01" y2="10"/><line x1="10" y1="10" x2="10.01" y2="10"/><line x1="14" y1="10" x2="14.01" y2="10"/><line x1="18" y1="10" x2="18.01" y2="10"/><line x1="7" y1="14" x2="17" y2="14"/></svg>
        </button>
      {/if}
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
            {#if DESKTOP_APP}
              <button class="ap-item" onclick={() => { attachMenuOpen = false; btSheet.set([]); }}>
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><polyline points="7 7 17 17 12 22 12 2 17 7 7 17"/></svg>
                <span>{$t("Bluetooth")}</span>
              </button>
            {/if}
            <button class="ap-item" onclick={() => { attachMenuOpen = false; if ($activeChat) openArticleComposer($activeChat.id); }}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M4 4h12a3 3 0 0 1 3 3v14H7a3 3 0 0 1-3-3V4z"/><path d="M19 7v14"/><line x1="7" y1="9" x2="15" y2="9"/><line x1="7" y1="13" x2="15" y2="13"/></svg>
              <span>{$t("Article")}</span>
            </button>
            <button class="ap-item" onclick={() => { attachMenuOpen = false; if (!canSendKind(activeGroup, myUsername, "poll")) { showToast("Only admins can create polls here"); return; } openPollComposer(); }}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><line x1="6" y1="20" x2="6" y2="12"/><line x1="12" y1="20" x2="12" y2="4"/><line x1="18" y1="20" x2="18" y2="9"/></svg>
              <span>{$t("Poll")}</span>
            </button>
            <button class="ap-item" onclick={() => { attachMenuOpen = false; if ($activeChat) openPlanner("event", $activeChat.id); }}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="5" width="18" height="16" rx="3"/><line x1="3" y1="10" x2="21" y2="10"/><line x1="8" y1="3" x2="8" y2="7"/><line x1="16" y1="3" x2="16" y2="7"/></svg>
              <span>{$t("Event")}</span>
            </button>
            <button class="ap-item" onclick={() => { attachMenuOpen = false; void newDoc(); }}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/><line x1="8" y1="13" x2="16" y2="13"/><line x1="8" y1="17" x2="13" y2="17"/></svg>
              <span>{$t("Document")}</span>
            </button>
            <button class="ap-item" onclick={() => { attachMenuOpen = false; void newBoard(); }}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2"/><line x1="9" y1="3" x2="9" y2="21"/><line x1="15" y1="3" x2="15" y2="21"/></svg>
              <span>{$t("Board")}</span>
            </button>
            <button class="ap-item" onclick={() => { attachMenuOpen = false; if ($activeChat) openPlanner("tasks", $activeChat.id); }}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><polyline points="4 7 6 9 10 5"/><line x1="13" y1="7" x2="20" y2="7"/><polyline points="4 15 6 17 10 13"/><line x1="13" y1="15" x2="20" y2="15"/></svg>
              <span>{$t("Checklist")}</span>
            </button>
            {#if $activeChat && !isGroup && !isBotChat && $activeChat.id !== "__saved__"}
              <button class="ap-item" onclick={() => { attachMenuOpen = false; if ($activeChat) capsuleComposer.set($activeChat.id); }}>
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="5" width="18" height="14" rx="2"/><polyline points="3 7 12 13 21 7"/><circle cx="18" cy="17" r="3" fill="var(--bg-shell)"/><path d="M18 15.5v1.6l1 .7"/></svg>
                <span>{$t("Sealed message")}</span>
              </button>
            {/if}
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
          placeholder={liveChat ? (activeRoom ? `Написать в #${activeRoom.name}…` : `Написать ${$activeChat.name.split(" ")[0]}...`) : "Start a real chat first — see the banner above"}
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
  {/if}
</main>

<svelte:window onmousedown={(e) => { if ((ttlMenu || callMenu || toolsMenu) && !(e.target as HTMLElement).closest?.(".ttl-wrap")) { ttlMenu = false; callMenu = false; toolsMenu = false; } }} />

<style>
  .post-foot { display: flex; align-items: center; gap: 8px; margin-top: 3px; }
  .post-foot.out { align-self: flex-end; }
  .post-foot .thread-chip { margin-top: 0; }
  .views-chip { font-size: 11.5px; color: var(--fg-dim); font-variant-numeric: tabular-nums; }
  .event-room { color: var(--accent); font-weight: 600; }
  .event-room.ended { color: var(--fg-dim); }
  .thread-chip { display: inline-flex; align-items: center; gap: 6px; margin-top: 3px; padding: 4px 10px; border-radius: 999px; border: 1px solid var(--border); background: var(--bg-bubble-in); color: var(--accent); font: inherit; font-size: 12px; font-weight: 700; cursor: pointer; transition: border-color 0.12s, background 0.12s; }
  .thread-chip.out { align-self: flex-end; }
  .thread-chip:hover { border-color: var(--accent); }
  .tc-arrow { color: var(--fg-dim); font-size: 14px; line-height: 1; }
  .lock-gate { position: absolute; inset: 0; z-index: 30; display: grid; place-items: center; background: var(--bg-shell); animation: ttl-pop 0.18s ease-out; }
  .lg-card { width: min(320px, calc(100% - 32px)); display: flex; flex-direction: column; align-items: center; gap: 8px; text-align: center; }
  .lg-ic { width: 64px; height: 64px; border-radius: 50%; display: grid; place-items: center; background: var(--accent-soft); color: var(--accent); margin-bottom: 6px; }
  .lg-ic :global(svg) { width: 28px; height: 28px; }
  .lg-card b { font-size: 17px; }
  .lg-sub, .lg-hint { font-size: 13px; color: var(--fg-muted); }
  .lg-in { width: 100%; box-sizing: border-box; margin-top: 6px; padding: 11px 14px; border-radius: 12px; border: 1px solid var(--border-strong); background: var(--bg-input); color: var(--fg); font: inherit; font-size: 15px; text-align: center; outline: none; }
  .lg-in:focus { border-color: var(--accent); }
  .lg-in.err { border-color: #ff5c5c; animation: lg-shake 0.3s; }
  .lg-err { font-size: 12.5px; color: #ff5c5c; }
  .lg-btn { width: 100%; border: none; border-radius: 12px; padding: 11px; background: var(--accent); color: #06070C; font: inherit; font-weight: 800; cursor: pointer; }
  .lg-btn:disabled { opacity: 0.6; }
  @keyframes lg-shake { 25% { transform: translateX(-6px); } 75% { transform: translateX(6px); } }
  .ctx-emoji.fav { position: relative; }
  .ctx-emoji.fav::after { content: ""; position: absolute; right: -3px; top: 20%; bottom: 20%; width: 1px; background: var(--border-strong); }
  .sb-reacts { display: flex; align-items: center; gap: 5px; padding: 4px 14px 8px; overflow-x: auto; scrollbar-width: none; animation: ttl-pop 0.14s ease-out; }
  .sb-rl { font-size: 11px; font-weight: 700; color: var(--fg-dim); text-transform: uppercase; letter-spacing: 0.04em; margin-right: 4px; white-space: nowrap; }
  .sb-react { display: inline-flex; align-items: center; gap: 4px; padding: 3px 9px; border-radius: 999px; border: 1px solid var(--border); background: var(--bg-input); color: var(--fg); font: inherit; font-size: 14px; cursor: pointer; flex-shrink: 0; }
  .sb-react i { font-style: normal; font-size: 11px; color: var(--fg-dim); }
  .sb-react.on { border-color: var(--accent); background: var(--accent-soft); }
  .sbr-media { width: 18px; height: 18px; display: inline-grid; }
  .sbr-media :global(img), .sbr-media :global(video), .sbr-media :global(canvas) { width: 18px; height: 18px; object-fit: contain; }
  .card-meta { font-size: 10.5px; color: var(--fg-dim); padding: 2px 6px 0; font-variant-numeric: tabular-nums; }
  :global(.flash) .bubble, :global(.flash) .ev, :global(.flash) .tl { animation: msg-flash 1.3s ease-out; }
  @keyframes msg-flash { 0%, 30% { box-shadow: 0 0 0 3px var(--accent); } 100% { box-shadow: 0 0 0 0 transparent; } }
  /* "Forwarded from" — the author's avatar in the corner. */
  .fwd-tag {
    display: inline-flex; align-items: center; gap: 7px; max-width: min(100%, 420px);
    margin: 0 0 2px; padding: 3px 10px 3px 3px; border-radius: 999px;
    border: 1px solid color-mix(in srgb, var(--accent) 22%, transparent);
    background: color-mix(in srgb, var(--bg-bubble-in) 85%, var(--accent) 15%);
    color: var(--fg-muted); font: inherit; font-size: 12px; cursor: pointer; text-align: left;
  }
  .fwd-tag.out { align-self: flex-end; }
  .fwd-tag:disabled { cursor: default; }
  .fwd-tag:not(:disabled):hover { border-color: var(--accent); }
  .fwd-av { width: 20px; height: 20px; border-radius: 50%; flex-shrink: 0; display: grid; place-items: center; font-size: 10px; font-weight: 800; color: #06070C; background: linear-gradient(135deg, var(--accent), var(--accent-2)); background-size: cover; background-position: center; }
  .fwd-txt { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .fwd-txt i { font-style: normal; color: var(--fg-dim); }
  .fwd-txt b { color: var(--accent); }
  .fwd-chat { color: var(--fg-dim); }
  /* Bot keyboards */
  .ikb { display: flex; flex-direction: column; gap: 4px; margin-top: 4px; width: min(100%, 420px); }
  .ikb-row { display: flex; gap: 4px; }
  .ikb-btn {
    flex: 1; min-width: 0; position: relative; display: flex; align-items: center; justify-content: center; gap: 6px;
    padding: 8px 10px; border-radius: 10px; border: 1px solid color-mix(in srgb, var(--accent) 25%, transparent);
    background: color-mix(in srgb, var(--bg-bubble-in) 82%, var(--accent) 18%); color: var(--fg);
    font: inherit; font-size: 13.5px; font-weight: 600; cursor: pointer; transition: background 0.12s, transform 0.08s;
  }
  .ikb-btn span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .ikb-btn:hover { background: color-mix(in srgb, var(--bg-bubble-in) 70%, var(--accent) 30%); }
  .ikb-btn:active { transform: scale(0.98); }
  .ikb-btn.busy { opacity: 0.6; }
  .ikb-btn.busy::after { content: ""; position: absolute; right: 8px; width: 10px; height: 10px; border-radius: 50%; border: 2px solid var(--accent-soft); border-top-color: var(--accent); animation: ikb-rot 0.7s linear infinite; }
  @keyframes ikb-rot { to { transform: rotate(360deg); } }
  .ikb-ic { font-style: normal; font-size: 11px; color: var(--accent); margin-left: 6px; }
  .rkb { display: flex; flex-direction: column; gap: 5px; padding: 8px 12px 4px; animation: ttl-pop 0.14s ease-out; }
  .rkb.many { max-height: 190px; overflow-y: auto; }
  .rkb-row { display: flex; gap: 5px; }
  .rkb-btn { flex: 1; min-width: 0; padding: 10px 8px; border-radius: 10px; border: 1px solid var(--border); background: var(--bg-input); color: var(--fg); font: inherit; font-size: 14px; font-weight: 600; cursor: pointer; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .rkb-btn:hover { background: var(--bg-hover); border-color: var(--border-strong); }
  .rkb-toggle.on { color: var(--accent); }
  .bot-menu-btn { display: inline-flex; align-items: center; gap: 6px; flex-shrink: 0; height: 36px; padding: 0 12px; border-radius: 10px; border: none; background: var(--accent); color: #06070C; font: inherit; font-size: 13px; font-weight: 700; cursor: pointer; max-width: 160px; }
  .bot-menu-btn span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .bot-menu-btn svg { width: 16px; height: 16px; flex-shrink: 0; }
  .ttl-wrap { position: relative; display: inline-flex; }
  .hm-pop { width: 270px; padding: 6px; display: flex; flex-direction: column; gap: 2px; }
  .hm-item { display: flex; align-items: center; gap: 11px; width: 100%; padding: 8px 10px; border: none; border-radius: 10px; background: none; color: var(--fg); font: inherit; text-align: left; cursor: pointer; transition: background 0.12s; }
  .hm-item:hover { background: var(--bg-hover); }
  .hm-item.on .hm-ic { color: var(--accent); background: var(--accent-soft); }
  .hm-ic { width: 32px; height: 32px; flex-shrink: 0; border-radius: 9px; display: grid; place-items: center; background: var(--bg-input); color: var(--fg-muted); }
  .hm-ic :global(svg) { width: 17px; height: 17px; }
  .hm-tx { min-width: 0; display: flex; flex-direction: column; gap: 1px; }
  .hm-tx b { font-size: 13.5px; font-weight: 600; }
  .hm-tx small { font-size: 11.5px; color: var(--fg-dim); }
  .call-btn { position: relative; }
  .call-btn.live { color: var(--online); }
  .hm-caret { position: absolute; right: 3px; bottom: 4px; width: 0; height: 0; border-left: 3px solid transparent; border-right: 3px solid transparent; border-top: 4px solid currentColor; opacity: 0.6; }
  .ttl-btn { position: relative; }
  .ttl-btn.on { color: var(--accent); }
  .ttl-badge { position: absolute; right: -2px; bottom: 0; font-size: 9px; font-weight: 800; line-height: 1; padding: 2px 3px; border-radius: 6px; background: var(--accent); color: #06070C; pointer-events: none; }
  .ttl-pop {
    position: absolute; top: calc(100% + 8px); right: 0; z-index: 40; width: 290px;
    background: var(--bg-shell); border: 1px solid var(--border-strong); border-radius: 14px; padding: 12px;
    box-shadow: 0 18px 40px -12px rgba(0, 0, 0, 0.55); animation: ttl-pop 0.16s cubic-bezier(.2,.9,.3,1.15); transform-origin: top right;
    cursor: default;
  }
  @keyframes ttl-pop { from { opacity: 0; transform: translateY(-6px) scale(0.97); } }
  .ttl-h { font-size: 12px; font-weight: 800; color: var(--accent); margin-bottom: 8px; }
  .ttl-opts { display: grid; grid-template-columns: repeat(3, 1fr); gap: 6px; }
  .ttl-opts button { border: 1px solid var(--border); background: var(--bg-input); color: var(--fg); border-radius: 9px; padding: 7px 4px; font: inherit; font-size: 12.5px; font-weight: 600; cursor: pointer; transition: background 0.12s, border-color 0.12s; }
  .ttl-opts button:hover:not(:disabled) { background: var(--bg-hover); border-color: var(--border-strong); }
  .ttl-opts button.on { background: var(--accent-soft); border-color: var(--accent); color: var(--accent); }
  .ttl-opts button:disabled { opacity: 0.55; cursor: default; }
  .ttl-note { font-size: 11.5px; color: var(--fg-dim); line-height: 1.45; margin-top: 8px; }
  .ttl-sep { height: 1px; background: var(--border); margin: 10px -12px; }
  .ttl-toggle { width: 100%; display: flex; align-items: center; gap: 10px; border: none; background: none; color: var(--fg); padding: 2px 0; text-align: left; font: inherit; cursor: pointer; }
  .ttl-tt { flex: 1; display: flex; flex-direction: column; gap: 2px; }
  .ttl-tt b { font-size: 13px; }
  .ttl-tt span { font-size: 11.5px; color: var(--fg-dim); }
  .ttl-sw { width: 34px; height: 20px; border-radius: 10px; background: var(--bg-hover); border: 1px solid var(--border-strong); position: relative; flex-shrink: 0; transition: background 0.15s; }
  .ttl-sw::after { content: ""; position: absolute; top: 2px; left: 2px; width: 14px; height: 14px; border-radius: 50%; background: var(--fg-muted); transition: transform 0.15s, background 0.15s; }
  .ttl-sw.on { background: var(--accent); border-color: transparent; }
  .ttl-sw.on::after { transform: translateX(14px); background: #fff; }
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
    position: relative;
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
    position: relative;
    z-index: 1;
    min-height: 0;
    overscroll-behavior: contain;
  }
  /* isolate: the wallpaper / messages z-order stays inside this box, so the
     composer's popups (attach menu, mentions…) still draw above the messages. */
  /* The wallpaper sits on its own composited layer behind the scroller, so
     scrolling moves only the messages (a background on the scroller itself
     with attachment:local was re-rasterised on every scroll step). */
  .msg-stage { flex: 1; min-height: 0; position: relative; display: flex; flex-direction: column; isolation: isolate; }
  .chat-wall {
    position: absolute; inset: 0; z-index: 0; pointer-events: none;
    background: var(--chat-wallpaper, transparent);
    transform: translateZ(0);
    contain: strict;
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
    animation: msg-in 0.34s cubic-bezier(.2,.9,.25,1.08);
    transform-origin: bottom left;
  }
  .msg.out .bubble { transform-origin: bottom right; }
  @keyframes msg-in {
    from { opacity: 0; transform: translateY(8px) scale(0.97); }
    to   { opacity: 1; transform: translateY(0) scale(1); }
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
  .sticker-btn { border: none; background: transparent; padding: 0; cursor: pointer; display: block; border-radius: 12px; transition: transform 0.15s ease; }
  .sticker-btn:hover { transform: scale(1.04); }
  .sticker-btn:active { transform: scale(0.97); }
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
  .sel-actions { display: flex; gap: 6px; min-width: 0; overflow-x: auto; scrollbar-width: none; }
  .sel-actions .sel-act { flex-shrink: 0; }
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
  /* Touch: Telegram-style pop with a soft spring, over a dimmed chat. */
  .ctx-menu.touch {
    transform-origin: top left;
    animation: ctx-pop 0.22s cubic-bezier(0.2, 0.9, 0.25, 1.12);
    border-radius: 14px;
  }
  .ctx-menu.touch :global(.ctx-item) { padding-top: 11px; padding-bottom: 11px; }
  @keyframes ctx-pop {
    from { opacity: 0; transform: scale(0.82) translateY(-6px); }
    60% { opacity: 1; }
    to { opacity: 1; transform: scale(1) translateY(0); }
  }
  .mesh-bar {
    display: flex; align-items: center; gap: 10px; margin: 0 12px 6px; padding: 8px 10px 8px 12px;
    border-radius: 12px; font-size: 12.5px; line-height: 1.35; color: var(--fg-muted);
    background: color-mix(in srgb, #0A84FF 12%, var(--bg-surface)); border: 1px solid color-mix(in srgb, #0A84FF 35%, transparent);
    animation: veil-in .2s ease-out;
  }
  .mesh-ic { width: 18px; height: 18px; color: #0A84FF; flex-shrink: 0; }
  .mesh-ic svg { width: 100%; height: 100%; }
  .mesh-tx { flex: 1; min-width: 0; }
  .mesh-go { border: none; border-radius: 9px; padding: 6px 11px; background: #0A84FF; color: #fff; font: inherit; font-size: 12px; font-weight: 700; cursor: pointer; white-space: nowrap; }
  .mesh-go:disabled { opacity: .6; }
  .ctx-veil {
    position: fixed; inset: 0; z-index: 119;
    background: rgba(0, 0, 0, 0.28);
    animation: veil-in 0.18s ease-out;
  }
  @keyframes veil-in { from { opacity: 0; } }
  .msg.menu-target { position: relative; z-index: 120; }
  .msg.menu-target :global(.bubble) { animation: bubble-lift 0.22s cubic-bezier(0.2, 0.9, 0.25, 1.12) both; }
  @keyframes bubble-lift { from { transform: scale(1); } 50% { transform: scale(0.97); } to { transform: scale(1); } }
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
  .plain-note { color: #f5b14c; }
  /* ---- voice room full screen ---- */
  .voice-full { position: absolute; left: 0; right: 0; bottom: 0; z-index: 4; animation: vf-in .35s cubic-bezier(.2,.8,.2,1); }
  @keyframes vf-in { from { opacity: 0; transform: scale(.985); } }
  .active-chat.with-rooms .voice-full { left: 232px; }
  .voice-full.with-chat { right: 380px; border-right: 1px solid var(--border); }
  .active-chat.voice-chat > :not(.chat-header):not(.rooms):not(.voice-full) { align-self: flex-end; width: 380px; max-width: 380px; box-sizing: border-box; }
  .active-chat.voice-on:not(.voice-chat) .composer-note { visibility: hidden; }
  .active-chat.voice-chat .kbds { display: none; }
  .rp-l { width: 20px; height: 20px; border-radius: 50%; display: grid; place-items: center; font-size: 10px; background: linear-gradient(135deg, var(--accent), var(--accent-2)); color: #06070c; }
  @media (max-width: 760px) { .active-chat.with-rooms .voice-full { left: 0; } .voice-full.with-chat { right: 0; top: auto !important; height: 45%; } }
  /* ---- rooms sidebar ---- */
  .active-chat.with-rooms { padding-left: 232px; }
  .rooms { position: absolute; left: 0; top: 0; bottom: 0; width: 232px; display: flex; flex-direction: column;
    background: color-mix(in srgb, var(--bg-list) 92%, transparent); border-right: 1px solid var(--border); z-index: 5;
    animation: rooms-in .3s var(--ease-out, ease-out); }
  @keyframes rooms-in { from { opacity: 0; transform: translateX(-8px); } }
  .rooms-head { display: flex; align-items: center; gap: 10px; height: 64px; padding: 0 14px; border: none; border-bottom: 1px solid var(--border);
    background: transparent; color: var(--fg); cursor: pointer; text-align: left; }
  .rooms-head:hover { background: var(--bg-hover); }
  .rooms-head img, .rh-ava { width: 30px; height: 30px; border-radius: 9px; object-fit: cover; display: grid; place-items: center; font-weight: 800; color: #06070c; flex-shrink: 0; }
  .rh-name { flex: 1; font-weight: 800; font-size: 14px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .rh-chev { color: var(--fg-dim); }
  .rooms-list { flex: 1; overflow-y: auto; padding: 8px; }
  .rooms-cat { font-size: 11px; font-weight: 800; letter-spacing: .06em; text-transform: uppercase; color: var(--fg-dim); padding: 12px 8px 6px; }
  .room { width: 100%; display: flex; align-items: center; gap: 8px; padding: 7px 10px; border: none; border-radius: 8px; background: transparent;
    color: var(--fg-muted); font-size: 14px; cursor: pointer; text-align: left; transition: background .15s, color .15s, transform .15s; }
  .room:hover { background: var(--bg-hover); color: var(--fg); }
  .room:active { transform: scale(.98); }
  .room.active { background: var(--bg-active); color: var(--fg); box-shadow: inset 3px 0 0 var(--accent); }
  .room.unread { color: var(--fg); font-weight: 700; }
  .room-ic { width: 18px; text-align: center; color: var(--fg-dim); font-weight: 700; flex-shrink: 0; }
  .room-ic.tint { width: 20px; height: 20px; border-radius: 50%; display: grid; place-items: center; font-size: 10.5px; font-weight: 800; color: #fff;
    background: var(--tint); box-shadow: 0 2px 6px -2px var(--tint); }
  .room.muted { opacity: .55; }
  .room-topic { max-width: 320px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--fg-muted); }
  .room-menu { position: fixed; z-index: 400; width: 220px; padding: 6px; border-radius: 12px; background: var(--bg-shell);
    border: 1px solid var(--border-strong); box-shadow: 0 18px 50px -12px rgba(0,0,0,.7); display: flex; flex-direction: column; gap: 1px;
    animation: rm-in .16s cubic-bezier(.2,.9,.25,1.1); transform-origin: top left; }
  @keyframes rm-in { from { opacity: 0; transform: scale(.95); } }
  .rm-head { display: flex; align-items: center; gap: 8px; padding: 6px 8px 8px; font-size: 13px; border-bottom: 1px solid var(--border); margin-bottom: 4px; }
  .rm-head b { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .room-menu button { text-align: left; border: none; background: transparent; color: var(--fg); font: inherit; font-size: 13px; padding: 7px 10px; border-radius: 8px; cursor: pointer; }
  .room-menu button:hover { background: var(--bg-hover, var(--bg-input)); }
  .room-menu button.danger { color: #ff6b6b; }
  .rm-sep { height: 1px; background: var(--border); margin: 4px 2px; }
  .room-name { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .room-lock { font-size: 11px; opacity: .7; }
  .room-badge { min-width: 18px; padding: 0 6px; height: 18px; border-radius: 9px; background: var(--accent); color: #06070c; font-size: 11px; font-weight: 800; display: grid; place-items: center; }
  .room-people { display: flex; flex-direction: column; gap: 4px; padding: 2px 0 6px 34px; }
  .rp { display: flex; align-items: center; gap: 6px; font-size: 12px; color: var(--fg-muted); }
  .rp img, .rp { --s: 20px; }
  .rp img { width: 20px; height: 20px; border-radius: 50%; }
  .rp i { font-style: normal; }
  .room-switch { border: none; background: var(--accent-soft); color: var(--accent); border-radius: 8px; padding: 1px 8px; font: inherit; font-weight: 800; cursor: pointer; margin-right: 6px;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis; min-width: 0; max-width: 62%; flex-shrink: 1; }
  .group-name-btn.dim { color: var(--fg-muted); font-weight: 600; font-size: .85em; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; min-width: 0; flex-shrink: 1; }
  .peer-name { min-width: 0; }
  .rooms-veil { display: none; }
  .grp-title-m { display: none; }
  /* Phone: the room gets the whole title line; the group name moves to the
     status line ("Aqua Lab · 4 members"). */
  @media (max-width: 720px) {
    .group-name-btn.dim { display: none; }
    .room-switch { max-width: 100%; }
    .grp-title-m { display: inline; color: var(--fg); font-weight: 600; margin-right: 6px; }
    .grp-title-m::after { content: "·"; margin-left: 6px; color: var(--fg-dim); }
    .active-chat.with-rooms .chat-actions .group-gear { display: none; }
  }
  @media (min-width: 761px) { .room-switch { pointer-events: none; } }
  @media (max-width: 760px) {
    .active-chat.with-rooms { padding-left: 0; }
    .rooms { width: min(280px, 82vw); transform: translateX(-102%); transition: transform .3s var(--ease-out, ease); box-shadow: 0 0 40px rgba(0,0,0,.5); z-index: 30; animation: none; }
    .rooms.open { transform: none; }
    /* Tap anywhere outside the drawer to close it. */
    .rooms-veil { display: block; position: absolute; inset: 0; z-index: 29; background: rgba(4, 6, 12, .45); animation: rv-in .2s ease-out; }
  }
  @keyframes rv-in { from { opacity: 0; } }
  .rules-bar { display: flex; gap: 10px; align-items: flex-start; padding: 10px 14px; border-bottom: 1px solid var(--border);
    background: color-mix(in srgb, var(--acc) 12%, var(--bg-shell)); animation: rb-in .35s cubic-bezier(.2,.8,.2,1); }
  .rb-text { flex: 1; min-width: 0; font-size: 13px; }
  .rb-welcome { font-weight: 600; margin-bottom: 4px; }
  .rules-bar summary { cursor: pointer; color: var(--acc); font-weight: 700; }
  .rules-bar summary::marker { color: var(--acc); }
  .rules-bar p { margin: 6px 0 0; white-space: pre-wrap; line-height: 1.45; max-height: 30vh; overflow-y: auto; }
  .rb-ok { border: none; border-radius: 8px; padding: 6px 12px; background: var(--acc); color: #06070c; font-weight: 700; cursor: pointer; }
  @keyframes rb-in { from { opacity: 0; transform: translateY(-6px); } to { opacity: 1; transform: none; } }
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
    position: relative;
    flex-shrink: 0;
  }
  /* The blur lives on a pseudo-layer: a backdrop-filter on the bar itself would make it a
     "backdrop root", and popups inside it (attach menu, room list) couldn't blur the chat. */
  .composer::before, .chat-header::before {
    content: "";
    position: absolute;
    inset: 0;
    z-index: -1;
    pointer-events: none;
    backdrop-filter: blur(12px);
    -webkit-backdrop-filter: blur(12px);
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
