<script module lang="ts">
  import { writable } from "svelte/store";
  export const meCardOpen = writable(false);
</script>

<script lang="ts">
  /**
   * Phones: a tap on your own avatar at the top opens this card in the
   * middle of the screen — who you are, your status, and the way to your
   * profile, Saved Messages, stories and settings (Telegram).
   */
  import { t, tr } from "../i18n";
  import { info, selectChat } from "../stores";
  import { profile } from "../profile";
  import { myOnline, type OnlineStatus } from "../presence";
  import { openProfile } from "./ProfileModal.svelte";
  import { openSettings } from "../panels/settings";
  import { averageColor, mixHex, patternRingUrl, cardLook, shellColor, CARD_INTENSITY, CARD_LENGTH } from "../cardStyle";
  import { addStory, myLiveStories } from "../storyFeed";
  import { openStories } from "../stories";

  const STATUSES: { id: OnlineStatus; label: string }[] = [
    { id: "online", label: "Online" }, { id: "idle", label: "Idle" }, { id: "dnd", label: "Do Not Disturb" }, { id: "invisible", label: "Invisible" },
  ];
  const name = $derived($profile.display_name && $profile.display_name !== "You" ? $profile.display_name : ($info?.username ?? ""));
  let sampled = $state("");
  $effect(() => {
    const u = $profile.banner_url && !/\.(mp4|webm|mov)(\?|$)/i.test($profile.banner_url) ? $profile.banner_url : "";
    sampled = "";
    if (u) void averageColor(u).then((c) => { if (u === $profile.banner_url) sampled = c; });
  });
  const look = $derived(cardLook($profile.card || sampled || mixHex($profile.banner_a, $profile.banner_b), $profile.card_i ?? CARD_INTENSITY, $profile.card_g ?? CARD_LENGTH, shellColor()));
  const pat = $derived($profile.pattern ? patternRingUrl($profile.pattern) : "");
  function close() { meCardOpen.set(false); }
  function go(f: () => void) { close(); f(); }
</script>

{#if $meCardOpen}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="mc-back" onclick={close}>
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div class="mc" role="dialog" aria-modal="true" tabindex="-1" aria-label={$t("Profile")} onclick={(e) => e.stopPropagation()}
      style:--card-bg={look.background} class:light={look.light}>
      <button class="x" data-x data-keep-x onclick={close} aria-label={$t("Close")}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M6 6l12 12M18 6 6 18"/></svg>
      </button>
      <div class="banner" style:background={$profile.banner_url && !/\.(mp4|webm|mov)(\?|$)/i.test($profile.banner_url) ? `center / cover no-repeat url("${$profile.banner_url}")` : `linear-gradient(135deg, ${$profile.banner_a}, ${$profile.banner_b})`}>
        {#if pat}<div class="pattern" style:background-image={`url("${pat}")`}></div>{/if}
      </div>
      <button class="ava" class:ring={$myLiveStories.length > 0} onclick={() => go(() => ($myLiveStories.length ? openStories([""], "") : openProfile()))}
        style:background-image={$profile.avatar_url ? `url("${$profile.avatar_url}")` : `linear-gradient(135deg, ${$profile.banner_a}, ${$profile.banner_b})`}>{$profile.avatar_url ? "" : name.charAt(0).toUpperCase()}</button>
      <div class="who">
        <b>{name}</b>
        <small>@{$info?.username ?? ""}{$profile.status ? ` · ${$profile.status}` : ""}</small>
      </div>
      <div class="status" role="radiogroup" aria-label={$t("Status")}>
        {#each STATUSES as s (s.id)}
          <button role="radio" aria-checked={$myOnline === s.id} class:on={$myOnline === s.id} onclick={() => myOnline.set(s.id)}>
            <i class="dot {s.id}"></i>{$t(s.label)}
          </button>
        {/each}
      </div>
      <div class="acts">
        <button onclick={() => go(() => openProfile())}><span>👤</span>{$t("My profile")}</button>
        <button onclick={() => go(() => void addStory("camera"))}><span>⊕</span>{$t("Add a story")}</button>
        <button onclick={() => go(() => void selectChat("__saved__"))}><span>🔖</span>{$t("Saved Messages")}</button>
        <button onclick={() => go(() => openSettings())}><span>⚙</span>{$t("Settings")}</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .mc-back { position: fixed; inset: 0; z-index: 10080; display: grid; place-items: center; padding: 18px; background: rgba(0,0,0,.5); animation: mc-fade .16s ease; }
  @keyframes mc-fade { from { opacity: 0; } }
  .mc { position: relative; width: min(360px, 100%); border-radius: calc(22px * var(--rk, 1)); overflow: hidden; background: var(--card-bg, var(--bg-shell));
    border: 1px solid var(--border-strong); box-shadow: 0 30px 80px -20px rgba(0,0,0,.8); padding-bottom: 12px; animation: mc-pop .24s cubic-bezier(.2,.9,.3,1.15); }
  @keyframes mc-pop { from { transform: scale(.9); opacity: 0; } }
  .mc.light { --fg: #15171d; --fg-muted: #383d48; --fg-dim: #5a606d; --border: rgba(0,0,0,.14); --bg-hover: rgba(0,0,0,.06); color: var(--fg); }
  .x { position: absolute; top: 10px; right: 10px; z-index: 2; width: 34px; height: 34px; border: none; border-radius: 50%; display: grid; place-items: center; color: #fff; background: rgba(0,0,0,.3); cursor: pointer; }
  .x svg { width: 18px; height: 18px; }
  .banner { position: relative; height: 110px; overflow: hidden; }
  .pattern { position: absolute; inset: 0; pointer-events: none; background-size: auto 180%; background-repeat: no-repeat; background-position: center top; opacity: .5; mix-blend-mode: overlay; }
  .ava { display: grid; place-items: center; width: 92px; height: 92px; margin: -46px auto 0; position: relative; border-radius: var(--ava-r, 50%); border: 4px solid var(--card-bg, var(--bg-shell));
    background: center / cover; color: rgba(6,7,12,.85); font: 800 34px var(--font-ui, inherit); cursor: pointer; padding: 0; }
  .ava.ring { box-shadow: 0 0 0 3px var(--accent); }
  .who { display: flex; flex-direction: column; align-items: center; gap: 2px; padding: 8px 16px 4px; text-align: center; }
  .who b { font-size: 19px; font-weight: 800; }
  .who small { font-size: 13px; color: var(--fg-muted); }
  .status { display: flex; flex-wrap: wrap; justify-content: center; gap: 6px; padding: 10px 14px; }
  .status button { display: inline-flex; align-items: center; gap: 6px; padding: 6px 10px; border-radius: calc(14px * var(--rk, 1)); border: 1px solid var(--border); background: transparent; color: var(--fg); font: 500 12.5px var(--font-ui, inherit); cursor: pointer; }
  .status button.on { border-color: var(--accent); background: var(--accent-soft); }
  .dot { width: 8px; height: 8px; border-radius: 50%; background: var(--online); }
  .dot.idle { background: #FAA61A; }
  .dot.dnd { background: var(--danger); }
  .dot.invisible { background: var(--fg-dim); }
  .acts { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; padding: 4px 14px 0; }
  .acts button { display: flex; align-items: center; gap: 8px; padding: 11px 12px; border-radius: calc(14px * var(--rk, 1)); border: 1px solid var(--border); background: color-mix(in srgb, var(--fg) 4%, transparent);
    color: var(--fg); font: 600 13.5px var(--font-ui, inherit); cursor: pointer; text-align: left; }
  .acts button:active { transform: scale(.97); }
  .acts span { width: 20px; text-align: center; color: var(--accent); }
</style>
