<script lang="ts">
  /**
   * Settings → Privacy & Security → Sign-in protection: two-step codes
   * (TOTP, any authenticator app) and "sign-in on new devices" — both kept
   * and enforced by the relay (aqua-server/src/totp.rs, docs/accounts.md).
   */
  import { onMount } from "svelte";
  import { api, type SecurityState } from "../tauri";
  import { t, tr } from "../i18n";
  import { showToast } from "./Toast.svelte";
  import { askInput } from "./InputPrompt.svelte";
  import QrCode from "./QrCode.svelte";

  let sec = $state<SecurityState | null>(null);
  let err = $state<string | null>(null);
  /** Setting up: the secret for the app, then the first code. */
  let setup = $state<{ secret: string; uri: string } | null>(null);
  let setupCode = $state("");
  let setupErr = $state<string | null>(null);
  let busy = $state(false);
  /** Backup codes to show once (after turning on or making new ones). */
  let codes = $state<string[] | null>(null);
  let saved = $state(false);

  function msg(e: unknown): string {
    const m = String(e);
    if (/wrong two-step/i.test(m)) return tr("Wrong code — check the time on your phone and try the next one");
    if (/code required/i.test(m)) return tr("Enter the code from the app");
    if (/needs updating/i.test(m)) return tr("This server doesn't support it yet — it needs updating");
    if (/not connected/i.test(m)) return tr("Not connected to a server");
    return m;
  }

  async function load() {
    try { sec = await api.securityGet(); err = null; } catch (e) { err = msg(e); }
  }
  onMount(() => { void load(); });

  async function startSetup() {
    busy = true;
    try { setup = await api.totpSetup(); setupCode = ""; setupErr = null; } catch (e) { showToast(msg(e)); }
    busy = false;
  }
  async function finishSetup() {
    if (!setup) return;
    busy = true;
    setupErr = null;
    try {
      const r = await api.totpEnable(setupCode.trim());
      setup = null;
      codes = r.backup; saved = false;
      await load();
    } catch (e) { setupErr = msg(e); }
    busy = false;
  }
  function askCode(title: string, description: string, submit: string, run: (code: string) => Promise<void>) {
    askInput({
      title, description, submitLabel: submit,
      fields: [{ label: tr("Code from the app or a backup code"), placeholder: "123456", monospace: true }],
      onSubmit: async ([code]) => {
        if (!code.trim()) return tr("Enter the code from the app");
        try { await run(code.trim()); return null; } catch (e) { return msg(e); }
      },
    });
  }
  function turnOff() {
    askCode(tr("Turn off two-step verification?"), tr("A new device will need only the e-mail code and the password again."), tr("Turn off"), async (c) => {
      await api.totpDisable(c);
      showToast(tr("Two-step verification is off"));
      await load();
    });
  }
  function newCodes() {
    askCode(tr("New backup codes"), tr("The old ones stop working."), tr("Make new codes"), async (c) => {
      codes = (await api.totpBackup(c)).backup; saved = false;
      await load();
    });
  }
  /** New phone: the codes move to another authenticator app (a current
   *  code or a backup code first; the old app works until the new one is
   *  confirmed). */
  function moveApp() {
    askCode(tr("Move the codes to another app?"), tr("Enter a code from the current app (or a backup code). The current app keeps working until the new one is set up."), tr("Continue"), async (c) => {
      setup = await api.totpSetup(undefined, c); setupCode = ""; setupErr = null;
    });
  }
  async function setLock(lock: boolean) {
    if (!sec) return;
    if (!lock && sec.totp) {
      askCode(tr("Allow sign-in on new devices?"), tr("Enter a code: turning this back on needs it."), tr("Allow"), async (c) => {
        sec = await api.securitySetLock(false, c);
      });
      return;
    }
    busy = true;
    try { sec = await api.securitySetLock(lock); showToast(lock ? tr("Nobody can sign in on a new device now") : tr("New devices can sign in again")); }
    catch (e) { showToast(msg(e)); }
    busy = false;
  }
  async function copy(text: string) {
    try { await navigator.clipboard.writeText(text); showToast(tr("Copied")); } catch { /* no clipboard */ }
  }
</script>

<section class="group">
  <div class="label">{$t("Sign-in protection")}</div>
  {#if err && !sec}
    <div class="foot-note">{err}</div>
  {:else if !sec}
    <div class="foot-note">…</div>
  {:else}
    <button class="toggle-row" disabled={busy} onclick={() => (sec?.totp ? (sec.required ? showToast(tr("This server requires two-step verification — it can't be turned off")) : turnOff()) : setup ? (setup = null) : startSetup())}>
      <div class="tr-text">
        <div class="tr-title">{$t("Two-step verification")}</div>
        <div class="tr-sub">{$t("On a new device, after the e-mail code and the password, the account asks for a 6-digit code from an authenticator app — Google Authenticator, Aegis, 2FAS, Bitwarden…")}</div>
      </div>
      <span class="switch" class:on={sec.totp || !!setup}></span>
    </button>

    {#if setup}
      <div class="sp-setup">
        <p>{$t("1. Scan the code in the authenticator app (or add the key by hand).")}</p>
        <div class="sp-qr"><QrCode text={setup.uri} size={190} color="#0b1220" letter="2" /></div>
        <button class="sp-key" onclick={() => copy(setup!.secret)} title={$t("Copy")}>{setup.secret.replace(/(.{4})/g, "$1 ").trim()}</button>
        <p>{$t("2. Enter the 6-digit code it shows.")}</p>
        <div class="sp-row">
          <input class="txt sp-code" bind:value={setupCode} placeholder="123456" inputmode="numeric" autocomplete="one-time-code" maxlength="7"
                 onkeydown={(e) => e.key === "Enter" && finishSetup()} />
          <button class="mini-btn primary" disabled={busy || setupCode.replace(/\D/g, "").length !== 6} onclick={finishSetup}>{$t("Turn on")}</button>
          <button class="mini-btn" onclick={() => (setup = null)}>{$t("Cancel")}</button>
        </div>
        {#if setupErr}<div class="sp-err">{setupErr}</div>{/if}
      </div>
    {/if}

    {#if codes}
      <div class="sp-setup">
        <b>{$t("Backup codes")}</b>
        <p>{$t("Each one works once instead of a code from the app — for when the phone with it is lost. Keep them somewhere safe; they're shown only now.")}</p>
        <ol class="sp-codes">{#each codes as c (c)}<li>{c}</li>{/each}</ol>
        <div class="sp-row">
          <button class="mini-btn" onclick={() => copy(codes!.join("\n"))}>{$t("Copy")}</button>
          <label class="sp-ok"><input type="checkbox" bind:checked={saved} /> {$t("I've saved them")}</label>
          <button class="mini-btn primary" disabled={!saved} onclick={() => (codes = null)}>{$t("Done")}</button>
        </div>
      </div>
    {:else if sec.totp}
      <div class="info-row">
        <span class="v">{$t("Backup codes left: {n}", { n: sec.backup_left })}</span>
        <button class="mini-btn" onclick={newCodes}>{$t("New codes")}</button>
        <button class="mini-btn" onclick={moveApp}>{$t("Another app")}</button>
      </div>
      {#if sec.required}<div class="foot-note">{$t("Required on this server: it stays on. New phone? “Another app” moves the codes to it.")}</div>{/if}
    {/if}

    <button class="toggle-row" disabled={busy} onclick={() => setLock(!sec?.lock)}>
      <div class="tr-text">
        <div class="tr-title">{$t("Sign-in on new devices")}</div>
        <div class="tr-sub">{sec.lock
          ? $t("Off: nobody can sign in to your account on another phone or computer — not even with the password and the code. Your devices keep working. To add one, turn this on for a minute.")
          : $t("On: you can sign in on another phone or computer with your e-mail and password.")}</div>
      </div>
      <span class="switch" class:on={!sec.lock}></span>
    </button>
    <div class="foot-note">{$t("Either way, a new device gets your chats only after you approve it on one of your devices. Lost every device? The 24-word recovery key opens both.")}</div>
  {/if}
</section>

<style>
  /* The look of the other settings rows (SettingsPanel keeps its own scoped). */
  .label { font-size: 12px; font-weight: 700; color: var(--accent); margin-bottom: 10px; }
  .toggle-row { display: flex; align-items: center; justify-content: space-between; gap: 12px; width: 100%; padding: 9px 0; background: transparent; border: none; cursor: pointer; text-align: left; color: inherit; border-bottom: 1px solid var(--border); font: inherit; }
  .toggle-row:disabled { cursor: default; opacity: .7; }
  .tr-text { min-width: 0; }
  .tr-title { font-size: 13.5px; font-weight: 600; color: var(--fg); }
  .tr-sub { font-size: 11.5px; color: var(--fg-dim); margin-top: 2px; line-height: 1.4; }
  .switch { flex-shrink: 0; width: 40px; height: 23px; border-radius: 100px; background: var(--bg-input); border: 1px solid var(--border-strong); position: relative; transition: background 0.15s ease, border-color 0.15s ease; }
  .switch::after { content: ''; position: absolute; top: 2px; left: 2px; width: 17px; height: 17px; border-radius: 50%; background: var(--fg-dim); transition: transform 0.15s ease, background 0.15s ease; }
  .switch.on { background: var(--accent-soft); border-color: var(--accent); }
  .switch.on::after { transform: translateX(17px); background: var(--accent); }
  .mini-btn { flex-shrink: 0; border: 1px solid var(--border-strong); background: var(--bg-input); color: var(--fg); border-radius: calc(8px * var(--rk, 1)); padding: 4px 10px; font-size: 12px; font-weight: 600; cursor: pointer; }
  .mini-btn.primary { background: var(--accent); color: #06070C; border-color: transparent; }
  .mini-btn:disabled { opacity: 0.6; }
  .foot-note { font-size: 12px; color: var(--fg-dim); line-height: 1.55; margin-top: 8px; }
  .info-row { display: flex; align-items: baseline; gap: 10px; padding: 6px 0; font-size: 12.5px; border-bottom: 1px solid var(--border); }
  .info-row .v { flex: 1; color: var(--fg); min-width: 0; }
  .txt { min-width: 0; background: var(--bg-input); color: var(--fg); border: 1px solid var(--border); border-radius: calc(8px * var(--rk, 1)); padding: 7px 9px; font-size: 13px; }
  .sp-setup { display: flex; flex-direction: column; gap: 8px; padding: 12px; margin: 4px 0 8px; border-radius: calc(12px * var(--rk, 1)); background: var(--bg-input); border: 1px solid var(--border); }
  .sp-setup p { margin: 0; font-size: 13px; color: var(--fg-muted); line-height: 1.4; }
  .sp-qr { align-self: center; padding: 10px; background: #fff; border-radius: calc(12px * var(--rk, 1)); line-height: 0; }
  .sp-key { align-self: center; border: 1px dashed var(--border-strong); background: none; color: var(--fg); padding: 6px 10px; border-radius: calc(8px * var(--rk, 1)); font-family: var(--font-mono, ui-monospace, monospace); font-size: 13px; letter-spacing: 0.04em; cursor: pointer; user-select: text; word-break: break-all; }
  .sp-row { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .sp-code { width: 120px; font-family: var(--font-mono, ui-monospace, monospace); font-size: 16px; letter-spacing: 0.15em; text-align: center; }
  .sp-err { color: #e5484d; font-size: 12.5px; }
  .sp-codes { list-style: none; margin: 0; padding: 0; display: grid; grid-template-columns: repeat(2, 1fr); gap: 4px 12px; font-family: var(--font-mono, ui-monospace, monospace); font-size: 14px; user-select: text; }
  .sp-ok { display: flex; align-items: center; gap: 6px; font-size: 13px; cursor: pointer; margin-right: auto; }
</style>
