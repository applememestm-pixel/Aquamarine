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
  import { t } from "../i18n";

  let pass = $state("");
  let pass2 = $state("");
  let autoMin = $state(5);
  let err = $state("");
  let busy = $state(false);
  let shake = $state(false);
  let input: HTMLInputElement | undefined = $state();

  const AUTO: [number, string][] = [[1, "1 min"], [5, "5 min"], [15, "15 min"], [60, "1 hour"], [0, "Off"]];

  // Signed in but no passcode yet → ask for one (once the sign-in window closes).
  $effect(() => {
    if ($info?.registered && !$wizardOpen && !$lockConf?.hash && !$lockConf?.skipped) passcodeSetupOpen.set(true);
  });
  $effect(() => {
    if ($locked || $passcodeSetupOpen) { pass = ""; pass2 = ""; err = ""; void tick().then(() => input?.focus()); }
  });

  async function doUnlock() {
    if (!pass || busy) return;
    busy = true;
    const ok = await unlock(pass);
    busy = false;
    if (!ok) { err = $t("Wrong passcode"); pass = ""; shake = true; setTimeout(() => (shake = false), 450); }
  }
  async function doSetup() {
    if (pass.length < 4) { err = $t("At least 4 characters"); return; }
    if (pass !== pass2) { err = $t("Passcodes don't match"); return; }
    busy = true;
    await setPasscode(pass, autoMin);
    busy = false;
    passcodeSetupOpen.set(false);
  }
  async function forgot() {
    // Like Telegram: the only way past a forgotten passcode is to sign out.
    try { info.set(await api.logOut()); } catch { /* ignore */ }
    lockConf.set(null);
    locked.set(false);
    passcodeSetupOpen.set(false);
    wizardOpen.set(true);
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
      <div class="ava">
        {#if $profile.avatar_url}<img src={$profile.avatar_url} alt="" />{:else}<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="4" y="11" width="16" height="10" rx="2"/><path d="M8 11V7a4 4 0 0 1 8 0v4"/></svg>{/if}
      </div>
      {#if $locked}
        <h2>{$t("Aquamarine is locked")}</h2>
        <p>{$info?.username ? "@" + $info.username : ""}</p>
        <input bind:this={input} type="password" bind:value={pass} placeholder={$t("Passcode")} onkeydown={(e) => e.key === "Enter" && doUnlock()} />
        {#if err}<div class="err">{err}</div>{/if}
        <button class="primary" onclick={doUnlock} disabled={busy}>{$t("Unlock")}</button>
        <button class="link" onclick={forgot}>{$t("Forgot passcode? Sign out")}</button>
      {:else}
        <h2>{$t("Set a passcode")}</h2>
        <p>{$t("Protects this device: Aquamarine asks for it on start and after you've been away.")}</p>
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
    width: min(360px, 92vw); display: flex; flex-direction: column; align-items: center; gap: 10px;
    padding: 28px 26px; border-radius: 20px; background: var(--bg-shell); border: 1px solid var(--border-strong);
    box-shadow: 0 30px 80px -20px rgba(0, 0, 0, 0.7), 0 0 40px -18px var(--accent-glow);
  }
  .shake { animation: shake 0.42s; }
  @keyframes shake { 20%, 60% { transform: translateX(-8px); } 40%, 80% { transform: translateX(8px); } }
  .ava {
    width: 72px; height: 72px; border-radius: 50%; overflow: hidden; display: grid; place-items: center;
    background: linear-gradient(135deg, var(--accent), var(--accent-2)); color: #06070c; margin-bottom: 4px;
    box-shadow: 0 0 24px -4px var(--accent-glow);
  }
  .ava img { width: 100%; height: 100%; object-fit: cover; }
  .ava svg { width: 30px; height: 30px; }
  h2 { margin: 0; font-size: 18px; font-weight: 800; color: var(--fg); }
  p { margin: 0 0 6px; font-size: 12.5px; color: var(--fg-muted); text-align: center; line-height: 1.5; }
  input {
    width: 100%; box-sizing: border-box; background: var(--bg-input); border: 1px solid var(--border); border-radius: 10px;
    color: var(--fg); font: inherit; font-size: 15px; padding: 11px 14px; outline: none; text-align: center; letter-spacing: 0.2em;
  }
  input:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  .err { color: #ff8f8f; font-size: 12.5px; }
  .primary {
    width: 100%; border: none; border-radius: 10px; padding: 11px; font: inherit; font-weight: 800; font-size: 14px; cursor: pointer;
    background: linear-gradient(135deg, var(--accent), var(--accent-2)); color: #06070c; margin-top: 4px;
  }
  .primary:disabled { opacity: 0.6; }
  .link { background: none; border: none; color: var(--fg-dim); font: inherit; font-size: 12px; cursor: pointer; }
  .link:hover { color: var(--accent); }
  .auto { width: 100%; display: flex; flex-direction: column; gap: 6px; margin-top: 2px; }
  .auto > span { font-size: 11px; font-weight: 700; color: var(--fg-dim); text-transform: uppercase; letter-spacing: 0.06em; }
  .chips { display: flex; flex-wrap: wrap; gap: 5px; }
  .chips button { border: 1px solid var(--border); background: var(--bg-input); color: var(--fg-muted); border-radius: 100px; padding: 5px 10px; font: inherit; font-size: 12px; font-weight: 600; cursor: pointer; }
  .chips button.on { border-color: var(--accent); color: var(--accent); background: var(--accent-soft); }
  .hint { font-size: 11.5px; color: var(--fg-dim); }
</style>
