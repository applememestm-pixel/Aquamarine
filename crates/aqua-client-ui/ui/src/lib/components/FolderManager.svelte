<script module lang="ts">
  import { writable } from "svelte/store";
  /** null: closed; "": the list of folders; an id: that folder's settings. */
  export const folderManagerOpen = writable<string | null>(null);
  export function openFolderManager(id?: string | Event) { folderManagerOpen.set(typeof id === "string" ? id : ""); }
  export function closeFolderManager() { folderManagerOpen.set(null); }
</script>

<script lang="ts">
  /**
   * Chat folders like Telegram: the list (order, add, delete), and a
   * folder's own page — name and icon, tag colour, which kinds of chats it
   * takes in, which it leaves out, and chats added or left out by hand.
   */
  import { t, tr } from "../i18n";
  import { chats } from "../stores";
  import { contacts } from "../contacts";
  import { avatars } from "../people";
  import { mutedChats, markedUnread, archived } from "../chatPrefs";
  import { groupMap } from "../groups";
  import { folders, createFolder, deleteFolder, updateFolder, moveFolder, folderHas, folderTags, FOLDER_COLORS,
    type Folder, type FolderKind, type FolderSkip, type ChatFacts } from "../folders";
  import { askConfirm } from "./Confirm.svelte";
  import type { ChatSummary } from "../types";

  const KINDS: { id: FolderKind; label: string; ic: string }[] = [
    { id: "contacts", label: "Contacts", ic: "👤" },
    { id: "others", label: "Not in contacts", ic: "👥" },
    { id: "groups", label: "Groups", ic: "💬" },
    { id: "channels", label: "Channels", ic: "📢" },
  ];
  const SKIPS: { id: FolderSkip; label: string; ic: string }[] = [
    { id: "muted", label: "Muted", ic: "🔕" },
    { id: "read", label: "Read", ic: "✓" },
    { id: "archived", label: "Archived", ic: "🗄" },
  ];
  const ICONS = ["", "💬", "👥", "📢", "⭐", "💼", "🎮", "🎵", "📚", "❤️", "🏠", "✈️", "🔥", "🤖", "🛒", "💡"];

  let folder = $derived($folderManagerOpen ? $folders.find((f) => f.id === $folderManagerOpen) ?? null : null);
  let picking = $state<"in" | "out" | null>(null);
  let pickQuery = $state("");
  let iconMenu = $state(false);
  $effect(() => { void $folderManagerOpen; picking = null; pickQuery = ""; iconMenu = false; });

  function kindOf(c: ChatSummary): ChatFacts["kind"] {
    if (c.channel || $groupMap[c.id]?.kind === "channel") return "channels";
    if (c.id.startsWith("grp_")) return "groups";
    return "people";
  }
  function facts(c: ChatSummary): ChatFacts {
    return { id: c.id, kind: kindOf(c), contact: !!$contacts[c.id], muted: $mutedChats.has(c.id), unread: c.unread > 0 || $markedUnread.has(c.id), archived: $archived.has(c.id) };
  }
  const realChats = $derived($chats.filter((c) => !c.id.startsWith("__")));
  const countOf = (f: Folder) => realChats.filter((c) => folderHas(f, facts(c))).length;

  function add() {
    const id = createFolder(tr("New folder"));
    folderManagerOpen.set(id);
  }
  async function remove(f: Folder) {
    const ok = await askConfirm({ title: tr("Delete the folder “{name}”?", { name: f.name }), text: tr("The chats stay, only the folder goes."), ok: tr("Delete"), danger: true });
    if (!ok) return;
    deleteFolder(f.id);
    folderManagerOpen.set("");
  }
  function toggleKind(f: Folder, k: FolderKind) {
    const l = f.kinds ?? [];
    updateFolder(f.id, { kinds: l.includes(k) ? l.filter((x) => x !== k) : [...l, k] });
  }
  function toggleSkip(f: Folder, k: FolderSkip) {
    const l = f.skip ?? [];
    updateFolder(f.id, { skip: l.includes(k) ? l.filter((x) => x !== k) : [...l, k] });
  }
  function toggleIn(f: Folder, id: string) {
    const on = f.chatIds.includes(id);
    updateFolder(f.id, { chatIds: on ? f.chatIds.filter((x) => x !== id) : [...f.chatIds, id], excludeIds: (f.excludeIds ?? []).filter((x) => x !== id) });
  }
  function toggleOut(f: Folder, id: string) {
    const l = f.excludeIds ?? [];
    const on = l.includes(id);
    updateFolder(f.id, { excludeIds: on ? l.filter((x) => x !== id) : [...l, id], chatIds: on ? f.chatIds : f.chatIds.filter((x) => x !== id) });
  }
  const pickList = $derived(realChats.filter((c) => !pickQuery.trim() || c.name.toLowerCase().includes(pickQuery.trim().toLowerCase()) || c.id.includes(pickQuery.trim().toLowerCase())));
  const nameOf = (id: string) => $chats.find((c) => c.id === id)?.name ?? id;
  function back() {
    if (picking) { picking = null; return; }
    if (folder) { folderManagerOpen.set(""); return; }
    closeFolderManager();
  }
  function key(e: KeyboardEvent) {
    if ($folderManagerOpen === null || e.key !== "Escape") return;
    e.preventDefault(); e.stopPropagation(); back();
  }
</script>

<svelte:window onkeydowncapture={key} />

{#snippet ava(c: ChatSummary)}
  <span class="fm-av" style="--gradient: {c.gradient}">{c.letter}{#if $avatars[c.id]}<img src={$avatars[c.id]} alt="" loading="lazy" />{/if}</span>
{/snippet}

{#if $folderManagerOpen !== null}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="fm-backdrop" onclick={closeFolderManager}>
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div class="fm-sheet" role="dialog" aria-modal="true" tabindex="-1" aria-label={$t("Chat folders")} onclick={(e) => e.stopPropagation()}>
      <div class="fm-head">
        {#if folder}
          <button class="fm-ic" onclick={back} aria-label={$t("Back")}><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M15 18l-6-6 6-6"/></svg></button>
        {/if}
        <h3>{folder ? (picking === "in" ? $t("Add chats") : picking === "out" ? $t("Leave chats out") : $t("Edit folder")) : $t("Chat folders")}</h3>
        <button data-x class="fm-ic" onclick={closeFolderManager} aria-label={$t("Close")}><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M6 6l12 12M18 6 6 18"/></svg></button>
      </div>

      <div class="fm-body">
        {#if !folder}
          <p class="fm-note">{$t("Folders sort chats into tabs above the list. A folder can take in whole kinds of chats — say, all groups — and leave out muted or read ones.")}</p>
          <div class="fm-card">
            {#each $folders as f, i (f.id)}
              <div class="fm-row">
                <button class="fm-main" onclick={() => folderManagerOpen.set(f.id)}>
                  <span class="fm-fic">{f.emoji || "📁"}</span>
                  <span class="fm-txt"><b>{f.name}</b><small>{tr("{n} chats", { n: countOf(f) })}</small></span>
                  {#if f.color != null && f.color >= 0}<span class="fm-dot" style:background={FOLDER_COLORS[f.color]}></span>{/if}
                </button>
                <button class="fm-ic sm" disabled={i === 0} onclick={() => moveFolder(f.id, -1)} aria-label={$t("Move up")}><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 15l-6-6-6 6"/></svg></button>
                <button class="fm-ic sm" disabled={i === $folders.length - 1} onclick={() => moveFolder(f.id, 1)} aria-label={$t("Move down")}><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M6 9l6 6 6-6"/></svg></button>
              </div>
            {/each}
            <button class="fm-row fm-add" onclick={add}><span class="fm-fic">＋</span><b>{$t("Create a folder")}</b></button>
          </div>
          <label class="fm-row fm-switch">
            <span class="fm-txt"><b>{$t("Folder tags")}</b><small>{$t("Under each chat in “All”: which folders it's in")}</small></span>
            <input type="checkbox" bind:checked={$folderTags} /><span class="knob"></span>
          </label>
        {:else if picking}
          {@const f = folder}
          <input class="fm-search" placeholder={$t("Search")} bind:value={pickQuery} />
          <div class="fm-card fm-pick">
            {#each pickList as c (c.id)}
              {@const on = picking === "in" ? f.chatIds.includes(c.id) : (f.excludeIds ?? []).includes(c.id)}
              <button class="fm-row" class:on onclick={() => (picking === "in" ? toggleIn(f, c.id) : toggleOut(f, c.id))}>
                {@render ava(c)}
                <span class="fm-txt"><b>{c.name}</b><small>{c.id.startsWith("grp_") ? $t(kindOf(c) === "channels" ? "Channel" : "Group") : "@" + c.id}</small></span>
                <span class="fm-chk" class:on>{on ? "✓" : ""}</span>
              </button>
            {/each}
          </div>
        {:else}
          {@const f = folder}
          <div class="fm-namebar">
            <button class="fm-iconbtn" onclick={() => (iconMenu = !iconMenu)} title={$t("Icon")} aria-label={$t("Icon")}>{f.emoji || "📁"}</button>
            <input class="fm-name" value={f.name} maxlength="24" oninput={(e) => updateFolder(f.id, { name: (e.currentTarget as HTMLInputElement).value })} placeholder={$t("Folder name")} />
          </div>
          {#if iconMenu}
            <div class="fm-icons">
              {#each ICONS as ic (ic)}
                <button class:on={(f.emoji ?? "") === ic} onclick={() => { updateFolder(f.id, { emoji: ic || undefined }); iconMenu = false; }}>{ic || "📁"}</button>
              {/each}
            </div>
          {/if}

          <div class="fm-label">{$t("Included chats")}</div>
          <div class="fm-card">
            <button class="fm-row fm-add" onclick={() => (picking = "in")}><span class="fm-fic">＋</span><b>{$t("Add chats")}</b></button>
            {#each KINDS as k (k.id)}
              {@const on = (f.kinds ?? []).includes(k.id)}
              <button class="fm-row" class:on onclick={() => toggleKind(f, k.id)}>
                <span class="fm-fic">{k.ic}</span><span class="fm-txt"><b>{$t(k.label)}</b></span><span class="fm-chk" class:on>{on ? "✓" : ""}</span>
              </button>
            {/each}
            {#each f.chatIds as id (id)}
              {@const c = $chats.find((x) => x.id === id)}
              <div class="fm-row">
                {#if c}{@render ava(c)}{:else}<span class="fm-fic">💬</span>{/if}
                <span class="fm-txt"><b>{nameOf(id)}</b></span>
                <button class="fm-ic sm" onclick={() => toggleIn(f, id)} aria-label={$t("Remove")}><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M6 6l12 12M18 6 6 18"/></svg></button>
              </div>
            {/each}
          </div>
          <p class="fm-note">{$t("Choose chats or whole kinds of chats for this folder.")}</p>

          <div class="fm-label">{$t("Excluded chats")}</div>
          <div class="fm-card">
            <button class="fm-row fm-add" onclick={() => (picking = "out")}><span class="fm-fic">－</span><b>{$t("Leave chats out")}</b></button>
            {#each SKIPS as k (k.id)}
              {@const on = (f.skip ?? []).includes(k.id)}
              <button class="fm-row" class:on onclick={() => toggleSkip(f, k.id)}>
                <span class="fm-fic">{k.ic}</span><span class="fm-txt"><b>{$t(k.label)}</b></span><span class="fm-chk" class:on>{on ? "✓" : ""}</span>
              </button>
            {/each}
            {#each f.excludeIds ?? [] as id (id)}
              {@const c = $chats.find((x) => x.id === id)}
              <div class="fm-row">
                {#if c}{@render ava(c)}{:else}<span class="fm-fic">💬</span>{/if}
                <span class="fm-txt"><b>{nameOf(id)}</b></span>
                <button class="fm-ic sm" onclick={() => toggleOut(f, id)} aria-label={$t("Remove")}><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M6 6l12 12M18 6 6 18"/></svg></button>
              </div>
            {/each}
          </div>

          <div class="fm-label">{$t("Folder tag colour")}</div>
          <div class="fm-colors">
            {#each FOLDER_COLORS as col, i (i)}
              <button class="fm-col" class:on={f.color === i} style:--fc={col} onclick={() => updateFolder(f.id, { color: i })} aria-label={$t("Colour {n}", { n: i + 1 })}></button>
            {/each}
            <button class="fm-col none" class:on={f.color == null || f.color < 0} onclick={() => updateFolder(f.id, { color: -1 })} title={$t("No tag")} aria-label={$t("No tag")}>⌀</button>
            {#if f.color != null && f.color >= 0}<span class="fm-tagprev" style:--fc={FOLDER_COLORS[f.color]}>{f.name}</span>{/if}
          </div>

          <div class="fm-card fm-danger">
            <button class="fm-row" onclick={() => remove(f)}><b>{$t("Delete folder")}</b></button>
          </div>
          <p class="fm-note">{tr("{n} chats in this folder now", { n: countOf(f) })}</p>
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .fm-backdrop { position: fixed; inset: 0; background: rgba(6, 7, 12, 0.55); backdrop-filter: blur(4px); display: grid; place-items: center; z-index: 115; padding: 20px; animation: fm-fade 0.15s ease-out; }
  @keyframes fm-fade { from { opacity: 0; } }
  .fm-sheet { width: min(440px, 94vw); max-height: 86vh; display: flex; flex-direction: column; background: var(--bg-shell); border: 1px solid var(--border-strong);
    border-radius: calc(16px * var(--rk, 1)); overflow: hidden; box-shadow: 0 32px 80px -16px rgba(0,0,0,0.6); }
  :global(html.phone) .fm-backdrop { padding: 0; place-items: end stretch; }
  :global(html.phone) .fm-sheet { width: 100%; max-height: 92dvh; border-radius: calc(18px * var(--rk, 1)) calc(18px * var(--rk, 1)) 0 0; padding-bottom: env(safe-area-inset-bottom, 0px); }
  .fm-head { display: flex; align-items: center; gap: 6px; padding: 10px 10px 10px 16px; border-bottom: 1px solid var(--border); }
  .fm-head:has(.fm-ic:first-child) { padding-left: 8px; }
  .fm-head h3 { margin: 0; flex: 1; font-size: 15.5px; font-weight: 700; }
  .fm-ic { width: 34px; height: 34px; flex-shrink: 0; border: none; border-radius: 50%; background: transparent; color: var(--fg-muted); display: grid; place-items: center; cursor: pointer; }
  .fm-ic:hover:not(:disabled) { background: var(--bg-hover); color: var(--fg); }
  .fm-ic:disabled { opacity: .3; cursor: default; }
  .fm-ic svg { width: 18px; height: 18px; }
  .fm-ic.sm { width: 30px; height: 30px; }
  .fm-ic.sm svg { width: 16px; height: 16px; }
  .fm-body { overflow-y: auto; padding: 12px 12px 16px; display: flex; flex-direction: column; gap: 8px; }
  .fm-note { margin: 0 6px; font-size: 12px; line-height: 1.45; color: var(--fg-dim); }
  .fm-label { margin: 8px 6px 0; font-size: 12.5px; font-weight: 700; color: var(--accent); }
  .fm-card { display: flex; flex-direction: column; background: var(--bg-surface); border: 1px solid var(--border); border-radius: calc(14px * var(--rk, 1)); overflow: hidden; }
  .fm-row { display: flex; align-items: center; gap: 12px; width: 100%; min-height: 48px; box-sizing: border-box; padding: 6px 8px 6px 12px; border: none; background: transparent; color: var(--fg); text-align: left; font: inherit; cursor: pointer; }
  .fm-card > .fm-row + .fm-row, .fm-card > .fm-row + div.fm-row { border-top: 1px solid var(--border); }
  button.fm-row:hover, .fm-main:hover { background: var(--bg-hover); }
  .fm-main { display: flex; align-items: center; gap: 12px; flex: 1; min-width: 0; padding: 0; border: none; background: transparent; color: inherit; font: inherit; text-align: left; cursor: pointer; border-radius: calc(8px * var(--rk, 1)); }
  .fm-fic { width: 32px; height: 32px; flex-shrink: 0; display: grid; place-items: center; font-size: 19px; color: var(--accent); }
  .fm-txt { display: flex; flex-direction: column; gap: 1px; flex: 1; min-width: 0; }
  .fm-txt b { font-size: 14px; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .fm-txt small { font-size: 12px; color: var(--fg-dim); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .fm-dot { width: 10px; height: 10px; border-radius: 50%; flex-shrink: 0; margin-right: 4px; }
  .fm-add b { color: var(--accent); font-size: 14px; font-weight: 600; }
  .fm-chk { width: 22px; height: 22px; flex-shrink: 0; border-radius: 50%; border: 2px solid var(--border-strong); display: grid; place-items: center; font-size: 12px; font-weight: 800; color: #06070c; margin-right: 4px; transition: background .15s, border-color .15s; }
  .fm-chk.on { background: var(--accent); border-color: var(--accent); }
  .fm-av { position: relative; width: 32px; height: 32px; border-radius: var(--ava-r, 50%); overflow: hidden; display: grid; place-items: center; font-weight: 700; font-size: 13px;
    color: rgba(6,7,12,0.85); background: var(--gradient, var(--bg-surface-2)); flex-shrink: 0; }
  .fm-av img { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: cover; }
  .fm-switch { cursor: pointer; background: var(--bg-surface); border: 1px solid var(--border); border-radius: calc(14px * var(--rk, 1)); padding-right: 14px; }
  .fm-switch input { position: absolute; opacity: 0; pointer-events: none; }
  .knob { position: relative; width: 36px; height: 20px; flex-shrink: 0; border-radius: 10px; background: var(--border-strong); transition: background .18s; }
  .knob::after { content: ""; position: absolute; top: 2px; left: 2px; width: 16px; height: 16px; border-radius: 50%; background: #fff; transition: transform .18s; box-shadow: 0 1px 3px rgba(0,0,0,.35); }
  .fm-switch input:checked + .knob { background: var(--accent); }
  .fm-switch input:checked + .knob::after { transform: translateX(16px); }
  .fm-namebar { display: flex; align-items: center; gap: 8px; background: var(--bg-surface); border: 1px solid var(--border); border-radius: calc(14px * var(--rk, 1)); padding: 6px; }
  .fm-iconbtn { width: 40px; height: 40px; flex-shrink: 0; border: none; border-radius: calc(10px * var(--rk, 1)); background: var(--bg-input); font-size: 20px; cursor: pointer; }
  .fm-name { flex: 1; min-width: 0; height: 40px; border: none; outline: none; background: transparent; color: var(--fg); font: 600 15px var(--font-ui, inherit); }
  .fm-icons { display: grid; grid-template-columns: repeat(8, 1fr); gap: 4px; padding: 6px; background: var(--bg-surface); border: 1px solid var(--border); border-radius: calc(14px * var(--rk, 1)); }
  .fm-icons button { aspect-ratio: 1; border: none; border-radius: calc(10px * var(--rk, 1)); background: transparent; font-size: 19px; cursor: pointer; }
  .fm-icons button:hover, .fm-icons button.on { background: var(--accent-soft); }
  .fm-colors { display: flex; align-items: center; flex-wrap: wrap; gap: 10px; padding: 4px 6px; }
  .fm-col { width: 28px; height: 28px; border-radius: 50%; border: none; cursor: pointer; background: var(--fc); box-shadow: 0 0 0 2px var(--bg-shell); transition: transform .15s, box-shadow .15s; }
  .fm-col.on { box-shadow: 0 0 0 2px var(--bg-shell), 0 0 0 4px var(--fc, var(--fg-muted)); transform: scale(1.05); }
  .fm-col.none { background: var(--bg-input); color: var(--fg-dim); font-size: 15px; }
  .fm-tagprev { margin-left: auto; font-size: 11px; font-weight: 700; padding: 3px 8px; border-radius: calc(6px * var(--rk, 1)); color: var(--fc); background: color-mix(in srgb, var(--fc) 16%, transparent); }
  .fm-search { height: 38px; padding: 0 14px; border-radius: calc(19px * var(--rk, 1)); border: 1px solid var(--border); background: var(--bg-input); color: var(--fg); font: inherit; font-size: 14px; outline: none; }
  .fm-search:focus { border-color: var(--accent-soft); }
  .fm-pick { max-height: 60vh; overflow-y: auto; }
  .fm-danger b { color: var(--danger); font-size: 14px; font-weight: 600; padding-left: 4px; }
</style>
