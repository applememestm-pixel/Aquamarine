<script lang="ts">
  /** A small kanban board inside the chat: everyone moves and adds cards. */
  import { t, tr } from "../i18n";
  import { boardState, boardNow, cardOp, newWsId, type BoardMsg } from "../workspace";
  import Markup from "./Markup.svelte";

  let { board, chat, outgoing = false }: { board: BoardMsg; chat: string; outgoing?: boolean } = $props();
  let cards = $derived(boardNow(board, $boardState));
  let adding = $state<number | null>(null);
  let text = $state("");
  let editing = $state<string | null>(null);
  let editText = $state("");
  let drag = $state<string | null>(null);

  async function add(col: number) {
    const v = text.trim();
    if (!v) { adding = null; return; }
    text = "";
    await cardOp(chat, board.id, newWsId(), { text: v, col });
  }
  async function saveEdit(id: string) {
    const v = editText.trim();
    editing = null;
    if (v) await cardOp(chat, board.id, id, { text: v });
  }
</script>

<div class="bd" class:out={outgoing}>
  <div class="bd-head"><span class="bd-kick">🗂 {$t("Board")}</span><b>{board.title}</b><span class="bd-n">{tr("{n} cards", { n: cards.length })}</span></div>
  <div class="bd-cols">
    {#each board.cols as col, ci (ci)}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="bd-col" class:over={drag !== null}
        ondragover={(e) => e.preventDefault()}
        ondrop={(e) => { e.preventDefault(); if (drag) void cardOp(chat, board.id, drag, { col: ci }); drag = null; }}>
        <div class="bd-ch">{col} <i>{cards.filter((c) => c.c === ci).length}</i></div>
        {#each cards.filter((c) => c.c === ci) as c (c.id)}
          <div class="bd-card" draggable="true" role="listitem" ondragstart={() => (drag = c.id)} ondragend={() => (drag = null)}>
            {#if editing === c.id}
              <!-- svelte-ignore a11y_autofocus -->
              <textarea rows="2" bind:value={editText} autofocus onblur={() => saveEdit(c.id)}
                onkeydown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); void saveEdit(c.id); } if (e.key === "Escape") editing = null; }}></textarea>
            {:else}
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <div class="bd-t" ondblclick={() => { editing = c.id; editText = c.t; }} title={$t("Double-click to edit")}><Markup src={c.t} compact /></div>
              <div class="bd-acts">
                {#if ci > 0}<button onclick={() => cardOp(chat, board.id, c.id, { col: ci - 1 })} aria-label={$t("Move left")}>‹</button>{/if}
                {#if ci < board.cols.length - 1}<button onclick={() => cardOp(chat, board.id, c.id, { col: ci + 1 })} aria-label={$t("Move right")}>›</button>{/if}
                <button class="del" onclick={() => cardOp(chat, board.id, c.id, { del: true })} aria-label={$t("Delete")}>✕</button>
              </div>
            {/if}
          </div>
        {/each}
        {#if adding === ci}
          <!-- svelte-ignore a11y_autofocus -->
          <textarea class="bd-new" rows="2" bind:value={text} autofocus placeholder={$t("Card text…")} maxlength="500"
            onblur={() => add(ci)} onkeydown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); void add(ci); } if (e.key === "Escape") { adding = null; text = ""; } }}></textarea>
        {:else}
          <button class="bd-add" onclick={() => { adding = ci; text = ""; }}>＋ {$t("Card")}</button>
        {/if}
      </div>
    {/each}
  </div>
</div>

<style>
  .bd { width: min(620px, 78vw); border-radius: var(--bubble-r, 14px); border: 1px solid var(--border); background: var(--bg-bubble-in); padding: 12px; display: flex; flex-direction: column; gap: 10px; }
  .bd.out { border-color: color-mix(in srgb, var(--accent) 30%, transparent); }
  .bd-head { display: flex; align-items: baseline; gap: 8px; }
  .bd-kick { font-size: 11px; font-weight: 700; color: var(--accent); text-transform: uppercase; letter-spacing: 0.03em; }
  .bd-head b { flex: 1; font-size: 15px; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .bd-n { font-size: 11.5px; color: var(--fg-dim); }
  .bd-cols { display: grid; grid-auto-flow: column; grid-auto-columns: minmax(150px, 1fr); gap: 8px; overflow-x: auto; padding-bottom: 2px; }
  .bd-col { display: flex; flex-direction: column; gap: 6px; padding: 8px; border-radius: 10px; background: var(--bg-input); min-height: 80px; transition: box-shadow 0.12s; }
  .bd-col.over:hover { box-shadow: inset 0 0 0 2px var(--accent); }
  .bd-ch { font-size: 12px; font-weight: 800; color: var(--fg-muted); display: flex; justify-content: space-between; }
  .bd-ch i { font-style: normal; color: var(--fg-dim); font-weight: 600; }
  .bd-card { padding: 7px 8px; border-radius: 8px; background: var(--bg-shell); border: 1px solid var(--border); display: flex; flex-direction: column; gap: 4px; cursor: grab; animation: bd-in 0.16s ease-out; }
  .bd-card:hover { border-color: var(--border-strong); }
  @keyframes bd-in { from { opacity: 0; transform: translateY(4px); } }
  .bd-t { color: var(--fg); font-size: 13px; line-height: 1.4; word-wrap: break-word; max-height: 160px; overflow: hidden; }
  .bd-acts { display: flex; gap: 2px; justify-content: flex-end; opacity: 0; transition: opacity 0.12s; }
  .bd-card:hover .bd-acts { opacity: 1; }
  .bd-acts button { width: 22px; height: 22px; border-radius: 6px; border: none; background: var(--bg-hover); color: var(--fg-muted); cursor: pointer; font-size: 13px; line-height: 1; }
  .bd-acts button:hover { color: var(--accent); }
  .bd-acts .del:hover { color: #ff5c5c; }
  .bd-add { border: 1px dashed var(--border-strong); border-radius: 8px; background: none; color: var(--fg-dim); font: inherit; font-size: 12.5px; padding: 6px; cursor: pointer; }
  .bd-add:hover { color: var(--accent); border-color: var(--accent); }
  .bd-new, .bd-card textarea { width: 100%; box-sizing: border-box; resize: none; border: 1px solid var(--accent); border-radius: 8px; background: var(--bg-shell); color: var(--fg); font: inherit; font-size: 13px; padding: 6px 8px; outline: none; }
</style>
