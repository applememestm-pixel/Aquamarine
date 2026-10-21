<script lang="ts">
  /** A game's portrait poster: the real cover art when there is one (Steam
   *  library capsule / Wikipedia box art), else a poster made from its icon. */
  import { gameGradient } from "../played";
  import { t } from "../i18n";

  let { name, cover = "", icon = "", hours = 0, label = true }: { name: string; cover?: string; icon?: string; hours?: number; label?: boolean } = $props();
  let broken = $state(false);
  let wide = $state(false);
  $effect(() => { void cover; broken = false; wide = false; });
  let src = $derived(cover && !broken ? cover : "");
  function loaded(e: Event) {
    const i = e.currentTarget as HTMLImageElement;
    // A logo or a landscape header: show it whole over its own blur.
    wide = i.naturalWidth > 0 && i.naturalWidth / i.naturalHeight > 0.85;
  }
</script>

<div class="gp" class:art={!!src} class:wide title={name} style:--gg={gameGradient(name)}>
  {#if src}
    {#if wide}<img class="gp-blur" {src} alt="" />{/if}
    <img class="gp-cover" {src} alt={name} loading="lazy" onload={loaded} onerror={() => (broken = true)} />
  {:else if icon}
    <img class="gp-blur" src={icon} alt="" /><img class="gp-ic" src={icon} alt="" />
  {:else}
    <span class="gp-ph">{name.charAt(0).toUpperCase()}</span>
  {/if}
  {#if label || !src}<span class="gp-name">{name}</span>{/if}
  {#if hours > 0}<span class="gp-h">{hours} {$t("h")}</span>{/if}
</div>

<style>
  .gp { position: relative; aspect-ratio: 3 / 4; border-radius: 10px; overflow: hidden; color: #fff; background: var(--gg);
    box-shadow: 0 0 0 1px rgba(255,255,255,0.06) inset; transition: transform 0.2s cubic-bezier(.2,.8,.2,1), box-shadow 0.2s; }
  .gp:hover { transform: translateY(-3px); box-shadow: 0 14px 28px -12px rgba(0,0,0,0.7); }
  .gp-cover { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: cover; transition: transform 0.35s cubic-bezier(.2,.8,.2,1); }
  .gp.wide .gp-cover { object-fit: contain; padding: 10%; box-sizing: border-box; filter: drop-shadow(0 6px 14px rgba(0,0,0,0.5)); }
  .gp:hover .gp-cover { transform: scale(1.05); }
  .gp-blur { position: absolute; inset: -20%; width: 140%; height: 140%; object-fit: cover; filter: blur(18px) saturate(1.4) brightness(0.65); }
  .gp-ic { position: absolute; top: 20%; left: 50%; transform: translateX(-50%); width: 50%; aspect-ratio: 1; border-radius: 14px; box-shadow: 0 8px 20px rgba(0,0,0,0.45); }
  .gp-ph { position: absolute; top: 24%; left: 50%; transform: translateX(-50%); font-size: 34px; font-weight: 900; opacity: 0.9; }
  .gp-name { position: absolute; left: 0; right: 0; bottom: 0; padding: 22px 8px 7px; font-size: 12px; font-weight: 700; line-height: 1.25;
    background: linear-gradient(transparent, rgba(0,0,0,0.8)); overflow: hidden; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; }
  .gp.art .gp-name { opacity: 0; transition: opacity 0.2s; }
  .gp.art:hover .gp-name { opacity: 1; }
  .gp-h { position: absolute; top: 6px; right: 6px; font-size: 10.5px; font-weight: 700; background: rgba(0,0,0,0.6); border-radius: 6px; padding: 1px 5px; }
</style>
