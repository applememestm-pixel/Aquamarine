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
  import { settings, PALETTES, WALLPAPERS, EDITABLE_COLORS, type Mode, type Density } from "../theme";
  import { info } from "../stores";
  import { icons } from "../icons";
  import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
  import { api, bytesToDataUrl } from "../tauri";
  import { showToast } from "./Toast.svelte";
  import type { BotInfo } from "../types";
  import VoiceVideoSettings from "./VoiceVideoSettings.svelte";
  import AnimatedMedia from "./AnimatedMedia.svelte";
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

  // ---- Section navigation ----
  type Sec = "ghost" | "language" | "appearance" | "theme" | "behaviour" | "notifications" | "voice" | "wallpaper" | "accent" | "bots" | "account" | "about" | "privacy" | "stickers";
  let section = $state<Sec>("notifications");
  // Telegram-mobile style: coloured icon tiles in grouped lists.
  interface Item { id: Sec | "profile" | "shortcuts"; label: string; color: string; icon: string; hint?: string }
  const ic = (p: string) => `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">${p}</svg>`;
  const GROUPS: Item[][] = [
    [
      { id: "profile", label: "Edit profile", color: "#007AFF", icon: ic(`<path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"/><circle cx="12" cy="7" r="4"/>`) },
      { id: "ghost", label: "Ghost mode", color: "#5E5CE6", icon: ic(`<path d="M9 10h.01"/><path d="M15 10h.01"/><path d="M12 2a8 8 0 0 0-8 8v12l3-3 2.5 2.5L12 19l2.5 2.5L17 19l3 3V10a8 8 0 0 0-8-8z"/>`) },
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
    const u = botUsername.trim().toLowerCase();
    if (!/^[a-z0-9_.]{3,32}$/.test(u)) { botError = "Username: 3–32 chars, a-z 0-9 _ ."; return; }
    botBusy = true;
    try {
      await api.createBot(u, botDisplay.trim() || u);
      botUsername = ""; botDisplay = "";
      await loadBots();
      showToast(`Bot @${u} created`);
    } catch (e) {
      botError = String(e);
    } finally {
      botBusy = false;
    }
  }

  async function copyToken(token: string) {
    try { await navigator.clipboard.writeText(token); showToast(tr("Token copied")); }
    catch { showToast(tr("Copy blocked")); }
  }

  // Load bots whenever the panel opens.
  $effect(() => {
    if ($settingsOpen) void loadBots();
  });

  function setMode(m: Mode) { settings.update((s) => ({ ...s, mode: m })); }
  function setPalette(id: string) { settings.update((s) => ({ ...s, palette: id })); }
  function setDensity(d: Density) { settings.update((s) => ({ ...s, density: d })); }
  function setFontScale(v: number) { settings.update((s) => ({ ...s, fontScale: Math.max(0.85, Math.min(1.25, v)) })); }
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

  function reset() {
    settings.set({ mode: "dark", palette: "aqua", density: "comfortable", fontScale: 1, wallpaper: "aurora",
      enterToSend: true, autoDownloadMedia: true, linkPreviews: true, notifySound: true, notifyDesktop: false,
      customColors: {}, customFontName: undefined, customFontData: undefined });
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
          {#each quickReactions($reactionPack.items) as r (r)}
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
        <div class="label">{$t("Panel")}</div>
        <div class="foot-note">
          {$t("Open with the smiley in the message box or")} <b>{$t("Ctrl+E")}</b>{$t(". Drop GIF / PNG / TGS files onto the panel to add them, import any Telegram sticker or emoji pack from")} <b>+</b>{$t(", and right-click a sticker or GIF in a chat to save it.")}
        </div>
      </section>
      {/if}

      {#if section === "appearance"}
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

        <button class="reset" style="margin-top:16px" onclick={clearColors}>{$t("Clear custom colours")}</button>
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

      {#if section === "bots"}
      <section class="group">
        <div class="label">{$t("Bots")}</div>
        {#if !$info?.connected}
          <div class="bots-note">{$t("Connect to a server to create bots.")}</div>
        {:else}
          <div class="bot-create">
            <input bind:value={botUsername} placeholder={$t("bot username (e.g. weatherbot)")} oninput={() => (botUsername = botUsername.toLowerCase())} />
            <input bind:value={botDisplay} placeholder={$t("display name")} />
            <button class="bot-add" onclick={createBot} disabled={botBusy}>{$t(botBusy ? "…" : "Create")}</button>
          </div>
          {#if botError}<div class="bots-note err">{botError}</div>{/if}
          {#if bots.length === 0}
            <div class="bots-note">No bots yet. Create one — you'll get an API token to drive it over HTTP (<code>/v1/bot/updates</code>, <code>/v1/bot/send</code>).</div>
          {:else}
            {#each bots as b (b.username)}
              <div class="bot-row">
                <div class="bot-main">
                  <div class="bot-name">@{b.username}</div>
                  <div class="bot-token" title={b.token}>{b.token}</div>
                </div>
                <button class="bot-copy" onclick={() => copyToken(b.token)} title={$t("Copy token")}>{$t("Copy")}</button>
              </div>
            {/each}
          {/if}
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
    box-shadow: 0 0 0 1px rgba(0, 229, 199, 0.25);
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

  .bots-note { font-size: 12px; color: var(--fg-dim); line-height: 1.5; margin-top: 4px; }
  .bots-note.err { color: #ff9fb0; }
  .bots-note code { font-family: var(--font-mono); font-size: 11px; color: var(--accent); }
  .bot-create { display: flex; gap: 6px; margin-bottom: 8px; }
  .bot-create input {
    flex: 1; min-width: 0;
    background: var(--bg-input); border: 1px solid var(--border);
    border-radius: 8px; color: var(--fg); font-family: inherit;
    font-size: 12.5px; padding: 7px 10px; outline: none;
  }
  .bot-create input:focus { border-color: var(--accent); }
  .bot-add {
    padding: 7px 14px; border: none; border-radius: 8px;
    background: linear-gradient(135deg, var(--accent), var(--accent-2));
    color: #06070C; font-weight: 700; font-size: 12.5px; cursor: pointer;
    flex-shrink: 0;
  }
  .bot-add:disabled { opacity: 0.5; cursor: not-allowed; }
  .bot-row {
    display: flex; align-items: center; gap: 8px;
    padding: 8px 10px; margin-bottom: 4px;
    background: var(--bg-input); border: 1px solid var(--border);
    border-radius: 10px;
  }
  .bot-main { flex: 1; min-width: 0; }
  .bot-name { font-size: 13px; font-weight: 600; }
  .bot-token {
    font-family: var(--font-mono); font-size: 10.5px; color: var(--fg-dim);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .bot-copy {
    padding: 5px 10px; border: 1px solid var(--border); border-radius: 7px;
    background: transparent; color: var(--fg-muted); font-size: 11px;
    font-weight: 600; cursor: pointer; flex-shrink: 0;
  }
  .bot-copy:hover { color: var(--accent); border-color: var(--accent-soft); }

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
</style>
