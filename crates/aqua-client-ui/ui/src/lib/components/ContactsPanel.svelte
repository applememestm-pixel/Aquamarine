<script lang="ts">
  import { t } from "../i18n";
  import { contacts, saveContact } from "../contacts";
  import { chats, activeTab, selectChat, startChat, info } from "../stores";
  import { openProfile } from "./ProfileModal.svelte";
  import { openNewChatDialog } from "../newChat";
  import { icons } from "../icons";
  import { avatars } from "../people";

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
    {#if rows.length === 0}
      <div class="empty">
        <div class="empty-icon">{@html icons.contacts}</div>
        <div class="empty-title">
          {$t(search ? "No matching contacts" : "Your contact book is empty")}
        </div>
        <div class="empty-sub">
          {#if search}
            Try a different name, phone or username — or add a new contact with the <b>{$t("+ Add")}</b> button above.
          {:else}
            Click <b>{$t("+ Add")}</b> {$t("to save someone's name and phone against their username. New chats created from")} <b>{$t("+ New chat")}</b> are also added here automatically.
          {/if}
        </div>
      </div>
    {:else}
      {#each rows as r (r.username)}
        <div class="row">
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
    border-radius: 8px;
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
    border-radius: 10px;
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

  .list { flex: 1; overflow-y: auto; padding: 6px; }
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
    border-radius: 10px;
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
    border-radius: 50%;
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
    border-radius: 7px;
    color: var(--fg-muted);
    font-size: 11px;
    font-weight: 600;
    cursor: pointer;
  }
  .chip:hover { color: var(--accent); border-color: var(--accent-soft); }
  .chip.ok { color: var(--accent); border-color: var(--accent-soft); background: var(--accent-soft); }
</style>
