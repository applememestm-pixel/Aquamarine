<script module lang="ts">
  import { settingsOpen, settingsSection, openSettings, closeSettings } from "../panels/settings";
  export { settingsOpen, settingsSection, openSettings, closeSettings };
</script>

<script lang="ts">
  import Select from "./Select.svelte";
  import Donut from "./Donut.svelte";
  import { LINKS } from "../links";
  import { NOTES, openWhatsNew } from "./WhatsNew.svelte";
  import { AUTODL_DEFAULT, POWER_DEFAULT } from "../theme";
  import { powerActive, battery, powerMode } from "../power";
  import { autostartEnabled, setAutostart } from "../desktop";
  import { pickFolder } from "../dialog";
  const PHONE_UI = /android|iphone|ipad/i.test(navigator.userAgent);
  import { hasWidget } from "../widget";
  import { plugins, pluginsOn, installPlugin, removePlugin, togglePlugin, EXAMPLE_PLUGIN } from "../plugins";
  import { ATTACH_ITEMS, attachFront, attachHidden, attachDock, attachBots, toggleAttachFront, toggleAttachHidden, moveAttach, toggleAttachBot, resetAttachMenu } from "../attachMenu";
  import { chatLocks } from "../chatLocks";
  import { askSetLock, askRemoveLock, askChangeLock } from "../chatLockUi";
  let blockPick = $state("");
  async function testNotification() {
    const { ensureNotifyPermission, osNotify } = await import("../notify");
    if (!(await ensureNotifyPermission(true))) { showToast(tr("The system doesn't allow notifications for Aquamarine")); return; }
    await osNotify("Aquamarine", tr("Notifications work 🎉"));
  }
  let bioAvail = $state(false);
  void biometricAvailable().then((v) => (bioAvail = v));
  let lockPick = $state("");
  const blockable = $derived($chats.filter((c) => !c.id.startsWith("grp_") && !c.id.startsWith("__") && !c.channel && c.id !== "welcome" && !$blocked.has(c.id)));
  const lockable = $derived($chats.filter((c) => !c.id.startsWith("__") && !$chatLocks[c.id]));
  import UsernameEdit from "./UsernameEdit.svelte";
  import NetworkSettings from "./NetworkSettings.svelte";
  import ColorInput from "./ColorInput.svelte";
  import { onMount } from "svelte";
  import { fitWallpaper, followWallpaper } from "../theme";
  import SegFit from "./SegFit.svelte";
  import { BUILD, BUILD_SHORT, updatePrefs, updateState, checkUpdates } from "../updates";
  import { settings, PALETTES, WALLPAPERS, EDITABLE_COLORS, THEMES, DEFAULTS, MENU_ALPHA, type Mode, type Density } from "../theme";
  import { info } from "../stores";
  import { icons } from "../icons";
  import { openFileDialog } from "../dialog";
  import { DEFAULT_ICON, setAppIcon } from "../appIcon";
  import { cropImage } from "./ImageCropper.svelte";
  import { openStickerEditor, openStickerQueue, stickerLibraryTick } from "../panels/stickerEditor";
  import { openPackManager } from "./PackManager.svelte";
  import { askInput, closeInput } from "./InputPrompt.svelte";
  import AnimatedMedia from "./AnimatedMedia.svelte";
  import type { StickerPack } from "../types";
  import { ensureNotifyPermission } from "../notify";
  import { api, bytesToDataUrl, assetUrl, type DeviceInfo, type GpuStatus } from "../tauri";
  import { showToast } from "./Toast.svelte";
  import type { BotInfo } from "../types";
  import VoiceVideoSettings from "./VoiceVideoSettings.svelte";
  import { openProfile } from "./ProfileModal.svelte";
  import { openRegistration, registrationDismissed } from "./RegistrationWizard.svelte";
  import { shortcutsOpen } from "../shortcuts";
  import { profile } from "../profile";
  import { myOnline, shareActivity, myActivity, activityVerb } from "../presence";
  import { reactionPack, quickReactions, reactionUrl, parseReaction, importReactionPack, REACTION_PACK_LINK } from "../reactionPack";
  import PrivacyRules from "./PrivacyRules.svelte";
  import { ghost, blocked, setBlocked } from "../ghost";
  import { lockConf, setAutoLock, lockNow, passcodeSetupOpen, biometricAvailable, setBiometric } from "../lock";
  import { t, lang, LANGS, tr } from "../i18n";
  import { chats, activeId, loadActiveMessages } from "../stores";
  import { BUILTIN_SOUNDS, customSounds, refreshSounds, playSound, soundLabel, addSoundBytes } from "../sounds";
  import { exportThemeToFile, sendThemeToChat, pickAndApplyTheme } from "../themeShare";
  import { askConfirm } from "./Confirm.svelte";
  import { backupState, refreshBackup, backupNow } from "../backup";
  import { handoffOn } from "../handoff";
  import { save as saveDialog } from "@tauri-apps/plugin-dialog";
  import { meshStatus, meshSyncNow, meshSetEnabled, refreshMesh } from "../mesh";

  // ---- Section navigation ----
  type Sec = "network" | "nearby" | "host" | "ghost" | "language" | "appearance" | "theme" | "behaviour" | "notifications" | "voice" | "wallpaper" | "accent" | "bots" | "account" | "updates" | "about" | "privacy" | "stickers" | "backups" | "devices" | "storage" | "advanced" | "power" | "window";
  let section = $state<Sec>("notifications");
  // Telegram-mobile style: coloured icon tiles in grouped lists.
  interface Item { id: Sec | "profile" | "shortcuts"; label: string; color: string; icon: string; hint?: string }
  const ic = (p: string) => `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">${p}</svg>`;
  const GROUPS: Item[][] = [
    [
      { id: "profile", label: "Edit profile", color: "#007AFF", icon: ic(`<path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"/><circle cx="12" cy="7" r="4"/>`) },
      { id: "devices", label: "Devices", color: "#34C759", icon: ic(`<rect x="2" y="4" width="13" height="10" rx="2"/><line x1="5" y1="18" x2="12" y2="18"/><rect x="17" y="8" width="5" height="12" rx="1"/>`) },
      { id: "privacy", label: "Privacy & Security", color: "#8E8E93", icon: ic(`<rect x="3" y="11" width="18" height="11" rx="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/>`) },
      { id: "ghost", label: "Ghost mode", color: "#5E5CE6", icon: ic(`<path d="M9 10h.01"/><path d="M15 10h.01"/><path d="M12 2a8 8 0 0 0-8 8v12l3-3 2.5 2.5L12 19l2.5 2.5L17 19l3 3V10a8 8 0 0 0-8-8z"/>`) },
    ],
    [
      { id: "notifications", label: "Notifications", color: "#FF3B30", icon: ic(`<path d="M18 8A6 6 0 0 0 6 8c0 7-3 9-3 9h18s-3-2-3-9"/><path d="M13.73 21a2 2 0 0 1-3.46 0"/>`) },
      { id: "behaviour", label: "Chat settings", color: "#007AFF", icon: ic(`<path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/>`) },
      { id: "storage", label: "Data and storage", color: "#30D158", icon: ic(`<ellipse cx="12" cy="5" rx="8" ry="3"/><path d="M4 5v6c0 1.7 3.6 3 8 3s8-1.3 8-3V5"/><path d="M4 11v6c0 1.7 3.6 3 8 3s8-1.3 8-3v-6"/>`) },
      { id: "power", label: "Power saving", color: "#FFCC00", icon: ic(`<rect x="2" y="7" width="17" height="10" rx="2"/><line x1="22" y1="11" x2="22" y2="13"/><path d="M10 9l-2 3h3l-2 3"/>`) },
      { id: "stickers", label: "Stickers & Emoji", color: "#FF9500", icon: ic(`<circle cx="12" cy="12" r="10"/><path d="M8 14s1.5 2 4 2 4-2 4-2"/><line x1="9" y1="9" x2="9.01" y2="9"/><line x1="15" y1="9" x2="15.01" y2="9"/>`) },
      { id: "voice", label: "Voice & Video", color: "#34C759", icon: ic(`<rect x="9" y="2" width="6" height="12" rx="3"/><path d="M5 10v1a7 7 0 0 0 14 0v-1"/><line x1="12" y1="18" x2="12" y2="22"/>`) },
      { id: "language", label: "Language", color: "#AF52DE", icon: ic(`<circle cx="12" cy="12" r="10"/><line x1="2" y1="12" x2="22" y2="12"/><path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z"/>`) },
    ],
    [
      { id: "appearance", label: "Appearance", color: "#32ADE6", icon: ic(`<circle cx="12" cy="12" r="5"/><line x1="12" y1="1" x2="12" y2="3"/><line x1="12" y1="21" x2="12" y2="23"/><line x1="4.22" y1="4.22" x2="5.64" y2="5.64"/><line x1="18.36" y1="18.36" x2="19.78" y2="19.78"/><line x1="1" y1="12" x2="3" y2="12"/><line x1="21" y1="12" x2="23" y2="12"/>`) },
      { id: "window", label: "Window & system", color: "#64D2FF", icon: ic(`<rect x="3" y="4" width="18" height="16" rx="2"/><line x1="3" y1="9" x2="21" y2="9"/><circle cx="6.5" cy="6.5" r=".5"/>`) },
    ],
    [
      { id: "account", label: "Account & server", color: "#007AFF", icon: ic(`<rect x="2" y="2" width="20" height="8" rx="2"/><rect x="2" y="14" width="20" height="8" rx="2"/><line x1="6" y1="6" x2="6.01" y2="6"/><line x1="6" y1="18" x2="6.01" y2="18"/>`) },
      { id: "network", label: "Network & proxy", color: "#5856D6", icon: ic(`<circle cx="12" cy="12" r="10"/><path d="M2 12h20"/><path d="M12 2a15 15 0 0 1 0 20"/><path d="M12 2a15 15 0 0 0 0 20"/>`) },
      { id: "host", label: "Server on this device", color: "#00C7BE", icon: ic(`<rect x="3" y="3" width="18" height="7" rx="2"/><rect x="3" y="14" width="18" height="7" rx="2"/><line x1="7" y1="6.5" x2="7.01" y2="6.5"/><line x1="7" y1="17.5" x2="7.01" y2="17.5"/><path d="M14 6.5h3M14 17.5h3"/>`) },
      { id: "nearby", label: "Without internet", color: "#0A84FF", icon: ic(`<path d="M7 7l10 10-5 5V2l5 5L7 17"/>`) },
      { id: "backups", label: "Backups", color: "#30B0C7", icon: ic(`<path d="M12 2a10 10 0 1 0 10 10"/><path d="M12 6v6l4 2"/><path d="M22 2v6h-6"/>`) },
      { id: "bots", label: "Bots", color: "#00C7BE", icon: ic(`<rect x="4" y="8" width="16" height="12" rx="2"/><path d="M12 8V4"/><circle cx="12" cy="3" r="1"/><line x1="9" y1="13" x2="9.01" y2="13"/><line x1="15" y1="13" x2="15.01" y2="13"/>`) },
    ],
    [
      { id: "shortcuts", label: "Keyboard shortcuts", color: "#5AC8FA", icon: ic(`<rect x="2" y="6" width="20" height="12" rx="2"/><line x1="6" y1="10" x2="6.01" y2="10"/><line x1="10" y1="10" x2="10.01" y2="10"/><line x1="14" y1="10" x2="14.01" y2="10"/><line x1="18" y1="10" x2="18.01" y2="10"/><line x1="7" y1="14" x2="17" y2="14"/>`), hint: "Ctrl+/" },
      { id: "updates", label: "Updates", color: "#34C759", icon: ic(`<path d="M12 3v12"/><path d="m7 10 5 5 5-5"/><path d="M5 21h14"/>`) },
      { id: "advanced", label: "Experimental", color: "#636366", icon: ic(`<line x1="4" y1="21" x2="4" y2="14"/><line x1="4" y1="10" x2="4" y2="3"/><line x1="12" y1="21" x2="12" y2="12"/><line x1="12" y1="8" x2="12" y2="3"/><line x1="20" y1="21" x2="20" y2="16"/><line x1="20" y1="12" x2="20" y2="3"/><line x1="1" y1="14" x2="7" y2="14"/><line x1="9" y1="8" x2="15" y2="8"/><line x1="17" y1="16" x2="23" y2="16"/>`) },
      { id: "about", label: "About Aquamarine", color: "#8E8E93", icon: ic(`<circle cx="12" cy="12" r="10"/><line x1="12" y1="16" x2="12" y2="12"/><line x1="12" y1="8" x2="12.01" y2="8"/>`) },
    ],
  ];
  const SECTIONS = GROUPS.flat().filter((x) => x.id !== "profile" && x.id !== "shortcuts") as (Item & { id: Sec })[];
  const sectionItem = $derived(GROUPS.flat().find((x) => x.id === section));
  const sectionTitle = $derived(sectionItem?.label ?? "Settings");

  // ---- Phone layout (<= 720px): Telegram-style list of tiles; tapping one
  //      opens that section full-screen with  ‹ [icon] Title  on top. The
  //      system Back button steps back out (section -> list -> closed).
  const isPhone = () => typeof window !== "undefined" && window.matchMedia("(max-width: 720px)").matches;
  let detail = $state(false);
  // Phones have no keyboard shortcuts — hide that row there.
  let phone = $state(isPhone());
  /** Settings → Experimental → Plugins: code being pasted (null: closed). */
  let pluginPaste = $state<string | null>(null);
  onMount(() => {
    const mq = window.matchMedia("(max-width: 720px)");
    const on = () => { phone = mq.matches; };
    mq.addEventListener("change", on);
    return () => mq.removeEventListener("change", on);
  });
  const TOUCH = typeof navigator !== "undefined" && /android|iphone|ipad/i.test(navigator.userAgent);
  function enterDetail() {
    if (!isPhone() || detail) return;
    detail = true;
    try { history.pushState({ aquaSettings: "detail" }, ""); } catch { /* ignore */ }
  }
  function backToList() {
    if (history.state?.aquaSettings === "detail") history.back(); // popstate handler clears `detail`
    else detail = false;
  }
  function closeAll() {
    const depth = history.state?.aquaSettings === "detail" ? 2 : history.state?.aquaSettings === "list" ? 1 : 0;
    detail = false;
    closeSettings();
    if (depth) { try { history.go(-depth); } catch { /* ignore */ } }
  }
  $effect(() => {
    if (!$settingsOpen) { detail = false; return; }
    if (isPhone() && !history.state?.aquaSettings) {
      try { history.pushState({ aquaSettings: "list" }, ""); } catch { /* ignore */ }
    }
  });
  // Mark the body so App's chat Back handler leaves the chat alone while
  // settings are on top (set/cleared in an effect, i.e. after this event).
  $effect(() => {
    const on = $settingsOpen;
    document.body.classList.toggle("settings-open", on);
    return () => document.body.classList.remove("settings-open");
  });
  onMount(() => {
    const onPop = () => {
      if (!$settingsOpen) return;
      const st = history.state?.aquaSettings;
      if (st === "list") detail = false;
      else if (!st) { detail = false; closeSettings(); }
    };
    window.addEventListener("popstate", onPop);
    return () => window.removeEventListener("popstate", onPop);
  });

  function openItem(it: Item) {
    if (it.id === "profile") { closeAll(); openProfile(); return; }
    if (it.id === "shortcuts") { closeAll(); shortcutsOpen.set(true); return; }
    section = it.id;
    enterDetail();
  }

  // ---- Privacy ----
  async function copyFp() {
    try { await navigator.clipboard.writeText($info?.session_fingerprint ?? ""); showToast(tr("Fingerprint copied")); }
    catch { showToast(tr("Copy blocked")); }
  }
  async function logOut() {
    try {
      info.set(await api.logOut());
      registrationDismissed.set(false);
      closeSettings();
      openRegistration();
      showToast(tr("Logged out"));
    } catch (e) { showToast(`Logout failed: ${e}`); }
  }

  // ---- Stickers & Emoji: the reaction pack ----
  let rpToken = $state("");
  let rpBusy = $state(false);
  try { rpToken = localStorage.getItem("aqua.tgBotToken") ?? ""; } catch { /* ignore */ }
  async function importRp() {
    if (!rpToken.trim()) { showToast(tr("Paste a Telegram bot token (from @BotFather) first")); return; }
    rpBusy = true;
    try { const n = await importReactionPack(rpToken.trim()); showToast(`Imported ${n} reactions`); }
    catch (e) { showToast(`Import failed: ${e}`, 5000); }
    finally { rpBusy = false; }
  }

  $effect(() => {
    const want = $settingsSection;
    // The look-related pages were merged into Appearance: jump to their part.
    const sub = want === "wallpaper" || want === "accent" || want === "theme" ? want : null;
    if (sub || (want && SECTIONS.some((x) => x.id === want))) {
      section = (sub ? "appearance" : want) as Sec;
      settingsSection.set(null);
      queueMicrotask(enterDetail);
      if (sub) setTimeout(() => document.getElementById(`ap-${sub}`)?.scrollIntoView({ block: "start", behavior: "smooth" }), 60);
    }
  });

  // ---- Theme editor ----
  // Effective hex for a slot: the user's override, else the mode default.
  function effMode(): "dark" | "light" {
    if ($settings.mode === "auto") {
      return window.matchMedia?.("(prefers-color-scheme: light)").matches ? "light" : "dark";
    }
    return $settings.mode;
  }
  function colorValue(key: string, dark: string, light: string): string {
    return $settings.customColors?.[key] ?? (effMode() === "light" ? light : dark);
  }
  function setColor(key: string, value: string) {
    settings.update((s) => ({ ...s, customColors: { ...(s.customColors ?? {}), [key]: value } }));
  }
  function clearColors() {
    settings.update((s) => ({ ...s, customColors: {} }));
    showToast(tr("Custom colours cleared"));
  }
  const COLOR_GROUPS = ["Accent", "Surfaces", "Text"] as const;

  async function pickCustomFont() {
    try {
      const result = await openFileDialog({
        multiple: false, directory: false, title: "Pick a font file",
        filters: [{ name: "Fonts", extensions: ["ttf", "otf", "woff", "woff2"] }],
      });
      const path = Array.isArray(result) ? (result[0] ?? null) : result;
      if (!path) return;
      const bytes = await api.readUserFont(path);
      const lower = path.toLowerCase();
      const mime = lower.endsWith(".woff2") ? "font/woff2"
        : lower.endsWith(".woff") ? "font/woff"
        : lower.endsWith(".otf") ? "font/otf"
        : "font/ttf";
      const url = bytesToDataUrl(bytes, mime);
      const name = path.split(/[\\/]/).pop() ?? "Custom font";
      settings.update((s) => ({ ...s, customFontName: name, customFontData: url }));
      showToast(`Font set: ${name}`);
    } catch (e) {
      showToast(`Could not load font: ${e}`);
    }
  }
  function clearCustomFont() {
    settings.update((s) => ({ ...s, customFontName: undefined, customFontData: undefined }));
    showToast(tr("Font reset to default"));
  }

  // ---- Backups ----
  let bkBusy = $state(false);
  let bkPw = $state("");
  let bkPhrase = $state<string | null>(null);
  $effect(() => { if ($settingsOpen && section === "backups") { void refreshBackup(); void loadRecovery(); } });
  /** Account recovery key: known on this device / registered on the relay. */
  let rc = $state<{ local: boolean; server: boolean | null } | null>(null);
  async function loadRecovery() { try { rc = await api.recoveryStatus(); } catch { rc = null; } }
  async function rcShow() {
    const ok = await askConfirm({ title: tr("Show the recovery key?"), text: tr("Anyone who sees these 24 words and gets into your e-mail can restore your account. Make sure nobody is looking at your screen."), ok: tr("Show") });
    if (!ok) return;
    if (!rc?.local && rc?.server) {
      const again = await askConfirm({ title: tr("Make a new recovery key?"), text: tr("This device doesn't know the account's current key. A new one replaces it: the old words stop working, and the backup is sealed for the new ones."), ok: tr("Make a new one") });
      if (!again) return;
    }
    try { bkPhrase = await api.recoveryCreate(); await loadRecovery(); }
    catch (e) {
      // Offline: still show the words this device has.
      try { bkPhrase = await api.backupPhrase(); } catch { /* ignore */ }
      showToast(tr("The key isn't registered on the server yet: {e}", { e: String(e) }));
    }
  }

  // ---- Devices: every device has its own keys; all of them get every message.
  let devices = $state<DeviceInfo[] | null>(null);
  let devErr = $state<string | null>(null);
  async function loadDevices() {
    devErr = null;
    try { devices = await api.listDevices(); }
    catch (e) { devices = null; devErr = String(e); }
  }
  $effect(() => { if ($settingsOpen && section === "devices") void loadDevices(); });
  // ---- Server on this device: the relay inside the app ----
  type HostStatus = { running: boolean; port: number; lan_url: string | null; public_url: string | null; admin_url: string | null; dir: string; dir_chosen?: boolean; error: string | null; background: boolean };
  let host = $state<HostStatus | null>(null);
  let hostBusy = $state(false);
  let hostPort = $state(7777);
  const invoke = async <T,>(cmd: string, args?: Record<string, unknown>): Promise<T> => (await import("@tauri-apps/api/core")).invoke<T>(cmd, args);
  async function hostRefresh() { try { host = await invoke<HostStatus>("relay_host_status"); } catch (e) { host = null; } }
  async function hostToggle() {
    hostBusy = true;
    try {
      host = await invoke<HostStatus>(host?.running ? "relay_host_stop" : "relay_host_start", host?.running ? {} : { port: hostPort });
      try { localStorage.setItem("aqua.hostRelay", host.running ? String(host.port) : ""); } catch { /* ignore */ }
    } catch (e) { showToast(String(e)); await hostRefresh(); }
    hostBusy = false;
  }

  // ---- Data and storage (Telegram's "Download path" + "Local storage") ----
  type StConf = { mode: "default" | "temp" | "custom"; custom: string; total_mb: number; cache_mb: number; max_days: number };
  type StInfo = { conf: StConf; downloads: string; default_dir: string; temp_dir: string };
  type StStats = { images: number; videos: number; audio: number; stickers: number; files: number; cache: number; count: number };
  let st = $state<StInfo | null>(null);
  let stStats = $state<StStats | null>(null);
  let stPick = $state<Record<string, boolean>>({ images: true, videos: true, audio: true, stickers: true, files: true, cache: true });
  let stBusy = $state(false);
  type TRow = { chat: string; name: string; bytes: number; count: number };
  type Traffic = { received: Record<string, TRow[]>; sent: Record<string, TRow[]> };
  let stTx = $state<Traffic | null>(null);
  let stOpen = $state<{ side: "rx" | "tx"; kind: string } | null>(null);
  const sumOf = (rows: TRow[] | undefined) => (rows ?? []).reduce((n, r) => n + r.bytes, 0);
  const stTxTotal = $derived(stTx ? Object.values(stTx.sent).reduce((n, rows) => n + sumOf(rows), 0) : 0);
  async function stLoad() {
    try { st = await invoke<StInfo>("storage_get"); stStats = await invoke<StStats>("storage_stats"); stTx = await invoke<Traffic>("storage_traffic"); } catch (e) { console.warn(e); }
  }
  async function stSave(patch: Partial<StConf>) {
    if (!st) return;
    try { st = await invoke<StInfo>("storage_set", { conf: { ...st.conf, ...patch } }); await invoke("storage_trim"); stStats = await invoke<StStats>("storage_stats"); }
    catch (e) { showToast(String(e)); }
  }
  async function stPickFolder() {
    const p = await pickFolder(tr("Folder for downloaded files"));
    if (p) await stSave({ mode: "custom", custom: p });
  }
  async function stClear() {
    const kinds = Object.keys(stPick).filter((k) => stPick[k]);
    if (!kinds.length) return;
    const ok = await askConfirm({ title: tr("Clear {size}?", { size: fmtBytes(stSelected) }), text: tr("The files are deleted from this device only. In chats they can be downloaded again while the server keeps them."), ok: tr("Clear"), danger: true });
    if (!ok) return;
    stBusy = true;
    try { const n = await invoke<number>("storage_clear", { kinds }); showToast(tr("Freed {size}", { size: fmtBytes(n) })); await stLoad(); }
    catch (e) { showToast(String(e)); }
    stBusy = false;
  }
  const ST_KINDS = [
    { id: "images", label: "Images", color: "#4FA3FF" },
    { id: "videos", label: "Videos", color: "#B66BFF" },
    { id: "audio", label: "Music & voice", color: "#3DDC97" },
    { id: "stickers", label: "Stickers & GIFs", color: "#FF9F43" },
    { id: "files", label: "Files", color: "#FF6B8B" },
    { id: "cache", label: "Media cache", color: "#8E8E93" },
  ] as const;
  const stTotal = $derived(stStats ? ST_KINDS.reduce((n, k) => n + stStats![k.id], 0) : 0);
  const stSelected = $derived(stStats ? ST_KINDS.reduce((n, k) => n + (stPick[k.id] ? stStats![k.id] : 0), 0) : 0);
  /** Donut segments: [offset, length, colour] on a circle of length 100. */
  const stArcs = $derived.by(() => {
    if (!stStats || !stTotal) return [] as { off: number; len: number; color: string }[];
    let off = 0;
    return ST_KINDS.filter((k) => stStats![k.id] > 0).map((k) => { const len = (stStats![k.id] / stTotal) * 100; const a = { off, len, color: k.color }; off += len; return a; });
  });
  function fmtBytes(n: number): string {
    if (n < 1024) return `${n} B`;
    if (n < 1048576) return `${(n / 1024).toFixed(1)} KB`;
    if (n < 1073741824) return `${(n / 1048576).toFixed(1)} MB`;
    return `${(n / 1073741824).toFixed(2)} GB`;
  }
  $effect(() => { if ($settingsOpen && section === "storage") void stLoad(); });
  async function exportChats() {
    try {
      const { save } = await import("@tauri-apps/plugin-dialog");
      const dest = await save({ defaultPath: `aquamarine-${$info?.username ?? "chats"}-${new Date().toISOString().slice(0, 10)}.json`, filters: [{ name: "JSON", extensions: ["json"] }] });
      if (!dest) return;
      const n = await invoke<number>("export_chats", { dest });
      showToast(tr("Exported: {size}", { size: fmtBytes(n) }));
    } catch (e) { showToast(String(e)); }
  }
  function setAutoDl(k: "private" | "groups" | "channels" | "limitMb", v: boolean | number) {
    settings.update((x) => ({ ...x, autoDl: { ...AUTODL_DEFAULT, ...(x.autoDl ?? {}), [k]: v } }));
  }
  const pw = $derived({ ...POWER_DEFAULT, ...($settings.power ?? {}) });
  const pwMode = $derived(powerMode($settings));
  function setPower(k: keyof typeof POWER_DEFAULT, v: boolean) {
    settings.update((x) => ({ ...x, power: { ...POWER_DEFAULT, ...(x.power ?? {}), [k]: v } }));
  }
  // ---- Advanced: launch at start-up ----
  let autostart = $state(false);
  $effect(() => { if ($settingsOpen && section === "window") void autostartEnabled().then((v) => (autostart = v)); });
  async function toggleAutostart() {
    try { autostart = await setAutostart(!autostart); } catch (e) { showToast(String(e)); }
  }

  /** The server's data folder: chosen on the first start, changeable while it's off. */
  async function hostSetDir(path: string | null, copy = true) {
    if (path === null) return;
    try { host = await invoke<HostStatus>("relay_host_set_dir", { path, copy }); showToast(tr("Server data folder: {p}", { p: host.dir })); }
    catch (e) { showToast(String(e)); }
  }
  async function hostPickDir() {
    const p = await pickFolder(tr("Folder for the server's data"));
    if (p) await hostSetDir(p.replace(/[\\/]+$/, "") + (/[\\/]Aquamarine Server$/i.test(p) ? "" : (p.includes("\\") ? "\\" : "/") + "Aquamarine Server"));
  }
  async function hostCopy(u: string | null) {
    if (!u) return;
    try { await navigator.clipboard.writeText(u); showToast(tr("Copied")); } catch { showToast(u); }
  }
  $effect(() => {
    if (!($settingsOpen && section === "host")) return;
    void hostRefresh();
    const id = setInterval(hostRefresh, 5000);
    return () => clearInterval(id);
  });
  let meshBusy = $state(false);
  $effect(() => { if ($settingsOpen && section === "nearby") void refreshMesh(); });
  async function devRemove(d: DeviceInfo) {
    const ok = await askConfirm({
      title: tr("Sign out “{n}”?", { n: d.name || (d.primary ? tr("Main device") : d.id) }),
      text: d.primary ? tr("This device becomes the main one. The other device loses access to the account.") : tr("The device loses access to the account and stops getting messages."),
      ok: tr("Sign out"), danger: true,
    });
    if (!ok) return;
    try { await api.removeDevice(d.id); showToast(tr("Device signed out")); }
    catch (e) { showToast(String(e)); }
    await loadDevices();
  }
  let devEdit = $state<string | null>(null);
  let devName = $state("");
  async function devRename(d: DeviceInfo) {
    const name = devName.trim();
    devEdit = null;
    if (!name || name === d.name) return;
    try { await api.renameDevice(d.id, name); } catch (e) { showToast(String(e)); }
    await loadDevices();
  }
  function devAdded(sec: number): string {
    return sec ? new Date(sec * 1000).toLocaleDateString() : "";
  }
  function fmtWhen(sec: number): string {
    const d = new Date(sec * 1000);
    return d.toLocaleString();
  }
  async function bkEnable() {
    if (bkPw.length < 8) { showToast(tr("Password must be at least 8 characters")); return; }
    bkBusy = true;
    try {
      bkPhrase = await api.backupEnable(bkPw);
      bkPw = "";
      await refreshBackup();
      try { await backupNow(true); } catch (e) { showToast(tr("Backups are on; the first upload failed: {e}", { e: String(e) })); }
    } catch (e) { showToast(String(e)); }
    finally { bkBusy = false; }
  }
  async function bkNow() {
    bkBusy = true;
    try { await backupNow(true); showToast(tr("Backup uploaded")); }
    catch (e) { showToast(tr("Backup failed: {e}", { e: String(e) })); }
    finally { bkBusy = false; }
  }
  async function bkDisable() {
    const ok = await askConfirm({ title: tr("Turn off backups?"), text: tr("The encrypted copy on the server is deleted. Chats stay on this device."), ok: tr("Turn off"), danger: true });
    if (!ok) return;
    try { await api.backupDisable(); bkPhrase = null; await refreshBackup(); } catch (e) { showToast(String(e)); }
  }

  // ---- Bots ----
  let bots = $state<BotInfo[]>([]);
  let botUsername = $state("");
  let botDisplay = $state("");
  let botBusy = $state(false);
  let botError = $state<string | null>(null);

  // ---- Outbound proxy ("connect through a server") ----
  let proxyUrl = $state($info?.proxy ?? "");
  let proxyBusy = $state(false);
  let proxyNote = $state<string | null>(null);
  $effect(() => { proxyUrl = $info?.proxy ?? ""; });
  async function saveProxy() {
    proxyBusy = true; proxyNote = null;
    try {
      info.set(await api.setNetworkProxy(proxyUrl.trim()));
      proxyNote = (proxyUrl.trim() ? tr("Saved — applies on the next connect.") : tr("Direct connection — proxy off."));
    } catch (e) {
      proxyNote = tr("Could not set proxy: {e}", { e: String(e) });
    } finally { proxyBusy = false; }
  }

  async function loadBots() {
    if (!$info?.connected) return;
    try { bots = await api.listBots(); } catch { bots = []; }
  }

  async function createBot() {
    botError = null;
    const u = botUsername.trim().toLowerCase().replace(/^@/, "");
    if (!/^[a-z0-9_.]{3,32}$/.test(u)) { botError = tr("Username: 3–32 chars, a-z 0-9 _ ."); return; }
    botBusy = true;
    try {
      const made = await api.createBot(u, botDisplay.trim() || u);
      botUsername = ""; botDisplay = "";
      await loadBots();
      botShown = new Set([...botShown, made.username]);
      showToast(tr("Bot @{u} created", { u }));
    } catch (e) {
      botError = String(e);
    } finally {
      botBusy = false;
    }
  }

  // Tokens are masked until you ask to see them.
  let botShown = $state(new Set<string>());
  function toggleToken(u: string) {
    const n = new Set(botShown);
    if (n.has(u)) n.delete(u); else n.add(u);
    botShown = n;
  }
  function maskToken(tok: string, hint = "") { return tok ? tok.slice(0, 9) + "•".repeat(14) + tok.slice(-4) : "aqbot_" + "•".repeat(17) + hint; }

  async function copyText(text: string, done = tr("Copied")) {
    try { await navigator.clipboard.writeText(text); showToast(done); }
    catch { showToast(tr("Copy blocked")); }
  }
  async function copyToken(token: string) { await copyText(token, tr("Token copied")); }
  /** Telegram libraries want "<digits>:<secret>": the relay's numeric id of
   *  the bot (first 6 bytes of SHA-256 of its username) in front of the token. */
  async function tgToken(username: string, token: string): Promise<string> {
    const h = new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(username)));
    let id = 0;
    for (let i = 0; i < 6; i++) id = id * 256 + h[i];
    return `${Math.max(1, id)}:${token}`;
  }
  async function copyTgToken(username: string, token: string) { await copyText(await tgToken(username, token), tr("Telegram-style token copied")); }
  const tgBase = $derived(($info?.server_url || "http://127.0.0.1:7777").replace(/\/+$/, ""));

  async function resetBotToken(b: BotInfo) {
    const ok = await askConfirm({
      title: tr("Reset the token of @{u}?", { u: b.username }),
      text: tr("The old token stops working at once — programs using it must get the new one."),
      ok: tr("Reset token"),
      danger: true,
    });
    if (!ok) return;
    try {
      await api.createBot(b.username, b.display_name || b.username);
      await loadBots();
      botShown = new Set([...botShown, b.username]);
      showToast(tr("New token issued"));
    } catch (e) { showToast(String(e)); }
  }

  async function deleteBot(b: BotInfo) {
    const ok = await askConfirm({
      title: tr("Delete @{u}?", { u: b.username }),
      text: tr("The bot disappears from the directory and its token stops working. The username becomes free."),
      ok: tr("Delete"),
      danger: true,
    });
    if (!ok) return;
    try {
      await api.deleteBot(b.username);
      bots = bots.filter((x) => x.username !== b.username);
      showToast(tr("Bot deleted"));
    } catch (e) { showToast(String(e)); }
  }

  async function openBotChat(b: BotInfo) {
    try {
      await api.startBotChat(b.username, "/start");
      const { chats, selectChat, activeTab } = await import("../stores");
      chats.set(await api.listChats());
      activeTab.set("chats");
      await selectChat(b.username);
      closeSettings();
    } catch (e) { showToast(tr("Could not start bot: {e}", { e: String(e) })); }
  }

  let consoleCmd = $derived(`python3 aqua-console.py --server ${$info?.server_url ?? "http://127.0.0.1:8080"}`);
  async function saveConsoleScript() {
    try {
      const path = await saveDialog({ title: tr("Save the console bot"), defaultPath: "aqua-console.py", filters: [{ name: "Python", extensions: ["py"] }] });
      if (!path) return;
      await api.saveConsoleBot(path);
      showToast(tr("Saved — run it with: {c}", { c: "python3 " + path.split(/[\\/]/).pop() }));
    } catch (e) { showToast(String(e)); }
  }
  let apiOpen = $state(false);

  // Load bots whenever the panel opens.
  $effect(() => {
    if ($settingsOpen) void loadBots();
  });

  function setMode(m: Mode) { settings.update((s) => ({ ...s, mode: m })); }
  function setPalette(id: string) { settings.update((s) => ({ ...s, palette: id })); }
  function setDensity(d: Density) { settings.update((s) => ({ ...s, density: d })); }
  function setFontScale(v: number) { settings.update((s) => ({ ...s, fontScale: Math.max(0.85, Math.min(1.4, v)) })); }
  let uiScalePreview = $state<number | null>(null);
  function setUiScale(v: number) { settings.update((s) => ({ ...s, uiScale: Math.max(0.8, Math.min(1.3, v)) })); }
  // ---- your stickers ----
  let myPacks = $state<StickerPack[]>([]);
  /** Packs imported from Telegram: read-only, kept up to date from Telegram. */
  let tgPacks = $state<StickerPack[]>([]);
  let tgBusy = $state<string | null>(null);
  async function syncTg(p: StickerPack) {
    tgBusy = p.title;
    try {
      const r = await api.syncTelegramPack(p.title);
      showToast(r.changed ? tr("“{t}” updated: +{a} −{r}", { t: p.title, a: r.added, r: r.removed }) : tr("“{t}” is up to date", { t: p.title }));
      stickerLibraryTick.update((n) => n + 1);
    } catch (e) { showToast(String(e)); }
    finally { tgBusy = null; }
  }
  async function deleteTgPack(p: StickerPack) {
    if (!(await askConfirm({ title: tr("Delete pack?"), text: tr("“{pack}” and all its stickers are removed from this device.", { pack: p.title }), ok: tr("Delete"), danger: true }))) return;
    try { await api.deleteStickerPack(p.title); stickerLibraryTick.update((n) => n + 1); } catch (e) { showToast(String(e)); }
  }
  let openPack = $state<string | null>(null);
  let renaming = $state<string | null>(null);
  let renameTo = $state("");
  async function loadMyPacks() {
    try {
      const lib = await api.listLocalMedia();
      myPacks = lib.packs.filter((p) => !p.source && p.editable !== false);
      tgPacks = lib.packs.filter((p) => !!p.source);
    } catch { myPacks = []; tgPacks = []; }
  }
  $effect(() => { void $stickerLibraryTick; if (section === "stickers") void loadMyPacks(); });
  async function newSticker(pack?: string) {
    try {
      const result = await openFileDialog({ multiple: false, directory: false, title: tr("Pick a picture for the sticker"),
        filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp", "gif"] }] });
      const path = Array.isArray(result) ? (result[0] ?? null) : result;
      if (!path) return;
      const bytes = await api.readUserImage(path);
      const low = path.toLowerCase();
      const mime = low.endsWith(".png") ? "image/png" : low.endsWith(".webp") ? "image/webp" : low.endsWith(".gif") ? "image/gif" : "image/jpeg";
      openStickerEditor(bytes, mime, path.split(/[\\/]/).pop() ?? "sticker", pack);
    } catch (e) { showToast(`${e}`); }
  }
  /** A whole pack: name it, pick the pictures, the editor walks through them. */
  async function pickMany(title: string): Promise<string[]> {
    const result = await openFileDialog({ multiple: true, directory: false, title, filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp", "gif"] }] });
    return Array.isArray(result) ? result : result ? [result] : [];
  }
  function newPackFlow() {
    askInput({
      title: tr("New sticker pack"),
      description: tr("Then pick the pictures — several at once. The editor opens each in turn: cut out, caption, emoji."),
      fields: [{ label: tr("Pack name"), placeholder: tr("My stickers") }],
      submitLabel: tr("Pick pictures"),
      onSubmit: async ([name]) => {
        const n = name.trim();
        if (!n) return tr("Give the pack a name");
        if (myPacks.some((p) => p.title === n)) return tr("A pack with this name already exists");
        closeInput();
        const paths = await pickMany(tr("Pictures for “{p}”", { p: n }));
        if (paths.length) await openStickerQueue(paths, n);
        return null;
      },
    });
  }
  async function addManyTo(pack: string) {
    try { const paths = await pickMany(tr("Pictures for “{p}”", { p: pack })); if (paths.length) await openStickerQueue(paths, pack); }
    catch (e) { showToast(`${e}`); }
  }
  async function deleteMySticker(path: string) {
    try { await api.deleteMediaItem(path); await loadMyPacks(); } catch (e) { showToast(`${e}`); }
  }
  async function deleteMyPack(p: StickerPack) {
    const ok = await askConfirm({ title: tr("Delete the pack “{p}”?", { p: p.title }), text: tr("All {n} stickers in it are deleted from this device.", { n: p.items.length }), ok: tr("Delete"), danger: true });
    if (!ok) return;
    try { await api.deleteStickerPack(p.title); await loadMyPacks(); } catch (e) { showToast(`${e}`); }
  }
  async function renameMyPack(p: StickerPack) {
    const n = renameTo.trim();
    renaming = null;
    if (!n || n === p.title) return;
    try { await api.renameStickerPack(p.title, n); if (openPack === p.title) openPack = n; await loadMyPacks(); } catch (e) { showToast(`${e}`); }
  }

  // ---- app icon ----
  let iconMode = $derived($settings.appIcon?.mode === "custom" ? "custom" : "default");
  async function pickCustomIcon() {
    try {
      const result = await openFileDialog({ multiple: false, directory: false, title: tr("Pick an icon"), filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp"] }] });
      const path = Array.isArray(result) ? (result[0] ?? null) : result;
      if (!path) return;
      const raw = await api.readUserImage(path);
      const src = URL.createObjectURL(new Blob([new Uint8Array(raw)]));
      const blob = await cropImage(src, { aspect: 1, outWidth: 256, title: tr("Position the icon") });
      URL.revokeObjectURL(src);
      if (!blob) return;
      const url = bytesToDataUrl(Array.from(new Uint8Array(await blob.arrayBuffer())), blob.type || "image/webp");
      setAppIcon({ mode: "custom", custom: url });
      showToast(tr("App icon changed"));
    } catch (e) {
      showToast(`${e}`);
    }
  }

  function setWallpaper(id: string) { settings.update((s) => ({ ...s, wallpaper: id })); }

  // ---- graphics mode (gpu.rs) ----
  let gpu = $state<GpuStatus | null>(null);
  const isLinuxWk = typeof document !== "undefined" && document.documentElement.classList.contains("wk-lowfx");
  async function toggleDmabuf() {
    if (!gpu) return;
    const on = !gpu.dmabuf;
    const ok = await askConfirm({
      title: on ? tr("Turn on direct GPU rendering?") : tr("Turn off direct GPU rendering?"),
      text: on ? tr("Frames go from the graphics card straight to the screen — much smoother on Linux. On some NVIDIA + Wayland setups the window stays blank: then do nothing, and in 20 seconds Aquamarine switches back by itself.") : tr("Aquamarine restarts in the safe mode."),
      ok: tr("Restart"),
    });
    if (!ok) return;
    try { await api.setDmabuf(on); await api.restartApp(); } catch (e) { showToast(String(e)); }
  }
  $effect(() => { if (section === "behaviour" && !gpu) void api.gpuStatus().then((g) => (gpu = g)).catch(() => {}); });
  async function setGpu(mode: "auto" | "on" | "off") {
    try { await api.setGpuMode(mode); if (gpu) gpu = { ...gpu, mode }; showToast(tr("Restart Aquamarine to apply")); }
    catch (e) { showToast(String(e)); }
  }

  // ---- wallpaper library (files, not data: URLs) ----
  let myWalls = $state<string[]>([]);
  async function loadWalls() {
    try { myWalls = await api.listWallpapers(); } catch { myWalls = []; }
    // The library may have renamed an extension-less file (Android gallery
    // picks): follow it, so the wallpaper in use keeps showing.
    followWallpaper(myWalls);
  }
  $effect(() => { if (section === "wallpaper") void loadWalls(); });
  async function pickCustomWallpaper() {
    try {
      const result = await openFileDialog({
        multiple: true, directory: false, title: tr("Add wallpapers"),
        filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp", "gif", "avif"] }],
      });
      const paths = Array.isArray(result) ? result : result ? [result] : [];
      if (!paths.length) return;
      let last = "";
      for (const p of paths) {
        const fit = await fitWallpaper(await api.readUserImage(p), p.split(/[\\/]/).pop() ?? "wallpaper.jpg");
        last = await api.saveWallpaper(fit.name, fit.bytes);
      }
      await loadWalls();
      if (last) useWall(last);
      showToast(paths.length > 1 ? tr("{n} wallpapers added", { n: paths.length }) : tr("Wallpaper set"));
    } catch (e) {
      showToast(`Could not add wallpaper: ${e}`);
    }
  }
  function useWall(path: string) { settings.update((s) => ({ ...s, wallpaper: "custom", wallpaperPath: path, wallpaperCustom: undefined })); }
  async function removeWall(path: string) {
    try {
      await api.deleteWallpaper(path);
      if ($settings.wallpaperPath === path) settings.update((s) => ({ ...s, wallpaper: "aurora", wallpaperPath: undefined }));
      await loadWalls();
    } catch (e) { showToast(`${e}`); }
  }


  function toggle(key: "enterToSend" | "autoDownloadMedia" | "linkPreviews" | "notifySound" | "notifyDesktop" | "showSeconds") {
    settings.update((s) => ({ ...s, [key]: !s[key] }));
  }
  async function enableDesktopNotifications() {
    const on = !$settings.notifyDesktop;
    settings.update((s) => ({ ...s, notifyDesktop: on }));
    // Turning it on asks the system for permission (the old check ran
    // after the flip and so never asked).
    if (on && !(await ensureNotifyPermission(true))) showToast(tr("The system doesn't allow notifications for Aquamarine — turn them on in its settings."), 5000);
  }

  // ---- Themes / customisation / sounds ----
  function pickTheme(id: string) {
    settings.update((s) => ({ ...s, themePreset: id || undefined, ...(id ? { customColors: {} } : {}) }));
  }
  function setNum(key: "stickerSize" | "mediaMaxWidth" | "bubbleRadius" | "soundVolume", v: number) {
    settings.update((s) => ({ ...s, [key]: v }));
  }
  function setFlag(key: "autoplayGifs" | "autoplayStickers" | "autoplayVideos") {
    settings.update((s) => ({ ...s, [key]: !s[key] }));
  }
  function setAlpha(key: "bubbles" | "panels" | "inputs", v: number) {
    settings.update((s) => ({ ...s, alphas: { ...(s.alphas ?? {}), [key]: v } }));
  }
  let themeName = $state("My theme");
  async function saveTheme() {
    try { const p = await exportThemeToFile(themeName); if (p) showToast(tr("Theme saved")); }
    catch (e) { showToast(String(e), 6000); }
  }
  async function sendTheme() {
    const id = $activeId;
    const chat = $chats.find((c) => c.id === id);
    if (!chat) { showToast(tr("Open a chat first")); return; }
    try { await sendThemeToChat(chat.id, themeName); showToast(tr("Theme sent to {name}", { name: chat.name })); }
    catch (e) { showToast(String(e), 6000); }
  }
  async function openTheme() {
    try { const n = await pickAndApplyTheme(); if (n) showToast(tr("Theme “{name}” applied", { name: n })); }
    catch (e) { showToast(String(e), 6000); }
  }
  const allSounds = $derived([
    ...BUILTIN_SOUNDS.map((b) => ({ id: b.id, name: b.name, custom: false })),
    ...$customSounds.map((c) => ({ id: "file:" + c.id, name: c.name, custom: true })),
  ]);
  $effect(() => { if (section === "notifications") void refreshSounds(); });
  async function addSounds() {
    const res = await openFileDialog({ multiple: true, directory: false, title: tr("Add sounds"), filters: [{ name: "Audio", extensions: ["mp3", "ogg", "opus", "wav", "m4a", "flac", "aac"] }] });
    const paths = Array.isArray(res) ? res : res ? [res] : [];
    for (const p of paths) {
      try {
        const bytes = new Uint8Array(await (await fetch(await (await import("../media")).streamUrl(p))).arrayBuffer());
        const f = await addSoundBytes(p.split(/[\\/]/).pop() ?? "sound.mp3", bytes);
        showToast(tr("Added “{name}”", { name: f.name }));
      } catch (e) { showToast(String(e), 6000); }
    }
  }
  async function removeSound(id: string) {
    if (!(await askConfirm({ title: tr("Delete this sound?"), ok: tr("Delete"), danger: true }))) return;
    try {
      await api.deleteSound(id.slice(5));
      settings.update((s) => ({
        ...s,
        messageSound: s.messageSound === id ? "ding" : s.messageSound,
        chatSounds: Object.fromEntries(Object.entries(s.chatSounds ?? {}).filter(([, v]) => v !== id)),
      }));
      await refreshSounds();
    } catch (e) { showToast(String(e)); }
  }
  let perChatPick = $state("");
  function setChatSound(chatId: string, id: string) {
    settings.update((s) => {
      const m = { ...(s.chatSounds ?? {}) };
      if (id) m[chatId] = id; else delete m[chatId];
      return { ...s, chatSounds: m };
    });
  }

  function reset() {
    settings.set({ ...DEFAULTS, customColors: {}, customFontName: undefined, customFontData: undefined });
  }

  onMount(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") closeSettings(); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  // ---- a resizable window (desktop) ----
  const SIZE_KEY = "aqua.settings.size";
  function loadSheetSize(): { w: number; h: number } | null {
    try { const v = JSON.parse(localStorage.getItem(SIZE_KEY) || "null"); return v && v.w > 0 && v.h > 0 ? v : null; } catch { return null; }
  }
  let sheetSize = $state<{ w: number; h: number } | null>(loadSheetSize());
  function setSheetSize(v: { w: number; h: number } | null) {
    sheetSize = v;
    try { if (v) localStorage.setItem(SIZE_KEY, JSON.stringify(v)); else localStorage.removeItem(SIZE_KEY); } catch { /* storage blocked */ }
  }
  function startSheetResize(e: PointerEvent) {
    const sheet = (e.currentTarget as HTMLElement).parentElement!;
    const r = sheet.getBoundingClientRect();
    const x0 = e.clientX, y0 = e.clientY, w0 = r.width, h0 = r.height;
    e.preventDefault();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    // The window stays centred: the corner follows the pointer when both sides grow.
    const move = (ev: PointerEvent) => {
      sheetSize = { w: Math.round(Math.max(560, Math.min(window.innerWidth * 0.98, w0 + 2 * (ev.clientX - x0)))), h: Math.round(Math.max(420, Math.min(window.innerHeight * 0.96, h0 + 2 * (ev.clientY - y0)))) };
    };
    const up = () => { window.removeEventListener("pointermove", move); window.removeEventListener("pointerup", up); if (sheetSize) setSheetSize(sheetSize); };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }
</script>

{#if $settingsOpen}
  <div
    class="backdrop"
    role="button"
    tabindex="-1"
    aria-label={$t("Close settings")}
    onclick={(e) => { if ((e.target as HTMLElement).classList.contains("backdrop")) closeAll(); }}
    onkeydown={(e) => { if (e.target === e.currentTarget && (e.key === "Escape" || e.key === "Enter")) closeAll(); }}
  >
    <div class="sheet" data-own-back class:detail class:wide={section === "appearance"} class:sized={!!sheetSize} role="dialog" aria-modal="true" tabindex="-1"
      style:width={sheetSize ? `min(${sheetSize.w}px, 98vw)` : undefined} style:height={sheetSize ? `min(${sheetSize.h}px, 96vh)` : undefined}>
      <!-- Drag the corner to resize (remembered); double-click — back to the default. -->
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="sz-grip" onpointerdown={startSheetResize} ondblclick={() => setSheetSize(null)} title={$t("Drag to resize the window · double-click to reset")}></div>
      <div class="layout">
        <nav class="nav">
          <div class="list-head phone-only">
            <h2>{$t("Settings")}</h2>
            <button data-x class="close" onclick={closeAll} title={$t("Close")}>×</button>
          </div>
          <button class="me-card" onclick={() => { closeAll(); openProfile(); }} title={$t("Edit profile")}>
            <span class="me-ava" style:background={`linear-gradient(135deg, ${$profile.banner_a}, ${$profile.banner_b})`}>
              {#if $profile.avatar_url}<img src={$profile.avatar_url} alt="" />{:else}{($profile.display_name !== "You" ? $profile.display_name : $info?.username ?? "?").charAt(0).toUpperCase()}{/if}
            </span>
            <span class="me-text">
              <span class="me-name">{$profile.display_name !== "You" ? $profile.display_name : ($info?.username ?? "")}</span>
              <span class="me-sub">@{$info?.username ?? "—"}{$profile.phone ? ` · ${$profile.phone}` : ""}</span>
              <span class="me-online" class:off={!$info?.connected}>{$info?.connected ? ($myOnline === "invisible" ? "invisible" : $myOnline === "dnd" ? "do not disturb" : $myOnline) : "offline"}</span>
            </span>
          </button>
          {#each GROUPS as g, gi}
            <div class="nav-group">
              {#each g.filter((x) => !((x.id === "shortcuts" || x.id === "window") && (phone || TOUCH))) as it}
                <button class="nav-item" class:active={section === it.id} onclick={() => openItem(it)}>
                  <span class="tile" style:background={it.color}>{@html it.icon}</span>
                  <span class="nav-lbl">{$t(it.label)}</span>
                  {#if it.id === "language"}<span class="nav-hint">{LANGS.find((l) => l.id === $lang)?.native}</span>{:else if it.hint}<span class="nav-hint">{it.hint}</span>{/if}
                  <span class="chev">›</span>
                </button>
              {/each}
            </div>
          {/each}
        </nav>

        <div class="content">
          <div class="head">
            <button class="back phone-only" onclick={backToList} aria-label={$t("Back")}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 18 9 12 15 6"/></svg>
            </button>
            <div class="head-title">
              {#if sectionItem}<span class="tile" style:background={sectionItem.color}>{@html sectionItem.icon}</span>{/if}
              <h2>{$t(sectionTitle)}</h2>
            </div>
            <button data-x class="close" onclick={closeAll} title={$t("Close (Esc)")}>×</button>
          </div>

      {#if section === "ghost"}
      <section class="group">
        <div class="label">{$t("Ghost mode")}</div>
        {#each ([
          ["noReadReceipts", "Don't send read receipts", "People won't see that you've read their messages"],
          ["noTyping", "Don't send “typing…”", "Nobody sees when you're writing"],
        ] as const) as [k, title, sub] (k)}
          <button class="toggle-row" onclick={() => ghost.update((g) => ({ ...g, [k]: !g[k] }))}>
            <div class="tr-text"><div class="tr-title">{$t(title)}</div><div class="tr-sub">{$t(sub)}</div></div>
            <span class="switch" class:on={$ghost[k]}></span>
          </button>
        {/each}
        <button class="toggle-row" onclick={() => myOnline.set($myOnline === "invisible" ? "online" : "invisible")}>
          <div class="tr-text"><div class="tr-title">{$t("Don't send online status")}</div><div class="tr-sub">{$t("You look offline but can keep chatting")}</div></div>
          <span class="switch" class:on={$myOnline === "invisible"}></span>
        </button>
      </section>
      <section class="group">
        <div class="label">{$t("Spy mode")}</div>
        {#each ([
          ["keepDeleted", "Keep deleted messages", "Messages the sender deletes stay here, marked “deleted”"],
          ["keepEdits", "Keep edit history", "Edited messages show “edited”; hover to see earlier versions"],
        ] as const) as [k, title, sub] (k)}
          <button class="toggle-row" onclick={() => ghost.update((g) => ({ ...g, [k]: !g[k] }))}>
            <div class="tr-text"><div class="tr-title">{$t(title)}</div><div class="tr-sub">{$t(sub)}</div></div>
            <span class="switch" class:on={$ghost[k]}></span>
          </button>
        {/each}
        <div class="foot-note">{$t("Everything here stays on this device. Like AyuGram, but built in.")}</div>
      </section>
      {/if}

      {#if section === "language"}
      <section class="group">
        <div class="label">{$t("Interface language")}</div>
        {#each LANGS as l (l.id)}
          <button class="lang-row" class:on={$lang === l.id} onclick={() => lang.set(l.id)}>
            <span class="lang-radio" class:on={$lang === l.id}></span>
            <span class="lang-text"><span class="lang-native">{l.native}</span><span class="lang-name">{l.name}</span></span>
          </button>
        {/each}
        <div class="foot-note">{$t("Messages are never translated or sent anywhere — this only changes the app's own text.")}</div>
      </section>
      {/if}

      {#if section === "privacy"}
      <PrivacyRules />
      <section class="group">
        <div class="label">{$t("Passcode lock")}</div>
        {#if $lockConf?.hash}
          <div class="row">
            <span class="row-label">{$t("Auto-lock")}</span>
            <SegFit label={$t("Auto-lock")} value={$lockConf.autoMin} onpick={(m) => setAutoLock(m)}
              options={([[1, "1 min"], [5, "5 min"], [15, "15 min"], [60, "1 hour"], [0, "Off"]] as const).map(([id, l]) => ({ id: id as number, label: $t(l) }))} />
          </div>
          {#if bioAvail}
            <button class="toggle-row" onclick={() => setBiometric(!$lockConf?.bio)}>
              <div class="tr-text"><div class="tr-title">{$t("Unlock with fingerprint")}</div><div class="tr-sub">{$t("Or with your face, if the phone can. The passcode still works.")}</div></div>
              <span class="switch" class:on={!!$lockConf?.bio}></span>
            </button>
          {/if}
          <div class="info-row"><span class="v">{$t("Ctrl+L locks the app any time.")}</span>
            <button class="mini-btn" onclick={() => { closeSettings(); lockNow(); }}>{$t("Lock now")}</button>
            <button class="mini-btn" onclick={() => { lockConf.set(null); passcodeSetupOpen.set(true); }}>{$t("Change")}</button>
          </div>
        {:else}
          <button class="mini-btn" onclick={() => passcodeSetupOpen.set(true)}>{$t("Set a passcode")}</button>
        {/if}
      </section>
      <section class="group">
        <div class="label">{$t("Blocked users")}</div>
        {#if $blocked.size === 0}
          <div class="foot-note">{$t("Nobody is blocked.")}</div>
        {:else}
          {#each [...$blocked] as u (u)}
            <div class="info-row"><span class="v">{$chats.find((c) => c.id === u)?.name ?? u} <span class="dim">@{u}</span></span><button class="mini-btn" onclick={() => setBlocked(u, false)}>{$t("Unblock")}</button></div>
          {/each}
        {/if}
        {#if blockable.length}
          <div class="row">
            <Select class="sel" bind:value={blockPick} options={[{ value: "", label: $t("Block someone…") }, ...blockable.map((c) => ({ value: c.id, label: `${c.name} · @${c.id}` }))]} />
            <button class="mini-btn danger" disabled={!blockPick} onclick={() => { setBlocked(blockPick, true); blockPick = ""; }}>{$t("Block")}</button>
          </div>
        {/if}
      </section>
      <section class="group">
        <div class="label">{$t("Locked chats")}</div>
        {#if !Object.keys($chatLocks).length}
          <div class="foot-note">{$t("No chat has its own password.")}</div>
        {/if}
        {#each Object.keys($chatLocks) as id (id)}
          <div class="info-row">
            <span class="v">🔒 {$chats.find((c) => c.id === id)?.name ?? id}</span>
            <button class="mini-btn" onclick={() => askChangeLock(id)}>{$t("Change password")}</button>
            <button class="mini-btn" onclick={() => askRemoveLock(id)}>{$t("Remove")}</button>
          </div>
        {/each}
        {#if lockable.length}
          <div class="row">
            <Select class="sel" bind:value={lockPick} options={[{ value: "", label: $t("Lock a chat…") }, ...lockable.map((c) => ({ value: c.id, label: c.name }))]} />
            <button class="mini-btn primary" disabled={!lockPick} onclick={() => { askSetLock(lockPick); lockPick = ""; }}>{$t("Set password")}</button>
          </div>
        {/if}
        <div class="foot-note">{$t("A locked chat hides its name and messages until you enter its password. Only on your devices — the other side sees nothing.")}</div>
      </section>
      <section class="group">
        <div class="label">{$t("Who sees what")}</div>
        <div class="row">
          <span class="row-label">{$t("Status")}</span>
          <SegFit label={$t("Status")} value={$myOnline} onpick={(id) => myOnline.set(id)}
            options={([["online", "Online"], ["idle", "Idle"], ["dnd", "Busy"], ["invisible", "Invisible"]] as const).map(([id, l]) => ({ id, label: $t(l) }))} />
        </div>
        <button class="toggle-row" onclick={() => shareActivity.update((v) => !v)}>
          <div class="tr-text">
            <div class="tr-title">{$t("Share my activity")}</div>
            <div class="tr-sub">
              Show people you chat with the game you play, the music you listen to and the video you watch — detected from your open windows, nothing else is read.
              {#if $shareActivity && $myActivity[0]}<br /><b>Now: {activityVerb($myActivity[0])} {$myActivity[0].name}</b>{/if}
            </div>
          </div>
          <span class="switch" class:on={$shareActivity}></span>
        </button>
      </section>
      <section class="group">
        <div class="label">{$t("Encryption")}</div>
        <div class="info-row">
          <span class="k">{$t("Fingerprint")}</span>
          <span class="v mono">{$info?.session_fingerprint ?? "—"}</span>
          <button class="mini-btn" onclick={copyFp}>{$t("Copy")}</button>
        </div>
        <div class="info-row"><span class="k">{$t("Prekeys")}</span><span class="v">{$info?.opk_available ?? 0} one-time keys · signed key rotates in {$info?.spk_rotates_in_days ?? "?"} d</span></div>
        <div class="info-row"><span class="k">{$t("Vault")}</span><span class="v mono dir" title={$info?.data_dir ?? ""}>{$info?.data_dir ?? "—"}</span></div>
        <div class="foot-note">{$t("Messages are end-to-end encrypted (X3DH + Double Ratchet). Compare fingerprints with your contact to rule out a man in the middle.")}</div>
      </section>
      <section class="group">
        <button class="danger-row" onclick={logOut}>{$t("Log out")}</button>
      </section>
      {/if}

      {#if section === "stickers"}
      <section class="group">
        <div class="label">{$t("Reactions")}</div>
        <div class="rp-row">
          {#each [...new Set(quickReactions($reactionPack.items))] as r (r)}
            {@const ru = reactionUrl(r, $reactionPack.items)}
            <span class="rp-cell">{#if ru}<AnimatedMedia src={ru.url} mime={ru.mime} alt={parseReaction(r).emoji} play="auto" />{:else}{r}{/if}</span>
          {/each}
        </div>
        {#if $reactionPack.source === "bundled"}
          <div class="foot-note">{$t("The")} <b>{$t("El Taller")}</b> {$t("pack ships with this build of Aquamarine — everyone sees the same reactions.")}</div>
        {:else if $reactionPack.source === "imported"}
          <div class="foot-note">{$t("Using the imported")} <b>{$t("El Taller")}</b> pack ({$reactionPack.items.length} emoji). To ship it to everyone, run <code>tools\bundle-reactions.bat</code> and rebuild.</div>
        {:else}
          <div class="foot-note">{$t("Reactions use the")} <b>{$t("El Taller")}</b> {$t("emoji pack (")}<a href={REACTION_PACK_LINK} target="_blank" rel="noopener">{$t("eltaller47")}</a>{$t("). Import it once with a Telegram bot token from @BotFather — the token only talks to Telegram.")}</div>
          <div class="rp-import">
            <input class="txt" type="password" placeholder={$t("Telegram bot token")} bind:value={rpToken} />
            <button class="mini-btn primary" onclick={importRp} disabled={rpBusy}>{$t(rpBusy ? "Importing…" : "Import")}</button>
          </div>
        {/if}
      </section>
      <section class="group">
        <div class="label">{$t("Your stickers")}</div>
        <div class="my-actions">
          <button class="mini-btn primary" onclick={newPackFlow}>＋ {$t("New pack")}</button>
          <button class="mini-btn" onclick={() => newSticker()}>＋ {$t("New sticker")}</button>
          <button class="mini-btn" onclick={() => openStickerEditor([], "", "text", myPacks[0]?.title)}>Aa {$t("Text sticker")}</button>
        </div>
        {#if myPacks.length === 0}
          <div class="foot-note">{$t("Stickers you make in the editor live here. Pick a picture — the editor cuts out the background, adds captions, drawing and a white outline, like Telegram.")}</div>
        {:else}
          {#each myPacks as p (p.title)}
            <div class="my-pack" class:open={openPack === p.title}>
              <div class="mp-head">
                <button class="mp-toggle" onclick={() => (openPack = openPack === p.title ? null : p.title)}>
                  <span class="mp-thumb">{#if p.items[0]}<AnimatedMedia src={assetUrl(p.items[0].path)} mime={p.items[0].mime} play="hover" alt="" />{/if}</span>
                  {#if renaming === p.title}
                    <!-- svelte-ignore a11y_autofocus -->
                    <input class="txt mp-rename" bind:value={renameTo} autofocus onclick={(e) => e.stopPropagation()}
                      onkeydown={(e) => { if (e.key === "Enter") void renameMyPack(p); if (e.key === "Escape") renaming = null; }}
                      onblur={() => void renameMyPack(p)} />
                  {:else}
                    <span class="mp-title">{p.title}</span>
                  {/if}
                  <span class="mp-count">{p.items.length}</span>
                  <span class="mp-chev">›</span>
                </button>
                <button class="mini-btn" onclick={() => openPackManager(p.title)} title={$t("Edit pack: emoji, order, icon, delete")}>✎ {$t("Manage")}</button>
              </div>
              {#if openPack === p.title}
                <div class="mp-grid">
                  {#each p.items as it (it.id)}
                    <div class="mp-item">
                      <AnimatedMedia src={assetUrl(it.path)} mime={it.mime} play="hover" alt={it.emoji ?? ""} />
                      {#if it.emoji}<span class="mp-emoji">{it.emoji}</span>{/if}
                      <button class="mp-del" onclick={() => deleteMySticker(it.path)} title={$t("Delete")} aria-label={$t("Delete")}>×</button>
                    </div>
                  {/each}
                  <button class="mp-add" onclick={() => addManyTo(p.title)} title={$t("Add stickers — several at once")}>＋</button>
                </div>
                <div class="my-actions">
                  <button class="mini-btn" onclick={() => { renaming = p.title; renameTo = p.title; }}>{$t("Rename")}</button>
                  <button class="mini-btn danger" onclick={() => deleteMyPack(p)}>{$t("Delete pack")}</button>
                </div>
              {/if}
            </div>
          {/each}
        {/if}
      </section>
      <section class="group">
        <div class="label">{$t("Telegram packs")}</div>
        {#if tgPacks.length === 0}
          <div class="foot-note">{$t("Packs you import from Telegram appear here. They can't be edited, but they update from Telegram.")}</div>
        {:else}
          {#each tgPacks as p (p.title)}
            <div class="my-pack" class:open={openPack === "tg:" + p.title}>
              <div class="mp-head">
                <button class="mp-toggle" onclick={() => (openPack = openPack === "tg:" + p.title ? null : "tg:" + p.title)}>
                  <span class="mp-thumb">{#if p.icon || p.items[0]}<AnimatedMedia src={assetUrl(p.icon ?? p.items[0].path)} mime={p.items.find((i) => i.path === p.icon)?.mime ?? p.items[0]?.mime ?? "image/webp"} play="hover" alt="" />{/if}</span>
                  <span class="mp-title">{p.title}</span>
                  <span class="tg-badge" title="Telegram">
                    <svg viewBox="0 0 24 24" width="12" height="12" aria-hidden="true"><path fill="currentColor" d="M21.9 4.3 18.6 19.8c-.2 1.1-.9 1.4-1.8.9l-5-3.7-2.4 2.3c-.3.3-.5.5-1 .5l.4-5.1 9.2-8.3c.4-.4-.1-.6-.6-.2L6 13.4 1.1 11.9c-1.1-.3-1.1-1.1.2-1.6L20.5 2.9c.9-.3 1.7.2 1.4 1.4Z"/></svg>
                  </span>
                  <span class="mp-count">{p.items.length}</span>
                  <span class="mp-chev">›</span>
                </button>
                <button class="mini-btn" onclick={() => syncTg(p)} disabled={tgBusy === p.title} title={$t("Get the latest version from Telegram")}>{tgBusy === p.title ? "…" : "↻"}</button>
                <button class="mini-btn danger" onclick={() => deleteTgPack(p)} title={$t("Delete pack")}>✕</button>
              </div>
              {#if openPack === "tg:" + p.title}
                <div class="mp-grid">
                  {#each p.items as it (it.id)}
                    <div class="mp-item">
                      <AnimatedMedia src={assetUrl(it.path)} mime={it.mime} play="hover" alt={it.emoji ?? ""} lazy />
                      {#if it.emoji}<span class="mp-emoji">{it.emoji}</span>{/if}
                    </div>
                  {/each}
                </div>
                {#if p.source}<div class="foot-note">t.me/{p.kind === "emoji" ? "addemoji" : "addstickers"}/{p.source}</div>{/if}
              {/if}
            </div>
          {/each}
        {/if}
      </section>
      <section class="group">
        <div class="label">{$t("Panel")}</div>
        <div class="foot-note">
          {$t("Open with the smiley in the message box or")} <b>{$t("Ctrl+E")}</b>{$t(". Drop GIF / PNG / TGS files onto the panel to add them, import any Telegram sticker or emoji pack from")} <b>+</b>{$t(", and right-click a sticker or GIF in a chat to save it.")}
        </div>
      </section>
      {/if}

      {#if section === "appearance"}
      <div class="ap-split">
      <div class="ap-controls">
      <section class="group">
        <div class="label">{$t("Themes")}</div>
        <div class="themes">
          <button class="theme-card" class:active={!$settings.themePreset} onclick={() => pickTheme("")}>
            <span class="tc-prev" style="--s:#0C0F17;--l:#0A0D15;--b:#141825;--a:#00E5C7;--f:#E5EAF2">
              <i class="tc-l"></i><i class="tc-in"></i><i class="tc-out"></i>
            </span>
            <span class="tc-name">Aquamarine</span>
          </button>
          {#each THEMES as th (th.id)}
            <button class="theme-card" class:active={$settings.themePreset === th.id} onclick={() => pickTheme(th.id)} title={th.name}>
              <span class="tc-prev" style={`--s:${th.colors["--bg-shell"]};--l:${th.colors["--bg-list"]};--b:${th.colors["--bg-bubble-in"]};--a:${th.colors["--accent"]};--f:${th.colors["--fg"]}`}>
                <i class="tc-l"></i><i class="tc-in"></i><i class="tc-out"></i>
              </span>
              <span class="tc-name">{th.name}</span>
            </button>
          {/each}
        </div>
        <div class="foot-note">{$t("A theme sets every colour; the accent, theme editor and transparency still work on top. Share the whole look in Theme editor → Share.")}</div>
      </section>
      <section class="group">
        <div class="label">{$t("Appearance")}</div>
        <div class="row">
          <span class="row-label">{$t("Shape")}</span>
          <SegFit label={$t("Shape")} value={$settings.corners ?? 1} onpick={(v) => settings.update((x) => ({ ...x, corners: v }))}
            options={([[0.3, "Square"], [0.65, "Soft"], [1, "Standard"], [1.6, "Round"]] as const).map(([id, l]) => ({ id: id as number, label: $t(l) }))} />
        </div>
        <button class="toggle-row" onclick={() => settings.update((x) => ({ ...x, contrastRows: !x.contrastRows }))}>
          <div class="tr-text"><div class="tr-title">{$t("Contrast rows")}</div><div class="tr-sub">{$t("Every other chat and person in lists is a shade darker, so neighbours don't merge (like Spotify)")}</div></div>
          <span class="switch" class:on={!!$settings.contrastRows}></span>
        </button>
        <button class="toggle-row" onclick={() => settings.update((x) => ({ ...x, glassMenus: !x.glassMenus }))}>
          <div class="tr-text"><div class="tr-title">{$t("See-through menus")}</div><div class="tr-sub">{$t("Menus, pop-ups and windows become frosted glass over the chat")}</div></div>
          <span class="switch" class:on={!!$settings.glassMenus}></span>
        </button>
        {#if $settings.glassMenus}
          <div class="row">
            <span class="row-label">{$t("Opacity")}</span>
            <div class="range">
              <input type="range" min="0.35" max="0.95" step="0.05" value={$settings.menuAlpha ?? MENU_ALPHA}
                oninput={(e) => settings.update((x) => ({ ...x, menuAlpha: +(e.target as HTMLInputElement).value }))} />
              <span class="range-val">{Math.round(($settings.menuAlpha ?? MENU_ALPHA) * 100)}%</span>
            </div>
          </div>
        {/if}
        <div class="row">
          <span class="row-label">{$t("Mode")}</span>
          <SegFit label={$t("Mode")} value={$settings.mode} onpick={(m) => setMode(m)}
            options={(["dark", "light", "auto"] as const).map((m) => ({ id: m, label: $t(m === "dark" ? "Dark" : m === "light" ? "Light" : "Auto") }))} />
        </div>

        <div class="row">
          <span class="row-label">{$t("Density")}</span>
          <SegFit label={$t("Density")} value={$settings.density} onpick={(d) => setDensity(d)}
            options={(["comfortable", "compact"] as const).map((d) => ({ id: d, label: $t(d === "compact" ? "Compact" : "Comfortable") }))} />
        </div>

        <div class="row">
          <span class="row-label">{$t("Text size")}</span>
          <div class="range">
            <input
              type="range" min="0.85" max="1.4" step="0.05"
              value={$settings.fontScale}
              oninput={(e) => setFontScale(parseFloat((e.target as HTMLInputElement).value))}
            />
            <span class="range-val">{Math.round($settings.fontScale * 100)}%</span>
          </div>
        </div>
        <!-- Applied on release: zooming while the finger is on the slider
             would move the slider under it. -->
        <div class="row">
          <span class="row-label">{$t("Interface scale")}</span>
          <div class="range">
            <input
              type="range" min="0.8" max="1.3" step="0.05"
              value={$settings.uiScale ?? 1}
              oninput={(e) => (uiScalePreview = parseFloat((e.target as HTMLInputElement).value))}
              onchange={(e) => { uiScalePreview = null; setUiScale(parseFloat((e.target as HTMLInputElement).value)); }}
            />
            <span class="range-val">{Math.round((uiScalePreview ?? $settings.uiScale ?? 1) * 100)}%</span>
          </div>
        </div>
      </section>
      <section class="group">
        <div class="label">{$t("Bubbles & media size")}</div>
        <div class="row">
          <span class="row-label">{$t("Stickers")}</span>
          <div class="range">
            <input type="range" min="80" max="256" step="8" value={$settings.stickerSize} oninput={(e) => setNum("stickerSize", +(e.target as HTMLInputElement).value)} />
            <span class="range-val">{$settings.stickerSize}px</span>
          </div>
        </div>
        <div class="row">
          <span class="row-label">{$t("Photos & GIFs")}</span>
          <div class="range">
            <input type="range" min="220" max="560" step="10" value={$settings.mediaMaxWidth} oninput={(e) => setNum("mediaMaxWidth", +(e.target as HTMLInputElement).value)} />
            <span class="range-val">{$settings.mediaMaxWidth}px</span>
          </div>
        </div>
        <div class="row">
          <span class="row-label">{$t("Bubble corners")}</span>
          <div class="range">
            <input type="range" min="2" max="24" step="1" value={$settings.bubbleRadius} oninput={(e) => setNum("bubbleRadius", +(e.target as HTMLInputElement).value)} />
            <span class="range-val">{$settings.bubbleRadius}px</span>
          </div>
        </div>
      </section>
      <div class="ap-div" id="ap-wallpaper"><span>{$t("Chat wallpaper")}</span></div>
      <section class="group">
        <div class="label">{$t("Chat wallpaper")}</div>
        <div class="walls">
          {#each WALLPAPERS as w}
            <button
              class="wall"
              class:active={$settings.wallpaper === w.id}
              onclick={() => setWallpaper(w.id)}
              title={w.name}
            >
              <span class="wall-swatch" style={w.css ? `background: ${w.css};` : "background: var(--bg-input); border-style: dashed;"}>
                {#if w.id === "none"}<span class="wall-mark">✕</span>{/if}
              </span>
              <span class="wall-name">{$t(w.name)}</span>
            </button>
          {/each}
        </div>
      </section>
      <section class="group">
        <div class="label">{$t("Your wallpapers")}</div>
        <div class="walls">
          <button class="wall" onclick={pickCustomWallpaper} title={$t("Add pictures — several at once")}>
            <span class="wall-swatch custom"><span class="wall-mark">+</span></span>
            <span class="wall-name">{$t("Add…")}</span>
          </button>
          {#each myWalls as w (w)}
            <div class="wall own" class:active={$settings.wallpaper === "custom" && $settings.wallpaperPath === w}>
              <button class="wall-pick" onclick={() => useWall(w)} title={$t("Use this wallpaper")}>
                <img class="wall-swatch img" src={assetUrl(w)} alt="" loading="lazy" />
              </button>
              <button class="wall-del" onclick={() => removeWall(w)} title={$t("Delete")} aria-label={$t("Delete")}>×</button>
            </div>
          {/each}
        </div>
        {#if $settings.wallpaper === "custom"}
          <div class="row">
            <span class="row-label">{$t("Darken")}</span>
            <div class="range">
              <input type="range" min="0" max="0.8" step="0.05" value={$settings.wallpaperDim ?? 0.3} oninput={(e) => settings.update((s) => ({ ...s, wallpaperDim: +(e.target as HTMLInputElement).value }))} />
              <span class="range-val">{Math.round(($settings.wallpaperDim ?? 0.3) * 100)}%</span>
            </div>
          </div>
        {/if}
        <div class="foot-note">{$t("Pictures are kept in the app's data folder; switch between them any time.")}</div>
      </section>
      <div class="ap-div" id="ap-accent"><span>{$t("Colours & icon")}</span></div>
      <section class="group">
        <div class="label">{$t("Accent colour")}</div>
        <div class="palettes">
          {#each PALETTES as p}
            <button
              class="palette"
              class:active={$settings.palette === p.id}
              onclick={() => setPalette(p.id)}
              title={p.name}
              style={`--pa:${p.accent}; --pb:${p.accent2};`}
            >
              <span class="swatch"></span>
              <span class="pal-name">{p.name}</span>
              {#if $settings.palette === p.id}
                <span class="chk">{@html icons.check}</span>
              {/if}
            </button>
          {/each}
        </div>
      </section>
      <section class="group">
        <div class="label">{$t("App icon")}</div>
        <div class="icons-row">
          <button class="app-ic" class:active={iconMode === "default"} onclick={() => setAppIcon({ mode: "default" })} title="Aquamarine">
            <img src={DEFAULT_ICON} alt="" /><span>Aquamarine</span>
          </button>
          <button class="app-ic" class:active={iconMode === "custom"} onclick={pickCustomIcon} title={$t("Your own picture")}>
            {#if $settings.appIcon?.custom}<img src={$settings.appIcon.custom} alt="" />{:else}<span class="plus">＋</span>{/if}<span>{$t("Custom…")}</span>
          </button>
        </div>
        <div class="foot-note">{$t("Shown in the title bar, the sidebar and the taskbar. On Wayland the taskbar uses the installed icon.")}</div>
      </section>
      <div class="ap-div" id="ap-theme"><span>{$t("Theme editor")}</span></div>
      <section class="group">
        <div class="label">{$t("Theme editor")}</div>
        <div class="te-intro">{$t("Override any colour. Changes apply live and are saved to this device.")}</div>


        {#each COLOR_GROUPS as grp}
          <div class="te-group-title">{$t(grp)}</div>
          <div class="te-colors">
            {#each EDITABLE_COLORS.filter((c) => c.group === grp) as c (c.key)}
              <label class="te-color" title={$t(c.desc)}>
                <ColorInput value={colorValue(c.key, c.dark, c.light)} oninput={(v) => setColor(c.key, v)} size={28} title={$t(c.label)} />
                <span class="te-name">{$t(c.label)}</span>
                <span class="te-help" title={$t(c.desc)}>*<span class="te-tip">{$t(c.desc)}</span></span>
              </label>
            {/each}
          </div>
        {/each}

        <div class="te-group-title">{$t("Interface font")}</div>
        <div class="te-font">
          <div class="te-font-cur">{$settings.customFontName ?? "Default (Onest)"}</div>
          <div class="te-font-btns">
            <button class="te-btn" onclick={pickCustomFont}>{$t("Load font file…")}</button>
            {#if $settings.customFontName}
              <button class="te-btn ghost" onclick={clearCustomFont}>{$t("Reset")}</button>
            {/if}
          </div>
          <div class="te-hint">{$t(".ttf · .otf · .woff · .woff2 — pick any font from your system folders.")}</div>
        </div>

        <div class="te-group-title">{$t("Transparency")}</div>
        {#each ([["bubbles", "Message bubbles"], ["panels", "Chat list & rail"], ["inputs", "Fields & cards"]] as const) as [k, lbl] (k)}
          <div class="row">
            <span class="row-label">{$t(lbl)}</span>
            <div class="range">
              <input type="range" min="0.3" max="1" step="0.02" value={$settings.alphas?.[k] ?? 1} oninput={(e) => setAlpha(k, +(e.target as HTMLInputElement).value)} />
              <span class="range-val">{Math.round(($settings.alphas?.[k] ?? 1) * 100)}%</span>
            </div>
          </div>
        {/each}

        <button class="reset" style="margin-top:16px" onclick={clearColors}>{$t("Clear custom colours")}</button>
      </section>
      <section class="group">
        <div class="label">{$t("Share")}</div>
        <div class="foot-note" style="margin:0 0 10px">{$t("One .aquatheme file with everything: theme and colours with transparency, wallpaper, font, chat sizes, autoplay and your message sound. Others open it or tap “Apply theme” on it in a chat.")}</div>
        <div class="share-row">
          <input class="txt" bind:value={themeName} placeholder={$t("Theme name")} />
          <button class="mini-btn primary" onclick={saveTheme}>{$t("Save to file…")}</button>
          <button class="mini-btn" onclick={sendTheme}>{$t("Send to current chat")}</button>
        </div>
        <button class="mini-btn" style="margin-top:10px" onclick={openTheme}>{$t("Open a theme file…")}</button>
      </section>
      </div>
      <aside class="ap-preview" aria-label={$t("Preview")}>
        <div class="pv-card">
          <div class="pv-head">
            <span class="pv-ava">A</span>
            <div class="pv-meta"><b>{$t("Alice")}</b><span>{$t("online")}</span></div>
          </div>
          <div class="pv-stage">
            <div class="pv-wall"></div>
            <div class="pv-msgs">
              <span class="pv-date">{$t("Today")}</span>
              <div class="pv-b in first">{$t("Hi! How does the new theme look?")}<span class="pv-t">12:40</span></div>
              <div class="pv-b in">{$t("The wallpaper shows through here 🌊")}<span class="pv-t">12:40</span></div>
              <div class="pv-b out first"><span class="pv-reply"><b>{$t("Alice")}</b>{$t("How does the new theme look?")}</span>{$t("Looks great — every change shows up right here.")}<span class="pv-t">12:41 ✓✓</span></div>
              <div class="pv-b out">{$t("Bold")} <b>{$t("text")}</b>, <a href="#top" onclick={(e) => e.preventDefault()}>{$t("a link")}</a> {$t("and")} <code>code</code><span class="pv-t">12:41 ✓✓</span></div>
              <div class="pv-sticker">🐬</div>
            </div>
          </div>
          <div class="pv-foot">
            <span class="pv-field">{$t("Write a message…")}</span>
            <span class="pv-send"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><line x1="22" y1="2" x2="11" y2="13"/><polygon points="22 2 15 22 11 13 2 9 22 2"/></svg></span>
          </div>
        </div>
        <div class="pv-note">{$t("Live preview — every change applies at once")}</div>
      </aside>
      </div>
      <section class="group">
        <div class="label">{$t("Avatar corners")}</div>
        {#each [["avatarRadius", "People", "ava-p"], ["avatarRadiusGroup", "Groups and servers", "ava-g"], ["avatarRadiusChannel", "Channels", "ava-c"]] as [k, l, cls] (k)}
          {@const v = (($settings as unknown as Record<string, number | undefined>)[k] ?? (k === "avatarRadius" ? undefined : $settings.avatarRadius)) ?? 50}
          <div class="ava-row">
            <span class="ava-demo {cls}" style:border-radius="{v}%">{l === "People" ? ($info?.username ?? "A").slice(0, 2).toUpperCase() : l === "Channels" ? "📢" : "👥"}</span>
            <div class="ava-ctl">
              <div class="ava-head"><b>{$t(l)}</b><span class="pill-tag">{v >= 50 ? $t("Circle") : v <= 0 ? $t("Square") : `${v}%`}</span></div>
              <div class="range"><input type="range" min="0" max="50" step="1" value={v}
                oninput={(e) => settings.update((x) => ({ ...x, [k]: parseInt((e.target as HTMLInputElement).value) }))} /></div>
            </div>
          </div>
        {/each}
      </section>
      {#if phone}
      <section class="group">
        <div class="label">{$t("Glass panels")}</div>
        <button class="toggle-row" onclick={() => settings.update((x) => ({ ...x, glass: !x.glass }))}>
          <div class="tr-text"><div class="tr-title">{$t("See-through floating panels")}</div><div class="tr-sub">{$t("The chat header, the message field and the tab bar float over the chat, like in the new Telegram")}</div></div>
          <span class="switch" class:on={!!$settings.glass}></span>
        </button>
        {#if $settings.glass}
          {@const ga = $settings.glassAlpha ?? 55}
          <div class="ava-row">
            <div class="ava-ctl">
              <div class="ava-head"><b>{$t("Opacity")}</b><span class="pill-tag">{ga === 0 ? $t("Outline only") : `${ga}%`}</span></div>
              <div class="range"><input type="range" min="0" max="100" step="5" value={ga}
                oninput={(e) => settings.update((x) => ({ ...x, glassAlpha: parseInt((e.target as HTMLInputElement).value) }))} /></div>
            </div>
          </div>
        {/if}
      </section>
      {/if}
      {/if}

      {#if section === "behaviour"}
      <section class="group">
        <div class="label">{$t("Attach menu")}</div>
        <div class="foot-note" style="margin:0 0 8px">{$t("What the 📎 button shows right away and in what order. The rest waits under “More”; hidden ones aren't shown at all.")}</div>
        {#each [...$attachFront.map((id) => ATTACH_ITEMS.find((x) => x.id === id)).filter((x) => !!x), ...ATTACH_ITEMS.filter((x) => !$attachFront.includes(x.id))] as it, i (it.id)}
          {@const front = $attachFront.includes(it.id)}
          {@const hid = $attachHidden.includes(it.id)}
          <div class="toggle-row am-row" class:off={hid}>
            <div class="tr-text"><div class="tr-title">{$t(it.label)}</div>
              <div class="tr-sub">{hid ? $t("Hidden") : front ? $t("Right away") : $t("Under “More”")}</div></div>
            {#if front}
              <button class="am-btn" disabled={i === 0} onclick={() => moveAttach(it.id, -1)} aria-label={$t("Move up")}>↑</button>
              <button class="am-btn" disabled={i === $attachFront.length - 1} onclick={() => moveAttach(it.id, 1)} aria-label={$t("Move down")}>↓</button>
            {/if}
            <button class="am-btn" onclick={() => toggleAttachHidden(it.id)} title={$t(hid ? "Show" : "Hide")} aria-label={$t(hid ? "Show" : "Hide")}>
              {#if hid}<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M3 3l18 18M10.6 10.6a2 2 0 0 0 2.8 2.8M9.9 5.1A9.8 9.8 0 0 1 12 5c5 0 9 4.5 10 7-.4 1-1.2 2.3-2.4 3.5M6.2 6.2C4.3 7.5 2.9 9.4 2 12c1 2.5 5 7 10 7 1.7 0 3.3-.5 4.6-1.3"/></svg>
              {:else}<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M2 12c1-2.5 5-7 10-7s9 4.5 10 7c-1 2.5-5 7-10 7S3 14.5 2 12Z"/><circle cx="12" cy="12" r="3"/></svg>{/if}
            </button>
            <button class="switch-btn" onclick={() => toggleAttachFront(it.id)} aria-label={$t("Right away")}><span class="switch" class:on={front}></span></button>
          </div>
        {/each}
        {#if phone}
          <button class="toggle-row" onclick={() => attachDock.update((v) => !v)}>
            <div class="tr-text"><div class="tr-title">{$t("Floating action bar")}</div><div class="tr-sub">{$t("On phones the actions float at the bottom; off — they sit in the sheet under the gallery")}</div></div>
            <span class="switch" class:on={$attachDock}></span>
          </button>
        {/if}
        <div class="label" style="margin-top:12px">{$t("Bots in the attach menu")}</div>
        {#each $attachBots as b (b.bot)}
          <div class="toggle-row am-row">
            <div class="tr-text"><div class="tr-title">{b.title}</div><div class="tr-sub">@{b.bot}</div></div>
            <button class="mini-btn" onclick={() => toggleAttachBot(b)}>{$t("Remove")}</button>
          </div>
        {:else}
          <div class="foot-note" style="margin:0 0 8px">{$t("Open a bot with a mini app → ⋯ → “Add to attach menu”.")}</div>
        {/each}
        <button class="mini-btn" onclick={resetAttachMenu}>{$t("Reset")}</button>
      </section>
      <section class="group">
        <div class="label">{$t("Media")}</div>
        <div class="row">
          <span class="row-label">{$t("Save-to-favourites reaction")}</span>
          <SegFit label={$t("Save-to-favourites reaction")} value={$settings.favReaction ?? "⭐"} onpick={(e) => settings.update((x) => ({ ...x, favReaction: e }))}
            options={["⭐", "🔖", "📌", "💾", ""].map((e) => ({ id: e, label: e || $t("Off") }))} />
        </div>
        <div class="foot-note" style="margin:0 0 8px">{$t("React with it and the message is copied to Saved Messages. Only you see this reaction.")}</div>
        <button class="toggle-row" onclick={() => setFlag("autoplayGifs")}>
          <div class="tr-text"><div class="tr-title">{$t("Autoplay GIFs")}</div><div class="tr-sub">{$t("Off: GIFs wait for a click")}</div></div>
          <span class="switch" class:on={$settings.autoplayGifs}></span>
        </button>
        <button class="toggle-row" onclick={() => setFlag("autoplayStickers")}>
          <div class="tr-text"><div class="tr-title">{$t("Animated stickers & emoji")}</div><div class="tr-sub">{$t("Off: they play when you hover them")}</div></div>
          <span class="switch" class:on={$settings.autoplayStickers}></span>
        </button>
        <button class="toggle-row" onclick={() => setFlag("autoplayVideos")}>
          <div class="tr-text"><div class="tr-title">{$t("Autoplay videos")}</div><div class="tr-sub">{$t("Round videos and videos behind links start muted by themselves")}</div></div>
          <span class="switch" class:on={$settings.autoplayVideos}></span>
        </button>
      </section>
      {#if gpu}
      <section class="group">
        <div class="label">{$t("Graphics")}</div>
        <div class="row">
          <span class="row-label">{$t("Drawing")}</span>
          <SegFit label={$t("Drawing")} value={gpu.mode} onpick={(id) => setGpu(id)}
            options={([["auto", "Auto"], ["on", "Graphics card"], ["off", "CPU"]] as const).map(([id, l]) => ({ id, label: $t(l) }))} />
        </div>
        <div class="foot-note" style="margin:4px 0 0">
          {gpu.software ? $t("Now: drawn by the CPU (no GPU compositing).") : $t("Now: the graphics card composites the window.")}
          {gpu.reason === "nvidia-mismatch" ? " " + $t("The NVIDIA driver was updated without a restart — restart the computer to get the graphics card back.") : ""}
          {$t("Auto switches to the CPU when the graphics driver can't be used. Takes effect after restarting Aquamarine.")}
        </div>
        {#if isLinuxWk && !gpu.software}
          <button class="toggle-row" onclick={toggleDmabuf}>
            <div class="tr-text"><div class="tr-title">{$t("Direct GPU rendering (DMA-BUF)")}</div><div class="tr-sub">{$t("Smoother scrolling on Linux. If the window turns blank, it switches back by itself.")}</div></div>
            <span class="switch" class:on={!!gpu.dmabuf}></span>
          </button>
        {/if}
      </section>
      {/if}
      <section class="group">
        <div class="label">{$t("Behaviour")}</div>
        <button class="toggle-row" onclick={() => toggle("enterToSend")}>
          <div class="tr-text">
            <div class="tr-title">{$t("Enter to send")}</div>
            <div class="tr-sub">{$t("Off: Enter makes a newline, Ctrl/⌘+Enter sends")}</div>
          </div>
          <span class="switch" class:on={$settings.enterToSend}></span>
        </button>
        <button class="toggle-row" onclick={() => toggle("autoDownloadMedia")}>
          <div class="tr-text">
            <div class="tr-title">{$t("Auto-download media")}</div>
            <div class="tr-sub">{$t("Fetch images / voice / video automatically on receive")}</div>
          </div>
          <span class="switch" class:on={$settings.autoDownloadMedia}></span>
        </button>
        <button class="toggle-row" onclick={() => toggle("linkPreviews")}>
          <div class="tr-text">
            <div class="tr-title">{$t("Link previews")}</div>
            <div class="tr-sub">{$t("Show a card for links; videos behind links load and play in the chat. The site sees your IP (or your proxy's).")}</div>
          </div>
          <span class="switch" class:on={$settings.linkPreviews}></span>
        </button>
        <button class="toggle-row" onclick={() => { toggle("showSeconds"); void loadActiveMessages(); void api.listChats().then((c) => chats.set(c)).catch(() => {}); }}>
          <div class="tr-text">
            <div class="tr-title">{$t("Seconds in message times")}</div>
            <div class="tr-sub">{$t("12:30:45 instead of 12:30 (for messages from this version on).")}</div>
          </div>
          <span class="switch" class:on={!!$settings.showSeconds}></span>
        </button>
      </section>
      {/if}

      {#if section === "notifications"}
      {#if hasWidget}
      <section class="group">
        <div class="label">{$t("Home-screen widget")}</div>
        <div class="foot-note" style="margin:0 0 8px">{$t("Hold an empty spot on the home screen → Widgets → Aquamarine. It shows the chats with unread messages; with an app passcode — only how many.")}</div>
        <button class="toggle-row" onclick={() => settings.update((x) => ({ ...x, widgetText: !x.widgetText }))}>
          <div class="tr-text"><div class="tr-title">{$t("Message text in the widget")}</div><div class="tr-sub">{$t("Off: only names and counts — anyone who sees your screen won't read your messages")}</div></div>
          <span class="switch" class:on={!!$settings.widgetText}></span>
        </button>
      </section>
      {/if}
      <section class="group">
        <div class="label">{$t("Notifications")}</div>
        <button class="toggle-row" onclick={() => toggle("notifySound")}>
          <div class="tr-text">
            <div class="tr-title">{$t("Sound")}</div>
            <div class="tr-sub">{$t("Play a chime on incoming messages")}</div>
          </div>
          <span class="switch" class:on={$settings.notifySound}></span>
        </button>
        <button class="toggle-row" onclick={enableDesktopNotifications}>
          <div class="tr-text">
            <div class="tr-title">{$t("Desktop notifications")}</div>
            <div class="tr-sub">{$t("Show an OS notification when a message arrives")}</div>
          </div>
          <span class="switch" class:on={$settings.notifyDesktop}></span>
        </button>
        {#if $settings.notifyDesktop}
          <button class="mini-btn" onclick={testNotification}>{$t("Send a test notification")}</button>
          <div class="foot-note">{$t("Notifications come for chats you aren't looking at. If the test doesn't show up, notifications for Aquamarine are off in the system (on Linux — in your shell's notification settings).")}</div>
        {/if}
      </section>
      <section class="group">
        <div class="label">{$t("Sounds")}</div>
        <div class="row">
          <span class="row-label">{$t("Volume")}</span>
          <div class="range">
            <input type="range" min="0" max="1" step="0.05" value={$settings.soundVolume} oninput={(e) => setNum("soundVolume", +(e.target as HTMLInputElement).value)} onchange={() => playSound($settings.messageSound)} />
            <span class="range-val">{Math.round(($settings.soundVolume ?? 0.7) * 100)}%</span>
          </div>
        </div>
        <div class="snd-list" role="radiogroup" aria-label={$t("Message sound")}>
          {#each allSounds as snd (snd.id)}
            <div class="snd" class:on={$settings.messageSound === snd.id}>
              <button class="snd-pick" role="radio" aria-checked={$settings.messageSound === snd.id} onclick={() => { settings.update((s) => ({ ...s, messageSound: snd.id })); void playSound(snd.id); }}>
                <span class="lang-radio" class:on={$settings.messageSound === snd.id}></span>
                <span class="snd-name">{snd.name}</span>
                {#if snd.custom}<span class="snd-tag">{$t("yours")}</span>{/if}
              </button>
              <button class="icon-btn" onclick={() => playSound(snd.id)} title={$t("Listen")} aria-label={$t("Listen")}>▶</button>
              {#if snd.custom}<button class="icon-btn" onclick={() => removeSound(snd.id)} title={$t("Delete")} aria-label={$t("Delete")}>✕</button>{/if}
            </div>
          {/each}
        </div>
        <button class="mini-btn" style="margin-top:10px" onclick={addSounds}>{$t("Add sound files…")}</button>
        <div class="foot-note">{$t("Your sounds live in the app's Sounds folder: mp3, ogg, opus, wav, m4a, flac — up to 3 MB each.")}</div>
      </section>
      <section class="group">
        <div class="label">{$t("Calendar reminders")}</div>
        <div class="row">
          <span class="row-label">{$t("Reminder sound")}</span>
          <Select class="sel" value={$settings.reminderSound ?? "alarm"} options={allSounds.map((snd) => ({ value: snd.id, label: $t(snd.name) }))}
            onchange={(v) => { settings.update((x) => ({ ...x, reminderSound: v })); void playSound(v); }} />
          <button class="icon-btn" onclick={() => playSound($settings.reminderSound ?? "alarm")} aria-label={$t("Listen")}>▶</button>
        </div>
        <div class="foot-note">{$t("Events in chats remind you with a pop-up and this sound — 1 day, 1 hour or 5 minutes before, as you choose on each event.")}</div>
      </section>
      <section class="group">
        <div class="label">{$t("Per-person sounds")}</div>
        {#each Object.entries($settings.chatSounds ?? {}) as [cid, sid] (cid)}
          <div class="row">
            <span class="row-label ell">{$chats.find((c) => c.id === cid)?.name ?? cid}</span>
            <Select class="sel" value={sid} options={[{ value: "", label: $t("Default") }, ...allSounds.map((snd) => ({ value: snd.id, label: $t(snd.name) }))]}
              onchange={(v) => setChatSound(cid, v)} />
            <button class="icon-btn" onclick={() => playSound(sid)} aria-label={$t("Listen")}>▶</button>
          </div>
        {/each}
        <div class="row">
          <Select class="sel" bind:value={perChatPick}
            options={[{ value: "", label: $t("Choose a chat…") }, ...$chats.filter((c) => !($settings.chatSounds ?? {})[c.id]).map((c) => ({ value: c.id, label: c.name }))]} />
          <button class="mini-btn primary" disabled={!perChatPick} onclick={() => { setChatSound(perChatPick, $settings.messageSound === "ding" ? "chime" : "ding"); perChatPick = ""; }}>{$t("Add")}</button>
        </div>
        <div class="foot-note">{$t("Give someone their own sound — you'll know who wrote without looking.")}</div>
      </section>
      {/if}

      {#if section === "voice"}
        <VoiceVideoSettings />
      {/if}

      {#if section === "account"}
      <section class="group">
        <div class="label">{$t("Session")}</div>
        <div class="info-row ue">
          <span class="k">{$t("Username")}</span>
          <div class="v"><UsernameEdit /></div>
        </div>
        <div class="info-row">
          <span class="k">{$t("Server")}</span>
          <span class="v mono">{$info?.server_url ?? "not connected"}</span>
        </div>
        <div class="info-row">
          <span class="k">{$t("Fingerprint")}</span>
          <span class="v mono">{$info?.session_fingerprint ?? "—"}</span>
        </div>
        <div class="info-row">
          <span class="k">{$t("Vault")}</span>
          <span class="v mono dir">{$info?.data_dir ?? "—"}</span>
        </div>
      </section>
      <section class="group">
        <div class="label">{$t("Proxy")}</div>
        <button class="mini-btn" onclick={() => { section = "network"; }}>{$t("Network & proxy")} ›</button>
      </section>
      {/if}

      {#if section === "network"}
        <NetworkSettings />
      {/if}

      {#if section === "devices"}
      <section class="group">
        <div class="label">{$t("Devices")}</div>
        <div class="bots-note">{$t("Your account works on several devices at once: phone, computer, tablet. Each device has its own encryption keys and gets every message. To add one, open Aquamarine there and choose “Sign in” with your e-mail and password.")}</div>
        {#if devErr}
          <div class="bots-note">{$t("Couldn't load the list: {e}", { e: devErr })}</div>
        {:else if !devices}
          <div class="bots-note">…</div>
        {:else}
          <div class="dev-list">
            {#each devices as d (d.id)}
              <div class="dev-row">
                <span class="bk-dot" class:on={d.online}></span>
                <div class="dev-main">
                  {#if devEdit === d.id}
                    <!-- svelte-ignore a11y_autofocus -->
                    <input class="txt dev-input" bind:value={devName} autofocus placeholder={$t("Device name")}
                      onkeydown={(e) => {
                        if (e.key === "Enter" || e.key === "Escape") { e.preventDefault(); e.stopPropagation(); }
                        if (e.key === "Enter") void devRename(d);
                        if (e.key === "Escape") devEdit = null;
                      }}
                      onblur={() => { if (devEdit === d.id) void devRename(d); }} />
                  {:else}
                  <div class="dev-name">
                    {d.name || (d.primary ? $t("Main device") : $t("Device {id}", { id: d.id.slice(0, 6) }))}
                    {#if d.current}<span class="dev-tag">{$t("this device")}</span>{/if}
                    {#if d.primary}<span class="dev-tag dim">{$t("main")}</span>{/if}
                  </div>
                  <div class="dev-sub">{d.online ? $t("online") : $t("offline")}{d.added ? " · " + tr("added {d}", { d: devAdded(d.added) }) : ""} · {d.id}</div>
                  {/if}
                </div>
                {#if !d.primary && devEdit !== d.id}<button class="bot-link" onclick={() => { devName = d.name; devEdit = d.id; }}>{$t("Rename")}</button>{/if}
                {#if !d.current}<button class="bot-link danger" onclick={() => devRemove(d)}>{$t("Sign out")}</button>{/if}
              </div>
            {/each}
          </div>
        {/if}
      </section>
      <section class="group">
        <div class="label">{$t("Handoff")}</div>
        <button class="toggle-row" onclick={() => handoffOn.update((v) => !v)}>
          <div class="tr-text"><div class="tr-title">{$t("Continue on another device")}</div><div class="tr-sub">{$t("When you switch away from Aquamarine here, your other devices offer to open the same chat with what you were typing. Only your devices get it, end-to-end encrypted.")}</div></div>
          <span class="switch" class:on={$handoffOn}></span>
        </button>
      </section>
      {/if}

      {#if section === "storage"}
      <section class="group">
        <div class="label">{$t("Download path")}</div>
        {#if st}
          <button class="radio-row" class:on={st.conf.mode === "default"} onclick={() => stSave({ mode: "default" })}>
            <span class="rd"></span><span class="tr-text"><span class="tr-title">{$t(PHONE_UI ? "In the app" : "Aquamarine folder in the system “Downloads”")}</span><span class="tr-sub mono">{st.default_dir}</span></span>
          </button>
          <button class="radio-row" class:on={st.conf.mode === "temp"} onclick={() => stSave({ mode: "temp" })}>
            <span class="rd"></span><span class="tr-text"><span class="tr-title">{$t("Temporary folder, cleared when you log out")}</span><span class="tr-sub mono">{st.temp_dir}</span></span>
          </button>
          {#if !PHONE_UI}
            <button class="radio-row" class:on={st.conf.mode === "custom"} onclick={() => (st?.conf.custom ? stSave({ mode: "custom" }) : stPickFolder())}>
              <span class="rd"></span><span class="tr-text"><span class="tr-title">{$t("Your folder, cleared only by you")}</span><span class="tr-sub mono">{st.conf.custom || $t("not chosen yet")}</span></span>
              <span class="mini-btn" role="presentation" onclick={(e) => { e.stopPropagation(); void stPickFolder(); }}>{$t("Choose…")}</span>
            </button>
            <button class="toggle-row" onclick={() => settings.update((x) => ({ ...x, askDownloadPath: !x.askDownloadPath }))}>
              <div class="tr-text"><div class="tr-title">{$t("Ask where to save each file")}</div><div class="tr-sub">{$t("After a download, a “Save as” window opens.")}</div></div>
              <span class="switch" class:on={!!$settings.askDownloadPath}></span>
            </button>
          {/if}
          <div class="foot-note">{$t("New files go there; files already saved stay where they are.")}</div>
        {/if}
      </section>
      <section class="group">
        <div class="label">{$t("Automatic media download")}</div>
        <button class="toggle-row" onclick={() => toggle("autoDownloadMedia")}>
          <div class="tr-text"><div class="tr-title">{$t("Auto-download media")}</div><div class="tr-sub">{$t("Fetch images / voice / video automatically on receive")}</div></div>
          <span class="switch" class:on={$settings.autoDownloadMedia}></span>
        </button>
        {#if $settings.autoDownloadMedia}
          {@const ad = { ...AUTODL_DEFAULT, ...($settings.autoDl ?? {}) }}
          {#each [["private", "In private chats"], ["groups", "In groups"], ["channels", "In channels"]] as [k, l] (k)}
            <button class="toggle-row" onclick={() => setAutoDl(k as "private", !ad[k as "private"])}>
              <div class="tr-text"><div class="tr-title">{$t(l)}</div></div>
              <span class="switch" class:on={ad[k as "private"]}></span>
            </button>
          {/each}
          <div class="row">
            <span class="row-label">{$t("Up to")}</span>
            <SegFit label={$t("Up to")} value={ad.limitMb} onpick={(v) => setAutoDl("limitMb", v)}
              options={[1, 5, 25, 100, 500].map((v) => ({ id: v, label: `${v} ${$t("MB")}` }))} />
          </div>
        {/if}
      </section>
      <section class="group">
        <div class="label">{$t("Local storage")}</div>
        {#if stStats}
          <div class="st-top">
            <Donut size={150} title={fmtBytes(stTotal)} sub={tr("{n} files", { n: stStats.count })} fmt={fmtBytes} picked={stOpen?.side === "rx" ? stOpen.kind : ""}
              segs={ST_KINDS.map((k) => ({ id: k.id, label: $t(k.label), color: k.color, value: stStats![k.id] }))}
              onpick={(id) => (stOpen = stOpen?.side === "rx" && stOpen.kind === id ? null : { side: "rx", kind: id })} />
            <div class="st-side">
              <div class="st-cap">{$t("On this device")}</div>
              {#if stTx}
                <Donut size={110} title={fmtBytes(stTxTotal)} sub={$t("sent")} fmt={fmtBytes} picked={stOpen?.side === "tx" ? stOpen.kind : ""}
                  segs={ST_KINDS.filter((k) => k.id !== "cache").map((k) => ({ id: k.id, label: $t(k.label), color: k.color, value: sumOf(stTx!.sent[k.id]) }))}
                  onpick={(id) => (stOpen = stOpen?.side === "tx" && stOpen.kind === id ? null : { side: "tx", kind: id })} />
                <div class="st-cap">{$t("Sent by you")}</div>
              {/if}
            </div>
          </div>
          {#if stOpen && stTx}
            {@const rows = (stOpen.side === "rx" ? stTx.received : stTx.sent)[stOpen.kind] ?? []}
            {@const kk = ST_KINDS.find((k) => k.id === stOpen?.kind)}
            <div class="st-drill" style:--c={kk?.color}>
              <div class="st-drill-h"><i></i><b>{$t(kk?.label ?? "")}</b><span>{stOpen.side === "rx" ? $t("came from") : $t("went to")}</span><button class="x" onclick={() => (stOpen = null)} aria-label={$t("Close")}>×</button></div>
              {#if !rows.length}<div class="foot-note">{$t(stOpen.kind === "cache" ? "Article videos and music played in the app." : "Nothing here yet.")}</div>{/if}
              {#each rows as r (r.chat)}
                <div class="st-drow"><span class="st-dn">{r.name}</span><span class="st-dbar"><span style:width="{Math.max(3, (r.bytes / Math.max(1, rows[0].bytes)) * 100)}%"></span></span><span class="st-dv">{fmtBytes(r.bytes)} · {r.count}</span></div>
              {/each}
            </div>
          {/if}
          {#each ST_KINDS as k (k.id)}
            <button class="st-row" onclick={() => (stPick = { ...stPick, [k.id]: !stPick[k.id] })}>
              <span class="st-check" class:on={stPick[k.id]} style:--c={k.color}>✓</span>
              <span class="st-name">{$t(k.label)}{#if stTotal}<small>{Math.round((stStats[k.id] / stTotal) * 100)}%</small>{/if}</span>
              <span class="st-size">{fmtBytes(stStats[k.id])}</span>
            </button>
          {/each}
          <button class="st-clear" disabled={stBusy || !stSelected} onclick={stClear}>{$t("Clear")} {fmtBytes(stSelected)}</button>
          <div class="foot-note">{$t("Only files Aquamarine saved itself are deleted. In chats they can be downloaded again while the server keeps them.")}</div>
        {:else}
          <div class="foot-note">{$t("Counting…")}</div>
        {/if}
      </section>
      {#if st}
      <section class="group">
        <div class="label">{$t("Limits")}</div>
        <div class="row">
          <span class="row-label">{$t("Total size")}</span>
          <SegFit label={$t("Total size")} value={st.conf.total_mb} onpick={(v) => stSave({ total_mb: v })}
            options={[[1024, "1 GB"], [2048, "2 GB"], [5120, "5 GB"], [10240, "10 GB"], [0, "∞"]].map(([id, l]) => ({ id: id as number, label: String(l) }))} />
        </div>
        <div class="row">
          <span class="row-label">{$t("Media cache")}</span>
          <SegFit label={$t("Media cache")} value={st.conf.cache_mb} onpick={(v) => stSave({ cache_mb: v })}
            options={[[256, "256 MB"], [512, "512 MB"], [1024, "1 GB"], [4096, "4 GB"], [0, "∞"]].map(([id, l]) => ({ id: id as number, label: String(l) }))} />
        </div>
        <div class="row">
          <span class="row-label">{$t("Clear files older than")}</span>
          <SegFit label={$t("Clear files older than")} value={st.conf.max_days} onpick={(v) => stSave({ max_days: v })}
            options={[[7, "1 week"], [30, "1 month"], [90, "3 months"], [365, "1 year"], [0, "Never"]].map(([id, l]) => ({ id: id as number, label: $t(String(l)) }))} />
        </div>
        <div class="foot-note">{$t("When files take more, the ones unused the longest go first.")}</div>
      </section>
      {/if}
      <section class="group">
        <div class="label">{$t("Export")}</div>
        <button class="mini-btn" onclick={exportChats}>{$t("Export my chats (JSON)")}</button>
        <div class="foot-note">{$t("All your chats with texts, times and names of files — to keep or move elsewhere. The file isn't encrypted: keep it safe.")}</div>
      </section>
      {/if}

      {#if section === "advanced"}
      <section class="group">
        <div class="label">{$t("Experimental")}</div>
        <div class="bots-note">{$t("Features that are still being tried out. They may change, break or disappear in the next versions.")}</div>
        <button class="toggle-row" onclick={() => settings.update((x) => ({ ...x, floatKb: !x.floatKb }))}>
          <div class="tr-text"><div class="tr-title">{$t("Floating bot keyboard")}</div>
            <div class="tr-sub">{$t("A bot's buttons float over the chat: drag them anywhere by the strip on top, resize by the corner or with two fingers (double-tap the strip — normal size).")}</div></div>
          <span class="switch" class:on={!!$settings.floatKb}></span>
        </button>
      </section>
      <section class="group">
        <div class="label">🧩 {$t("Plugins")}</div>
        <button class="toggle-row" onclick={() => pluginsOn.update((v) => !v)}>
          <div class="tr-text"><div class="tr-title">{$t("Plugins")}</div>
            <div class="tr-sub">{$t("Small JavaScript add-ons, like in exteraGram: commands, changing what you send, reacting to messages.")}</div></div>
          <span class="switch" class:on={$pluginsOn}></span>
        </button>
        {#if $pluginsOn}
          <div class="bots-note">{$t("A plugin sees the messages it's given, but runs locked away: no network, no access to your keys, files or the rest of the app. Install only plugins you trust.")}</div>
          {#each $plugins as pl (pl.id)}
            <div class="toggle-row am-row" class:off={!pl.enabled}>
              <div class="tr-text"><div class="tr-title">{pl.name} <span class="pill-tag">{pl.version}</span></div>
                <div class="tr-sub">{pl.description || "—"}{pl.author ? ` · ${pl.author}` : ""}</div></div>
              <button class="mini-btn" onclick={() => { if (confirm(tr("Remove the plugin “{n}”?", { n: pl.name }))) removePlugin(pl.id); }}>{$t("Remove")}</button>
              <button class="switch-btn" onclick={() => togglePlugin(pl.id)} aria-label={$t("On")}><span class="switch" class:on={pl.enabled}></span></button>
            </div>
          {:else}
            <div class="foot-note" style="margin:0 0 8px">{$t("No plugins yet.")}</div>
          {/each}
          <div class="pl-acts">
            <label class="mini-btn">{$t("Install from file…")}
              <input type="file" accept=".js,.mjs,.txt,text/javascript" hidden onchange={async (e) => {
                const f = (e.currentTarget as HTMLInputElement).files?.[0];
                if (!f) return;
                try { const p = installPlugin(await f.text()); showToast(tr("Plugin “{n}” installed", { n: p.name })); } catch (err) { showToast(String(err)); }
                (e.currentTarget as HTMLInputElement).value = "";
              }} />
            </label>
            <button class="mini-btn" onclick={() => (pluginPaste = pluginPaste == null ? "" : null)}>{$t("Paste code")}</button>
            <button class="mini-btn" onclick={() => { const p = installPlugin(EXAMPLE_PLUGIN); showToast(tr("Plugin “{n}” installed", { n: p.name })); }}>{$t("Example plugin")}</button>
          </div>
          {#if pluginPaste != null}
            <textarea class="pl-code" rows="8" spellcheck="false" placeholder={"// @name My plugin\naqua.command(\"hi\", \"Say hi\", () => \"Hi!\");"} bind:value={pluginPaste}></textarea>
            <button class="mini-btn" disabled={!pluginPaste.trim()} onclick={() => { try { const p = installPlugin(pluginPaste ?? ""); pluginPaste = null; showToast(tr("Plugin “{n}” installed", { n: p.name })); } catch (err) { showToast(String(err)); } }}>{$t("Install")}</button>
          {/if}
        {/if}
      </section>
      {/if}

      {#if section === "power"}
      <section class="group">
        <div class="label">{$t("Power saving mode")} <span class="pill-tag" class:on={$powerActive}>{$powerActive ? $t("ON") : $t("OFF")}</span></div>
        <div class="pw-scale"><span>{$t("Off")}</span><span class="mid">{pwMode > 0 && pwMode < 100 ? $t("When below {n}%", { n: pwMode }) : ""}</span><span>{$t("On")}</span></div>
        <div class="range"><input type="range" min="0" max="100" step="5" value={pwMode}
          oninput={(e) => settings.update((x) => ({ ...x, powerMode: parseInt((e.target as HTMLInputElement).value) }))} /></div>
        <div class="foot-note">{pwMode >= 100 ? $t("Power saving is always on: less movement, less battery and CPU.") : pwMode <= 0 ? $t("Power saving is off. Each switch below decides by itself.") : $battery ? $t("Automatically reduce power usage and animations when your battery is below {n}% (now {b}%{c}).", { n: pwMode, b: $battery.level, c: $battery.charging ? ", " + tr("charging") : "" }) : $t("This device doesn't tell the battery level — the mode stays off until you set it to On.")}</div>
      </section>
      <section class="group" class:pw-dim={$powerActive}>
        <div class="label">{$t("Power saving options")}</div>
        {#each [["stickers", "Animated stickers"], ["emoji", "Animated emoji"], ["chat", "Effects in chats"], ["calls", "Animations in calls"]] as [k, l] (k)}
          <button class="toggle-row" onclick={() => setPower(k as "ui", !pw[k as "ui"])}>
            <div class="tr-text"><div class="tr-title">{$t(l)}</div></div>
            <span class="switch" class:on={pw[k as "ui"]}></span>
          </button>
        {/each}
        <button class="toggle-row" onclick={() => setFlag("autoplayVideos")}>
          <div class="tr-text"><div class="tr-title">{$t("Autoplay videos")}</div></div>
          <span class="switch" class:on={$settings.autoplayVideos}></span>
        </button>
        <button class="toggle-row" onclick={() => setFlag("autoplayGifs")}>
          <div class="tr-text"><div class="tr-title">{$t("Autoplay GIFs")}</div></div>
          <span class="switch" class:on={$settings.autoplayGifs}></span>
        </button>
        <button class="toggle-row" onclick={() => setPower("particles", !pw.particles)}>
          <div class="tr-text"><div class="tr-title">{$t("Particles")}</div><div class="tr-sub">{$t("The glows moving behind the app.")}</div></div>
          <span class="switch" class:on={pw.particles}></span>
        </button>
      </section>
      <section class="group">
        <button class="toggle-row" onclick={() => setPower("ui", !pw.ui)}>
          <div class="tr-text"><div class="tr-title">{$t("Enable smooth transitions")}</div></div>
          <span class="switch" class:on={pw.ui}></span>
        </button>
        <div class="foot-note">{$t("You can disable animated transitions between different sections of the app. Stickers and emoji that don't play by themselves play on hover.")}</div>
      </section>
      {/if}

      {#if section === "window"}
      <section class="group">
        <div class="label">{$t("Window title bar")}</div>
        {#each [["titleChatName", "Show chat name", true], ["titleAccount", "Show active account", false], ["titleUnread", "Total unread count", true]] as [k, l, def] (k)}
          {@const on = ($settings as unknown as Record<string, boolean | undefined>)[k as string] ?? def}
          <button class="toggle-row" onclick={() => settings.update((x) => ({ ...x, [k as string]: !on }))}>
            <div class="tr-text"><div class="tr-title">{$t(String(l))}</div></div>
            <span class="switch" class:on={on}></span>
          </button>
        {/each}
      </section>
      <section class="group">
        <div class="label">{$t("System integration")}</div>
        <button class="toggle-row" onclick={() => settings.update((x) => ({ ...x, tray: !x.tray }))}>
          <div class="tr-text"><div class="tr-title">{$t("Show tray icon")}</div><div class="tr-sub">{$t("Closing the window then hides it to the tray; “Quit” in the icon's menu closes the app.")}</div></div>
          <span class="switch" class:on={!!$settings.tray}></span>
        </button>
        {#if $settings.tray}
          <button class="toggle-row" onclick={() => settings.update((x) => ({ ...x, trayMono: !x.trayMono }))}>
            <div class="tr-text"><div class="tr-title">{$t("Use monochrome icon")}</div></div>
            <span class="switch" class:on={!!$settings.trayMono}></span>
          </button>
        {/if}
        <button class="toggle-row" onclick={toggleAutostart}>
          <div class="tr-text"><div class="tr-title">{$t("Launch Aquamarine when the system starts")}</div></div>
          <span class="switch" class:on={autostart}></span>
        </button>
        <button class="toggle-row" onclick={() => settings.update((x) => ({ ...x, restoreWindows: !x.restoreWindows }))}>
          <div class="tr-text"><div class="tr-title">{$t("Restore last opened windows")}</div><div class="tr-sub">{$t("Chats opened in their own windows open again on start.")}</div></div>
          <span class="switch" class:on={!!$settings.restoreWindows}></span>
        </button>
      </section>
      {/if}

      {#if section === "host"}
      <section class="group">
        <div class="label">{$t("Your own server, right here")}</div>
        <div class="bots-note">{$t("This device becomes an Aquamarine server: you and your friends sign in to its address. Messages stay end-to-end encrypted — the server only passes sealed envelopes along. Keep the device on and connected; on a phone a notification shows while it runs.")}</div>
        {#if host && host.dir_chosen === false && !host.running}
          <div class="host-first">
            <b>{$t("Where should the server keep its data?")}</b>
            <span>{$t("Accounts, encrypted files and settings of the server go to one folder. You can move it later.")}</span>
            <div class="host-first-btns">
              <button class="mini-btn primary" onclick={() => hostSetDir("", false)}>{$t("In the app's folder")}</button>
              {#if !PHONE_UI}<button class="mini-btn" onclick={hostPickDir}>{$t("Choose a folder…")}</button>{/if}
            </div>
            <code class="host-dir">{host.dir}</code>
          </div>
        {/if}
        <button class="toggle-row" disabled={hostBusy || (host?.dir_chosen === false && !host?.running)} onclick={hostToggle}>
          <div class="tr-text">
            <div class="tr-title">{$t("Run the server")}</div>
            <div class="tr-sub">{hostBusy ? "…" : host?.running ? $t("Running on port {p}.", { p: host.port }) : host?.error ? host.error : $t("Off.")}</div>
          </div>
          <span class="switch" class:on={host?.running}></span>
        </button>
        {#if !host?.running}
          <label class="host-port">{$t("Port")} <input class="txt" type="number" min="1024" max="65535" bind:value={hostPort} /></label>
        {/if}
        {#if host && host.dir_chosen !== false}
          <div class="host-addr"><span>{$t("Data folder")}</span><code title={host.dir}>{host.dir}</code>
            {#if !PHONE_UI}<button class="mesh-btn" disabled={host.running} title={host.running ? $t("Stop the server first") : ""} onclick={hostPickDir}>{$t("Change…")}</button>{/if}
          </div>
          {#if !PHONE_UI && !host.running}<div class="bots-note">{$t("Changing it copies the server's data to the new folder; the old one stays until you delete it.")}</div>{/if}
        {/if}
      </section>
      {#if host?.running}
      <section class="group">
        <div class="label">{$t("Addresses")}</div>
        <div class="host-addr"><span>{$t("Same Wi-Fi")}</span><code>{host.lan_url ?? "—"}</code><button class="mesh-btn" onclick={() => hostCopy(host?.lan_url ?? null)}>{$t("Copy")}</button></div>
        <div class="host-addr"><span>{$t("Internet")}</span><code>{host.public_url ?? $t("looking it up…")}</code><button class="mesh-btn" onclick={() => hostCopy(host?.public_url ?? null)} disabled={!host.public_url}>{$t("Copy")}</button></div>
        <div class="bots-note">{$t("The internet address works once the router forwards TCP port {p} to this device (and the provider gives you a white IP). On a phone, mobile networks almost never allow it — use Wi-Fi.", { p: host.port })}</div>
        {#if host.admin_url}
          <button class="bot-btn" onclick={() => void api.openExternal(host!.admin_url!).catch(() => hostCopy(host?.admin_url ?? null))}>{$t("Open the admin panel")}</button>
        {/if}
      </section>
      {/if}
      {/if}

      {#if section === "nearby"}
      {@const ms = $meshStatus}
      <section class="group">
        <div class="label">{$t("Wi-Fi and Bluetooth, like Briar")}</div>
        <div class="bots-note">{$t("No internet or the server is down? Messages to your contacts wait on this device, and as soon as their phone or computer is on the same Wi-Fi (or hotspot) or within Bluetooth range, the two devices swap them directly. When the server is back, whatever is still waiting goes through it. Everything stays end-to-end encrypted; only contacts you've already chatted with can connect.")}</div>
        <button class="toggle-row" onclick={() => meshSetEnabled(!(ms?.enabled ?? true))}>
          <div class="tr-text"><div class="tr-title">{$t("Deliver without internet")}</div><div class="tr-sub">{ms?.ready ? $t("Listening for contacts nearby.") : $t("Not listening yet.")}</div></div>
          <span class="switch" class:on={ms?.enabled ?? true}></span>
        </button>
        <div class="mesh-ways">
          <span class="mesh-way" class:on={ms?.lan_ready}><span class="bk-dot" class:on={ms?.lan_ready}></span>Wi-Fi / {$t("local network")}</span>
          <span class="mesh-way" class:on={ms?.bt_ready}><span class="bk-dot" class:on={ms?.bt_ready}></span>Bluetooth{#if ms && !ms.bt_supported} · {$t("not on this platform yet")}{:else if ms && !ms.bt_ready && ms.note} · {$t(ms.note)}{/if}</span>
        </div>
        <div class="mesh-row">
          <div class="mesh-big">{ms?.queued ?? 0}</div>
          <div class="mesh-txt">
            <div class="tr-title">{$t("waiting to be delivered")}</div>
            <div class="tr-sub">{Object.entries(ms?.per_chat ?? {}).map(([u, n]) => `${u} · ${n}`).join(", ") || $t("Nothing is waiting.")}</div>
          </div>
          <button class="mesh-btn" disabled={meshBusy || !(ms?.supported) || !(ms?.enabled ?? true)} onclick={async () => { meshBusy = true; const e = await meshSyncNow(); meshBusy = false; showToast(e ? tr(e.replace(/^Error: /, "")) : tr("Done")); }}>
            {meshBusy || ms?.syncing ? $t("Searching…") : $t("Find contacts nearby")}
          </button>
        </div>
      </section>
      {#if ms?.last?.length}
      <section class="group">
        <div class="label">{$t("Recent meetings")}</div>
        {#each ms.last as r}
          <div class="mesh-log">
            <span class="bk-dot" class:on={!r.error}></span>
            <span class="mesh-who">{r.peer ? r.peer.split("~")[0] : r.device}</span>
            <span class="mesh-what">{r.error ? $t(r.error) : `↑${r.sent} ↓${r.received}`}</span>
            <span class="mesh-when">{new Date(r.at * 1000).toLocaleString([], { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" })}</span>
          </div>
        {/each}
      </section>
      {/if}
      <section class="group">
        <div class="label">{$t("How to start")}</div>
        <div class="bots-note">{$t("1. Chat with the person at least once through a server — that's when your devices exchange keys. 2. Be on the same Wi-Fi (a phone's hotspot works without internet), or keep Bluetooth on: Android finds nearby phones by itself; on Linux and Windows pair the devices once in the system Bluetooth settings. 3. Messages are swapped automatically every couple of minutes while the server is away, or right now with “Find contacts nearby”.")}</div>
      </section>
      {/if}

      {#if section === "backups"}
      <section class="group">
        <div class="label">{$t("Encrypted backup")}</div>
        <div class="bots-note">{$t("Your keys, chats, groups and settings are sealed on this device and kept on your server — and, in a federated network, copied to every other server within seconds. None of them can read it. Sign in on a new device with your e-mail and password and everything comes back. Photos and videos are not included.")}</div>
        {#if !$backupState?.enabled}
          <div class="bk-row">
            <input class="txt" type="password" placeholder={$t("Account password")} bind:value={bkPw} onkeydown={(e) => { if (e.key === "Enter") void bkEnable(); }} />
            <button class="bot-btn primary" onclick={bkEnable} disabled={bkBusy || bkPw.length < 8}>{bkBusy ? "…" : $t("Turn on")}</button>
          </div>
          <div class="bots-note">{$t("Use the password of your account: it opens the backup on a new device.")}</div>
        {:else}
          <div class="bk-stat">
            <span class="bk-dot on"></span>
            <span>{$backupState.last ? tr("Last backup: {when} · {size}", { when: fmtWhen($backupState.last.at), size: fmtBytes($backupState.last.size) }) : $t("On — no backup uploaded yet")}</span>
          </div>
          <div class="bot-actions">
            <button class="bot-btn primary" onclick={bkNow} disabled={bkBusy || !$info?.connected}>{bkBusy ? "…" : $t("Back up now")}</button>
            <span class="bot-sp"></span>
            <button class="bot-link danger" onclick={bkDisable}>{$t("Turn off")}</button>
          </div>
          <div class="bots-note">{$t("Uploaded by itself about 20 seconds after an important change (groups, contacts, profile, settings), after new messages at most every 20 minutes, and at least every 6 hours.")}</div>
        {/if}
      </section>
      <section class="group">
        <div class="label">{$t("Recovery key")}</div>
        <div class="bots-note">{$t("24 words that reset a forgotten password and open your encrypted backup. The server keeps only a check derived from them — it can't reproduce the words.")}</div>
        <div class="bk-stat">
          <span class="bk-dot" class:on={rc?.server === true}></span>
          <span>
            {#if rc?.server === true}{$t("Set — a forgotten password is reset with the e-mail code plus these words")}
            {:else if rc?.server === false}{$t("Not set — whoever gets into your e-mail can reset the password")}
            {:else}{$t("Connect to the server to check")}{/if}
          </span>
        </div>
        <div class="bot-actions">
          <button class="bot-btn primary" onclick={rcShow} disabled={!$info?.connected && !rc?.local}>{rc?.local ? $t("Show recovery key") : $t("Create recovery key")}</button>
        </div>
        {#if bkPhrase}
          <div class="bk-phrase">
            <div class="bk-ph-title">{$t("Recovery key — write it down and keep it offline")}</div>
            <ol>{#each bkPhrase.split(" ") as w, i (i)}<li>{w}</li>{/each}</ol>
            <div class="bots-note">{$t("If you forget the password: Sign in → Forgot password?, the e-mail code and these 24 words. Nobody, including the server admin, can recover the backup without the password or the words.")}</div>
            <button class="bot-link" onclick={() => navigator.clipboard?.writeText(bkPhrase ?? "").then(() => showToast(tr("Recovery key copied")))}>{$t("Copy")}</button>
            <button class="bot-link" onclick={() => (bkPhrase = null)}>{$t("Hide")}</button>
          </div>
        {/if}
      </section>
      {/if}

      {#if section === "bots"}
      <section class="group">
        <div class="label">{$t("Your bots")}</div>
        {#if !$info?.connected}
          <div class="bots-note">{$t("Connect to a server to create bots.")}</div>
        {:else}
          <form class="bot-create" onsubmit={(e) => { e.preventDefault(); void createBot(); }}>
            <label class="bc-field">
              <span>{$t("Username")}</span>
              <div class="bc-at"><i>@</i><input bind:value={botUsername} placeholder="weatherbot" spellcheck="false" autocomplete="off" oninput={() => (botUsername = botUsername.toLowerCase())} /></div>
            </label>
            <label class="bc-field">
              <span>{$t("Display name")}</span>
              <input bind:value={botDisplay} placeholder={$t("Weather")} />
            </label>
            <button class="bot-add" type="submit" disabled={botBusy || !botUsername.trim()}>{botBusy ? "…" : $t("Create")}</button>
          </form>
          {#if botError}<div class="bots-note err">{botError}</div>{/if}
          {#if bots.length === 0}
            <div class="bots-note">{$t("No bots yet. A bot is an account driven by a program over HTTP — create one and you get its token.")}</div>
          {:else}
            <div class="bot-list">
            {#each bots as b (b.username)}
              <div class="bot-card">
                <div class="bot-head">
                  <div class="bot-ava">{(b.display_name || b.username).slice(0, 1).toUpperCase()}</div>
                  <div class="bot-main">
                    <div class="bot-name">{b.display_name || b.username} <span class="bot-tag">BOT</span></div>
                    <div class="bot-user">@{b.username}</div>
                  </div>
                  <button class="bot-btn primary" onclick={() => openBotChat(b)}>{$t("Open chat")}</button>
                </div>
                <div class="bot-tokrow">
                  <code class="bot-token" title={botShown.has(b.username) ? b.token : ""}>{botShown.has(b.username) && b.token ? b.token : maskToken(b.token, b.hint)}</code>
                  {#if b.token}
                    <button class="bot-btn" onclick={() => toggleToken(b.username)}>{botShown.has(b.username) ? $t("Hide") : $t("Show")}</button>
                    <button class="bot-btn" onclick={() => copyToken(b.token)}>{$t("Copy")}</button>
                    <button class="bot-btn" title={$t("For bots written for Telegram")} onclick={() => copyTgToken(b.username, b.token)}>{$t("Telegram token")}</button>
                  {/if}
                </div>
                {#if !b.token}<div class="bots-note">{$t("The server keeps only a fingerprint of the token, and this device doesn't have it. Reset the token to get a new one.")}</div>{/if}
                <div class="bot-actions">
                  <span class="bot-sp"></span>
                  <button class="bot-link" onclick={() => resetBotToken(b)}>{$t("Reset token")}</button>
                  <button class="bot-link danger" onclick={() => deleteBot(b)}>{$t("Delete")}</button>
                </div>
              </div>
            {/each}
            </div>
          {/if}
        {/if}
      </section>

      <section class="group">
        <div class="label">{$t("Bots written for Telegram")}</div>
        <div class="bots-note">{$t("This server speaks the Telegram Bot API too: an existing Telegram bot works here unchanged — copy its “Telegram token” above and point the library at this server instead of api.telegram.org. Inline buttons appear as a numbered list; the user answers with the number and the bot gets the button press.")}</div>
        <div class="bot-cmd"><code>python-telegram-bot: .base_url("{tgBase}/bot")</code><button class="bot-btn" onclick={() => copyText(`.base_url("${tgBase}/bot")`, tr("Copied"))}>{$t("Copy")}</button></div>
        <div class="bot-cmd"><code>aiogram: TelegramAPIServer.from_base("{tgBase}")</code><button class="bot-btn" onclick={() => copyText(`TelegramAPIServer.from_base("${tgBase}")`, tr("Copied"))}>{$t("Copy")}</button></div>
        <div class="bot-cmd"><code>telegraf / grammY: apiRoot: "{tgBase}"</code><button class="bot-btn" onclick={() => copyText(`apiRoot: "${tgBase}"`, tr("Copied"))}>{$t("Copy")}</button></div>
      </section>

      <section class="group">
        <div class="label">{$t("Console chat")}</div>
        <div class="bots-note">{$t("A ready-made bot: it prints a chat in your terminal and sends whatever you type. Write to your bot in Aquamarine (or let friends write to it) and answer from the console. Room mode forwards every message to everyone who joined — a tiny chat room. Needs only Python 3.")}</div>
        <div class="bot-cmd">
          <code>{consoleCmd}</code>
          <button class="bot-btn" onclick={() => copyText(consoleCmd, tr("Command copied"))}>{$t("Copy")}</button>
        </div>
        <div class="bot-actions">
          <button class="bot-btn primary" onclick={saveConsoleScript}>{$t("Save aqua-console.py…")}</button>
          <button class="bot-link" onclick={() => api.openExternal("https://github.com/applememestm-pixel/Aquamarine/blob/main/tools/aqua-console.py").catch(() => {})}>{$t("View source")}</button>
        </div>
        <div class="bots-note">{$t("It asks for the token with hidden input. Commands: /to <user>, /who, /room, /quit. Bot chats are not end-to-end encrypted.")}</div>
      </section>

      <section class="group">
        <button class="bot-api-toggle" onclick={() => (apiOpen = !apiOpen)} aria-expanded={apiOpen}>
          <span>{$t("HTTP API for your own bots")}</span><span class="chev" class:open={apiOpen}>›</span>
        </button>
        {#if apiOpen}
<pre class="bot-api">Authorization: Bot TOKEN
GET  /v1/bot/me
     → {"{"} "username", "display_name" {"}"}
GET  /v1/bot/updates?after=ID&amp;timeout=25
     → [{"{"} "id", "from", "text", "ts" {"}"}]   (long poll, pass last id + 1)
POST /v1/bot/send   {"{"} "to": "alice", "text": "hi" {"}"}
     403 — they never wrote to the bot · 429 — 30/s per bot, 1/s per chat</pre>
          <div class="bots-note">{$t("Users start a bot by searching its @username; the first message is /start.")}</div>
        {/if}
      </section>
      {/if}

      {#if section === "updates"}
      <section class="group">
        <div class="label">{$t("Updates")}</div>
        <div class="info-row"><span class="k">{$t("This build")}</span><span class="v mono">{BUILD.version || "—"}{BUILD_SHORT ? ` · ${BUILD_SHORT}` : ""} · {BUILD.channel}</span></div>
        <div class="row">
          <span class="row-label">{$t("Channel")}</span>
          <SegFit label={$t("Channel")} value={$updatePrefs.channel} onpick={(c) => { updatePrefs.update((p) => ({ ...p, channel: c, skip: undefined })); void checkUpdates(true); }}
            options={[{ id: "nightly" as const, label: $t("Every build") }, { id: "stable" as const, label: $t("Releases only") }]} />
        </div>
        <button class="toggle-row" onclick={() => updatePrefs.update((p) => ({ ...p, auto: !p.auto }))}>
          <div class="tr-text"><div class="tr-title">{$t("Check for updates")}</div><div class="tr-sub">{$t("On start and every 6 hours; a card offers the new build")}</div></div>
          <span class="switch" class:on={$updatePrefs.auto}></span>
        </button>
        <div class="info-row">
          <span class="v">
            {#if $updateState.s === "checking"}{$t("Checking…")}
            {:else if $updateState.s === "none"}{$t("You have the newest build")}
            {:else if $updateState.s === "available" || $updateState.s === "downloading" || $updateState.s === "installing" || $updateState.s === "permission"}{$t("A new build is available — see the card at the top")}
            {:else if $updateState.s === "error"}<span class="upd-err">{$updateState.error}</span>
            {/if}
          </span>
          <button class="mini-btn" onclick={() => checkUpdates(true)} disabled={$updateState.s === "checking" || $updateState.s === "downloading"}>{$t("Check now")}</button>
        </div>
        <div class="foot-note">{$t("The file is checked against the release's SHA-256 list before it's installed. On Android an update installs over the app only when both are signed with the same key.")}</div>
      </section>
      <section class="group">
        <div class="label">{$t("What's new")}</div>
        <button class="toggle-row" onclick={() => settings.update((x) => ({ ...x, whatsNew: x.whatsNew === false }))}>
          <div class="tr-text"><div class="tr-title">{$t("Show what's new after updates")}</div><div class="tr-sub">{$t("A window with the release notes once after each update.")}</div></div>
          <span class="switch" class:on={$settings.whatsNew !== false}></span>
        </button>
        {#if NOTES.md}<button class="mini-btn" onclick={() => { closeSettings(); openWhatsNew(); }}>{$t("Open what's new")} · {NOTES.version}</button>{/if}
      </section>
      {/if}

      {#if section === "about"}
      <section class="group">
        <div class="label">{$t("About")}</div>
        <div class="about">
          <b>{$t("Aquamarine")}</b> {$t("— an end-to-end encrypted messenger.")} <span class="ab-ver">{BUILD.version || ""}</span><br />
          {$t("X3DH · Double Ratchet · ChaCha20-Poly1305 · your keys never leave this device.")}
        </div>
      </section>
      <section class="group">
        <div class="label">{$t("Links")}</div>
        {#each LINKS as l (l.id)}
          <button class="toggle-row ab-link" onclick={() => void api.openExternal(l.url).catch((e) => showToast(String(e)))}>
            <span class="ab-ic"><svg viewBox="0 0 24 24">{@html l.icon}</svg></span>
            <div class="tr-text"><div class="tr-title">{l.label}</div><div class="tr-sub">{$t(l.sub)} · {l.url.replace(/^https?:\/\//, "")}</div></div>
            <span class="ab-go">›</span>
          </button>
        {/each}
      </section>
      <section class="group">
        <button class="reset" onclick={reset}>{$t("Reset all settings to defaults")}</button>
      </section>
      {/if}

        </div>
      </div>

      <div class="foot">
        <button class="done" onclick={closeAll}>{$t("Done")}</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .radio-row { width: 100%; display: flex; align-items: center; gap: 12px; padding: 9px 4px; border: none; background: none; color: var(--fg); font: inherit; text-align: left; cursor: pointer; border-radius: calc(10px * var(--rk, 1)); }
  .radio-row:hover { background: var(--bg-hover); }
  .radio-row .rd { width: 18px; height: 18px; border-radius: 50%; border: 2px solid var(--fg-dim); flex-shrink: 0; position: relative; }
  .radio-row.on .rd { border-color: var(--accent); }
  .radio-row.on .rd::after { content: ""; position: absolute; inset: 3px; border-radius: 50%; background: var(--accent); }
  .radio-row .tr-text { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .tr-sub.mono { font-family: var(--font-mono); font-size: 11px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .st-top { display: flex; align-items: center; gap: 18px; margin: 4px 0 10px; }
  .st-top { flex-wrap: wrap; justify-content: center; }
  .st-side { display: flex; flex-direction: column; align-items: center; gap: 6px; }
  .st-cap { font-size: 11px; font-weight: 700; text-transform: uppercase; letter-spacing: .06em; color: var(--fg-dim); }
  .st-drill { margin: 4px 0 10px; padding: 10px 12px; border-radius: calc(12px * var(--rk, 1)); background: color-mix(in srgb, var(--c) 10%, var(--bg-input));
    border: 1px solid color-mix(in srgb, var(--c) 35%, transparent); animation: drill-in .2s ease-out; }
  @keyframes drill-in { from { opacity: 0; transform: translateY(-4px); } }
  .st-drill-h { display: flex; align-items: center; gap: 8px; margin-bottom: 6px; font-size: 13.5px; }
  .st-drill-h i { width: 10px; height: 10px; border-radius: 3px; background: var(--c); }
  .st-drill-h span { color: var(--fg-dim); flex: 1; } .st-drill-h .x { border: none; background: none; color: var(--fg-dim); font-size: 18px; cursor: pointer; }
  .st-drow { display: flex; align-items: center; gap: 10px; padding: 4px 0; font-size: 12.5px; }
  .st-dn { flex: 0 0 34%; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .st-dbar { flex: 1; height: 6px; border-radius: 3px; background: var(--bg-shell); overflow: hidden; }
  .st-dbar span { display: block; height: 100%; background: var(--c); border-radius: 3px; transition: width .3s; }
  .st-dv { color: var(--fg-dim); font-variant-numeric: tabular-nums; white-space: nowrap; }
  .st-row { width: 100%; display: flex; align-items: center; gap: 10px; padding: 7px 4px; border: none; background: none; color: var(--fg); font: inherit; cursor: pointer; text-align: left; }
  .st-check { width: 18px; height: 18px; border-radius: 50%; display: grid; place-items: center; font-size: 11px; font-weight: 900; color: transparent; border: 2px solid var(--c); flex-shrink: 0; }
  .st-check.on { background: var(--c); color: #fff; }
  .st-name { flex: 1; font-size: 13.5px; } .st-name small { margin-left: 6px; color: var(--fg-dim); font-weight: 700; }
  .st-size { color: var(--accent); font-size: 12.5px; font-variant-numeric: tabular-nums; }
  .st-clear { width: 100%; margin-top: 8px; padding: 10px; border: none; border-radius: calc(12px * var(--rk, 1)); background: var(--accent); color: #06070C; font: inherit; font-weight: 800; cursor: pointer; }
  .st-clear:disabled { opacity: .45; cursor: default; }
  .pill-tag { font-size: 10px; font-weight: 800; padding: 1px 6px; border-radius: 5px; background: var(--accent); color: #06070C; margin-left: 6px; text-transform: uppercase; }
  .ava-row { display: flex; align-items: center; gap: 14px; margin: 8px 0; }
  .ava-ctl { flex: 1; min-width: 0; } .ava-head { display: flex; align-items: center; gap: 6px; font-size: 13.5px; }
  .ava-demo.ava-g { background: linear-gradient(135deg, #ff7cac, #7b5cff); } .ava-demo.ava-c { background: linear-gradient(135deg, #ffb067, #ff5f6d); }
  .ava-prev { display: flex; align-items: center; gap: 12px; margin: 6px 0 10px; }
  .ava-prev .tr-text { display: flex; flex-direction: column; } .ava-prev small { color: var(--fg-dim); }
  .ava-demo { width: 48px; height: 48px; flex-shrink: 0; display: grid; place-items: center; font-weight: 800; color: #fff; border-radius: var(--ava-r, 50%);
    background: linear-gradient(135deg, var(--accent), var(--accent-2)); }
  .host-first { display: flex; flex-direction: column; gap: 6px; padding: 12px; margin: 6px 0 10px; border-radius: calc(12px * var(--rk, 1));
    background: var(--accent-soft); border: 1px solid color-mix(in srgb, var(--accent) 30%, transparent); font-size: 13px; }
  .host-first span { color: var(--fg-muted); }
  .host-first-btns { display: flex; gap: 8px; flex-wrap: wrap; margin-top: 4px; }
  .host-dir { font-size: 11.5px; color: var(--fg-dim); word-break: break-all; }
  .backdrop {
    position: fixed; inset: 0;
    background: rgba(6, 7, 12, 0.55);
    backdrop-filter: blur(4px);
    display: grid; place-items: center;
    z-index: 260; /* above the call overlay (250), below toasts (300) */
    animation: fade-in 0.15s ease-out;
    padding: 20px;
  }
  @keyframes fade-in { from { opacity: 0; } to { opacity: 1; } }
  .sz-grip { position: absolute; right: 2px; bottom: 2px; width: 18px; height: 18px; z-index: 5; cursor: nwse-resize; opacity: .5;
    background: linear-gradient(135deg, transparent 50%, var(--fg-dim) 50%, var(--fg-dim) 58%, transparent 58%, transparent 70%, var(--fg-dim) 70%, var(--fg-dim) 78%, transparent 78%); border-bottom-right-radius: 14px; }
  .sz-grip:hover { opacity: 1; }
  .sheet { position: relative; }
  @media (max-width: 720px) { .sz-grip { display: none; } }
  .sheet {
    width: min(940px, 96vw);
    height: min(680px, 90vh);
    display: flex; flex-direction: column;
    background: var(--bg-shell);
    border: 1px solid var(--border);
    border-radius: calc(16px * var(--rk, 1));
    box-shadow: 0 32px 80px -16px rgba(0, 0, 0, 0.6);
    overflow: hidden;
    animation: slide-up 0.22s ease-out;
  }
  .layout { flex: 1; display: grid; grid-template-columns: 290px 1fr; min-height: 0; }
  /* Telegram-mobile look: a profile card, then grouped rounded lists of
     rows with coloured icon tiles and chevrons. */
  .nav {
    border-right: 1px solid var(--border);
    padding: 14px 12px;
    display: flex; flex-direction: column; gap: 12px;
    overflow-y: auto;
    background: var(--bg-list);
  }
  .nav > * { flex-shrink: 0; }
  .me-card {
    display: flex; align-items: center; gap: 14px;
    padding: 12px; border-radius: calc(14px * var(--rk, 1)); border: none; cursor: pointer; text-align: left;
    background: var(--bg-surface); color: inherit;
  }
  .me-card:hover { background: var(--bg-hover); }
  .me-ava {
    position: relative; width: 58px; height: 58px; border-radius: var(--ava-r, 50%); flex-shrink: 0;
    display: grid; place-items: center; font-size: 22px; font-weight: 800; color: rgba(6, 7, 12, 0.85); overflow: hidden;
  }
  .me-ava img { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: cover; }
  .me-text { display: flex; flex-direction: column; min-width: 0; }
  .me-name { font-size: 16px; font-weight: 800; color: var(--fg); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .me-sub { font-size: 12.5px; color: var(--fg-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .me-online { font-size: 12px; color: var(--accent); }
  .me-online.off { color: var(--fg-dim); }
  .nav-group { background: var(--bg-surface); border-radius: calc(14px * var(--rk, 1)); overflow: hidden; }
  .nav-item {
    width: 100%;
    display: flex; align-items: center; gap: 12px;
    padding: 8px 12px;
    background: transparent; border: none;
    color: var(--fg); cursor: pointer;
    font-size: 14px; font-weight: 500; text-align: left;
    position: relative;
  }
  .nav-item + .nav-item::before {
    content: ""; position: absolute; top: 0; left: 52px; right: 0; height: 1px; background: var(--border);
  }
  .nav-item:hover { background: var(--bg-hover); }
  .nav-item.active { background: var(--accent-soft); }
  .nav-item.active .nav-lbl { color: var(--accent); font-weight: 700; }
  .tile {
    width: 28px; height: 28px; border-radius: calc(8px * var(--rk, 1)); flex-shrink: 0;
    display: grid; place-items: center; color: #fff;
  }
  .tile :global(svg) { width: 16px; height: 16px; }
  .nav-lbl { flex: 1; }
  .nav-hint { font-size: 11.5px; color: var(--fg-dim); }
  .chev { color: var(--fg-dim); font-size: 18px; line-height: 1; }
  .content { overflow-y: auto; overflow-x: hidden; padding: 0 0 14px; }
  .content .head { position: sticky; top: 0; z-index: 2; background: var(--bg-shell); }
  .mini-btn {
    flex-shrink: 0; border: 1px solid var(--border-strong); background: var(--bg-input); color: var(--fg);
    border-radius: calc(8px * var(--rk, 1)); padding: 4px 10px; font-size: 12px; font-weight: 600; cursor: pointer;
  }
  .mini-btn.primary { background: var(--accent); color: #06070C; border-color: transparent; }
  .mini-btn:disabled { opacity: 0.6; }
  .foot-note { font-size: 12px; color: var(--fg-dim); line-height: 1.55; margin-top: 8px; }
  .foot-note code { font-size: 11px; }
  .lang-row {
    display: flex; align-items: center; gap: 12px; width: 100%; padding: 10px 8px; border-radius: calc(10px * var(--rk, 1));
    background: none; border: none; color: var(--fg); font: inherit; text-align: left; cursor: pointer;
  }
  .lang-row:hover { background: var(--bg-hover); }
  .lang-radio { width: 18px; height: 18px; border-radius: 50%; border: 2px solid var(--fg-dim); position: relative; flex-shrink: 0; }
  .lang-radio.on { border-color: var(--accent); }
  .lang-radio.on::after { content: ""; position: absolute; inset: 3px; border-radius: 50%; background: var(--accent); box-shadow: 0 0 8px var(--accent-glow); }
  .lang-text { display: flex; flex-direction: column; }
  .lang-native { font-size: 14px; font-weight: 700; }
  .lang-name { font-size: 12px; color: var(--fg-dim); }
  .danger-row {
    width: 100%; text-align: left; background: transparent; border: none; cursor: pointer;
    color: #ff6b6b; font-size: 14px; font-weight: 600; padding: 4px 0;
  }
  .rp-row { display: flex; gap: 8px; flex-wrap: wrap; }
  .rp-cell { width: 36px; height: 36px; display: grid; place-items: center; font-size: 26px; }
  .rp-import { display: flex; gap: 8px; margin-top: 10px; }
  .txt {
    flex: 1; min-width: 0; background: var(--bg-input); color: var(--fg);
    border: 1px solid var(--border); border-radius: calc(8px * var(--rk, 1)); padding: 7px 9px; font-size: 13px;
  }
  @keyframes slide-up {
    from { opacity: 0; transform: translateY(12px); }
    to   { opacity: 1; transform: translateY(0); }
  }
  .head {
    display: flex; align-items: center; justify-content: space-between;
    padding: 14px 18px; border-bottom: 1px solid var(--border);
  }
  .head h2 { margin: 0; font-size: 15.5px; font-weight: 700; letter-spacing: -0.01em; }
  .head-title { display: flex; align-items: center; gap: 10px; flex: 1; min-width: 0; }
  .head-title h2 { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .phone-only { display: none; }
  .back {
    width: 36px; height: 36px; margin-left: -8px; flex-shrink: 0;
    background: transparent; border: none; border-radius: calc(10px * var(--rk, 1));
    color: var(--accent); cursor: pointer; align-items: center; justify-content: center;
  }
  .back svg { width: 24px; height: 24px; }
  .back:hover { background: var(--bg-hover); }
  .list-head { align-items: center; justify-content: space-between; padding: 2px 4px 0; }
  .list-head h2 { margin: 0; font-size: 22px; font-weight: 800; letter-spacing: -0.02em; }

  /* ---- Phone: one screen at a time (list, or the open section) ---- */
  @media (max-width: 720px) {
    .backdrop { padding: 0; background: var(--bg-shell); backdrop-filter: none; }
    .sheet {
      width: 100vw !important; height: 100% !important; border: none; border-radius: 0; box-shadow: none;
      padding: env(safe-area-inset-top, 0px) env(safe-area-inset-right, 0px)
               env(safe-area-inset-bottom, 0px) env(safe-area-inset-left, 0px);
      box-sizing: border-box;
    }
    .layout { grid-template-columns: 1fr; }
    .nav { border-right: none; padding: 12px 12px 20px; }
    .nav-item { padding: 9px 12px; font-size: 14.5px; }
    .nav-item + .nav-item::before { left: 52px; }
    .nav-item.active { background: transparent; }
    .nav-item.active .nav-lbl { color: var(--fg); font-weight: 500; }
    .tile { width: 28px; height: 28px; }
    .phone-only { display: flex; }
    .sheet:not(.detail) .content { display: none; }
    .sheet.detail .nav { display: none; }
    .sheet .foot { display: none; }
    .head { padding: 10px 12px; gap: 6px; }
    .head h2 { font-size: 17px; }
    .head .close { display: none; }
    .content { padding-bottom: 24px; }
  }
  .close {
    width: 30px; height: 30px;
    background: transparent; border: 1px solid var(--border);
    border-radius: calc(8px * var(--rk, 1)); color: var(--fg-dim);
    font-size: 18px; line-height: 1; cursor: pointer;
  }
  .close:hover { color: var(--fg); background: var(--bg-hover); }


  /* ---- Appearance: controls on the left, a live chat preview on the right ---- */
  @media (min-width: 721px) {
    .sheet.wide:not(.sized) { width: min(1220px, 97vw); height: min(800px, 93vh); }
  }
  .ap-split { display: grid; grid-template-columns: minmax(0, 1fr) 330px; gap: 4px; align-items: start; padding-right: 16px; }
  .ap-controls { min-width: 0; }
  .ap-preview { position: sticky; top: 64px; margin-top: 12px; display: flex; flex-direction: column; gap: 8px; }
  .mesh-ways { display: flex; flex-wrap: wrap; gap: 8px; margin: 8px 0 0; }
  .mesh-way { display: inline-flex; align-items: center; gap: 7px; padding: 6px 11px; border-radius: 999px; font-size: 12.5px; color: var(--fg-muted); background: var(--bg-input); border: 1px solid var(--border); }
  .mesh-way.on { color: var(--fg); border-color: color-mix(in srgb, var(--accent) 40%, transparent); }
  .mesh-row { display: flex; flex-wrap: wrap; align-items: center; gap: 10px 14px; margin: 10px 0 4px; padding: 12px 14px; border-radius: calc(14px * var(--rk, 1)); background: var(--bg-input); border: 1px solid var(--border); }
  .mesh-big { font-size: 28px; font-weight: 800; color: var(--accent); min-width: 36px; text-align: center; font-variant-numeric: tabular-nums; }
  .mesh-txt { flex: 1 1 140px; min-width: 0; }
  @media (max-width: 520px) { .mesh-btn { flex: 1 1 100%; } }
  .mesh-btn { border: none; border-radius: calc(10px * var(--rk, 1)); padding: 9px 14px; background: var(--accent); color: #06070C; font: inherit; font-size: 13px; font-weight: 700; cursor: pointer; white-space: nowrap; }
  .mesh-btn:disabled { opacity: .5; cursor: default; }
  .mesh-log { display: flex; align-items: center; gap: 10px; padding: 8px 16px; font-size: 13px; }
  .mesh-who { font-weight: 600; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .mesh-what { color: var(--fg-muted); flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .mesh-when { color: var(--fg-dim); font-size: 12px; white-space: nowrap; }
  .ap-div { display: flex; align-items: center; gap: 12px; margin: 26px 16px 4px; scroll-margin-top: 64px; }
  .ap-div::before, .ap-div::after { content: ""; flex: 1; height: 1px; background: linear-gradient(90deg, transparent, var(--border-strong), transparent); }
  .ap-div span { font-size: 11px; font-weight: 800; letter-spacing: 0.08em; text-transform: uppercase; color: var(--fg-muted); }
  .pv-card { border: 1px solid var(--border-strong); border-radius: calc(16px * var(--rk, 1)); overflow: hidden; background: var(--bg-shell); display: flex; flex-direction: column; height: min(560px, calc(93vh - 140px)); }
  .pv-head { display: flex; align-items: center; gap: 10px; padding: 10px 12px; background: var(--bg-list); border-bottom: 1px solid var(--border); }
  .pv-ava { width: 32px; height: 32px; border-radius: var(--ava-r, 50%); display: grid; place-items: center; font-weight: 800; font-size: 14px; color: #06070C; background: linear-gradient(135deg, var(--accent), var(--accent-2)); }
  .pv-meta { display: flex; flex-direction: column; line-height: 1.2; }
  .pv-meta b { font-size: 13.5px; color: var(--fg); }
  .pv-meta span { font-size: 11px; color: var(--accent); }
  .pv-stage { position: relative; flex: 1; min-height: 0; isolation: isolate; overflow: hidden; }
  .pv-wall { position: absolute; inset: 0; z-index: 0; background: var(--chat-wallpaper, transparent); }
  .pv-msgs { position: relative; z-index: 1; height: 100%; box-sizing: border-box; padding: 12px 10px; display: flex; flex-direction: column; justify-content: flex-end; gap: 4px; font-size: calc(13px * var(--font-scale, 1)); }
  .pv-date { align-self: center; font-size: 10.5px; font-weight: 700; text-transform: uppercase; color: var(--fg-muted); background: color-mix(in srgb, var(--bg-shell) 70%, transparent); padding: 3px 10px; border-radius: 999px; margin-bottom: 4px; }
  .pv-b { max-width: 82%; padding: calc(7px * var(--density, 1)) 10px; line-height: 1.4; color: var(--fg); background: var(--bg-bubble-in); border: 1px solid var(--border); border-radius: var(--bubble-r, 14px); word-wrap: break-word; }
  .pv-b.in { align-self: flex-start; }
  .pv-b.in.first { border-top-left-radius: 4px; }
  .pv-b.out { align-self: flex-end; background: linear-gradient(135deg, color-mix(in srgb, var(--accent) 16%, transparent), color-mix(in srgb, var(--accent-2) 14%, transparent)), var(--bg-bubble-in); border-color: color-mix(in srgb, var(--accent) 22%, transparent); }
  .pv-b.out.first { border-top-right-radius: 4px; margin-top: 6px; }
  .pv-b a { color: var(--accent); }
  .pv-b code { font-family: var(--font-mono, monospace); font-size: 0.9em; background: var(--bg-hover); padding: 0 4px; border-radius: calc(4px * var(--rk, 1)); }
  .pv-t { float: right; margin: 5px 0 0 10px; font-size: 10px; color: var(--fg-dim); }
  .pv-b.out .pv-t { color: var(--accent); opacity: 0.85; }
  .pv-reply { display: block; border-left: 3px solid var(--accent); padding: 2px 8px; margin-bottom: 4px; border-radius: calc(4px * var(--rk, 1)); background: var(--accent-soft); font-size: 0.9em; color: var(--fg-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .pv-reply b { display: block; color: var(--accent); font-size: 0.92em; }
  .pv-sticker { align-self: flex-start; font-size: calc(var(--sticker-size, 128px) * 0.42); line-height: 1; margin-top: 4px; }
  .pv-foot { display: flex; align-items: center; gap: 8px; padding: 9px 10px; border-top: 1px solid var(--border); background: var(--bg-list); }
  .pv-field { flex: 1; font-size: 12px; color: var(--fg-dim); background: var(--bg-input); border: 1px solid var(--border); border-radius: calc(10px * var(--rk, 1)); padding: 7px 10px; }
  .pv-send { width: 30px; height: 30px; border-radius: 50%; display: grid; place-items: center; color: #06070C; background: linear-gradient(135deg, var(--accent), var(--accent-2)); flex-shrink: 0; }
  .pv-send svg { width: 14px; height: 14px; }
  .pv-note { text-align: center; font-size: 11.5px; color: var(--fg-dim); }
  @media (max-width: 1080px) {
    .ap-split { grid-template-columns: 1fr; padding-right: 0; }
    .ap-preview { order: -1; top: 56px; z-index: 1; margin: 10px 16px 0; padding-bottom: 6px; background: var(--bg-shell); }
    .pv-card { height: 250px; }
    .pv-note { display: none; }
  }
  /* Sections slide in when you switch (transform/opacity only: cheap). */
  .content .group, .content .ap-div, .content .ap-preview { animation: sec-in 0.2s cubic-bezier(.2,.8,.3,1) both; }
  .content .group:nth-of-type(2) { animation-delay: 0.025s; }
  .content .group:nth-of-type(3) { animation-delay: 0.05s; }
  @keyframes sec-in { from { opacity: 0; transform: translateY(6px); } }
  @media (prefers-reduced-motion: reduce) { .content .group, .content .ap-div, .content .ap-preview { animation: none; } }
  /* Global inside the panel: sections drawn by child components
     (NetworkSettings, VoiceVideoSettings) look like the rest. */
  .ab-link { gap: 12px; }
  .ab-ic { width: 34px; height: 34px; border-radius: calc(10px * var(--rk, 1)); background: var(--bg-input); display: grid; place-items: center; color: var(--fg); flex-shrink: 0; }
  .ab-ic svg { width: 19px; height: 19px; }
  .ab-go { color: var(--fg-dim); font-size: 20px; }
  .ab-ver { color: var(--fg-dim); font-size: 12px; }
  .pw-scale { display: flex; justify-content: space-between; font-size: 13px; color: var(--fg-muted); margin: 2px 0 6px; }
  .pw-scale .mid { color: var(--accent); font-weight: 600; }
  .pw-dim { opacity: .55; }
  .label .pill-tag.on { background: var(--accent); color: #06070C; }
  .content :global(.group), .group {
    margin: 12px 16px 0;
    padding: 12px 16px 14px;
    background: var(--bg-surface);
    border-radius: calc(14px * var(--rk, 1));
  }
  .content :global(.group > .label), .label {
    font-size: 12px;
    font-weight: 700;
    color: var(--accent);
    margin-bottom: 10px;
  }

  .row { display: flex; align-items: center; gap: 14px; margin-bottom: 10px; }
  .row:last-child { margin-bottom: 0; }
  .row-label { flex: 0 0 90px; color: var(--fg-muted); font-size: 13px; }

  .range { display: flex; align-items: center; gap: 10px; flex: 1; }
  .range input[type="range"] {
    flex: 1;
    accent-color: var(--accent);
  }
  .range-val { min-width: 42px; text-align: right; font-variant-numeric: tabular-nums; color: var(--fg-muted); font-size: 12px; }

  .walls {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 8px;
  }
  .wall {
    display: flex; flex-direction: column; align-items: center; gap: 6px;
    padding: 4px;
    background: transparent;
    border: 1px solid transparent;
    border-radius: calc(10px * var(--rk, 1));
    cursor: pointer;
    color: var(--fg-muted);
    transition: all 0.12s ease;
  }
  .wall:hover { color: var(--fg); }
  .wall.active {
    border-color: var(--accent);
    background: var(--accent-soft);
    color: var(--accent);
  }
  .wall-swatch {
    width: 100%;
    aspect-ratio: 4 / 3;
    border-radius: calc(8px * var(--rk, 1));
    border: 1px solid var(--border);
    background-size: cover;
    background-position: center;
    display: grid; place-items: center;
  }
  .wall-swatch.custom { border-style: dashed; }
  .wall-mark {
    font-size: 20px;
    font-weight: 300;
    color: var(--fg-dim);
  }
  .wall-name {
    font-size: 10.5px;
    font-weight: 600;
    text-align: center;
  }

  .themes { display: grid; grid-template-columns: repeat(auto-fill, minmax(118px, 1fr)); gap: 8px; }
  .theme-card {
    display: flex; flex-direction: column; gap: 6px; padding: 6px;
    border: 1.5px solid var(--border); border-radius: calc(12px * var(--rk, 1)); background: transparent;
    color: var(--fg-muted); cursor: pointer; text-align: left;
  }
  .theme-card:hover { border-color: var(--border-strong); color: var(--fg); }
  .theme-card.active { border-color: var(--accent); color: var(--accent); background: var(--accent-soft); }
  .tc-prev {
    position: relative; display: block; height: 58px; border-radius: calc(8px * var(--rk, 1)); overflow: hidden;
    background: var(--s); border: 1px solid rgba(127, 127, 127, 0.18);
  }
  .tc-prev i { position: absolute; display: block; border-radius: calc(6px * var(--rk, 1)); }
  .tc-l { left: 0; top: 0; bottom: 0; width: 28%; border-radius: 0 !important; background: var(--l); }
  .tc-in { left: 34%; top: 12px; width: 44%; height: 12px; background: var(--b); }
  .tc-out { right: 8%; top: 32px; width: 40%; height: 12px; background: color-mix(in srgb, var(--a) 55%, var(--s)); }
  .tc-name { font-size: 11.5px; font-weight: 700; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .snd-list { display: flex; flex-direction: column; gap: 2px; }
  .snd { display: flex; align-items: center; gap: 4px; border-radius: calc(9px * var(--rk, 1)); }
  .snd.on { background: var(--accent-soft); }
  .snd-pick { flex: 1; display: flex; align-items: center; gap: 10px; padding: 7px 8px; border: none; background: none; color: var(--fg); font: inherit; font-size: 13.5px; cursor: pointer; text-align: left; }
  .snd-name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .snd-tag { font-size: 10.5px; font-weight: 700; color: var(--accent); }
  .icon-btn {
    flex: 0 0 30px; height: 30px; display: grid; place-items: center;
    border: none; border-radius: calc(8px * var(--rk, 1)); background: transparent; color: var(--fg-muted); cursor: pointer; font-size: 12px;
  }
  .icon-btn:hover { background: var(--bg-hover); color: var(--fg); }
  .sel {
    flex: 1; min-width: 0; height: 32px; padding: 0 8px; border-radius: calc(8px * var(--rk, 1));
    border: 1px solid var(--border); background: var(--bg-input); color: var(--fg); font: inherit; font-size: 13px;
  }
  .ell { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .share-row { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
  .share-row .txt { flex: 1 1 160px; height: 32px; box-sizing: border-box; }
  .share-row .mini-btn, .rp-import .mini-btn { height: 32px; box-sizing: border-box; }
  .rp-import .txt { height: 32px; box-sizing: border-box; }
  .palettes {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 8px;
  }
  .palette {
    display: flex; align-items: center; gap: 10px;
    padding: 9px 12px;
    background: var(--bg-input);
    border: 1px solid var(--border);
    border-radius: calc(10px * var(--rk, 1));
    cursor: pointer;
    transition: all 0.12s ease;
    color: var(--fg);
    text-align: left;
  }
  .palette:hover { border-color: var(--border-strong); }
  .palette.active {
    border-color: var(--pa);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--pa) 25%, transparent);
  }
  .swatch {
    width: 20px; height: 20px;
    border-radius: 50%;
    background: linear-gradient(135deg, var(--pa), var(--pb));
    box-shadow: 0 2px 6px -1px color-mix(in srgb, var(--pa) 60%, transparent);
    flex-shrink: 0;
  }
  .pal-name { flex: 1; font-size: 13px; font-weight: 600; }
  .chk :global(svg) { width: 14px; height: 14px; color: var(--accent); }

  .info-row {
    display: flex; align-items: baseline; gap: 10px;
    padding: 6px 0;
    font-size: 12.5px;
  }
  .info-row .k { flex: 0 0 90px; color: var(--fg-dim); font-weight: 600; }
  .info-row .v { flex: 1; color: var(--fg); min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .info-row.ue { align-items: flex-start; }
  .info-row.ue .v { white-space: normal; overflow: visible; }
  .info-row .mono { font-family: var(--font-mono); color: var(--fg-muted); font-size: 11.5px; }
  .info-row .dir { direction: rtl; text-align: left; }

  .upd-err { color: var(--danger, #FF4D6D); white-space: normal; }
  .about { font-size: 12.5px; color: var(--fg-muted); line-height: 1.6; }
  .about b { color: var(--accent); }

  .toggle-row {
    display: flex; align-items: center; justify-content: space-between; gap: 12px;
    width: 100%;
    padding: 9px 0;
    background: transparent; border: none; cursor: pointer;
    text-align: left; color: inherit;
    border-bottom: 1px solid var(--border);
  }
  .toggle-row:last-child { border-bottom: none; }
  /* Attach menu / nav bar rows: name, then small controls on the right. */
  .am-row { cursor: default; gap: 6px; }
  .am-row .tr-text { flex: 1; }
  .am-row.off .tr-title { color: var(--fg-dim); text-decoration: line-through; }
  .am-btn { width: 30px; height: 30px; flex-shrink: 0; display: grid; place-items: center; border-radius: calc(8px * var(--rk, 1));
    border: 1px solid var(--border); background: var(--bg-input); color: var(--fg-muted); cursor: pointer; font: 700 14px var(--font-ui); }
  .am-btn:disabled { opacity: .35; cursor: default; }
  .am-btn svg { width: 16px; height: 16px; }
  .pl-acts { display: flex; flex-wrap: wrap; gap: 8px; margin: 8px 0; }
  .pl-code { width: 100%; font: 12px/1.5 var(--font-mono, monospace); background: var(--bg-input); color: var(--fg); border: 1px solid var(--border);
    border-radius: calc(10px * var(--rk, 1)); padding: 8px 10px; margin-bottom: 8px; resize: vertical; }
  .switch-btn { background: none; border: none; padding: 0; cursor: pointer; display: flex; }
  .tr-text { min-width: 0; }
  .tr-title { font-size: 13.5px; font-weight: 600; color: var(--fg); }
  .tr-sub { font-size: 11.5px; color: var(--fg-dim); margin-top: 2px; }
  .switch {
    flex-shrink: 0;
    width: 40px; height: 23px;
    border-radius: 100px;
    background: var(--bg-input);
    border: 1px solid var(--border-strong);
    position: relative;
    transition: background 0.15s ease, border-color 0.15s ease;
  }
  .switch::after {
    content: '';
    position: absolute; top: 2px; left: 2px;
    width: 17px; height: 17px;
    border-radius: 50%;
    background: var(--fg-dim);
    transition: transform 0.15s ease, background 0.15s ease;
  }
  .switch.on {
    background: var(--accent-soft);
    border-color: var(--accent);
  }
  .switch.on::after { transform: translateX(17px); background: var(--accent); }

  .bots-note { font-size: 12px; color: var(--fg-dim); line-height: 1.5; margin-top: 6px; }
  .bots-note.err { color: #ff9fb0; }
  .bots-note code { font-family: var(--font-mono); font-size: 11px; color: var(--accent); }
  .bot-create {
    display: grid; grid-template-columns: 1fr 1fr auto; gap: 8px;
    align-items: end; margin-bottom: 10px;
  }
  .bc-field { display: flex; flex-direction: column; gap: 4px; min-width: 0; }
  .bc-field > span { font-size: 11px; color: var(--fg-muted); font-weight: 600; }
  .bc-field input, .bc-at {
    height: 34px; box-sizing: border-box; width: 100%; min-width: 0;
    background: var(--bg-input); border: 1px solid var(--border);
    border-radius: calc(8px * var(--rk, 1)); color: var(--fg); font-family: inherit;
    font-size: 12.5px; padding: 0 10px; outline: none;
  }
  .bc-at { display: flex; align-items: center; gap: 2px; }
  .bc-at i { font-style: normal; color: var(--fg-dim); }
  .bc-at input { height: 100%; border: none; background: transparent; padding: 0; }
  .bc-field input:focus, .bc-at:focus-within { border-color: var(--accent); }
  .bot-add {
    height: 34px; padding: 0 16px; border: none; border-radius: calc(8px * var(--rk, 1));
    background: linear-gradient(135deg, var(--accent), var(--accent-2));
    color: var(--accent-ink, #06070C); font-weight: 700; font-size: 12.5px; cursor: pointer;
  }
  .bot-add:disabled { opacity: 0.5; cursor: not-allowed; }
  .bot-list { display: flex; flex-direction: column; gap: 8px; }
  .bot-card {
    padding: 10px 12px; background: var(--bg-input);
    border: 1px solid var(--border); border-radius: calc(12px * var(--rk, 1));
    display: flex; flex-direction: column; gap: 8px;
  }
  .bot-head { display: flex; align-items: center; gap: 10px; }
  .bot-ava {
    width: 34px; height: 34px; border-radius: var(--ava-r, 50%); flex-shrink: 0;
    display: grid; place-items: center; font-weight: 700; font-size: 14px;
    background: linear-gradient(135deg, var(--accent), var(--accent-2)); color: var(--accent-ink, #06070C);
  }
  .bot-main { flex: 1; min-width: 0; }
  .bot-name { font-size: 13px; font-weight: 600; display: flex; align-items: center; gap: 6px; }
  .bot-tag {
    font-size: 9px; font-weight: 800; letter-spacing: 0.06em; padding: 1px 5px;
    border-radius: calc(4px * var(--rk, 1)); background: color-mix(in srgb, var(--accent) 18%, transparent); color: var(--accent);
  }
  .bot-user { font-size: 11.5px; color: var(--fg-dim); }
  .bot-tokrow, .bot-cmd { display: flex; align-items: center; gap: 6px; }
  .bot-token, .bot-cmd code {
    flex: 1; min-width: 0; height: 28px; line-height: 28px; padding: 0 8px; box-sizing: border-box;
    font-family: var(--font-mono); font-size: 10.5px; color: var(--fg-muted);
    background: var(--bg-shell); border: 1px solid var(--border); border-radius: calc(7px * var(--rk, 1));
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap; user-select: all;
  }
  .bot-cmd { margin-top: 8px; }
  .bot-btn {
    height: 28px; padding: 0 10px; border: 1px solid var(--border); border-radius: calc(7px * var(--rk, 1));
    background: transparent; color: var(--fg-muted); font-size: 11.5px;
    font-weight: 600; cursor: pointer; flex-shrink: 0; white-space: nowrap;
  }
  .bot-btn:hover { color: var(--accent); border-color: color-mix(in srgb, var(--accent) 45%, transparent); }
  .bot-btn.primary { color: var(--accent); border-color: color-mix(in srgb, var(--accent) 45%, transparent); background: color-mix(in srgb, var(--accent) 10%, transparent); }
  .bot-actions { display: flex; align-items: center; gap: 4px; flex-wrap: wrap; margin-top: 2px; }
  .bot-sp { flex: 1; }
  .bot-link {
    border: none; background: none; padding: 4px 6px; border-radius: calc(6px * var(--rk, 1));
    color: var(--fg-dim); font-size: 11.5px; font-weight: 600; cursor: pointer;
  }
  .bot-link:hover { color: var(--accent); background: color-mix(in srgb, var(--accent) 8%, transparent); }
  .bot-link.danger:hover { color: #ff7a90; background: rgba(255, 90, 120, 0.08); }
  .bot-api-toggle {
    width: 100%; display: flex; justify-content: space-between; align-items: center;
    border: none; background: none; color: var(--fg); padding: 0;
    font-size: 12.5px; font-weight: 600; cursor: pointer;
  }
  .bot-api-toggle .chev { transition: transform 0.15s; color: var(--fg-dim); }
  .bot-api-toggle .chev.open { transform: rotate(90deg); }
  .bot-api {
    margin: 10px 0 0; padding: 10px; border-radius: calc(8px * var(--rk, 1)); overflow-x: auto;
    background: var(--bg-shell); border: 1px solid var(--border);
    font-family: var(--font-mono); font-size: 10.5px; line-height: 1.6; color: var(--fg-muted);
  }
  @media (max-width: 520px) {
    .bot-create { grid-template-columns: 1fr; }
  }

  .tg-badge { display: inline-grid; place-items: center; width: 18px; height: 18px; border-radius: 50%; background: #2AABEE; color: #fff; flex-shrink: 0; }
  .bk-row { display: flex; gap: 8px; margin-top: 10px; }
  .dev-list { display: flex; flex-direction: column; gap: 2px; margin-top: 10px; }
  .dev-row { display: flex; align-items: center; gap: 10px; padding: 9px 4px; border-top: 1px solid var(--border); }
  .dev-row:first-child { border-top: none; }
  .dev-main { flex: 1; min-width: 0; }
  .dev-input { width: 100%; }
  .dev-name { font-size: 14px; font-weight: 600; display: flex; flex-wrap: wrap; align-items: center; gap: 6px; }
  .dev-sub { font-size: 12px; color: var(--fg-dim); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .dev-tag { font-size: 10.5px; font-weight: 700; text-transform: uppercase; letter-spacing: .04em; color: var(--accent); background: color-mix(in srgb, var(--accent) 14%, transparent); border-radius: calc(6px * var(--rk, 1)); padding: 1px 6px; }
  .dev-tag.dim { color: var(--fg-dim); background: color-mix(in srgb, var(--fg-dim) 14%, transparent); }
  .bk-row .txt { flex: 1; min-width: 0; }
  .bk-stat { display: flex; align-items: center; gap: 8px; font-size: 13px; margin: 10px 0 6px; }
  .bk-dot { width: 8px; height: 8px; border-radius: 50%; background: var(--fg-dim); }
  .bk-dot.on { background: #34C759; box-shadow: 0 0 0 3px rgba(52, 199, 89, 0.18); }
  .bk-phrase { margin-top: 12px; padding: 12px; border-radius: calc(12px * var(--rk, 1)); border: 1px dashed color-mix(in srgb, var(--accent) 50%, transparent); background: var(--bg-input); }
  .bk-ph-title { font-size: 12px; font-weight: 700; color: var(--accent); margin-bottom: 8px; }
  .bk-phrase ol { display: grid; grid-template-columns: repeat(4, 1fr); gap: 4px 10px; margin: 0; padding-left: 22px; font-family: var(--font-mono); font-size: 12.5px; }
  @media (max-width: 520px) { .bk-phrase ol { grid-template-columns: repeat(2, 1fr); } }

  /* Theme editor */
  .te-intro { font-size: 12px; color: var(--fg-dim); line-height: 1.5; margin-bottom: 12px; }
  .te-group-title {
    font-size: 11px; font-weight: 700; color: var(--fg-muted);
    text-transform: uppercase; letter-spacing: 0.06em;
    margin: 14px 0 8px;
  }
  .te-colors { display: grid; grid-template-columns: repeat(2, 1fr); gap: 8px; }
  .te-color {
    position: relative;
    display: flex; align-items: center; gap: 9px;
    padding: 7px 10px;
    background: var(--bg-input); border: 1px solid var(--border);
    border-radius: calc(9px * var(--rk, 1)); cursor: pointer;
  }
  .te-color:hover { border-color: var(--border-strong); }
  .te-help {
    margin-left: auto; position: relative;
    width: 16px; height: 16px; flex-shrink: 0;
    display: grid; place-items: center;
    color: var(--fg-dim); font-weight: 700; cursor: help;
  }
  .te-help:hover { color: var(--accent); }
  .te-tip {
    position: absolute; bottom: calc(100% + 8px); right: -4px;
    width: 200px;
    background: var(--bg-surface-2); color: var(--fg);
    border: 1px solid var(--border-strong); border-radius: calc(9px * var(--rk, 1));
    padding: 8px 10px; font-size: 11.5px; font-weight: 400; line-height: 1.45;
    box-shadow: 0 12px 30px -8px rgba(0,0,0,0.6);
    opacity: 0; pointer-events: none; transform: translateY(4px);
    transition: opacity 0.12s ease, transform 0.12s ease; z-index: 20;
    text-align: left;
  }
  .te-help:hover .te-tip { opacity: 1; transform: translateY(0); }

  /* Live preview card */
  /* Hide the native colour input; the swatch is the visible control. */
  .te-color input[type="color"] {
    position: absolute; width: 0; height: 0; opacity: 0; pointer-events: none;
  }
  .te-swatch {
    width: 22px; height: 22px; border-radius: calc(6px * var(--rk, 1));
    border: 1px solid var(--border-strong); flex-shrink: 0;
  }
  .te-name { font-size: 12.5px; color: var(--fg); font-weight: 600; }

  .te-font { margin-top: 2px; }
  .te-font-cur {
    font-size: 13px; color: var(--fg); font-weight: 600;
    padding: 8px 10px; background: var(--bg-input);
    border: 1px solid var(--border); border-radius: calc(9px * var(--rk, 1)); margin-bottom: 8px;
  }
  .te-font-btns { display: flex; gap: 6px; }
  .te-btn {
    padding: 7px 12px; border-radius: calc(8px * var(--rk, 1));
    border: 1px solid var(--accent-soft); background: var(--accent-soft);
    color: var(--accent); font-size: 12.5px; font-weight: 600; cursor: pointer;
  }
  .te-btn:hover { background: color-mix(in srgb, var(--accent) 22%, transparent); }
  .te-btn.ghost {
    border-color: var(--border); background: transparent; color: var(--fg-muted);
  }
  .te-btn.ghost:hover { color: var(--fg); border-color: var(--border-strong); }
  .te-hint { font-size: 11px; color: var(--fg-dim); margin-top: 6px; line-height: 1.4; }

  .foot {
    display: flex; justify-content: space-between; gap: 10px;
    padding: 12px 18px;
    border-top: 1px solid var(--border);
    background: var(--bg-list);
  }
  .reset {
    padding: 8px 14px;
    background: transparent;
    border: 1px solid var(--border);
    border-radius: calc(10px * var(--rk, 1));
    color: var(--fg-dim);
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .reset:hover { color: var(--fg); border-color: var(--border-strong); }
  .done {
    padding: 8px 18px;
    background: linear-gradient(135deg, var(--accent), var(--accent-2));
    color: #06070C;
    border: none;
    border-radius: calc(10px * var(--rk, 1));
    font-size: 13px;
    font-weight: 700;
    cursor: pointer;
    box-shadow: 0 6px 16px -4px var(--accent-glow);
  }
  .done:hover { transform: translateY(-1px); }
  .icons-row { display: flex; flex-wrap: wrap; gap: 10px; }
  .app-ic { display: flex; flex-direction: column; align-items: center; gap: 5px; padding: 8px; width: 76px;
    border: 1px solid var(--border); border-radius: calc(14px * var(--rk, 1)); background: var(--bg-input); color: var(--fg-muted);
    font-size: 11px; cursor: pointer; transition: transform .25s cubic-bezier(.2,.8,.2,1), border-color .2s, background .2s; }
  .app-ic:hover { transform: translateY(-2px); }
  .app-ic.active { border-color: var(--accent); background: var(--accent-soft); color: var(--fg); }
  .app-ic img, .app-ic .plus { width: 48px; height: 48px; border-radius: calc(12px * var(--rk, 1)); object-fit: cover; }
  .app-ic .plus { display: grid; place-items: center; font-size: 22px; border: 1px dashed var(--border-strong); }
  .my-actions { display: flex; gap: 8px; flex-wrap: wrap; margin: 4px 0 10px; }
  .my-pack { border: 1px solid var(--border); border-radius: calc(12px * var(--rk, 1)); margin-bottom: 8px; overflow: hidden; transition: border-color .2s; }
  .my-pack.open { border-color: var(--accent); }
  .mp-toggle { width: 100%; display: flex; align-items: center; gap: 10px; padding: 8px 10px; border: none; background: var(--bg-input); color: var(--fg); cursor: pointer; text-align: left; }
  .mp-thumb { width: 36px; height: 36px; display: grid; place-items: center; flex-shrink: 0; }
  .mp-thumb :global(img), .mp-thumb :global(video) { max-width: 100%; max-height: 100%; }
  .mp-title { flex: 1; font-weight: 700; font-size: 13px; }
  .mp-rename { flex: 1; }
  .mp-count { font-size: 12px; color: var(--fg-dim); }
  .mp-chev { transition: transform .25s cubic-bezier(.2,.8,.2,1); color: var(--fg-dim); }
  .my-pack.open .mp-chev { transform: rotate(90deg); }
  .mp-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(72px, 1fr)); gap: 6px; padding: 10px; animation: mp-in .3s cubic-bezier(.2,.8,.2,1); }
  @keyframes mp-in { from { opacity: 0; transform: translateY(-4px); } }
  .mp-item { position: relative; aspect-ratio: 1; display: grid; place-items: center; border-radius: calc(10px * var(--rk, 1)); background: var(--bg-input); }
  .mp-item :global(img), .mp-item :global(video) { max-width: 88%; max-height: 88%; }
  .mp-emoji { position: absolute; left: 4px; bottom: 2px; font-size: 13px; }
  .mp-del { position: absolute; top: 3px; right: 3px; width: 20px; height: 20px; border-radius: 50%; border: none; background: rgba(0,0,0,.6); color: #fff;
    cursor: pointer; opacity: 0; transition: opacity .15s; line-height: 1; }
  .mp-item:hover .mp-del { opacity: 1; }
  .mp-add { aspect-ratio: 1; border-radius: calc(10px * var(--rk, 1)); border: 1.5px dashed var(--border-strong); background: none; color: var(--fg-dim); font-size: 22px; cursor: pointer; }
  .mp-add:hover { color: var(--accent); border-color: var(--accent); }
  .mini-btn.danger { color: #ff6b6b; }
  .info-row .dim { color: var(--fg-dim); font-size: 12px; }
  .wall.own { position: relative; padding: 0; }
  .wall-pick { border: none; background: none; padding: 0; cursor: pointer; width: 100%; }
  .wall-swatch.img { object-fit: cover; display: block; width: 100%; }
  .wall-del { position: absolute; top: 6px; right: 6px; width: 22px; height: 22px; border-radius: 50%; border: none;
    background: rgba(0,0,0,.6); color: #fff; cursor: pointer; opacity: 0; transition: opacity .15s; }
  .wall.own:hover .wall-del { opacity: 1; }
  .host-port { display: flex; align-items: center; gap: 10px; padding: 8px 14px 12px; font-size: 13.5px; color: var(--fg-dim); }
  .host-port input { width: 110px; }
  .host-addr { display: flex; align-items: center; gap: 10px; padding: 8px 14px; }
  .host-addr span { width: 84px; flex-shrink: 0; font-size: 13px; color: var(--fg-dim); }
  .host-addr code { flex: 1; min-width: 0; font-family: var(--font-mono, monospace); font-size: 13px; overflow-wrap: anywhere; user-select: all; }
</style>
