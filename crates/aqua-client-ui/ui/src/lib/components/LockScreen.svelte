<script lang="ts">
  /**
   * Passcode lock screen + the "set a passcode" step new accounts go
   * through (Telegram-style local passcode with auto-lock).
   */
  import { onMount, tick } from "svelte";
  import { locked, passcodeSetupOpen, lockConf, setPasscode, unlock, lockNow, initLock } from "../lock";
  import { wizardOpen } from "./RegistrationWizard.svelte";
  import { info } from "../stores";
  import { api } from "../tauri";
  import { profile } from "../profile";
  import { t, tr } from "../i18n";
  import { askConfirm } from "./Confirm.svelte";
  import { showToast } from "./Toast.svelte";

  let pass = $state("");
  let pass2 = $state("");
  let autoMin = $state(5);
  let err = $state("");
  let busy = $state(false);
  let shake = $state(false);
  let input: HTMLInputElement | undefined = $state();
  let showPw = $state(false);
  let myName = $derived($profile.display_name && $profile.display_name !== "You" ? $profile.display_name : ($info?.username ?? ""));
  // "Forgot password": prove it's you with a code sent to the account e-mail.
  let forgotStep = $state<0 | 1>(0);
  let code = $state("");
  let sentTo = $state("");

  const AUTO: [number, string][] = [[1, "1 min"], [5, "5 min"], [15, "15 min"], [60, "1 hour"], [0, "Off"]];

  // Signed in but no passcode yet → ask for one (once the sign-in window closes).
  $effect(() => {
    if ($info?.registered && !$wizardOpen && !$lockConf?.hash && !$lockConf?.skipped) passcodeSetupOpen.set(true);
  });
  $effect(() => {
    if ($locked || $passcodeSetupOpen) { pass = ""; pass2 = ""; err = ""; forgotStep = 0; code = ""; void tick().then(() => input?.focus()); }
  });

  async function doUnlock() {
    if (!pass || busy) return;
    busy = true;
    const ok = await unlock(pass);
    busy = false;
    if (!ok) { err = $t("Wrong password"); pass = ""; shake = true; setTimeout(() => (shake = false), 450); }
  }
  async function doSetup() {
    if (pass.length < 4) { err = $t("At least 4 characters"); return; }
    if (pass !== pass2) { err = $t("Passcodes don't match"); return; }
    busy = true;
    await setPasscode(pass, autoMin);
    busy = false;
    passcodeSetupOpen.set(false);
  }
  /** Sign out of this account on this device (a fresh sign-in restores it from the backup). */
  async function logOutProfile() {
    if (!(await askConfirm({ title: tr("Log out of the profile?"), text: tr("You can sign back in with your e-mail and password — chats come back from your encrypted backup."), ok: tr("Log out"), danger: true }))) return;
    try { info.set(await api.logOut()); } catch { /* ignore */ }
    lockConf.set(null);
    locked.set(false);
    passcodeSetupOpen.set(false);
    wizardOpen.set(true);
  }
  /** Forgot the password: a code to the account e-mail removes it. */
  async function forgot() {
    err = "";
    const email = $info?.email ?? "";
    const server = $info?.server_url ?? "";
    if (!email || !server) {
      err = tr("No e-mail is linked to this account here — log out and sign in again.");
      return;
    }
    busy = true;
    try {
      const r = await api.requestEmailCode(server, email, "signin");
      if (r.dev_code) code = r.dev_code;
      sentTo = email.replace(/^(.).*?(@.*)$/, "$1•••$2");
      forgotStep = 1;
      void tick().then(() => input?.focus());
    } catch (e) { err = tr("Could not send the code: {e}", { e: String(e) }); }
    finally { busy = false; }
  }
  async function checkForgot() {
    if (code.trim().length < 4 || busy) return;
    busy = true;
    try {
      await api.verifyEmailCode($info?.server_url ?? "", $info?.email ?? "", code.trim());
      // Straight in; the old password is gone (a new one can be set in Settings → Privacy).
      lockConf.set({ hash: "", salt: "", autoMin: 0, skipped: true });
      locked.set(false);
      showToast(tr("Signed in. The app password is removed — set a new one in Settings."));
    } catch { err = tr("Wrong or expired code"); code = ""; }
    finally { busy = false; }
  }
  function skipSetup() {
    lockConf.set({ hash: "", salt: "", autoMin: 0, skipped: true });
    passcodeSetupOpen.set(false);
  }

  onMount(() => {
    initLock();
    const k = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.code === "KeyL" && !$locked) { e.preventDefault(); lockNow(); }
    };
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  });
</script>

{#if $locked || $passcodeSetupOpen}
  <div class="lock" role="dialog" aria-modal="true">
    <div class="card" class:shake>
      <div class="banner" style:background={$profile.banner_url ? `center / cover no-repeat url("${$profile.banner_url}")` : `linear-gradient(135deg, ${$profile.banner_a}, ${$profile.banner_b})`}></div>
      <div class="ava">
        {#if $profile.avatar_url}<img src={$profile.avatar_url} alt="" />{:else}<span>{(myName || "?").charAt(0).toUpperCase()}</span>{/if}
      </div>
      <div class="who">
        <b>{myName}</b>
        {#if $info?.username}<span>@{$info.username}</span>{/if}
      </div>
      {#if $locked && forgotStep === 0}
        <form class="form" onsubmit={(e) => { e.preventDefault(); void doUnlock(); }}>
          <div class="pw">
            <input bind:this={input} type={showPw ? "text" : "password"} bind:value={pass} placeholder={$t("Password")} autocomplete="current-password" aria-label={$t("Password")} />
            <button type="button" class="eye" onclick={() => (showPw = !showPw)} aria-label={$t(showPw ? "Hide" : "Show")}>
              {#if showPw}<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M17.94 17.94A10 10 0 0 1 12 20c-7 0-11-8-11-8a18.5 18.5 0 0 1 5.06-5.94"/><path d="M9.9 4.24A9 9 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19"/><line x1="1" y1="1" x2="23" y2="23"/></svg>{:else}<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/><circle cx="12" cy="12" r="3"/></svg>{/if}
            </button>
          </div>
          {#if err}<div class="err">{err}</div>{/if}
          <button class="primary" type="submit" disabled={busy || !pass}>{busy ? "…" : $t("Sign in")}</button>
        </form>
        <div class="pair">
          <button class="ghost" onclick={forgot} disabled={busy}>{$t("Forgot password")}</button>
          <button class="ghost danger" onclick={logOutProfile}>{$t("Log out of the profile")}</button>
        </div>
      {:else if $locked}
        <p>{tr("We sent a code to {e}. Enter it to sign in without the password.", { e: sentTo })}</p>
        <form class="form" onsubmit={(e) => { e.preventDefault(); void checkForgot(); }}>
          <input bind:this={input} class="code" inputmode="numeric" maxlength="8" bind:value={code} placeholder="••••••" aria-label={$t("Code")} />
          {#if err}<div class="err">{err}</div>{/if}
          <button class="primary" type="submit" disabled={busy || code.trim().length < 4}>{busy ? "…" : $t("Sign in")}</button>
        </form>
        <div class="pair">
          <button class="ghost" onclick={() => { forgotStep = 0; err = ""; }}>{$t("Back")}</button>
          <button class="ghost danger" onclick={logOutProfile}>{$t("Log out of the profile")}</button>
        </div>
      {:else}
        <h2>{$t("Set a passcode")}</h2>
        <p>{$t("Protects this device: Aquamarine asks for it on start and after you've been away.")}</p>
        <div class="form">
          <input bind:this={input} type="password" bind:value={pass} placeholder={$t("Passcode")} autocomplete="new-password" />
          <input type="password" bind:value={pass2} placeholder={$t("Repeat passcode")} autocomplete="new-password" onkeydown={(e) => e.key === "Enter" && doSetup()} />
          <div class="auto">
            <span>{$t("Auto-lock")}</span>
            <div class="chips">
              {#each AUTO as [m, l] (m)}<button class:on={autoMin === m} onclick={() => (autoMin = m)}>{$t(l)}</button>{/each}
            </div>
          </div>
          {#if err}<div class="err">{err}</div>{/if}
          <button class="primary" onclick={doSetup} disabled={busy}>{$t("Save passcode")}</button>
        </div>
        <div class="pair"><button class="ghost" onclick={skipSetup}>{$t("Not now")}</button></div>
        <div class="hint">{$t("Ctrl+L locks the app any time.")}</div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .lock {
    position: fixed; inset: var(--tb-h, 0px) 0 0 0; z-index: 390; display: grid; place-items: center;
    background:
      radial-gradient(900px 500px at 20% 0%, var(--accent-glow), transparent 60%),
      radial-gradient(800px 500px at 90% 100%, color-mix(in srgb, var(--accent-2) 25%, transparent), transparent 60%),
      var(--bg-void, #06070c);
  }
  .card {
    position: relative; width: min(380px, 92vw); display: flex; flex-direction: column; align-items: center; gap: 10px;
    padding: 0 24px 22px; border-radius: 22px; background: var(--bg-shell); border: 1px solid var(--border-strong); overflow: hidden;
    box-shadow: 0 30px 80px -20px rgba(0, 0, 0, 0.7), 0 0 40px -18px var(--accent-glow);
    animation: card-in 0.35s cubic-bezier(.2,.9,.3,1.1);
  }
  @keyframes card-in { from { opacity: 0; transform: translateY(14px) scale(0.98); } }
  .banner { align-self: stretch; height: 96px; margin: 0 -24px; }
  .shake { animation: shake 0.42s; }
  @keyframes shake { 20%, 60% { transform: translateX(-8px); } 40%, 80% { transform: translateX(8px); } }
  .ava {
    width: 92px; height: 92px; border-radius: 50%; overflow: hidden; display: grid; place-items: center; margin-top: -50px;
    background: linear-gradient(135deg, var(--accent), var(--accent-2)); color: #06070c; font-size: 36px; font-weight: 900;
    border: 5px solid var(--bg-shell); box-shadow: 0 0 24px -4px var(--accent-glow);
  }
  .ava img { width: 100%; height: 100%; object-fit: cover; }
  .who { display: flex; flex-direction: column; align-items: center; gap: 1px; margin-bottom: 6px; }
  .who b { font-size: 19px; font-weight: 800; color: var(--fg); }
  .who span { font-size: 13px; color: var(--fg-muted); }
  .form { width: 100%; display: flex; flex-direction: column; gap: 10px; }
  .pw { position: relative; }
  .pw input { padding-right: 44px; }
  .eye { position: absolute; right: 6px; top: 50%; transform: translateY(-50%); width: 34px; height: 34px; border: none; border-radius: 8px; background: none; color: var(--fg-dim); cursor: pointer; display: grid; place-items: center; }
  .eye:hover { color: var(--fg); }
  .eye svg { width: 18px; height: 18px; }
  .code { letter-spacing: 0.5em; font-size: 20px; font-weight: 800; }
  .pair { width: 100%; display: flex; gap: 8px; margin-top: 4px; }
  .ghost { flex: 1; border: 1px solid var(--border); background: var(--bg-input); color: var(--fg-muted); border-radius: 10px; padding: 9px 8px; font: inherit; font-size: 12.5px; font-weight: 700; cursor: pointer; transition: color .15s, border-color .15s; }
  .ghost:hover { color: var(--accent); border-color: color-mix(in srgb, var(--accent) 45%, transparent); }
  .ghost.danger:hover { color: #ff8f8f; border-color: color-mix(in srgb, #ff5c5c 45%, transparent); }
  .ghost:disabled { opacity: 0.6; }
  h2 { margin: 0; font-size: 18px; font-weight: 800; color: var(--fg); }
  p { margin: 0 0 6px; font-size: 12.5px; color: var(--fg-muted); text-align: center; line-height: 1.5; }
  input {
    width: 100%; box-sizing: border-box; background: var(--bg-input); border: 1px solid var(--border); border-radius: 10px;
    color: var(--fg); font: inherit; font-size: 15px; padding: 12px 14px; outline: none; text-align: center;
  }
  input:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  .err { color: #ff8f8f; font-size: 12.5px; }
  .primary {
    width: 100%; border: none; border-radius: 10px; padding: 11px; font: inherit; font-weight: 800; font-size: 14px; cursor: pointer;
    background: linear-gradient(135deg, var(--accent), var(--accent-2)); color: #06070c; margin-top: 4px;
  }
  .primary:disabled { opacity: 0.6; }
  .auto { width: 100%; display: flex; flex-direction: column; gap: 6px; margin-top: 2px; }
  .auto > span { font-size: 11px; font-weight: 700; color: var(--fg-dim); text-transform: uppercase; letter-spacing: 0.06em; }
  .chips { display: flex; flex-wrap: wrap; gap: 5px; }
  .chips button { border: 1px solid var(--border); background: var(--bg-input); color: var(--fg-muted); border-radius: 100px; padding: 5px 10px; font: inherit; font-size: 12px; font-weight: 600; cursor: pointer; }
  .chips button.on { border-color: var(--accent); color: var(--accent); background: var(--accent-soft); }
  .hint { font-size: 11.5px; color: var(--fg-dim); }
</style>
