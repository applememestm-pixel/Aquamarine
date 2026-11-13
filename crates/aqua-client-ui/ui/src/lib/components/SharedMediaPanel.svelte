<script module lang="ts">
  import { writable } from "svelte/store";
  export const sharedMediaOpen = writable(false);
  export function closeSharedMedia() { sharedMediaOpen.set(false); }
  export function toggleSharedMedia() { sharedMediaOpen.update((v) => !v); }
</script>

<script lang="ts">
  import { sideResize } from "../sideResize";
  import { t } from "../i18n";
  import { activeChat } from "../stores";
  import SharedMedia from "./SharedMedia.svelte";
</script>

{#if $sharedMediaOpen && $activeChat}
  <aside class="sm" use:sideResize={"sharedmediapanel"} data-back>
    <div class="sm-head">
      <h3>{$t("Shared media")}</h3>
      <button data-x class="sm-close" onclick={closeSharedMedia} title={$t("Close")}>×</button>
    </div>
    {#key $activeChat.id}<SharedMedia peerId={$activeChat.id} title={$activeChat.name} />{/key}
  </aside>
{/if}

<style>
  .sm {
    width: 320px;
    flex-shrink: 0;
    background: var(--bg-list);
    border-left: 1px solid var(--border);
    display: flex; flex-direction: column;
    min-height: 0;
    animation: sm-in 0.18s ease-out;
  }
  @keyframes sm-in { from { opacity: 0; transform: translateX(10px); } to { opacity: 1; transform: translateX(0); } }
  .sm-head { display: flex; align-items: center; justify-content: space-between; padding: 14px 16px; border-bottom: 1px solid var(--border); }
  .sm-head h3 { margin: 0; font-size: 14px; font-weight: 700; }
  .sm-close { width: 28px; height: 28px; background: transparent; border: 1px solid var(--border); border-radius: calc(8px * var(--rk, 1)); color: var(--fg-dim); font-size: 17px; cursor: pointer; }
  .sm-close:hover { color: var(--fg); background: var(--bg-hover); }
  @media (max-width: 960px) { .sm { display: none; } }
</style>
