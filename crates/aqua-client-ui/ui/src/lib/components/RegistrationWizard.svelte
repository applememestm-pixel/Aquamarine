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
  import { adoptMyCard } from "../people";
  import { appIconUrl } from "../appIcon";
  import ServerPicker from "./ServerPicker.svelte";
  import { scanQr } from "./QrScanner.svelte";
  import { parseQr, saveInvite, inviteFor } from "../qr";
  import { t, tr } from "../i18n";
  import { api } from "../tauri";
  import { info, chats } from "../stores";
  import { profile as localProfile, updateProfile } from "../profile";
  import { connect } from "../stores";
  import { showToast } from "./Toast.svelte";
  import { icons } from "../icons";
  import { applyUiState, refreshBackup } from "../backup";
  import { accounts, loadAccounts, switchAccount } from "../accounts";
  import { confirmWithTelegram } from "../tgLogin";
  import { fly, fade } from "svelte/transition";

  /**
   * Sign-up like Telegram: a welcome page, then one thing per page with
   * "Next" — @username, name, password, server — and beside it one of the
   * messenger's advantages per page, animated. The last page explains what
   * a server is and how it's chosen.
   */
  let page = $state(0);
  /** Which way the pages slide (1 forward, -1 back). */
  let dir = $state(1);
  const PAGES = 4;
  function go(n: number) { dir = n >= page ? 1 : -1; error = null; existsHint = false; boundHint = false; page = Math.max(0, Math.min(PAGES, n)); }
  $effect(() => { if ($wizardOpen) { page = 0; dir = 1; } });
  /** "Next": check this page, then the next one (the last creates the account). */
  function next() {
    error = null;
    if (page === 1 && !/^[a-z0-9_.]{3,32}$/.test(login)) { error = tr("@username: 3–32 characters, a-z 0-9 _ . only"); return; }
    if (page === 3) {
      if (!(passUser && !password) && password.length < 8) { error = tr("Password must be at least 8 characters"); return; }
      if (password !== password2) { error = tr("Passwords don't match"); return; }
    }
    if (page < PAGES) { go(page + 1); return; }
    void submit();
  }
  /** One advantage per page (the welcome page shows the first). */
  const ADV = [
    { ic: "lock", title: "Private by design", text: "Messages are end-to-end encrypted (X3DH · Double Ratchet). Not even the server can read them." },
    { ic: "at", title: "No phone, no e-mail", text: "Just an @username. Nobody needs your number to write to you." },
    { ic: "palette", title: "Make it yours", text: "Themes, name colours, stickers, emoji and wallpapers — for you and for the people you talk to." },
    { ic: "key", title: "Your keys stay with you", text: "Keys are made and kept on this device. Two-step verification adds your other devices." },
    { ic: "net", title: "Any server, one network", text: "The public server, a friend's or your own — your account works on every server of the network." },
  ] as const;

  // Opened by "Add account": the other accounts on this device stay one tap away.
  $effect(() => { if ($wizardOpen) void loadAccounts(); });
  const otherAccounts = $derived($accounts.filter((a) => !a.active && a.registered && a.username));
  let leaving = $state(false);
  async function backTo(slot: string) {
    leaving = true;
    try { await switchAccount(slot); } catch (e) { leaving = false; showToast(String(e)); }
  }

  const IS_ANDROID = typeof navigator !== "undefined" && /android/i.test(navigator.userAgent);
  // The project's public relay: a new device signs up there unless another
  // server is chosen (a home relay on 127.0.0.1 / the LAN is still in the list).
  const PUBLIC_SERVER = "http://193.233.245.196:7777";
  const DEFAULT_SERVER = PUBLIC_SERVER;

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

  /** A server's error in plain words, plus what to do about it. */
  function explainNet(m: string, url: string): string | null {
    if (/server is too old/i.test(m)) return tr("The server at {u} runs an old Aquamarine. Since 1.2.0 the app works only with servers 1.2.0 or newer — ask its owner to update it.", { u: url });
    if (/needs an e-mail to sign up/i.test(m)) return tr("This server runs an Aquamarine older than 1.2.0-1 and still asks for an e-mail — ask its owner to update it.");
    const local = /\/\/(127\.0\.0\.1|localhost|\[?::1\]?)(?=[:/]|$)/i.test(url);
    if (/10061|ECONNREFUSED|Connection refused|actively refused|отверг/i.test(m))
      return local
        ? tr("Nothing answers at {u}: that's this computer, and no Aquamarine server runs on it. Pick a server from the list or type the address your friend gave you.", { u: url })
        : tr("The server at {u} doesn't answer: it's off, the port is closed, or the address is wrong.", { u: url });
    if (/InvalidContentType|wrong version number|tls|ssl|certificate/i.test(m))
      return tr("{u} doesn't speak HTTPS. Try the address with http:// instead.", { u: url });
    if (/timed out|timeout/i.test(m)) return tr("The server at {u} takes too long to answer — check the address and your connection.", { u: url });
    if (/dns|resolve|lookup|No such host|getaddrinfo/i.test(m)) return tr("There's no such address: {u}. Check the spelling.", { u: url });
    return null;
  }
  /** Run a server request; an https:// address that turns out to be a plain
   *  HTTP server is retried once with http:// (and kept). */
  async function withServer<T>(fn: (url: string) => Promise<T>): Promise<T> {
    try { return await fn(serverUrl.trim()); }
    catch (e) {
      const m = String(e);
      if (/^https:\/\//i.test(serverUrl.trim()) && /InvalidContentType|wrong version number|tls|ssl/i.test(m)) {
        const alt = serverUrl.trim().replace(/^https:/i, "http:");
        const r = await fn(alt);
        serverUrl = alt;
        return r;
      }
      throw e;
    }
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
        // "Sign in" with the account's password (+ code) adds this device.
        mode = "signin";
        username = i.username;
        error = tr("@{name} isn't linked to this device yet. Sign in below with its password — this device is added to the account (that needs two-step verification on it), the others stay signed in.", { name: i.username });
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

  // Since 1.2.0-1 there is no e-mail: step 1 is @username + password.
  // 4 = recovery key for the backup, 7 = two-step code, 8 = recovery key
  // to get past "no new devices" / a lost code phone.
  // 9 = confirm the sign-in in Telegram.
  let step = $state<1 | 4 | 7 | 8 | 9>(1);
  /** A confirmed Telegram sign-in (sign-up links it; sign-in uses it). */
  let tg = $state<{ ticket: string; name: string } | null>(null);
  let tgWaiting = $state(false);
  let tgStop = { aborted: false };
  /** The bot link of the sign-in in progress (to open it again). */
  let tgLink = $state("");
  async function telegram(then: "signup" | "signin") {
    error = null;
    serverUrl = normalizeServer(serverUrl);
    tgWaiting = true;
    tgStop = { aborted: false };
    try {
      tg = await confirmWithTelegram(serverUrl.trim(), tgStop, (l) => (tgLink = l));
      if (then === "signup" && !username.trim() && /^@[a-z0-9_]{3,32}$/i.test(tg.name)) username = tg.name.slice(1).toLowerCase();
      if (then === "signin") { await signIn(); }
    } catch (e) {
      const m = String(e);
      if (!/cancelled/.test(m)) error = /no Telegram bot/i.test(m) ? tr("This server has no Telegram bot — sign in with the password and an authenticator app, or ask the server's admin to add a bot.") : m;
    } finally {
      tgWaiting = false;
      tgLink = "";
    }
  }
  /** Step 7: the code from the authenticator app (or a backup code). */
  let totp = $state("");
  /** Step 8: sign in with the recovery key instead of the code. */
  let useRecovery = $state(false);
  /** Step 8 because sign-in on new devices is off (not a lost code phone). */
  let locked = $state(false);
  /** Step 4: the backup didn't open with the password — recovery key. */
  let phrase = $state("");
  let serverUrl = $state(DEFAULT_SERVER);
  /** @username (or, for an account from before 1.2.0-1, its e-mail). */
  let username = $state("");
  let displayName = $state("");
  /** An invitation code or a pass from another server (private servers). */
  let inviteText = $state("");
  let busy = $state(false);
  let error = $state<string | null>(null);
  let mode = $state<"signup" | "signin">("signup");
  /** The advantage beside this page (sign-in: "your keys stay with you"). */
  const adv = $derived(ADV[mode === "signup" ? page : 3]);
  /** The @username is taken: offer to sign in. */
  let existsHint = $state(false);
  /** The account lives on another device (no two-step verification). */
  let boundHint = $state(false);
  let password = $state("");
  let password2 = $state("");
  let showPw = $state(false);
  const pwScore = $derived(
    (password.length >= 8 ? 1 : 0) + (password.length >= 12 ? 1 : 0) +
    (/[A-ZА-Я]/.test(password) && /[a-zа-я]/.test(password) ? 1 : 0) + (/\d/.test(password) ? 1 : 0) + (/[^\w\s]/.test(password) ? 1 : 0),
  );
  const login = $derived(username.trim().replace(/^@/, "").toLowerCase());
  /** A pass from my server (Settings → Account & server): I join this
   *  private server with my account there — same @name, nothing to choose. */
  function passUserOf(text: string): string | null {
    const m = /^aqpass:([A-Za-z0-9_-]+)\./.exec(text.trim());
    if (!m) return null;
    try {
      const b = m[1].replace(/-/g, "+").replace(/_/g, "/");
      const v = JSON.parse(atob(b + "=".repeat((4 - (b.length % 4)) % 4)));
      return typeof v.u === "string" ? v.u : null;
    } catch { return null; }
  }
  const passUser = $derived(mode === "signup" ? passUserOf(inviteText) : null);
  $effect(() => { if (passUser) username = passUser; });

  /** A server's QR or an invitation from an invite-only server's admin. */
  async function scanInvite() {
    const text = await scanQr();
    if (!text) return;
    const r = parseQr(text);
    if (r?.kind === "join") { saveInvite(r.url, r.code); serverUrl = r.url; mode = "signup"; showToast(tr("Invitation accepted — create your account on this server")); }
    else if (r?.kind === "server") { serverUrl = r.url; }
    else showToast(tr("That QR code isn't an Aquamarine server or invitation"));
  }

  function plainError(m: string): string {
    if (/already taken/i.test(m)) { existsHint = true; return tr("@{u} is already taken.", { u: login }); }
    if (/lives on one device/i.test(m)) { boundHint = true; return tr("@{u} lives on another device: it has no two-step verification, so it can't be moved. Turn it on there (Settings → Privacy & Security), then sign in here.", { u: login }); }
    if (/wrong @username or password/i.test(m)) return tr("Wrong @username or password");
    if (/too many wrong/i.test(m)) return tr("Too many wrong attempts — try again in 15 minutes");
    if (/already registered from this network/i.test(m)) { existsHint = true; return tr("An account was already registered from this network. One per network — sign in to it instead."); }
    if (/invite-only/i.test(m)) return tr("This server is private: scan the invitation QR from its admin, or paste a pass from your server (Settings → Account & server → Pass).");
    if (/needs an e-mail to sign up/i.test(m)) return tr("This server runs an Aquamarine older than 1.2.0-1 and still asks for an e-mail — ask its owner to update it.");
    if (/username must be/i.test(m)) return tr("@username: 3–32 characters, a-z 0-9 _ . only");
    return explainNet(m, serverUrl.trim()) ?? m;
  }

  async function submit() {
    error = null; existsHint = false; boundHint = false;
    if (mode === "signup" && !/^[a-z0-9_.]{3,32}$/.test(login)) { error = tr("@username: 3–32 characters, a-z 0-9 _ . only"); return; }
    if (mode === "signin" && !login) { error = tr("Enter your @username"); return; }
    if (!(passUser && !password) && password.length < 8) { error = tr("Password must be at least 8 characters"); return; }
    if (mode === "signup" && password !== password2) { error = tr("Passwords don't match"); return; }
    serverUrl = normalizeServer(serverUrl);
    if (mode === "signup") await signUp(); else await signIn();
  }

  async function signUp() {
    busy = true;
    wizardHold.set(true);
    let u = login;
    try {
      u = await withServer((url) => api.signupPlain(url, u, password, displayName.trim() || undefined, inviteText.trim() || inviteFor(url) || undefined, tg?.ticket)) || u;
      tg = null;
      const err = await connect(serverUrl.trim(), u);
      if (err) { error = plainError(err); wizardHold.set(false); return; }
      updateProfile({ display_name: displayName.trim() || u, about: "An Aquamarine user.", phone: "" });
      try { await api.publishProfile(displayName.trim() || u, "", ""); } catch { /* non-fatal */ }
      try { info.set(await api.completeRegistration(u, null)); } catch { /* non-fatal */ }
      try { chats.set(await api.listChats()); } catch { /* non-fatal */ }
      showToast(tr("Registered as @{u}", { u }));
      closeRegistration();
    } catch (e) {
      error = plainError(String(e));
      // Back to the page the problem is on (the server page shows the rest).
      if (/already taken|username must be|already registered from this network/i.test(String(e))) { const m = error; go(1); error = m; existsHint = /already/.test(String(e)); }
      wizardHold.set(false);
    } finally {
      busy = false;
    }
  }

  /** The account's sign-in protection said no: ask for what it wants. */
  function gate(e: unknown): boolean {
    const m = String(e);
    if (/two-step verification code required/i.test(m)) { step = 7; error = null; return true; }
    if (/confirm the sign-in in Telegram/i.test(m)) { step = 9; error = null; tg = null; return true; }
    if (/not the Telegram account linked/i.test(m)) { step = 9; tg = null; error = tr("That's another Telegram account — confirm in the one linked to @{u}", { u: login }); return true; }
    if (/Telegram confirmation has expired/i.test(m)) { step = 9; tg = null; error = tr("The Telegram confirmation has expired — confirm again"); return true; }
    if (/wrong two-step/i.test(m)) { step = 7; totp = ""; error = tr("Wrong code — try the next one from the app, or a backup code"); return true; }
    if (/turned off for this account/i.test(m)) { step = 8; locked = true; useRecovery = false; error = null; return true; }
    if (/recovery key doesn't match/i.test(m) && useRecovery) { step = 8; useRecovery = false; error = tr("That recovery key doesn't match this account"); return true; }
    return false;
  }

  /** Existing account with two-step verification: add this device, take
   *  the encrypted backup (if any), connect, done. */
  async function signIn(withPhrase = false) {
    busy = true;
    let u = "";
    try {
      const code = step === 7 ? totp.trim() : undefined;
      const rec = useRecovery ? phrase.trim() : undefined;
      u = await withServer((url) => api.signinPlain(url, username.trim(), password, code, rec, step === 9 ? tg?.ticket : undefined));
      tg = null;
    } catch (e) {
      busy = false;
      if (gate(e)) return;
      error = plainError(String(e));
      step = 1;
      return;
    }
    await afterSignIn(u, withPhrase);
    busy = false;
  }

  async function afterSignIn(u: string, withPhrase: boolean, skipBackup = false) {
    let restored = false;
    // The restore failed (network, server): there may be a copy on the
    // server that this device couldn't read. Then backups stay off here —
    // turning them on would upload this empty device over that copy, and the
    // chats in it would be gone for good.
    let restoreError = "";
    if (!skipBackup) {
      try {
        const r = await api.backupRestore(serverUrl.trim(), "", password, "", withPhrase ? phrase.trim() : undefined);
        if (r.found && r.need_phrase) { step = 4; error = withPhrase ? tr("That recovery key doesn't open the backup") : null; signedAs = u; return; }
        if (r.found) { applyUiState(r.ui); restored = true; }
      } catch (e) { console.warn("backup restore", e); restoreError = String(e); }
    }
    try {
      const err = await connect(serverUrl.trim(), u);
      if (err) { error = err; return; }
      try { info.set(await api.completeRegistration(u, null)); } catch { /* non-fatal */ }
      await adoptMyCard(u);
      try { chats.set(await api.listChats()); } catch { /* non-fatal */ }
      if (!restored && !restoreError) { try { await api.backupEnable(password); } catch { /* non-fatal */ } }
      void refreshBackup();
      showToast(restored ? tr("Signed in as @{u} — chats restored from your backup", { u })
        : restoreError ? tr("Signed in as @{u}, but the chats backup couldn't be read: {e}. Sign out and sign in again to retry — until then this device doesn't touch the backup.", { u, e: restoreError })
        : tr("Signed in as @{u}", { u }));
      closeRegistration();
      if (restored) setTimeout(() => location.reload(), 600);
    } catch (e) {
      error = tr("Could not sign in: {e}", { e: String(e) });
      step = 1;
    }
  }
  /** Step 4 keeps the username the relay gave back. */
  let signedAs = "";

  /** Step 7 / 8: try again with the code or the recovery key. */
  async function retrySignIn(withRecovery: boolean) {
    error = null;
    useRecovery = withRecovery;
    await signIn(withRecovery);
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
        <div class="hero-brand"><img class="logo-lg" src={$appIconUrl} alt="" draggable="false" /><b class="hero-title">{$t("Aquamarine")}</b></div>
        {#key adv.ic}
          <div class="adv" in:fly={{ y: 16, duration: 380, delay: 80 }} out:fade={{ duration: 120 }}>
            <div class="adv-ic {adv.ic}" aria-hidden="true">
              {#if adv.ic === "lock"}
                <svg viewBox="0 0 48 48" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><path class="shackle" d="M15 21v-6a9 9 0 0 1 18 0v6"/><rect x="10" y="21" width="28" height="20" rx="5"/><circle cx="24" cy="31" r="2.6" fill="currentColor"/></svg>
              {:else if adv.ic === "at"}
                <svg viewBox="0 0 48 48" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round"><circle cx="24" cy="24" r="7"/><path class="ring" d="M31 24v3a5 5 0 0 0 10 0v-3a17 17 0 1 0-6.6 13.4"/></svg>
              {:else if adv.ic === "palette"}
                <svg viewBox="0 0 48 48" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><path d="M24 6a18 18 0 0 0 0 36c2.5 0 3.5-2 2.6-4-1-2.4.6-4.6 3-4.6H34a8 8 0 0 0 8-8C42 14 34 6 24 6z"/><circle class="d1" cx="15" cy="21" r="2.8" fill="#FF6B6B" stroke="none"/><circle class="d2" cx="21" cy="13.5" r="2.8" fill="#FFD43B" stroke="none"/><circle class="d3" cx="30" cy="14" r="2.8" fill="#51CF66" stroke="none"/><circle class="d4" cx="34.5" cy="21.5" r="2.8" fill="#4DABF7" stroke="none"/></svg>
              {:else if adv.ic === "key"}
                <svg viewBox="0 0 48 48" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><circle cx="16" cy="30" r="8"/><path d="M22 24 38 8M32 14l5 5M28 18l4 4"/></svg>
              {:else}
                <svg viewBox="0 0 48 48" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round"><circle cx="24" cy="10" r="5"/><circle cx="10" cy="36" r="5"/><circle cx="38" cy="36" r="5"/><path class="link1" d="M21.5 14.5 12.5 31.5M26.5 14.5l9 17M15 36h18"/></svg>
              {/if}
            </div>
            <div class="adv-t">
              <h2 class="adv-title">{$t(adv.title)}</h2>
              <p class="adv-text">{$t(adv.text)}</p>
            </div>
          </div>
        {/key}
        {#if mode === "signup"}
          <div class="hero-dots" aria-hidden="true">{#each ADV as _, i (i)}<i class:on={i === page}></i>{/each}</div>
        {/if}
      </aside>

      <div class="panel">
      {#if error}
        <div class="err">
          {error}
          {#if existsHint}
            <button class="err-act" onclick={() => { mode = "signin"; error = null; existsHint = false; password2 = ""; }}>{$t("Sign in to it")}</button>
          {/if}
        </div>
      {/if}

      {#if step === 1 && mode === "signup" && page === 0}
        <!-- Welcome: Telegram's "Start Messaging" page. -->
        <div class="intro" in:fly={{ x: -40 * dir, duration: 260 }}>
          <img class="intro-logo" src={$appIconUrl} alt="" draggable="false" />
          <h1>{$t("Welcome to Aquamarine")}</h1>
          <p>{$t("A fast, private messenger with end-to-end encryption. It takes a minute to set up.")}</p>
          {#if $info?.registered && $info?.username}
            <button class="continue-btn" onclick={continueAs} disabled={busy}>
              {busy ? $t("Signing in…") : $t("Continue as @{name}", { name: $info.username ?? "" })}
            </button>
          {/if}
          {#if otherAccounts.length && !$info?.registered}
            <div class="other-acc">
              {#each otherAccounts as a (a.slot)}
                <button class="link" disabled={leaving} onclick={() => backTo(a.slot)}>← {$t("Back to @{name}", { name: a.username ?? "" })}</button>
              {/each}
            </div>
          {/if}
          <button class="primary big" onclick={() => go(1)}>{$t("Create account")}</button>
          <button class="ghost big" onclick={() => { mode = "signin"; error = null; }}>{$t("I already have an account")}</button>
          <button class="link skip" onclick={skipForNow}>{$t("Skip")}</button>
        </div>
      {:else if step === 1 && mode === "signup"}
        <div class="pager">
          <button class="pg-back" type="button" onclick={() => go(page - 1)} aria-label={$t("Back")}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M15 18l-6-6 6-6"/></svg></button>
          <div class="pg-bar"><i style:width="{(page / PAGES) * 100}%"></i></div>
          <span class="pg-n">{page} / {PAGES}</span>
        </div>
        <form class="form" onsubmit={(e) => { e.preventDefault(); next(); }}>
          {#key page}
          <div class="pg" in:fly={{ x: 46 * dir, duration: 260 }}>
          {#if page === 1}
            <h2 class="pg-title">{$t("Choose a @username")}</h2>
            <p class="note">{$t("People find and write to you by it. 3–32 characters: a-z, 0-9, _ and dots.")}</p>
            <label class="field">
              <span>@username{passUser ? " · " + $t("from your server") : ""}</span>
              <div class="at-in"><i>@</i>
                <!-- svelte-ignore a11y_autofocus -->
                <input bind:value={username} autocomplete="username" autocapitalize="none" spellcheck="false" readonly={!!passUser} autofocus
                       placeholder={$t("ivan2027")} oninput={() => { username = username.toLowerCase().replace(/^@/, ""); }} />
              </div>
            </label>
          {:else if page === 2}
            <h2 class="pg-title">{$t("What's your name?")}</h2>
            <p class="note">{$t("Shown in chats instead of the @username. You can skip it and change it any time.")}</p>
            <label class="field">
              <span>{$t("Name (optional)")}</span>
              <!-- svelte-ignore a11y_autofocus -->
              <input bind:value={displayName} autocomplete="nickname" placeholder="Иван" autofocus />
            </label>
          {:else if page === 3}
            <h2 class="pg-title">{$t("Set a password")}</h2>
            <label class="field">
              <span>{$t("Password")}</span>
              <div class="pw">
                <!-- svelte-ignore a11y_autofocus -->
                <input bind:value={password} type={showPw ? "text" : "password"} autocomplete="new-password" autofocus placeholder={$t("At least 8 characters")} />
                <button class="pw-eye" type="button" onclick={() => (showPw = !showPw)} title={showPw ? $t("Hide") : $t("Show")}>{showPw ? "◉" : "◎"}</button>
              </div>
              {#if password}<span class="pw-meter"><i style:width="{pwScore * 20}%" class:ok={pwScore >= 3}></i></span>{/if}
            </label>
            <label class="field">
              <span>{$t("Repeat password")}</span>
              <input bind:value={password2} type={showPw ? "text" : "password"} autocomplete="new-password" />
            </label>
            <p class="note">{$t("No e-mail or phone needed. The account lives on this device; to use it on others and keep your chats on the server, turn on two-step verification later in Settings → Privacy & Security.")}</p>
            {#if tg}
              <p class="note tg-ok">✓ {$t("Telegram {name} will be linked: it confirms sign-ins on new devices, and the account isn't tied to this device.", { name: tg.name })}</p>
            {:else}
              <button class="tg-btn" type="button" disabled={tgWaiting} onclick={() => telegram("signup")}>
                <svg viewBox="0 0 24 24" fill="currentColor"><path d="M21.9 4.6 18.7 19.7c-.2 1-.9 1.3-1.8.8l-4.9-3.6-2.4 2.3c-.3.3-.5.5-1 .5l.3-5 9.1-8.2c.4-.4-.1-.6-.6-.2L6.2 13.4l-4.8-1.5c-1-.3-1-1 .2-1.5L20.6 3.3c.9-.3 1.6.2 1.3 1.3z"/></svg>
                {tgWaiting ? $t("Waiting for Telegram… tap Start in the bot") : $t("Link Telegram — it lets you sign in on other devices")}
              </button>
              {#if tgWaiting && tgLink}<button class="link" type="button" onclick={() => api.openExternal(tgLink).catch(() => {})}>{$t("Open the bot again")} →</button>{/if}
            {/if}
          {:else}
            <h2 class="pg-title">{$t("Choose a server")}</h2>
            <!-- What a server is and how it's chosen — the question everyone has. -->
            <ul class="srv-explain">
              <li><b>{$t("What is it?")}</b> {$t("A server is the relay that passes your messages along. It can't read them: they're encrypted on your device and opened only on the other person's.")}</li>
              <li><b>{$t("Which one?")}</b> {$t("Leave the public Aquamarine server if you don't know — it suits everyone. Got an address or a QR code from a friend, a community, or your own server? Use that.")}</li>
              <li><b>{$t("Does it matter?")}</b> {$t("Servers join into networks: the account works on every server of its network, and people on other servers can still write to you. You can switch later — it changes only the route.")}</li>
            </ul>
            <div class="field">
              <span>{$t("Server")} {inviteFor(serverUrl) ? "· " + $t("invitation ✓") : ""}</span>
              <ServerPicker bind:value={serverUrl} placeholder={DEFAULT_SERVER}
                defaults={[PUBLIC_SERVER, IS_ANDROID ? "http://10.0.2.2:7777" : "http://127.0.0.1:7777"]} />
              <input class="inv-in" bind:value={inviteText} autocomplete="off" spellcheck="false"
                     placeholder={$t("Invitation code or pass (private servers)")} />
              <button class="scan-inv" type="button" onclick={scanInvite}>
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M4 8V5a1 1 0 0 1 1-1h3M16 4h3a1 1 0 0 1 1 1v3M20 16v3a1 1 0 0 1-1 1h-3M8 20H5a1 1 0 0 1-1-1v-3M7 12h10"/></svg>
                {$t("Scan a server or invitation QR")}
              </button>
            </div>
          {/if}
          </div>
          {/key}
          <div class="actions">
            <button class="primary big" type="submit" disabled={busy}>
              {busy ? $t("Registering…") : page < PAGES ? $t("Next") : $t("Create account")}
            </button>
          </div>
        </form>
      {:else if step === 1}
        <div class="pager">
          <button class="pg-back" type="button" onclick={() => { mode = "signup"; go(0); }} aria-label={$t("Back")}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M15 18l-6-6 6-6"/></svg></button>
          <h2 class="pg-title inline">{$t("Sign in")}</h2>
        </div>
        <form class="form" onsubmit={(e) => { e.preventDefault(); void submit(); }} in:fly={{ x: 46, duration: 260 }}>
          <label class="field">
            <span>@username</span>
            <div class="at-in"><i>@</i>
              <input bind:value={username} autocomplete="username" autocapitalize="none" spellcheck="false" placeholder={$t("username")} />
            </div>
          </label>
          <label class="field">
            <span>{$t("Password")}</span>
            <div class="pw">
              <input bind:value={password} type={showPw ? "text" : "password"} autocomplete="current-password" />
              <button class="pw-eye" type="button" onclick={() => (showPw = !showPw)} title={showPw ? $t("Hide") : $t("Show")}>{showPw ? "◉" : "◎"}</button>
            </div>
          </label>
          <p class="note">{$t("A new device can be added only to an account with two-step verification: you'll be asked for a code from your authenticator app.")}</p>
          <details class="srv">
            <summary><span class="srv-k">{$t("Server")}</span> <b>{serverUrl.replace(/^https?:\/\//, "")}</b><span class="srv-ch">{$t("Change")}</span></summary>
            <div class="field">
              <ServerPicker bind:value={serverUrl} placeholder={DEFAULT_SERVER}
                defaults={[PUBLIC_SERVER, IS_ANDROID ? "http://10.0.2.2:7777" : "http://127.0.0.1:7777"]} />
              <button class="scan-inv" type="button" onclick={scanInvite}>
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M4 8V5a1 1 0 0 1 1-1h3M16 4h3a1 1 0 0 1 1 1v3M20 16v3a1 1 0 0 1-1 1h-3M8 20H5a1 1 0 0 1-1-1v-3M7 12h10"/></svg>
                {$t("Scan a server or invitation QR")}
              </button>
            </div>
          </details>
          <div class="actions">
            <button class="ghost" type="button" onclick={() => { mode = "signup"; go(1); }}>{$t("Create account")}</button>
            <button class="primary" type="submit" disabled={busy}>{busy ? $t("Signing in…") : $t("Sign in")}</button>
          </div>
        </form>
      {:else if step === 7}
        <div class="form">
          <b class="rk-title">{$t("Two-step verification")}</b>
          <p class="note">{$t("This account asks for a code from the authenticator app on a new device. Open the app and enter the 6-digit code for Aquamarine.")}</p>
          <label class="field">
            <span>{$t("Code")}</span>
            <!-- svelte-ignore a11y_autofocus -->
            <input bind:value={totp} placeholder="123456" class="code-input" autocomplete="one-time-code" autofocus
                   onkeydown={(e) => e.key === "Enter" && totp.trim() && retrySignIn(false)} />
          </label>
          <p class="note dim">{$t("No phone with the app? Enter one of the backup codes you saved, or use the recovery key.")}</p>
          <button class="link" onclick={() => { phrase = ""; error = null; locked = false; step = 8; }}>{$t("Use the recovery key")}</button>
          <div class="actions">
            <button class="ghost" onclick={() => { totp = ""; step = 1; }}>{$t("Back")}</button>
            <button class="primary" onclick={() => retrySignIn(false)} disabled={busy || !totp.trim()}>{$t(busy ? "Checking…" : "Sign in")}</button>
          </div>
        </div>
      {:else if step === 9}
        <div class="form">
          <b class="rk-title">{$t("Confirm in Telegram")}</b>
          <p class="note">{$t("This account is linked to Telegram. Open the server's bot and tap Start — then this device is added to the account.")}</p>
          <button class="tg-btn big" type="button" disabled={tgWaiting || busy} onclick={() => telegram("signin")}>
            <svg viewBox="0 0 24 24" fill="currentColor"><path d="M21.9 4.6 18.7 19.7c-.2 1-.9 1.3-1.8.8l-4.9-3.6-2.4 2.3c-.3.3-.5.5-1 .5l.3-5 9.1-8.2c.4-.4-.1-.6-.6-.2L6.2 13.4l-4.8-1.5c-1-.3-1-1 .2-1.5L20.6 3.3c.9-.3 1.6.2 1.3 1.3z"/></svg>
            {tgWaiting ? $t("Waiting for Telegram… tap Start in the bot") : $t("Open Telegram")}
          </button>
          {#if tgWaiting && tgLink}<button class="link" type="button" onclick={() => api.openExternal(tgLink).catch(() => {})}>{$t("Open the bot again")} →</button>{/if}
          <button class="link" onclick={() => { tgStop.aborted = true; phrase = ""; error = null; locked = false; step = 8; }}>{$t("Use the recovery key")}</button>
          <div class="actions">
            <button class="ghost" onclick={() => { tgStop.aborted = true; step = 1; }}>{$t("Back")}</button>
          </div>
        </div>
      {:else if step === 8}
        <div class="form">
          {#if locked}
            <b class="rk-title">{$t("Sign-in on new devices is off")}</b>
            <p class="note">{$t("The owner of this account turned off sign-in on new devices. Turn it on in Settings → Privacy & Security on a device that's signed in, then try again.")}</p>
            <p class="note dim">{$t("Lost every device? The 24-word recovery key opens the account (and the phone with the codes isn't needed).")}</p>
          {:else}
            <b class="rk-title">{$t("Sign in with the recovery key")}</b>
            <p class="note">{$t("The 24-word recovery key you saved when you created the account opens it without a code from the app.")}</p>
          {/if}
          <label class="field">
            <span>{$t("Recovery key")}</span>
            <textarea bind:value={phrase} rows="4" spellcheck="false" autocomplete="off" placeholder="word word word …"></textarea>
          </label>
          <div class="actions">
            <button class="ghost" onclick={() => { phrase = ""; step = 1; }}>{$t("Back")}</button>
            <button class="primary" onclick={() => retrySignIn(true)} disabled={busy || phrase.trim().split(/\s+/).length !== 24}>{$t(busy ? "Checking…" : "Sign in")}</button>
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
            <button class="ghost" onclick={() => { busy = true; void afterSignIn(signedAs, false, true).finally(() => (busy = false)); }} disabled={busy}>{$t("Start without the backup")}</button>
            <button class="primary" onclick={() => { busy = true; void afterSignIn(signedAs, true).finally(() => (busy = false)); }} disabled={busy || phrase.trim().split(/\s+/).length !== 24}>{$t("Restore")}</button>
          </div>
        </div>
      {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .at-in { position: relative; display: flex; }
  .at-in i { position: absolute; left: 12px; top: 50%; transform: translateY(-50%); font-style: normal; color: var(--fg-dim); font-weight: 700; pointer-events: none; }
  .field .at-in input { flex: 1; padding-left: 28px; }
  .inv-in { margin-top: 6px; }
  .tg-btn { display: inline-flex; align-items: center; justify-content: center; gap: 8px; align-self: flex-start; padding: 8px 14px; border-radius: calc(10px * var(--rk, 1)); border: 1px solid color-mix(in srgb, #2AABEE 55%, transparent); background: color-mix(in srgb, #2AABEE 14%, transparent); color: #5CC3F5; font: 600 13px var(--font-ui); cursor: pointer; }
  .tg-btn.big { align-self: stretch; padding: 12px; font-size: 14px; }
  .tg-btn svg { width: 17px; height: 17px; }
  .tg-btn:disabled { opacity: .7; cursor: progress; }
  .note.tg-ok { color: #5CC3F5; }
  .srv { font-size: 12.5px; color: var(--fg-dim); }
  /* A visible control, not a line of small print: which server is the first thing people look for. */
  .srv summary { cursor: pointer; list-style: none; display: flex; align-items: center; gap: 8px; padding: 8px 10px;
    border: 1px solid var(--border-strong); border-radius: calc(10px * var(--rk, 1)); background: var(--bg-input); }
  .srv summary:hover { border-color: var(--accent); }
  .srv-k { color: var(--fg-dim); }
  .srv-ch { margin-left: auto; color: var(--accent); font-weight: 700; }
  .srv[open] .srv-ch { visibility: hidden; }
  .srv summary::-webkit-details-marker { display: none; }
  .srv summary b { color: var(--fg); font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
  .srv[open] summary { margin-bottom: 6px; }
  .other-acc { display: flex; flex-direction: column; gap: 6px; margin-bottom: 12px; }
  .reset-head { display: flex; flex-direction: column; gap: 6px; margin-bottom: 12px; }
  .link { align-self: flex-start; border: none; background: none; padding: 0; color: var(--accent); font: inherit; font-size: 13px; font-weight: 600; cursor: pointer; }
  .link:hover { text-decoration: underline; }
  .link.forgot { margin-top: 6px; font-weight: 500; }
  .err-act { display: block; margin-top: 8px; border: 1px solid currentColor; background: none; color: inherit; font: inherit; font-weight: 700; font-size: 13px; padding: 6px 12px; border-radius: calc(8px * var(--rk, 1)); cursor: pointer; }
  .note.dim { opacity: 0.75; font-size: 12.5px; }
  .rk-title { font-size: 18px; }
  .rk-words { list-style: none; margin: 4px 0 0; padding: 12px; display: grid; grid-template-columns: repeat(3, 1fr); gap: 6px 10px; background: var(--bg-input); border-radius: calc(12px * var(--rk, 1)); border: 1px solid var(--border); font-family: var(--font-mono, ui-monospace, monospace); font-size: 13.5px; user-select: text; }
  .rk-ok { display: flex; align-items: center; gap: 8px; font-size: 13.5px; cursor: pointer; }
  .pw { position: relative; display: flex; }
  .field .pw input, .pw input { flex: 1; padding-right: 38px; }
  .pw-eye { position: absolute; right: 6px; top: 50%; transform: translateY(-50%); background: none; border: none; color: var(--fg-dim); font-size: 16px; cursor: pointer; }
  .pw-meter { display: block; height: 4px; border-radius: calc(4px * var(--rk, 1)); background: var(--bg-input); overflow: hidden; margin-top: 6px; }
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
    border-radius: calc(20px * var(--rk, 1));
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
    padding: 26px 30px;
    gap: 14px;
    background:
      radial-gradient(600px 400px at 20% 0%, var(--accent-soft), transparent 60%),
      linear-gradient(160deg, color-mix(in srgb, var(--accent) 12%, var(--bg-list)), var(--bg-list));
    border-right: 1px solid var(--border);
    display: flex; flex-direction: column;
  }
  .hero-brand { display: flex; align-items: center; gap: 10px; }
  .logo-lg {
    width: 34px; height: 34px; border-radius: calc(10px * var(--rk, 1));
    display: block;
    box-shadow: 0 0 22px -4px var(--accent-glow);
  }
  .hero-title { font-size: 17px; font-weight: 800; letter-spacing: -0.01em; }
  /* One advantage per page, animated (Telegram's intro). */
  .adv { margin: auto 0; display: flex; flex-direction: column; align-items: flex-start; gap: 10px; }
  .adv-ic { width: 92px; height: 92px; display: grid; place-items: center; color: var(--accent); border-radius: calc(26px * var(--rk, 1));
    background: radial-gradient(circle at 30% 25%, color-mix(in srgb, var(--accent) 30%, transparent), color-mix(in srgb, var(--accent) 8%, transparent));
    box-shadow: 0 14px 40px -14px var(--accent-glow); animation: adv-bob 4s ease-in-out infinite; margin-bottom: 8px; }
  .adv-ic svg { width: 52px; height: 52px; overflow: visible; }
  @keyframes adv-bob { 50% { transform: translateY(-6px) rotate(-2deg); } }
  .adv-ic.lock .shackle { animation: adv-shackle 2.6s ease-in-out infinite; transform-origin: 33px 21px; }
  @keyframes adv-shackle { 0%, 55%, 100% { transform: translateY(0); } 70% { transform: translateY(-4px); } }
  .adv-ic.at .ring { stroke-dasharray: 110; animation: adv-draw 2.8s ease-in-out infinite; }
  @keyframes adv-draw { 0% { stroke-dashoffset: 110; } 50%, 100% { stroke-dashoffset: 0; } }
  .adv-ic.palette circle { animation: adv-pop 2.4s ease-in-out infinite; transform-box: fill-box; transform-origin: center; }
  .adv-ic.palette .d2 { animation-delay: .2s; } .adv-ic.palette .d3 { animation-delay: .4s; } .adv-ic.palette .d4 { animation-delay: .6s; }
  @keyframes adv-pop { 0%, 60%, 100% { transform: scale(1); } 30% { transform: scale(1.45); } }
  .adv-ic.key svg { animation: adv-turn 3s ease-in-out infinite; transform-origin: 16px 30px; }
  @keyframes adv-turn { 0%, 60%, 100% { transform: rotate(0); } 75% { transform: rotate(-18deg); } 90% { transform: rotate(6deg); } }
  .adv-ic.net .link1 { stroke-dasharray: 4 5; animation: adv-flow 1s linear infinite; }
  @keyframes adv-flow { to { stroke-dashoffset: -18; } }
  .adv-t { display: flex; flex-direction: column; gap: 8px; }
  .adv-title { margin: 0; font-size: 24px; font-weight: 800; letter-spacing: -0.02em; line-height: 1.15; }
  .adv-text { margin: 0; font-size: 13.5px; color: var(--fg-muted); line-height: 1.55; max-width: 34ch; }
  .hero-dots { display: flex; gap: 6px; }
  .hero-dots i { width: 7px; height: 7px; border-radius: 4px; background: var(--border-strong); transition: width .3s, background .3s; }
  .hero-dots i.on { width: 22px; background: var(--accent); }
  /* Welcome page */
  .intro { display: flex; flex-direction: column; align-items: center; text-align: center; gap: 10px; margin: auto 0; }
  .intro-logo { width: 84px; height: 84px; border-radius: calc(24px * var(--rk, 1)); box-shadow: 0 0 40px -6px var(--accent-glow); animation: adv-bob 5s ease-in-out infinite; margin-bottom: 6px; }
  .intro h1 { margin: 0; font-size: 24px; font-weight: 800; letter-spacing: -0.02em; }
  .intro p { margin: 0 0 10px; font-size: 13.5px; color: var(--fg-muted); line-height: 1.5; max-width: 36ch; }
  .intro .primary, .intro .ghost, .intro .continue-btn { width: 100%; }
  .intro .other-acc { align-self: stretch; margin: 0; }
  .link.skip { align-self: center; margin-top: 4px; color: var(--fg-dim); font-weight: 500; }
  .primary.big, .ghost.big { padding: 12px 18px; font-size: 14.5px; border-radius: calc(12px * var(--rk, 1)); }
  .actions .primary.big { flex: 1; }
  /* Pages: back, progress, one thing at a time */
  .pager { display: flex; align-items: center; gap: 10px; margin-bottom: 16px; }
  .pg-back { width: 34px; height: 34px; flex-shrink: 0; display: grid; place-items: center; border-radius: 50%; border: 1px solid var(--border); background: none; color: var(--fg-muted); cursor: pointer; }
  .pg-back:hover { color: var(--fg); border-color: var(--border-strong); }
  .pg-back svg { width: 18px; height: 18px; }
  .pg-bar { flex: 1; height: 4px; border-radius: 2px; background: var(--bg-input); overflow: hidden; }
  .pg-bar i { display: block; height: 100%; background: linear-gradient(90deg, var(--accent), var(--accent-2)); transition: width .35s cubic-bezier(.2, .8, .2, 1); }
  .pg-n { font-size: 12px; color: var(--fg-dim); font-variant-numeric: tabular-nums; }
  .pg { display: flex; flex-direction: column; gap: 12px; }
  .pg-title { margin: 0; font-size: 21px; font-weight: 800; letter-spacing: -0.01em; }
  .pg-title.inline { font-size: 18px; }
  .srv-explain { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 8px; }
  .srv-explain li { font-size: 12.5px; line-height: 1.5; color: var(--fg-muted); padding: 9px 11px; border-radius: calc(10px * var(--rk, 1)); background: var(--bg-input); border: 1px solid var(--border); }
  .srv-explain b { color: var(--fg); display: block; margin-bottom: 2px; }
  @media (prefers-reduced-motion: reduce) { .adv-ic, .adv-ic *, .intro-logo { animation: none !important; } }
  :global(html.wk-lowfx) .adv-ic, :global(html.wk-lowfx) .adv-ic *, :global(html.wk-lowfx) .intro-logo { animation: none !important; }
  .panel { padding: 26px; display: flex; flex-direction: column; overflow-y: auto; }
  .sheet.login { min-height: min(560px, 92vh); max-height: 94vh; }
  @media (max-width: 680px) {
    /* Phone: the advantage sits above the page, smaller. */
    .sheet.login { grid-template-columns: 1fr; grid-template-rows: auto 1fr; }
    .hero { border-right: none; border-bottom: 1px solid var(--border); padding: 16px 20px; gap: 6px; }
    .hero-brand { display: none; }
    .adv { margin: 0; flex-direction: row; align-items: center; gap: 12px; }
    .adv-ic { width: 54px; height: 54px; flex-shrink: 0; margin: 0; border-radius: calc(16px * var(--rk, 1)); }
    .adv-ic svg { width: 32px; height: 32px; }
    .adv-t { gap: 3px; }
    .adv-title { font-size: 15.5px; }
    .adv-text { font-size: 12px; }
    .hero-dots { display: none; }
  }
  @keyframes slide-up { from { opacity: 0; transform: translateY(12px);} to { opacity: 1; transform: translateY(0);} }

  .brand-row { display: flex; align-items: center; gap: 14px; margin-bottom: 18px; }
  .logo {
    width: 48px; height: 48px;
    border-radius: calc(14px * var(--rk, 1));
    background: linear-gradient(135deg, var(--accent), var(--accent-2));
    display: grid; place-items: center;
    font-weight: 800; font-size: 22px; color: #06070C;
    box-shadow: 0 0 24px -4px var(--accent-glow);
    flex-shrink: 0;
  }
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
    padding: 8px 12px; border-radius: calc(10px * var(--rk, 1));
    font-size: 12.5px; margin-bottom: 12px;
  }

  .continue-box { margin-bottom: 14px; }
  .cb-label { font-size: 11px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.06em; color: var(--fg-dim); margin-bottom: 8px; }
  .continue-btn {
    width: 100%; padding: 12px 16px; border-radius: calc(12px * var(--rk, 1)); border: none; cursor: pointer;
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
    border-radius: calc(10px * var(--rk, 1));
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

  .actions { display: flex; justify-content: flex-end; gap: 10px; margin-top: 6px; }
  .ghost, .primary { padding: 9px 18px; border-radius: calc(10px * var(--rk, 1)); font-size: 13px; font-weight: 700; cursor: pointer; border: none; }
  .ghost { background: transparent; border: 1px solid var(--border); color: var(--fg-muted); }
  .ghost:hover { color: var(--fg); border-color: var(--border-strong); }
  .primary { background: linear-gradient(135deg, var(--accent), var(--accent-2)); color: #06070C; box-shadow: 0 6px 16px -4px var(--accent-glow); }
  .primary:hover:not(:disabled) { transform: translateY(-1px); }
  button:disabled { opacity: 0.5; cursor: not-allowed; }
  .note.warn { color: #ffb454; line-height: 1.45; }
  .scan-inv { margin-top: 6px; display: inline-flex; align-items: center; gap: 6px; align-self: flex-start; padding: 5px 10px; border-radius: calc(9px * var(--rk, 1));
    border: 1px dashed var(--border-strong); background: transparent; color: var(--fg-muted); font: 600 12px var(--font-ui); cursor: pointer; }
  .scan-inv:hover { color: var(--accent); border-color: var(--accent); }
  .scan-inv svg { width: 15px; height: 15px; }
</style>
