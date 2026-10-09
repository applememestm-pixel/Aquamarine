<script lang="ts">
  import { t } from "../i18n";
  // Custom audio / video player with our own controls, so none of the
  // Chromium native-player chrome (the ⋮ overflow with Download /
  // Picture-in-Picture / playback-speed) ever shows.
  //
  // Props:
  //   src   — media URL (data: or blob:)
  //   kind  — "audio" | "voice" | "video"
  //   onExpand — optional: called when the user clicks the video to
  //              open it in the fullscreen lightbox.
  let { src, kind = "audio", onExpand = undefined }:
    { src: string; kind?: "audio" | "voice" | "video"; onExpand?: (() => void) | undefined } = $props();

  let media = $state<HTMLMediaElement | null>(null);
  let playing = $state(false);
  let current = $state(0);
  let duration = $state(0);
  let volume = $state(1);
  let muted = $state(false);
  let seeking = $state(false);

  // Playback speed — cycles through the usual messenger set. Ported
  // from the old app's advanced audio/video players.
  const SPEEDS = [1, 1.25, 1.5, 1.75, 2];
  let speedIdx = $state(0);
  let speed = $derived(SPEEDS[speedIdx]);
  function cycleSpeed() {
    speedIdx = (speedIdx + 1) % SPEEDS.length;
    if (media) media.playbackRate = SPEEDS[speedIdx];
  }

  function toggle() {
    if (!media) return;
    if (media.paused) media.play(); else media.pause();
  }
  function onTime() { if (media && !seeking) current = media.currentTime; }
  function onMeta() { if (media) { duration = media.duration || 0; media.playbackRate = SPEEDS[speedIdx]; } }
  function onPlay() { playing = true; }
  function onPause() { playing = false; }
  function onEnded() { playing = false; current = 0; }

  function seek(e: Event) {
    const v = parseFloat((e.target as HTMLInputElement).value);
    current = v;
    if (media) media.currentTime = v;
  }
  function setVol(e: Event) {
    volume = parseFloat((e.target as HTMLInputElement).value);
    if (media) { media.volume = volume; media.muted = volume === 0; }
    muted = volume === 0;
  }
  function toggleMute() {
    if (!media) return;
    muted = !muted;
    media.muted = muted;
  }

  function fmt(s: number): string {
    if (!isFinite(s) || s < 0) s = 0;
    const m = Math.floor(s / 60);
    const sec = Math.floor(s % 60);
    return `${m}:${String(sec).padStart(2, "0")}`;
  }

  let pct = $derived(duration > 0 ? (current / duration) * 100 : 0);

  // ---- Waveform (ported from the old messenger's WaveformPanel) ----
  // For voice notes we decode the audio once and draw ~48 amplitude bars,
  // with a click-to-seek track. Falls back to the range slider on failure.
  const BARS = 48;
  let peaks = $state<number[]>([]);
  let decodedFor = "";
  $effect(() => {
    if (kind !== "voice" || !src || decodedFor === src) return;
    decodedFor = src;
    peaks = [];
    void computePeaks(src);
  });
  async function computePeaks(url: string) {
    try {
      const resp = await fetch(url);
      const buf = await resp.arrayBuffer();
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
      peaks = out.map((v) => Math.max(0.08, v / max)); // normalise 0.08–1
      try { await ctx.close(); } catch { /* ignore */ }
    } catch {
      peaks = []; // keep the slider fallback
    }
  }
  function seekToFraction(frac: number) {
    if (!media || !duration) return;
    const t = Math.max(0, Math.min(duration, frac * duration));
    current = t;
    media.currentTime = t;
  }
  function onWaveClick(e: MouseEvent) {
    const el = e.currentTarget as HTMLElement;
    const r = el.getBoundingClientRect();
    seekToFraction((e.clientX - r.left) / r.width);
  }
</script>

<div class="player" class:video={kind === "video"}>
  {#if kind === "video"}
    <!-- svelte-ignore a11y_media_has_caption -->
    <video
      bind:this={media}
      {src}
      class="video-el"
      preload="metadata"
      ontimeupdate={onTime}
      onloadedmetadata={onMeta}
      onplay={onPlay}
      onpause={onPause}
      onended={onEnded}
      onclick={toggle}
    ></video>
    {#if onExpand}
      <button class="expand" onclick={onExpand} title={$t("Fullscreen")}>⤢</button>
    {/if}
    {#if !playing}
      <button class="big-play" onclick={toggle} aria-label={$t("Play")}>▶</button>
    {/if}
  {:else}
    <audio
      bind:this={media}
      {src}
      preload="metadata"
      ontimeupdate={onTime}
      onloadedmetadata={onMeta}
      onplay={onPlay}
      onpause={onPause}
      onended={onEnded}
    ></audio>
    <div class="wave-icon">{$t(kind === "voice" ? "🎙" : "🎵")}</div>
  {/if}

  <div class="controls">
    <button class="play" onclick={toggle} aria-label={$t(playing ? "Pause" : "Play")}>
      {#if playing}
        <svg viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="5" width="4" height="14" rx="1"/><rect x="14" y="5" width="4" height="14" rx="1"/></svg>
      {:else}
        <svg viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg>
      {/if}
    </button>
    <span class="time">{fmt(current)}</span>
    {#if kind === "voice" && peaks.length > 0}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <div class="wave" onclick={onWaveClick} title={$t("Seek")}>
        {#each peaks as p, i}
          <span class="wave-bar" class:played={(i + 0.5) / peaks.length <= (duration ? current / duration : 0)} style={`height:${Math.round(p * 100)}%`}></span>
        {/each}
      </div>
    {:else}
      <input
        class="seek"
        type="range" min="0" max={duration || 0} step="0.1"
        value={current}
        style={`--p:${pct}%`}
        oninput={seek}
        onpointerdown={() => (seeking = true)}
        onpointerup={() => (seeking = false)}
      />
    {/if}
    <span class="time dur">{fmt(duration)}</span>
    <button class="speed" class:on={speed !== 1} onclick={cycleSpeed} title={$t("Playback speed")}>
      {speed}×
    </button>
    <button class="vol" onclick={toggleMute} aria-label={$t("Mute")}>
      {#if muted || volume === 0}🔇{:else if volume < 0.5}🔉{:else}🔊{/if}
    </button>
    <input class="vol-slider" type="range" min="0" max="1" step="0.05" value={volume} oninput={setVol} />
  </div>
</div>

<style>
  .player {
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-width: 260px;
  }
  .player.video { min-width: 280px; }

  .video-el {
    width: 100%;
    max-height: 360px;
    border-radius: 10px;
    background: #000;
    display: block;
    cursor: pointer;
  }
  .player.video { position: relative; }
  .expand {
    position: absolute; top: 8px; right: 8px;
    width: 30px; height: 30px;
    border: none; border-radius: 8px;
    background: rgba(0,0,0,0.5); color: #fff;
    font-size: 15px; cursor: pointer;
    backdrop-filter: blur(4px);
  }
  .expand:hover { background: rgba(0,0,0,0.75); }
  .big-play {
    position: absolute; top: 50%; left: 50%;
    transform: translate(-50%, -50%);
    width: 56px; height: 56px;
    border: none; border-radius: 50%;
    background: rgba(0,0,0,0.55); color: #fff;
    font-size: 20px; cursor: pointer;
    display: grid; place-items: center;
    backdrop-filter: blur(4px);
    padding-left: 4px;
  }
  .big-play:hover { background: var(--accent); color: #06070C; }

  .wave-icon {
    display: none; /* icon lives in the parent bubble already */
  }

  .controls {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .play {
    flex-shrink: 0;
    width: 34px; height: 34px;
    border-radius: 50%;
    border: none;
    background: linear-gradient(135deg, var(--accent), var(--accent-2));
    color: #06070C;
    cursor: pointer;
    display: grid; place-items: center;
  }
  .play svg { width: 18px; height: 18px; }
  .play:hover { transform: scale(1.05); }
  .time { font-size: 11px; color: var(--fg-dim); font-variant-numeric: tabular-nums; flex-shrink: 0; min-width: 30px; }
  .time.dur { text-align: right; }

  .wave {
    flex: 1;
    display: flex; align-items: center; gap: 2px;
    height: 28px; min-width: 0; cursor: pointer;
  }
  .wave-bar {
    flex: 1;
    min-height: 3px;
    border-radius: 2px;
    background: var(--border-strong);
    transition: background 0.1s ease;
  }
  .wave-bar.played { background: var(--accent); }

  .seek {
    flex: 1;
    -webkit-appearance: none;
    appearance: none;
    height: 5px;
    border-radius: 3px;
    background: linear-gradient(to right, var(--accent) 0%, var(--accent) var(--p, 0%), var(--border-strong) var(--p, 0%), var(--border-strong) 100%);
    cursor: pointer;
    min-width: 0;
  }
  .seek::-webkit-slider-thumb {
    -webkit-appearance: none;
    width: 13px; height: 13px;
    border-radius: 50%;
    background: var(--accent);
    box-shadow: 0 0 6px -1px var(--accent-glow);
    cursor: pointer;
  }

  .speed {
    flex-shrink: 0;
    min-width: 34px;
    padding: 2px 6px;
    border-radius: 6px;
    border: 1px solid var(--border);
    background: transparent;
    color: var(--fg-dim);
    font-size: 11px;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
    cursor: pointer;
  }
  .speed:hover { color: var(--fg); border-color: var(--border-strong); }
  .speed.on {
    color: var(--accent);
    border-color: var(--accent-soft);
    background: var(--accent-soft);
  }

  .vol {
    flex-shrink: 0;
    background: transparent; border: none;
    font-size: 14px; cursor: pointer;
    width: 24px;
  }
  .vol-slider {
    width: 54px;
    -webkit-appearance: none; appearance: none;
    height: 4px; border-radius: 2px;
    background: var(--border-strong);
    cursor: pointer;
    flex-shrink: 0;
  }
  .vol-slider::-webkit-slider-thumb {
    -webkit-appearance: none;
    width: 11px; height: 11px; border-radius: 50%;
    background: var(--fg-muted); cursor: pointer;
  }
</style>
