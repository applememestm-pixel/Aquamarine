<script module lang="ts">
  import { writable } from "svelte/store";
  export const wizardOpen = writable<boolean>(false);
  /** Set once the user explicitly skips, so the entry gate stops
   *  reopening the window for the rest of the session. */
  export const registrationDismissed = writable<boolean>(false);
  export function openRegistration() { wizardOpen.set(true); }
  /** Sign-up in progress: keep the window open after the connection comes
   *  up (the recovery key is shown last). */
  export const wizardHold = writable<boolean>(false);
  export function closeRegistration() { wizardHold.set(false); wizardOpen.set(false); }
</script>

<script lang="ts">
  import { appIconUrl } from "../appIcon";
  import ServerPicker from "./ServerPicker.svelte";
  import { t, tr } from "../i18n";
  import { api } from "../tauri";
  import { info, chats } from "../stores";
  import { profile as localProfile, updateProfile } from "../profile";
  import { connect } from "../stores";
  import { showToast } from "./Toast.svelte";
  import { icons } from "../icons";
  import { applyUiState, refreshBackup } from "../backup";

  // On Android, 127.0.0.1 is the phone itself. The emulator reaches the PC's
  // relay at 10.0.2.2; a real phone needs the PC's LAN IP or a public relay.
  const IS_ANDROID = typeof navigator !== "undefined" && /android/i.test(navigator.userAgent);
  const DEFAULT_SERVER = IS_ANDROID ? "http://10.0.2.2:7777" : "http://127.0.0.1:7777";

  // The self-hosted relay speaks plain HTTP by default. A loopback/LAN
  // address typed with https:// can never connect (TLS handshake fails), so
  // downgrade it automatically — that's almost always a typo, not a real
  // TLS server on a private IP.
  function normalizeServer(raw: string): string {
    let u = raw.trim().replace(/\/+$/, "");
    if (IS_ANDROID) u = u.replace(/^(https?:\/\/)(127\.0\.0\.1|localhost)(?=[:/]|$)/i, "$110.0.2.2");
    const m = u.match(/^https:\/\/(.+)$/i);
    if (m) {
      const host = m[1].split(/[:/]/)[0];
      const isLocal =
        host === "localhost" || host === "127.0.0.1" || host === "::1" ||
        host.startsWith("192.168.") || host.startsWith("10.") ||
        /^172\.(1[6-9]|2[0-9]|3[0-1])\./.test(host);
      if (isLocal) u = "http://" + m[1];
    }
    return u;
  }

  // One-click re-entry for an already-registered account on this device.
  async function continueAs() {
    const i = $info;
    if (!i?.username) return;
    busy = true;
    error = null;
    try {
      const err = await connect(normalizeServer(i.server_url || serverUrl), i.username);
      if (!err) {
        try { chats.set(await api.listChats()); } catch { /* non-fatal */ }
        closeRegistration();
      } else if (/different identity|already claimed|\b403\b|forbidden/i.test(err)) {
        // The relay answers 403 when @name is bound to another device's key
        // (registered from a phone / emulator, or this device's keys were reset).
        // "Sign in" with the account's email + password moves it to this device.
        mode = "signin";
        error = tr("@{name} isn't linked to this device yet (it was signed up or signed in on another one). Sign in below with this account's e-mail and password — this device is added to the account, the others stay signed in.", { name: i.username });
      } else if (/not regist/i.test(err)) {
        error = tr("@{name} isn't registered on this server. Register below to set it up.", { name: i.username });
      } else {
        error = tr("Could not reach the server — is it running at this address?") + `  (${err})`;
      }
    } catch (e) {
      error = `Could not sign in: ${e}`;
    } finally {
      busy = false;
    }
  }

  // Steps: 1 email → 2 code → 3 username+profile → done
  // 4 = recovery key for the backup, 5 = recovery key for a password
  // reset, 6 = "save your recovery key" after sign-up.
  let step = $state<1 | 2 | 3 | 4 | 5 | 6>(1);
  let recoveryWords = $state("");
  let savedWords = $state(false);
  /** Step 4: the backup didn't open with the password — recovery key. */
  let phrase = $state("");
  let serverUrl = $state(DEFAULT_SERVER);
  let email = $state("");
  let code = $state("");
  let devCode = $state<string | null>(null);
  /** The relay couldn't e-mail the code (its mail server refused). */
  let mailFailed = $state<string | null>(null);
  let ticket = $state("");
  let username = $state("");
  let displayName = $state("");
  let about = $state("");
  let phone = $state("");
  let busy = $state(false);
  let error = $state<string | null>(null);
  /** Discord / Spotify style: create an account, or sign in to one. */
  let mode = $state<"signup" | "signin" | "reset">("signup");
  /** The relay said the e-mail already has an account: offer to sign in. */
  let existsHint = $state(false);
  let password = $state("");
  let password2 = $state("");
  let showPw = $state(false);
  const pwScore = $derived(
    (password.length >= 8 ? 1 : 0) + (password.length >= 12 ? 1 : 0) +
    (/[A-ZА-Я]/.test(password) && /[a-zа-я]/.test(password) ? 1 : 0) + (/\d/.test(password) ? 1 : 0) + (/[^\w\s]/.test(password) ? 1 : 0),
  );

  async function sendCode() {
    error = null;
    if (!email.includes("@")) { error = tr("Enter a valid email"); return; }
    if (password.length < 8) { error = tr("Password must be at least 8 characters"); return; }
    if (mode !== "signin" && password !== password2) { error = tr("Passwords don't match"); return; }
    serverUrl = normalizeServer(serverUrl); // fix https→http for local relays, reflect it
    busy = true;
    existsHint = false;
    try {
      const r = await api.requestEmailCode(serverUrl.trim(), email.trim(), mode);
      devCode = r.dev_code; mailFailed = r.delivery_failed ? (r.error || "?") : null;
      step = 2;
      if (devCode) code = devCode; // prefill in dev mode for convenience
      startResendCooldown();
    } catch (e) {
      const m = String(e);
      if (/already exists/i.test(m)) { error = tr("An account with this e-mail already exists."); existsHint = true; }
      else if (/no account with this e-mail/i.test(m)) error = tr("There's no account with this e-mail on this server.");
      else error = tr("Could not send code: {e}", { e: m });
    } finally {
      busy = false;
    }
  }

  /** Forgotten password: e-mail ticket + recovery key → new password, then
   *  the usual sign-in (restoring the backup with the key). */
  async function doReset() {
    error = null;
    busy = true;
    try {
      await api.passwordReset(serverUrl.trim(), ticket, email.trim(), phrase.trim(), password);
      showToast(tr("Password changed"));
      await finishSignIn(!!phrase.trim());
    } catch (e) {
      const m = String(e);
      error = /recovery key/i.test(m) ? tr("This recovery key doesn't match the account. Check the words (they're in the order they were shown).")
        : /expected 24 words|unknown word|checksum/i.test(m) ? tr("The recovery key is mistyped: {e}", { e: m.replace(/^.*recovery key: /, "") })
        : tr("Could not reset the password: {e}", { e: m });
    } finally {
      busy = false;
    }
  }

  async function copyWords() {
    try { await navigator.clipboard.writeText(recoveryWords); showToast(tr("Recovery key copied")); } catch { /* ignore */ }
  }

  // Resend code with a 45-second cooldown.
  let resendIn = $state(0);
  let resendTimer: ReturnType<typeof setInterval> | null = null;
  function startResendCooldown() {
    resendIn = 45;
    if (resendTimer) clearInterval(resendTimer);
    resendTimer = setInterval(() => {
      resendIn -= 1;
      if (resendIn <= 0 && resendTimer) { clearInterval(resendTimer); resendTimer = null; }
    }, 1000);
  }
  async function resendCode() {
    if (resendIn > 0 || busy) return;
    error = null;
    busy = true;
    try {
      const r = await api.requestEmailCode(serverUrl.trim(), email.trim(), mode);
      devCode = r.dev_code; mailFailed = r.delivery_failed ? (r.error || "?") : null;
      if (devCode) code = devCode;
      startResendCooldown();
    } catch (e) {
      error = `Could not resend: ${e}`;
    } finally {
      busy = false;
    }
  }

  async function checkCode() {
    error = null;
    if (code.trim().length < 4) { error = "Enter the 6-digit code"; return; }
    busy = true;
    try {
      ticket = await api.verifyEmailCode(serverUrl.trim(), email.trim(), code.trim());
      if (mode === "signin") { await finishSignIn(); return; }
      if (mode === "reset") { phrase = ""; step = 5; return; }
      step = 3;
    } catch (e) {
      error = `Wrong or expired code: ${e}`;
    } finally {
      busy = false;
    }
  }

  /** Existing account → restore its encrypted backup (if any), move it to
   *  this device, connect, done. */
  async function finishSignIn(withPhrase = false) {
    let restored = false;
    try {
      const r = await api.backupRestore(serverUrl.trim(), ticket, password, email.trim(), withPhrase ? phrase.trim() : undefined);
      if (r.found && r.need_phrase) { step = 4; error = withPhrase ? tr("That recovery key doesn't open the backup") : null; return; }
      if (r.found) { applyUiState(r.ui); restored = true; }
    } catch (e) {
      // No backup / old relay: carry on with a fresh device.
      console.warn("backup restore", e);
    }
    try {
      const u = await api.loginAccount(serverUrl.trim(), ticket, password, email.trim());
      const err = await connect(serverUrl.trim(), u);
      if (err) { error = err; return; }
      try { info.set(await api.completeRegistration(u, email.trim() || null)); } catch { /* non-fatal */ }
      try { chats.set(await api.listChats()); } catch { /* non-fatal */ }
      if (!restored) { try { await api.backupEnable(password); } catch { /* non-fatal */ } }
      void refreshBackup();
      showToast(restored ? tr("Signed in as @{u} — chats restored from your backup", { u }) : tr("Signed in as @{u}", { u }));
      closeRegistration();
      // Restored settings (theme, folders…) apply on a fresh start of the UI.
      if (restored) setTimeout(() => location.reload(), 600);
    } catch (e) {
      const m = String(e);
      error = /wrong email or password/i.test(m) ? tr("Wrong email or password") : tr("Could not sign in: {e}", { e: m });
      if (mode === "reset") mode = "signin";
      step = 1;
    }
  }

  /** Recovery key step: give up on the backup and start fresh. */
  async function skipRestore() {
    phrase = "";
    try {
      const u = await api.loginAccount(serverUrl.trim(), ticket, password, email.trim());
      const err = await connect(serverUrl.trim(), u);
      if (err) { error = err; return; }
      try { info.set(await api.completeRegistration(u, email.trim() || null)); } catch { /* non-fatal */ }
      try { await api.backupEnable(password); } catch { /* non-fatal */ }
      closeRegistration();
    } catch (e) { error = String(e); }
  }

  async function finish() {
    error = null;
    const u = username.trim().toLowerCase();
    if (!/^[a-z0-9_.]{3,32}$/.test(u)) {
      error = "Username: 3–32 chars, a-z 0-9 _ . only";
      return;
    }
    busy = true;
    wizardHold.set(true);
    try {
      // 1. Register the account FIRST (HTTP sign-in + directory write). The
      //    server now requires a registered username before it will accept a
      //    bundle upload or a WebSocket, so this must precede connect.
      await api.registerAccount(serverUrl.trim(), u, ticket, displayName.trim(), about.trim(), phone.trim(), password, email.trim());
      // 2. Connect (signs in + opens WS + uploads bundle), which also
      //    saves the auto-reconnect hints.
      const err = await connect(serverUrl.trim(), u);
      if (err) { error = err; busy = false; wizardHold.set(false); return; }
      // 3. Mirror into the local profile store so the sidebar + profile
      //    modal reflect it immediately.
      updateProfile({
        display_name: displayName.trim() || u,
        about: about.trim() || "An Aquamarine user.",
        phone: phone.trim(),
      });
      // 4. Publish profile to the directory (idempotent with register,
      //    but covers the display-name-only edit path).
      try { await api.publishProfile(displayName.trim() || u, about.trim(), phone.trim()); } catch { /* non-fatal */ }
      // 5. Persist the account record — this is what flips the entry gate
      //    off permanently on this device. Refresh the info store so the
      //    gate ($info.registered) closes immediately.
      try { info.set(await api.completeRegistration(u, email.trim() || null)); } catch { /* non-fatal */ }
      try { chats.set(await api.listChats()); } catch { /* non-fatal */ }
      // Encrypted backups on by default (the password opens them).
      try { await api.backupEnable(password); void refreshBackup(); } catch { /* non-fatal */ }
      showToast(tr("Registered as @{u}", { u }));
      // The 24-word recovery key: shown once now, registered on the relay
      // (only a check derived from it) so a forgotten password can be reset.
      try {
        recoveryWords = await api.recoveryCreate();
        savedWords = false;
        step = 6;
      } catch {
        closeRegistration();
      }
    } catch (e) {
      error = `Registration failed: ${e}`;
      wizardHold.set(false);
    } finally {
      busy = false;
    }
  }

  function skipForNow() {
    // Fallback: no registration this session. Don't nag again until next
    // launch.
    registrationDismissed.set(true);
    closeRegistration();
  }
</script>

{#if $wizardOpen}
  <div class="backdrop">
    <div class="sheet login" role="dialog" aria-modal="true" tabindex="-1">
      <aside class="hero">
        <img class="logo-lg" src={$appIconUrl} alt="" draggable="false" />
        <h1 class="hero-title">{$t("Aquamarine")}</h1>
        <p class="hero-tag">{$t("An open, end-to-end encrypted messenger. Your keys never leave this device.")}</p>
        <ul class="hero-feat">
          <li>{@html icons.lock}<span>{$t("X3DH · Double Ratchet encryption")}</span></li>
          <li><span class="hf-ic">⌁</span><span>{$t("Connect to any server by address — host your own")}</span></li>
          <li><span class="hf-ic">✦</span><span>{$t("Stickers, polls, calls, themes — all yours")}</span></li>
        </ul>
      </aside>

      <div class="panel">
      <div class="steps">
        <span class="dot" class:active={step >= 1} class:done={step > 1}>1</span>
        <span class="bar" class:done={step > 1}></span>
        <span class="dot" class:active={step >= 2} class:done={step > 2}>2</span>
        <span class="bar" class:done={step > 2}></span>
        <span class="dot" class:active={step >= 3}>{step === 6 ? "✓" : 3}</span>
      </div>

      {#if error}
        <div class="err">
          {error}
          {#if existsHint}
            <button class="err-act" onclick={() => { mode = "signin"; error = null; existsHint = false; password2 = ""; }}>{$t("Sign in to it")}</button>
          {/if}
        </div>
      {/if}

      {#if step === 1 && $info?.registered && $info?.username}
        <div class="continue-box">
          <div class="cb-label">{$t("Already registered on this device")}</div>
          <button class="continue-btn" onclick={continueAs} disabled={busy}>
            {busy ? "Signing in…" : `Continue as @${$info.username}`}
          </button>
          <div class="cb-or"><span>{$t("or register / log in below")}</span></div>
        </div>
      {/if}

      {#if step === 1}
        {#if mode === "reset"}
          <div class="reset-head">
            <button class="link" onclick={() => { mode = "signin"; error = null; }}>← {$t("Sign in")}</button>
            <b>{$t("Reset password")}</b>
            <p class="note">{$t("We'll send a code to your e-mail. Then enter your 24-word recovery key and a new password — your chats come back from the encrypted backup.")}</p>
          </div>
        {:else}
          <div class="mode-tabs">
            <button class:on={mode === "signup"} onclick={() => { mode = "signup"; error = null; existsHint = false; }}>{$t("Create account")}</button>
            <button class:on={mode === "signin"} onclick={() => { mode = "signin"; error = null; existsHint = false; }}>{$t("Sign in")}</button>
          </div>
        {/if}
        <div class="form">
          <div class="field">
            <span>{$t("Server")}</span>
            <ServerPicker bind:value={serverUrl} placeholder={DEFAULT_SERVER}
              defaults={IS_ANDROID ? ["http://10.0.2.2:7777"] : ["http://127.0.0.1:7777"]} />
          </div>
          <label class="field">
            <span>{$t("Email")}</span>
            <input bind:value={email} type="email" autocomplete="email" placeholder={$t("you@example.com")} />
          </label>
          <label class="field">
            <span>{mode === "reset" ? $t("New password") : $t("Password")}</span>
            <div class="pw">
              <input bind:value={password} type={showPw ? "text" : "password"} autocomplete={mode === "signin" ? "current-password" : "new-password"}
                     placeholder={mode === "signin" ? "" : $t("At least 8 characters")} onkeydown={(e) => e.key === "Enter" && mode === "signin" && sendCode()} />
              <button class="pw-eye" type="button" onclick={() => (showPw = !showPw)} title={showPw ? $t("Hide") : $t("Show")}>{showPw ? "◉" : "◎"}</button>
            </div>
            {#if mode !== "signin" && password}
              <span class="pw-meter"><i style:width="{pwScore * 20}%" class:ok={pwScore >= 3}></i></span>
            {/if}
            {#if mode === "signin"}
              <button class="link forgot" type="button" onclick={() => { mode = "reset"; error = null; password = ""; password2 = ""; }}>{$t("Forgot password?")}</button>
            {/if}
          </label>
          {#if mode !== "signin"}
            <label class="field">
              <span>{$t("Repeat password")}</span>
              <input bind:value={password2} type={showPw ? "text" : "password"} autocomplete="new-password" onkeydown={(e) => e.key === "Enter" && sendCode()} />
            </label>
          {/if}
          <p class="note">{$t("We'll send a 6-digit verification code. In dev mode the code appears on the next screen automatically.")}</p>
          <div class="actions">
            <button class="ghost" onclick={skipForNow}>{$t("Skip")}</button>
            <button class="primary" onclick={sendCode} disabled={busy}>{$t(busy ? "Sending…" : "Send code")}</button>
          </div>
        </div>
      {:else if step === 2}
        <div class="form">
          <label class="field">
            <span>{$t("Verification code")}</span>
            <input bind:value={code} placeholder="123456" maxlength="6" class="code-input" onkeydown={(e) => e.key === "Enter" && checkCode()} />
          </label>
          {#if devCode}
            <p class="note dev">{$t("Dev mode — your code is")} <b>{devCode}</b> {$t("(also printed in the server console).")}</p>
          {:else if mailFailed}
            <p class="note warn">{$t("The server couldn't send the e-mail ({e}). The code is shown in the server window and in its admin panel — if this is your server, take it from there; otherwise ask the server's owner.", { e: mailFailed })}</p>
          {:else}
            <p class="note">{$t("Check your inbox at {email}.", { email })}</p>
          {/if}
          <button class="resend" onclick={resendCode} disabled={resendIn > 0 || busy}>
            {resendIn > 0 ? $t("Resend code in {n}s", { n: resendIn }) : $t("Resend code")}
          </button>
          <div class="actions">
            <button class="ghost" onclick={() => (step = 1)}>{$t("Back")}</button>
            <button class="primary" onclick={checkCode} disabled={busy}>{$t(busy ? "Verifying…" : "Verify")}</button>
          </div>
        </div>
      {:else if step === 5}
        <div class="form">
          <p class="note">{$t("Enter the 24-word recovery key you saved when you created the account (Settings → Backups → Recovery key shows it on a signed-in device).")}</p>
          <label class="field">
            <span>{$t("Recovery key")}</span>
            <textarea bind:value={phrase} rows="4" spellcheck="false" autocomplete="off" placeholder="word word word …"></textarea>
          </label>
          <p class="note dim">{$t("Never saved one? Leave it empty: if the account has no recovery key, the e-mail confirms it's you — but the encrypted backup can't be opened, chats start fresh.")}</p>
          <div class="actions">
            <button class="ghost" onclick={() => (step = 2)}>{$t("Back")}</button>
            <button class="primary" onclick={doReset} disabled={busy || (phrase.trim() !== "" && phrase.trim().split(/\s+/).length !== 24)}>{$t(busy ? "Checking…" : "Set the new password")}</button>
          </div>
        </div>
      {:else if step === 6}
        <div class="form">
          <b class="rk-title">{$t("Save your recovery key")}</b>
          <p class="note">{$t("These 24 words are the only way back in if you forget the password. Write them down or keep them in a password manager — nobody, not even the server, can restore them.")}</p>
          <ol class="rk-words">
            {#each recoveryWords.split(/\s+/) as w, i (i)}<li><span>{i + 1}</span>{w}</li>{/each}
          </ol>
          <button class="resend" onclick={copyWords}>{$t("Copy")}</button>
          <label class="rk-ok"><input type="checkbox" bind:checked={savedWords} /> {$t("I've saved the recovery key")}</label>
          <div class="actions">
            <button class="primary" onclick={() => { recoveryWords = ""; closeRegistration(); }} disabled={!savedWords}>{$t("Done")}</button>
          </div>
        </div>
      {:else if step === 4}
        <div class="form">
          <p class="note">{$t("This account has an encrypted backup, but your current password doesn't open it (the password was changed after it was made). Enter the 24-word recovery key to restore your chats.")}</p>
          <label class="field">
            <span>{$t("Recovery key")}</span>
            <textarea bind:value={phrase} rows="4" spellcheck="false" placeholder="word word word …"></textarea>
          </label>
          <div class="actions">
            <button class="ghost" onclick={skipRestore} disabled={busy}>{$t("Start without the backup")}</button>
            <button class="primary" onclick={() => finishSignIn(true)} disabled={busy || phrase.trim().split(/\s+/).length !== 24}>{$t("Restore")}</button>
          </div>
        </div>
      {:else}
        <div class="form">
          <label class="field">
            <span>@username *</span>
            <input bind:value={username} placeholder={$t("ivan2027")} oninput={() => (username = username.toLowerCase())} />
          </label>
          <label class="field">
            <span>{$t("Display name")}</span>
            <input bind:value={displayName} placeholder="Иван Иванов" />
          </label>
          <label class="field">
            <span>{$t("About")}</span>
            <textarea bind:value={about} rows="2" placeholder={$t("A little about you…")}></textarea>
          </label>
          <label class="field">
            <span>{$t("Phone (optional)")}</span>
            <input bind:value={phone} placeholder="+7 999 123-45-67" />
          </label>
          <div class="actions">
            <button class="ghost" onclick={() => (step = 2)}>{$t("Back")}</button>
            <button class="primary" onclick={finish} disabled={busy}>{$t(busy ? "Registering…" : "Finish")}</button>
          </div>
        </div>
      {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .reset-head { display: flex; flex-direction: column; gap: 6px; margin-bottom: 12px; }
  .reset-head b { font-size: 18px; }
  .link { align-self: flex-start; border: none; background: none; padding: 0; color: var(--accent); font: inherit; font-size: 13px; font-weight: 600; cursor: pointer; }
  .link:hover { text-decoration: underline; }
  .link.forgot { margin-top: 6px; font-weight: 500; }
  .err-act { display: block; margin-top: 8px; border: 1px solid currentColor; background: none; color: inherit; font: inherit; font-weight: 700; font-size: 13px; padding: 6px 12px; border-radius: 8px; cursor: pointer; }
  .note.dim { opacity: 0.75; font-size: 12.5px; }
  .rk-title { font-size: 18px; }
  .rk-words { list-style: none; margin: 4px 0 0; padding: 12px; display: grid; grid-template-columns: repeat(3, 1fr); gap: 6px 10px; background: var(--bg-input); border-radius: 12px; border: 1px solid var(--border); font-family: var(--font-mono, ui-monospace, monospace); font-size: 13.5px; user-select: text; }
  .rk-words li { display: flex; gap: 6px; align-items: baseline; }
  .rk-words li span { color: var(--fg-dim); font-size: 11px; min-width: 16px; text-align: right; }
  .rk-ok { display: flex; align-items: center; gap: 8px; font-size: 13.5px; cursor: pointer; }
  .mode-tabs { display: flex; gap: 4px; background: var(--bg-input); border-radius: 12px; padding: 4px; margin-bottom: 14px; }
  .mode-tabs button { flex: 1; border: none; background: none; color: var(--fg-muted); font: inherit; font-weight: 700; font-size: 13.5px; padding: 9px; border-radius: 9px; cursor: pointer; }
  .mode-tabs button.on { background: var(--bg-shell); color: var(--accent); box-shadow: 0 0 0 1px var(--border-strong), 0 4px 14px -6px var(--accent-glow); }
  .pw { position: relative; display: flex; }
  .pw input { flex: 1; padding-right: 38px; }
  .pw-eye { position: absolute; right: 6px; top: 50%; transform: translateY(-50%); background: none; border: none; color: var(--fg-dim); font-size: 16px; cursor: pointer; }
  .pw-meter { display: block; height: 4px; border-radius: 4px; background: var(--bg-input); overflow: hidden; margin-top: 6px; }
  .pw-meter i { display: block; height: 100%; background: #ff6b6b; transition: width 0.2s, background 0.2s; }
  .pw-meter i.ok { background: var(--accent); }
  .backdrop {
    position: fixed; inset: 0;
    background:
      radial-gradient(1000px 600px at 15% 0%, var(--accent-glow), transparent 55%),
      radial-gradient(900px 600px at 90% 100%, var(--accent-2-glow, color-mix(in srgb, var(--accent-2) 35%, transparent)), transparent 55%),
      var(--bg-void, #06070C);
    display: grid; place-items: center;
    z-index: 140;
    padding: 20px;
  }
  .sheet {
    width: min(460px, 94vw);
    background: var(--bg-shell);
    border: 1px solid var(--border-strong);
    border-radius: 20px;
    box-shadow: 0 40px 100px -20px rgba(0,0,0,0.75);
    padding: 26px;
    animation: slide-up 0.24s ease-out;
  }
  /* Two-pane login: brand hero + the form panel. */
  .sheet.login {
    width: min(820px, 96vw);
    padding: 0;
    display: grid;
    grid-template-columns: 1fr 1.1fr;
    overflow: hidden;
  }
  .hero {
    padding: 34px 30px;
    background:
      radial-gradient(600px 400px at 20% 0%, var(--accent-soft), transparent 60%),
      linear-gradient(160deg, color-mix(in srgb, var(--accent) 12%, var(--bg-list)), var(--bg-list));
    border-right: 1px solid var(--border);
    display: flex; flex-direction: column;
  }
  .logo-lg {
    width: 56px; height: 56px; border-radius: 16px;
    display: block;
    box-shadow: 0 0 30px -4px var(--accent-glow);
    margin-bottom: 18px;
  }
  .hero-title { margin: 0; font-size: 26px; font-weight: 800; letter-spacing: -0.02em; }
  .hero-tag { margin: 8px 0 22px; font-size: 13px; color: var(--fg-muted); line-height: 1.55; }
  .hero-feat { list-style: none; margin: auto 0 0; padding: 0; display: flex; flex-direction: column; gap: 12px; }
  .hero-feat li { display: flex; align-items: center; gap: 10px; font-size: 12.5px; color: var(--fg); }
  .hero-feat :global(svg) { width: 16px; height: 16px; color: var(--accent); flex-shrink: 0; }
  .hf-ic { width: 16px; text-align: center; color: var(--accent); flex-shrink: 0; }
  .panel { padding: 26px; display: flex; flex-direction: column; overflow-y: auto; }
  @media (max-width: 680px) {
    .sheet.login { grid-template-columns: 1fr; }
    .hero { display: none; }
  }
  @keyframes slide-up { from { opacity: 0; transform: translateY(12px);} to { opacity: 1; transform: translateY(0);} }

  .brand-row { display: flex; align-items: center; gap: 14px; margin-bottom: 18px; }
  .logo {
    width: 48px; height: 48px;
    border-radius: 14px;
    background: linear-gradient(135deg, var(--accent), var(--accent-2));
    display: grid; place-items: center;
    font-weight: 800; font-size: 22px; color: #06070C;
    box-shadow: 0 0 24px -4px var(--accent-glow);
    flex-shrink: 0;
  }
  .brand-row h2 { margin: 0; font-size: 18px; font-weight: 800; letter-spacing: -0.01em; }
  .sub { margin: 2px 0 0; font-size: 12.5px; color: var(--fg-muted); line-height: 1.4; }

  .steps { display: flex; align-items: center; justify-content: center; gap: 4px; margin-bottom: 18px; }
  .dot {
    width: 26px; height: 26px; border-radius: 50%;
    display: grid; place-items: center;
    background: var(--bg-input); border: 1px solid var(--border);
    font-size: 12px; font-weight: 700; color: var(--fg-dim);
  }
  .dot.active { background: var(--accent-soft); border-color: var(--accent); color: var(--accent); }
  .dot.done { background: var(--accent); border-color: var(--accent); color: #06070C; }
  .bar { width: 40px; height: 2px; background: var(--border); }
  .bar.done { background: var(--accent); }

  .err {
    background: rgba(255,77,109,0.12);
    border: 1px solid rgba(255,77,109,0.3);
    color: #ff9fb0;
    padding: 8px 12px; border-radius: 10px;
    font-size: 12.5px; margin-bottom: 12px;
  }

  .continue-box { margin-bottom: 14px; }
  .cb-label { font-size: 11px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.06em; color: var(--fg-dim); margin-bottom: 8px; }
  .continue-btn {
    width: 100%; padding: 12px 16px; border-radius: 12px; border: none; cursor: pointer;
    font-size: 14.5px; font-weight: 700; color: #06070C;
    background: linear-gradient(135deg, var(--accent), var(--accent-2));
    box-shadow: 0 6px 16px -4px var(--accent-glow);
  }
  .continue-btn:hover:not(:disabled) { transform: translateY(-1px); }
  .continue-btn:disabled { opacity: 0.5; cursor: default; }
  .cb-or { display: flex; align-items: center; gap: 10px; margin: 14px 0 2px; color: var(--fg-dim); font-size: 11.5px; }
  .cb-or::before, .cb-or::after { content: ""; flex: 1; height: 1px; background: var(--border); }

  .form { display: flex; flex-direction: column; gap: 12px; }
  .field { display: flex; flex-direction: column; gap: 4px; }
  .field span { font-size: 11px; font-weight: 700; color: var(--fg-dim); text-transform: uppercase; letter-spacing: 0.06em; }
  .field input, .field textarea {
    background: var(--bg-input);
    border: 1px solid var(--border);
    border-radius: 10px;
    color: var(--fg);
    font-family: inherit;
    font-size: 14px;
    padding: 9px 12px;
    outline: none;
    resize: vertical;
  }
  .field input:focus, .field textarea:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  .code-input { font-family: var(--font-mono); font-size: 22px; letter-spacing: 0.3em; text-align: center; }
  .note { font-size: 11.5px; color: var(--fg-dim); line-height: 1.5; margin: 0; }
  .resend {
    align-self: flex-start;
    background: transparent; border: none;
    color: var(--accent); font-size: 12px; font-weight: 600;
    cursor: pointer; padding: 0;
  }
  .resend:disabled { color: var(--fg-dim); cursor: not-allowed; }
  .note.dev { color: var(--accent); }
  .note.dev b { font-family: var(--font-mono); }

  .actions { display: flex; justify-content: flex-end; gap: 10px; margin-top: 6px; }
  .ghost, .primary { padding: 9px 18px; border-radius: 10px; font-size: 13px; font-weight: 700; cursor: pointer; border: none; }
  .ghost { background: transparent; border: 1px solid var(--border); color: var(--fg-muted); }
  .ghost:hover { color: var(--fg); border-color: var(--border-strong); }
  .primary { background: linear-gradient(135deg, var(--accent), var(--accent-2)); color: #06070C; box-shadow: 0 6px 16px -4px var(--accent-glow); }
  .primary:hover:not(:disabled) { transform: translateY(-1px); }
  button:disabled { opacity: 0.5; cursor: not-allowed; }
  .note.warn { color: #ffb454; line-height: 1.45; }
</style>
