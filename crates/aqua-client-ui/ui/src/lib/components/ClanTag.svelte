<script lang="ts">
  import { t, tr } from "../i18n";
  /**
   * A Discord-style server tag next to someone's name: badge + up to 4
   * characters in the group's colour. Click → what the group is, and a way
   * to ask to join.
   */
  import { badgeSvg } from "../clanBadges";
  import { groupMap, requestJoinGroup } from "../groups";
  import { info, selectChat } from "../stores";
  import { api } from "../tauri";
  import { showToast } from "./Toast.svelte";

  interface Tag { text: string; badge: string; color: string; gid?: string; title?: string; owner?: string }
  let { tag, size = "sm" }: { tag: Tag | null | undefined; size?: "sm" | "md" } = $props();
  let open = $state(false);
  let root: HTMLSpanElement | undefined = $state();

  const isMember = $derived(!!tag?.gid && !!$groupMap[tag.gid]?.members.some((m) => m.username === $info?.username));

  async function requestJoin() {
    if (!tag?.gid || !tag.owner) return;
    try {
      await requestJoinGroup(tag.gid, tag.owner, tag.title ?? "", "");
      showToast(tr("Request sent — an admin will let you in"));
      open = false;
    } catch (e) { showToast(`Could not send request: ${e}`); }
  }
</script>

<svelte:window onmousedown={(e) => { if (open && root && !root.contains(e.target as Node)) open = false; }} />

{#if tag && tag.text}
  <span class="wrap" bind:this={root}>
    <button
      class="clan {size}"
      style:--c={tag.color || "var(--accent)"}
      title={tag.title ? `${tag.title} — server tag` : "Server tag"}
      onclick={(e) => { e.stopPropagation(); open = !open; }}
    >
      <span class="ic">{@html badgeSvg(tag.badge)}</span>{tag.text}
    </button>
    {#if open}
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="pop" onclick={(e) => e.stopPropagation()}>
        <div class="pop-head" style:--c={tag.color || "var(--accent)"}>
          <span class="big">{@html badgeSvg(tag.badge)}</span>
          <div>
            <div class="pt">{tag.title ?? "Community"}</div>
            <div class="ps">{$t("Server tag")} <b>{tag.text}</b></div>
          </div>
        </div>
        {#if tag.gid}
          {#if isMember}
            <button class="pbtn" onclick={() => { open = false; void selectChat(tag.gid!); }}>{$t("Open")}</button>
          {:else if tag.owner}
            <button class="pbtn" onclick={requestJoin}>{$t("Request to join")}</button>
          {/if}
        {/if}
      </div>
    {/if}
  </span>
{/if}

<style>
  .wrap { position: relative; display: inline-flex; vertical-align: middle; }
  .clan {
    display: inline-flex; align-items: center; gap: 3px;
    border: 1px solid color-mix(in srgb, var(--c) 45%, transparent);
    background: color-mix(in srgb, var(--c) 16%, transparent);
    color: var(--c);
    border-radius: calc(6px * var(--rk, 1)); cursor: pointer;
    font-weight: 800; letter-spacing: 0.04em; line-height: 1;
    font-family: inherit;
  }
  .clan.sm { font-size: 10px; padding: 2px 5px 2px 4px; }
  .clan.md { font-size: 12px; padding: 3px 7px 3px 5px; }
  .ic { display: inline-grid; place-items: center; }
  .clan.sm .ic :global(svg) { width: 11px; height: 11px; }
  .clan.md .ic :global(svg) { width: 14px; height: 14px; }
  .clan:hover { background: color-mix(in srgb, var(--c) 26%, transparent); }
  .pop {
    position: absolute; left: 0; top: calc(100% + 6px); z-index: 60; width: 240px;
    background: var(--bg-surface); border: 1px solid var(--border-strong); border-radius: calc(12px * var(--rk, 1));
    box-shadow: 0 16px 40px -10px rgba(0, 0, 0, 0.7); padding: 12px; text-align: left;
    font-weight: 500; letter-spacing: normal;
  }
  .pop-head { display: flex; gap: 10px; align-items: center; }
  .big {
    width: 40px; height: 40px; border-radius: calc(12px * var(--rk, 1)); display: grid; place-items: center; flex-shrink: 0;
    color: var(--c); background: color-mix(in srgb, var(--c) 18%, transparent);
  }
  .big :global(svg) { width: 22px; height: 22px; }
  .pt { font-size: 14px; font-weight: 800; color: var(--fg); }
  .ps { font-size: 12px; color: var(--fg-muted); }
  .pbtn {
    width: 100%; margin-top: 10px; border: none; border-radius: calc(8px * var(--rk, 1)); padding: 8px;
    background: var(--accent); color: #06070C; font-weight: 700; cursor: pointer;
  }
</style>
