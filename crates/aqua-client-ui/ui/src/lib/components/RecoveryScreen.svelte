<script lang="ts">
  // The data of this account can't be opened (Rust: vault_key.rs). Nothing
  // else of the app runs; nothing is deleted from here.
  import { onMount } from "svelte";
  import { api, type VaultBoot } from "../tauri";
  import { t, tr } from "../i18n";
  import { initTheme } from "../theme";
  import { accounts, loadAccounts, switchAccount } from "../accounts";

  let { info }: { info: VaultBoot } = $props();

  let step = $state<"" | "words" | "aside">("");
  let words = $state("");
  let busy = $state(false);
  let error = $state("");
  let details = $state(false);

  onMount(() => {
    initTheme();
    void loadAccounts();
  });

  async function run(act: () => Promise<unknown>) {
    busy = true;
    error = "";
    try { await act(); } catch (e) { error = tr(String(e)); }
    busy = false;
  }
</script>

<main>
  <div class="card">
    <h1>{$t("This device's data can't be opened")}</h1>

    {#if step === "words"}
      <p>{$t("Enter the 24 words of your recovery key.")}</p>
      <textarea rows="4" bind:value={words} autocapitalize="off" autocomplete="off" spellcheck="false" aria-label={$t("Recovery key")}></textarea>
      <button class="primary" disabled={busy || words.trim().split(/\s+/).length !== 24} onclick={() => run(() => api.vaultUnlockPhrase(words))}>{$t("Open")}</button>
      <button disabled={busy} onclick={() => { step = ""; error = ""; }}>{$t("Back")}</button>
    {:else if step === "aside"}
      <p>{$t("The data that can't be opened is moved aside, not deleted. Then sign in to your account again and restore the chats from your backup or your other devices.")}</p>
      <button class="primary" disabled={busy} onclick={() => run(() => api.vaultSetAside())}>{$t("Start over")}</button>
      <button disabled={busy} onclick={() => { step = ""; error = ""; }}>{$t("Cancel")}</button>
    {:else}
      <p>{$t("The system didn't give out the key that protects the data. Nothing has been deleted.")}</p>
      <button class="primary" disabled={busy} onclick={() => run(() => api.appRelaunch())}>{$t("Retry")}</button>
      {#if info.has_recovery}
        <button disabled={busy} onclick={() => { step = "words"; }}>{$t("Enter the recovery key")}</button>
      {/if}
      <button disabled={busy} onclick={() => { step = "aside"; }}>{$t("Sign in again and restore")}</button>
      {#each $accounts.filter((a) => !a.active && a.has_keys) as a (a.slot)}
        <button disabled={busy} onclick={() => run(() => switchAccount(a.slot))}>{$t("Switch to {name}", { name: a.username ?? a.last ?? $t("Another account") })}</button>
      {/each}
      <button class="link" onclick={() => { details = !details; }}>{$t("Details")}</button>
      {#if details}<pre>{info.reason}</pre>{/if}
    {/if}

    {#if error}<p class="error" role="alert">{error}</p>{/if}
  </div>
</main>

<style>
  main {
    height: 100%; overflow-y: auto; display: flex;
    padding: calc(24px + env(safe-area-inset-top)) 20px calc(24px + env(safe-area-inset-bottom));
    box-sizing: border-box; background: var(--bg-shell); color: var(--fg);
  }
  .card { margin: auto; width: 100%; max-width: 360px; display: flex; flex-direction: column; gap: 10px; }
  h1 { font-size: 19px; margin: 0; }
  p { margin: 0 0 6px; font-size: 14px; line-height: 1.45; }
  button {
    background: var(--bg-input); color: var(--fg); border: 1px solid var(--border-strong);
    border-radius: calc(10px * var(--rk, 1)); padding: 11px 14px; font: inherit; font-size: 14px; font-weight: 600; cursor: pointer;
  }
  button.primary { background: var(--accent); border-color: var(--accent); color: var(--bg-shell); }
  button.link { background: none; border: none; font-weight: 400; opacity: .7; }
  button:disabled { opacity: .5; cursor: default; }
  textarea {
    resize: vertical; background: var(--bg-input); color: var(--fg); border: 1px solid var(--border-strong);
    border-radius: calc(10px * var(--rk, 1)); padding: 10px 12px; font: inherit; font-size: 14px;
  }
  pre { margin: 0; white-space: pre-wrap; word-break: break-word; font-size: 11.5px; opacity: .7; user-select: text; }
  .error { color: var(--danger); }
</style>
