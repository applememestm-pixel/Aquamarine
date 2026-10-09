<script module lang="ts">
  import { writable } from "svelte/store";
  const toastText = writable<string>("");
  const toastVisible = writable<boolean>(false);
  let hideTimer: ReturnType<typeof setTimeout> | null = null;

  export function showToast(text: string, ms = 2400) {
    toastText.set(text);
    toastVisible.set(true);
    if (hideTimer) clearTimeout(hideTimer);
    hideTimer = setTimeout(() => toastVisible.set(false), ms);
  }

  export { toastText, toastVisible };
</script>

<script lang="ts">
  import { icons } from "../icons";
</script>

<div class="toast" class:show={$toastVisible}>
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
    border-radius: 10px;
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
  .toast :global(svg) { width: 14px; height: 14px; color: var(--accent); }
</style>
