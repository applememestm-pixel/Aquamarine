<script module lang="ts">
  // ── Waveform peaks ────────────────────────────────────────────────────
  // The <audio> element is NOT routed through WebAudio (a
  // MediaElementSource on a non-CORS asset URL would silently output
  // zeros). Instead the file is fetched and decoded separately, offline.
  const BARS = 64;
  const MAX_BYTES = 40 * 1024 * 1024; // don't decode huge podcasts (PCM blows up ~10x)
  const CACHE_MAX = 24;
  const peakCache = new Map<string, Promise<number[]>>();

  /** Deterministic pseudo-random bars seeded from the title (FNV-1a + mulberry32). */
  function seededBars(seed: string): number[] {
    let h = 0x811c9dc5;
    for (let i = 0; i < seed.length; i++) {
      h ^= seed.charCodeAt(i);
      h = Math.imul(h, 0x01000193);
    }
    let a = h >>> 0;
    const rnd = () => {
      a = (a + 0x6d2b79f5) | 0;
      let t = Math.imul(a ^ (a >>> 15), 1 | a);
      t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
    const raw = Array.from({ length: BARS }, () => rnd());
    // Light smoothing + a soft envelope so it reads like music, not noise.
    return raw.map((v, i) => {
      const prev = raw[i - 1] ?? v;
      const next = raw[i + 1] ?? v;
      const smooth = (prev + 2 * v + next) / 4;
      const env = 0.65 + 0.35 * Math.sin((i / (BARS - 1)) * Math.PI);
      return Math.max(0.12, Math.min(1, (0.25 + 0.75 * smooth) * env));
    });
  }

  async function decodePeaks(src: string): Promise<number[]> {
    const res = await fetch(src);
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    const len = Number(res.headers.get("content-length") || 0);
    if (len > MAX_BYTES) throw new Error("too large");
    const buf = await res.arrayBuffer();
    if (buf.byteLength > MAX_BYTES || buf.byteLength === 0) throw new Error("bad size");

    const w = window as unknown as {
      OfflineAudioContext?: typeof OfflineAudioContext;
      webkitOfflineAudioContext?: typeof OfflineAudioContext;
      AudioContext?: typeof AudioContext;
      webkitAudioContext?: typeof AudioContext;
    };
    let audioBuf: AudioBuffer;
    const Offline = w.OfflineAudioContext ?? w.webkitOfflineAudioContext;
    if (Offline) {
      // Offline context: decodes without opening an output device.
      const ctx = new Offline(1, 1, 44100);
      audioBuf = await ctx.decodeAudioData(buf);
    } else {
      const AC = w.AudioContext ?? w.webkitAudioContext;
      if (!AC) throw new Error("no WebAudio");
      const ctx = new AC();
      try { audioBuf = await ctx.decodeAudioData(buf); }
      finally { ctx.close().catch(() => {}); }
    }

    const chans = Math.min(2, audioBuf.numberOfChannels);
    const data = Array.from({ length: chans }, (_, c) => audioBuf.getChannelData(c));
    const total = audioBuf.length;
    const per = Math.max(1, Math.floor(total / BARS));
    const stride = Math.max(1, Math.floor(per / 3000)); // bound the work per bucket
    const out: number[] = [];
    for (let b = 0; b < BARS; b++) {
      const start = b * per;
      const end = Math.min(total, start + per);
      let peak = 0, sq = 0, n = 0;
      for (let i = start; i < end; i += stride) {
        let s = 0;
        for (let c = 0; c < chans; c++) s += Math.abs(data[c][i]);
        s /= chans;
        if (s > peak) peak = s;
        sq += s * s; n++;
      }
      // Blend peak with RMS — pure peaks look flat on loudness-mastered music.
      const rms = n ? Math.sqrt(sq / n) : 0;
      out.push(peak * 0.35 + rms * 0.65 * 1.6);
    }
    const max = Math.max(...out, 1e-6);
    return out.map((v) => Math.max(0.08, Math.min(1, Math.pow(v / max, 0.9))));
  }

  /** Cached per src; always resolves (falls back to seeded bars). */
  function getPeaks(src: string, title: string): Promise<number[]> {
    const hit = peakCache.get(src);
    if (hit) return hit;
    const p = decodePeaks(src).catch(() => seededBars(title || src));
    peakCache.set(src, p);
    if (peakCache.size > CACHE_MAX) {
      const oldest = peakCache.keys().next().value;
      if (oldest !== undefined) peakCache.delete(oldest);
    }
    return p;
  }
</script>

<script lang="ts">
  // Full-size "now playing" sheet. Drives the ONE persistent <audio>
  // owned by TopPlayer (passed in as `audio`); closing it only hides the
  // sheet — playback continues in the top bar.
  import { onMount } from "svelte";
  import { fade, scale } from "svelte/transition";
  import { t } from "../i18n";
  import { queue, queueIndex, nextTrack, prevTrack, type Track } from "../player";

  interface Props {
    audio: HTMLAudioElement;
    track: Track;
    onclose: () => void;
  }
  let { audio, track, onclose }: Props = $props();

  const SPEEDS = [0.5, 0.75, 1, 1.25, 1.5, 2];

  let playing = $state(false);
  let current = $state(0);
  let duration = $state(0);
  let volume = $state(1);
  let muted = $state(false);
  let rate = $state(1);
  let loop = $state(false);

  // Seek-slider drag: shows the drag time, commits on release.
  let dragging = $state(false);
  let dragTime = $state(0);
  let hoverSeek = $state(false);
  let seekEl = $state<HTMLDivElement | null>(null);
  let sheetEl = $state<HTMLDivElement | null>(null);

  let shown = $derived(dragging ? dragTime : current);
  let frac = $derived(duration > 0 ? Math.max(0, Math.min(1, shown / duration)) : 0);
  let volShown = $derived(muted ? 0 : volume);

  let hasQueue = $derived($queue.length > 1 && $queueIndex >= 0);
  let canNext = $derived(hasQueue && $queueIndex < $queue.length - 1);

  // ── Sync with the shared element ──────────────────────────────────────
  function readDuration() {
    duration = isFinite(audio.duration) ? audio.duration : 0;
  }
  $effect(() => {
    const a = audio;
    playing = !a.paused;
    current = a.currentTime;
    volume = a.volume;
    muted = a.muted;
    rate = a.playbackRate;
    loop = a.loop;
    readDuration();

    const onPlay = () => (playing = true);
    const onPause = () => (playing = false);
    const onTime = () => { current = a.currentTime; };
    const onDur = () => readDuration();
    const onVol = () => { volume = a.volume; muted = a.muted; };
    const onRate = () => { rate = a.playbackRate; };
    const onLoad = () => { current = 0; dragging = false; loop = a.loop; readDuration(); };
    const evs: [string, EventListener][] = [
      ["play", onPlay], ["playing", onPlay], ["pause", onPause], ["ended", onPause],
      ["timeupdate", onTime], ["seeked", onTime],
      ["durationchange", onDur], ["loadedmetadata", onDur],
      ["volumechange", onVol], ["ratechange", onRate], ["loadstart", onLoad],
    ];
    for (const [n, f] of evs) a.addEventListener(n, f);
    return () => { for (const [n, f] of evs) a.removeEventListener(n, f); };
  });

  // Smooth progress while playing (timeupdate only fires ~4x/s).
  $effect(() => {
    if (!playing) return;
    const a = audio;
    let raf = 0;
    const tick = () => { current = a.currentTime; raf = requestAnimationFrame(tick); };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  });

  // ── Waveform ──────────────────────────────────────────────────────────
  let peaks = $state<number[]>([]);
  let peaksReady = $state(false);
  $effect(() => {
    const src = track.src;
    const title = track.title;
    let cancelled = false;
    peaks = seededBars(title || src); // placeholder; morphs into real peaks
    peaksReady = false;
    getPeaks(src, title).then((p) => {
      if (!cancelled) { peaks = p; peaksReady = true; }
    });
    return () => { cancelled = true; };
  });

  function waveSeek(e: MouseEvent) {
    const el = e.currentTarget as HTMLElement;
    const r = el.getBoundingClientRect();
    if (r.width <= 0) return;
    seekTo(((e.clientX - r.left) / r.width) * duration);
  }

  // ── Cover ─────────────────────────────────────────────────────────────
  let coverFailedFor = $state<string | null>(null);
  let showCover = $derived(!!track.cover && coverFailedFor !== track.cover);

  // ── Transport ─────────────────────────────────────────────────────────
  function toggle() {
    if (audio.paused) audio.play().catch(() => {}); else audio.pause();
  }
  function seekTo(s: number) {
    if (!(duration > 0)) return;
    const v = Math.max(0, Math.min(duration, s));
    audio.currentTime = v;
    current = v;
  }
  function skip(d: number) { seekTo(audio.currentTime + d); }
  function prev() {
    // Spotify-style: restart unless we're at the very beginning.
    if (audio.currentTime > 3 || !prevTrack()) seekTo(0);
  }
  function next() { nextTrack(); }
  function setVolume(v: number) {
    const nv = Math.max(0, Math.min(1, Math.round(v * 100) / 100));
    audio.volume = nv;
    audio.muted = nv === 0;
  }
  function toggleMute() {
    if (audio.muted || audio.volume === 0) {
      if (audio.volume === 0) audio.volume = 0.5;
      audio.muted = false;
    } else {
      audio.muted = true;
    }
  }
  function setRate(s: number) {
    audio.defaultPlaybackRate = s;
    audio.playbackRate = s;
  }
  function toggleLoop() {
    audio.loop = !audio.loop;
    loop = audio.loop;
  }

  // ── Seek slider (pointer) ─────────────────────────────────────────────
  function timeAt(clientX: number): number {
    if (!seekEl || !(duration > 0)) return 0;
    const r = seekEl.getBoundingClientRect();
    const f = Math.max(0, Math.min(1, (clientX - r.left) / Math.max(1, r.width)));
    return f * duration;
  }
  function onSeekDown(e: PointerEvent) {
    if (e.button !== 0 || !(duration > 0)) return;
    e.preventDefault();
    seekEl?.setPointerCapture(e.pointerId);
    dragging = true;
    dragTime = timeAt(e.clientX);
  }
  function onSeekMove(e: PointerEvent) {
    if (dragging) dragTime = timeAt(e.clientX);
  }
  function onSeekUp(e: PointerEvent) {
    if (!dragging) return;
    dragTime = timeAt(e.clientX);
    dragging = false;
    seekTo(dragTime);
    try { seekEl?.releasePointerCapture(e.pointerId); } catch { /* ignore */ }
  }
  function onSeekCancel() { dragging = false; }

  // ── Keyboard (capture phase, only while the sheet is open) ────────────
  function isTyping(target: EventTarget | null): boolean {
    const el = target as HTMLElement | null;
    if (!el || !el.tagName) return false;
    if (el.isContentEditable) return true;
    const tag = el.tagName;
    if (tag === "TEXTAREA" || tag === "SELECT") return true;
    if (tag === "INPUT") {
      const type = (el as HTMLInputElement).type;
      return !["range", "checkbox", "radio", "button", "submit"].includes(type);
    }
    return false;
  }
  function onKey(e: KeyboardEvent) {
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    if (isTyping(e.target)) return;
    let handled = true;
    switch (e.code === "Space" ? "Space" : e.key) {
      case "Space": if (!e.repeat) toggle(); break;
      case "ArrowLeft": skip(e.shiftKey ? -15 : -5); break;
      case "ArrowRight": skip(e.shiftKey ? 15 : 5); break;
      case "ArrowUp": setVolume((audio.muted ? 0 : audio.volume) + 0.05); break;
      case "ArrowDown": setVolume((audio.muted ? 0 : audio.volume) - 0.05); break;
      case "Escape": onclose(); break;
      default:
        // Physical key codes so M / L work on any keyboard layout.
        if (e.code === "KeyM") { if (!e.repeat) toggleMute(); }
        else if (e.code === "KeyL") { if (!e.repeat) toggleLoop(); }
        else handled = false;
    }
    if (handled) { e.preventDefault(); e.stopPropagation(); }
  }

  // A focused button would otherwise also "click" on Space keyup.
  function onKeyUp(e: KeyboardEvent) {
    if (e.code !== "Space" || e.ctrlKey || e.metaKey || e.altKey || isTyping(e.target)) return;
    e.preventDefault();
    e.stopPropagation();
  }

  onMount(() => {
    const prevFocus = document.activeElement as HTMLElement | null;
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("keyup", onKeyUp, true);
    sheetEl?.focus({ preventScroll: true });
    return () => {
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("keyup", onKeyUp, true);
      if (prevFocus && prevFocus.isConnected) {
        try { prevFocus.focus({ preventScroll: true }); } catch { /* ignore */ }
      }
    };
  });

  function fmt(s: number): string {
    if (!isFinite(s) || s < 0) s = 0;
    const h = Math.floor(s / 3600);
    const m = Math.floor((s % 3600) / 60);
    const sec = Math.floor(s % 60);
    const ss = String(sec).padStart(2, "0");
    return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${ss}` : `${m}:${ss}`;
  }
</script>

<div class="mp-layer" transition:fade={{ duration: 140 }}>
  <button class="mp-scrim" tabindex="-1" aria-label={$t("Close player")} onclick={onclose}></button>

  <div
    class="mp-sheet"
    role="dialog"
    aria-modal="true"
    aria-label={$t("Music player")}
    tabindex="-1"
    bind:this={sheetEl}
    in:scale={{ start: 0.96, duration: 170 }}
  >
    <div class="mp-cover" class:playing>
      <div class="mp-glow" aria-hidden="true"></div>
      {#if showCover}
        <img src={track.cover} alt="" draggable="false" onerror={() => (coverFailedFor = track.cover ?? null)} />
      {:else}
        <div class="mp-tile" aria-hidden="true">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
            <path d="M9 18V5l12-2v13"/>
            <circle cx="6" cy="18" r="3" fill="currentColor"/>
            <circle cx="18" cy="16" r="3" fill="currentColor"/>
          </svg>
        </div>
      {/if}
    </div>

    <div class="mp-meta">
      {#if hasQueue}
        <div class="mp-pos">{$t("Track {n} of {total}", { n: $queueIndex + 1, total: $queue.length })}</div>
      {/if}
      <div class="mp-title" title={track.title}>{track.title}</div>
      {#if track.sub}<div class="mp-sub" title={track.sub}>{track.sub}</div>{/if}
    </div>

    <button
      class="mp-wave"
      class:ready={peaksReady}
      tabindex="-1"
      aria-label={$t("Seek")}
      onclick={waveSeek}
    >
      {#each peaks as p, i (i)}
        <span class="bar" class:played={(i + 0.5) / peaks.length <= frac} style={`height:${Math.round(p * 100)}%`}></span>
      {/each}
    </button>

    <div class="mp-progress">
      <div
        class="mp-seek"
        class:drag={dragging}
        class:hover={hoverSeek}
        role="slider"
        tabindex="0"
        aria-label={$t("Seek")}
        aria-valuemin={0}
        aria-valuemax={Math.round(duration)}
        aria-valuenow={Math.round(shown)}
        aria-valuetext={`${fmt(shown)} / ${fmt(duration)}`}
        bind:this={seekEl}
        onpointerdown={onSeekDown}
        onpointermove={onSeekMove}
        onpointerup={onSeekUp}
        onpointercancel={onSeekCancel}
        onpointerenter={() => (hoverSeek = true)}
        onpointerleave={() => (hoverSeek = false)}
      >
        <div class="track"><div class="fill" style={`width:${frac * 100}%`}></div></div>
        <div class="thumb" style={`left:${frac * 100}%`}></div>
        {#if dragging}
          <div class="bubble" style={`left:${frac * 100}%`}>{fmt(dragTime)}</div>
        {/if}
      </div>
      <div class="mp-times">
        <span class:hot={dragging}>{fmt(shown)}</span>
        <span>{duration > 0 ? fmt(duration) : "–:––"}</span>
      </div>
    </div>

    <div class="mp-controls">
      {#if hasQueue}
        <button class="mp-btn" onclick={prev} title={$t("Previous")} aria-label={$t("Previous")}>
          <svg viewBox="0 0 24 24" fill="currentColor"><rect x="5" y="5" width="2.4" height="14" rx="1"/><path d="M19 5.5v13a.8.8 0 0 1-1.2.7L8.6 12.7a.8.8 0 0 1 0-1.4l9.2-6.5a.8.8 0 0 1 1.2.7z"/></svg>
        </button>
      {/if}
      <button class="mp-btn" onclick={() => skip(-10)} title={$t("Back 10 seconds")} aria-label={$t("Back 10 seconds")}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
          <path d="M3.5 12a8.5 8.5 0 1 0 2.6-6.1"/><path d="M3 3.5v4.2h4.2"/>
          <text x="12.4" y="15.3" text-anchor="middle" font-size="7.2" font-weight="700" fill="currentColor" stroke="none">10</text>
        </svg>
      </button>

      <button class="mp-play" onclick={toggle} aria-label={playing ? $t("Pause") : $t("Play")}>
        {#if playing}
          <svg viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="5" width="4" height="14" rx="1.2"/><rect x="14" y="5" width="4" height="14" rx="1.2"/></svg>
        {:else}
          <svg viewBox="0 0 24 24" fill="currentColor"><path d="M8.5 5.6v12.8a.9.9 0 0 0 1.4.8l10-6.4a.9.9 0 0 0 0-1.6l-10-6.4a.9.9 0 0 0-1.4.8z"/></svg>
        {/if}
      </button>

      <button class="mp-btn" onclick={() => skip(10)} title={$t("Forward 10 seconds")} aria-label={$t("Forward 10 seconds")}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
          <path d="M20.5 12a8.5 8.5 0 1 1-2.6-6.1"/><path d="M21 3.5v4.2h-4.2"/>
          <text x="11.6" y="15.3" text-anchor="middle" font-size="7.2" font-weight="700" fill="currentColor" stroke="none">10</text>
        </svg>
      </button>
      {#if hasQueue}
        <button class="mp-btn" onclick={next} disabled={!canNext} title={$t("Next")} aria-label={$t("Next")}>
          <svg viewBox="0 0 24 24" fill="currentColor"><rect x="16.6" y="5" width="2.4" height="14" rx="1"/><path d="M5 5.5v13a.8.8 0 0 0 1.2.7l9.2-6.5a.8.8 0 0 0 0-1.4L6.2 4.8A.8.8 0 0 0 5 5.5z"/></svg>
        </button>
      {/if}
    </div>

    <div class="mp-row">
      <button class="mp-icon" onclick={toggleMute} title={muted ? $t("Unmute") : $t("Mute")} aria-label={muted ? $t("Unmute") : $t("Mute")}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M11 5 6 9H3v6h3l5 4z" fill="currentColor" stroke="none"/>
          {#if volShown === 0}
            <path d="m16 9 5 6M21 9l-5 6"/>
          {:else if volShown < 0.5}
            <path d="M15.5 9.5a3.5 3.5 0 0 1 0 5"/>
          {:else}
            <path d="M15.5 9.5a3.5 3.5 0 0 1 0 5"/><path d="M18.5 6.5a7.5 7.5 0 0 1 0 11"/>
          {/if}
        </svg>
      </button>
      <input
        class="mp-vol"
        type="range" min="0" max="1" step="0.01"
        value={volShown}
        style={`--p:${volShown * 100}%`}
        aria-label={$t("Volume")}
        oninput={(e) => setVolume(parseFloat((e.currentTarget as HTMLInputElement).value))}
      />
      <span class="mp-volpct">{Math.round(volShown * 100)}%</span>

      <div class="mp-spacer"></div>

      <button
        class="mp-icon"
        class:on={loop}
        onclick={toggleLoop}
        title={$t("Repeat")}
        aria-label={$t("Repeat")}
        aria-pressed={loop}
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M17 2l3 3-3 3"/><path d="M4 11V9a4 4 0 0 1 4-4h12"/>
          <path d="M7 22l-3-3 3-3"/><path d="M20 13v2a4 4 0 0 1-4 4H4"/>
        </svg>
      </button>
      <button class="mp-icon" onclick={onclose} title={$t("Close player")} aria-label={$t("Close player")}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M6 9l6 6 6-6"/></svg>
      </button>
    </div>

    <div class="mp-speed">
      <span class="mp-speed-label">{$t("Playback speed")}</span>
      <div class="chips" role="radiogroup" aria-label={$t("Playback speed")}>
        {#each SPEEDS as s (s)}
          <button
            class="chip"
            class:on={rate === s}
            role="radio"
            aria-checked={rate === s}
            onclick={() => setRate(s)}
          >{s}×</button>
        {/each}
      </div>
    </div>
  </div>
</div>

<style>
  .mp-layer {
    position: fixed; inset: 0;
    z-index: 190;
    display: grid; place-items: center;
    padding: 16px;
  }
  .mp-scrim {
    position: absolute; inset: 0;
    border: none; padding: 0; margin: 0; cursor: default;
    background: rgba(6, 7, 12, 0.7);
    backdrop-filter: blur(10px);
    -webkit-backdrop-filter: blur(10px);
  }
  .mp-sheet {
    position: relative;
    width: min(520px, 100%);
    max-height: calc(100vh - 32px);
    overflow-y: auto;
    overflow-x: hidden;
    padding: 28px 28px 20px;
    border-radius: calc(22px * var(--rk, 1));
    background:
      radial-gradient(120% 60% at 50% 0%, var(--accent-soft), transparent 60%),
      var(--bg-shell);
    border: 1px solid var(--border-strong);
    box-shadow: 0 30px 80px -20px rgba(0, 0, 0, 0.7), 0 0 0 1px var(--accent-soft);
    display: flex; flex-direction: column; align-items: stretch; gap: 16px;
    outline: none;
    color: var(--fg);
  }

  /* ── Cover ── */
  .mp-cover {
    position: relative;
    align-self: center;
    width: min(300px, 100%, 38vh);
    aspect-ratio: 1;
    flex-shrink: 0;
    margin: 4px 0 6px;
  }
  .mp-glow {
    position: absolute; inset: -9%;
    border-radius: 50%;
    background: conic-gradient(from 0deg, var(--accent), var(--accent-2), var(--accent), var(--accent-2), var(--accent));
    filter: blur(28px);
    opacity: 0.22;
    animation: mp-spin 10s linear infinite;
    animation-play-state: paused;
    transition: opacity 0.5s ease;
    pointer-events: none;
  }
  .mp-cover.playing .mp-glow { opacity: 0.55; animation-play-state: running; }
  @keyframes mp-spin { to { transform: rotate(360deg); } }

  .mp-cover img, .mp-tile {
    position: relative;
    width: 100%; height: 100%;
    border-radius: calc(18px * var(--rk, 1));
    border: 1px solid var(--border-strong);
    object-fit: cover;
    display: block;
    user-select: none;
  }
  .mp-tile {
    display: grid; place-items: center;
    background:
      radial-gradient(circle at 28% 22%, rgba(255, 255, 255, 0.16), transparent 42%),
      radial-gradient(circle at 80% 85%, var(--accent-glow), transparent 55%),
      linear-gradient(140deg, var(--accent-2) 0%, var(--bg-surface) 58%, var(--accent) 135%);
    color: rgba(255, 255, 255, 0.92);
    overflow: hidden;
  }
  .mp-tile svg {
    width: 34%; height: 34%;
    filter: drop-shadow(0 0 14px var(--accent-glow));
  }

  /* ── Meta ── */
  .mp-meta { text-align: center; min-width: 0; }
  .mp-pos {
    font-size: 10.5px; font-weight: 700; letter-spacing: 0.08em; text-transform: uppercase;
    color: var(--accent); margin-bottom: 4px;
  }
  .mp-title {
    font-size: 19px; font-weight: 700; line-height: 1.25;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .mp-sub {
    margin-top: 3px; font-size: 13px; color: var(--fg-muted);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }

  /* ── Waveform ── */
  .mp-wave {
    display: flex; align-items: center; gap: 2px;
    height: 44px; width: 100%;
    padding: 0; margin: 0;
    background: transparent; border: none; cursor: pointer;
  }
  .mp-wave .bar {
    flex: 1 1 0; min-width: 0;
    min-height: 3px;
    border-radius: 2px;
    background: var(--fg-dim);
    opacity: 0.35;
    transition: height 0.45s cubic-bezier(.2, .8, .2, 1), background-color 0.15s, opacity 0.15s;
  }
  .mp-wave:not(.ready) .bar { opacity: 0.18; }
  .mp-wave .bar.played {
    background: var(--accent);
    opacity: 1;
    box-shadow: 0 0 6px -1px var(--accent-glow);
  }
  .mp-wave:hover .bar:not(.played) { opacity: 0.55; }

  /* ── Seek slider ── */
  .mp-progress { display: flex; flex-direction: column; gap: 6px; }
  .mp-seek {
    position: relative;
    height: 16px;
    cursor: pointer;
    touch-action: none;
    outline: none;
  }
  .mp-seek .track {
    position: absolute; left: 0; right: 0; top: 50%;
    height: 4px; margin-top: -2px;
    border-radius: 2px;
    background: var(--border-strong);
    overflow: hidden;
  }
  .mp-seek .fill {
    height: 100%;
    background: var(--fg);
    border-radius: 2px;
  }
  .mp-seek.hover .fill, .mp-seek.drag .fill, .mp-seek:focus-visible .fill { background: var(--accent); }
  .mp-seek .thumb {
    position: absolute; top: 50%;
    width: 13px; height: 13px; margin: -6.5px 0 0 -6.5px;
    border-radius: 50%;
    background: var(--fg);
    box-shadow: 0 0 10px var(--accent-glow);
    transform: scale(0);
    transition: transform 0.12s ease;
    pointer-events: none;
  }
  .mp-seek.hover .thumb, .mp-seek.drag .thumb, .mp-seek:focus-visible .thumb { transform: scale(1); }
  .mp-seek .bubble {
    position: absolute; bottom: calc(100% + 6px);
    transform: translateX(-50%);
    padding: 3px 7px; border-radius: calc(6px * var(--rk, 1));
    background: var(--bg-surface); border: 1px solid var(--border-strong);
    font-size: 11px; font-weight: 600; font-variant-numeric: tabular-nums;
    color: var(--accent);
    white-space: nowrap; pointer-events: none;
  }
  .mp-times {
    display: flex; justify-content: space-between;
    font-size: 11.5px; color: var(--fg-dim);
    font-variant-numeric: tabular-nums;
  }
  .mp-times .hot { color: var(--accent); }

  /* ── Transport ── */
  .mp-controls {
    display: flex; align-items: center; justify-content: center;
    gap: 18px;
    margin: 2px 0 4px;
  }
  .mp-btn {
    width: 42px; height: 42px; padding: 0;
    display: grid; place-items: center;
    border-radius: 50%;
    border: none; background: transparent;
    color: var(--fg-muted); cursor: pointer;
    transition: color 0.12s, background-color 0.12s, transform 0.12s;
  }
  .mp-btn svg { width: 24px; height: 24px; }
  .mp-btn:hover:not(:disabled) { color: var(--fg); background: var(--bg-hover); }
  .mp-btn:active:not(:disabled) { transform: scale(0.94); }
  .mp-btn:disabled { opacity: 0.35; cursor: default; }
  .mp-play {
    width: 64px; height: 64px; padding: 0;
    display: grid; place-items: center;
    border-radius: 50%;
    border: none;
    background: linear-gradient(135deg, var(--accent), var(--accent-2));
    color: #06070C;
    cursor: pointer;
    box-shadow: 0 0 0 1px var(--accent-soft), 0 8px 28px -6px var(--accent-glow), 0 0 22px -4px var(--accent-glow);
    transition: transform 0.12s ease, box-shadow 0.2s ease;
  }
  .mp-play svg { width: 28px; height: 28px; }
  .mp-play:hover { transform: scale(1.05); box-shadow: 0 0 0 1px var(--accent-soft), 0 10px 34px -6px var(--accent-glow), 0 0 30px -2px var(--accent-glow); }
  .mp-play:active { transform: scale(0.97); }

  button:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }

  /* ── Secondary row ── */
  .mp-row {
    display: flex; align-items: center; gap: 8px;
    padding-top: 12px;
    border-top: 1px solid var(--border);
  }
  .mp-spacer { flex: 1; }
  .mp-icon {
    width: 32px; height: 32px; padding: 0;
    display: grid; place-items: center;
    flex-shrink: 0;
    border-radius: calc(8px * var(--rk, 1));
    border: 1px solid transparent; background: transparent;
    color: var(--fg-muted); cursor: pointer;
  }
  .mp-icon svg { width: 17px; height: 17px; }
  .mp-icon:hover { color: var(--fg); background: var(--bg-hover); }
  .mp-icon.on { color: var(--accent); background: var(--accent-soft); border-color: var(--accent-soft); }
  .mp-vol {
    width: 130px; min-width: 60px; flex-shrink: 1;
    -webkit-appearance: none; appearance: none;
    height: 4px; border-radius: 2px;
    background: linear-gradient(to right, var(--accent) 0%, var(--accent) var(--p, 0%), var(--border-strong) var(--p, 0%), var(--border-strong) 100%);
    cursor: pointer;
  }
  .mp-vol::-webkit-slider-thumb {
    -webkit-appearance: none;
    width: 12px; height: 12px; border-radius: 50%;
    background: var(--fg);
    box-shadow: 0 0 6px var(--accent-glow);
  }
  .mp-vol::-moz-range-thumb {
    width: 12px; height: 12px; border: none; border-radius: 50%;
    background: var(--fg);
  }
  .mp-volpct {
    width: 34px; font-size: 11px; color: var(--fg-dim);
    font-variant-numeric: tabular-nums;
  }

  /* ── Speed chips ── */
  .mp-speed {
    display: flex; align-items: center; justify-content: space-between;
    gap: 10px; flex-wrap: wrap;
  }
  .mp-speed-label { font-size: 11.5px; color: var(--fg-dim); }
  .chips {
    display: flex; gap: 4px; flex-wrap: wrap;
    padding: 3px;
    border-radius: calc(10px * var(--rk, 1));
    background: var(--bg-input);
    border: 1px solid var(--border);
  }
  .chip {
    min-width: 40px;
    padding: 4px 7px;
    border-radius: calc(7px * var(--rk, 1));
    border: 1px solid transparent;
    background: transparent;
    color: var(--fg-muted);
    font-size: 11.5px; font-weight: 700;
    font-variant-numeric: tabular-nums;
    cursor: pointer;
  }
  .chip:hover { color: var(--fg); background: var(--bg-hover); }
  .chip.on {
    color: var(--accent);
    background: var(--accent-soft);
    border-color: var(--accent-soft);
  }

  @media (max-width: 480px) {
    .mp-sheet { padding: 20px 16px 16px; gap: 14px; }
    .mp-controls { gap: 10px; }
    .mp-volpct { display: none; }
  }
  @media (prefers-reduced-motion: reduce) {
    .mp-glow { animation: none; }
    .mp-wave .bar { transition: none; }
  }
</style>
