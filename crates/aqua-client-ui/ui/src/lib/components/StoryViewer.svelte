<script lang="ts">
  /**
   * Stories, the way Telegram shows them.
   *
   * Phone: full screen; tap right / left — next / previous, hold — pause
   * (the bars and buttons step aside), swipe sideways — the next person
   * slides into view, swipe down — close, swipe up — reply (or,
   * on my own story, who watched it). A reply field, ❤ and the menu sit
   * under the picture.
   * Computer: the story in the middle at 9:16, the people before / after it
   * at the sides (click to jump), arrows and the keyboard (← → Space Esc).
   * Files are fetched and decrypted on first view, the next ones ahead.
   */
  import { untrack } from "svelte";
  import { motionMs } from "../motion";
  import { t, tr } from "../i18n";
  import { api, assetUrl } from "../tauri";
  import { onBack } from "../back";
  import {
    storyViewer, myStories, saneStories, markSeen, deleteStory, isLive, sendStoryView, toggleLike, toggleHidden,
    likedStories, seenStories, storyViews, hiddenStories, closeFriends, storyNav, storyArchive, pinnedStories,
    pinStory, unpinStory, storyFile, allMyStories, storyArchiveOpen, type Story,
  } from "../stories";
  import { peerPresence } from "../presence";
  import { avatars, peerCards } from "../people";
  import { profile } from "../profile";
  import { chats, selectChat } from "../stores";
  import { streamSrc } from "../media";
  import { askConfirm } from "./Confirm.svelte";
  import { showToast } from "./Toast.svelte";
  import { IS_MOBILE } from "../platform";
  import { reactionPack, quickReactions, reactionUrl, parseReaction, REACTION_PACK, DEFAULT_REACTIONS } from "../reactionPack";

  const PHOTO_MS = 6000;
  /** The quick reactions — our own pixel emoji (the reaction pack), as in
   *  the chat's reaction bar; plain emoji only while the pack isn't there. */
  const QUICK_BASE = ["❤️", "🔥", "😂", "😮", "😢", "👍"];
  const QUICK = $derived.by(() => {
    const all = quickReactions($reactionPack.items); // in DEFAULT_REACTIONS order
    return QUICK_BASE.map((e) => all[DEFAULT_REACTIONS.indexOf(e)] ?? e);
  });

  let v = $derived($storyViewer);
  /** Live stories, my archive (newest first), or the ones pinned to a profile. */
  const src = $derived(v?.src ?? "live");
  const storiesOf = (u: string): Story[] => src === "archive" ? allMyStories($myStories, $storyArchive)
    : src === "pinned" ? (u === "" ? $pinnedStories.slice().reverse() : saneStories($peerPresence[u]?.sp, Date.now(), true).reverse())
    : u === "" ? $myStories.filter((s) => isLive(s)) : saneStories($peerPresence[u]?.st);
  let list = $derived<Story[]>(v ? storiesOf(v.user) : []);
  let cur = $derived(v ? list[v.index] ?? null : null);
  const currentId = $derived(cur?.id);
  let url = $state("");
  let photoReady = $state(false);
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
  /** People one after another downwards (a feed) instead of sliding sideways. */
  const vert = $derived($storyNav === "vertical");
  const paused = $derived(held || replyFocus || menu || viewersOpen || hidden);

  /** Decrypted files, by story id (shared with the side previews). */
  const cache = new Map<string, string>();
  const pending = new Map<string, Promise<string>>();
  let cacheTick = $state(0);
  function load(s: Story): Promise<string> {
    const hit = cache.get(s.id);
    if (hit) {
      cache.delete(s.id);
      cache.set(s.id, hit);
      return Promise.resolve(hit);
    }
    const existing = pending.get(s.id);
    if (existing) return existing;

    const own = untrack(() => [$myStories, $storyArchive, $pinnedStories].some((stories) => stories.some((item) => item.id === s.id)));
    const request = (own ? storyFile(s) : api.fetchAttachment(s.media)).then((path) => {
      const url = assetUrl(path);
      cache.set(s.id, url);
      if (cache.size > 12) {
        const oldest = cache.keys().next().value;
        if (oldest !== undefined) cache.delete(oldest);
      }
      cacheTick++;
      return url;
    }).finally(() => pending.delete(s.id));
    pending.set(s.id, request);
    return request;
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
  let retry = $state(0);
  $effect(() => {
    const id = currentId;
    void retry;
    const s = untrack(() => cur);
    if (!id || !s) return;
    let active = true;
    const owner = untrack(() => v?.user ?? "");
    err = ""; progress = 0; photoReady = false;
    const hit = cache.get(s.id);
    url = hit ?? "";
    void load(s).then((u) => {
      if (!active || cur?.id !== s.id) return;
      url = u;
      if (owner && src === "live") {
        const first = !$seenStories.includes(s.id);
        markSeen(s.id);
        if (first) sendStoryView(owner, s.id);
      }
      prefetch();
    }).catch((e) => {
      if (!active || cur?.id !== s.id) return;
      // The relay lost the file (its cache is cleared on restart): the
      // author's app puts it back once it's online (stories.ts::healMyStories).
      err = /responded 404/.test(String(e)) ? tr("The file isn't on the server right now — it comes back when the author is online.") : String(e);
      if (owner === "" && src === "live") void import("../stories").then((m) => m.healMyStories());
    });
    return () => { active = false; };
  });
  function prefetch() {
    if (!v) return;
    const u = neighbour(1);
    const nextStory = list[v.index + 1] ?? (u == null ? undefined : storiesOf(u)[startIndex(u)]);
    if (nextStory) void load(nextStory).catch(() => {});
  }
  // No story left (expired / deleted): next person or close.
  $effect(() => { if (v && !cur) { if (list.length && v.index >= list.length) nextUser(); else if (!list.length) nextUser(); } });
  $effect(() => { if (!v) { cancelGesture(); viewersOpen = false; menu = false; reply = ""; replyFocus = false; held = false; } });

  // Photo timer (videos follow their own time).
  $effect(() => {
    if (!cur || !photoReady || cur.media.kind === "video") return;
    let last = performance.now();
    let raf = 0;
    const step = (now: number) => {
      if (!paused && !turning && !dragging) progress = Math.min(1, progress + (now - last) / PHOTO_MS);
      last = now;
      if (progress >= 1) { next(); return; }
      raf = requestAnimationFrame(step);
    };
    raf = requestAnimationFrame(step);
    return () => cancelAnimationFrame(raf);
  });
  $effect(() => { const el = videoEl; if (!el) return; if (paused || turning || dragging) el.pause(); else void el.play().catch(() => {}); });

  /** Where to start in someone's stories: the first one not yet seen. */
  function startIndex(u: string): number {
    if (u === "" || src !== "live") return 0;
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
  function close() {
    cancelGesture();
    storyViewer.set(null);
  }
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
    if (!v || turning) return;
    if (v.index + 1 < list.length) storyViewer.set({ ...v, index: v.index + 1 });
    else if (neighbour(1) != null) advance(1);
    else close();
  }
  function prev() {
    if (!v || turning) return;
    if (v.index > 0) storyViewer.set({ ...v, index: v.index - 1 });
    else prevUser();
  }

  let down: { x: number; y: number; at: number; width: number; id: number } | null = null;
  let axis: "" | "x" | "y" = "";
  let holdTimer: ReturnType<typeof setTimeout> | undefined;
  let motionTimer: ReturnType<typeof setTimeout> | undefined;
  let dy = $state(0);
  let offset = $state(0);
  let previewDir = $state<1 | -1 | 0>(0);
  let turning = $state(false);
  let dragging = $state(false);
  let cubeEl = $state<HTMLDivElement | null>(null);

  function resetPointer() {
    clearTimeout(holdTimer);
    down = null;
    held = false;
    dragging = false;
  }

  function cancelGesture() {
    resetPointer();
    clearTimeout(motionTimer);
    turning = false;
    offset = 0;
    previewDir = 0;
    dy = 0;
  }

  function pdown(e: PointerEvent) {
    if (!e.isPrimary || e.button !== 0 || turning || menu || viewersOpen || (e.target as Element).closest("button, input, .no-tap")) return;
    const stage = e.currentTarget as HTMLElement;
    down = { x: e.clientX, y: e.clientY, at: performance.now(), width: stage.clientWidth, id: e.pointerId };
    dragging = true;
    axis = "";
    stage.setPointerCapture(e.pointerId);
    holdTimer = setTimeout(() => { held = true; }, 220);
  }

  function pmove(e: PointerEvent) {
    if (!down || e.pointerId !== down.id) return;
    const dx = e.clientX - down.x;
    const delta = e.clientY - down.y;
    if (!axis && Math.hypot(dx, delta) > 10) {
      axis = Math.abs(dx) > Math.abs(delta) ? "x" : "y";
      clearTimeout(holdTimer);
      held = false;
    }
    if (axis === "x" && !vert) {
      offset = neighbour(dx < 0 ? 1 : -1) == null ? dx * 0.18 : dx;
    } else if (axis === "y") {
      if (vert && (delta < 0 || neighbour(-1) != null)) {
        offset = neighbour(delta < 0 ? 1 : -1) == null ? delta * 0.18 : delta;
      } else {
        offset = 0;
        dy = Math.max(0, delta);
      }
    }
    if (offset) { dy = 0; previewDir = offset < 0 ? 1 : -1; }
  }

  function pup(e: PointerEvent) {
    const gesture = down;
    if (!gesture || e.pointerId !== gesture.id) return;
    const wasHeld = held;
    resetPointer();
    const fast = performance.now() - gesture.at < 260;
    if (offset) {
      const dir = offset < 0 ? 1 : -1;
      const extent = vert ? cubeEl?.clientHeight ?? 0 : gesture.width;
      if ((Math.abs(offset) > extent * 0.22 || (fast && Math.abs(offset) > 40)) && neighbour(dir) != null) advance(dir);
      else animate(0);
      return;
    }
    if (axis === "y") {
      const delta = e.clientY - gesture.y;
      if (dy > 110 || (fast && dy > 50)) { close(); return; }
      if (dy) animate(0);
      if (delta < -70 && !vert) {
        if (v?.user === "") viewersOpen = true;
        else replyEl?.focus();
      }
      return;
    }
    if (axis || wasHeld) return;
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    if (e.clientX - rect.left < gesture.width * 0.3) prev(); else next();
  }

  function pcancel(e: PointerEvent) {
    if (down?.id !== e.pointerId) return;
    resetPointer();
    if (offset || dy) animate(0);
  }

  function animate(target: number, user?: string) {
    const duration = motionMs(230);
    const finish = () => {
      if (user !== undefined) goUser(user);
      turning = false;
      offset = 0;
      previewDir = 0;
      dy = 0;
    };
    if (!duration) { finish(); return; }
    turning = true;
    if (target) previewDir = target < 0 ? 1 : -1;
    offset = target;
    dy = 0;
    motionTimer = setTimeout(finish, duration);
  }

  function advance(dir: 1 | -1) {
    if (turning) return;
    const user = neighbour(dir);
    if (user == null) return;
    const extent = vert ? cubeEl?.clientHeight : cubeEl?.clientWidth;
    if (!extent) { goUser(user); return; }
    animate(-dir * extent, user);
  }
  /** Arrows / keys between people. */
  function stepUser(dir: 1 | -1) {
    if (turning) return;
    if (neighbour(dir) == null) { if (dir === 1) close(); return; }
    advance(dir);
  }
  // Computer, feed mode: the wheel goes from person to person.
  let wheelAcc = 0, wheelAt = 0;
  function wheel(e: WheelEvent) {
    if (!vert || viewersOpen || (e.target as Element).closest?.(".sheet")) return;
    const now = performance.now();
    if (now - wheelAt < 450) return;
    wheelAcc += e.deltaY;
    if (Math.abs(wheelAcc) > 60) { const d = wheelAcc > 0 ? 1 : -1; wheelAcc = 0; wheelAt = now; stepUser(d); }
  }
  function key(e: KeyboardEvent) {
    if (!v || replyFocus || menu || viewersOpen || e.defaultPrevented || (e.target instanceof Element && e.target.closest("button, input"))) return;
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
  function quick(r: string) {
    const s = cur, owner = v?.user;
    if (!s || !owner) return;
    if (!$likedStories.includes(s.id)) toggleLike(owner, s.id);
    replyEl?.blur();
    // A pack emoji goes into the reply as the chat's custom-emoji token, so
    // it shows as the same picture there.
    const { file, pack, emoji } = parseReaction(r);
    void sendReply(file ? `[ce:${pack ?? REACTION_PACK}/${file}]` : emoji || r);
  }
  const pinnedNow = $derived(!!cur && $pinnedStories.some((p) => p.id === cur!.id || p.from === cur!.id));
  async function pin() {
    const s = cur;
    menu = false;
    if (!s) return;
    showToast(tr("Pinning to your profile…"), 20000);
    try { await pinStory(s); showToast(tr("Pinned to your profile — people who may see it find it there"), 3500); }
    catch (e) { showToast(`${tr("Could not pin")}: ${e}`, 6000); }
  }
  /** Send this story to a chat (Telegram's "Share"): as a photo / video. */
  async function share() {
    const s = cur, owner = v?.user ?? "";
    menu = false;
    if (!s) return;
    try {
      const path = owner === "" ? await storyFile(s) : await api.fetchAttachment(s.media);
      const { openForward } = await import("./ForwardPicker.svelte");
      const name = s.media.name || (s.media.kind === "video" ? "story.mp4" : "story.jpg");
      close();
      openForward([{ kind: "file", path, name, author: owner ? { from: owner, name: nameOf(owner) } : undefined }]);
    } catch (e) { showToast(`${tr("Could not share")}: ${e}`, 5000); }
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
  const turnTo = $derived(previewDir === 1 ? sideR : previewDir === -1 ? sideL : null);
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
      {#key (s?.id ?? "") + ":" + (live ? url : cached(s))}
        <img class="fg" src={live ? url : cached(s)} alt="" draggable="false" decoding="async"
          onload={() => { if (live) photoReady = true; }}
          onerror={() => {
            if (!live || !s) return;
            cache.delete(s.id);
            url = "";
            err = tr("Could not load the story");
          }} />
      {/key}
    {:else if live && err}
      <div class="msg">{$t("Could not load the story")}<small>{err}</small><button class="pin-btn" onclick={() => retry++}>{$t("Retry")}</button></div>
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
        <span class="bar"><i style:transform="scaleX({live && v ? (i < v.index ? 1 : i === v.index ? progress : 0) : 0})"></i></span>
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
    style:--dy="{dy}px" style:--dismiss-opacity={Math.max(0.42, 1 - dy / 550)} class:anim={turning} onwheel={wheel}>
    {#if !IS_MOBILE}
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
          <div class="cube" class:vert style:--offset="{offset}px"
            onpointerdown={pdown} onpointermove={pmove} onpointerup={pup} onpointercancel={pcancel} onlostpointercapture={pcancel} oncontextmenu={(e) => e.preventDefault()}>
            <div class="face front">{@render face(v.user, cur, true)}</div>
            {#if turnTo != null}
              <div class="face preview" inert class:right={!vert && previewDir === 1} class:left={!vert && previewDir === -1} class:below={vert && previewDir === 1} class:above={vert && previewDir === -1}>{@render face(turnTo, turnStory, false)}</div>
            {/if}
          </div>
          {#if menu}
            <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
            <div class="menu-scrim" onclick={() => (menu = false)}></div>
            <div class="menu" role="menu">
              {#if v.user === ""}
                {#if src === "live"}<button onclick={() => { menu = false; viewersOpen = true; }}>{$t("Viewers")}</button>{/if}
                {#if src === "pinned"}
                  <button onclick={() => { const s = cur; menu = false; if (s) void unpinStory(s.id).then(() => showToast(tr("Removed from your profile"))); }}>{$t("Remove from profile")}</button>
                {:else if pinnedNow}
                  <button onclick={() => { const s = cur; menu = false; if (s) void unpinStory(s.id).then(() => showToast(tr("Removed from your profile"))); }}>{$t("Remove from profile")}</button>
                {:else}
                  <button onclick={pin}>{$t("Pin to profile")}</button>
                {/if}
                <button onclick={share}>{$t("Share")}</button>
                {#if src === "live"}<button onclick={() => { menu = false; close(); storyArchiveOpen.set(true); }}>{$t("Story archive")}</button>{/if}
                {#if src !== "pinned"}<button class="danger" onclick={remove}>{$t("Delete story")}</button>{/if}
              {:else}
                {@const u = v.user}
                <button onclick={() => { menu = false; close(); void selectChat(u); }}>{$t("Send message")}</button>
                <button onclick={share}>{$t("Share")}</button>
                {#if src === "live"}<button onclick={() => { menu = false; const was = $hiddenStories.includes(u); toggleHidden(u); showToast(tr(was ? "Stories are shown again" : "Stories hidden — they're now at the end of the strip")); }}>{$t($hiddenStories.includes(u) ? "Show stories" : "Hide stories")}</button>{/if}
              {/if}
            </div>
          {/if}
        </div>

        <div class="bottom" class:gone={held}>
          {#if v.user === "" && src !== "live"}
            <span class="src-tag">{$t(src === "archive" ? "Story archive" : "Pinned to profile")}{#if src === "archive" && pinnedNow} · {$t("in profile")}{/if}</span>
            <span class="grow"></span>
            {#if src === "archive" && !pinnedNow}<button class="pin-btn" onclick={pin}>{$t("Pin to profile")}</button>{/if}
          {:else if v.user === ""}
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
                  {@const ru = reactionUrl(q, $reactionPack.items)}
                  <button onpointerdown={(e) => e.preventDefault()} onclick={() => quick(q)} aria-label={parseReaction(q).emoji || q}>
                    {#if ru}<img class="qe" src={ru.url} alt={parseReaction(q).emoji} draggable="false" />{:else}{parseReaction(q).emoji || q}{/if}
                  </button>
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
              {#if w.r}{@const ru = reactionUrl(w.r, $reactionPack.items)}<span class="vr-emo">{#if ru}<img class="qe sm" src={ru.url} alt={w.r} />{:else}{w.r}{/if}</span>{/if}
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
    animation: sv-in .16s ease-out; }
  @keyframes sv-in { from { opacity: 0; } }
  .stage-row { flex: 1; min-height: 0; display: flex; align-items: center; justify-content: center;
    transform: translateY(var(--dy)); opacity: var(--dismiss-opacity); }
  .col { position: relative; flex: 1; height: 100%; min-width: 0; display: flex; flex-direction: column; }
  .cube-wrap { position: relative; flex: 1; min-height: 0; overflow: hidden; border-radius: calc(14px * var(--rk, 1)); background: #111216; }
  .cube { position: absolute; inset: 0; touch-action: none; transform: translateX(var(--offset)); }
  .cube.vert { transform: translateY(var(--offset)); }
  .sv.anim .cube, .sv.anim .stage-row { transition: transform 230ms cubic-bezier(.2,.72,.25,1), opacity 230ms ease-out; }
  .face { position: absolute; inset: 0; overflow: hidden; background: #111216; }
  .face.right { transform: translateX(100%); }
  .face.left { transform: translateX(-100%); }
  .face.below { transform: translateY(100%); }
  .face.above { transform: translateY(-100%); }
  .preview { pointer-events: none; }
  .media { position: absolute; inset: 0; display: grid; place-items: center; }
  .media img, .media video { grid-area: 1 / 1; width: 100%; height: 100%; object-fit: contain; pointer-events: none; }
  .media img.fg, .media video { position: relative; z-index: 1; }
  .cap { z-index: 2; }
  .ph { position: absolute; inset: 0; background: #111 center / cover; opacity: .35; }
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
  .bar i { display: block; height: 100%; background: #fff; transform-origin: left; }
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
  .qe { width: 34px; height: 34px; object-fit: contain; display: block; pointer-events: none; }
  .qe.sm { width: 22px; height: 22px; }
  .views { display: flex; align-items: center; gap: 8px; border: none; background: transparent; color: #fff; font: 600 14px var(--font-ui, inherit); cursor: pointer; padding: 8px 6px; }
  .views svg { width: 20px; height: 20px; }
  .vr { opacity: .8; font-weight: 500; }
  .grow { flex: 1; }
  .src-tag { font-size: 13.5px; opacity: .8; padding: 0 6px; }
  .pin-btn { border: none; border-radius: 999px; padding: 9px 16px; background: rgba(255,255,255,.14); color: #fff; font: 600 13.5px var(--font-ui, inherit); cursor: pointer; }
  .pin-btn:hover { background: rgba(255,255,255,.22); }
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
  .close-x { position: absolute; top: 14px; right: 14px; z-index: 6; background: rgba(255,255,255,.08); }
  .desk .stage-row { gap: 12px; padding: 20px 0; }
  .desk .col { flex: 0 1 auto; height: min(100%, 900px); aspect-ratio: 9 / 17.2; max-width: calc(100% - 120px); }
  @media (prefers-reduced-motion: reduce) { .sv { animation: none; } }
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
