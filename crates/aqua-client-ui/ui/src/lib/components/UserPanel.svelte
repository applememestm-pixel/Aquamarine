<script lang="ts">
  import { t, tr } from "../i18n";
  /**
   * Discord-style user panel (bottom-left): avatar with status, name,
   * what you're doing, and quick mic / deafen / settings toggles. Clicking
   * the identity opens a mini card with the status picker.
   */
  import { onMount } from "svelte";
  import { info } from "../stores";
  import { profile, updateProfile } from "../profile";
  import { myOnline, myActivity, shareActivity, activityVerb, activityLine, broadcastPresence, type OnlineStatus } from "../presence";
  import { callMuted, callDeafened, callState, toggleCallMute, toggleCallDeafen } from "./CallModal.svelte";
  import { openSettings } from "./SettingsPanel.svelte";
  import { openProfile } from "./ProfileModal.svelte";
  import { showToast } from "./Toast.svelte";

  let menuOpen = $state(false);
  let statusDraft = $state("");
  let root: HTMLDivElement | undefined = $state();

  const STATUSES: { id: OnlineStatus; label: string; hint: string }[] = [
    { id: "online", label: "Online", hint: "" },
    { id: "idle", label: "Idle", hint: "" },
    { id: "dnd", label: "Do Not Disturb", hint: "You won't get notification sounds" },
    { id: "invisible", label: "Invisible", hint: "You'll look offline, but can still chat" },
  ];

  const name = $derived($profile.display_name && $profile.display_name !== "You" ? $profile.display_name : ($info?.username ?? "you"));
  const letter = $derived(name.charAt(0).toUpperCase() || "?");
  const act = $derived($shareActivity ? $myActivity[0] : undefined);
  const subline = $derived.by(() => {
    if ($callState.kind === "connected") return "In a call";
    if (act) return `${activityVerb(act)} ${act.name}`;
    if ($profile.status) return $profile.status;
    return $myOnline === "invisible" ? "Invisible" : `@${$info?.username ?? ""}`;
  });

  function toggleMenu() {
    menuOpen = !menuOpen;
    if (menuOpen) statusDraft = $profile.status ?? "";
  }

  function setOnline(s: OnlineStatus) {
    myOnline.set(s);
  }

  function saveStatus() {
    const v = statusDraft.trim().slice(0, 128);
    if (v === ($profile.status ?? "")) return;
    updateProfile({ status: v });
    broadcastPresence();
    showToast(v ? "Status updated" : "Status cleared");
  }

  async function copyUsername() {
    try { await navigator.clipboard.writeText(`@${$info?.username ?? ""}`); showToast(tr("Username copied")); }
    catch { showToast(tr("Copy blocked")); }
  }

  onMount(() => {
    const onDown = (e: MouseEvent) => {
      if (menuOpen && root && !root.contains(e.target as Node)) { saveStatus(); menuOpen = false; }
    };
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape" && menuOpen) { menuOpen = false; } };
    window.addEventListener("mousedown", onDown);
    window.addEventListener("keydown", onKey);
    return () => { window.removeEventListener("mousedown", onDown); window.removeEventListener("keydown", onKey); };
  });

  const svg = (p: string) => `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">${p}</svg>`;
  const SLASH = `<line x1="3" y1="3" x2="21" y2="21"/>`;
  const MIC = `<rect x="9" y="2" width="6" height="12" rx="3"/><path d="M5 10v1a7 7 0 0 0 14 0v-1"/><line x1="12" y1="18" x2="12" y2="22"/>`;
  const HEAD = `<path d="M3 18v-6a9 9 0 0 1 18 0v6"/><path d="M21 19a2 2 0 0 1-2 2h-1v-6h3zM3 19a2 2 0 0 0 2 2h1v-6H3z"/>`;
  const GEAR = `<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/>`;
</script>

<div class="user-panel" bind:this={root}>
  {#if menuOpen}
    <div class="card" role="dialog" aria-label={$t("Your status")}>
      <div class="card-banner" style:background={$profile.banner_url ? `center / cover no-repeat url("${$profile.banner_url}")` : `linear-gradient(135deg, ${$profile.banner_a}, ${$profile.banner_b})`}></div>
      <div class="card-ava-wrap">
        <div class="ava big" style:background={`linear-gradient(135deg, ${$profile.banner_a}, ${$profile.banner_b})`}>
          {#if $profile.avatar_url}<img src={$profile.avatar_url} alt="" />{:else}{letter}{/if}
          <span class="dot big {$myOnline}"></span>
        </div>
      </div>
      <div class="card-body">
        <div class="card-name">{name}</div>
        <button class="card-user" onclick={copyUsername} title={$t("Copy username")}>@{$info?.username ?? ""}</button>
        {#if act}
          <div class="card-act">
            <div class="act-k">{activityVerb(act)}</div>
            <div class="act-n">{act.name}</div>
            {#if activityLine(act)}<div class="act-d">{activityLine(act)}</div>{/if}
          </div>
        {/if}
        <input
          class="status-input"
          placeholder={$t("Set a custom status")}
          maxlength="128"
          bind:value={statusDraft}
          onkeydown={(e) => { if (e.key === "Enter") { saveStatus(); (e.target as HTMLInputElement).blur(); } }}
          onblur={saveStatus}
        />
        <div class="sep"></div>
        {#each STATUSES as st}
          <button class="menu-item" class:active={$myOnline === st.id} onclick={() => setOnline(st.id)}>
            <span class="dot {st.id}"></span>
            <span class="mi-text"><span class="mi-label">{$t(st.label)}</span>{#if st.hint}<span class="mi-hint">{$t(st.hint)}</span>{/if}</span>
          </button>
        {/each}
        <div class="sep"></div>
        <button class="menu-item" onclick={() => { menuOpen = false; openProfile(); }}>{$t("Edit profile")}</button>
        <button class="menu-item" onclick={() => { shareActivity.update((v) => !v); }}>
          <span class="mi-text"><span class="mi-label">{$t($shareActivity ? "Hide my activity" : "Share my activity")}</span><span class="mi-hint">{$t("Games, music and videos you have open")}</span></span>
        </button>
      </div>
    </div>
  {/if}

  <button class="who" onclick={toggleMenu} title={$t("Set status")}>
    <div class="ava" style:background={`linear-gradient(135deg, ${$profile.banner_a}, ${$profile.banner_b})`}>
      {#if $profile.avatar_url}<img src={$profile.avatar_url} alt="" />{:else}{letter}{/if}
      <span class="dot {$info?.connected ? $myOnline : 'offline'}"></span>
    </div>
    <div class="who-text">
      <div class="who-name">{name}</div>
      <div class="who-sub">{subline}</div>
    </div>
  </button>
  <div class="btns">
    <button class="ub" class:off={$callMuted} onclick={toggleCallMute} title={$t($callMuted ? "Unmute (Ctrl+Shift+M)" : "Mute (Ctrl+Shift+M)")} aria-label={$t("Toggle microphone")}>
      {@html svg(MIC + ($callMuted ? SLASH : ""))}
    </button>
    <button class="ub" class:off={$callDeafened} onclick={toggleCallDeafen} title={$t($callDeafened ? "Undeafen (Ctrl+Shift+D)" : "Deafen (Ctrl+Shift+D)")} aria-label={$t("Toggle sound")}>
      {@html svg(HEAD + ($callDeafened ? SLASH : ""))}
    </button>
    <button class="ub" onclick={() => openSettings()} title={$t("Settings (Ctrl+,)")} aria-label={$t("Settings")}>{@html svg(GEAR)}</button>
  </div>
</div>

<style>
  .user-panel {
    grid-column: 1 / span 2;
    grid-row: 2;
    position: relative;
    display: flex; align-items: center; gap: 4px;
    margin: 0 8px 8px;
    padding: 6px 6px 6px 6px;
    background: var(--bg-input);
    border: 1px solid var(--border);
    border-radius: 12px;
    z-index: 30;
  }
  .who {
    flex: 1; min-width: 0;
    display: flex; align-items: center; gap: 9px;
    background: transparent; border: none; color: inherit; cursor: pointer;
    padding: 3px 6px 3px 3px; border-radius: 8px; text-align: left;
  }
  .who:hover { background: var(--bg-hover); }
  .ava {
    position: relative; flex-shrink: 0;
    width: 34px; height: 34px; border-radius: 50%;
    display: grid; place-items: center;
    color: rgba(6, 7, 12, 0.85); font-weight: 700; font-size: 14px;
  }
  .ava img { position: absolute; inset: 0; width: 100%; height: 100%; border-radius: 50%; object-fit: cover; }
  .ava.big { width: 76px; height: 76px; font-size: 30px; border: 5px solid var(--bg-surface); box-sizing: content-box; }
  .dot {
    position: absolute; right: -2px; bottom: -2px;
    width: 10px; height: 10px; border-radius: 50%;
    border: 3px solid var(--bg-input);
    background: var(--fg-dim);
  }
  .dot.big { width: 16px; height: 16px; right: 0; bottom: 0; border-color: var(--bg-surface); }
  .menu-item .dot { position: static; border: none; width: 10px; height: 10px; flex-shrink: 0; }
  .dot.online { background: #3BA55D; }
  .dot.idle { background: #FAA61A; }
  .dot.dnd { background: #ED4245; }
  .dot.invisible, .dot.offline { background: #80848E; }
  .who-text { min-width: 0; }
  .who-name { font-size: 13.5px; font-weight: 700; color: var(--fg); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .who-sub { font-size: 11.5px; color: var(--fg-dim); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .btns { display: flex; gap: 2px; flex-shrink: 0; }
  .ub {
    width: 32px; height: 32px; border-radius: 8px; border: none;
    background: transparent; color: var(--fg-muted); cursor: pointer;
    display: grid; place-items: center;
  }
  .ub :global(svg) { width: 18px; height: 18px; }
  .ub:hover { background: var(--bg-hover); color: var(--fg); }
  .ub.off { color: #ED4245; }
  .ub.off:hover { background: rgba(237, 66, 69, 0.12); }

  .card {
    position: absolute; left: 0; bottom: calc(100% + 8px);
    width: 300px;
    background: var(--bg-surface);
    border: 1px solid var(--border-strong);
    border-radius: 14px;
    box-shadow: 0 18px 48px -12px rgba(0, 0, 0, 0.7);
    overflow: hidden;
    animation: pop 0.14s ease-out;
  }
  @keyframes pop { from { opacity: 0; transform: translateY(6px); } to { opacity: 1; transform: none; } }
  .card-banner { height: 64px; }
  .card-ava-wrap { margin: -40px 0 0 14px; }
  .card-body { padding: 8px 14px 12px; }
  .card-name { font-size: 18px; font-weight: 800; color: var(--fg); }
  .card-user { background: none; border: none; padding: 0; color: var(--fg-muted); font-size: 13px; cursor: pointer; }
  .card-user:hover { color: var(--accent); }
  .card-act { margin-top: 10px; padding: 8px 10px; border-radius: 8px; background: var(--bg-input); }
  .act-k { font-size: 10.5px; font-weight: 800; text-transform: uppercase; letter-spacing: 0.06em; color: var(--fg-dim); }
  .act-n { font-size: 13px; font-weight: 700; color: var(--fg); margin-top: 2px; }
  .act-d { font-size: 12px; color: var(--fg-muted); }
  .status-input {
    width: 100%; box-sizing: border-box; margin-top: 10px;
    background: var(--bg-input); color: var(--fg);
    border: 1px solid var(--border); border-radius: 8px;
    padding: 7px 9px; font-size: 13px; font-family: inherit;
  }
  .status-input:focus { outline: none; border-color: var(--accent); }
  .sep { height: 1px; background: var(--border); margin: 8px 0; }
  .menu-item {
    width: 100%; display: flex; align-items: center; gap: 10px;
    background: transparent; border: none; color: var(--fg);
    padding: 7px 8px; border-radius: 6px; cursor: pointer; text-align: left;
    font-size: 13px; font-weight: 600;
  }
  .menu-item:hover { background: var(--accent-soft); color: var(--accent); }
  .menu-item.active { background: var(--bg-hover); }
  .mi-text { display: flex; flex-direction: column; min-width: 0; }
  .mi-hint { font-size: 11px; font-weight: 500; color: var(--fg-dim); }

  @media (max-width: 720px) { .user-panel { display: none; } }
</style>
