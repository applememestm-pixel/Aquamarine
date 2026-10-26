<script module lang="ts">
  import { writable } from "svelte/store";
  import type { DirProfile } from "../types";
  /** null = closed; a string = the search prefilled; a profile = straight to "write to them". */
  const req = writable<{ q: string; who: DirProfile | null } | null>(null);
  export function openNewChat(prefill: string | DirProfile = "") {
    req.set(typeof prefill === "string" ? { q: prefill, who: null } : { q: "", who: prefill });
  }
</script>

<script lang="ts">
  /**
   * "New message", like Telegram's: search people by @name or name (the
   * server's directory), your chats below; pick one → their card and the
   * first message. Only people the server knows — no chats with nobody.
   */
  import { tick } from "svelte";
  import { get } from "svelte/store";
  import { t, tr } from "../i18n";
  import { api } from "../tauri";
  import { chats, info, selectChat, startChat, activeTab } from "../stores";
  import { avatars, refreshPeer } from "../people";
  import { saveContact } from "../contacts";
  import { onBack } from "../back";
  import { openNewGroup } from "./NewGroupWizard.svelte";

  let q = $state("");
  let who = $state<DirProfile | null>(null);
  let first = $state("");
  let results = $state<DirProfile[]>([]);
  let searching = $state(false);
  let searched = $state("");
  let err = $state("");
  let busy = $state(false);
  let inputEl = $state<HTMLInputElement | null>(null);
  let msgEl = $state<HTMLTextAreaElement | null>(null);
  const open = $derived(!!$req);

  $effect(() => {
    const r = $req;
    if (!r) return;
    q = r.q; who = r.who; err = ""; results = []; searched = "";
    first = tr("Hi! 👋");
    void tick().then(() => (who ? msgEl : inputEl)?.focus());
  });
  $effect(() => { if (open) return onBack(back); });

  function close() { req.set(null); }
  function back() { if (who && !$req?.who) { who = null; err = ""; void tick().then(() => inputEl?.focus()); } else close(); }

  const clean = (s: string) => s.trim().replace(/^@+/, "").toLowerCase();
  // People you already talk to (1:1, not groups / channels / service chats).
  const mine = $derived.by(() => {
    const s = clean(q);
    return $chats.filter((c) => !c.id.startsWith("grp_") && !c.id.startsWith("__") && !c.channel && c.id !== "welcome")
      .filter((c) => !s || c.id.includes(s) || c.name.toLowerCase().includes(s))
      .slice(0, 30);
  });

  let timer: ReturnType<typeof setTimeout> | null = null;
  $effect(() => {
    const s = clean(q);
    if (timer) clearTimeout(timer);
    if (!open || who || s.length < 2) { results = []; searched = ""; return; }
    timer = setTimeout(async () => {
      if (!get(info)?.connected) { err = tr("Not connected to a server"); return; }
      searching = true; err = "";
      try {
        let r = await api.searchDirectory(s);
        // An exact @name the search didn't list (search shows only some).
        if (/^[a-z0-9_]{3,32}$/.test(s) && !r.some((p) => p.username === s)) {
          const exact = await api.fetchDirectoryProfile(s).catch(() => null);
          if (exact) r = [exact, ...r];
        }
        const have = new Set(mine.map((c) => c.id));
        results = r.filter((p) => p.username !== get(info)?.username && !have.has(p.username)).slice(0, 30);
        for (const p of results.slice(0, 12)) void refreshPeer(p.username);
      } catch (e) { results = []; err = String(e); }
      finally { searching = false; searched = s; }
    }, 260);
  });

  async function openChat(id: string) {
    close();
    activeTab.set("chats");
    await selectChat(id);
  }

  function pick(p: DirProfile) {
    // Already a chat: just open it.
    if ($chats.some((c) => c.id === p.username)) { void openChat(p.username); return; }
    who = p; err = "";
    void tick().then(() => { msgEl?.focus(); msgEl?.select(); });
  }

  async function send() {
    const p = who, text = first.trim();
    if (!p || !text || busy) return;
    busy = true; err = "";
    try {
      if (p.display_name || p.phone) saveContact(p.username, { full_name: p.display_name || undefined, phone: p.phone || undefined });
      if (p.is_bot) {
        await api.startBotChat(p.username, text.startsWith("/") ? text : "/start");
        chats.set(await api.listChats());
        await openChat(p.username);
        return;
      }
      const e = await startChat(p.username, text);
      if (e) {
        const m = /No user @(\S+)/.exec(e);
        err = m ? tr("No user @{u} on this server", { u: m[1] }) : e;
        return;
      }
      close();
      activeTab.set("chats");
    } finally { busy = false; }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") { e.preventDefault(); back(); }
  }
  const letter = (s: string) => (s || "?").charAt(0).toUpperCase();
</script>

{#if open}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="nc-veil" onclick={close}></div>
  <div class="nc" role="dialog" aria-modal="true" aria-label={$t("New message")} tabindex="-1" onkeydown={onKey}>
    <header class="nc-head">
      <button class="nc-ic" onclick={back} aria-label={$t("Back")}>
        {#if who && !$req?.who}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M15 18l-6-6 6-6"/></svg>
        {:else}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="M6 6l12 12M18 6 6 18"/></svg>
        {/if}
      </button>
      <b>{$t("New message")}</b>
    </header>

    {#if who}
      {@const p = who}
      <div class="nc-card">
        <div class="nc-ava big" style:background-image={$avatars[p.username] ? `url("${$avatars[p.username]}")` : undefined}>{$avatars[p.username] ? "" : letter(p.display_name || p.username)}</div>
        <div class="nc-name">{p.display_name || p.username}{#if p.is_bot}<span class="nc-bot">BOT</span>{/if}</div>
        <div class="nc-user">@{p.username}</div>
        {#if p.about}<div class="nc-about">{p.about}</div>{/if}
      </div>
      <form class="nc-send" onsubmit={(e) => { e.preventDefault(); void send(); }}>
        <textarea bind:this={msgEl} bind:value={first} rows="2" maxlength="4000" placeholder={$t("Message")}
          onkeydown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); void send(); } }}></textarea>
        <button class="nc-go" type="submit" disabled={busy || !first.trim()} aria-label={$t("Send")}>
          {#if busy}<span class="nc-spin"></span>{:else}<svg viewBox="0 0 24 24" fill="currentColor"><path d="M3.4 20.4 21 12 3.4 3.6l-.1 6.5L15 12 3.3 13.9z"/></svg>{/if}
        </button>
      </form>
      {#if err}<div class="nc-err">{err}</div>{/if}
      <p class="nc-note">{$t(p.is_bot ? "The bot gets /start." : "The chat appears once the first message is sent — end-to-end encrypted.")}</p>
    {:else}
      <div class="nc-search">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/></svg>
        <input bind:this={inputEl} bind:value={q} placeholder={$t("@username or name")} autocomplete="off" autocapitalize="off" spellcheck="false" />
        {#if searching}<span class="nc-spin"></span>{/if}
      </div>
      <div class="nc-list">
        {#if !q.trim()}
          <button class="nc-row act" onclick={() => { close(); openNewGroup("group"); }}>
            <span class="nc-ava ic"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2"/><circle cx="9" cy="7" r="4"/><path d="M22 21v-2a4 4 0 0 0-3-3.87M16 3.13a4 4 0 0 1 0 7.75"/></svg></span>
            <span class="nc-main"><b>{$t("New group")}</b></span>
          </button>
          <button class="nc-row act" onclick={() => { close(); openNewGroup("channel"); }}>
            <span class="nc-ava ic"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 11v2a1 1 0 0 0 1 1h2l5 4V6L6 10H4a1 1 0 0 0-1 1z"/><path d="M15.5 8.5a5 5 0 0 1 0 7M18.5 5.5a9 9 0 0 1 0 13"/></svg></span>
            <span class="nc-main"><b>{$t("New channel")}</b></span>
          </button>
        {/if}
        {#if results.length}
          <div class="nc-sec">{$t("Global search")}</div>
          {#each results as p (p.username)}
            <button class="nc-row" onclick={() => pick(p)}>
              <span class="nc-ava" style:background-image={$avatars[p.username] ? `url("${$avatars[p.username]}")` : undefined}>{$avatars[p.username] ? "" : letter(p.display_name || p.username)}</span>
              <span class="nc-main"><b>{p.display_name || p.username}{#if p.is_bot}<span class="nc-bot">BOT</span>{/if}</b><small>@{p.username}{#if p.about}{" · " + p.about}{/if}</small></span>
            </button>
          {/each}
        {:else if searched && !searching && clean(q) === searched && !mine.length}
          <div class="nc-empty">{$t("No one called @{u} on this server", { u: searched })}</div>
        {/if}
        {#if mine.length}
          <div class="nc-sec">{$t(q.trim() ? "Your chats" : "Recent")}</div>
          {#each mine as c (c.id)}
            <button class="nc-row" onclick={() => void openChat(c.id)}>
              <span class="nc-ava" style:background-image={$avatars[c.id] ? `url("${$avatars[c.id]}")` : c.gradient}>{$avatars[c.id] ? "" : c.letter}</span>
              <span class="nc-main"><b>{c.name}</b><small>@{c.id}</small></span>
            </button>
          {/each}
        {/if}
        {#if err}<div class="nc-err">{err}</div>{/if}
      </div>
    {/if}
  </div>
{/if}

<style>
  .nc-veil { position: fixed; inset: 0; z-index: 240; background: rgba(0,0,0,.45); animation: nc-fade .18s ease; }
  .nc { position: fixed; z-index: 241; left: 50%; top: 50%; transform: translate(-50%, -50%); width: min(440px, calc(100vw - 24px)); height: min(620px, calc(100vh - 48px));
    display: flex; flex-direction: column; border-radius: 18px; background: var(--bg-surface); border: 1px solid var(--border-strong);
    box-shadow: 0 30px 80px -24px rgba(0,0,0,.8); overflow: hidden; animation: nc-in .24s cubic-bezier(.22,1,.36,1); outline: none; }
  @keyframes nc-fade { from { opacity: 0; } }
  @keyframes nc-in { from { opacity: 0; transform: translate(-50%, -48%) scale(.97); } }
  .nc-head { display: flex; align-items: center; gap: 8px; padding: 10px 12px 6px; }
  .nc-head b { font-size: 17px; }
  .nc-ic { width: 36px; height: 36px; border: none; border-radius: 50%; background: transparent; color: var(--fg-muted); cursor: pointer; display: grid; place-items: center; }
  .nc-ic:hover { background: var(--bg-hover); color: var(--fg); }
  .nc-ic svg { width: 20px; height: 20px; }
  .nc-search { display: flex; align-items: center; gap: 8px; margin: 4px 14px 8px; padding: 0 12px; height: 42px; border-radius: 21px; background: var(--bg-input); color: var(--fg-dim); }
  .nc-search svg { width: 18px; height: 18px; flex-shrink: 0; }
  .nc-search input { flex: 1; min-width: 0; border: none; outline: none; background: transparent; color: var(--fg); font: inherit; font-size: 15px; }
  .nc-list { flex: 1; overflow-y: auto; padding: 0 6px 10px; }
  .nc-sec { padding: 12px 10px 4px; font-size: 12.5px; font-weight: 700; color: var(--accent); }
  .nc-row { width: 100%; display: flex; align-items: center; gap: 12px; padding: 7px 10px; border: none; border-radius: 12px; background: transparent; color: var(--fg); font: inherit; text-align: left; cursor: pointer; }
  .nc-row:hover, .nc-row:focus-visible { background: var(--bg-hover); outline: none; }
  .nc-row.act b { color: var(--accent); font-weight: 600; }
  .nc-ava { width: 42px; height: 42px; border-radius: 50%; flex-shrink: 0; display: grid; place-items: center; font-weight: 800; font-size: 17px; color: #06070C;
    background: linear-gradient(135deg, var(--accent), var(--accent-2)); background-size: cover; background-position: center; }
  .nc-ava.ic { background: var(--accent-soft); color: var(--accent); }
  .nc-ava.ic svg { width: 21px; height: 21px; }
  .nc-ava.big { width: 88px; height: 88px; font-size: 34px; }
  .nc-main { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 2px; }
  .nc-main b { font-size: 15px; font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .nc-main small { font-size: 13px; color: var(--fg-dim); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .nc-bot { margin-left: 6px; padding: 1px 5px; border-radius: 5px; font-size: 10px; font-weight: 800; background: var(--accent-soft); color: var(--accent); vertical-align: 2px; }
  .nc-empty { padding: 18px 12px; font-size: 14px; color: var(--fg-dim); text-align: center; }
  .nc-err { margin: 8px 14px; font-size: 13.5px; color: var(--danger, #FF4D6D); }
  .nc-card { display: flex; flex-direction: column; align-items: center; gap: 4px; padding: 18px 20px 12px; text-align: center; }
  .nc-name { margin-top: 8px; font-size: 19px; font-weight: 700; }
  .nc-user { font-size: 14px; color: var(--accent); }
  .nc-about { margin-top: 6px; font-size: 13.5px; color: var(--fg-muted); max-width: 320px; }
  .nc-send { display: flex; align-items: flex-end; gap: 8px; margin: 10px 14px 0; padding: 6px 6px 6px 14px; border-radius: 22px; background: var(--bg-input); }
  .nc-send textarea { flex: 1; min-width: 0; resize: none; border: none; outline: none; background: transparent; color: var(--fg); font: inherit; font-size: 15px; padding: 8px 0; max-height: 140px; }
  .nc-go { width: 40px; height: 40px; flex-shrink: 0; border: none; border-radius: 50%; display: grid; place-items: center; cursor: pointer; color: #06070C; background: linear-gradient(135deg, var(--accent), var(--accent-2)); }
  .nc-go:disabled { opacity: .45; cursor: default; }
  .nc-go svg { width: 19px; height: 19px; }
  .nc-note { margin: 10px 18px; font-size: 12.5px; color: var(--fg-dim); text-align: center; }
  .nc-spin { width: 16px; height: 16px; border-radius: 50%; border: 2px solid currentColor; border-right-color: transparent; animation: nc-rot .7s linear infinite; }
  @keyframes nc-rot { to { transform: rotate(360deg); } }
  @media (max-width: 720px) {
    .nc { left: 0; top: 0; transform: none; width: 100vw; height: 100%; border-radius: 0; border: none; padding-top: env(safe-area-inset-top, 0px); animation: nc-up .26s cubic-bezier(.22,1,.36,1); }
    @keyframes nc-up { from { transform: translateY(24px); opacity: 0; } }
  }
</style>
