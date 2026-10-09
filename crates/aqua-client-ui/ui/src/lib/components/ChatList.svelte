<script lang="ts">
  import { t, tr } from "../i18n";
  import { avatars, peerCards, clans } from "../people";
  import ClanTag from "./ClanTag.svelte";
  import { keepOnScreen } from "../actions";
  import { archived, mutedChats, setArchived, setMuted } from "../chatPrefs";
  import { parseJoinLink } from "../groupRules";
  import { groupMap, requestJoinGroup } from "../groups";
  import { peerPresence, activityVerb } from "../presence";
  import { icons } from "../icons";
  import { info, search, visibleChats, activeId, selectChat, startChat } from "../stores";
  import { showToast } from "./Toast.svelte";
  import { openNewChatDialog } from "../newChat";
  import { openNewGroup } from "./NewGroupWizard.svelte";

  let plusMenuOpen = $state(false);
  import { contacts, saveContact } from "../contacts";
  import { api } from "../tauri";
  import type { DirProfile } from "../types";
  import { folders, activeFolder } from "../folders";
  import { openFolderManager } from "./FolderManager.svelte";

  // ---- Archive (Telegram): archived chats live behind one row ----
  let showArchive = $state(false);
  let archivedChats = $derived($visibleChats.filter((c) => $archived.has(c.id)));
  let archivedUnread = $derived(archivedChats.filter((c) => c.unread > 0 && !$mutedChats.has(c.id)).length);
  $effect(() => { if (showArchive && archivedChats.length === 0) showArchive = false; });

  // Chats filtered by archive + the active folder ("all" shows everything).
  // A search looks through archived chats too.
  let shownChats = $derived.by(() => {
    const af = $activeFolder;
    let list = $visibleChats;
    if (!$search.trim()) list = list.filter((c) => (showArchive ? $archived.has(c.id) : !$archived.has(c.id)));
    if (af === "all" || showArchive) return list;
    const f = $folders.find((x) => x.id === af);
    if (!f) return list;
    const set = new Set(f.chatIds);
    return list.filter((c) => set.has(c.id));
  });

  // ---- Right-click a chat: archive / mute ----
  let ctx = $state<{ x: number; y: number; id: string } | null>(null);
  function openCtx(e: MouseEvent, id: string) {
    e.preventDefault();
    ctx = { x: Math.min(e.clientX, window.innerWidth - 200), y: Math.min(e.clientY, window.innerHeight - 140), id };
  }
  function archiveChat(id: string) {
    const on = !$archived.has(id);
    setArchived(id, on);
    showToast(on ? "Chat archived" : "Chat moved back to the list");
    ctx = null;
  }

  // ---- Invite links pasted into the search box ----
  let joinLink = $derived(parseJoinLink($search ?? ""));
  let joinIsMember = $derived(!!joinLink && !!$groupMap[joinLink.gid]?.members.some((m) => m.username === $info?.username));
  let joinSent = $state("");
  async function requestJoin() {
    const j = joinLink;
    if (!j) return;
    try {
      await requestJoinGroup(j.gid, j.admin, j.title, "");
      joinSent = j.gid;
      showToast(`Request sent to @${j.admin}`);
    } catch (e) {
      showToast(`Could not send request: ${e}`);
    }
  }

  // ---- Keyboard: ↑/↓ through the list from the search box, Enter opens ----
  let kbIndex = $state(-1);
  $effect(() => { void $search; kbIndex = $search ? 0 : -1; });
  function onSearchKey(e: KeyboardEvent) {
    const n = shownChats.length;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      if (!n) return;
      e.preventDefault();
      kbIndex = e.key === "ArrowDown" ? (kbIndex + 1) % n : (kbIndex - 1 + n) % n;
      document.querySelectorAll(".chat-item")[kbIndex]?.scrollIntoView({ block: "nearest" });
    } else if (e.key === "Enter") {
      const c = shownChats[kbIndex >= 0 ? kbIndex : 0];
      if (!c) return;
      e.preventDefault();
      void selectChat(c.id);
      search.set("");
      kbIndex = -1;
      (e.target as HTMLInputElement).blur();
      queueMicrotask(() => window.dispatchEvent(new CustomEvent("aqua:focus-composer")));
    } else if (e.key === "Escape") {
      if ($search) { search.set(""); } else (e.target as HTMLInputElement).blur();
    }
  }

  async function copyFingerprint() {
    const fp = $info?.session_fingerprint ?? "";
    try {
      await navigator.clipboard.writeText(fp);
      showToast(tr("Session fingerprint copied"));
    } catch {
      showToast(tr("Copy blocked by browser"));
    }
  }

  // Decide whether the current search term "looks like" a username we
  // could reasonably try to start a chat with: non-empty, no spaces,
  // not already the name of a visible chat.
  function computeStartHint(term: string, chats: typeof $visibleChats): boolean {
    const t = term.trim();
    if (t.length === 0) return false;
    if (t.includes("://")) return false;
    if (t.includes(" ")) return false;
    const lower = t.toLowerCase();
    return !chats.some(
      (c) => c.name.toLowerCase() === lower || c.id.toLowerCase() === lower,
    );
  }
  let searchTerm   = $derived(($search ?? "").trim());
  let showStartHint = $derived(computeStartHint($search ?? "", $visibleChats));

  // Live server-directory search. Debounced; results shown under the
  // "Start chat with" row so you can find people you've never chatted
  // with, by username OR display name.
  let dirResults = $state<DirProfile[]>([]);
  let dirSearching = $state(false);
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;

  $effect(() => {
    const q = $search.trim();
    if (debounceTimer) clearTimeout(debounceTimer);
    if (q.length < 2 || !$info?.connected) { dirResults = []; return; }
    debounceTimer = setTimeout(async () => {
      dirSearching = true;
      try {
        const r = await api.searchDirectory(q);
        // Hide people we already have a chat with (shown above).
        const existing = new Set($visibleChats.map((c) => c.id));
        dirResults = r.filter((p) => !existing.has(p.username));
      } catch {
        dirResults = [];
      } finally {
        dirSearching = false;
      }
    }, 280);
  });

  async function startWithDir(p: DirProfile) {
    if (p.display_name || p.phone) {
      saveContact(p.username, { full_name: p.display_name, phone: p.phone });
    }
    if (p.is_bot) {
      // Bots use a plaintext channel (no X3DH).
      try {
        await api.startBotChat(p.username, "/start");
        const { chats, selectChat, activeTab } = await import("../stores");
        chats.set(await api.listChats());
        activeTab.set("chats");
        await selectChat(p.username);
        search.set("");
      } catch (e) {
        showToast(`Could not start bot: ${e}`);
      }
      return;
    }
    const err = await startChat(p.username, "Привет!", {
      full_name: p.display_name || undefined,
      phone: p.phone || undefined,
    });
    if (err) { showToast(err); return; }
    search.set("");
  }
</script>

<aside class="list">
  <div class="list-header">
    <label class="search">
      {@html icons.search}
      <input
        id="search"
        placeholder={$t("Search by name, phone or username")}
        autocomplete="off"
        bind:value={$search}
        onkeydown={onSearchKey}
        onblur={() => (kbIndex = -1)}
      />
      <span class="search-kbd">{$t("Ctrl K")}</span>
    </label>
    <div class="plus-wrap">
      <button
        class="plus-btn"
        class:nudge={$visibleChats.every((c) => !c.live)}
        onclick={() => (plusMenuOpen = !plusMenuOpen)}
        title={$t("New chat, group or channel")}
        aria-label={$t("New")}
      >
        <span class="plus-glyph">+</span>
      </button>
      {#if plusMenuOpen}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div class="plus-pop" role="menu" tabindex="-1">
          <button onclick={() => { plusMenuOpen = false; openNewChatDialog(); }}>{$t("New chat")}</button>
          <button onclick={() => { plusMenuOpen = false; openNewGroup("group"); }}>{$t("New group")}</button>
          <button onclick={() => { plusMenuOpen = false; openNewGroup("channel"); }}>{$t("New channel")}</button>
        </div>
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div class="plus-scrim" onclick={() => (plusMenuOpen = false)}></div>
      {/if}
    </div>
  </div>

  {#if $folders.length > 0}
    <div class="folder-strip">
      <button class="folder-tab" class:active={$activeFolder === "all"} onclick={() => activeFolder.set("all")}>{$t("All")}</button>
      {#each $folders as f (f.id)}
        <button class="folder-tab" class:active={$activeFolder === f.id} onclick={() => activeFolder.set(f.id)}>
          {f.name}{#if f.chatIds.length > 0}<span class="ft-count">{f.chatIds.length}</span>{/if}
        </button>
      {/each}
      <button class="folder-edit" onclick={openFolderManager} title={$t("Manage folders")}>+</button>
    </div>
  {:else}
    <div class="folder-strip empty-strip">
      <button class="folder-tab active">{$t("All")}</button>
      <button class="folder-edit" onclick={openFolderManager} title={$t("Create folders")}>{$t("+ Folders")}</button>
    </div>
  {/if}

  <div class="chats">
    {#if joinLink}
      <div class="join-card">
        <div class="jc-ava">{joinLink.title.charAt(0).toUpperCase()}</div>
        <div class="jc-body">
          <div class="jc-title">{joinLink.title}</div>
          <div class="jc-sub">Invite link · admin @{joinLink.admin}</div>
        </div>
        {#if joinIsMember}
          <button class="jc-btn" onclick={() => { const id = joinLink!.gid; search.set(""); void selectChat(id); }}>{$t("Open")}</button>
        {:else if joinSent === joinLink.gid}
          <span class="jc-done">{$t("Requested")}</span>
        {:else}
          <button class="jc-btn" onclick={requestJoin}>{$t("Request to join")}</button>
        {/if}
      </div>
    {/if}
    {#if !$search.trim() && showArchive}
      <button class="arch-back" onclick={() => (showArchive = false)}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 18 9 12 15 6"/></svg>
        {$t("Archived chats")}
      </button>
    {:else if !$search.trim() && archivedChats.length > 0 && $activeFolder === "all"}
      <button class="chat-item arch-row" onclick={() => (showArchive = true)}>
        <div class="arch-ic">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="5" rx="1.5"/><path d="M5 9v9a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V9"/><line x1="10" y1="13" x2="14" y2="13"/></svg>
        </div>
        <div class="chat-body">
          <div class="chat-row-1"><span class="chat-name">{$t("Archived chats")}</span></div>
          <div class="chat-row-2">
            <span class="chat-preview">{archivedChats.map((c) => c.name).slice(0, 4).join(", ")}</span>
            {#if archivedUnread > 0}<span class="chat-badge muted">{archivedUnread}</span>{/if}
          </div>
        </div>
      </button>
    {/if}
    {#if dirResults.length > 0}
      <div class="dir-head">{$t("Found on the server")}</div>
      {#each dirResults as p (p.username)}
        <button class="dir-row" onclick={() => startWithDir(p)} title={`Start a chat with @${p.username}`}>
          <div class="dir-avatar">{(p.display_name || p.username).charAt(0).toUpperCase()}</div>
          <div class="dir-body">
            <div class="dir-name">
              {p.display_name || p.username}
              {#if p.is_bot}<span class="bot-tag">BOT</span>{/if}
            </div>
            <div class="dir-sub">@{p.username}{#if p.about} · {p.about}{/if}</div>
          </div>
          <span class="dir-go">+</span>
        </button>
      {/each}
    {/if}
    {#if showStartHint && dirResults.length === 0 && !dirSearching}
      <button
        class="start-row"
        onclick={() => openNewChatDialog(searchTerm)}
        title={`Start a new chat with "${searchTerm}" on the server`}
      >
        <div class="start-icon">{@html icons.edit}</div>
        <div class="start-body">
          <div class="start-title">{$t("Start chat with")} <b>“{searchTerm}”</b></div>
          <div class="start-sub">{$t("No directory match — click to open a chat with that exact username")}</div>
        </div>
      </button>
    {/if}
    {#each shownChats as c, ci (c.id)}
      <button
        class="chat-item"
        class:active={c.id === $activeId}
        class:kb={ci === kbIndex}
        onclick={() => selectChat(c.id)}
        oncontextmenu={(e) => openCtx(e, c.id)}
      >
        <div class="avatar sz-44" class:online={c.online} style="--gradient: {c.gradient}">
          {c.letter}
          {#if $avatars[c.id]}<img class="ava-img" src={$avatars[c.id]} alt="" loading="lazy" />{/if}
        </div>
        <div class="chat-body">
          <div class="chat-row-1">
            <span class="chat-name">
              <span class="cn-text">{c.name}</span>
              {#if c.verified}<span class="verified-badge">{@html icons.verified}</span>{/if}
              {#if $clans[c.id]}<ClanTag tag={$clans[c.id]} />{/if}
              {#if $mutedChats.has(c.id)}<span class="muted-ic" title={$t("Muted")}><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M11 5 6 9H2v6h4l5 4V5z"/><line x1="23" y1="9" x2="17" y2="15"/><line x1="17" y1="9" x2="23" y2="15"/></svg></span>{/if}
            </span>
            <span class="chat-time">{c.time}</span>
          </div>
          <div class="chat-row-2">
            {#if c.preview_is_typing}
              <span class="chat-preview typing">{c.preview}</span>
            {:else if $peerPresence[c.id]?.activity?.[0]}
              {@const a = $peerPresence[c.id].activity![0]}
              <span class="chat-preview activity" title={`${activityVerb(a)} ${a.name}${a.details ? " — " + a.details : ""}`}>{activityVerb(a)} <b>{a.name}</b></span>
            {:else}
              <span class="chat-preview">
                {#if c.has_lock}<span class="lock-mini">{@html icons.lock}</span>{/if}
                {#if c.preview_by_me}<span class="me-prefix">{$t("you:")}</span>{/if}{c.preview}
              </span>
            {/if}
            {#if c.unread > 0 && c.id !== $activeId}
              <span class="chat-badge" class:muted={c.channel || $mutedChats.has(c.id)}>{c.unread}</span>
            {/if}
          </div>
        </div>
      </button>
    {/each}
  </div>

  {#if ctx}
    {@const cid = ctx.id}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div class="ctx-scrim" onclick={() => (ctx = null)} oncontextmenu={(e) => { e.preventDefault(); ctx = null; }}></div>
    <div class="chat-ctx" use:keepOnScreen style:left="{ctx.x}px" style:top="{ctx.y}px" role="menu">
      <button onclick={() => archiveChat(cid)}>{$t($archived.has(cid) ? "Unarchive" : "Archive")}</button>
      <button onclick={() => { setMuted(cid); ctx = null; }}>{$t($mutedChats.has(cid) ? "Unmute" : "Mute notifications")}</button>
      <button onclick={() => { void selectChat(cid); ctx = null; }}>{$t("Open")}</button>
    </div>
  {/if}
</aside>

<style>
  .chat-item.kb { box-shadow: inset 0 0 0 2px var(--accent); }
  .cn-text { overflow: hidden; text-overflow: ellipsis; min-width: 0; }
  .muted-ic { display: inline-grid; color: var(--fg-dim); flex-shrink: 0; }
  .muted-ic svg { width: 13px; height: 13px; }
  .arch-ic {
    width: 44px; height: 44px; border-radius: 50%; display: grid; place-items: center;
    background: var(--bg-input); color: var(--fg-muted); border: 1px solid var(--border);
  }
  .arch-ic svg { width: 21px; height: 21px; }
  .arch-back {
    display: flex; align-items: center; gap: 8px; width: 100%; padding: 8px 10px 10px;
    background: none; border: none; color: var(--fg); font: inherit; font-weight: 800; font-size: 14px; cursor: pointer;
  }
  .arch-back svg { width: 18px; height: 18px; color: var(--accent); }
  .arch-back:hover { color: var(--accent); }
  .join-card {
    display: flex; align-items: center; gap: 11px; margin: 4px 6px 8px; padding: 10px;
    border-radius: 12px; border: 1px solid color-mix(in srgb, var(--accent) 35%, transparent);
    background: color-mix(in srgb, var(--accent) 7%, transparent);
  }
  .jc-ava {
    width: 40px; height: 40px; border-radius: 50%; flex-shrink: 0; display: grid; place-items: center;
    background: linear-gradient(135deg, var(--accent), var(--accent-2)); color: #06070C; font-weight: 800;
  }
  .jc-body { flex: 1; min-width: 0; }
  .jc-title { font-weight: 700; font-size: 13.5px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .jc-sub { font-size: 11.5px; color: var(--fg-dim); }
  .jc-btn { border: none; border-radius: 8px; padding: 7px 11px; background: var(--accent); color: #06070C; font: inherit; font-size: 12px; font-weight: 800; cursor: pointer; white-space: nowrap; }
  .jc-done { font-size: 12px; color: var(--fg-muted); font-weight: 700; }
  .ctx-scrim { position: fixed; inset: 0; z-index: 70; }
  .chat-ctx {
    position: fixed; z-index: 71; min-width: 180px; padding: 5px;
    background: var(--bg-shell); border: 1px solid var(--border-strong); border-radius: 12px;
    box-shadow: 0 18px 44px -14px rgba(0,0,0,0.6); display: flex; flex-direction: column; gap: 2px;
  }
  .chat-ctx button {
    text-align: left; padding: 8px 12px; border-radius: 8px; border: none; background: transparent;
    color: var(--fg); font: inherit; font-size: 13.5px; cursor: pointer;
  }
  .chat-ctx button:hover { background: var(--accent-soft); color: var(--accent); }
  .chat-preview.activity { color: var(--accent); }
  .chat-preview.activity b { font-weight: 700; }
  .list {
    background: var(--bg-list);
    border-right: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .list-header {
    padding: 14px 14px 10px;
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .search {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    background: var(--bg-input);
    border: 1px solid var(--border);
    border-radius: 10px;
    transition: border-color 0.15s ease, box-shadow 0.15s ease;
  }
  .search:focus-within {
    border-color: var(--accent-soft);
    box-shadow: 0 0 0 3px var(--accent-soft);
  }
  .search :global(svg) { width: 15px; height: 15px; color: var(--fg-dim); flex-shrink: 0; }
  .search input {
    flex: 1;
    background: transparent;
    border: none;
    outline: none;
    color: var(--fg);
    font-family: inherit;
    font-size: 13.5px;
    min-width: 0;
  }
  .search input::placeholder { color: var(--fg-dim); }
  .search-kbd {
    font-family: var(--font-mono);
    font-size: 10.5px;
    color: var(--fg-dim);
    background: rgba(255,255,255,0.04);
    padding: 2px 5px;
    border-radius: 4px;
    border: 1px solid var(--border);
  }
  /* Small square "+" button next to the search bar. Pulses while
     the user has no real chats yet so it reads as the primary
     action, otherwise it sits quietly. */
  .plus-wrap { position: relative; flex-shrink: 0; }
  .plus-pop {
    position: absolute; top: 42px; right: 0; z-index: 40;
    background: var(--bg-shell); border: 1px solid var(--border-strong);
    border-radius: 12px; padding: 5px; min-width: 160px;
    box-shadow: 0 18px 44px -14px rgba(0,0,0,0.6);
    display: flex; flex-direction: column; gap: 2px;
  }
  .plus-pop button {
    text-align: left; padding: 9px 12px; border-radius: 8px; border: none;
    background: transparent; color: var(--fg); font: inherit; font-size: 13.5px; font-weight: 500; cursor: pointer;
  }
  .plus-pop button:hover { background: var(--accent-soft); color: var(--accent); }
  .plus-scrim { position: fixed; inset: 0; z-index: 30; }
  .plus-btn {
    width: 34px; height: 34px;
    display: grid; place-items: center;
    background: linear-gradient(135deg, var(--accent) 0%, var(--accent-2) 100%);
    color: #06070C;
    border: none;
    border-radius: 10px;
    cursor: pointer;
    flex-shrink: 0;
    box-shadow: 0 4px 14px -6px var(--accent-glow);
    transition: all 0.15s ease;
  }
  .plus-btn:hover {
    transform: translateY(-1px);
    box-shadow: 0 6px 18px -4px var(--accent-glow);
  }
  .plus-glyph {
    font-size: 20px;
    font-weight: 800;
    line-height: 1;
    display: inline-block;
    margin-top: -1px;
  }
  .plus-btn.nudge {
    animation: plus-pulse 1.8s ease-in-out infinite;
  }
  @keyframes plus-pulse {
    0%, 100% { box-shadow: 0 4px 14px -6px var(--accent-glow), 0 0 0 0 var(--accent-glow); }
    50%      { box-shadow: 0 4px 14px -6px var(--accent-glow), 0 0 0 6px rgba(0, 229, 199, 0); }
  }

  /* "Start chat with …" row shown at the top of the list when the
     search term doesn't match an existing chat. One click opens the
     new-chat dialog with the typed name pre-filled. */
  .dir-head {
    font-size: 10.5px;
    font-weight: 700;
    color: var(--fg-dim);
    text-transform: uppercase;
    letter-spacing: 0.08em;
    padding: 8px 12px 4px;
  }
  .dir-row {
    display: grid;
    grid-template-columns: 36px 1fr auto;
    gap: 11px;
    align-items: center;
    width: calc(100% - 12px);
    margin: 0 6px 2px;
    padding: 8px 10px;
    border-radius: 10px;
    background: transparent;
    border: none;
    text-align: left;
    color: inherit;
    cursor: pointer;
  }
  .dir-row:hover { background: var(--bg-hover); }
  .dir-avatar {
    width: 36px; height: 36px;
    border-radius: 50%;
    background: linear-gradient(135deg, var(--accent), var(--accent-2));
    display: grid; place-items: center;
    font-weight: 800; font-size: 14px; color: #06070C;
  }
  .dir-body { min-width: 0; }
  .dir-name { font-weight: 600; font-size: 13.5px; display: flex; align-items: center; gap: 6px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .bot-tag { font-size: 9px; font-weight: 800; background: var(--accent-soft); color: var(--accent); padding: 1px 5px; border-radius: 4px; letter-spacing: 0.05em; }
  .dir-sub { font-size: 11.5px; color: var(--fg-dim); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .dir-go { font-size: 20px; font-weight: 700; color: var(--accent); padding-right: 6px; }

  .start-row {
    display: grid;
    grid-template-columns: 44px 1fr;
    gap: 12px;
    padding: 10px 10px;
    margin: 4px 6px 8px;
    border-radius: 10px;
    cursor: pointer;
    background: linear-gradient(135deg, rgba(0,229,199,0.08), rgba(123,92,255,0.06));
    border: 1px dashed rgba(0, 229, 199, 0.35);
    text-align: left;
    color: var(--fg);
    width: calc(100% - 12px);
    transition: all 0.12s ease;
    align-items: center;
  }
  .start-row:hover {
    background: linear-gradient(135deg, rgba(0,229,199,0.14), rgba(123,92,255,0.10));
    border-style: solid;
  }
  .start-icon {
    width: 44px; height: 44px;
    border-radius: 12px;
    background: linear-gradient(135deg, var(--accent) 0%, var(--accent-2) 100%);
    color: #06070C;
    display: grid; place-items: center;
    flex-shrink: 0;
  }
  .start-icon :global(svg) { width: 18px; height: 18px; }
  .start-title { font-size: 13.5px; font-weight: 500; color: var(--fg); }
  .start-title :global(b) { color: var(--accent); }
  .start-sub   { font-size: 11.5px; color: var(--fg-dim); margin-top: 2px; }

  /* Folder strip (Telegram-style) */
  .folder-strip {
    display: flex; align-items: center; gap: 4px;
    padding: 0 10px 8px;
    overflow-x: auto;
    border-bottom: 1px solid var(--border);
  }
  .folder-strip.empty-strip { padding-bottom: 8px; }
  .folder-tab {
    display: inline-flex; align-items: center; gap: 5px;
    padding: 5px 12px; border-radius: 100px;
    background: transparent; border: 1px solid transparent;
    color: var(--fg-muted); font-size: 12.5px; font-weight: 600;
    cursor: pointer; flex-shrink: 0; white-space: nowrap;
  }
  .folder-tab:hover { color: var(--fg); background: var(--bg-hover); }
  .folder-tab.active { color: var(--accent); background: var(--accent-soft); border-color: rgba(0,229,199,0.25); }
  .ft-count {
    font-size: 10px; font-variant-numeric: tabular-nums;
    background: var(--bg-input); color: var(--fg-dim);
    border-radius: 100px; padding: 0 5px;
  }
  .folder-tab.active .ft-count { background: rgba(0,229,199,0.2); color: var(--accent); }
  .folder-edit {
    flex-shrink: 0;
    padding: 5px 10px; border-radius: 100px;
    background: transparent; border: 1px dashed var(--border-strong);
    color: var(--fg-dim); font-size: 12px; font-weight: 600; cursor: pointer;
    white-space: nowrap;
  }
  .folder-edit:hover { color: var(--accent); border-color: var(--accent-soft); }

  .chats {
    flex: 1;
    overflow-y: auto;
    padding: 4px 6px 6px;
  }

  .chat-item {
    display: grid;
    grid-template-columns: 44px 1fr;
    gap: 12px;
    padding: 10px 10px;
    border-radius: 10px;
    cursor: pointer;
    transition: background-color 0.12s ease;
    align-items: center;
    min-width: 0;
    width: 100%;
    background: transparent;
    border: none;
    text-align: left;
    color: inherit;
  }
  .chat-item:hover { background: var(--bg-hover); }
  .chat-item.active {
    background: var(--bg-active);
    box-shadow: inset 3px 0 0 var(--accent);
  }
  .chat-body { min-width: 0; }
  .chat-row-1 {
    display: flex; align-items: center; gap: 6px;
    justify-content: space-between;
  }
  .chat-name {
    font-weight: 600;
    font-size: 14.5px;
    color: var(--fg);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .chat-time { font-size: 11.5px; color: var(--fg-dim); flex-shrink: 0; font-variant-numeric: tabular-nums; }
  .chat-item.active .chat-time { color: var(--accent); }
  .chat-row-2 {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 3px;
    justify-content: space-between;
  }
  .chat-preview {
    font-size: 13px;
    color: var(--fg-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
    flex: 1;
    display: flex;
    align-items: center;
    gap: 5px;
  }
  .lock-mini { display: inline-flex; align-items: center; }
  .lock-mini :global(svg) { width: 11px; height: 11px; color: var(--accent); opacity: 0.7; flex-shrink: 0; }
  .me-prefix { color: var(--fg-dim); flex-shrink: 0; }
  .chat-preview.typing { color: var(--accent); }
  .chat-badge {
    background: var(--accent);
    color: #06070C;
    font-weight: 700;
    font-size: 11px;
    padding: 2px 7px;
    border-radius: 10px;
    min-width: 18px;
    text-align: center;
    box-shadow: 0 0 12px -2px var(--accent-glow);
    font-variant-numeric: tabular-nums;
    flex-shrink: 0;
  }
  .chat-badge.muted {
    background: var(--fg-dim);
    color: var(--fg);
    box-shadow: none;
  }

  .session-info {
    padding: 10px 14px;
    border-top: 1px solid var(--border);
    background: rgba(0,0,0,0.15);
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 11.5px;
    color: var(--fg-dim);
  }
  .session-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  .session-fp {
    font-family: var(--font-mono);
    color: var(--fg-muted);
    letter-spacing: 0.02em;
  }
  .copy-fp {
    background: transparent;
    border: none;
    color: var(--fg-dim);
    cursor: pointer;
    padding: 2px;
    display: grid; place-items: center;
    border-radius: 4px;
  }
  .copy-fp:hover { color: var(--accent); background: var(--bg-hover); }
  .copy-fp :global(svg) { width: 12px; height: 12px; }
  .data-dir .dir-path {
    direction: rtl;
    text-align: left;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 220px;
    font-size: 10.5px;
  }
  .accent-dot {
    width: 6px; height: 6px; border-radius: 50%;
    background: var(--accent);
    box-shadow: 0 0 6px var(--accent-glow);
    display: inline-block;
    margin-right: 4px;
    animation: pulse 2s ease-in-out infinite;
  }
  @keyframes pulse {
    0%, 100% { opacity: 1; transform: scale(1); }
    50%      { opacity: 0.5; transform: scale(0.85); }
  }
  .verified-badge :global(svg) { width: 13px; height: 13px; color: var(--accent); filter: drop-shadow(0 0 4px var(--accent-glow)); }
</style>
