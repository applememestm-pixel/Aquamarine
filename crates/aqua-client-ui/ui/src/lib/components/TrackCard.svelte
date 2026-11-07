<script lang="ts">
  /**
   * A track from Navidrome in a chat (`[track:v1]{…}`, lib/music.ts). It
   * plays for whoever has that Navidrome (the same server's, or the same
   * own one); everyone else sees the card without sound. With `lt` it is a
   * "listen together" invite from that person.
   */
  import { t, tr } from "../i18n";
  import { showToast } from "./Toast.svelte";
  import { fmtDuration } from "../media";
  import { nowPlaying } from "../player";
  import {
    musicStatus, musicOn, cardSource, cardSong, coverUrl, playSongs, downloadSong,
    following, hosting, joinListening, leaveListening, type TrackCard,
  } from "../music";
  import { info } from "../stores";
  import { settings } from "../theme";

  let { card, outgoing = false, chat = "", from = "" }: { card: TrackCard; outgoing?: boolean; chat?: string; from?: string } = $props();

  const src = $derived(cardSource(card, $musicStatus));
  const playing = $derived(!!$nowPlaying?.nd && $nowPlaying.nd.id === card.id && $nowPlaying.nd.src === src);
  const me = $derived($info?.username ?? "");
  const invite = $derived(!!card.lt && !outgoing && card.lt !== me);
  const joined = $derived(!!$following && $following.chat === chat && $following.host === card.lt);

  let cover = $state("");
  $effect(() => {
    cover = "";
    const s = src;
    if (s && card.cover) void coverUrl(s, card.cover, 300).then((u) => { if (src === s) cover = u; });
  });

  async function play() {
    if (!src) return;
    try { await playSongs([cardSong(card, src)], 0); } catch (e) { showToast(String(e)); }
  }
  async function save() {
    if (!src) return;
    showToast(tr("Downloading…"));
    try { const p = await downloadSong({ src, id: card.id, title: card.title, artist: card.artist }); showToast(tr("Saved: {p}", { p }), 6000); }
    catch (e) { showToast(String(e)); }
  }
  function join() {
    if (!src) { showToast(tr("You don't have access to this Navidrome")); return; }
    if (!card.lt) return;
    if ($hosting) { showToast(tr("You're hosting listening together yourself")); return; }
    joinListening(chat, card.lt);
    showToast(tr("Listening together with @{h}", { h: card.lt }));
  }
</script>

<div class="track" class:out={outgoing} class:mute={!src}>
  <div class="art" style:background-image={cover ? `url("${cover}")` : undefined}>
    {#if !cover}<span>♪</span>{/if}
    {#if src}
      <button class="play" onclick={play} aria-label={playing ? $t("Playing") : $t("Play")}>
        {#if playing}<span class="eq"><i></i><i></i><i></i></span>
        {:else}<svg viewBox="0 0 24 24"><path d="M8 5v14l11-7z"/></svg>{/if}
      </button>
    {/if}
  </div>
  <div class="meta">
    <b title={card.title}>{card.title}</b>
    <small>{[card.artist, card.album].filter(Boolean).join(" — ")}</small>
    <span class="foot">
      {#if card.dur}<span class="dur">{fmtDuration(card.dur)}</span>{/if}
      {#if src}
        <button class="mini" onclick={save} title={$t("Download")} aria-label={$t("Download")}>
          <svg viewBox="0 0 24 24"><path d="M12 3v12"/><path d="M7 10l5 5 5-5"/><path d="M5 21h14"/></svg>
        </button>
      {:else}
        <span class="no">{!$musicOn && !$settings.navidrome ? $t("Navidrome add-on is off") : $t("No access to this Navidrome")}</span>
      {/if}
    </span>
    {#if invite}
      {#if joined}
        <button class="lt on" onclick={leaveListening}>👥 {$t("Leave")}</button>
      {:else}
        <button class="lt" onclick={join} disabled={!src}>👥 {$t("Join @{h}", { h: card.lt ?? from })}</button>
      {/if}
    {:else if card.lt && outgoing && $hosting === chat}
      <span class="lt-on">👥 {$t("You're hosting — they can join")}</span>
    {/if}
  </div>
</div>

<style>
  .track { display: flex; gap: 10px; align-items: center; padding: 8px; width: min(320px, 76vw); box-sizing: border-box;
    background: var(--bubble-in, var(--bg-elev)); border: 1px solid var(--border); border-radius: calc(16px * var(--rk, 1)); color: var(--fg); }
  .track.out { background: var(--bubble-out, var(--accent-soft)); }
  .art { position: relative; width: 64px; height: 64px; flex: none; border-radius: calc(11px * var(--rk, 1)); background-size: cover; background-position: center;
    background-color: color-mix(in srgb, var(--accent) 30%, transparent); display: grid; place-items: center; overflow: hidden; }
  .art > span { font-size: 26px; color: #fff; opacity: .8; }
  .play { position: absolute; inset: 0; display: grid; place-items: center; border: none; cursor: pointer; background: rgba(0, 0, 0, .28); color: #fff; }
  .play:hover { background: rgba(0, 0, 0, .42); }
  .play svg { width: 26px; height: 26px; fill: currentColor; }
  .eq { display: flex; gap: 3px; align-items: flex-end; height: 18px; }
  .eq i { width: 4px; background: #fff; border-radius: 2px; animation: eq 0.9s ease-in-out infinite; }
  .eq i:nth-child(2) { animation-delay: .2s; } .eq i:nth-child(3) { animation-delay: .4s; }
  @keyframes eq { 0%, 100% { height: 5px; } 50% { height: 18px; } }
  .meta { min-width: 0; flex: 1; display: flex; flex-direction: column; gap: 1px; }
  .meta b, .meta small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .meta b { font-size: 14px; } .meta small { font-size: 12px; color: var(--fg-muted); }
  .foot { display: flex; align-items: center; gap: 6px; margin-top: 3px; min-height: 24px; }
  .dur { font-size: 11.5px; color: var(--fg-muted); font-variant-numeric: tabular-nums; }
  .no { font-size: 11.5px; color: var(--fg-muted); }
  .mini { width: 26px; height: 26px; display: grid; place-items: center; border: none; background: transparent; color: var(--fg-muted);
    border-radius: calc(8px * var(--rk, 1)); cursor: pointer; margin-left: auto; }
  .mini:hover { background: var(--bg-hover); color: var(--fg); }
  .mini svg { width: 16px; height: 16px; fill: none; stroke: currentColor; stroke-width: 2; stroke-linecap: round; stroke-linejoin: round; }
  .lt { margin-top: 6px; height: 30px; border: none; border-radius: calc(10px * var(--rk, 1)); cursor: pointer; font: inherit; font-size: 12.5px; font-weight: 700;
    background: linear-gradient(135deg, var(--accent), var(--accent-2)); color: #061015; }
  .lt:disabled { opacity: .5; cursor: default; }
  .lt.on { background: transparent; border: 1px solid var(--accent); color: var(--accent); }
  .lt-on { margin-top: 4px; font-size: 11.5px; color: var(--accent); }
  .mute .meta b { color: var(--fg); }
</style>
