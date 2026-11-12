<script lang="ts">
  /** The one (animated) emoji next to a name, Telegram-Premium style. A pack
   *  emoji the device doesn't have is fetched from the relay (reactionPack).
   *  `zoom`: a click shows it big, in full resolution (emoji or sticker). */
  import AnimatedMedia from "./AnimatedMedia.svelte";
  import { reactionPack, reactionUrl, parseReaction, reactionSource, brokenReaction } from "../reactionPack";
  import { t } from "../i18n";

  let { token = "", size = 18, zoom = false }: { token?: string | null; size?: number; zoom?: boolean } = $props();
  const tok = $derived((token ?? "").trim());
  const media = $derived(tok && tok !== "-" ? ((void $reactionPack), reactionUrl(tok)) : null);
  const plain = $derived(tok && tok !== "-" ? parseReaction(tok).emoji : "");
  const source = $derived(tok && tok !== "-" ? reactionSource(tok) : null);
  let big = $state(false);

  /** The big view lives in <body>: no parent's overflow or transform clips it. */
  function portal(node: HTMLElement) {
    document.body.appendChild(node);
    return { destroy() { node.remove(); } };
  }
  $effect(() => {
    if (!big) return;
    const k = (e: KeyboardEvent) => { if (e.key === "Escape") { e.stopPropagation(); big = false; } };
    document.addEventListener("keydown", k, true);
    return () => document.removeEventListener("keydown", k, true);
  });
</script>

{#if tok && tok !== "-"}
  {#if zoom}
    <button type="button" class="es zoomable" style:width="{size}px" style:height="{size}px" style:font-size="{Math.round(size * 0.86)}px"
      title={plain} onclick={(e) => { e.stopPropagation(); big = true; }} aria-label={plain}>
      {#if media}<AnimatedMedia emoji src={media.url} mime={media.mime} alt={plain} play="auto" onbroken={brokenReaction} />{:else}{plain}{/if}
    </button>
  {:else}
    <span class="es" style:width="{size}px" style:height="{size}px" style:font-size="{Math.round(size * 0.86)}px" title={plain}>
      {#if media}<AnimatedMedia emoji src={media.url} mime={media.mime} alt={plain} play="auto" onbroken={brokenReaction} />{:else}{plain}{/if}
    </span>
  {/if}
  {#if big}
    <div class="es-big" use:portal role="presentation" onclick={() => (big = false)}>
      <div class="es-big-art">
        {#if media}<AnimatedMedia src={media.url} mime={media.mime} alt={plain} play="auto" onbroken={brokenReaction} />{:else}<span class="es-big-txt">{plain}</span>{/if}
      </div>
      {#if source}<div class="es-big-note">{$t("From the pack")} <b>{source.title}</b></div>{/if}
    </div>
  {/if}
{/if}

<style>
  .es { display: inline-grid; place-items: center; flex-shrink: 0; vertical-align: -3px; line-height: 1; margin-left: 4px; }
  .es :global(img), .es :global(video), .es :global(canvas) { width: 100%; height: 100%; object-fit: contain; display: block; }
  /* The player's own box fills the square too, so the picture is centred in it. */
  .es > :global(*) { width: 100%; height: 100%; }
  button.es { border: none; background: none; padding: 0; cursor: zoom-in; color: inherit; transition: transform .15s; }
  button.es:hover { transform: scale(1.15); }
  .es-big { position: fixed; inset: 0; z-index: 400; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 14px;
    background: rgba(4, 6, 10, .7); backdrop-filter: blur(8px); cursor: zoom-out; animation: es-fade .18s ease-out; }
  @keyframes es-fade { from { opacity: 0; } }
  .es-big-art { width: min(260px, 70vw); height: min(260px, 70vw); display: grid; place-items: center; animation: es-pop .32s cubic-bezier(.2, 1.3, .3, 1); }
  @keyframes es-pop { from { transform: scale(.3); opacity: 0; } }
  .es-big-art :global(img), .es-big-art :global(video), .es-big-art :global(canvas), .es-big-art :global(.am) { width: 100%; height: 100%; object-fit: contain; }
  .es-big-txt { font-size: min(200px, 54vw); line-height: 1; font-family: "Noto Color Emoji", "Apple Color Emoji", "Segoe UI Emoji", sans-serif; }
  .es-big-note { color: #fff; opacity: .85; font-size: 13px; }
</style>
