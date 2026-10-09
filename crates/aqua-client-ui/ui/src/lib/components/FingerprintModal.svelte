<script module lang="ts">
  import { writable } from "svelte/store";
  import type { FingerprintView } from "../types";

  const open = writable<boolean>(false);
  const view = writable<FingerprintView | null>(null);

  export function openFingerprint(v: FingerprintView) {
    view.set(v);
    open.set(true);
  }
  export function closeFingerprint() { open.set(false); }
  export { open as fpOpen, view as fpView };
</script>

<script lang="ts">
  import { t } from "../i18n";
  import { onMount } from "svelte";
  import { showToast } from "./Toast.svelte";

  onMount(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") closeFingerprint(); };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  });

  function markVerified() {
    const name = $view?.peer_name ?? "This peer";
    closeFingerprint();
    showToast(`${name}'s key marked verified`);
  }
</script>

{#if $open && $view}
  <div
    class="modal-bg"
    role="button"
    tabindex="-1"
    aria-label={$t("Close verification dialog")}
    onclick={(e) => { if (e.target === e.currentTarget) closeFingerprint(); }}
    onkeydown={(e) => { if (e.key === "Escape" && e.target === e.currentTarget) closeFingerprint(); }}
  >
    <div class="modal" role="dialog" aria-modal="true">
      <h2>Verify {$view.peer_name}'s key</h2>
      <p>Compare these words in person or on a trusted channel. If they match on both devices, no one is impersonating {$view.peer_name}.</p>
      <div class="fp-grid">
        {#each $view.words as w, i (i)}
          <div class="word">{w}</div>
        {/each}
      </div>
      <div class="fp-instruction">
        {$t("Also visible as SHA-256 fingerprint:")}
        <span class="hex">{$view.hex}</span>
      </div>
      <div class="modal-actions">
        <button class="btn" onclick={closeFingerprint}>{$t("Cancel")}</button>
        <button class="btn primary" onclick={markVerified}>{$t("They match")}</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .modal-bg {
    position: fixed;
    inset: 0;
    background: rgba(6,7,12,0.6);
    backdrop-filter: blur(6px);
    display: grid;
    place-items: center;
    z-index: 200;
    padding: 20px;
  }
  .modal {
    background: var(--bg-surface);
    border: 1px solid var(--border-strong);
    border-radius: 16px;
    padding: 28px;
    max-width: 420px;
    width: 100%;
    box-shadow: 0 32px 64px -16px rgba(0,0,0,0.6), var(--shadow-accent);
  }
  h2 { margin: 0 0 4px; font-size: 18px; font-weight: 700; letter-spacing: -0.01em; }
  p  { margin: 0 0 20px; color: var(--fg-muted); font-size: 13.5px; }
  .fp-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
    padding: 16px;
    background: var(--bg-input);
    border: 1px solid var(--border);
    border-radius: 12px;
    font-family: var(--font-mono);
    font-size: 15px;
    letter-spacing: 0.03em;
    margin-bottom: 20px;
  }
  .word {
    color: var(--accent);
    font-weight: 500;
    padding: 4px 10px;
    background: rgba(0,229,199,0.06);
    border-radius: 6px;
    text-align: center;
  }
  .word:nth-child(even) { color: var(--accent-2); background: rgba(123,92,255,0.06); }
  .fp-instruction {
    font-size: 12.5px;
    color: var(--fg-muted);
    padding: 10px 12px;
    background: rgba(0,0,0,0.2);
    border-radius: 8px;
    border-left: 2px solid var(--accent);
    margin-bottom: 20px;
    word-break: break-all;
  }
  .fp-instruction .hex {
    font-family: var(--font-mono);
    color: var(--accent);
    font-size: 12px;
  }
  .modal-actions { display: flex; gap: 10px; justify-content: flex-end; }
  .btn {
    padding: 10px 18px;
    border-radius: 10px;
    border: 1px solid var(--border-strong);
    background: var(--bg-input);
    color: var(--fg);
    font-family: inherit;
    font-weight: 600;
    font-size: 13.5px;
    cursor: pointer;
    transition: all 0.12s ease;
  }
  .btn:hover { background: var(--bg-hover); border-color: var(--fg-dim); }
  .btn.primary {
    background: linear-gradient(135deg, var(--accent), var(--accent-2));
    color: #06070C;
    border-color: transparent;
    box-shadow: 0 0 16px -4px var(--accent-glow);
  }
  .btn.primary:hover { box-shadow: 0 0 24px -2px var(--accent-glow); }
</style>
