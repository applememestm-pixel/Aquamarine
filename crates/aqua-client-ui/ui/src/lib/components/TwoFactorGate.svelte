<script module lang="ts">
  import { writable } from "svelte/store";
  interface Need { server: string; resolve: () => void }
  const need = writable<Need | null>(null);
  /**
   * The server requires two-step codes (1.2.0) and this account has none:
   * it won't let the app connect until they're set up. Resolves once they
   * are; `connect()` (stores.ts) then connects again.
   */
  export function requireTwoFactor(server: string): Promise<void> {
    return new Promise((resolve) => need.set({ server, resolve }));
  }
</script>

<script lang="ts">
  import { api } from "../tauri";
  import { t, tr } from "../i18n";
  import QrCode from "./QrCode.svelte";

  let setup = $state<{ secret: string; uri: string } | null>(null);
  let code = $state("");
  let err = $state<string | null>(null);
  let busy = $state(false);
  let codes = $state<string[] | null>(null);
  let saved = $state(false);

  function msg(e: unknown): string {
    const m = String(e);
    if (/wrong two-step/i.test(m)) return tr("Wrong code — check the time on your phone and try the next one");
    return m;
  }

  async function start(server: string) {
    err = null; busy = true;
    try { setup = await api.totpSetup(server); } catch (e) { err = msg(e); }
    busy = false;
  }
  // A fresh secret each time the gate opens.
  $effect(() => { if ($need && !setup && !codes && !busy && !err) void start($need.server); });

  async function enable() {
    if (!$need) return;
    busy = true; err = null;
    try { codes = (await api.totpEnable(code.trim(), $need.server)).backup; saved = false; }
    catch (e) { err = msg(e); }
    busy = false;
  }
  function done() {
    const n = $need;
    need.set(null);
    setup = null; codes = null; code = ""; err = null;
    n?.resolve();
  }
  async function copy(text: string) {
    try { await navigator.clipboard.writeText(text); } catch { /* no clipboard */ }
  }
</script>

{#if $need}
  <div class="tf-back fullscreen-layer">
    <div class="tf" role="dialog" aria-modal="true" data-own-back aria-label={$t("Turn on two-step verification")}>
      <div class="tf-ic" aria-hidden="true">
        <svg viewBox="0 0 24 24" width="30" height="30" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3l7 3v5c0 4.5-3 8.5-7 10-4-1.5-7-5.5-7-10V6z"/><path d="M9 12l2 2 4-4"/></svg>
      </div>
      {#if codes}
        <h3>{$t("Backup codes")}</h3>
        <p>{$t("Each one works once instead of a code from the app — for when the phone with it is lost. Keep them somewhere safe; they're shown only now.")}</p>
        <ol class="tf-codes">{#each codes as c (c)}<li>{c}</li>{/each}</ol>
        <button class="tf-link" onclick={() => copy(codes!.join("\n"))}>{$t("Copy")}</button>
        <label class="tf-ok"><input type="checkbox" bind:checked={saved} /> {$t("I've saved them")}</label>
        <button class="tf-btn" disabled={!saved} onclick={done}>{$t("Continue")}</button>
      {:else}
        <h3>{$t("Turn on two-step verification")}</h3>
        <p>{$t("Since 1.2.0 every account needs it: whoever learns your password still can't sign in without a code from your phone. Until it's on, this server doesn't let the app connect.")}</p>
        <p class="dim">{$t("You need an authenticator app: Google Authenticator, Aegis, 2FAS, Bitwarden — any of them.")}</p>
        {#if setup}
          <p class="step">{$t("1. Scan the code in the authenticator app (or add the key by hand).")}</p>
          <div class="tf-qr"><QrCode text={setup.uri} size={180} color="#0b1220" letter="2" /></div>
          <button class="tf-key" onclick={() => copy(setup!.secret)} title={$t("Copy")}>{setup.secret.replace(/(.{4})/g, "$1 ").trim()}</button>
          <p class="step">{$t("2. Enter the 6-digit code it shows.")}</p>
          <input class="tf-code" bind:value={code} placeholder="123456" inputmode="numeric" autocomplete="one-time-code" maxlength="7"
                 onkeydown={(e) => e.key === "Enter" && code.replace(/\D/g, "").length === 6 && enable()} />
          <button class="tf-btn" disabled={busy || code.replace(/\D/g, "").length !== 6} onclick={enable}>{$t(busy ? "Checking…" : "Turn on")}</button>
        {:else if busy}
          <p class="dim">…</p>
        {/if}
        {#if err}
          <div class="tf-err">{err}</div>
          {#if !setup}<button class="tf-btn" onclick={() => { err = null; void start($need!.server); }}>{$t("Try again")}</button>{/if}
        {/if}
      {/if}
    </div>
  </div>
{/if}

<style>
  .tf-back { position: fixed; inset: 0; z-index: 10200; display: grid; place-items: center; overflow-y: auto; padding: 16px; background: color-mix(in srgb, var(--bg-base, #06070c) 86%, transparent); backdrop-filter: blur(8px); }
  .tf { width: min(420px, 100%); padding: 22px 20px 18px; border-radius: calc(18px * var(--rk, 1)); background: var(--bg-shell); border: 1px solid var(--border-strong); box-shadow: 0 24px 60px rgba(0, 0, 0, 0.5); display: flex; flex-direction: column; align-items: center; text-align: center; gap: 10px; }
  .tf-ic { width: 56px; height: 56px; border-radius: 50%; display: grid; place-items: center; background: color-mix(in srgb, var(--accent) 16%, transparent); color: var(--accent); }
  h3 { margin: 2px 0 0; font-size: 17px; font-weight: 700; }
  p { margin: 0; font-size: 13.5px; color: var(--fg-muted); line-height: 1.45; }
  p.dim { font-size: 12.5px; color: var(--fg-dim); }
  p.step { align-self: stretch; text-align: left; color: var(--fg); font-size: 13px; }
  .tf-qr { padding: 10px; background: #fff; border-radius: calc(12px * var(--rk, 1)); line-height: 0; }
  .tf-key { border: 1px dashed var(--border-strong); background: none; color: var(--fg); padding: 6px 10px; border-radius: calc(8px * var(--rk, 1)); font-family: var(--font-mono, ui-monospace, monospace); font-size: 13px; cursor: pointer; user-select: text; word-break: break-all; }
  .tf-code { width: 150px; text-align: center; font-family: var(--font-mono, ui-monospace, monospace); font-size: 20px; letter-spacing: 0.2em; padding: 8px; border-radius: calc(10px * var(--rk, 1)); border: 1px solid var(--border); background: var(--bg-input); color: var(--fg); }
  .tf-code:focus { outline: none; border-color: var(--accent); }
  .tf-btn { align-self: stretch; min-height: 42px; border: none; border-radius: calc(12px * var(--rk, 1)); background: var(--accent); color: #06070c; font: inherit; font-weight: 700; font-size: 14px; cursor: pointer; }
  .tf-btn:disabled { opacity: 0.55; cursor: default; }
  .tf-err { color: #e5484d; font-size: 12.5px; }
  .tf-codes { list-style: none; margin: 0; padding: 12px; display: grid; grid-template-columns: repeat(2, 1fr); gap: 4px 14px; font-family: var(--font-mono, ui-monospace, monospace); font-size: 14px; user-select: text; background: var(--bg-input); border-radius: calc(12px * var(--rk, 1)); border: 1px solid var(--border); align-self: stretch; }
  .tf-link { border: none; background: none; color: var(--accent); font: inherit; font-weight: 600; font-size: 13px; cursor: pointer; }
  .tf-ok { display: flex; align-items: center; gap: 8px; font-size: 13px; cursor: pointer; }
</style>
