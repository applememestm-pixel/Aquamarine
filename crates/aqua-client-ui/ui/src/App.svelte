<script lang="ts">
  import { onMount } from "svelte";
  import { bootstrap, activeTab, info, activeId, backToList } from "./lib/stores";
  import { openRegistration, closeRegistration, registrationDismissed } from "./lib/components/RegistrationWizard.svelte";
  import { initTheme } from "./lib/theme";
  import NavRail from "./lib/components/NavRail.svelte";
  import ChatList from "./lib/components/ChatList.svelte";
  import ChatWindow from "./lib/components/ChatWindow.svelte";
  import CallsPanel from "./lib/components/CallsPanel.svelte";
  import FilesPanel from "./lib/components/FilesPanel.svelte";
  import ContactsPanel from "./lib/components/ContactsPanel.svelte";
  import FingerprintModal from "./lib/components/FingerprintModal.svelte";
  import InputPrompt from "./lib/components/InputPrompt.svelte";
  import Toast from "./lib/components/Toast.svelte";
  import CallModal from "./lib/components/CallModal.svelte";
  import MediaPicker from "./lib/components/MediaPicker.svelte";
  import SettingsPanel, { openSettings } from "./lib/components/SettingsPanel.svelte";
  import ProfileModal from "./lib/components/ProfileModal.svelte";
  import PhotoEditor from "./lib/components/PhotoEditor.svelte";
  import ArticleComposer from "./lib/components/ArticleComposer.svelte";
  import CircleRecorder from "./lib/components/CircleRecorder.svelte";
  import StickerEditor from "./lib/components/StickerEditor.svelte";
  import RegistrationWizard from "./lib/components/RegistrationWizard.svelte";
  import LockScreen from "./lib/components/LockScreen.svelte";
  import GroupCallPanel from "./lib/components/GroupCallPanel.svelte";
  import { groupCall, initGroupCalls } from "./lib/groupCall";
  import Lightbox from "./lib/components/Lightbox.svelte";
  import ForwardPicker from "./lib/components/ForwardPicker.svelte";
  import TopPlayer from "./lib/components/TopPlayer.svelte";
  import FolderManager from "./lib/components/FolderManager.svelte";
  import SharedMediaPanel from "./lib/components/SharedMediaPanel.svelte";
  import PollComposer from "./lib/components/PollComposer.svelte";
  import NewGroupWizard from "./lib/components/NewGroupWizard.svelte";
  import GroupPanel from "./lib/components/GroupPanel.svelte";
  import UserPanel from "./lib/components/UserPanel.svelte";
  import TitleBar from "./lib/components/TitleBar.svelte";
  import ShortcutsHelp from "./lib/components/ShortcutsHelp.svelte";
  import ImageCropper from "./lib/components/ImageCropper.svelte";
  import { initPresence } from "./lib/presence";
  import { refreshPeer, syncMyProfileOnConnect } from "./lib/people";
  import { chats } from "./lib/stores";
  import { initShortcuts } from "./lib/shortcuts";
  import { loadReactionPack } from "./lib/reactionPack";

  // Entry gate: show the registration / login window on EVERY launch until
  // the user is in a live session. A registered user gets a one-click
  // "Continue as @me" inside the window; they can also register/log in as
  // someone else. Nothing auto-enters silently — the window is always the
  // door in, so registration is always testable.
  let everConnected = $state(false);

  // Profiles: publish ours on every (re)connect, fetch everyone we chat with.
  let wasConnected = false;
  $effect(() => {
    const on = !!$info?.connected;
    if (on && !wasConnected) void syncMyProfileOnConnect();
    wasConnected = on;
  });
  $effect(() => {
    if (!$info?.connected) return;
    for (const c of $chats) if (c.live) void refreshPeer(c.id);
  });
  $effect(() => {
    const i = $info;
    if (!i) return;
    if (i.connected) { everConnected = true; closeRegistration(); return; }
    // Only show the door on first launch — a transient drop mid-session must
    // not pop the login window over the user's chat (the plug icon reconnects).
    if (!everConnected && !$registrationDismissed) openRegistration();
  });

  onMount(() => {
    // No browser context menu ("Back / Reload / Print / Save as…") anywhere
    // in the app — only in text fields, where copy / paste / spellcheck help.
    const noNativeMenu = (e: MouseEvent) => {
      if (e.defaultPrevented) return;
      const t = e.target as HTMLElement | null;
      if (t?.closest?.("input, textarea, [contenteditable=\"true\"], [contenteditable=\"\"]")) return;
      e.preventDefault();
    };
    window.addEventListener("contextmenu", noNativeMenu);
    initGroupCalls();
    initTheme();
    bootstrap();
    initPresence();
    void loadReactionPack();
    const stopShortcuts = initShortcuts();
    // Browser shortcuts that would pop Chromium chrome (print dialog,
    // save-page, find bar, reload, zoom, view-source, downloads…). We
    // run as a desktop app, so none of these should ever fire.
    const BLOCKED_CTRL = new Set(["p", "s", "f", "g", "r", "u", "j", "o", "+", "-", "=", "_", "0"]);
    const onKey = (e: KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey;
      // Our own shortcuts first.
      if (mod && e.key.toLowerCase() === "k") {
        e.preventDefault();
        (document.querySelector<HTMLInputElement>("#search"))?.focus();
        return;
      }
      if (mod && e.key === ",") {
        e.preventDefault();
        openSettings();
        return;
      }
      // Reload keys — would blow away app state.
      if (e.key === "F5" || (mod && e.key.toLowerCase() === "r")) {
        e.preventDefault();
        return;
      }
      // Ctrl/Cmd browser combos.
      if (mod && BLOCKED_CTRL.has(e.key.toLowerCase())) {
        e.preventDefault();
        return;
      }
      // Alt+Arrow history navigation.
      if (e.altKey && (e.key === "ArrowLeft" || e.key === "ArrowRight")) {
        e.preventDefault();
        return;
      }
    };
    // Block Ctrl+wheel zoom (pinch-zoom on trackpads too).
    const onWheel = (e: WheelEvent) => { if (e.ctrlKey || e.metaKey) e.preventDefault(); };
    // Block file drag-and-drop onto the window (Chromium would try to
    // open/navigate to the dropped file).
    const onDrop = (e: DragEvent) => {
      const t = e.target as HTMLElement;
      if (!t.closest("input, textarea, [contenteditable]")) e.preventDefault();
    };
    const onDragOver = (e: DragEvent) => {
      const t = e.target as HTMLElement;
      if (!t.closest("input, textarea, [contenteditable]")) e.preventDefault();
    };
    // Suppress the native Chromium/webview right-click menu
    // (Reload / Back / Inspect…) everywhere except real text fields,
    // where the paste/copy menu is actually useful.
    const onCtx = (e: MouseEvent) => {
      const t = e.target as HTMLElement;
      const editable = t.closest("input, textarea, [contenteditable]");
      if (!editable) e.preventDefault();
    };
    // Block middle-click autoscroll + drag-navigation artefacts.
    const onAux = (e: MouseEvent) => { if (e.button === 1) e.preventDefault(); };
    // Single-media coordinator: whenever any audio/video starts playing,
    // pause every other one (inline previews, the lightbox, the top
    // background player). Only one thing plays at a time.
    // Call media (voice, stream audio, video tiles) is marked
    // `data-call-media` and is never paused by, nor pauses, anything else —
    // otherwise starting a video tile in a call silenced the peer's voice.
    const onMediaPlay = (e: Event) => {
      const el = e.target as HTMLMediaElement;
      // Muted loops (animated emoji / stickers / GIFs) never take part.
      if (el.hasAttribute?.("data-call-media") || el.muted) return;
      document.querySelectorAll("audio, video").forEach((m) => {
        const mm = m as HTMLMediaElement;
        if (mm.hasAttribute("data-call-media") || mm.muted) return;
        if (mm !== el && !mm.paused) { try { mm.pause(); } catch { /* ignore */ } }
      });
    };
    // Phone Back button / back gesture: leave the open chat for the list.
    const onPop = () => {
      if (document.body.classList.contains("chat-open")) backToList(true);
    };
    window.addEventListener("popstate", onPop);
    document.addEventListener("keydown", onKey);
    document.addEventListener("contextmenu", onCtx);
    document.addEventListener("auxclick", onAux);
    document.addEventListener("wheel", onWheel, { passive: false });
    document.addEventListener("play", onMediaPlay, true);
    window.addEventListener("drop", onDrop);
    window.addEventListener("dragover", onDragOver);
    return () => {
      stopShortcuts();
      window.removeEventListener("popstate", onPop);
      document.removeEventListener("keydown", onKey);
      document.removeEventListener("contextmenu", onCtx);
      document.removeEventListener("auxclick", onAux);
      document.removeEventListener("wheel", onWheel);
      document.removeEventListener("play", onMediaPlay, true);
      window.removeEventListener("drop", onDrop);
      window.removeEventListener("dragover", onDragOver);
    };
  });
</script>

<div class="root">
  <TitleBar />
  <TopPlayer />
  <div class="app">
    <NavRail />
    {#if $activeTab === "chats"}
      <ChatList />
    {:else if $activeTab === "calls"}
      <CallsPanel />
    {:else if $activeTab === "files"}
      <FilesPanel />
    {:else if $activeTab === "contacts"}
      <ContactsPanel />
    {/if}
    <div class="main-pane">
      <ChatWindow />
      <SharedMediaPanel />
    </div>
    {#if $groupCall && $groupCall.gid !== $activeId}<GroupCallPanel gid={$groupCall.gid} mode="pill" />{/if}
    <UserPanel />
  </div>
</div>

<FingerprintModal />
<InputPrompt />
<CallModal />
<MediaPicker />
<SettingsPanel />
<ProfileModal />
<PhotoEditor />
<ArticleComposer />
<CircleRecorder />
<StickerEditor />
<RegistrationWizard />
<LockScreen />
<Lightbox />
<ForwardPicker />
<FolderManager />
<PollComposer />
<NewGroupWizard />
<GroupPanel />
<ShortcutsHelp />
<ImageCropper />
<Toast />

<style>
  .root {
    height: 100%;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .root :global(.app) {
    height: auto;
    flex: 1 1 auto;
    min-height: 0;
  }
  /* Right pane holds the chat window + the optional shared-media panel. */
  .main-pane {
    grid-column: 3;
    grid-row: 1 / span 2;
    display: flex;
    min-width: 0;
    overflow: hidden;
  }
  .main-pane :global(.active-chat) {
    flex: 1 1 auto;
    min-width: 0;
  }
</style>
