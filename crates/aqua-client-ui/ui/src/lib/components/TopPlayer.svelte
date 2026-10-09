<script lang="ts">
  // Persistent background audio bar. Mounted once in App.svelte, so the
  // single <audio> element survives chat/tab switches and keeps playing.
  // The expanded MusicPlayer sheet is driven by this SAME element (passed
  // as a prop) — there is never a second <audio>.
  import {
    nowPlaying, stopTrack, nextTrack, playerExpanded, expandPlayer,
  } from "../player";
  import { t } from "../i18n";
  import MusicPlayer from "./MusicPlayer.svelte";
  import { untrack } from "svelte";

  const SPEEDS = [0.5, 0.75, 1, 1.25, 1.5, 2];
  const VOL_KEY = "aqua.player.volume";
  const SPEED_KEY = "aqua.player.speed";

  function loadNum(key: string, fallback: number, ok: (n: number) => boolean): number {
    try {
      const v = localStorage.getItem(key);
      if (v != null) {
        const n = parseFloat(v);
        if (isFinite(n) && ok(n)) return n;
      }
    } catch { /* storage unavailable */ }
    return fallback;
  }
  function saveNum(key: string, n: number) {
    try { localStorage.setItem(key, String(n)); } catch { /* ignore */ }
  }

  let audio = $state<HTMLAudioElement | null>(null);
  let playing = $state(false);
  let current = $state(0);
  let duration = $state(0);
  let volume = $state(loadNum(VOL_KEY, 1, (n) => n >= 0 && n <= 1));
  let muted = $state(false);
  let seeking = $state(false);
  let speed = $state(loadNum(SPEED_KEY, 1, (n) => SPEEDS.includes(n)));

  function cycleSpeed() {
    const i = SPEEDS.indexOf(speed);
    const next = SPEEDS[(i + 1) % SPEEDS.length];
    if (audio) { audio.defaultPlaybackRate = next; audio.playbackRate = next; }
    else { speed = next; saveNum(SPEED_KEY, next); }
  }

  // Apply persisted volume/speed whenever a (new) element gets bound.
  $effect(() => {
    const a = audio;
    if (!a) return;
    untrack(() => {
      a.volume = volume;
      a.muted = muted;
      a.defaultPlaybackRate = speed;
      a.playbackRate = speed;
    });
  });

  // When the track changes, load + autoplay. Identity (not src) is compared
  // so re-playing the same file / a duplicate queue entry restarts it.
  let lastTrack: object | null = null;
  let lastSrc = "";
  $effect(() => {
    const tr = $nowPlaying;
    if (!tr) { lastTrack = null; lastSrc = ""; return; }
    if (audio && tr !== lastTrack) {
      const sameSrc = tr.src === lastSrc;
      lastTrack = tr;
      lastSrc = tr.src;
      current = 0;
      // Defer so the <audio src> binding has updated.
      queueMicrotask(() => {
        if (!audio) return;
        if (sameSrc) { try { audio.currentTime = 0; } catch { /* ignore */ } }
        audio.defaultPlaybackRate = speed;
        audio.playbackRate = speed;
        audio.play().catch(() => { /* autoplay blocked — user can press play */ });
      });
    }
  });

  function toggle() {
    if (!audio) return;
    if (audio.paused) audio.play().catch(() => {}); else audio.pause();
  }
  function onTime() { if (audio && !seeking) current = audio.currentTime; }
  function onMeta() { if (audio) duration = isFinite(audio.duration) ? audio.duration : 0; }
  function onEnded() {
    // Queue: roll into the next track; otherwise rest at the start.
    if (nextTrack()) return;
    playing = false;
    current = 0;
  }
  // Both this bar and the expanded sheet change the element directly; these
  // events keep the bar's view (and persisted prefs) in sync either way.
  function onVolumeChange() {
    if (!audio) return;
    volume = audio.volume;
    muted = audio.muted;
    saveNum(VOL_KEY, audio.volume);
  }
  function onRateChange() {
    if (!audio) return;
    const r = audio.playbackRate;
    // A new src resets playbackRate to defaultPlaybackRate; ignore oddities.
    if (!SPEEDS.includes(r)) return;
    speed = r;
    saveNum(SPEED_KEY, r);
  }
  function seek(e: Event) {
    const v = parseFloat((e.target as HTMLInputElement).value);
    current = v;
    if (audio) audio.currentTime = v;
  }
  function setVol(e: Event) {
    const v = parseFloat((e.target as HTMLInputElement).value);
    if (!audio) { volume = v; return; }
    audio.volume = v;
    audio.muted = v === 0;
  }
  function toggleMute() {
    if (!audio) return;
    if (audio.muted || audio.volume === 0) {
      if (audio.volume === 0) audio.volume = 0.5;
      audio.muted = false;
    } else {
      audio.muted = true;
    }
  }

  function fmt(s: number): string {
    if (!isFinite(s) || s < 0) s = 0;
    const m = Math.floor(s / 60);
    const sec = Math.floor(s % 60);
    return `${m}:${String(sec).padStart(2, "0")}`;
  }
  let pct = $derived(duration > 0 ? (current / duration) * 100 : 0);
</script>

{#if $nowPlaying}
  <div class="top-player">
    <audio
      bind:this={audio}
      src={$nowPlaying.src}
      preload="metadata"
      ontimeupdate={onTime}
      onloadedmetadata={onMeta}
      ondurationchange={onMeta}
      onplay={() => (playing = true)}
      onpause={() => (playing = false)}
      onended={onEnded}
      onvolumechange={onVolumeChange}
      onratechange={onRateChange}
    ></audio>

    <button class="tp-play" onclick={toggle} aria-label={playing ? $t("Pause") : $t("Play")}>
      {#if playing}
        <svg viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="5" width="4" height="14" rx="1"/><rect x="14" y="5" width="4" height="14" rx="1"/></svg>
      {:else}
        <svg viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg>
      {/if}
    </button>

    <button class="tp-info" onclick={expandPlayer} title={$t("Open player")}>
      <span class="tp-title">{$nowPlaying.title}</span>
      {#if $nowPlaying.sub}<span class="tp-sub">{$nowPlaying.sub}</span>{/if}
    </button>

    <span class="tp-time">{fmt(current)}</span>
    <input
      class="tp-seek"
      type="range" min="0" max={duration || 0} step="0.1"
      value={current}
      style={`--p:${pct}%`}
      aria-label={$t("Seek")}
      oninput={seek}
      onpointerdown={() => (seeking = true)}
      onpointerup={() => (seeking = false)}
    />
    <span class="tp-time dur">{fmt(duration)}</span>

    <button class="tp-speed" class:on={speed !== 1} onclick={cycleSpeed} title={$t("Playback speed")}>{speed}×</button>
    <button class="tp-vol" onclick={toggleMute} aria-label={muted ? $t("Unmute") : $t("Mute")}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <path d="M11 5 6 9H3v6h3l5 4z" fill="currentColor" stroke="none"/>
        {#if muted || volume === 0}
          <path d="m16 9 5 6M21 9l-5 6"/>
        {:else if volume < 0.5}
          <path d="M15.5 9.5a3.5 3.5 0 0 1 0 5"/>
        {:else}
          <path d="M15.5 9.5a3.5 3.5 0 0 1 0 5"/><path d="M18.5 6.5a7.5 7.5 0 0 1 0 11"/>
        {/if}
      </svg>
    </button>
    <input
      class="tp-vol-slider" type="range" min="0" max="1" step="0.05"
      value={muted ? 0 : volume}
      style={`--p:${(muted ? 0 : volume) * 100}%`}
      aria-label={$t("Volume")}
      oninput={setVol}
    />

    <button class="tp-icon" onclick={expandPlayer} title={$t("Open player")} aria-label={$t("Open player")}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M15 3h6v6M9 21H3v-6M21 3l-7 7M3 21l7-7"/></svg>
    </button>
    <button class="tp-icon" onclick={stopTrack} title={$t("Stop")} aria-label={$t("Stop")}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M6 6l12 12M18 6 6 18"/></svg>
    </button>
  </div>

  <!-- Outside .top-player so its z-index:50 stacking context doesn't cap the sheet. -->
  {#if $playerExpanded && audio}
    <MusicPlayer {audio} track={$nowPlaying} onclose={() => playerExpanded.set(false)} />
  {/if}
{/if}

<style>
  .top-player {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 48px;
    padding: 0 14px;
    background: var(--bg-rail);
    border-bottom: 1px solid var(--border-strong);
    flex-shrink: 0;
    position: relative;
    z-index: 50;
  }
  .tp-play {
    flex-shrink: 0;
    width: 32px; height: 32px;
    border-radius: 50%;
    border: none;
    background: linear-gradient(135deg, var(--accent), var(--accent-2));
    color: #06070C;
    cursor: pointer;
    display: grid; place-items: center;
  }
  .tp-play svg { width: 17px; height: 17px; }
  .tp-play:hover { transform: scale(1.05); }

  .tp-info {
    min-width: 120px; max-width: 220px; flex-shrink: 0;
    display: flex; flex-direction: column; align-items: flex-start;
    background: transparent; border: none; padding: 2px 6px; margin: 0 -6px;
    border-radius: 6px; cursor: pointer; text-align: left; font: inherit;
  }
  .tp-info:hover { background: var(--bg-hover); }
  .tp-info:hover .tp-title { color: var(--accent); }
  .tp-title {
    max-width: 100%;
    font-size: 12.5px; font-weight: 600; color: var(--fg);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .tp-sub {
    max-width: 100%;
    font-size: 10.5px; color: var(--fg-dim);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }

  .tp-time { font-size: 11px; color: var(--fg-dim); font-variant-numeric: tabular-nums; flex-shrink: 0; min-width: 30px; }
  .tp-time.dur { text-align: right; }
  .tp-seek {
    flex: 1;
    -webkit-appearance: none; appearance: none;
    height: 5px; border-radius: 3px;
    background: linear-gradient(to right, var(--accent) 0%, var(--accent) var(--p, 0%), var(--border-strong) var(--p, 0%), var(--border-strong) 100%);
    cursor: pointer; min-width: 60px;
  }
  .tp-seek::-webkit-slider-thumb {
    -webkit-appearance: none;
    width: 13px; height: 13px; border-radius: 50%;
    background: var(--accent); box-shadow: 0 0 6px -1px var(--accent-glow); cursor: pointer;
  }
  .tp-speed {
    flex-shrink: 0; min-width: 38px;
    padding: 2px 6px; border-radius: 6px;
    border: 1px solid var(--border); background: transparent;
    color: var(--fg-dim); font-size: 11px; font-weight: 700;
    font-variant-numeric: tabular-nums; cursor: pointer;
  }
  .tp-speed:hover { color: var(--fg); border-color: var(--border-strong); }
  .tp-speed.on { color: var(--accent); border-color: var(--accent-soft); background: var(--accent-soft); }
  .tp-vol {
    flex-shrink: 0; background: transparent; border: none; cursor: pointer;
    width: 24px; height: 24px; padding: 0; display: grid; place-items: center;
    color: var(--fg-muted);
  }
  .tp-vol:hover { color: var(--fg); }
  .tp-vol svg { width: 17px; height: 17px; }
  .tp-vol-slider {
    width: 54px; -webkit-appearance: none; appearance: none;
    height: 4px; border-radius: 2px;
    background: linear-gradient(to right, var(--fg-muted) 0%, var(--fg-muted) var(--p, 0%), var(--border-strong) var(--p, 0%), var(--border-strong) 100%);
    cursor: pointer; flex-shrink: 0;
  }
  .tp-vol-slider::-webkit-slider-thumb {
    -webkit-appearance: none; width: 11px; height: 11px; border-radius: 50%;
    background: var(--fg-muted); cursor: pointer;
  }
  .tp-icon {
    flex-shrink: 0;
    width: 28px; height: 28px; padding: 0;
    display: grid; place-items: center;
    background: transparent; border: 1px solid var(--border);
    border-radius: 8px; color: var(--fg-dim); cursor: pointer;
  }
  .tp-icon svg { width: 14px; height: 14px; }
  .tp-icon:hover { color: var(--fg); background: var(--bg-hover); border-color: var(--border-strong); }

  @media (max-width: 720px) {
    .tp-info { display: none; }
    .tp-vol-slider { display: none; }
  }
</style>
