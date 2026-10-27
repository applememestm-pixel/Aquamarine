<script lang="ts">
  import Lazy from "./lib/components/Lazy.svelte";
  import { onMount } from "svelte";
  import { startAppIcon } from "./lib/appIcon";
  import { startAutoscroll } from "./lib/autoscroll";
  import { startKeyboardInset } from "./lib/keyboard";
  import { startBack, onBack } from "./lib/back";
  import { startRenames } from "./lib/rename";
  import { startNetwork } from "./lib/network";
  import { migrateCustomWallpaper, fitCurrentWallpaper, followWallpaper } from "./lib/theme";
  import { api } from "./lib/tauri";
  import { bootstrap, activeTab, info, activeId, backToList, connect, quickConnect } from "./lib/stores";
  import { openRegistration, closeRegistration, registrationDismissed, wizardHold } from "./lib/components/RegistrationWizard.svelte";
  import { initTheme, settings } from "./lib/theme";
  import { get } from "svelte/store";
  import { startPower } from "./lib/power";
  import NavRail from "./lib/components/NavRail.svelte";
  import ChatList from "./lib/components/ChatList.svelte";
  import ChatWindow from "./lib/components/ChatWindow.svelte";
  import CallsPanel from "./lib/components/CallsPanel.svelte";
  import FilesPanel from "./lib/components/FilesPanel.svelte";
  import ContactsPanel from "./lib/components/ContactsPanel.svelte";
  import FingerprintModal from "./lib/components/FingerprintModal.svelte";
  import InputPrompt from "./lib/components/InputPrompt.svelte";
  import NewChatSheet from "./lib/components/NewChatSheet.svelte";
  import QrScanner from "./lib/components/QrScanner.svelte";
  import WhatsNew, { maybeShowWhatsNew } from "./lib/components/WhatsNew.svelte";
  import Toast, { showToast } from "./lib/components/Toast.svelte";
  import Confirm from "./lib/components/Confirm.svelte";
  import SendDialog from "./lib/components/SendDialog.svelte";
  import { mediaSupport, installHint } from "./lib/media";
  import { tr } from "./lib/i18n";
  import CallModal from "./lib/components/CallModal.svelte";
  import MediaPicker from "./lib/components/MediaPicker.svelte";
  import { settingsOpen, openSettings } from "./lib/panels/settings";
  import ProfileModal from "./lib/components/ProfileModal.svelte";
  import { editorState } from "./lib/panels/photoEditor";
  import { articleState } from "./lib/panels/articleComposer";
  import ArticleReader from "./lib/components/ArticleReader.svelte";
  import CircleRecorder from "./lib/components/CircleRecorder.svelte";
  import PackSheet from "./lib/components/PackSheet.svelte";
  import MiniApp from "./lib/components/MiniApp.svelte";
  import PlannerComposer from "./lib/components/PlannerComposer.svelte";
  import DocEditor from "./lib/components/DocEditor.svelte";
  import { startWorkspace } from "./lib/workspace";
  import { startChannelStats } from "./lib/channelStats";
  import { startHandoff } from "./lib/handoff";
  import HandoffBanner from "./lib/components/HandoffBanner.svelte";
  import CapsuleComposer from "./lib/components/CapsuleComposer.svelte";
  import CapsuleAlerts from "./lib/components/CapsuleAlerts.svelte";
  import { startCapsules } from "./lib/capsules";
  import BluetoothSheet from "./lib/components/BluetoothSheet.svelte";
  import BluetoothIncoming from "./lib/components/BluetoothIncoming.svelte";
  import { startBluetooth } from "./lib/bluetooth";
  import { startMesh } from "./lib/mesh";
  import ReminderPopup from "./lib/components/ReminderPopup.svelte";
  import ChatCalendar from "./lib/components/ChatCalendar.svelte";
  import { startPlanner } from "./lib/planner";
  import { stickerEditorState, stickerQueue } from "./lib/panels/stickerEditor";
  import PackManager from "./lib/components/PackManager.svelte";
  import RegistrationWizard from "./lib/components/RegistrationWizard.svelte";
  import ChatGroupSheet from "./lib/components/ChatGroupSheet.svelte";
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
  import { groupPanelId } from "./lib/panels/groupPanel";
  import UserPanel from "./lib/components/UserPanel.svelte";
  import TitleBar from "./lib/components/TitleBar.svelte";
  import ShortcutsHelp from "./lib/components/ShortcutsHelp.svelte";
  import ImageCropper from "./lib/components/ImageCropper.svelte";
  import { initPresence, myActivity } from "./lib/presence";
  import { trackPlayed } from "./lib/played";
  import { startAutoBackup } from "./lib/backup";
  import { refreshPeer, syncMyProfileOnConnect } from "./lib/people";
  import { chats } from "./lib/stores";
  import { initShortcuts } from "./lib/shortcuts";
  import { loadReactionPack } from "./lib/reactionPack";
  import UpdateBanner from "./lib/components/UpdateBanner.svelte";
  import { startUpdates } from "./lib/updates";
  import { initDesktop } from "./lib/desktop";

  // Entry: an account registered on this device goes straight in (after
  // the app passcode, if one is set) — no "Continue as" window. The
  // sign-in window shows only for a new device, or when the relay says
  // this account isn't linked to this device any more. A relay that is
  // down doesn't block the app: chats open, delivery waits (mesh.rs) and
  // the connection is retried in the background.
  let everConnected = $state(false);
  let autoEntry = false;

  // Phones: Back from Files / Contacts / Calls goes to Chats, as in Telegram.
  $effect(() => {
    const tab = $activeTab;
    if (tab === "chats" || !matchMedia("(max-width: 720px)").matches) return;
    return onBack(() => activeTab.set("chats"));
  });

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
    if (i.connected) { everConnected = true; if (!$wizardHold) closeRegistration(); return; }
    // Only on launch — a transient drop mid-session must not pop the login
    // window over the user's chat (the plug icon reconnects).
    if (everConnected || $registrationDismissed) return;
    if (i.registered && i.username && i.server_url) {
      if (autoEntry) return;
      autoEntry = true;
      const user = i.username, url = i.server_url;
      void connect(url, user).then((err) => {
        if (!err) return;
        // The account belongs elsewhere now: the window explains what to do.
        if (/different identity|already claimed|\b403\b|forbidden|not regist/i.test(err)) openRegistration();
        else void quickConnect(url, user);   // relay down: keep trying quietly
      });
      return;
    }
    openRegistration();
  });

  onMount(() => {
    initDesktop();
    // Notifications are on by default: ask the system once, on first start.
    try {
      if (!localStorage.getItem("aqua.notifyAsked")) {
        localStorage.setItem("aqua.notifyAsked", "1");
        void import("./lib/notify").then((n) => n.ensureNotifyPermission(true));
      }
    } catch { /* storage blocked */ }
    // "Server on this device" was on: bring the relay back with the app.
    try {
      const port = Number(localStorage.getItem("aqua.hostRelay") || 0);
      if (port) void import("@tauri-apps/api/core").then((c) => c.invoke("relay_host_start", { port })).catch((e) => console.warn("[aqua] relay host:", e));
    } catch { /* storage blocked */ }
    startPlanner();
    startWorkspace();
    startCapsules();
    if (!/android|iphone|ipad/i.test(navigator.userAgent)) startBluetooth();
    startMesh();
    startChannelStats();
    startHandoff();
    const stopIcon = startAppIcon();
    const stopUpdates = startUpdates();
    void migrateCustomWallpaper(api.saveWallpaper)
      .then(() => api.listWallpapers().then(followWallpaper).catch(() => {}))
      .then(() => setTimeout(() => void fitCurrentWallpaper(api.saveWallpaper, api.deleteWallpaper, api.readMediaBytes), 4000));
    // Linux: without gst-plugins-good WebKitGTK hangs on the first sound or
    // video — say what to install up front (playback is refused until then).
    // No usable GPU (gpu.rs switched WebKit to plain CPU painting): lighter
    // effects, and say why if it's the NVIDIA driver waiting for a reboot.
    void api.gpuStatus().then((g) => {
      if (g.dmabuf_pending) askGpu();
      if (!g.software) return;
      document.documentElement.classList.add("soft-render");
      if (g.reason === "nvidia-mismatch") setTimeout(() => showToast(tr("The NVIDIA driver was updated but the computer wasn't restarted, so the graphics card can't be used and the app draws on the CPU. Restart the computer."), 20000), 2500);
    }).catch(() => {});
    void mediaSupport().then((m) => {
      if (m.blocking.length) showToast(tr("Sound and video need GStreamer plugins. Install: {cmd}", { cmd: installHint([...m.blocking, ...m.recommended]) }), 20000);
    });
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
    startPower();
    maybeShowWhatsNew(get(settings).whatsNew !== false);
    // Message times in this device's time zone (Rust can't ask Android for it).
    void api.setTzOffset(-new Date().getTimezoneOffset()).catch(() => {});
    bootstrap();
    initPresence();
    trackPlayed();
    startAutoBackup();
    // A game session just ended: republish the card so the game collection
    // on the profile picks up the new play time.
    let wasPlaying = false;
    myActivity.subscribe((a) => {
      const now = a.some((x) => x.kind === "playing");
      if (wasPlaying && !now) void import("./lib/people").then((m) => m.publishMyProfile()).catch(() => {});
      wasPlaying = now;
    });
    void loadReactionPack();
    const stopShortcuts = initShortcuts();
    // Browser shortcuts that would pop Chromium chrome (print dialog,
    // save-page, find bar, reload, zoom, view-source, downloads…). We
    // run as a desktop app, so none of these should ever fire.
    const BLOCKED_CTRL = new Set(["p", "s", "f", "g", "r", "u", "j", "o", "+", "-", "=", "_", "0"]);
    const onKey = (e: KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey;
      // Our own shortcuts live in lib/shortcuts.ts (rebindable).
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
    const stopAutoscroll = startAutoscroll();
    const stopKeyboard = startKeyboardInset();
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
    // Phone Back button / back gesture: closes whatever is on top (lib/back.ts).
    const stopBack = startBack();
    const stopRenames = startRenames();
    const stopNetwork = startNetwork();
    document.addEventListener("keydown", onKey);
    document.addEventListener("contextmenu", onCtx);

    document.addEventListener("wheel", onWheel, { passive: false });
    document.addEventListener("play", onMediaPlay, true);
    window.addEventListener("drop", onDrop);
    window.addEventListener("dragover", onDragOver);
    return () => {
      stopIcon();
      stopUpdates();
      stopShortcuts();
      stopBack();
      stopRenames();
      stopNetwork();
      document.removeEventListener("keydown", onKey);
      document.removeEventListener("contextmenu", onCtx);
      stopAutoscroll();
      stopKeyboard();
      document.removeEventListener("wheel", onWheel);
      document.removeEventListener("play", onMediaPlay, true);
      window.removeEventListener("drop", onDrop);
      window.removeEventListener("dragover", onDragOver);
    };
  });
  // Direct GPU rendering was just switched on: "does it look right?" —
  // with no answer (a blank window can't be clicked) it switches back.
  let gpuAsk = $state(0);
  let gpuTimer: ReturnType<typeof setInterval> | null = null;
  function askGpu() {
    gpuAsk = 20;
    gpuTimer = setInterval(() => {
      gpuAsk -= 1;
      if (gpuAsk <= 0) { if (gpuTimer) clearInterval(gpuTimer); void gpuRevert(); }
    }, 1000);
  }
  async function gpuKeep() {
    if (gpuTimer) clearInterval(gpuTimer);
    gpuAsk = 0;
    try { await api.confirmDmabuf(); showToast(tr("Direct GPU rendering stays on")); } catch (e) { showToast(String(e)); }
  }
  async function gpuRevert() {
    if (gpuTimer) clearInterval(gpuTimer);
    gpuAsk = 0;
    try { await api.setDmabuf(false); await api.restartApp(); } catch (e) { showToast(String(e)); }
  }
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
<NewChatSheet />
<QrScanner />
<WhatsNew />
<CallModal />
<MediaPicker />
<Lazy when={$settingsOpen} load={() => import("./lib/components/SettingsPanel.svelte")} />
<ProfileModal />
<Lazy when={!!$editorState} load={() => import("./lib/components/PhotoEditor.svelte")} />
<Lazy when={!!$articleState} load={() => import("./lib/components/ArticleComposer.svelte")} />
<ArticleReader />
<CircleRecorder />
<PackManager />
<Lazy when={!!($stickerEditorState || $stickerQueue)} load={() => import("./lib/components/StickerEditor.svelte")} />
<RegistrationWizard />
<ChatGroupSheet />
<LockScreen />
<Lightbox />
<ForwardPicker />
<FolderManager />
<PollComposer />
<NewGroupWizard />
<Lazy when={!!$groupPanelId} load={() => import("./lib/components/GroupPanel.svelte")} />
<ShortcutsHelp />
<ImageCropper />
<PackSheet />
<MiniApp />
<PlannerComposer />
<DocEditor />
<HandoffBanner />
<CapsuleComposer />
<CapsuleAlerts />
<BluetoothSheet />
<BluetoothIncoming />
<ReminderPopup />
<ChatCalendar />
<UpdateBanner />
<Toast />
{#if gpuAsk > 0}
  <div class="gpu-ask" role="alertdialog" aria-live="assertive">
    <div class="gpu-card">
      <b>{tr("Direct GPU rendering is on")}</b>
      <p>{tr("Is everything shown correctly? Without an answer it switches back in {n} s.", { n: gpuAsk })}</p>
      <div class="gpu-btns">
        <button class="gpu-keep" onclick={gpuKeep}>{tr("Keep")}</button>
        <button class="gpu-back" onclick={gpuRevert}>{tr("Switch back")}</button>
      </div>
    </div>
  </div>
{/if}
<Confirm />
<SendDialog />

<style>
  .gpu-ask { position: fixed; inset: 0; z-index: 100000; display: grid; place-items: center; background: rgba(4, 6, 12, 0.55); }
  .gpu-card { background: var(--bg-shell); border: 1px solid var(--border-strong); border-radius: calc(16px * var(--rk, 1)); padding: 20px 22px; max-width: 380px; display: flex; flex-direction: column; gap: 8px; }
  .gpu-card p { margin: 0; color: var(--fg-muted); font-size: 14px; line-height: 1.45; }
  .gpu-btns { display: flex; gap: 8px; justify-content: flex-end; margin-top: 6px; }
  .gpu-btns button { border-radius: calc(10px * var(--rk, 1)); padding: 8px 14px; font-weight: 600; cursor: pointer; border: 1px solid var(--border-strong); background: transparent; color: var(--fg); }
  .gpu-btns .gpu-keep { background: linear-gradient(135deg, var(--accent), var(--accent-2)); color: #061015; border: none; }
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
