<script lang="ts">
  /**
   * Group / channel voice chat UI. `stage` = the big Discord-style panel at
   * the top of the chat; `pill` = a small "you're in a call" bar shown
   * while you look at another chat.
   */
  import { groupCall, activeGroupCalls, joinGroupCall, leaveGroupCall, toggleGroupMute, toggleGroupDeafen, toggleGroupCamera, toggleGroupScreen } from "../groupCall";
  import { groupMap } from "../groups";
  import { info, selectChat } from "../stores";
  import { avatars } from "../people";
  import { isAdmin } from "../groupRules";
  import { t } from "../i18n";
  import { spatial, setSpatial } from "../spatial";
  import SpatialMap from "./SpatialMap.svelte";

  let { gid, mode = "stage", roomName = "", chatOpen = false, onToggleChat, onExpand }:
    { gid: string; mode?: "stage" | "pill" | "full"; roomName?: string; chatOpen?: boolean; onToggleChat?: () => void; onExpand?: () => void } = $props();
  let collapsed = $state(false);
  let focus = $state<string | null>(null);
  let now = $state(Date.now());
  $effect(() => { const i = setInterval(() => (now = Date.now()), 1000); return () => clearInterval(i); });

  const g = $derived($groupMap[gid]);
  const mine = $derived($groupCall && $groupCall.gid === gid ? $groupCall : null);
  const ongoing = $derived($activeGroupCalls[gid]);
  const isChannel = $derived(g?.kind === "channel");
  const canSpeak = $derived(!!g && (!isChannel || isAdmin(g, $info?.username ?? "")));
  const elapsed = $derived(mine ? fmt(now - mine.started) : ongoing ? fmt(now - ongoing.since) : "");

  function fmt(ms: number) {
    const s = Math.max(0, Math.floor(ms / 1000));
    const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), x = s % 60;
    return (h ? h + ":" + String(m).padStart(2, "0") : String(m)) + ":" + String(x).padStart(2, "0");
  }

  function srcObject(node: HTMLMediaElement, stream: MediaStream | null) {
    node.srcObject = stream;
    return { update(s: MediaStream | null) { if (node.srcObject !== s) node.srcObject = s; } };
  }

  interface Tile { user: string; me: boolean; stream: MediaStream | null; video: boolean; muted: boolean; speaking: boolean; state: string }
  const tiles = $derived.by((): Tile[] => {
    if (!mine) return [];
    const self: Tile = { user: $info?.username ?? "me", me: true, stream: mine.local, video: mine.camera || mine.screen, muted: mine.muted || mine.listener, speaking: mine.meSpeaking, state: "connected" };
    const others = Object.values(mine.peers).filter((p) => !isChannel || (g && isAdmin(g, p.user))).map((p) => ({ user: p.user, me: false, stream: p.stream, video: p.video, muted: p.muted, speaking: p.speaking, state: p.state }));
    return mine.listener ? others : [self, ...others];
  });
  const shown = $derived(focus ? tiles.filter((x) => x.user === focus) : tiles);
  let showMap = $state(false);
  function toggleSpatial() {
    if (!mine) return;
    const on = !$spatial.on;
    setSpatial(on, Object.fromEntries(Object.values(mine.peers).map((p) => [p.user, p.stream])), gid);
    showMap = on;
  }
  const speakingMap = $derived(mine ? Object.fromEntries(Object.values(mine.peers).map((p) => [p.user, p.speaking && !p.muted])) : {});
</script>

{#if mode === "pill"}
  {#if mine}
    <div class="pill">
      <span class="live-dot"></span>
      <button class="pill-main" onclick={() => selectChat(gid)}>
        <b>{isChannel ? $t("Live stream") : $t("Voice chat")}</b><span>{g?.title ?? ""} · {elapsed}</span>
      </button>
      {#if !mine.listener}
        <button class="pb" class:off={mine.muted} onclick={toggleGroupMute} title={mine.muted ? $t("Unmute") : $t("Mute")}>
          {#if mine.muted}<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><line x1="2" y1="2" x2="22" y2="22"/><path d="M18.9 13A7 7 0 0 0 19 12v-1"/><path d="M5 11v1a7 7 0 0 0 12 5"/><path d="M15 9.3V5a3 3 0 0 0-5.7-1.3"/><path d="M9 9v3a3 3 0 0 0 5.1 2.1"/><line x1="12" y1="19" x2="12" y2="22"/></svg>
          {:else}<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><rect x="9" y="2" width="6" height="12" rx="3"/><path d="M5 10v1a7 7 0 0 0 14 0v-1"/><line x1="12" y1="18" x2="12" y2="22"/></svg>{/if}
        </button>
      {/if}
      <button class="pb leave" onclick={leaveGroupCall} title={$t("Leave")}><svg viewBox="0 0 24 24" fill="currentColor"><path d="M12 9c-1.6 0-3.15.25-4.6.72v3.1c0 .39-.23.74-.56.9-.98.49-1.87 1.12-2.66 1.85-.18.18-.43.28-.7.28-.28 0-.53-.11-.71-.29L.29 13.08a.996.996 0 0 1 0-1.41C3.34 8.78 7.46 7 12 7s8.66 1.78 11.71 4.67c.18.18.29.43.29.71 0 .28-.11.53-.29.71l-2.48 2.48c-.18.18-.43.29-.71.29-.27 0-.52-.11-.7-.28a11.27 11.27 0 0 0-2.67-1.85.996.996 0 0 1-.56-.9v-3.1C15.15 9.25 13.6 9 12 9z"/></svg></button>
    </div>
  {/if}
{:else if mine}
  <section class="stage" class:full={mode === "full"} class:collapsed={collapsed && mode !== "full"}>
    <div class="st-head">
      <span class="live-dot"></span>
      {#if mode === "full"}
        <b class="fu-room">🔊 {roomName || $t("Voice chat")}</b>
      {:else}
        <b>{isChannel ? $t("Live stream") : $t("Voice chat")}</b>
      {/if}
      <span class="st-sub">{$t("{n} in the call", { n: Math.max(tiles.length, ongoing?.users.length ?? 0) })} · {elapsed}</span>
      {#if mode === "full"}
        <button class="fu-chat" class:on={chatOpen} onclick={() => onToggleChat?.()} title={$t("Chat")}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/></svg>
          <span>{$t(chatOpen ? "Hide chat" : "Chat")}</span>
        </button>
      {:else}
        {#if onExpand}<button class="st-x" onclick={() => onExpand?.()} title={$t("Full screen")}>⤢</button>{/if}
        <button class="st-x" onclick={() => (collapsed = !collapsed)} title={collapsed ? $t("Expand") : $t("Collapse")}>{collapsed ? "▾" : "▴"}</button>
      {/if}
    </div>
    {#if !collapsed || mode === "full"}
      <div class="grid" class:focus={!!focus} class:solo={shown.length === 1} style:--n={Math.min(4, Math.max(1, shown.length))}>
        {#each shown as tl (tl.user)}
          <button class="tile" class:speaking={tl.speaking && !tl.muted} class:video={tl.video} onclick={() => (focus = focus === tl.user ? null : tl.user)}>
            {#if tl.video && tl.stream}
              <!-- svelte-ignore a11y_media_has_caption -->
              <video use:srcObject={tl.stream} autoplay playsinline muted class:mirror={tl.me && mine.camera}></video>
            {:else}
              {#if mode === "full" && $avatars[tl.user]}<img class="tile-bg" src={$avatars[tl.user]} alt="" />{/if}
              <span class="ava">{#if $avatars[tl.user]}<img src={$avatars[tl.user]} alt="" />{:else}{tl.user.charAt(0).toUpperCase()}{/if}</span>
            {/if}
            <span class="name">{tl.me ? $t("You") : tl.user}{#if tl.muted}<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><line x1="2" y1="2" x2="22" y2="22"/><path d="M15 9.3V5a3 3 0 0 0-5.7-1.3"/><path d="M9 9v3a3 3 0 0 0 5.1 2.1"/></svg>{/if}</span>
            {#if !tl.me && tl.state !== "connected"}<span class="conn">{tl.state === "connecting" || tl.state === "new" ? $t("connecting…") : tl.state}</span>{/if}
          </button>
        {/each}
        {#if tiles.length === 0}
          <div class="empty">{$t("Waiting for the host…")}</div>
        {/if}
      </div>
    {/if}
    {#if $spatial.on && showMap && (!collapsed || mode === "full")}
      <SpatialMap users={Object.keys(mine.peers)} me={$info?.username ?? "me"} speaking={speakingMap} />
    {/if}
    <!-- remote audio (always mounted, even collapsed) -->
    {#each Object.values(mine.peers) as p (p.user)}
      {#if p.stream}<audio use:srcObject={p.stream} autoplay muted={mine.deafened || $spatial.on}></audio>{/if}
    {/each}
    <div class="ctrls">
      {#if !mine.listener}
        <button class="cb" class:off={mine.muted} onclick={toggleGroupMute} title={mine.muted ? $t("Unmute") : $t("Mute")}>
          {#if mine.muted}<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><line x1="2" y1="2" x2="22" y2="22"/><path d="M18.9 13A7 7 0 0 0 19 12v-1"/><path d="M5 11v1a7 7 0 0 0 12 5"/><path d="M15 9.3V5a3 3 0 0 0-5.7-1.3"/><path d="M9 9v3a3 3 0 0 0 5.1 2.1"/><line x1="12" y1="19" x2="12" y2="22"/></svg>
          {:else}<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><rect x="9" y="2" width="6" height="12" rx="3"/><path d="M5 10v1a7 7 0 0 0 14 0v-1"/><line x1="12" y1="18" x2="12" y2="22"/></svg>{/if}
        </button>
      {/if}
      <button class="cb" class:off={mine.deafened} onclick={toggleGroupDeafen} title={mine.deafened ? $t("Undeafen") : $t("Deafen")}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M3 14h3a2 2 0 0 1 2 2v3a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-7a9 9 0 0 1 18 0v7a2 2 0 0 1-2 2h-1a2 2 0 0 1-2-2v-3a2 2 0 0 1 2-2h3"/>{#if mine.deafened}<line x1="2" y1="2" x2="22" y2="22"/>{/if}</svg>
      </button>
      {#if !mine.listener}
        <button class="cb" class:on={mine.camera} onclick={toggleGroupCamera} title={mine.camera ? $t("Turn camera off") : $t("Turn camera on")}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m22 8-6 4 6 4V8Z"/><rect x="2" y="6" width="14" height="12" rx="2"/></svg>
        </button>
        <button class="cb" class:on={mine.screen} onclick={toggleGroupScreen} title={mine.screen ? $t("Stop streaming") : $t("Share your screen")}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="3" width="20" height="14" rx="2"/><line x1="8" y1="21" x2="16" y2="21"/><line x1="12" y1="17" x2="12" y2="21"/></svg>
        </button>
      {/if}
      <button class="cb" class:on={$spatial.on} onclick={toggleSpatial} title={$spatial.on ? $t("Spatial sound: on") : $t("Spatial sound")}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="2.5"/><path d="M7.5 7.5a6.4 6.4 0 0 0 0 9"/><path d="M16.5 7.5a6.4 6.4 0 0 1 0 9"/><path d="M4.6 4.6a10.5 10.5 0 0 0 0 14.8"/><path d="M19.4 4.6a10.5 10.5 0 0 1 0 14.8"/></svg>
      </button>
      {#if $spatial.on}<button class="cb map-btn" class:on={showMap} onclick={() => (showMap = !showMap)} title={$t("Room map")}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="9"/><circle cx="12" cy="12" r="1.5"/><circle cx="7" cy="8" r="1.5"/><circle cx="17" cy="9" r="1.5"/></svg>
      </button>{/if}
      <button class="cb leave" onclick={leaveGroupCall} title={$t("Leave")}>
        <svg viewBox="0 0 24 24" fill="currentColor"><path d="M12 9c-1.6 0-3.15.25-4.6.72v3.1c0 .39-.23.74-.56.9-.98.49-1.87 1.12-2.66 1.85-.18.18-.43.28-.7.28-.28 0-.53-.11-.71-.29L.29 13.08a.996.996 0 0 1 0-1.41C3.34 8.78 7.46 7 12 7s8.66 1.78 11.71 4.67c.18.18.29.43.29.71 0 .28-.11.53-.29.71l-2.48 2.48c-.18.18-.43.29-.71.29-.27 0-.52-.11-.7-.28a11.27 11.27 0 0 0-2.67-1.85.996.996 0 0 1-.56-.9v-3.1C15.15 9.25 13.6 9 12 9z"/></svg>
      </button>
    </div>
  </section>
{:else if ongoing}
  <div class="join-bar">
    <span class="live-dot"></span>
    <span class="jb-t"><b>{isChannel ? $t("Live stream") : $t("Voice chat")}</b> · {$t("{n} in the call", { n: ongoing.users.length })} · {elapsed}</span>
    <span class="jb-avas">{#each ongoing.users.slice(0, 4) as u (u)}<span class="mini">{#if $avatars[u]}<img src={$avatars[u]} alt="" />{:else}{u.charAt(0).toUpperCase()}{/if}</span>{/each}</span>
    <button class="jb-btn" onclick={() => joinGroupCall(gid)}>{canSpeak ? $t("Join") : $t("Listen")}</button>
  </div>
{/if}

<style>
  .live-dot { width: 8px; height: 8px; border-radius: 50%; background: #3ddc84; box-shadow: 0 0 10px #3ddc84; animation: pulse 1.6s ease-in-out infinite; flex-shrink: 0; }
  @keyframes pulse { 50% { opacity: 0.45; } }
  .stage {
    margin: 8px 12px 0; border-radius: 16px; border: 1px solid var(--border-strong); overflow: hidden;
    background: radial-gradient(600px 220px at 50% 0%, color-mix(in srgb, var(--accent) 12%, transparent), transparent 70%), var(--bg-shell);
    box-shadow: 0 18px 40px -22px rgba(0, 0, 0, 0.8);
  }
  .st-head { display: flex; align-items: center; gap: 8px; padding: 10px 14px; font-size: 13px; color: var(--fg); }
  .st-sub { color: var(--fg-muted); font-size: 12px; font-variant-numeric: tabular-nums; }
  .st-sub { margin-right: auto; }
  .st-x { background: none; border: none; color: var(--fg-dim); cursor: pointer; font-size: 14px; }
  .grid { display: grid; grid-template-columns: repeat(var(--n), minmax(0, 1fr)); gap: 8px; padding: 0 12px 4px; max-height: 340px; overflow-y: auto; }
  .grid.solo { grid-template-columns: minmax(0, 420px); justify-content: center; }
  /* ---- full screen (Discord voice channel) ---- */
  .stage.full { margin: 0; height: 100%; border-radius: 0; border: none; display: flex; flex-direction: column;
    background: radial-gradient(900px 400px at 50% 0%, color-mix(in srgb, var(--accent) 10%, transparent), transparent 70%), #08090f; }
  .stage.full .st-head { padding: 12px 18px; font-size: 14px; }
  .fu-room { font-size: 15px; }
  .fu-chat { display: flex; align-items: center; gap: 6px; border: 1px solid var(--border-strong); background: var(--bg-input); color: var(--fg);
    border-radius: 10px; padding: 7px 12px; font: inherit; font-size: 13px; font-weight: 700; cursor: pointer; transition: background .2s, color .2s, border-color .2s; }
  .fu-chat svg { width: 16px; height: 16px; }
  .fu-chat:hover, .fu-chat.on { background: var(--accent-soft); color: var(--accent); border-color: var(--accent); }
  .stage.full .grid { flex: 1; max-height: none; padding: 8px 24px; align-content: center; gap: 12px;
    grid-template-columns: repeat(auto-fit, minmax(min(100%, 300px), 1fr)); }
  .stage.full .grid.solo { grid-template-columns: minmax(0, min(100%, 760px)); }
  .stage.full .grid.focus { grid-template-columns: 1fr; }
  .stage.full .tile { border-radius: 16px; background: #15171f; animation: tile-in .35s cubic-bezier(.2,.9,.25,1.1) both; }
  @keyframes tile-in { from { opacity: 0; transform: scale(.94); } }
  .stage.full .ava { width: 96px; height: 96px; font-size: 38px; position: relative; box-shadow: 0 10px 30px rgba(0,0,0,.5); }
  .stage.full .tile.speaking .ava { box-shadow: 0 0 0 4px #3ddc84, 0 10px 30px rgba(0,0,0,.5); }
  .tile-bg { position: absolute; inset: -20%; width: 140%; height: 140%; object-fit: cover; filter: blur(30px) brightness(.45) saturate(1.3); }
  .stage.full .name { font-size: 13px; left: 12px; bottom: 10px; }
  .stage.full .ctrls { padding: 14px 0 20px; }
  .stage.full .cb { width: 50px; height: 50px; }
  .grid.focus { grid-template-columns: 1fr; }
  .tile {
    position: relative; aspect-ratio: 16 / 9; border-radius: 12px; background: var(--bg-input); border: 2px solid transparent;
    display: grid; place-items: center; overflow: hidden; cursor: pointer; padding: 0; color: var(--fg); transition: border-color 0.12s, box-shadow 0.12s;
  }
  .tile.speaking { border-color: #3ddc84; box-shadow: 0 0 0 2px rgba(61, 220, 132, 0.25), 0 0 22px -4px rgba(61, 220, 132, 0.6); }
  .tile video { width: 100%; height: 100%; object-fit: cover; background: #000; }
  .tile video.mirror { transform: scaleX(-1); }
  .ava { width: 56px; height: 56px; border-radius: 50%; overflow: hidden; display: grid; place-items: center; font-weight: 800; font-size: 22px; background: linear-gradient(135deg, var(--accent), var(--accent-2)); color: #06070c; }
  .ava img { width: 100%; height: 100%; object-fit: cover; }
  .name { position: absolute; left: 8px; bottom: 6px; display: flex; align-items: center; gap: 4px; font-size: 12px; font-weight: 700; padding: 2px 8px; border-radius: 6px; background: rgba(0, 0, 0, 0.55); color: #fff; }
  .name svg { width: 12px; height: 12px; color: #ff6b6b; }
  .conn { position: absolute; top: 6px; right: 8px; font-size: 10.5px; color: var(--fg-dim); }
  .empty { grid-column: 1 / -1; padding: 24px; text-align: center; color: var(--fg-muted); font-size: 13px; }
  .ctrls { display: flex; justify-content: center; gap: 10px; padding: 10px; }
  .cb, .pb {
    width: 42px; height: 42px; border-radius: 50%; border: 1px solid var(--border); background: var(--bg-input); color: var(--fg);
    display: grid; place-items: center; cursor: pointer; transition: all 0.12s;
  }
  .cb svg, .pb svg { width: 19px; height: 19px; }
  .cb:hover, .pb:hover { color: var(--accent); box-shadow: 0 0 14px -2px var(--accent-glow); }
  .cb.on { background: var(--accent-soft); color: var(--accent); border-color: var(--accent); }
  .cb.off, .pb.off { background: rgba(255, 77, 109, 0.14); color: #ff6b6b; border-color: rgba(255, 77, 109, 0.4); }
  .cb.leave, .pb.leave { background: #ed4245; border-color: #ed4245; color: #fff; }
  .cb.leave:hover, .pb.leave:hover { box-shadow: 0 0 16px -2px rgba(237, 66, 69, 0.8); color: #fff; }
  .join-bar {
    display: flex; align-items: center; gap: 10px; margin: 8px 12px 0; padding: 9px 12px; border-radius: 12px;
    border: 1px solid color-mix(in srgb, #3ddc84 35%, transparent); background: color-mix(in srgb, #3ddc84 8%, var(--bg-shell));
    font-size: 13px; color: var(--fg);
  }
  .jb-t { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--fg-muted); }
  .jb-t b { color: var(--fg); }
  .jb-avas { display: flex; }
  .mini { width: 24px; height: 24px; border-radius: 50%; margin-left: -6px; border: 2px solid var(--bg-shell); overflow: hidden; display: grid; place-items: center; font-size: 10px; font-weight: 800; background: linear-gradient(135deg, var(--accent), var(--accent-2)); color: #06070c; }
  .mini img { width: 100%; height: 100%; object-fit: cover; }
  .jb-btn { border: none; border-radius: 9px; padding: 7px 16px; font: inherit; font-weight: 800; font-size: 13px; cursor: pointer; background: #3ddc84; color: #06070c; }
  .pill {
    display: flex; align-items: center; gap: 8px; padding: 8px 10px; border-radius: 12px; margin: 0 8px 8px;
    background: var(--bg-shell); border: 1px solid color-mix(in srgb, #3ddc84 40%, transparent);
  }
  .pill-main { flex: 1; min-width: 0; display: flex; flex-direction: column; align-items: flex-start; background: none; border: none; color: var(--fg); cursor: pointer; font: inherit; text-align: left; }
  .pill-main b { font-size: 12.5px; color: #3ddc84; }
  .pill-main span { font-size: 11.5px; color: var(--fg-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; max-width: 100%; }
  .pb { width: 32px; height: 32px; }
  .pb svg { width: 16px; height: 16px; }
</style>
