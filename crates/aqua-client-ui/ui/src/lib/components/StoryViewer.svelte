<script lang="ts">
  /**
   * Full-screen stories (Telegram): bars on top, tap right / left for the
   * next / previous one, hold to pause, swipe down or Back to close.
   * Their files are fetched and decrypted on first view.
   */
  import { t, tr } from "../i18n";
  import { api, assetUrl } from "../tauri";
  import { onBack } from "../back";
  import { storyViewer, myStories, saneStories, markSeen, deleteStory, STORY_TTL_MS, type Story } from "../stories";
  import { peerPresence } from "../presence";
  import { avatars, peerCards } from "../people";
  import { profile } from "../profile";
  import { chats } from "../stores";
  import { streamSrc } from "../media";
  import { askConfirm } from "./Confirm.svelte";

  const PHOTO_MS = 6000;

  let v = $derived($storyViewer);
  let list = $derived<Story[]>(!v ? [] : v.user === "" ? $myStories.filter((s) => Date.now() - s.at < STORY_TTL_MS) : saneStories($peerPresence[v.user]?.st));
  let cur = $derived(v ? list[v.index] ?? null : null);
  let url = $state("");
  let loading = $state(false);
  let err = $state("");
  let progress = $state(0);
  let paused = $state(false);
  let videoEl = $state<HTMLVideoElement | null>(null);

  $effect(() => { if (v) return onBack(close); });

  // Load the current story's file.
  let loadedFor = "";
  $effect(() => {
    const s = cur;
    if (!s) return;
    if (loadedFor === s.id) return;
    loadedFor = s.id;
    url = ""; err = ""; progress = 0; loading = true;
    void api.fetchAttachment(s.media).then((p) => {
      if (cur?.id !== s.id) return;
      url = assetUrl(p);
      loading = false;
      if (v?.user) markSeen(s.id);
    }).catch((e) => { if (cur?.id === s.id) { err = String(e); loading = false; } });
  });
  // No story left (expired / deleted): next person or close.
  $effect(() => { if (v && !cur) { if (list.length && v.index >= list.length) nextUser(); else if (!list.length) nextUser(); } });

  // Photo timer (videos follow their own time).
  $effect(() => {
    if (!cur || !url || loading || cur.media.kind === "video") return;
    let last = performance.now();
    let raf = 0;
    const step = (now: number) => {
      if (!paused) progress = Math.min(1, progress + (now - last) / PHOTO_MS);
      last = now;
      if (progress >= 1) { next(); return; }
      raf = requestAnimationFrame(step);
    };
    raf = requestAnimationFrame(step);
    return () => cancelAnimationFrame(raf);
  });
  $effect(() => { const el = videoEl; if (!el) return; if (paused) el.pause(); else void el.play().catch(() => {}); });

  function close() { storyViewer.set(null); loadedFor = ""; }
  function nextUser() {
    if (!v) return;
    const i = v.users.indexOf(v.user);
    const u = v.users[i + 1];
    if (u === undefined) { close(); return; }
    storyViewer.set({ ...v, user: u, index: 0 });
  }
  function prevUser() {
    if (!v) return;
    const i = v.users.indexOf(v.user);
    if (i <= 0) { storyViewer.set({ ...v, index: 0 }); progress = 0; return; }
    storyViewer.set({ ...v, user: v.users[i - 1], index: 0 });
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

  // Tap / hold / swipe down.
  let down: { x: number; y: number; t: number } | null = null;
  let holdT: ReturnType<typeof setTimeout> | null = null;
  let dy = $state(0);
  function pdown(e: PointerEvent) {
    if ((e.target as HTMLElement).closest("button")) return;
    down = { x: e.clientX, y: e.clientY, t: performance.now() };
    holdT = setTimeout(() => (paused = true), 200);
  }
  function pmove(e: PointerEvent) {
    if (!down) return;
    dy = Math.max(0, e.clientY - down.y);
  }
  function pup(e: PointerEvent) {
    if (holdT) { clearTimeout(holdT); holdT = null; }
    const d = down;
    down = null;
    const wasPaused = paused;
    paused = false;
    if (!d) return;
    if (dy > 110) { dy = 0; close(); return; }
    dy = 0;
    if (wasPaused && performance.now() - d.t > 200) return;
    const w = (e.currentTarget as HTMLElement).clientWidth;
    if (e.clientX < w * 0.3) prev(); else next();
  }
  function key(e: KeyboardEvent) {
    if (!v) return;
    if (e.key === "ArrowRight") { e.preventDefault(); next(); }
    else if (e.key === "ArrowLeft") { e.preventDefault(); prev(); }
    else if (e.key === " ") { e.preventDefault(); paused = !paused; }
  }
  async function remove() {
    const s = cur;
    if (!s) return;
    paused = true;
    const ok = await askConfirm({ title: tr("Delete this story?"), ok: tr("Delete"), danger: true });
    paused = false;
    if (!ok) return;
    await deleteStory(s.id);
  }
  const nameOf = (u: string) => (u === "" ? $profile.display_name || tr("You") : $chats.find((c) => c.id === u)?.name ?? $peerCards[u]?.display_name ?? u);
  const avaOf = (u: string) => (u === "" ? $profile.avatar_url ?? "" : $avatars[u] ?? "");
  function ago(at: number): string {
    const m = Math.max(0, Math.round((Date.now() - at) / 60000));
    return m < 1 ? tr("just now") : m < 60 ? tr("{n} min ago", { n: m }) : tr("{n} h ago", { n: Math.floor(m / 60) });
  }
</script>

<svelte:window onkeydown={key} />

{#if v && cur}
  <div class="sv" data-own-back role="dialog" aria-modal="true" aria-label={$t("Stories")} style:--dy="{dy}px">
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="stage" onpointerdown={pdown} onpointermove={pmove} onpointerup={pup} onpointercancel={pup} oncontextmenu={(e) => e.preventDefault()}>
      {#if url && cur.media.kind === "video"}
        <!-- svelte-ignore a11y_media_has_caption -->
        <video bind:this={videoEl} use:streamSrc={url} autoplay playsinline
          ontimeupdate={(e) => { const el = e.currentTarget as HTMLVideoElement; if (el.duration) progress = el.currentTime / el.duration; }}
          onended={next}></video>
      {:else if url}
        <!-- Whole picture, never cropped or stretched; its own blur fills the edges (Telegram). -->
        <img class="bg" src={url} alt="" draggable="false" aria-hidden="true" />
        <img class="fg" src={url} alt="" draggable="false" />
      {:else if err}
        <div class="msg">{$t("Could not load the story")}<small>{err}</small></div>
      {:else}
        <div class="spin"></div>
      {/if}
      {#if cur.caption}<div class="cap">{cur.caption}</div>{/if}
    </div>
    <div class="top">
      <div class="bars">
        {#each list as s, i (s.id)}
          <span class="bar"><i style:width="{i < v.index ? 100 : i === v.index ? progress * 100 : 0}%"></i></span>
        {/each}
      </div>
      <div class="who">
        <span class="ava" style:background-image={avaOf(v.user) ? `url("${avaOf(v.user)}")` : undefined}>{avaOf(v.user) ? "" : nameOf(v.user).charAt(0).toUpperCase()}</span>
        <span class="nm"><b>{nameOf(v.user)}</b><small>{ago(cur.at)}</small></span>
        {#if v.user === ""}
          <button class="ic" onclick={remove} title={$t("Delete")} aria-label={$t("Delete")}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13"/></svg>
          </button>
        {/if}
        <button class="ic" data-x data-keep-x onclick={close} aria-label={$t("Close")}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M6 6l12 12M18 6 6 18"/></svg>
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .sv { position: fixed; inset: 0; z-index: 10094; background: #000; color: #fff; user-select: none; -webkit-user-select: none;
    transform: translateY(var(--dy)); opacity: calc(1 - var(--dy) / 600px); animation: sv-in .22s cubic-bezier(.2,.9,.25,1); }
  @keyframes sv-in { from { transform: scale(.94); opacity: 0; } }
  .stage { position: absolute; inset: 0; display: grid; place-items: center; touch-action: none; }
  .stage img, .stage video { grid-area: 1 / 1; max-width: 100%; max-height: 100%; width: 100%; height: 100%; object-fit: contain; pointer-events: none; }
  .stage img.bg { object-fit: cover; filter: blur(28px) brightness(.55); transform: scale(1.15); }
  .msg { display: flex; flex-direction: column; gap: 6px; align-items: center; font-size: 15px; text-align: center; padding: 20px; }
  .msg small { font-size: 12px; opacity: .6; max-width: 80vw; overflow-wrap: anywhere; }
  .spin { width: 36px; height: 36px; border-radius: 50%; border: 3px solid rgba(255,255,255,.25); border-top-color: #fff; animation: spin .8s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  .cap { position: absolute; left: 0; right: 0; bottom: 0; padding: 40px 18px calc(22px + env(safe-area-inset-bottom, 0px)); font-size: 15px; line-height: 1.4; text-align: center;
    background: linear-gradient(transparent, rgba(0,0,0,.6)); white-space: pre-wrap; overflow-wrap: anywhere; }
  .top { position: absolute; left: 0; right: 0; top: 0; padding: calc(8px + env(safe-area-inset-top, 0px)) 10px 18px; background: linear-gradient(rgba(0,0,0,.55), transparent); }
  .bars { display: flex; gap: 4px; }
  .bar { flex: 1; height: 3px; border-radius: 2px; background: rgba(255,255,255,.35); overflow: hidden; }
  .bar i { display: block; height: 100%; background: #fff; }
  .who { display: flex; align-items: center; gap: 10px; margin-top: 10px; }
  .ava { width: 36px; height: 36px; border-radius: var(--ava-r, 50%); background: rgba(255,255,255,.2) center / cover; display: grid; place-items: center; font-weight: 800; flex-shrink: 0; }
  .nm { display: flex; flex-direction: column; flex: 1; min-width: 0; }
  .nm b { font-size: 14.5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .nm small { font-size: 12px; opacity: .75; }
  .ic { width: 38px; height: 38px; border: none; border-radius: 50%; background: transparent; color: #fff; display: grid; place-items: center; cursor: pointer; }
  .ic:hover { background: rgba(255,255,255,.12); }
  .ic svg { width: 21px; height: 21px; }
</style>
