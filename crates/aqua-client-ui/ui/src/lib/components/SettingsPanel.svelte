<script module lang="ts">
  import { writable } from "svelte/store";
  export const settingsOpen = writable<boolean>(false);
  /** Section to jump to on the next open (e.g. "voice" from the call UI). */
  export const settingsSection = writable<string | null>(null);
  export function openSettings(section?: string) {
    if (section) settingsSection.set(section);
    settingsOpen.set(true);
  }
  export function closeSettings() { settingsOpen.set(false); }
</script>

<script lang="ts">
  import { onMount } from "svelte";
  import { settings, PALETTES, WALLPAPERS, EDITABLE_COLORS, THEMES, DEFAULTS, type Mode, type Density } from "../theme";
  import { info } from "../stores";
  import { icons } from "../icons";
  import { openFileDialog } from "../dialog";
  import { ICON_PRESETS, logoSvg, svgDataUrl, colorsFromAccent, setAppIcon } from "../appIcon";
  import { cropImage } from "./ImageCropper.svelte";
  import { openStickerEditor, stickerLibraryTick } from "./StickerEditor.svelte";
  import AnimatedMedia from "./AnimatedMedia.svelte";
  import type { StickerPack } from "../types";
  import { api, bytesToDataUrl, assetUrl } from "../tauri";
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
  import { lockConf, setAutoLock, lockNow, passcodeSetupOpen } from "../lock";
  import { t, lang, LANGS, tr } from "../i18n";
  import { chats, activeId } from "../stores";
  import { BUILTIN_SOUNDS, customSounds, refreshSounds, playSound, soundLabel, addSoundBytes } from "../sounds";
  import { exportThemeToFile, sendThemeToChat, pickAndApplyTheme } from "../themeShare";
  import { askConfirm } from "./Confirm.svelte";
  import { backupState, refreshBackup, backupNow } from "../backup";
  import { save as saveDialog } from "@tauri-apps/plugin-dialog";

  // ---- Section navigation ----
  type Sec = "ghost" | "language" | "appearance" | "theme" | "behaviour" | "notifications" | "voice" | "wallpaper" | "accent" | "bots" | "account" | "about" | "privacy" | "stickers" | "backups";
  let section = $state<Sec>("notifications");
  // Telegram-mobile style: coloured icon tiles in grouped lists.
  interface Item { id: Sec | "profile" | "shortcuts"; label: string; color: string; icon: string; hint?: string }
  const ic = (p: string) => `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">${p}</svg>`;
  const GROUPS: Item[][] = [
    [
      { id: "profile", label: "Edit profile", color: "#007AFF", icon: ic(`<path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"/><circle cx="12" cy="7" r="4"/>`) },
      { id: "ghost", label: "Ghost mode", color: "#5E5CE6", icon: ic(`<path d="M9 10h.01"/><path d="M15 10h.01"/><path d="M12 2a8 8 0 0 0-8 8v12l3-3 2.5 2.5L12 19l2.5 2.5L17 19l3 3V10a8 8 0 0 0-8-8z"/>`) },
      { id: "backups", label: "Backups", color: "#30B0C7", icon: ic(`<path d="M12 2a10 10 0 1 0 10 10"/><path d="M12 6v6l4 2"/><path d="M22 2v6h-6"/>`) },
      { id: "privacy", label: "Privacy & Security", color: "#8E8E93", icon: ic(`<rect x="3" y="11" width="18" height="11" rx="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/>`) },
    ],
    [
      { id: "notifications", label: "Notifications", color: "#FF3B30", icon: ic(`<path d="M18 8A6 6 0 0 0 6 8c0 7-3 9-3 9h18s-3-2-3-9"/><path d="M13.73 21a2 2 0 0 1-3.46 0"/>`) },
      { id: "behaviour", label: "Chat settings", color: "#007AFF", icon: ic(`<path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/>`) },
      { id: "voice", label: "Voice & Video", color: "#34C759", icon: ic(`<rect x="9" y="2" width="6" height="12" rx="3"/><path d="M5 10v1a7 7 0 0 0 14 0v-1"/><line x1="12" y1="18" x2="12" y2="22"/>`) },
      { id: "language", label: "Language", color: "#AF52DE", icon: ic(`<circle cx="12" cy="12" r="10"/><line x1="2" y1="12" x2="22" y2="12"/><path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z"/>`) },
      { id: "stickers", label: "Stickers & Emoji", color: "#FF9500", icon: ic(`<circle cx="12" cy="12" r="10"/><path d="M8 14s1.5 2 4 2 4-2 4-2"/><line x1="9" y1="9" x2="9.01" y2="9"/><line x1="15" y1="9" x2="15.01" y2="9"/>`) },
    ],
    [
      { id: "appearance", label: "Appearance", color: "#32ADE6", icon: ic(`<circle cx="12" cy="12" r="5"/><line x1="12" y1="1" x2="12" y2="3"/><line x1="12" y1="21" x2="12" y2="23"/><line x1="4.22" y1="4.22" x2="5.64" y2="5.64"/><line x1="18.36" y1="18.36" x2="19.78" y2="19.78"/><line x1="1" y1="12" x2="3" y2="12"/><line x1="21" y1="12" x2="23" y2="12"/>`) },
      { id: "accent", label: "Accent colour", color: "#AF52DE", icon: ic(`<path d="M12 2.69l5.66 5.66a8 8 0 1 1-11.31 0z"/>`) },
      { id: "theme", label: "Theme editor", color: "#5856D6", icon: ic(`<line x1="4" y1="21" x2="4" y2="14"/><line x1="4" y1="10" x2="4" y2="3"/><line x1="12" y1="21" x2="12" y2="12"/><line x1="12" y1="8" x2="12" y2="3"/><line x1="20" y1="21" x2="20" y2="16"/><line x1="20" y1="12" x2="20" y2="3"/><line x1="1" y1="14" x2="7" y2="14"/><line x1="9" y1="8" x2="15" y2="8"/><line x1="17" y1="16" x2="23" y2="16"/>`) },
      { id: "wallpaper", label: "Chat wallpaper", color: "#FF2D55", icon: ic(`<rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="8.5" cy="8.5" r="1.5"/><polyline points="21 15 16 10 5 21"/>`) },
    ],
    [
      { id: "shortcuts", label: "Keyboard shortcuts", color: "#5AC8FA", icon: ic(`<rect x="2" y="6" width="20" height="12" rx="2"/><line x1="6" y1="10" x2="6.01" y2="10"/><line x1="10" y1="10" x2="10.01" y2="10"/><line x1="14" y1="10" x2="14.01" y2="10"/><line x1="18" y1="10" x2="18.01" y2="10"/><line x1="7" y1="14" x2="17" y2="14"/>`), hint: "Ctrl+/" },
      { id: "bots", label: "Bots", color: "#00C7BE", icon: ic(`<rect x="4" y="8" width="16" height="12" rx="2"/><path d="M12 8V4"/><circle cx="12" cy="3" r="1"/><line x1="9" y1="13" x2="9.01" y2="13"/><line x1="15" y1="13" x2="15.01" y2="13"/>`) },
      { id: "account", label: "Account & server", color: "#007AFF", icon: ic(`<rect x="2" y="2" width="20" height="8" rx="2"/><rect x="2" y="14" width="20" height="8" rx="2"/><line x1="6" y1="6" x2="6.01" y2="6"/><line x1="6" y1="18" x2="6.01" y2="18"/>`) },
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
    if (want && SECTIONS.some((x) => x.id === want)) {
      section = want as Sec;
      settingsSection.set(null);
      queueMicrotask(enterDetail);
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
  $effect(() => { if ($settingsOpen && section === "backups") void refreshBackup(); });
  function fmtWhen(sec: number): string {
    const d = new Date(sec * 1000);
    return d.toLocaleString();
  }
  function fmtBytes(n: number): string {
    return n > 1048576 ? `${(n / 1048576).toFixed(1)} MB` : `${Math.max(1, Math.round(n / 1024))} KB`;
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
  async function bkShowPhrase() {
    const ok = await askConfirm({ title: tr("Show the recovery key?"), text: tr("Anyone who sees these 24 words and gets into your e-mail can restore your account. Make sure nobody is looking at your screen."), ok: tr("Show") });
    if (!ok) return;
    try { bkPhrase = await api.backupPhrase(); if (!bkPhrase) showToast(tr("This device doesn't have the recovery key (backup opened with the password)")); } catch (e) { showToast(String(e)); }
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
  function setFontScale(v: number) { settings.update((s) => ({ ...s, fontScale: Math.max(0.85, Math.min(1.25, v)) })); }
  // ---- your stickers ----
  let myPacks = $state<StickerPack[]>([]);
  let openPack = $state<string | null>(null);
  let renaming = $state<string | null>(null);
  let renameTo = $state("");
  async function loadMyPacks() {
    try {
      const lib = await api.listLocalMedia();
      myPacks = lib.packs.filter((p) => !p.source && p.kind !== "emoji");
    } catch { myPacks = []; }
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
  let iconMode = $derived($settings.appIcon?.mode ?? "theme");
  let themeIconUrl = $derived.by(() => {
    void $settings.palette; void $settings.themePreset;
    const acc = typeof document !== "undefined" ? getComputedStyle(document.documentElement).getPropertyValue("--accent").trim() : "";
    return svgDataUrl(logoSvg(colorsFromAccent(/^#[0-9a-f]{6}$/i.test(acc) ? acc : "#00E5C7")));
  });
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

  async function pickCustomWallpaper() {
    try {
      const result = await openFileDialog({
        multiple: false, directory: false, title: "Pick a wallpaper",
        filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp", "gif"] }],
      });
      const path = Array.isArray(result) ? (result[0] ?? null) : result;
      if (!path) return;
      const bytes = await api.readUserImage(path);
      // Guess mime from extension for the data URL.
      const lower = path.toLowerCase();
      const mime = lower.endsWith(".png") ? "image/png"
        : lower.endsWith(".webp") ? "image/webp"
        : lower.endsWith(".gif") ? "image/gif"
        : "image/jpeg";
      const url = bytesToDataUrl(bytes, mime);
      const css = `linear-gradient(rgba(12,15,23,0.35), rgba(12,15,23,0.35)), url("${url}")`;
      settings.update((s) => ({ ...s, wallpaper: "custom", wallpaperCustom: css }));
      showToast(tr("Wallpaper set"));
    } catch (e) {
      showToast(`Could not set wallpaper: ${e}`);
    }
  }

  function toggle(key: "enterToSend" | "autoDownloadMedia" | "linkPreviews" | "notifySound" | "notifyDesktop") {
    settings.update((s) => ({ ...s, [key]: !s[key] }));
  }
  async function enableDesktopNotifications() {
    settings.update((s) => ({ ...s, notifyDesktop: !s.notifyDesktop }));
    // Request OS permission when turning it on.
    if (!$settings.notifyDesktop && "Notification" in window) {
      try { await Notification.requestPermission(); } catch { /* ignore */ }
    }
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
</script>

{#if $settingsOpen}
  <div
    class="backdrop"
    role="button"
    tabindex="-1"
    aria-label={$t("Close settings")}
    onclick={(e) => { if ((e.target as HTMLElement).classList.contains("backdrop")) closeAll(); }}
    onkeydown={(e) => { if (e.key === "Escape" || e.key === "Enter") closeAll(); }}
  >
    <div class="sheet" class:detail role="dialog" aria-modal="true" tabindex="-1">
      <div class="layout">
        <nav class="nav">
          <div class="list-head phone-only">
            <h2>{$t("Settings")}</h2>
            <button class="close" onclick={closeAll} title={$t("Close")}>×</button>
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
              {#each g.filter((x) => !(x.id === "shortcuts" && (phone || TOUCH))) as it}
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
            <button class="close" onclick={closeAll} title={$t("Close (Esc)")}>×</button>
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
            <div class="seg">
              {#each ([[1, "1 min"], [5, "5 min"], [15, "15 min"], [60, "1 hour"], [0, "Off"]] as const) as [m, l] (m)}
                <button class="seg-opt" class:active={$lockConf.autoMin === m} onclick={() => setAutoLock(m)}>{$t(l)}</button>
              {/each}
            </div>
          </div>
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
          <div class="foot-note">{$t("Nobody is blocked. Block someone from their profile.")}</div>
        {:else}
          {#each [...$blocked] as u (u)}
            <div class="info-row"><span class="v">@{u}</span><button class="mini-btn" onclick={() => setBlocked(u, false)}>{$t("Unblock")}</button></div>
          {/each}
        {/if}
      </section>
      <section class="group">
        <div class="label">{$t("Who sees what")}</div>
        <div class="row">
          <span class="row-label">{$t("Status")}</span>
          <div class="seg">
            {#each ([["online", "Online"], ["idle", "Idle"], ["dnd", "Busy"], ["invisible", "Invisible"]] as const) as [id, lbl]}
              <button class="seg-opt" class:active={$myOnline === id} onclick={() => myOnline.set(id)}>{$t(lbl)}</button>
            {/each}
          </div>
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
          <button class="mini-btn primary" onclick={() => newSticker()}>＋ {$t("New sticker")}</button>
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
                  <button class="mp-add" onclick={() => newSticker(p.title)} title={$t("New sticker")}>＋</button>
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
        <div class="label">{$t("Panel")}</div>
        <div class="foot-note">
          {$t("Open with the smiley in the message box or")} <b>{$t("Ctrl+E")}</b>{$t(". Drop GIF / PNG / TGS files onto the panel to add them, import any Telegram sticker or emoji pack from")} <b>+</b>{$t(", and right-click a sticker or GIF in a chat to save it.")}
        </div>
      </section>
      {/if}

      {#if section === "appearance"}
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
          <span class="row-label">{$t("Mode")}</span>
          <div class="seg">
            {#each (["dark", "light", "auto"] as const) as m}
              <button
                class="seg-opt"
                class:active={$settings.mode === m}
                onclick={() => setMode(m)}
              >{m}</button>
            {/each}
          </div>
        </div>

        <div class="row">
          <span class="row-label">{$t("Density")}</span>
          <div class="seg">
            {#each (["comfortable", "compact"] as const) as d}
              <button
                class="seg-opt"
                class:active={$settings.density === d}
                onclick={() => setDensity(d)}
              >{d}</button>
            {/each}
          </div>
        </div>

        <div class="row">
          <span class="row-label">{$t("Font size")}</span>
          <div class="range">
            <input
              type="range" min="0.85" max="1.25" step="0.05"
              value={$settings.fontScale}
              oninput={(e) => setFontScale(parseFloat((e.target as HTMLInputElement).value))}
            />
            <span class="range-val">{Math.round($settings.fontScale * 100)}%</span>
          </div>
        </div>
      </section>
      {/if}

      {#if section === "behaviour"}
      <section class="group">
        <div class="label">{$t("Chat look & media")}</div>
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
      </section>
      {/if}

      {#if section === "notifications"}
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
        <div class="label">{$t("Per-person sounds")}</div>
        {#each Object.entries($settings.chatSounds ?? {}) as [cid, sid] (cid)}
          <div class="row">
            <span class="row-label ell">{$chats.find((c) => c.id === cid)?.name ?? cid}</span>
            <select class="sel" value={sid} onchange={(e) => setChatSound(cid, (e.target as HTMLSelectElement).value)}>
              <option value="">{$t("Default")}</option>
              {#each allSounds as snd (snd.id)}<option value={snd.id}>{snd.name}</option>{/each}
            </select>
            <button class="icon-btn" onclick={() => playSound(sid)} aria-label={$t("Listen")}>▶</button>
          </div>
        {/each}
        <div class="row">
          <select class="sel" bind:value={perChatPick}>
            <option value="">{$t("Choose a chat…")}</option>
            {#each $chats.filter((c) => !($settings.chatSounds ?? {})[c.id]) as c (c.id)}<option value={c.id}>{c.name}</option>{/each}
          </select>
          <button class="mini-btn primary" disabled={!perChatPick} onclick={() => { setChatSound(perChatPick, $settings.messageSound === "ding" ? "chime" : "ding"); perChatPick = ""; }}>{$t("Add")}</button>
        </div>
        <div class="foot-note">{$t("Give someone their own sound — you'll know who wrote without looking.")}</div>
      </section>
      {/if}

      {#if section === "voice"}
        <VoiceVideoSettings />
      {/if}

      {#if section === "wallpaper"}
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
              <span class="wall-name">{w.name}</span>
            </button>
          {/each}
          <button
            class="wall"
            class:active={$settings.wallpaper === "custom"}
            onclick={pickCustomWallpaper}
            title={$t("Upload your own image")}
          >
            <span class="wall-swatch custom" style={$settings.wallpaper === "custom" && $settings.wallpaperCustom ? `background: ${$settings.wallpaperCustom}; background-size: cover;` : ""}>
              {#if $settings.wallpaper !== "custom" || !$settings.wallpaperCustom}
                <span class="wall-mark">+</span>
              {/if}
            </span>
            <span class="wall-name">{$t("Custom…")}</span>
          </button>
        </div>
      </section>
      {/if}

      {#if section === "accent"}
      <section class="group">
        <div class="label">{$t("App icon")}</div>
        <div class="icons-row">
          <button class="app-ic" class:active={iconMode === "theme"} onclick={() => setAppIcon({ mode: "theme" })} title={$t("Follows the accent colour")}>
            <img src={themeIconUrl} alt="" /><span>{$t("Theme")}</span>
          </button>
          {#each ICON_PRESETS as p (p.id)}
            <button class="app-ic" class:active={iconMode === "preset" && $settings.appIcon?.preset === p.id} onclick={() => setAppIcon({ mode: "preset", preset: p.id })} title={p.name}>
              <img src={svgDataUrl(logoSvg(p))} alt="" /><span>{p.name}</span>
            </button>
          {/each}
          <button class="app-ic" class:active={iconMode === "custom"} onclick={pickCustomIcon} title={$t("Your own picture")}>
            {#if $settings.appIcon?.custom}<img src={$settings.appIcon.custom} alt="" />{:else}<span class="plus">＋</span>{/if}<span>{$t("Custom…")}</span>
          </button>
        </div>
        <div class="foot-note">{$t("Shown in the title bar, the sidebar and the taskbar. “Theme” repaints the logo in your accent colour. On Wayland the taskbar uses the installed icon.")}</div>
      </section>
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
      {/if}

      {#if section === "theme"}
      <section class="group">
        <div class="label">{$t("Theme editor")}</div>
        <div class="te-intro">Override any colour. Changes apply live and are saved to this device. Based on the current {effMode()} mode.</div>

        <!-- Live preview — reflects your edits as you make them. -->
        <div class="te-preview">
          <div class="tp-head">
            <span class="tp-ava"></span>
            <div class="tp-meta"><span class="tp-name">{$t("Aquamarine")}</span><span class="tp-sub">{$t("online · preview")}</span></div>
          </div>
          <div class="tp-body">
            <div class="tp-in">{$t("Here's how incoming messages look.")}</div>
            <div class="tp-in dim">{$t("Muted and dim text share this bubble.")}</div>
            <div class="tp-out">{$t("And this is a message from you.")}</div>
          </div>
          <div class="tp-foot">
            <span class="tp-field">{$t("Type a message…")}</span>
            <span class="tp-send"></span>
          </div>
        </div>

        {#each COLOR_GROUPS as grp}
          <div class="te-group-title">{grp}</div>
          <div class="te-colors">
            {#each EDITABLE_COLORS.filter((c) => c.group === grp) as c (c.key)}
              <label class="te-color" title={c.desc}>
                <input
                  type="color"
                  value={colorValue(c.key, c.dark, c.light)}
                  oninput={(e) => setColor(c.key, (e.target as HTMLInputElement).value)}
                />
                <span class="te-swatch" style={`background:${colorValue(c.key, c.dark, c.light)}`}></span>
                <span class="te-name">{c.label}</span>
                <span class="te-help" title={c.desc}>*<span class="te-tip">{c.desc}</span></span>
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
      {/if}

      {#if section === "account"}
      <section class="group">
        <div class="label">{$t("Session")}</div>
        <div class="info-row">
          <span class="k">{$t("Username")}</span>
          <span class="v">{$info?.username ?? "—"}</span>
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
        <div class="label">{$t("Connect through a server")}</div>
        <div class="row">
          <input class="proxy-in" bind:value={proxyUrl} placeholder="socks5://127.0.0.1:1080" spellcheck="false" />
          <button class="mini-btn" onclick={saveProxy} disabled={proxyBusy}>{proxyBusy ? "…" : $t("Save")}</button>
        </div>
        {#if proxyNote}<div class="foot-note">{proxyNote}</div>{/if}
        <div class="foot-note">
          {$t("Routes all traffic through a local SOCKS5 proxy — e.g. an OpenFlux tunnel you run. Leave empty for a direct connection. Aquamarine doesn't run the tunnel; point this at one you've started.")}
        </div>
      </section>
      {/if}

      {#if section === "backups"}
      <section class="group">
        <div class="label">{$t("Encrypted backup")}</div>
        <div class="bots-note">{$t("Your keys, chats, groups and settings are sealed on this device and kept on your server, which can't read them. Sign in on a new device with your e-mail and password and everything comes back. Photos and videos are not included.")}</div>
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
            <button class="bot-link" onclick={bkShowPhrase}>{$t("Show recovery key")}</button>
            <span class="bot-sp"></span>
            <button class="bot-link danger" onclick={bkDisable}>{$t("Turn off")}</button>
          </div>
          <div class="bots-note">{$t("Backups run by themselves every 6 hours while you're connected.")}</div>
        {/if}
        {#if bkPhrase}
          <div class="bk-phrase">
            <div class="bk-ph-title">{$t("Recovery key — write it down and keep it offline")}</div>
            <ol>{#each bkPhrase.split(" ") as w, i (i)}<li>{w}</li>{/each}</ol>
            <div class="bots-note">{$t("If you forget the password, these 24 words still open the backup. Nobody, including the server admin, can recover it without one of them.")}</div>
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

      {#if section === "about"}
      <section class="group">
        <div class="label">{$t("About")}</div>
        <div class="about">
          <b>{$t("Aquamarine")}</b> {$t("— end-to-end encrypted desktop messenger.")}<br />
          {$t("X3DH · Double Ratchet · ChaCha20-Poly1305 · your keys never leave this device.")}
        </div>
        <button class="reset" onclick={reset} style="margin-top:14px">{$t("Reset all settings to defaults")}</button>
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
  .sheet {
    width: min(940px, 96vw);
    height: min(680px, 90vh);
    display: flex; flex-direction: column;
    background: var(--bg-shell);
    border: 1px solid var(--border);
    border-radius: 16px;
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
    padding: 12px; border-radius: 14px; border: none; cursor: pointer; text-align: left;
    background: var(--bg-surface); color: inherit;
  }
  .me-card:hover { background: var(--bg-hover); }
  .me-ava {
    position: relative; width: 58px; height: 58px; border-radius: 50%; flex-shrink: 0;
    display: grid; place-items: center; font-size: 22px; font-weight: 800; color: rgba(6, 7, 12, 0.85); overflow: hidden;
  }
  .me-ava img { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: cover; }
  .me-text { display: flex; flex-direction: column; min-width: 0; }
  .me-name { font-size: 16px; font-weight: 800; color: var(--fg); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .me-sub { font-size: 12.5px; color: var(--fg-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .me-online { font-size: 12px; color: var(--accent); }
  .me-online.off { color: var(--fg-dim); }
  .nav-group { background: var(--bg-surface); border-radius: 14px; overflow: hidden; }
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
    width: 28px; height: 28px; border-radius: 8px; flex-shrink: 0;
    display: grid; place-items: center; color: #fff;
  }
  .tile :global(svg) { width: 16px; height: 16px; }
  .nav-lbl { flex: 1; }
  .nav-hint { font-size: 11.5px; color: var(--fg-dim); }
  .chev { color: var(--fg-dim); font-size: 18px; line-height: 1; }
  .content { overflow-y: auto; padding: 0 0 14px; }
  .content .head { position: sticky; top: 0; z-index: 2; background: var(--bg-shell); }
  .mini-btn {
    flex-shrink: 0; border: 1px solid var(--border-strong); background: var(--bg-input); color: var(--fg);
    border-radius: 8px; padding: 4px 10px; font-size: 12px; font-weight: 600; cursor: pointer;
  }
  .mini-btn.primary { background: var(--accent); color: #06070C; border-color: transparent; }
  .mini-btn:disabled { opacity: 0.6; }
  .foot-note { font-size: 12px; color: var(--fg-dim); line-height: 1.55; margin-top: 8px; }
  .foot-note code { font-size: 11px; }
  .lang-row {
    display: flex; align-items: center; gap: 12px; width: 100%; padding: 10px 8px; border-radius: 10px;
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
    border: 1px solid var(--border); border-radius: 8px; padding: 7px 9px; font-size: 13px;
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
    background: transparent; border: none; border-radius: 10px;
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
      width: 100vw; height: 100%; border: none; border-radius: 0; box-shadow: none;
      padding: env(safe-area-inset-top, 0px) env(safe-area-inset-right, 0px)
               env(safe-area-inset-bottom, 0px) env(safe-area-inset-left, 0px);
      box-sizing: border-box;
    }
    .layout { grid-template-columns: 1fr; }
    .nav { border-right: none; padding: 12px 12px 20px; }
    .nav-item { padding: 11px 14px; font-size: 15.5px; }
    .nav-item + .nav-item::before { left: 56px; }
    .nav-item.active { background: transparent; }
    .nav-item.active .nav-lbl { color: var(--fg); font-weight: 500; }
    .tile { width: 30px; height: 30px; }
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
    border-radius: 8px; color: var(--fg-dim);
    font-size: 18px; line-height: 1; cursor: pointer;
  }
  .close:hover { color: var(--fg); background: var(--bg-hover); }

  .group {
    margin: 12px 16px 0;
    padding: 12px 16px 14px;
    background: var(--bg-surface);
    border-radius: 14px;
  }
  .label {
    font-size: 12px;
    font-weight: 700;
    color: var(--accent);
    margin-bottom: 10px;
  }

  .row { display: flex; align-items: center; gap: 14px; margin-bottom: 10px; }
  .row:last-child { margin-bottom: 0; }
  .row-label { flex: 0 0 90px; color: var(--fg-muted); font-size: 13px; }

  .seg {
    display: flex;
    background: var(--bg-input);
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 3px;
    gap: 2px;
  }
  .seg-opt {
    padding: 5px 12px;
    background: transparent;
    border: none;
    border-radius: 7px;
    color: var(--fg-muted);
    font-size: 12.5px;
    font-weight: 600;
    text-transform: capitalize;
    cursor: pointer;
    transition: all 0.12s ease;
  }
  .seg-opt:hover { color: var(--fg); }
  .seg-opt.active {
    background: var(--accent-soft);
    color: var(--accent);
    box-shadow: 0 0 0 1px color-mix(in srgb, var(--accent) 25%, transparent);
  }

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
    border-radius: 10px;
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
    border-radius: 8px;
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
    border: 1.5px solid var(--border); border-radius: 12px; background: transparent;
    color: var(--fg-muted); cursor: pointer; text-align: left;
  }
  .theme-card:hover { border-color: var(--border-strong); color: var(--fg); }
  .theme-card.active { border-color: var(--accent); color: var(--accent); background: var(--accent-soft); }
  .tc-prev {
    position: relative; display: block; height: 58px; border-radius: 8px; overflow: hidden;
    background: var(--s); border: 1px solid rgba(127, 127, 127, 0.18);
  }
  .tc-prev i { position: absolute; display: block; border-radius: 6px; }
  .tc-l { left: 0; top: 0; bottom: 0; width: 28%; border-radius: 0 !important; background: var(--l); }
  .tc-in { left: 34%; top: 12px; width: 44%; height: 12px; background: var(--b); }
  .tc-out { right: 8%; top: 32px; width: 40%; height: 12px; background: color-mix(in srgb, var(--a) 55%, var(--s)); }
  .tc-name { font-size: 11.5px; font-weight: 700; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .snd-list { display: flex; flex-direction: column; gap: 2px; }
  .snd { display: flex; align-items: center; gap: 4px; border-radius: 9px; }
  .snd.on { background: var(--accent-soft); }
  .snd-pick { flex: 1; display: flex; align-items: center; gap: 10px; padding: 7px 8px; border: none; background: none; color: var(--fg); font: inherit; font-size: 13.5px; cursor: pointer; text-align: left; }
  .snd-name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .snd-tag { font-size: 10.5px; font-weight: 700; color: var(--accent); }
  .icon-btn {
    flex: 0 0 30px; height: 30px; display: grid; place-items: center;
    border: none; border-radius: 8px; background: transparent; color: var(--fg-muted); cursor: pointer; font-size: 12px;
  }
  .icon-btn:hover { background: var(--bg-hover); color: var(--fg); }
  .sel {
    flex: 1; min-width: 0; height: 32px; padding: 0 8px; border-radius: 8px;
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
    border-radius: 10px;
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
  .info-row .mono { font-family: var(--font-mono); color: var(--fg-muted); font-size: 11.5px; }
  .info-row .dir { direction: rtl; text-align: left; }

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
    border-radius: 8px; color: var(--fg); font-family: inherit;
    font-size: 12.5px; padding: 0 10px; outline: none;
  }
  .bc-at { display: flex; align-items: center; gap: 2px; }
  .bc-at i { font-style: normal; color: var(--fg-dim); }
  .bc-at input { height: 100%; border: none; background: transparent; padding: 0; }
  .bc-field input:focus, .bc-at:focus-within { border-color: var(--accent); }
  .bot-add {
    height: 34px; padding: 0 16px; border: none; border-radius: 8px;
    background: linear-gradient(135deg, var(--accent), var(--accent-2));
    color: var(--accent-ink, #06070C); font-weight: 700; font-size: 12.5px; cursor: pointer;
  }
  .bot-add:disabled { opacity: 0.5; cursor: not-allowed; }
  .bot-list { display: flex; flex-direction: column; gap: 8px; }
  .bot-card {
    padding: 10px 12px; background: var(--bg-input);
    border: 1px solid var(--border); border-radius: 12px;
    display: flex; flex-direction: column; gap: 8px;
  }
  .bot-head { display: flex; align-items: center; gap: 10px; }
  .bot-ava {
    width: 34px; height: 34px; border-radius: 50%; flex-shrink: 0;
    display: grid; place-items: center; font-weight: 700; font-size: 14px;
    background: linear-gradient(135deg, var(--accent), var(--accent-2)); color: var(--accent-ink, #06070C);
  }
  .bot-main { flex: 1; min-width: 0; }
  .bot-name { font-size: 13px; font-weight: 600; display: flex; align-items: center; gap: 6px; }
  .bot-tag {
    font-size: 9px; font-weight: 800; letter-spacing: 0.06em; padding: 1px 5px;
    border-radius: 4px; background: color-mix(in srgb, var(--accent) 18%, transparent); color: var(--accent);
  }
  .bot-user { font-size: 11.5px; color: var(--fg-dim); }
  .bot-tokrow, .bot-cmd { display: flex; align-items: center; gap: 6px; }
  .bot-token, .bot-cmd code {
    flex: 1; min-width: 0; height: 28px; line-height: 28px; padding: 0 8px; box-sizing: border-box;
    font-family: var(--font-mono); font-size: 10.5px; color: var(--fg-muted);
    background: var(--bg-shell); border: 1px solid var(--border); border-radius: 7px;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap; user-select: all;
  }
  .bot-cmd { margin-top: 8px; }
  .bot-btn {
    height: 28px; padding: 0 10px; border: 1px solid var(--border); border-radius: 7px;
    background: transparent; color: var(--fg-muted); font-size: 11.5px;
    font-weight: 600; cursor: pointer; flex-shrink: 0; white-space: nowrap;
  }
  .bot-btn:hover { color: var(--accent); border-color: color-mix(in srgb, var(--accent) 45%, transparent); }
  .bot-btn.primary { color: var(--accent); border-color: color-mix(in srgb, var(--accent) 45%, transparent); background: color-mix(in srgb, var(--accent) 10%, transparent); }
  .bot-actions { display: flex; align-items: center; gap: 4px; flex-wrap: wrap; margin-top: 2px; }
  .bot-sp { flex: 1; }
  .bot-link {
    border: none; background: none; padding: 4px 6px; border-radius: 6px;
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
    margin: 10px 0 0; padding: 10px; border-radius: 8px; overflow-x: auto;
    background: var(--bg-shell); border: 1px solid var(--border);
    font-family: var(--font-mono); font-size: 10.5px; line-height: 1.6; color: var(--fg-muted);
  }
  @media (max-width: 520px) {
    .bot-create { grid-template-columns: 1fr; }
  }

  .bk-row { display: flex; gap: 8px; margin-top: 10px; }
  .bk-row .txt { flex: 1; min-width: 0; }
  .bk-stat { display: flex; align-items: center; gap: 8px; font-size: 13px; margin: 10px 0 6px; }
  .bk-dot { width: 8px; height: 8px; border-radius: 50%; background: var(--fg-dim); }
  .bk-dot.on { background: #34C759; box-shadow: 0 0 0 3px rgba(52, 199, 89, 0.18); }
  .bk-phrase { margin-top: 12px; padding: 12px; border-radius: 12px; border: 1px dashed color-mix(in srgb, var(--accent) 50%, transparent); background: var(--bg-input); }
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
    border-radius: 9px; cursor: pointer;
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
    border: 1px solid var(--border-strong); border-radius: 9px;
    padding: 8px 10px; font-size: 11.5px; font-weight: 400; line-height: 1.45;
    box-shadow: 0 12px 30px -8px rgba(0,0,0,0.6);
    opacity: 0; pointer-events: none; transform: translateY(4px);
    transition: opacity 0.12s ease, transform 0.12s ease; z-index: 20;
    text-align: left;
  }
  .te-help:hover .te-tip { opacity: 1; transform: translateY(0); }

  /* Live preview card */
  .te-preview {
    border: 1px solid var(--border-strong); border-radius: 12px;
    overflow: hidden; margin-bottom: 14px; background: var(--bg-shell);
  }
  .tp-head {
    display: flex; align-items: center; gap: 9px;
    padding: 9px 12px; background: var(--bg-list);
    border-bottom: 1px solid var(--border);
  }
  .tp-ava { width: 30px; height: 30px; border-radius: 50%; background: linear-gradient(135deg, var(--accent), var(--accent-2)); flex-shrink: 0; }
  .tp-name { font-size: 13px; font-weight: 700; color: var(--fg); display: block; }
  .tp-sub { font-size: 10.5px; color: var(--fg-muted); }
  .tp-body { padding: 12px; display: flex; flex-direction: column; gap: 6px; background: var(--bg-shell); }
  .tp-in, .tp-out { max-width: 78%; padding: 7px 11px; font-size: 12.5px; border-radius: 4px 12px 12px 12px; }
  .tp-in { align-self: flex-start; background: var(--bg-bubble-in); color: var(--fg); border: 1px solid var(--border); }
  .tp-in.dim { color: var(--fg-muted); }
  .tp-out { align-self: flex-end; border-radius: 12px 4px 12px 12px; background: linear-gradient(135deg, color-mix(in srgb, var(--accent) 24%, var(--bg-bubble-in)), var(--bg-bubble-in)); color: var(--fg); border: 1px solid var(--accent-soft); }
  .tp-foot { display: flex; align-items: center; gap: 8px; padding: 9px 12px; border-top: 1px solid var(--border); background: var(--bg-list); }
  .tp-field { flex: 1; font-size: 12px; color: var(--fg-dim); background: var(--bg-input); border: 1px solid var(--border); border-radius: 8px; padding: 6px 10px; }
  .tp-send { width: 28px; height: 28px; border-radius: 50%; background: linear-gradient(135deg, var(--accent), var(--accent-2)); flex-shrink: 0; }
  /* Hide the native colour input; the swatch is the visible control. */
  .te-color input[type="color"] {
    position: absolute; width: 0; height: 0; opacity: 0; pointer-events: none;
  }
  .te-swatch {
    width: 22px; height: 22px; border-radius: 6px;
    border: 1px solid var(--border-strong); flex-shrink: 0;
  }
  .te-name { font-size: 12.5px; color: var(--fg); font-weight: 600; }

  .te-font { margin-top: 2px; }
  .te-font-cur {
    font-size: 13px; color: var(--fg); font-weight: 600;
    padding: 8px 10px; background: var(--bg-input);
    border: 1px solid var(--border); border-radius: 9px; margin-bottom: 8px;
  }
  .te-font-btns { display: flex; gap: 6px; }
  .te-btn {
    padding: 7px 12px; border-radius: 8px;
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
    border-radius: 10px;
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
    border-radius: 10px;
    font-size: 13px;
    font-weight: 700;
    cursor: pointer;
    box-shadow: 0 6px 16px -4px var(--accent-glow);
  }
  .done:hover { transform: translateY(-1px); }
  .icons-row { display: flex; flex-wrap: wrap; gap: 10px; }
  .app-ic { display: flex; flex-direction: column; align-items: center; gap: 5px; padding: 8px; width: 76px;
    border: 1px solid var(--border); border-radius: 14px; background: var(--bg-input); color: var(--fg-muted);
    font-size: 11px; cursor: pointer; transition: transform .25s cubic-bezier(.2,.8,.2,1), border-color .2s, background .2s; }
  .app-ic:hover { transform: translateY(-2px); }
  .app-ic.active { border-color: var(--accent); background: var(--accent-soft); color: var(--fg); }
  .app-ic img, .app-ic .plus { width: 48px; height: 48px; border-radius: 12px; object-fit: cover; }
  .app-ic .plus { display: grid; place-items: center; font-size: 22px; border: 1px dashed var(--border-strong); }
  .my-actions { display: flex; gap: 8px; flex-wrap: wrap; margin: 4px 0 10px; }
  .my-pack { border: 1px solid var(--border); border-radius: 12px; margin-bottom: 8px; overflow: hidden; transition: border-color .2s; }
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
  .mp-item { position: relative; aspect-ratio: 1; display: grid; place-items: center; border-radius: 10px; background: var(--bg-input); }
  .mp-item :global(img), .mp-item :global(video) { max-width: 88%; max-height: 88%; }
  .mp-emoji { position: absolute; left: 4px; bottom: 2px; font-size: 13px; }
  .mp-del { position: absolute; top: 3px; right: 3px; width: 20px; height: 20px; border-radius: 50%; border: none; background: rgba(0,0,0,.6); color: #fff;
    cursor: pointer; opacity: 0; transition: opacity .15s; line-height: 1; }
  .mp-item:hover .mp-del { opacity: 1; }
  .mp-add { aspect-ratio: 1; border-radius: 10px; border: 1.5px dashed var(--border-strong); background: none; color: var(--fg-dim); font-size: 22px; cursor: pointer; }
  .mp-add:hover { color: var(--accent); border-color: var(--accent); }
  .mini-btn.danger { color: #ff6b6b; }
</style>
