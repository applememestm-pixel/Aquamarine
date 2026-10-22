<script lang="ts">
  /**
   * One chat group (lib/chatGroups.ts): its chats with search, "Show as one
   * chat", rename, add / remove chats, ungroup. Opens from the group's row
   * in the chat list or its tag next to a chat's name.
   */
  import { chatGroups, openGroupId, setCollapsed, renameGroup, removeFromGroup, addToGroup, deleteGroup } from "../chatGroups";
  import { chats, selectChat } from "../stores";
  import { avatars } from "../people";
  import { mutedChats } from "../chatPrefs";
  import { t, tr, tn } from "../i18n";
  import { askInput, closeInput } from "./InputPrompt.svelte";
  import { plainText, hasMarkup } from "../markup";

  let group = $derived($chatGroups.find((g) => g.id === $openGroupId) ?? null);
  let members = $derived(group ? $chats.filter((c) => group!.chatIds.includes(c.id)) : []);
  let query = $state("");
  let searching = $state(false);
  let adding = $state(false);
  let shown = $derived(members.filter((c) => !query.trim() || c.name.toLowerCase().includes(query.trim().toLowerCase())));
  let others = $derived(group ? $chats.filter((c) => !group!.chatIds.includes(c.id) && (!query.trim() || c.name.toLowerCase().includes(query.trim().toLowerCase()))) : []);
  $effect(() => { if (!$openGroupId) { query = ""; searching = false; adding = false; } });

  const preview = (p: string) => (p && hasMarkup(p) ? plainText(p).replace(/\s*\n\s*/g, " ") : p);
  function close() { openGroupId.set(null); }
  function open(id: string) { close(); void selectChat(id); }
  function rename() {
    if (!group) return;
    const g = group;
    askInput({
      title: tr("Rename the group"),
      fields: [{ label: tr("Name"), value: g.name }, { label: tr("Emoji (optional)"), value: g.emoji ?? "" }],
      submitLabel: tr("Save"),
      onSubmit: async ([n, e]) => {
        if (!(n ?? "").trim()) return tr("Enter a name");
        renameGroup(g.id, n ?? "", e ?? "");
        closeInput();
        return null;
      },
    });
  }
  function onKey(e: KeyboardEvent) { if (e.key === "Escape" && $openGroupId) { e.stopPropagation(); close(); } }
</script>

<svelte:window onkeydown={onKey} />

{#if group}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="veil" onclick={close}>
    <div class="sheet" role="dialog" aria-modal="true" tabindex="-1" onclick={(e) => e.stopPropagation()}>
      <button class="x" onclick={close} aria-label={$t("Close")}>✕</button>
      <div class="head">
        <div class="stack big">
          {#if group.emoji}
            <span class="emo">{group.emoji}</span>
          {:else}
            {#each members.slice(0, 3) as m, i (m.id)}
              <span class="mini avatar" style="--gradient: {m.gradient}; --k: {i}">{m.letter}{#if $avatars[m.id]}<img class="ava-img" src={$avatars[m.id]} alt="" />{/if}</span>
            {/each}
          {/if}
        </div>
        <button class="name" onclick={rename} title={$t("Rename")}>{group.name} <span class="pen">✎</span></button>
        <div class="count">{$tn(members.length, "chat")}</div>
      </div>

      <div class="tools">
        <button class="tool" class:on={searching} onclick={() => { searching = !searching; adding = false; query = ""; }}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/></svg>
          <span>{$t("Search")}</span>
        </button>
        <button class="tool" class:on={adding} onclick={() => { adding = !adding; searching = false; query = ""; }}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg>
          <span>{$t("Add chats")}</span>
        </button>
      </div>
      {#if searching || adding}
        <!-- svelte-ignore a11y_autofocus -->
        <input class="q" bind:value={query} autofocus placeholder={$t(adding ? "Find a chat to add" : "Search in the group")} />
      {/if}

      {#if !adding}
        <button class="toggle" onclick={() => setCollapsed(group!.id, !group!.collapsed)}>
          <span>{$t("Show as one chat")}</span>
          <span class="switch" class:on={group.collapsed}></span>
        </button>
        <div class="hint">{$t("Fold all chats of the group into one row of the chat list.")}</div>
      {/if}

      <div class="sec">{$t(adding ? "Add to the group" : "Chats in the group")}</div>
      <div class="list">
        {#if adding}
          {#each others as c (c.id)}
            <button class="row" onclick={() => addToGroup(group!.id, c.id)}>
              <span class="avatar sz-40" style="--gradient: {c.gradient}">{c.letter}{#if $avatars[c.id]}<img class="ava-img" src={$avatars[c.id]} alt="" />{/if}</span>
              <span class="body"><span class="r1"><b>{c.name}</b></span><span class="r2">{preview(c.preview)}</span></span>
              <span class="plus">＋</span>
            </button>
          {:else}
            <div class="empty">{$t("No other chats")}</div>
          {/each}
        {:else}
          {#each shown as c, i (c.id)}
            <div class="row" class:alt={i % 2 === 1}>
              <button class="open" onclick={() => open(c.id)}>
                <span class="avatar sz-40" style="--gradient: {c.gradient}">{c.letter}{#if $avatars[c.id]}<img class="ava-img" src={$avatars[c.id]} alt="" />{/if}</span>
                <span class="body">
                  <span class="r1"><b>{c.name}</b><span class="time">{c.time}</span></span>
                  <span class="r2"><span class="pv">{#if c.preview_by_me}{$t("you:")} {/if}{preview(c.preview)}</span>
                    {#if c.unread > 0}<span class="badge" class:muted={$mutedChats.has(c.id)}>{c.unread}</span>{/if}</span>
                </span>
              </button>
              <button class="rm" onclick={() => removeFromGroup(c.id)} title={$t("Remove from the group")} aria-label={$t("Remove from the group")}>✕</button>
            </div>
          {:else}
            <div class="empty">{$t("Nothing found")}</div>
          {/each}
        {/if}
      </div>

      <button class="ungroup" onclick={() => deleteGroup(group!.id)}>{$t("Ungroup (the chats stay)")}</button>
    </div>
  </div>
{/if}

<style>
  .veil { position: fixed; inset: 0; z-index: 95; display: grid; place-items: center; padding: 16px;
    background: rgba(0,0,0,.45); backdrop-filter: blur(10px); -webkit-backdrop-filter: blur(10px); animation: fade .18s ease-out; }
  @keyframes fade { from { opacity: 0; } }
  .sheet { position: relative; width: min(420px, 100%); max-height: calc(100vh - 32px); overflow-y: auto; padding: 22px 16px 16px;
    background: var(--bg-shell); border: 1px solid var(--border-strong); border-radius: 18px; box-shadow: 0 30px 70px -24px rgba(0,0,0,.75);
    animation: pop .26s cubic-bezier(.2,.9,.25,1.12); }
  @keyframes pop { from { opacity: 0; transform: scale(.92) translateY(8px); } }
  .x { position: absolute; top: 12px; right: 12px; width: 30px; height: 30px; border: none; border-radius: 9px; background: transparent; color: var(--fg-muted); cursor: pointer; font-size: 14px; }
  .x:hover { background: var(--bg-hover); color: var(--fg); }
  .head { display: flex; flex-direction: column; align-items: center; gap: 6px; margin-bottom: 14px; }
  .stack { position: relative; width: 44px; height: 44px; flex-shrink: 0; }
  .stack.big { width: 84px; height: 84px; }
  .stack .mini { position: absolute; width: 60%; height: 60%; font-size: 12px; border: 2px solid var(--bg-shell);
    left: calc(var(--k) * 20%); top: calc(var(--k) * 20%); z-index: calc(3 - var(--k)); }
  .stack.big .mini { font-size: 18px; border-width: 3px; }
  .emo { display: grid; place-items: center; width: 100%; height: 100%; border-radius: 50%; background: var(--bg-input); font-size: 40px; }
  .name { border: none; background: none; color: var(--fg); font: inherit; font-size: 18px; font-weight: 800; cursor: pointer; }
  .pen { font-size: 13px; color: var(--fg-dim); }
  .count { font-size: 12.5px; color: var(--fg-dim); }
  .tools { display: flex; gap: 8px; margin-bottom: 10px; }
  .tool { flex: 1; display: flex; flex-direction: column; align-items: center; gap: 4px; padding: 10px; border-radius: 12px; border: 1px solid var(--border);
    background: var(--bg-input); color: var(--fg-muted); font: inherit; font-size: 12px; cursor: pointer; }
  .tool svg { width: 18px; height: 18px; }
  .tool:hover, .tool.on { color: var(--accent); border-color: color-mix(in srgb, var(--accent) 40%, transparent); }
  .q { width: 100%; box-sizing: border-box; margin-bottom: 10px; padding: 9px 12px; border-radius: 10px; border: 1px solid var(--border); background: var(--bg-input); color: var(--fg); font: inherit; outline: none; }
  .q:focus { border-color: var(--accent); }
  .toggle { width: 100%; display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 11px 12px; border-radius: 12px; border: none;
    background: var(--bg-input); color: var(--fg); font: inherit; font-size: 14px; cursor: pointer; }
  .switch { width: 38px; height: 22px; border-radius: 11px; background: var(--border-strong); position: relative; flex-shrink: 0; transition: background .2s; }
  .switch::after { content: ""; position: absolute; top: 3px; left: 3px; width: 16px; height: 16px; border-radius: 50%; background: #fff; transition: transform .2s cubic-bezier(.2,.9,.3,1.2); }
  .switch.on { background: var(--accent); }
  .switch.on::after { transform: translateX(16px); }
  .hint { font-size: 12px; color: var(--fg-dim); padding: 6px 4px 0; }
  .sec { margin: 14px 4px 6px; font-size: 12px; font-weight: 700; color: var(--accent); }
  .list { display: flex; flex-direction: column; gap: 2px; }
  .row { display: flex; align-items: center; gap: 4px; border-radius: 12px; }
  button.row { width: 100%; gap: 11px; padding: 8px; border: none; background: transparent; color: var(--fg); font: inherit; text-align: left; cursor: pointer; }
  .row:hover { background: var(--bg-hover); }
  .open { flex: 1; min-width: 0; display: flex; align-items: center; gap: 11px; padding: 8px; border: none; background: transparent; color: var(--fg); font: inherit; text-align: left; cursor: pointer; border-radius: 12px; }
  .body { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 2px; }
  .r1 { display: flex; gap: 8px; align-items: baseline; }
  .r1 b { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 14px; }
  .time { font-size: 11.5px; color: var(--fg-dim); }
  .r2 { display: flex; gap: 8px; align-items: center; font-size: 12.5px; color: var(--fg-muted); }
  .pv { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .badge { min-width: 18px; padding: 0 6px; height: 18px; border-radius: 9px; background: var(--accent); color: #06070C; font-size: 11px; font-weight: 800; display: grid; place-items: center; }
  .badge.muted { background: var(--fg-dim); }
  .rm { width: 28px; height: 28px; flex-shrink: 0; border: none; border-radius: 8px; background: transparent; color: var(--fg-dim); cursor: pointer; opacity: 0; }
  .row:hover .rm, .rm:focus-visible { opacity: 1; }
  @media (hover: none) { .rm { opacity: .7; } }
  .rm:hover { color: #F23F43; background: var(--bg-input); }
  .plus { color: var(--accent); font-size: 18px; }
  .empty { padding: 14px; text-align: center; color: var(--fg-dim); font-size: 13px; }
  .ungroup { width: 100%; margin-top: 14px; padding: 10px; border-radius: 12px; border: 1px solid var(--border); background: transparent; color: #F23F43; font: inherit; font-size: 13px; cursor: pointer; }
  .ungroup:hover { background: color-mix(in srgb, #F23F43 10%, transparent); }
</style>
