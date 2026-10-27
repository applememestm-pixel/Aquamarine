<script lang="ts">
  /** Someone sends a file over Bluetooth: accept or decline, then progress. */
  import { t, tr } from "../i18n";
  import { btOffers, btAnswer, btTransfers } from "../bluetooth";
  import { invoke } from "@tauri-apps/api/core";

  const mb = (n: number) => (n >= 1048576 ? (n / 1048576).toFixed(1) + " MB" : n ? Math.max(1, Math.round(n / 1024)) + " KB" : "");
  let incoming = $derived(Object.values($btTransfers).filter((x) => x.incoming));
  let hidden = $state<Set<string>>(new Set());
  const hide = (id: string) => (hidden = new Set([...hidden, id]));
</script>

{#if $btOffers.length || incoming.some((x) => !hidden.has(x.id))}
  <div class="bi">
    {#each $btOffers as o (o.id)}
      <div class="bi-card" role="alert">
        <b>📶 {tr("{d} wants to send you a file", { d: o.from || tr("A Bluetooth device") })}</b>
        <span class="bi-f">{o.name}{o.size ? " · " + mb(o.size) : ""}</span>
        <div class="bi-btns">
          <button class="bi-btn" onclick={() => btAnswer(o.id, true)}>{$t("Accept")}</button>
          <button class="bi-btn ghost" onclick={() => btAnswer(o.id, false)}>{$t("Decline")}</button>
        </div>
      </div>
    {/each}
    {#each incoming.filter((x) => !hidden.has(x.id)) as x (x.id)}
      <div class="bi-card">
        <b>{x.status === "complete" ? "✓ " + $t("File received") : x.status === "error" ? "✕ " + $t("Transfer failed") : $t("Receiving…")}</b>
        <span class="bi-f">{x.name}{x.size ? " · " + mb(x.size) : ""}</span>
        {#if x.status !== "complete" && x.status !== "error"}
          <span class="bi-bar"><i style:width="{x.size ? Math.min(100, (x.sent / x.size) * 100) : 5}%"></i></span>
        {/if}
        <div class="bi-btns">
          {#if x.status === "complete" && x.path}<button class="bi-btn" onclick={() => { void invoke("bt_show_inbox").catch(() => {}); hide(x.id); }}>{$t("Show in folder")}</button>{/if}
          {#if x.status === "complete" || x.status === "error"}<button class="bi-btn ghost" onclick={() => hide(x.id)}>{$t("Close")}</button>{/if}
        </div>
      </div>
    {/each}
  </div>
{/if}

<style>
  .bi { position: fixed; right: 16px; bottom: 16px; z-index: 976; display: flex; flex-direction: column; gap: 8px; width: min(340px, calc(100vw - 32px)); }
  .bi-card { padding: 12px 14px; border-radius: calc(14px * var(--rk, 1)); background: var(--bg-shell); border: 1px solid color-mix(in srgb, #1f6fff 60%, var(--border)); display: flex; flex-direction: column; gap: 5px; box-shadow: 0 16px 40px -16px rgba(0,0,0,0.6); animation: bi-in 0.2s ease-out; }
  @keyframes bi-in { from { opacity: 0; transform: translateY(10px); } }
  .bi-card b { font-size: 13.5px; }
  .bi-f { font-size: 12.5px; color: var(--fg-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .bi-btns { display: flex; gap: 6px; margin-top: 4px; }
  .bi-btns:empty { display: none; }
  .bi-btn { border: none; border-radius: calc(9px * var(--rk, 1)); padding: 7px 12px; background: #1f6fff; color: #fff; font: inherit; font-weight: 700; cursor: pointer; }
  .bi-btn.ghost { background: var(--bg-input); color: var(--fg); }
  .bi-bar { height: 4px; border-radius: 2px; background: var(--bg-hover); overflow: hidden; }
  .bi-bar i { display: block; height: 100%; background: #1f6fff; transition: width 0.25s; }
</style>
