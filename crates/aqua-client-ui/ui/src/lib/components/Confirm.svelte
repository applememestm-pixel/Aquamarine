<script module lang="ts">
  import { writable, get as getStore } from "svelte/store";
  interface Ask { title: string; text?: string; ok?: string; danger?: boolean; check?: string; checked?: boolean; resolve: (v: boolean) => void }
  const pending = writable<Ask | null>(null);
  const checkState = writable(false);
  /** Same dialog with one checkbox ("Also delete for Alice"). */
  export function askConfirmCheck(o: { title: string; text?: string; ok?: string; danger?: boolean; check: string; checked?: boolean }): Promise<{ ok: boolean; checked: boolean }> {
    checkState.set(o.checked ?? false);
    return new Promise((resolve) => pending.set({ ...o, resolve: (v) => resolve({ ok: v, checked: v && getStore(checkState) }) }));
  }
  /** In-app confirm dialog (no browser / GTK dialog). Enter = yes, Esc = no. */
  export function askConfirm(o: { title: string; text?: string; ok?: string; danger?: boolean }): Promise<boolean> {
    return new Promise((resolve) => pending.set({ ...o, resolve }));
  }
</script>

<script lang="ts">
  import { t } from "../i18n";
  import { tick } from "svelte";
  let okBtn = $state<HTMLButtonElement | null>(null);
  function done(v: boolean) {
    $pending?.resolve(v);
    pending.set(null);
  }
  $effect(() => { if ($pending) void tick().then(() => okBtn?.focus()); });
  function key(e: KeyboardEvent) {
    if (!$pending) return;
    if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); done(false); }
    else if (e.key === "Enter") { e.preventDefault(); e.stopPropagation(); done(true); }
  }
</script>

<svelte:window onkeydowncapture={key} />

{#if $pending}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="cf-back" onclick={() => done(false)}>
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div class="cf" tabindex="-1" role="alertdialog" aria-modal="true" aria-label={$pending.title} onclick={(e) => e.stopPropagation()}>
      <h3>{$pending.title}</h3>
      {#if $pending.text}<p>{$pending.text}</p>{/if}
      {#if $pending.check}
        <label class="cf-check"><input type="checkbox" bind:checked={$checkState} /> <span>{$pending.check}</span></label>
      {/if}
      <div class="cf-row">
        <button class="cf-btn" onclick={() => done(false)}>{$t("Cancel")}</button>
        <button class="cf-btn ok" class:danger={$pending.danger} bind:this={okBtn} onclick={() => done(true)}>{$pending.ok ?? $t("OK")}</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .cf-back {
    position: fixed; inset: 0; z-index: 10100;
    display: grid; place-items: center;
    background: rgba(0, 0, 0, 0.5);
  }
  .cf {
    width: min(380px, 92vw);
    padding: 20px 20px 16px;
    border-radius: calc(14px * var(--rk, 1));
    background: var(--bg-shell);
    border: 1px solid var(--border-strong);
    box-shadow: 0 24px 60px rgba(0, 0, 0, 0.5);
  }
  h3 { margin: 0 0 6px; font-size: 16px; font-weight: 700; }
  p { margin: 0 0 4px; font-size: 13.5px; color: var(--fg-muted); line-height: 1.45; }
  .cf-row { display: flex; justify-content: flex-end; gap: 8px; margin-top: 16px; }
  .cf-btn {
    height: 34px; padding: 0 16px; border-radius: calc(9px * var(--rk, 1));
    border: 1px solid var(--border-strong); background: transparent;
    color: var(--fg); font-size: 13.5px; font-weight: 600; cursor: pointer;
  }
  .cf-btn:hover { background: var(--bg-hover); }
  .cf-btn.ok { border-color: transparent; background: var(--accent); color: #06070c; }
  .cf-btn.ok.danger { background: #e5484d; color: #fff; }
  .cf-btn:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .cf-check { display: flex; align-items: center; gap: 8px; margin: 2px 0 14px; font-size: 13px; color: var(--fg); cursor: pointer; }
  .cf-check input { width: 16px; height: 16px; accent-color: var(--accent); }
</style>
