<script lang="ts">
  import { t } from "../i18n";
  import { contacts, saveContact } from "../contacts";
  import { chats, activeTab, selectChat, startChat, info } from "../stores";
  import { openProfile } from "./ProfileModal.svelte";
  import { openNewChatDialog } from "../newChat";
  import { icons } from "../icons";
  import { avatars } from "../people";
  import { callHistory } from "../callHistory";
  import { showToast } from "./Toast.svelte";
  import { tr } from "../i18n";

  // Phones: Contacts is also where recent calls and invitations live (one
  // tab instead of three, like Telegram).
  const missed = $derived($callHistory.filter((c) => c.status === "missed" && Date.now() - c.started_at < 7 * 86400000).length);
  async function invite() {
    const me = $info?.username;
    let server = "";
    try { server = $info?.server_url ? new URL($info.server_url).host : ""; } catch { /* keep it empty */ }
    const text = me
      ? tr("Write to me in Aquamarine: @{name}{at}", { name: me, at: server ? ` (${tr("server")} ${server})` : "" })
      : tr("Let's talk in Aquamarine");
    try {
      if (navigator.share) { await navigator.share({ text }); return; }
    } catch { /* closed, or not allowed: copy instead */ }
    try { await navigator.clipboard.writeText(text); showToast(tr("Invitation copied — send it to a friend")); } catch { showToast(text); }
  }

  interface Row {
    username: string;
    full_name: string;
    phone: string;
    has_chat: boolean;
  }

  let search = $state("");

  let rows = $derived(buildRows($contacts, $chats, search));

  function buildRows(
    all: Record<string, { full_name?: string; phone?: string }>,
    chatList: typeof $chats,
    query: string,
  ): Row[] {
    const chatIds = new Set(chatList.map((c) => c.id));
    const q = query.trim().toLowerCase();
    const list: Row[] = Object.entries(all).map(([username, info]) => ({
      username,
      full_name: info.full_name ?? "",
      phone: info.phone ?? "",
      has_chat: chatIds.has(username),
    }));
    const filtered = q
      ? list.filter((r) =>
          r.username.toLowerCase().includes(q) ||
          r.full_name.toLowerCase().includes(q) ||
          r.phone.toLowerCase().includes(q),
        )
      : list;
    return filtered.sort((a, b) => (a.full_name || a.username).localeCompare(b.full_name || b.username));
  }

  async function openContact(r: Row) {
    if (r.has_chat) {
      activeTab.set("chats");
      await selectChat(r.username);
    } else {
      openProfile(r.username);
    }
  }

  async function startWith(r: Row) {
    if (!$info?.connected) { openNewChatDialog(r.username); return; }
    activeTab.set("chats");
    await startChat(r.username, "Привет!", { full_name: r.full_name, phone: r.phone });
  }
</script>

<aside class="panel">
  <div class="head">
    <h2>{$t("Contacts")}</h2>
    <button class="add-btn" onclick={() => openNewChatDialog()} title={$t("Add contact")}>{$t("+ Add")}</button>
  </div>

  <label class="search">
    {@html icons.search}
    <input placeholder={$t("Search contacts")} bind:value={search} />
  </label>

  <div class="list">
    <div class="quick">
      <button class="q-row" onclick={invite}>
        <span class="q-ic inv"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="9" cy="8" r="4"/><path d="M2 21a7 7 0 0 1 14 0"/><path d="M19 8v6M16 11h6"/></svg></span>
        <span class="q-lbl">{$t("Invite friends")}</span>
      </button>
      <button class="q-row" onclick={() => activeTab.set("calls")}>
        <span class="q-ic calls">{@html icons.phone}</span>
        <span class="q-lbl">{$t("Recent calls")}</span>
        {#if missed > 0}<span class="q-badge">{missed}</span>{/if}
      </button>
    </div>
    {#if rows.length === 0}
      <div class="empty">
        <div class="empty-icon">{@html icons.contacts}</div>
        <div class="empty-title">
          {$t(search ? "No matching contacts" : "Your contact book is empty")}
        </div>
        <div class="empty-sub">
          {#if search}
            {$t("Try a different name, phone or username — or add a new contact with the")} <b>{$t("+ Add")}</b> {$t("button above.")}
          {:else}
            {$t("Click")} <b>{$t("+ Add")}</b> {$t("to save someone's name and phone against their username. New chats created from")} <b>{$t("+ New chat")}</b> {$t("are also added here automatically.")}
          {/if}
        </div>
      </div>
    {:else}
      {#each rows as r, ri (r.username)}
        <div class="row" class:alt={ri % 2 === 1}>
          <button class="open" onclick={() => openContact(r)} title={$t(r.has_chat ? "Open chat" : "View profile")}>
            <div class="avatar" style:background-image={$avatars[r.username] ? `url("${$avatars[r.username]}")` : undefined} style:background-size="cover" style:background-position="center">{$avatars[r.username] ? "" : (r.full_name || r.username).charAt(0).toUpperCase()}</div>
            <div class="meta">
              <div class="name">{r.full_name || r.username}</div>
              <div class="sub">
                {#if r.full_name}@{r.username}{/if}
                {#if r.phone}
                  {#if r.full_name}<span class="dot">·</span>{/if}
                  <span class="phone">{r.phone}</span>
                {/if}
              </div>
            </div>
          </button>
          <div class="actions">
            {#if r.has_chat}
              <button class="chip ok" onclick={() => openContact(r)}>{$t("Open chat")}</button>
            {:else}
              <button class="chip" onclick={() => startWith(r)}>{$t("Start chat")}</button>
            {/if}
          </div>
        </div>
      {/each}
    {/if}
  </div>
</aside>

<style>
  .panel {
    background: var(--bg-list);
    border-right: 1px solid var(--border);
    display: flex; flex-direction: column;
    min-width: 0;
  }
  .head {
    padding: 14px 18px 10px;
    display: flex; align-items: center; justify-content: space-between;
  }
  .head h2 { margin: 0; font-size: 15.5px; font-weight: 700; letter-spacing: -0.01em; }
  .add-btn {
    padding: 5px 10px;
    background: linear-gradient(135deg, var(--accent), var(--accent-2));
    color: #06070C;
    font-weight: 700;
    font-size: 11.5px;
    border: none;
    border-radius: calc(8px * var(--rk, 1));
    cursor: pointer;
    box-shadow: 0 4px 10px -4px var(--accent-glow);
  }
  .add-btn:hover { transform: translateY(-1px); }

  .search {
    margin: 0 14px 10px;
    display: flex; align-items: center; gap: 8px;
    padding: 8px 12px;
    background: var(--bg-input);
    border: 1px solid var(--border);
    border-radius: calc(10px * var(--rk, 1));
  }
  .search :global(svg) { width: 14px; height: 14px; color: var(--fg-dim); flex-shrink: 0; }
  .search input {
    flex: 1;
    background: transparent;
    border: none;
    outline: none;
    color: var(--fg);
    font-family: inherit;
    font-size: 13px;
    min-width: 0;
  }

  .list { flex: 1; overflow-y: auto; overflow-x: hidden; padding: 6px; }
  .quick { display: none; }
  @media (max-width: 720px) {
    .head { padding: 14px 16px 10px; }
    .head h2 { font-size: 17px; }
    .add-btn { padding: 6px 12px; font-size: 12.5px; border-radius: calc(9px * var(--rk, 1)); }
    .search { margin: 0 14px 10px; height: 36px; box-sizing: border-box; padding: 0 12px; border-radius: calc(18px * var(--rk, 1)); border-color: transparent; background: var(--bg-surface-2, var(--bg-input)); }
    .search input { font-size: 14px; }
    .search :global(svg) { width: 15px; height: 15px; }
    .quick { display: flex; flex-direction: column; margin: 0 4px 8px; padding: 2px; border-radius: calc(14px * var(--rk, 1));
      background: color-mix(in srgb, var(--bg-surface-2, var(--bg-input)) 70%, transparent); }
    .q-row { display: flex; align-items: center; gap: 12px; padding: 8px 10px; border: none; background: transparent; color: var(--fg); font: 500 14.5px var(--font-ui); text-align: left; border-radius: calc(12px * var(--rk, 1)); }
    .q-row:active { background: var(--bg-hover); }
    .q-ic { width: 30px; height: 30px; border-radius: calc(9px * var(--rk, 1)); display: grid; place-items: center; color: #fff; flex-shrink: 0; }
    .q-ic :global(svg), .q-ic svg { width: 16px; height: 16px; }
    .q-ic.inv { background: linear-gradient(135deg, #3B9BFF, #2563EB); }
    .q-ic.calls { background: linear-gradient(135deg, #34D399, #059669); }
    .q-lbl { flex: 1; }
    .q-badge { min-width: 22px; height: 22px; padding: 0 6px; box-sizing: border-box; border-radius: calc(11px * var(--rk, 1)); background: var(--danger, #FF4D6D); color: #fff; font-size: 12px; font-weight: 800; display: grid; place-items: center; }
    .open { grid-template-columns: 44px 1fr; gap: 12px; padding: 7px 8px; }
    .avatar { width: 44px; height: 44px; font-size: 16px; }
    .name { font-size: 15px; }
    .sub { font-size: 12.5px; }
  }
  .empty { padding: 60px 22px; text-align: center; color: var(--fg-dim); font-size: 12.5px; }
  .empty-icon {
    display: inline-grid; place-items: center;
    width: 56px; height: 56px;
    border-radius: 50%;
    background: var(--accent-soft);
    color: var(--accent);
    margin-bottom: 14px;
  }
  .empty-icon :global(svg) { width: 24px; height: 24px; }
  .empty-title { font-size: 14px; font-weight: 700; color: var(--fg); margin-bottom: 4px; }
  .empty-sub { line-height: 1.5; }
  .empty-sub :global(b) { color: var(--fg); }

  .row {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 8px;
    align-items: center;
    padding: 2px 4px;
    border-radius: calc(10px * var(--rk, 1));
    transition: background 0.1s ease;
  }
  .row:hover { background: var(--bg-hover); }
  .open {
    display: grid;
    grid-template-columns: 36px 1fr;
    gap: 11px;
    align-items: center;
    padding: 6px 8px;
    background: transparent;
    border: none;
    text-align: left;
    color: inherit;
    cursor: pointer;
    min-width: 0;
  }
  .avatar {
    width: 36px; height: 36px;
    border-radius: var(--ava-r, 50%);
    background: linear-gradient(135deg, var(--accent) 0%, var(--accent-2) 100%);
    display: grid; place-items: center;
    font-weight: 800;
    font-size: 14px;
    color: #06070C;
  }
  .meta { min-width: 0; }
  .name { font-weight: 600; font-size: 13.5px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sub { font-size: 11.5px; color: var(--fg-dim); margin-top: 2px; font-family: var(--font-mono); }
  .phone { font-variant-numeric: tabular-nums; }
  .dot { margin: 0 4px; }

  .actions { flex-shrink: 0; padding-right: 8px; }
  .chip {
    padding: 4px 10px;
    background: var(--bg-input);
    border: 1px solid var(--border);
    border-radius: calc(7px * var(--rk, 1));
    color: var(--fg-muted);
    font-size: 11px;
    font-weight: 600;
    cursor: pointer;
  }
  .chip:hover { color: var(--accent); border-color: var(--accent-soft); }
  .chip.ok { color: var(--accent); border-color: var(--accent-soft); background: var(--accent-soft); }
</style>
