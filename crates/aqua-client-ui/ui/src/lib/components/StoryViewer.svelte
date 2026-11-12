<script lang="ts">
  /**
   * Stories, the way Telegram shows them.
   *
   * Phone: full screen; tap right / left — next / previous, hold — pause
   * (the bars and buttons step aside), swipe sideways — the next person
   * comes in on a turning cube, swipe down — close, swipe up — reply (or,
   * on my own story, who watched it). A reply field, ❤ and the menu sit
   * under the picture.
   * Computer: the story in the middle at 9:16, the people before / after it
   * at the sides (click to jump), arrows and the keyboard (← → Space Esc).
   * Files are fetched and decrypted on first view, the next ones ahead.
   */
  import { t, tr } from "../i18n";
  import { api, assetUrl } from "../tauri";
  import { onBack } from "../back";
  import {
    storyViewer, myStories, saneStories, markSeen, deleteStory, isLive, sendStoryView, toggleLike, toggleHidden,
    likedStories, seenStories, storyViews, hiddenStories, closeFriends, type Story,
  } from "../stories";
  import { peerPresence } from "../presence";
  import { avatars, peerCards } from "../people";
  import { profile } from "../profile";
  import { chats, selectChat } from "../stores";
  import { streamSrc } from "../media";
  import { askConfirm } from "./Confirm.svelte";
  import { showToast } from "./Toast.svelte";
  import { IS_MOBILE } from "../platform";

  const PHOTO_MS = 6000;
  const QUICK = ["❤️", "🔥", "😂", "😮", "😢", "👍"];

  let v = $derived($storyViewer);
  const storiesOf = (u: string): Story[] => (u === "" ? $myStories.filter((s) => isLive(s)) : saneStories($peerPresence[u]?.st));
  let list = $derived<Story[]>(v ? storiesOf(v.user) : []);
  let cur = $derived(v ? list[v.index] ?? null : null);
  let url = $state("");
  let loading = $state(false);
  let err = $state("");
  let progress = $state(0);
  let held = $state(false);
  let muted = $state(false);
  let videoEl = $state<HTMLVideoElement | null>(null);
  let reply = $state("");
  let replyFocus = $state(false);
  let replyEl = $state<HTMLInputElement | null>(null);
  let menu = $state(false);
  let viewersOpen = $state(false);
  let hidden = $state(typeof document !== "undefined" && document.visibilityState === "hidden");
  const paused = $derived(held || replyFocus || menu || viewersOpen || hidden);

  /** Decrypted files, by story id (shared with the side previews). */
  const cache = new Map<string, string>();
  let cacheTick = $state(0);
  async function load(s: Story): Promise<string> {
    const hit = cache.get(s.id);
    if (hit) return hit;
    const u = assetUrl(await api.fetchAttachment(s.media));
    cache.set(s.id, u);
    cacheTick++;
    return u;
  }
  const cached = (s: Story | undefined) => { void cacheTick; return s ? cache.get(s.id) ?? "" : ""; };

  // Back: the sheet / menu on top first, then the viewer — each its own entry.
  const isOpen = $derived(!!v);
  $effect(() => { if (isOpen) return onBack(close); });
  $effect(() => { if (viewersOpen) return onBack(() => (viewersOpen = false)); });
  $effect(() => { if (menu) return onBack(() => (menu = false)); });
  $effect(() => {
    const on = () => (hidden = document.visibilityState === "hidden");
    document.addEventListener("visibilitychange", on);
    return () => document.removeEventListener("visibilitychange", on);
  });

  // Load the current story's file, and the next ones ahead.
  let loadedFor = "";
  $effect(() => {
    const s = cur;
    if (!s) return;
    if (loadedFor === s.id) return;
    loadedFor = s.id;
    const owner = v?.user ?? "";
    err = ""; progress = 0;
    const hit = cache.get(s.id);
    url = hit ?? "";
    loading = !hit;
    void load(s).then((u) => {
      if (cur?.id !== s.id) return;
      url = u;
      loading = false;
      if (owner) {
        const first = !$seenStories.includes(s.id);
        markSeen(s.id);
        if (first) sendStoryView(owner, s.id);
      }
      prefetch();
    }).catch((e) => { if (cur?.id === s.id) { err = String(e); loading = false; } });
  });
  function prefetch() {
    if (!v) return;
    const nextHere = list[v.index + 1];
    if (nextHere) void load(nextHere).catch(() => {});
    for (const u of [neighbour(1), neighbour(-1)]) {
      if (u == null) continue;
      const first = storiesOf(u)[startIndex(u)];
      if (first) void load(first).catch(() => {});
    }
  }
  // No story left (expired / deleted): next person or close.
  $effect(() => { if (v && !cur) { if (list.length && v.index >= list.length) nextUser(); else if (!list.length) nextUser(); } });
  $effect(() => { if (!v) { viewersOpen = false; menu = false; reply = ""; replyFocus = false; held = false; } });

  // Photo timer (videos follow their own time).
  $effect(() => {
    if (!cur || !url || loading || cur.media.kind === "video") return;
    let last = performance.now();
    let raf = 0;
    const step = (now: number) => {
      if (!paused && !turning) progress = Math.min(1, progress + (now - last) / PHOTO_MS);
      last = now;
      if (progress >= 1) { next(); return; }
      raf = requestAnimationFrame(step);
    };
    raf = requestAnimationFrame(step);
    return () => cancelAnimationFrame(raf);
  });
  $effect(() => { const el = videoEl; if (!el) return; if (paused || turning) el.pause(); else void el.play().catch(() => {}); });

  /** Where to start in someone's stories: the first one not yet seen. */
  function startIndex(u: string): number {
    if (u === "") return 0;
    const l = storiesOf(u);
    const i = l.findIndex((s) => !$seenStories.includes(s.id));
    return i < 0 ? 0 : i;
  }
  function neighbour(dir: 1 | -1): string | null {
    if (!v) return null;
    const i = v.users.indexOf(v.user);
    const u = v.users[i + dir];
    return u === undefined ? null : u;
  }
  function close() { storyViewer.set(null); loadedFor = ""; }
  function goUser(u: string) {
    if (!v) return;
    storyViewer.set({ ...v, user: u, index: startIndex(u) });
  }
  function nextUser() {
    const u = neighbour(1);
    if (u == null) { close(); return; }
    goUser(u);
  }
  function prevUser() {
    const u = neighbour(-1);
    if (u == null) { if (v) storyViewer.set({ ...v, index: 0 }); progress = 0; return; }
    goUser(u);
  }
  function next() {
    if (!v) return;
    if (v.index + 1 < list.length) storyViewer.set({ ...v, index: v.index + 1 });
    else nextUser();
  }
  function prev() {
    if (!v) return;
    if (v.index > 0) storyViewer.set({ ...v, index: v.index - 1 });
    else prevUser();
  }

  // ---- Gestures: tap / hold / swipe sideways (cube) / down / up ----
  let down: { x: number; y: number; t: number; w: number } | null = null;
  let axis: "" | "x" | "y" = "";
  let holdT: ReturnType<typeof setTimeout> | null = null;
  let dy = $state(0);
  let angle = $state(0);
  let turning = $state(false);
  let cubeW = $state(400);
  function pdown(e: PointerEvent) {
    if ((e.target as HTMLElement).closest("button, input, .no-tap")) return;
    if (turning) return;
    const w = (e.currentTarget as HTMLElement).clientWidth;
    down = { x: e.clientX, y: e.clientY, t: performance.now(), w };
    cubeW = w;
    axis = "";
    (e.currentTarget as HTMLElement).setPointerCapture?.(e.pointerId);
    holdT = setTimeout(() => (held = true), 220);
  }
  function pmove(e: PointerEvent) {
    if (!down) return;
    const dx = e.clientX - down.x, ddy = e.clientY - down.y;
    if (!axis && Math.hypot(dx, ddy) > 10) {
      axis = Math.abs(dx) > Math.abs(ddy) ? "x" : "y";
      if (holdT) { clearTimeout(holdT); holdT = null; }
    }
    if (axis === "x") {
      // Nobody that way: a little give, no more.
      const has = neighbour(dx < 0 ? 1 : -1) != null;
      angle = Math.max(-90, Math.min(90, (-dx / down.w) * 90 * (has ? 1 : 0.15)));
    } else if (axis === "y") {
      dy = ddy;
    }
  }
  function pup(e: PointerEvent) {
    if (holdT) { clearTimeout(holdT); holdT = null; }
    const d = down;
    down = null;
    const wasHeld = held;
    held = false;
    if (!d) return;
    const fast = performance.now() - d.t < 260;
    if (axis === "x") {
      const dir = angle > 0 ? 1 : -1;
      if ((Math.abs(angle) > 22 || (fast && Math.abs(angle) > 8)) && neighbour(dir) != null) turn(dir);
      else settle();
      return;
    }
    if (axis === "y") {
      const yy = dy;
      dy = 0;
      if (yy > 110 || (fast && yy > 50)) { close(); return; }
      if (yy < -70) { if (v?.user === "") viewersOpen = true; else replyEl?.focus(); }
      return;
    }
    if (wasHeld && performance.now() - d.t > 220) return;
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    if (e.clientX - rect.left < d.w * 0.3) prev(); else next();
  }
  /** Finish the cube turn to the next / previous person. */
  function turn(dir: 1 | -1) {
    turning = true;
    angle = dir * 90;
    setTimeout(() => {
      const u = neighbour(dir);
      if (u != null) goUser(u);
      turning = false;
      angle = 0;
    }, 300);
  }
  function settle() { turning = true; angle = 0; setTimeout(() => (turning = false), 300); }
  /** Arrows / keys between people: the same cube, without a finger. */
  function stepUser(dir: 1 | -1) {
    if (turning) return;
    if (neighbour(dir) == null) { if (dir === 1) close(); return; }
    cubeW = cubeEl?.clientWidth ?? cubeW;
    turn(dir);
  }
  let cubeEl = $state<HTMLDivElement | null>(null);

  function key(e: KeyboardEvent) {
    if (!v || replyFocus) return;
    if (e.key === "ArrowRight") { e.preventDefault(); if (e.shiftKey) stepUser(1); else next(); }
    else if (e.key === "ArrowLeft") { e.preventDefault(); if (e.shiftKey) stepUser(-1); else prev(); }
    else if (e.key === "ArrowDown") { e.preventDefault(); stepUser(1); }
    else if (e.key === "ArrowUp") { e.preventDefault(); stepUser(-1); }
    else if (e.key === " ") { e.preventDefault(); held = !held; }
  }

  // ---- Reply, ❤, menu ----
  async function sendReply(text: string) {
    const s = cur, owner = v?.user;
    if (!s || !owner || !text.trim()) return;
    const quoted = `${s.media.kind === "video" ? "🎬" : "📷"} ${tr("Story")}${s.caption ? ": " + s.caption.slice(0, 120) : ""}`;
    try {
      await api.sendMessage(owner, "[reply:v1]" + JSON.stringify({ quoted_author: nameOf(owner), quoted_text: quoted, text: text.trim() }));
      showToast(tr("Message sent"), 1800);
    } catch (e) { showToast(`${tr("Could not send")}: ${e}`, 4000); }
  }
  function submitReply() {
    const text = reply;
    reply = "";
    replyEl?.blur();
    void sendReply(text);
  }
  function quick(emo: string) {
    const s = cur, owner = v?.user;
    if (!s || !owner) return;
    if (!$likedStories.includes(s.id)) toggleLike(owner, s.id);
    replyEl?.blur();
    void sendReply(emo);
  }
  async function remove() {
    const s = cur;
    if (!s) return;
    menu = false;
    held = true;
    const ok = await askConfirm({ title: tr("Delete this story?"), ok: tr("Delete"), danger: true });
    held = false;
    if (!ok) return;
    await deleteStory(s.id);
  }

  const nameOf = (u: string) => (u === "" ? $profile.display_name || tr("You") : $chats.find((c) => c.id === u)?.name ?? $peerCards[u]?.display_name ?? u);
  const avaOf = (u: string) => (u === "" ? $profile.avatar_url ?? "" : $avatars[u] ?? "");
  const letterOf = (u: string) => nameOf(u).charAt(0).toUpperCase();
  function ago(at: number): string {
    const m = Math.max(0, Math.round((Date.now() - at) / 60000));
    return m < 1 ? tr("just now") : m < 60 ? tr("{n} min ago", { n: m }) : tr("{n} h ago", { n: Math.floor(m / 60) });
  }
  const audLabel = (s: Story) => s.aud === "contacts" ? tr("My contacts") : s.aud === "close" ? tr("Close friends") : s.aud === "selected" ? tr("{n} people", { n: s.sel?.length ?? 0 }) : tr("Everyone");
  const views = $derived(cur && v?.user === "" ? ($storyViews[cur.id] ?? []).slice().reverse() : []);
  const sideL = $derived(v ? neighbour(-1) : null);
  const sideR = $derived(v ? neighbour(1) : null);
  /** The face turning in on the cube (its first story, if loaded). */
  const turnTo = $derived(angle > 0 ? sideR : angle < 0 ? sideL : null);
  const turnStory = $derived(turnTo != null ? storiesOf(turnTo)[startIndex(turnTo)] : undefined);
</script>

<svelte:window onkeydown={key} />

{#snippet face(u: string, s: Story | undefined, live: boolean)}
  <div class="media">
    {#if live && url && cur?.media.kind === "video"}
      <!-- svelte-ignore a11y_media_has_caption -->
      <video bind:this={videoEl} use:streamSrc={url} autoplay playsinline muted={muted}
        ontimeupdate={(e) => { const el = e.currentTarget as HTMLVideoElement; if (el.duration) progress = el.currentTime / el.duration; }}
        onended={next}></video>
    {:else if (live ? url : cached(s)) && s?.media.kind !== "video"}
      <!-- Whole picture, never cropped or stretched; its own blur fills the edges (Telegram). -->
      <img class="bg" src={live ? url : cached(s)} alt="" draggable="false" aria-hidden="true" />
      <img class="fg" src={live ? url : cached(s)} alt="" draggable="false" />
    {:else if live && err}
      <div class="msg">{$t("Could not load the story")}<small>{err}</small></div>
    {:else if live}
      <div class="spin"></div>
    {:else}
      <div class="ph" style:background-image={avaOf(u) ? `url("${avaOf(u)}")` : undefined}></div>
    {/if}
    {#if live && cur?.caption}<div class="cap">{cur.caption}</div>{/if}
  </div>
  <div class="top" class:gone={live && held}>
    <div class="bars">
      {#each live ? list : storiesOf(u) as st, i (st.id)}
        <span class="bar"><i style:width="{live && v ? (i < v.index ? 100 : i === v.index ? progress * 100 : 0) : 0}%"></i></span>
      {/each}
    </div>
    <div class="who">
      <button class="whob no-tap" onclick={() => { if (u) { close(); void selectChat(u); } }}>
        <span class="ava" style:background-image={avaOf(u) ? `url("${avaOf(u)}")` : undefined}>{avaOf(u) ? "" : letterOf(u)}</span>
        <span class="nm"><b>{nameOf(u)}</b><small>{s ? ago(s.at) : ""}{#if u === "" && s} · {audLabel(s)}{/if}</small></span>
      </button>
      {#if live}
        {#if cur?.media.kind === "video"}
          <button class="ic" onclick={() => (muted = !muted)} aria-label={$t(muted ? "Unmute" : "Mute")} title={$t(muted ? "Unmute" : "Mute")}>
            {#if muted}<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M11 5 6 9H3v6h3l5 4z"/><path d="m22 9-6 6M16 9l6 6"/></svg>
            {:else}<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M11 5 6 9H3v6h3l5 4z"/><path d="M15.5 8.5a5 5 0 0 1 0 7M19 5a10 10 0 0 1 0 14"/></svg>{/if}
          </button>
        {/if}
        <button class="ic" onclick={() => (menu = !menu)} aria-label={$t("More")} title={$t("More")}>
          <svg viewBox="0 0 24 24" fill="currentColor"><circle cx="12" cy="5" r="1.8"/><circle cx="12" cy="12" r="1.8"/><circle cx="12" cy="19" r="1.8"/></svg>
        </button>
        {#if IS_MOBILE}
          <button class="ic" data-keep-x onclick={close} aria-label={$t("Close")}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M6 6l12 12M18 6 6 18"/></svg>
          </button>
        {/if}
      {/if}
    </div>
  </div>
{/snippet}

{#if v && cur}
  <div class="sv" class:desk={!IS_MOBILE} data-own-back data-phone-full role="dialog" aria-modal="true" aria-label={$t("Stories")}
    style:--dy="{Math.max(0, dy)}px" style:--up="{Math.min(0, dy)}px">
    {#if !IS_MOBILE}
      <!-- The story's own blur behind everything (Telegram Desktop). -->
      {#if url && cur.media.kind !== "video"}<img class="sv-blur" src={url} alt="" aria-hidden="true" />{/if}
      <button class="ic close-x" data-x data-keep-x onclick={close} aria-label={$t("Close")}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M6 6l12 12M18 6 6 18"/></svg>
      </button>
    {/if}

    <div class="stage-row">
      {#if !IS_MOBILE}
        <div class="side left">
          {#if sideL != null}
            {@const st = storiesOf(sideL)[startIndex(sideL)]}
            <button class="peek" onclick={() => stepUser(-1)} aria-label={nameOf(sideL)}>
              {#if cached(st)}<img src={cached(st)} alt="" />{/if}
              <span class="peek-who"><span class="ava ring" style:background-image={avaOf(sideL) ? `url("${avaOf(sideL)}")` : undefined}>{avaOf(sideL) ? "" : letterOf(sideL)}</span><b>{nameOf(sideL)}</b></span>
            </button>
          {/if}
        </div>
        <button class="arrow" disabled={!v.index && sideL == null} onclick={prev} aria-label={$t("Previous")}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 18 9 12 15 6"/></svg>
        </button>
      {/if}

      <div class="col">
        <div class="cube-wrap" bind:this={cubeEl}>
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div class="cube" class:anim={turning} style:--w="{cubeW}px" style:--a="{angle}deg"
            onpointerdown={pdown} onpointermove={pmove} onpointerup={pup} onpointercancel={pup} oncontextmenu={(e) => e.preventDefault()}>
            <div class="face front">{@render face(v.user, cur, true)}</div>
            {#if turnTo != null}
              <div class="face" class:right={angle > 0} class:left={angle < 0}>{@render face(turnTo, turnStory, false)}</div>
            {/if}
          </div>
          {#if menu}
            <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
            <div class="menu-scrim" onclick={() => (menu = false)}></div>
            <div class="menu" role="menu">
              {#if v.user === ""}
                <button onclick={() => { menu = false; viewersOpen = true; }}>{$t("Viewers")}</button>
                <button class="danger" onclick={remove}>{$t("Delete story")}</button>
              {:else}
                {@const u = v.user}
                <button onclick={() => { menu = false; close(); void selectChat(u); }}>{$t("Send message")}</button>
                <button onclick={() => { menu = false; const was = $hiddenStories.includes(u); toggleHidden(u); showToast(tr(was ? "Stories are shown again" : "Stories hidden — they're now at the end of the strip")); }}>{$t($hiddenStories.includes(u) ? "Show stories" : "Hide stories")}</button>
              {/if}
            </div>
          {/if}
        </div>

        <div class="bottom" class:gone={held}>
          {#if v.user === ""}
            <button class="views" onclick={() => (viewersOpen = true)}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M2 12s3.6-7 10-7 10 7 10 7-3.6 7-10 7S2 12 2 12z"/><circle cx="12" cy="12" r="3"/></svg>
              {views.length ? $t("{n} views", { n: views.length }) : $t("No views yet")}
              {#if views.some((x) => x.r)}<span class="vr">{views.filter((x) => x.r).length} ❤️</span>{/if}
            </button>
            <span class="grow"></span>
            <button class="ic" onclick={remove} title={$t("Delete")} aria-label={$t("Delete")}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13"/></svg>
            </button>
          {:else}
            {#if replyFocus && !reply}
              <div class="quick">
                {#each QUICK as q (q)}
                  <button onpointerdown={(e) => e.preventDefault()} onclick={() => quick(q)}>{q}</button>
                {/each}
              </div>
            {/if}
            <input class="reply" bind:this={replyEl} bind:value={reply} placeholder={$t("Reply privately…")} maxlength="2000"
              onfocus={() => (replyFocus = true)} onblur={() => (replyFocus = false)}
              onkeydown={(e) => { if (e.key === "Enter") { e.preventDefault(); submitReply(); } else if (e.key === "Escape") { e.stopPropagation(); replyEl?.blur(); } }} />
            {#if reply.trim()}
              <button class="ic send" onpointerdown={(e) => e.preventDefault()} onclick={submitReply} aria-label={$t("Send")}>
                <svg viewBox="0 0 24 24" fill="currentColor"><path d="M3.4 20.4 21 12 3.4 3.6l-.01 6.53L15 12 3.39 13.87z"/></svg>
              </button>
            {:else}
              <button class="ic heart" class:on={$likedStories.includes(cur.id)} onclick={() => { const u = v?.user; if (u) toggleLike(u, cur!.id); }} aria-label={$t("Like")}>
                <svg viewBox="0 0 24 24" fill={$likedStories.includes(cur.id) ? "currentColor" : "none"} stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M12 20.5s-7.5-4.6-9.3-9.2C1.4 8 3.3 4.5 6.9 4.5c2 0 3.6 1.1 5.1 3 1.5-1.9 3.1-3 5.1-3 3.6 0 5.5 3.5 4.2 6.8-1.8 4.6-9.3 9.2-9.3 9.2z"/></svg>
              </button>
            {/if}
          {/if}
        </div>
      </div>

      {#if !IS_MOBILE}
        <button class="arrow" onclick={next} aria-label={$t("Next")}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><polyline points="9 18 15 12 9 6"/></svg>
        </button>
        <div class="side right">
          {#if sideR != null}
            {@const st = storiesOf(sideR)[startIndex(sideR)]}
            <button class="peek" onclick={() => stepUser(1)} aria-label={nameOf(sideR)}>
              {#if cached(st)}<img src={cached(st)} alt="" />{/if}
              <span class="peek-who"><span class="ava ring" style:background-image={avaOf(sideR) ? `url("${avaOf(sideR)}")` : undefined}>{avaOf(sideR) ? "" : letterOf(sideR)}</span><b>{nameOf(sideR)}</b></span>
            </button>
          {/if}
        </div>
      {/if}
    </div>

    {#if viewersOpen}
      <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
      <div class="sheet-scrim" onclick={() => (viewersOpen = false)}></div>
      <div class="sheet" role="dialog" aria-label={$t("Viewers")}>
        <div class="grab"></div>
        <div class="sh-head"><b>{views.length ? $t("{n} views", { n: views.length }) : $t("No views yet")}</b><small>{audLabel(cur)}</small></div>
        <div class="sh-list">
          {#if !views.length}<div class="sh-empty">{$t("People who watch your story will show up here.")}</div>{/if}
          {#each views as w (w.u)}
            <button class="vrow" onclick={() => { viewersOpen = false; close(); void selectChat(w.u); }}>
              <span class="ava" class:cf={$closeFriends.includes(w.u)} style:background-image={avaOf(w.u) ? `url("${avaOf(w.u)}")` : undefined}>{avaOf(w.u) ? "" : letterOf(w.u)}</span>
              <span class="nm"><b>{nameOf(w.u)}</b><small>{ago(w.at)}</small></span>
              {#if w.r}<span class="vr-emo">{w.r}</span>{/if}
            </button>
          {/each}
        </div>
      </div>
    {/if}
  </div>
{/if}

<style>
  .sv { position: fixed; inset: 0; z-index: 10094; background: #000; color: #fff; user-select: none; -webkit-user-select: none; overflow: hidden;
    display: flex; flex-direction: column; padding: var(--sat, env(safe-area-inset-top, 0px)) 0 var(--sab, env(safe-area-inset-bottom, 0px)); box-sizing: border-box;
    animation: sv-in .24s cubic-bezier(.2,.9,.25,1); }
  @keyframes sv-in { from { transform: scale(.9); opacity: 0; } }
  .stage-row { flex: 1; min-height: 0; display: flex; align-items: center; justify-content: center;
    transform: translateY(var(--dy)) scale(calc(1 - var(--dy) / 1600px)); opacity: calc(1 - var(--dy) / 700px); }
  .col { position: relative; flex: 1; height: 100%; min-width: 0; display: flex; flex-direction: column; }
  .cube-wrap { position: relative; flex: 1; min-height: 0; perspective: 1400px; }
  .cube { position: absolute; inset: 0; transform-style: preserve-3d; touch-action: none;
    transform: translateZ(calc(var(--w) / -2)) rotateY(calc(var(--a) * -1)) translateZ(calc(var(--w) / 2)); }
  .cube.anim { transition: transform .3s cubic-bezier(.3,.7,.3,1); }
  .face { position: absolute; inset: 0; backface-visibility: hidden; overflow: hidden; background: #000; border-radius: calc(12px * var(--rk, 1)); }
  .face.right { transform: translateZ(calc(var(--w) / -2)) rotateY(90deg) translateZ(calc(var(--w) / 2)); }
  .face.left { transform: translateZ(calc(var(--w) / -2)) rotateY(-90deg) translateZ(calc(var(--w) / 2)); }
  .face:not(.front) .media::after { content: ""; position: absolute; inset: 0; background: rgba(0,0,0,.25); }
  .media { position: absolute; inset: 0; display: grid; place-items: center; }
  .media img, .media video { grid-area: 1 / 1; width: 100%; height: 100%; object-fit: contain; pointer-events: none; }
  .media img.bg { object-fit: cover; filter: blur(28px) brightness(.55); transform: scale(1.15); }
  /* Above its own blur (a transformed layer paints over plain ones). */
  .media img.fg, .media video { position: relative; z-index: 1; }
  .cap { z-index: 2; }
  .ph { position: absolute; inset: 0; background: #111 center / cover; filter: blur(30px) brightness(.5); transform: scale(1.2); }
  .msg { display: flex; flex-direction: column; gap: 6px; align-items: center; font-size: 15px; text-align: center; padding: 20px; }
  .msg small { font-size: 12px; opacity: .6; max-width: 80vw; overflow-wrap: anywhere; }
  .spin { width: 36px; height: 36px; border-radius: 50%; border: 3px solid rgba(255,255,255,.25); border-top-color: #fff; animation: spin .8s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  .cap { position: absolute; left: 0; right: 0; bottom: 0; padding: 40px 18px 16px; font-size: 15px; line-height: 1.4; text-align: center;
    background: linear-gradient(transparent, rgba(0,0,0,.6)); white-space: pre-wrap; overflow-wrap: anywhere; }
  .top { position: absolute; z-index: 3; left: 0; right: 0; top: 0; padding: 8px 8px 22px; background: linear-gradient(rgba(0,0,0,.5), transparent); transition: opacity .18s; }
  .top.gone, .bottom.gone { opacity: 0; pointer-events: none; }
  .bars { display: flex; gap: 3px; }
  .bar { flex: 1; height: 2.5px; border-radius: 2px; background: rgba(255,255,255,.35); overflow: hidden; }
  .bar i { display: block; height: 100%; background: #fff; }
  .who { display: flex; align-items: center; gap: 4px; margin-top: 8px; }
  .whob { flex: 1; min-width: 0; display: flex; align-items: center; gap: 10px; border: none; background: transparent; color: inherit; padding: 0; text-align: left; cursor: pointer; }
  .ava { width: 36px; height: 36px; border-radius: var(--ava-r, 50%); background: rgba(255,255,255,.2) center / cover; display: grid; place-items: center; font-weight: 800; flex-shrink: 0; color: #fff; }
  .nm { display: flex; flex-direction: column; flex: 1; min-width: 0; }
  .nm b { font-size: 14.5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .nm small { font-size: 12px; opacity: .75; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .ic { width: 40px; height: 40px; flex-shrink: 0; border: none; border-radius: 50%; background: transparent; color: #fff; display: grid; place-items: center; cursor: pointer; }
  .ic:hover { background: rgba(255,255,255,.12); }
  .ic svg { width: 22px; height: 22px; }
  .bottom { flex-shrink: 0; position: relative; display: flex; align-items: center; gap: 8px; padding: 8px 10px; min-height: 60px; box-sizing: border-box; transition: opacity .18s; }
  .reply { flex: 1; min-width: 0; height: 42px; padding: 0 16px; border-radius: 21px; border: 1px solid rgba(255,255,255,.28); background: rgba(255,255,255,.06); color: #fff;
    font: 15px var(--font-ui, inherit); outline: none; user-select: text; -webkit-user-select: text; }
  .reply::placeholder { color: rgba(255,255,255,.6); }
  .reply:focus { border-color: rgba(255,255,255,.55); }
  .heart.on { color: #FF3B5C; }
  .heart svg { transition: transform .2s cubic-bezier(.3,1.6,.5,1); }
  .heart.on svg { transform: scale(1.12); }
  .send { background: var(--accent); color: #06070c; }
  .send:hover { background: var(--accent); filter: brightness(1.08); }
  .quick { position: absolute; left: 10px; right: 10px; bottom: 100%; display: flex; justify-content: space-around; padding: 8px; border-radius: 22px; background: rgba(30,30,34,.92);
    animation: q-in .18s ease-out; }
  @keyframes q-in { from { transform: translateY(8px); opacity: 0; } }
  .quick button { border: none; background: transparent; font-size: 28px; cursor: pointer; transition: transform .15s; }
  .quick button:hover { transform: scale(1.2); }
  .views { display: flex; align-items: center; gap: 8px; border: none; background: transparent; color: #fff; font: 600 14px var(--font-ui, inherit); cursor: pointer; padding: 8px 6px; }
  .views svg { width: 20px; height: 20px; }
  .vr { opacity: .8; font-weight: 500; }
  .grow { flex: 1; }
  .menu-scrim { position: absolute; inset: 0; z-index: 4; }
  .menu { position: absolute; top: 62px; right: 10px; z-index: 5; min-width: 200px; padding: 5px; border-radius: calc(12px * var(--rk, 1)); background: rgba(34,34,38,.97);
    box-shadow: 0 14px 30px -12px rgba(0,0,0,.9); display: flex; flex-direction: column; animation: q-in .14s ease-out; }
  .menu button { padding: 11px 14px; border: none; background: transparent; color: #fff; text-align: left; font: 500 14.5px var(--font-ui, inherit); border-radius: calc(8px * var(--rk, 1)); cursor: pointer; }
  .menu button:hover { background: rgba(255,255,255,.08); }
  .menu .danger { color: #FF5A6E; }
  .sheet-scrim { position: absolute; inset: 0; z-index: 8; background: rgba(0,0,0,.45); }
  .sheet { position: absolute; left: 0; right: 0; bottom: 0; z-index: 9; max-height: 70%; display: flex; flex-direction: column; background: var(--bg-shell); color: var(--fg);
    border-radius: calc(18px * var(--rk, 1)) calc(18px * var(--rk, 1)) 0 0; padding-bottom: var(--sab, env(safe-area-inset-bottom, 0px)); animation: sh-in .24s cubic-bezier(.2,.9,.25,1); }
  @keyframes sh-in { from { transform: translateY(100%); } }
  .grab { width: 36px; height: 4px; border-radius: 2px; background: var(--border-strong); margin: 8px auto 2px; }
  .sh-head { display: flex; flex-direction: column; padding: 6px 16px 10px; border-bottom: 1px solid var(--border); }
  .sh-head b { font-size: 16px; }
  .sh-head small { font-size: 12.5px; color: var(--fg-muted); }
  .sh-list { overflow-y: auto; padding: 6px; }
  .sh-empty { padding: 28px 16px; text-align: center; color: var(--fg-muted); font-size: 14px; }
  .vrow { width: 100%; display: flex; align-items: center; gap: 12px; padding: 8px 10px; border: none; background: transparent; color: var(--fg); border-radius: calc(10px * var(--rk, 1)); cursor: pointer; text-align: left; }
  .vrow:hover { background: var(--bg-hover); }
  .vrow .ava { width: 42px; height: 42px; background-color: var(--bg-surface-2); color: var(--fg); }
  .vrow .ava.cf { box-shadow: 0 0 0 2px var(--bg-shell), 0 0 0 4px #34C759; }
  .vr-emo { font-size: 22px; }

  /* Computer (Telegram Desktop): the story in the middle at 9:16, the
     people before / after it at the sides, the blur of it behind. */
  .sv.desk { background: rgba(8,8,10,.94); padding: 0; }
  .sv-blur { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: cover; filter: blur(60px) brightness(.35); transform: scale(1.2); pointer-events: none; }
  .close-x { position: absolute; top: 14px; right: 14px; z-index: 6; background: rgba(255,255,255,.08); }
  .desk .stage-row { gap: 12px; padding: 20px 0; }
  .desk .col { flex: 0 0 auto; height: min(100%, 900px); aspect-ratio: 9 / 17.2; }
  .desk .bottom { padding: 10px 0 0; }
  .desk .menu-scrim { position: fixed; }
  .side { flex: 1; height: 100%; display: flex; align-items: center; min-width: 0; }
  .side.left { justify-content: flex-end; }
  .peek { position: relative; height: min(46%, 420px); aspect-ratio: 9 / 16; border: none; padding: 0; border-radius: calc(12px * var(--rk, 1)); overflow: hidden; cursor: pointer;
    background: #1a1a1e; opacity: .55; transition: opacity .2s, transform .2s; }
  .peek:hover { opacity: .85; transform: scale(1.02); }
  .peek img { width: 100%; height: 100%; object-fit: cover; }
  .peek-who { position: absolute; inset: 0; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 8px; background: rgba(0,0,0,.35); color: #fff; }
  .peek-who b { font-size: 13.5px; max-width: 90%; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .peek-who .ava { width: 52px; height: 52px; }
  .peek-who .ava.ring { box-shadow: 0 0 0 2px #000, 0 0 0 4px var(--accent); }
  .arrow { flex-shrink: 0; width: 44px; height: 44px; border: none; border-radius: 50%; background: rgba(255,255,255,.1); color: #fff; display: grid; place-items: center; cursor: pointer; }
  .arrow:hover:not(:disabled) { background: rgba(255,255,255,.2); }
  .arrow:disabled { opacity: .25; cursor: default; }
  .arrow svg { width: 22px; height: 22px; }
</style>
