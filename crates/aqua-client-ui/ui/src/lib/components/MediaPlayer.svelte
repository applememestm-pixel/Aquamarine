<script module lang="ts">
  /** Volume / speed / mute are remembered for every player in the app. */
  const PREF_KEY = "aqua.player.v1";
  interface Prefs { volume: number; muted: boolean; speed: number }
  function loadPrefs(): Prefs {
    try {
      const p = JSON.parse(localStorage.getItem(PREF_KEY) ?? "null");
      if (p && typeof p.volume === "number") return { volume: p.volume, muted: !!p.muted, speed: p.speed || 1 };
    } catch { /* defaults */ }
    return { volume: 1, muted: false, speed: 1 };
  }
  function savePrefs(p: Prefs) {
    try { localStorage.setItem(PREF_KEY, JSON.stringify(p)); } catch { /* ignore */ }
  }
  export const SPEEDS = [0.5, 0.75, 1, 1.25, 1.5, 1.75, 2];
  /** Only one player plays at a time (starting one pauses the others). */
  let active: HTMLMediaElement | null = null;
</script>

<script lang="ts">
  /**
   * The app's one audio / video player (chat bubbles, carousels, the
   * lightbox, link previews). Own controls everywhere — the same look on
   * WebKitGTK, WebView2 and WKWebView — with:
   *  - seek bar with buffered range and hover time, ±10 s skips
   *  - speed menu, loop, volume (wheel works), all remembered
   *  - info: name, resolution, size, duration
   *  - "window" mode (fills the app window) and real full screen
   *  - keyboard: Space/K, ←/→ 5 s, J/L 10 s, ↑/↓ volume, M, F, W, 0–9, < >
   */
  import { t, tr } from "../i18n";
  import { streamable, fmtSize, localPathOf } from "../media";
  import { api } from "../tauri";
  import { showToast } from "./Toast.svelte";
  import { onDestroy } from "svelte";

  interface Props {
    src: string;
    kind?: "audio" | "voice" | "video";
    /** Extra button (opens the lightbox). */
    onExpand?: () => void;
    poster?: string;
    name?: string;
    size?: number;
    autoplay?: boolean;
    /** Fill the parent box (lightbox / carousel slide) instead of sizing to the video. */
    fill?: boolean;
  }
  let { src, kind = "audio", onExpand, poster, name, size, autoplay = false, fill = false }: Props = $props();

  // Local files stream over loopback HTTP (see lib/media.ts::streamUrl).
  let playSrc = $state("");
  let converted = "";      // src we already re-encoded (try once)
  let converting = $state(false);
  $effect(() => {
    const s = src;
    playSrc = "";
    started = false;
    converting = false;
    void streamable(s)
      .then((u) => { if (src === s) playSrc = u; })
      .catch((e) => { failed = e instanceof Error ? e.message : String(e); showToast(failed, 12000); });
  });

  const prefs = loadPrefs();
  let media = $state<HTMLMediaElement | null>(null);
  let box = $state<HTMLDivElement | null>(null);
  let playing = $state(false);
  let started = $state(false);
  let waiting = $state(false);
  let failed = $state("");
  let current = $state(0);
  let duration = $state(0);
  let buffered = $state(0);
  let volume = $state(prefs.volume);
  let muted = $state(prefs.muted);
  let speed = $state(prefs.speed);
  let loop = $state(false);
  let vw = $state(0);
  let vh = $state(0);
  let seeking = false;

  function persist() { savePrefs({ volume, muted, speed }); }

  function play() {
    if (!media) return;
    if (active && active !== media && !active.paused) active.pause();
    active = media;
    void media.play().catch((e) => { if (e?.name !== "AbortError") console.warn("[aqua-media] play()", e); });
  }
  // Phones: a tap on a video in the chat opens the viewer, as in Telegram
  // (playing it inside the bubble and a double tap for full screen was
  // clumsy on a small screen).
  const coarse = typeof matchMedia !== "undefined" && matchMedia("(pointer: coarse)").matches;
  function tapVideo() {
    if (onExpand && coarse && !theater && !isFs) { onExpand(); return; }
    toggle();
  }
  function toggle() {
    if (!media) return;
    if (media.paused) play(); else media.pause();
  }
  function onMeta() {
    if (!media) return;
    readDuration();
    media.playbackRate = speed;
    media.volume = volume;
    media.muted = muted;
    if (media instanceof HTMLVideoElement) { vw = media.videoWidth; vh = media.videoHeight; }
    if (autoplay && !started) play();
  }
  function readDuration() {
    if (!media) return;
    const d = media.duration;
    if (isFinite(d) && d > 0) duration = d;
    else if (media.seekable.length) duration = media.seekable.end(media.seekable.length - 1);
  }
  function onTime() {
    if (!media) return;
    if (!seeking) current = media.currentTime;
    if (!duration) readDuration();
    const b = media.buffered;
    for (let i = 0; i < b.length; i++) {
      if (b.start(i) <= media.currentTime + 0.5) buffered = b.end(i);
    }
  }
  function onPlay() { playing = true; started = true; failed = ""; poke(); }
  function onPause() { playing = false; showControls = true; }
  function onEnded() { playing = false; showControls = true; }
  function onError() {
    if (!media || !playSrc) return;
    const code = media.error?.code;
    // Unsupported codec / container: re-encode with ffmpeg and try again.
    const path = localPathOf(src);
    if ((code === 4 || code === 3) && path && converted !== src) {
      converted = src;
      converting = true;
      failed = "";
      const s = src;
      void api.transcodeMedia(path)
        .then((out) => api.mediaStreamUrl(out))
        .then((u) => { if (src === s) { playSrc = u; if (started || autoplay) queueMicrotask(play); } })
        .catch((e) => { if (src === s) failed = `${tr("This format can't be played here")}: ${e}`; })
        .finally(() => { if (src === s) converting = false; });
      return;
    }
    failed = code === 4 || code === 3 ? tr("This format can't be played here") : tr("Playback failed");
  }

  function seekTo(t: number) {
    if (!media || !duration) return;
    const v = Math.max(0, Math.min(duration, t));
    current = v;
    media.currentTime = v;
  }
  const skip = (d: number) => { if (media) seekTo(media.currentTime + d); poke(); };
  function setVolume(v: number) {
    volume = Math.max(0, Math.min(1, v));
    muted = volume === 0;
    if (media) { media.volume = volume; media.muted = muted; }
    persist();
  }
  function toggleMute() {
    muted = !muted;
    if (!muted && volume === 0) volume = 0.6;
    if (media) { media.muted = muted; media.volume = volume; }
    persist();
  }
  function setSpeed(s: number) {
    speed = s;
    if (media) media.playbackRate = s;
    speedOpen = false;
    persist();
  }
  function setLoop() { loop = !loop; if (media) media.loop = loop; }

  // ---- seek bar
  let bar = $state<HTMLDivElement | null>(null);
  let hoverX = $state<number | null>(null);
  let hoverT = $derived(hoverX != null && bar && duration ? (hoverX / bar.clientWidth) * duration : 0);
  function fracAt(e: PointerEvent): number {
    const r = bar!.getBoundingClientRect();
    return Math.max(0, Math.min(1, (e.clientX - r.left) / r.width));
  }
  function barDown(e: PointerEvent) {
    if (!bar || !duration) return;
    e.stopPropagation();
    seeking = true;
    bar.setPointerCapture(e.pointerId);
    current = fracAt(e) * duration;
  }
  function barMove(e: PointerEvent) {
    if (!bar) return;
    const r = bar.getBoundingClientRect();
    hoverX = Math.max(0, Math.min(r.width, e.clientX - r.left));
    if (seeking && duration) current = fracAt(e) * duration;
  }
  function barUp(e: PointerEvent) {
    if (!seeking) return;
    seeking = false;
    seekTo(fracAt(e) * duration);
  }

  // ---- window mode (fills the app window) / full screen
  let theater = $state(false);
  let isFs = $state(false);
  let home: { parent: Node; next: Node | null } | null = null;
  function setTheater(on: boolean) {
    if (!box || on === theater) return;
    // Out of the chat's stacking contexts (transforms / filters would
    // trap a position:fixed box inside the bubble): park it on <body>.
    if (on) {
      home = { parent: box.parentNode!, next: box.nextSibling };
      document.body.appendChild(box);
    } else if (home) {
      home.parent.insertBefore(box, home.next);
      home = null;
    }
    theater = on;
    if (!on) { sideways = false; if (isFs && "__TAURI_INTERNALS__" in window) { isFs = false; void windowFs(false); } }
    poke();
    queueMicrotask(() => box?.focus());
  }
  // Phones: "full screen" is the window mode, turned sideways when the video
  // is wide and the phone is held upright (Android's webview has no element
  // full screen and won't lock the orientation).
  const PHONE = typeof document !== "undefined" && document.documentElement.classList.contains("phone");
  let sideways = $state(false);
  // Desktop app: the element's full screen fills only the webview — make the
  // window itself full screen too.
  async function windowFs(on: boolean) {
    if (!("__TAURI_INTERNALS__" in window) || PHONE) return;
    try { const { getCurrentWindow } = await import("@tauri-apps/api/window"); await getCurrentWindow().setFullscreen(on); } catch { /* not allowed */ }
  }
  async function toggleFs() {
    if (!box) return;
    if (PHONE) {
      const on = !theater;
      sideways = on && vw > vh && innerHeight > innerWidth;
      setTheater(on);
      return;
    }
    if ("__TAURI_INTERNALS__" in window) {
      const on = !isFs;
      setTheater(on);
      isFs = on;
      await windowFs(on);
      return;
    }
    try {
      if (document.fullscreenElement) await document.exitFullscreen();
      else {
        const el = box as HTMLElement & { webkitRequestFullscreen?: () => Promise<void> };
        if (el.requestFullscreen) await el.requestFullscreen();
        else await el.webkitRequestFullscreen?.();
      }
    } catch {
      // No fullscreen API (some webviews): the window mode is the next best.
      setTheater(!theater);
    }
  }
  $effect(() => {
    const on = () => { isFs = !!box && document.fullscreenElement === box; poke(); };
    document.addEventListener("fullscreenchange", on);
    return () => document.removeEventListener("fullscreenchange", on);
  });
  onDestroy(() => {
    if (home && box) { try { box.remove(); } catch { /* ignore */ } }
    if (active === media) active = null;
  });

  // ---- controls auto-hide (video)
  let showControls = $state(true);
  let hideTimer: ReturnType<typeof setTimeout> | null = null;
  function poke() {
    showControls = true;
    if (hideTimer) clearTimeout(hideTimer);
    hideTimer = setTimeout(() => { if (playing && !speedOpen) showControls = false; }, 2600);
  }

  let speedOpen = $state(false);
  let infoOpen = $state(false);

  function key(e: KeyboardEvent) {
    if ((e.target as HTMLElement)?.tagName === "INPUT") return;
    const k = e.key;
    let hit = true;
    if (k === " " || k === "k" || k === "K") toggle();
    else if (k === "ArrowLeft") skip(-5);
    else if (k === "ArrowRight") skip(5);
    else if (k === "j" || k === "J") skip(-10);
    else if (k === "l" || k === "L") skip(10);
    else if (k === "ArrowUp") setVolume(volume + 0.05);
    else if (k === "ArrowDown") setVolume(volume - 0.05);
    else if (k === "m" || k === "M") toggleMute();
    else if ((k === "f" || k === "F") && kind === "video") void toggleFs();
    else if ((k === "w" || k === "W") && kind === "video") setTheater(!theater);
    else if (k === "Escape" && theater) setTheater(false);
    else if (k === "<" || k === ",") setSpeed(SPEEDS[Math.max(0, SPEEDS.indexOf(speed) - 1)] ?? 1);
    else if (k === ">" || k === ".") setSpeed(SPEEDS[Math.min(SPEEDS.length - 1, SPEEDS.indexOf(speed) + 1)] ?? 1);
    else if (/^[0-9]$/.test(k) && duration) seekTo((Number(k) / 10) * duration);
    else hit = false;
    if (hit) { e.preventDefault(); e.stopPropagation(); }
  }
  function wheelVol(e: WheelEvent) {
    e.preventDefault();
    setVolume(volume + (e.deltaY < 0 ? 0.05 : -0.05));
  }

  function fmt(s: number): string {
    if (!isFinite(s) || s < 0) s = 0;
    const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), sec = Math.floor(s % 60);
    return h ? `${h}:${String(m).padStart(2, "0")}:${String(sec).padStart(2, "0")}` : `${m}:${String(sec).padStart(2, "0")}`;
  }
  let pct = $derived(duration > 0 ? (current / duration) * 100 : 0);
  let bufPct = $derived(duration > 0 ? Math.min(100, (buffered / duration) * 100) : 0);
  let quality = $derived(vh ? (vh >= 2100 ? "4K" : vh >= 1400 ? "1440p" : vh >= 1000 ? "1080p" : vh >= 700 ? "720p" : `${vh}p`) : "");
  let infoLine = $derived([name, quality && `${vw}×${vh}`, size ? fmtSize(size) : "", duration ? fmt(duration) : ""].filter(Boolean).join(" · "));

  // ---- voice waveform: decode once, ~48 bars, click to seek.
  const BARS = 48;
  let peaks = $state<number[]>([]);
  let decodedFor = "";
  $effect(() => {
    if (kind !== "voice" || !playSrc || decodedFor === playSrc) return;
    decodedFor = playSrc;
    peaks = [];
    void computePeaks(playSrc);
  });
  async function computePeaks(url: string) {
    try {
      const buf = await (await fetch(url)).arrayBuffer();
      const Ctx = window.AudioContext || (window as unknown as { webkitAudioContext: typeof AudioContext }).webkitAudioContext;
      const ctx = new Ctx();
      const audio = await ctx.decodeAudioData(buf);
      const data = audio.getChannelData(0);
      const block = Math.floor(data.length / BARS) || 1;
      const out: number[] = [];
      let max = 0.0001;
      for (let i = 0; i < BARS; i++) {
        let sum = 0;
        for (let j = 0; j < block; j++) sum += Math.abs(data[i * block + j] || 0);
        const v = sum / block;
        out.push(v);
        if (v > max) max = v;
      }
      peaks = out.map((v) => Math.max(0.08, v / max));
      if (!duration && audio.duration) duration = audio.duration;
      try { await ctx.close(); } catch { /* ignore */ }
    } catch {
      peaks = [];
    }
  }
  function onWaveClick(e: MouseEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    seekTo(((e.clientX - r.left) / r.width) * duration);
  }

  const I = {
    play: "M8 5.5v13l10.5-6.5z",
    pause: "M7 5h3.5v14H7zM13.5 5H17v14h-3.5z",
    back: "M11.5 7V4L6 8.5l5.5 4.5V10a5 5 0 1 1-5 5H5a6.5 6.5 0 1 0 6.5-8z",
    fwd: "M12.5 7V4L18 8.5 12.5 13V10a5 5 0 1 0 5 5H19a6.5 6.5 0 1 1-6.5-8z",
    vol: "M4 9.5h3.5L12 5.5v13l-4.5-4H4zM15 8.5a5 5 0 0 1 0 7M17.5 6a8.5 8.5 0 0 1 0 12",
    mute: "M4 9.5h3.5L12 5.5v13l-4.5-4H4zM16 9.5l5 5M21 9.5l-5 5",
    loop: "M17 3l3 3-3 3M4 11V9a3 3 0 0 1 3-3h13M7 21l-3-3 3-3M20 13v2a3 3 0 0 1-3 3H4",
    theater: "M3 6.5A1.5 1.5 0 0 1 4.5 5h15A1.5 1.5 0 0 1 21 6.5v11a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 17.5zM7 9h10v6H7z",
    fs: "M4 9V4h5M15 4h5v5M20 15v5h-5M9 20H4v-5",
    fsx: "M9 4v5H4M20 9h-5V4M15 20v-5h5M4 15h5v5",
    expand: "M14 4h6v6M10 20H4v-6M20 4l-7 7M4 20l7-7",
    info: "M12 8h.01M11 12h1v5h1M12 3a9 9 0 1 1 0 18 9 9 0 0 1 0-18z",
  };
</script>

{#snippet ico(d: string, fillIcon = false)}
  <svg viewBox="0 0 24 24" aria-hidden="true" fill={fillIcon ? "currentColor" : "none"} stroke={fillIcon ? "none" : "currentColor"} stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path {d} /></svg>
{/snippet}

{#snippet seekbar()}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="bar" bind:this={bar}
    onpointerdown={barDown} onpointermove={barMove} onpointerup={barUp} onpointercancel={barUp}
    onpointerleave={() => (hoverX = null)}>
    <div class="rail">
      <div class="buf" style={`width:${bufPct}%`}></div>
      <div class="fill" style={`width:${pct}%`}></div>
      <div class="knob" style={`left:${pct}%`}></div>
    </div>
    {#if hoverX != null && duration}
      <span class="tip" style={`left:${hoverX}px`}>{fmt(hoverT)}</span>
    {/if}
  </div>
{/snippet}

{#snippet volume_()}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="vol" onwheel={wheelVol}>
    <button class="ib" onclick={toggleMute} title={$t("Mute (M)")} aria-label={$t("Mute")}>
      {@render ico(muted || volume === 0 ? I.mute : I.vol)}
    </button>
    <input class="vol-slider" type="range" min="0" max="1" step="0.02" value={muted ? 0 : volume}
      style={`--p:${(muted ? 0 : volume) * 100}%`}
      oninput={(e) => setVolume(parseFloat((e.target as HTMLInputElement).value))} aria-label={$t("Volume")} />
  </div>
{/snippet}

{#snippet speedMenu()}
  <div class="spd">
    <button class="txt" class:on={speed !== 1} onclick={() => (speedOpen = !speedOpen)} title={$t("Playback speed")}>{speed}×</button>
    {#if speedOpen}
      <div class="menu" role="menu">
        {#each SPEEDS as s (s)}
          <button role="menuitem" class:on={s === speed} onclick={() => setSpeed(s)}>{s}×</button>
        {/each}
      </div>
    {/if}
  </div>
{/snippet}

{#if kind === "video"}
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <div
    class="vp"
    class:fill
    class:theater
    class:fs={isFs}
    class:sideways
    class:hide={!showControls}
    bind:this={box}
    tabindex="0"
    role="group"
    aria-label={name ?? $t("Video")}
    onkeydown={key}
    onpointermove={poke}
  >
    <!-- svelte-ignore a11y_media_has_caption -->
    <video
      bind:this={media}
      src={playSrc || undefined}
      {poster}
      preload="metadata"
      playsinline
      {loop}
      onloadedmetadata={onMeta}
      ondurationchange={readDuration}
      ontimeupdate={onTime}
      onprogress={onTime}
      onplay={onPlay}
      onpause={onPause}
      onended={onEnded}
      onwaiting={() => (waiting = true)}
      onplaying={() => (waiting = false)}
      oncanplay={() => (waiting = false)}
      onerror={onError}
      onclick={(e) => { e.stopPropagation(); tapVideo(); }}
      ondblclick={(e) => { e.stopPropagation(); if (!coarse) void toggleFs(); }}
    ></video>

    {#if (theater || isFs) && name}
      <div class="top">{name}</div>
    {/if}
    {#if converting}
      <div class="center err">{$t("Converting to a playable format…")}</div>
    {:else if failed}
      <div class="center err">{failed}</div>
    {:else if waiting && playing}
      <div class="center"><span class="spin"></span></div>
    {:else if !playing}
      <button class="big" onclick={(e) => { e.stopPropagation(); tapVideo(); }} aria-label={$t("Play")}>{@render ico(I.play, true)}</button>
    {/if}

    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="ctl" onclick={(e) => e.stopPropagation()}>
      {@render seekbar()}
      <div class="row">
        <button class="ib main" onclick={toggle} title={$t(playing ? "Pause (K)" : "Play (K)")}>{@render ico(playing ? I.pause : I.play, true)}</button>
        <button class="ib" onclick={() => skip(-10)} title={$t("Back 10 s (J)")}>{@render ico(I.back)}</button>
        <button class="ib" onclick={() => skip(10)} title={$t("Forward 10 s (L)")}>{@render ico(I.fwd)}</button>
        {@render volume_()}
        <span class="time">{fmt(current)} <i>/</i> {fmt(duration)}</span>
        <span class="grow"></span>
        {#if quality}<span class="badge" title={`${vw}×${vh}`}>{quality}</span>{/if}
        {@render speedMenu()}
        <button class="ib" class:on={loop} onclick={setLoop} title={$t("Loop")}>{@render ico(I.loop)}</button>
        <div class="spd">
          <button class="ib" class:on={infoOpen} onclick={() => (infoOpen = !infoOpen)} title={$t("Info")}>{@render ico(I.info)}</button>
          {#if infoOpen}<div class="menu info">{infoLine || $t("Video")}</div>{/if}
        </div>
        {#if onExpand && !theater && !isFs}
          <button class="ib" onclick={onExpand} title={$t("Open in viewer")}>{@render ico(I.expand)}</button>
        {/if}
        <button class="ib" class:on={theater} onclick={() => setTheater(!theater)} title={$t(theater ? "Exit window mode (W)" : "Fill the window (W)")}>{@render ico(I.theater)}</button>
        <button class="ib" onclick={() => void toggleFs()} title={$t(isFs ? "Exit full screen (F)" : "Full screen (F)")}>{@render ico(isFs ? I.fsx : I.fs)}</button>
      </div>
    </div>
  </div>
{:else}
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <div class="ap" tabindex="0" role="group" aria-label={name ?? $t("Audio")} onkeydown={key}>
    <audio
      bind:this={media}
      src={playSrc || undefined}
      preload="metadata"
      {loop}
      onloadedmetadata={onMeta}
      ondurationchange={readDuration}
      ontimeupdate={onTime}
      onprogress={onTime}
      onplay={onPlay}
      onpause={onPause}
      onended={onEnded}
      onwaiting={() => (waiting = true)}
      onplaying={() => (waiting = false)}
      onerror={onError}
    ></audio>
    <button class="ab" onclick={toggle} aria-label={$t(playing ? "Pause" : "Play")}>
      {#if waiting && playing}<span class="spin sm"></span>{:else}{@render ico(playing ? I.pause : I.play, true)}{/if}
    </button>
    <div class="amid">
      {#if kind === "voice" && peaks.length}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div class="wave" onclick={onWaveClick} title={$t("Seek")}>
          {#each peaks as p, i (i)}
            <span class="wb" class:played={(i + 0.5) / peaks.length <= (duration ? current / duration : 0)} style={`height:${Math.round(p * 100)}%`}></span>
          {/each}
        </div>
      {:else}
        {@render seekbar()}
      {/if}
      <div class="arow">
        <span class="time">{fmt(current)} <i>/</i> {fmt(duration)}</span>
        {#if converting}<span class="aerr">⟳ {$t("Converting to a playable format…")}</span>{:else if failed}<span class="aerr" title={failed}>⚠ {failed}</span>{/if}
        <span class="grow"></span>
        {@render speedMenu()}
        {@render volume_()}
      </div>
    </div>
  </div>
{/if}

<style>
  /* ---------------------------------------------------------------- video */
  .vp {
    position: relative;
    display: block;
    width: 100%;
    min-width: 260px;
    border-radius: calc(12px * var(--rk, 1));
    overflow: hidden;
    background: #000;
    outline: none;
    color: #fff;
  }
  .vp video {
    display: block;
    width: 100%;
    max-height: 420px;
    background: #000;
    cursor: pointer;
  }
  .vp.fill { height: 100%; border-radius: 0; }
  .vp.fill video { height: 100%; max-height: none; object-fit: contain; }
  .vp.theater {
    position: fixed; inset: 0; z-index: 10050;
    border-radius: 0;
  }
  .vp.theater video, .vp.fs video { height: 100%; max-height: none; object-fit: contain; }
  .vp.fs { border-radius: 0; width: 100%; height: 100%; }
  /* Phone held upright, wide video: lay the player on its side. */
  .vp.theater.sideways { inset: auto; top: 50%; left: 50%; width: 100vh; height: 100vw; transform: translate(-50%, -50%) rotate(90deg); }
  .vp.hide { cursor: none; }
  .vp.hide .ctl, .vp.hide .top { opacity: 0; pointer-events: none; }

  .top {
    position: absolute; top: 0; left: 0; right: 0;
    padding: 14px 18px 30px;
    font-size: 14px; font-weight: 600;
    background: linear-gradient(rgba(0, 0, 0, 0.6), transparent);
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
    transition: opacity 0.2s;
  }
  .center {
    position: absolute; inset: 0;
    display: grid; place-items: center;
    pointer-events: none;
  }
  .center.err { padding: 20px; text-align: center; font-size: 13px; color: #ffb4b4; background: rgba(0, 0, 0, 0.55); }
  .big {
    position: absolute; top: 50%; left: 50%;
    transform: translate(-50%, -50%);
    width: 62px; height: 62px;
    border: none; border-radius: 50%;
    background: rgba(0, 0, 0, 0.55);
    color: #fff;
    display: grid; place-items: center;
    cursor: pointer;
    box-shadow: 0 0 0 1px rgba(255, 255, 255, 0.18);
  }
  .big :global(svg) { width: 28px; height: 28px; margin-left: 3px; }
  .big:hover { background: var(--accent); color: #06070c; }

  .ctl {
    position: absolute; left: 0; right: 0; bottom: 0;
    padding: 18px 10px 6px;
    background: linear-gradient(transparent, rgba(0, 0, 0, 0.72));
    transition: opacity 0.2s;
  }
  .row { display: flex; align-items: center; gap: 2px; min-width: 0; }
  .grow { flex: 1; min-width: 4px; }
  .ib {
    flex: 0 0 auto;
    width: 32px; height: 32px;
    display: grid; place-items: center;
    border: none; border-radius: calc(8px * var(--rk, 1));
    background: transparent; color: inherit;
    cursor: pointer;
  }
  .ib :global(svg) { width: 19px; height: 19px; }
  .ib:hover { background: rgba(255, 255, 255, 0.14); }
  .ib.on { color: var(--accent); }
  .ib.main :global(svg) { width: 21px; height: 21px; }
  .time { font-size: 12px; font-variant-numeric: tabular-nums; white-space: nowrap; padding: 0 6px; opacity: 0.92; }
  .time i { font-style: normal; opacity: 0.55; }
  .badge {
    font-size: 10.5px; font-weight: 800; letter-spacing: 0.02em;
    padding: 2px 6px; border-radius: calc(5px * var(--rk, 1));
    background: rgba(255, 255, 255, 0.16);
  }
  .txt {
    height: 28px; min-width: 40px; padding: 0 7px;
    border: none; border-radius: calc(7px * var(--rk, 1));
    background: transparent; color: inherit;
    font-size: 12px; font-weight: 800; font-variant-numeric: tabular-nums;
    cursor: pointer;
  }
  .txt:hover { background: rgba(255, 255, 255, 0.14); }
  .txt.on { color: var(--accent); }
  .spd { position: relative; }
  .menu {
    position: absolute; bottom: calc(100% + 6px); right: 0;
    z-index: 3;
    display: flex; flex-direction: column;
    min-width: 76px; padding: 4px;
    border-radius: calc(10px * var(--rk, 1));
    background: rgba(16, 18, 26, 0.96);
    box-shadow: 0 8px 28px rgba(0, 0, 0, 0.5), 0 0 0 1px rgba(255, 255, 255, 0.08);
    color: #fff;
  }
  .menu button {
    border: none; background: transparent; color: inherit;
    text-align: left; padding: 5px 10px; border-radius: calc(7px * var(--rk, 1));
    font-size: 12.5px; font-variant-numeric: tabular-nums; cursor: pointer;
  }
  .menu button:hover { background: rgba(255, 255, 255, 0.1); }
  .menu button.on { color: var(--accent); font-weight: 700; }
  .menu.info { min-width: 220px; max-width: 360px; padding: 8px 10px; font-size: 12px; line-height: 1.45; white-space: normal; overflow-wrap: anywhere; }

  /* seek bar (video + audio) */
  .bar { position: relative; height: 16px; display: flex; align-items: center; cursor: pointer; touch-action: none; }
  .rail { position: relative; flex: 1; height: 4px; border-radius: 3px; background: rgba(255, 255, 255, 0.22); }
  .bar:hover .rail { height: 6px; }
  .buf { position: absolute; inset: 0 auto 0 0; border-radius: 3px; background: rgba(255, 255, 255, 0.32); }
  .fill { position: absolute; inset: 0 auto 0 0; border-radius: 3px; background: var(--accent); }
  .knob {
    position: absolute; top: 50%;
    width: 12px; height: 12px; margin: -6px 0 0 -6px;
    border-radius: 50%; background: var(--accent);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 30%, transparent);
    opacity: 0;
  }
  .bar:hover .knob { opacity: 1; }
  .tip {
    position: absolute; bottom: 18px;
    transform: translateX(-50%);
    padding: 2px 6px; border-radius: calc(5px * var(--rk, 1));
    background: rgba(0, 0, 0, 0.8); color: #fff;
    font-size: 11px; font-variant-numeric: tabular-nums;
    pointer-events: none; white-space: nowrap;
  }

  .vol { display: flex; align-items: center; }
  .vol-slider {
    width: 0; opacity: 0;
    -webkit-appearance: none; appearance: none;
    height: 4px; border-radius: 2px;
    background: linear-gradient(to right, currentColor var(--p), rgba(127, 127, 127, 0.4) var(--p));
    cursor: pointer;
    transition: width 0.15s, opacity 0.15s;
  }
  .vol:hover .vol-slider, .vol:focus-within .vol-slider { width: 70px; opacity: 1; margin: 0 6px 0 2px; }
  .vol-slider::-webkit-slider-thumb { -webkit-appearance: none; width: 11px; height: 11px; border-radius: 50%; background: currentColor; }

  .spin {
    width: 34px; height: 34px; border-radius: 50%;
    border: 3px solid rgba(255, 255, 255, 0.2); border-top-color: var(--accent);
    animation: vp-spin 0.8s linear infinite;
  }
  .spin.sm { width: 16px; height: 16px; border-width: 2px; border-color: rgba(6, 7, 12, 0.3); border-top-color: #06070c; }
  @keyframes vp-spin { to { transform: rotate(360deg); } }

  /* ---------------------------------------------------------------- audio */
  .ap {
    display: flex; align-items: center; gap: 10px;
    min-width: 260px;
    outline: none;
    color: var(--fg);
  }
  .ab {
    flex: 0 0 40px; height: 40px;
    border: none; border-radius: 50%;
    background: linear-gradient(135deg, var(--accent), var(--accent-2));
    color: #06070c;
    display: grid; place-items: center;
    cursor: pointer;
  }
  .ab :global(svg) { width: 19px; height: 19px; }
  .ab:hover { filter: brightness(1.08); }
  .amid { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 2px; }
  .ap .rail { background: var(--border-strong); }
  .ap .buf { background: color-mix(in srgb, var(--fg) 18%, transparent); }
  .arow { display: flex; align-items: center; gap: 2px; min-width: 0; color: var(--fg-dim); }
  .ap .time { padding: 0; font-size: 11.5px; }
  .ap .ib { width: 28px; height: 28px; }
  .ap .ib :global(svg) { width: 17px; height: 17px; }
  .ap .ib:hover, .ap .txt:hover { background: var(--bg-hover); color: var(--fg); }
  .ap .txt { height: 24px; min-width: 34px; font-size: 11px; }
  .ap .menu { background: var(--bg-shell); color: var(--fg); }
  .aerr { font-size: 11px; color: #ff9a9a; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .wave { display: flex; align-items: center; gap: 2px; height: 26px; cursor: pointer; }
  .wb { flex: 1; min-height: 3px; border-radius: 2px; background: var(--border-strong); }
  .wb.played { background: var(--accent); }
</style>
