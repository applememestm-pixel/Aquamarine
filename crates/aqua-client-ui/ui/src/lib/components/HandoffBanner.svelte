<script lang="ts">
  import { t, tr } from "../i18n";
  import { handoffOffer, acceptHandoff } from "../handoff";
  import { chats } from "../stores";
</script>

{#if $handoffOffer}
  {@const o = $handoffOffer}
  <div class="ho" role="status">
    <span class="ho-ic">⇄</span>
    <span class="ho-main">
      <b>{tr("Continue on this device: {c}", { c: $chats.find((c) => c.id === o.chat)?.name ?? o.chat })}</b>
      <span>{o.dev ? tr("from {d}", { d: o.dev }) : ""}{o.draft ? (o.dev ? " · " : "") + "“" + o.draft.slice(0, 60) + (o.draft.length > 60 ? "…" : "") + "”" : ""}</span>
    </span>
    <button class="ho-go" onclick={acceptHandoff}>{$t("Continue")}</button>
    <button class="ho-x" onclick={() => handoffOffer.set(null)} aria-label={$t("Close")}>✕</button>
  </div>
{/if}

<style>
  .ho { position: fixed; left: 50%; bottom: 90px; transform: translateX(-50%); z-index: 970; display: flex; align-items: center; gap: 10px; max-width: min(520px, calc(100vw - 24px)); padding: 10px 10px 10px 14px; border-radius: calc(16px * var(--rk, 1)); background: var(--bg-shell); border: 1px solid var(--accent); box-shadow: 0 18px 40px -16px rgba(0,0,0,0.6); animation: ho-in 0.3s cubic-bezier(.2,.9,.3,1.2); }
  @keyframes ho-in { from { opacity: 0; transform: translate(-50%, 16px); } }
  .ho-ic { font-size: 20px; color: var(--accent); }
  .ho-main { min-width: 0; display: flex; flex-direction: column; gap: 1px; }
  .ho-main b { font-size: 13.5px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .ho-main span { font-size: 12px; color: var(--fg-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .ho-go { border: none; border-radius: calc(10px * var(--rk, 1)); padding: 8px 14px; background: var(--accent); color: #06070C; font: inherit; font-weight: 700; cursor: pointer; flex-shrink: 0; }
  .ho-x { width: 28px; height: 28px; border-radius: 50%; border: none; background: var(--bg-hover); color: var(--fg-muted); cursor: pointer; flex-shrink: 0; }
</style>
