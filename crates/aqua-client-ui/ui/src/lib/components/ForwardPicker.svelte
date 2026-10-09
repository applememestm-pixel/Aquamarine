<script module lang="ts">
  import { writable, get as getStore } from "svelte/store";

  /** One forwardable payload, extracted from a chat message. */
  export type ForwardItem =
    | { kind: "text"; text: string }
    | { kind: "file"; path: string; name: string };

  interface ForwardState {
    open: boolean;
    items: ForwardItem[];
  }
  export const forwardState = writable<ForwardState>({ open: false, items: [] });

  /** Open the forward picker for a set of messages. */
  export function openForward(items: ForwardItem[]) {
    if (items.length === 0) return;
    forwardState.set({ open: true, items });
  }
  export function closeForward() {
    forwardState.set({ open: false, items: [] });
  }
</script>

<script lang="ts">
  import { t } from "../i18n";
  import { onMount } from "svelte";
  import { chats, activeId, loadActiveMessages } from "../stores";
  import { api } from "../tauri";
  import { showToast } from "./Toast.svelte";

  const firstLetter = (s: string) => (s.trim()[0] ?? "?").toUpperCase();

  let query = $state("");
  let busy = $state(false);

  // Only live chats can receive a forward. Self-chat ("roma") is allowed.
  let targets = $derived(
    $chats
      .filter((c) => c.live || c.id === "roma")
      .filter((c) => c.name.toLowerCase().includes(query.trim().toLowerCase())),
  );

  async function forwardTo(peerId: string, peerName: string) {
    if (busy) return;
    busy = true;
    const items = getStore(forwardState).items;
    let ok = 0;
    let skipped = 0;
    try {
      for (const it of items) {
        try {
          if (it.kind === "text") {
            await api.sendMessage(peerId, it.text);
            ok++;
          } else {
            await api.attachFile(peerId, it.path);
            ok++;
          }
        } catch {
          skipped++;
        }
      }
      // If we forwarded into the chat we're already viewing, refresh it.
      if (peerId === getStore(activeId)) await loadActiveMessages();
      closeForward();
      let msg = `Forwarded ${ok} message${ok === 1 ? "" : "s"} to ${peerName}`;
      if (skipped) msg += ` (${skipped} skipped — not downloaded)`;
      showToast(msg);
    } finally {
      busy = false;
    }
  }

  function onEsc(e: KeyboardEvent) { if (e.key === "Escape") closeForward(); }
  onMount(() => {
    window.addEventListener("keydown", onEsc);
    return () => window.removeEventListener("keydown", onEsc);
  });
</script>

{#if $forwardState.open}
  <div
    class="fw-backdrop"
    role="button"
    tabindex="-1"
    aria-label={$t("Close")}
    onclick={(e) => { if ((e.target as HTMLElement).classList.contains("fw-backdrop")) closeForward(); }}
    onkeydown={(e) => { if (e.key === "Escape") closeForward(); }}
  >
    <div class="fw-sheet" role="dialog" aria-modal="true" tabindex="-1">
      <div class="fw-head">
        <h3>{$t("Forward to…")}</h3>
        <span class="fw-count">{$forwardState.items.length} selected</span>
        <button class="fw-close" onclick={closeForward} title={$t("Close (Esc)")}>×</button>
      </div>
      <input class="fw-search" placeholder={$t("Search chats")} bind:value={query} />
      <div class="fw-list">
        {#if targets.length === 0}
          <div class="fw-empty">{$t("No chats to forward to.")}</div>
        {:else}
          {#each targets as c (c.id)}
            <button class="fw-row" disabled={busy} onclick={() => forwardTo(c.id, c.name)}>
              <span class="fw-av" style="--gradient: {c.gradient}">{firstLetter(c.name)}</span>
              <span class="fw-name">{c.name}</span>
            </button>
          {/each}
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .fw-backdrop {
    position: fixed; inset: 0;
    background: rgba(6, 7, 12, 0.55);
    backdrop-filter: blur(4px);
    display: grid; place-items: center;
    z-index: 115;
    animation: fw-fade 0.15s ease-out;
  }
  @keyframes fw-fade { from { opacity: 0; } to { opacity: 1; } }
  .fw-sheet {
    width: min(400px, 92vw);
    max-height: 70vh;
    display: flex; flex-direction: column;
    background: var(--bg-shell);
    border: 1px solid var(--border);
    border-radius: 16px;
    box-shadow: 0 32px 80px -16px rgba(0, 0, 0, 0.6);
    overflow: hidden;
  }
  .fw-head {
    display: flex; align-items: center; gap: 10px;
    padding: 14px 16px; border-bottom: 1px solid var(--border);
  }
  .fw-head h3 { margin: 0; font-size: 14.5px; font-weight: 700; flex: 1; }
  .fw-count { font-size: 11.5px; color: var(--fg-dim); }
  .fw-close {
    width: 28px; height: 28px;
    background: transparent; border: 1px solid var(--border);
    border-radius: 8px; color: var(--fg-dim); font-size: 17px; cursor: pointer;
  }
  .fw-close:hover { color: var(--fg); background: var(--bg-hover); }
  .fw-search {
    margin: 10px 14px 6px;
    padding: 8px 11px;
    background: var(--bg-input); border: 1px solid var(--border);
    border-radius: 9px; color: var(--fg); font-family: inherit;
    font-size: 13px; outline: none;
  }
  .fw-search:focus { border-color: var(--accent); }
  .fw-list { overflow-y: auto; padding: 4px 8px 10px; }
  .fw-empty { padding: 24px; text-align: center; color: var(--fg-dim); font-size: 13px; }
  .fw-row {
    display: flex; align-items: center; gap: 11px;
    width: 100%; padding: 8px 10px;
    background: transparent; border: none; border-radius: 10px;
    cursor: pointer; text-align: left; color: var(--fg);
  }
  .fw-row:hover:not(:disabled) { background: var(--bg-hover); }
  .fw-row:disabled { opacity: 0.5; cursor: default; }
  .fw-av {
    width: 36px; height: 36px; border-radius: 50%;
    display: grid; place-items: center;
    font-weight: 700; font-size: 14px; color: rgba(6,7,12,0.85);
    background: var(--gradient, linear-gradient(135deg, #666, #999));
    flex-shrink: 0;
  }
  .fw-name { font-size: 13.5px; font-weight: 600; }
</style>
