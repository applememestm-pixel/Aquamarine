<script lang="ts">
  /** The one (animated) emoji next to a name, Telegram-Premium style. A pack
   *  emoji the device doesn't have is fetched from the relay (reactionPack). */
  import AnimatedMedia from "./AnimatedMedia.svelte";
  import { reactionPack, reactionUrl, parseReaction } from "../reactionPack";

  let { token = "", size = 18 }: { token?: string | null; size?: number } = $props();
  const tok = $derived((token ?? "").trim());
  const media = $derived(tok && tok !== "-" ? ((void $reactionPack), reactionUrl(tok)) : null);
  const plain = $derived(tok && tok !== "-" ? parseReaction(tok).emoji : "");
</script>

{#if tok && tok !== "-"}
  <span class="es" style:width="{size}px" style:height="{size}px" style:font-size="{Math.round(size * 0.86)}px" title={plain}>
    {#if media}<AnimatedMedia emoji src={media.url} mime={media.mime} alt={plain} play="auto" />{:else}{plain}{/if}
  </span>
{/if}

<style>
  .es { display: inline-grid; place-items: center; flex-shrink: 0; vertical-align: -3px; line-height: 1; margin-left: 4px; }
  .es :global(img), .es :global(video), .es :global(canvas) { width: 100%; height: 100%; object-fit: contain; }
</style>
