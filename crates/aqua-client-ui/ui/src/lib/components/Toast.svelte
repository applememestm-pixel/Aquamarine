<script module lang="ts">
  import { writable } from "svelte/store";
  import { quotaText } from "../quotaText";
  const toastText = writable<string>("");
  const toastVisible = writable<boolean>(false);
  let hideTimer: ReturnType<typeof setTimeout> | null = null;

  export function showToast(text: string, ms = 2400) {
    // "Over your share of the server": in people's words (quotaText.ts).
    const q = quotaText(text);
    if (q) { text = q; ms = Math.max(ms, 6000); }
    toastText.set(text);
    toastVisible.set(true);
    if (hideTimer) clearTimeout(hideTimer);
    hideTimer = setTimeout(() => toastVisible.set(false), ms);
  }

  export { toastText, toastVisible };
</script>

<script lang="ts">
  import { icons } from "../icons";
  import { IS_MOBILE } from "../platform";
</script>

<!-- Phones: at the top — at the bottom it covered the player's and the field's buttons. -->
<div class="toast" class:show={$toastVisible} class:top={IS_MOBILE}>
  {@html icons.check}
  <span>{$toastText}</span>
</div>

<style>
  .toast {
    position: fixed;
    bottom: 20px; left: 50%;
    transform: translateX(-50%) translateY(60px);
    background: var(--bg-surface-2);
    border: 1px solid var(--border-strong);
    color: var(--fg);
    padding: 10px 18px;
    border-radius: calc(10px * var(--rk, 1));
    font-size: 13px;
    opacity: 0;
    transition: all 0.25s ease;
    box-shadow: var(--shadow-accent);
    z-index: 300;
    display: flex;
    align-items: center;
    gap: 8px;
    pointer-events: none;
  }
  .toast.show { opacity: 1; transform: translateX(-50%) translateY(0); }
  .toast.top { bottom: auto; top: calc(10px + var(--sat, env(safe-area-inset-top, 0px))); transform: translateX(-50%) translateY(-60px); max-width: calc(100vw - 32px); box-sizing: border-box; }
  .toast.top.show { transform: translateX(-50%) translateY(0); }
  .toast :global(svg) { width: 14px; height: 14px; color: var(--accent); }
</style>
