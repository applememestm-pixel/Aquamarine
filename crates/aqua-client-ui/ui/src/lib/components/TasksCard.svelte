<script lang="ts">
  import { t, tr } from "../i18n";
  import { taskState, tick, addTask, needed, ME, type TasksMsg, type TaskItem } from "../planner";
  import { avatars } from "../people";

  let { tl, chat, members, outgoing = false }: { tl: TasksMsg; chat: string; members: number; outgoing?: boolean } = $props();

  let st = $derived($taskState[tl.id] ?? { ticks: {}, added: [] });
  let items = $derived<TaskItem[]>([...tl.items, ...st.added]);
  let need = $derived(needed(tl, members));
  let doneOf = (id: string) => (st.ticks[id]?.length ?? 0) >= need;
  let doneCount = $derived(items.filter((i) => doneOf(i.id)).length);
  let newText = $state("");

  /** Sequential lists: an item opens once everything before it is done. */
  function locked(i: number): boolean {
    if (!tl.seq) return false;
    for (let k = 0; k < i; k++) if (!doneOf(items[k].id)) return true;
    return false;
  }

  let rule = $derived(
    tl.need.mode === "all" ? $t("everyone must tick")
      : tl.need.mode === "k" ? tr("{n} people must tick", { n: tl.need.n })
      : tr("{n}% must tick", { n: tl.need.n }),
  );

  async function add() {
    const v = newText.trim();
    if (!v) return;
    newText = "";
    await addTask(chat, tl.id, v);
  }
</script>

<div class="tl" class:out={outgoing}>
  <div class="tl-head">
    <span class="tl-kicker">☑️ {$t("Checklist")}</span>
    <span class="tl-prog">{doneCount}/{items.length}</span>
  </div>
  <div class="tl-title">{tl.title}</div>
  <div class="tl-bar"><i style:width="{items.length ? (doneCount / items.length) * 100 : 0}%"></i></div>
  <div class="tl-rule">{rule}{tl.seq ? " · " + $t("one after another") : ""}{members > 1 ? " · " + tr("{a} of {b}", { a: need, b: members }) : ""}</div>
  <ul class="tl-items">
    {#each items as it, i (it.id)}
      {@const who = st.ticks[it.id] ?? []}
      {@const mine = who.includes(ME)}
      {@const done = doneOf(it.id)}
      {@const lock = locked(i)}
      <li class:done class:lock>
        <button class="tl-check" class:on={mine} disabled={lock} onclick={() => tick(chat, tl.id, it.id, !mine)}
          aria-label={mine ? $t("Untick") : $t("Tick")}>{#if mine}✓{/if}</button>
        <span class="tl-t">{it.t}</span>
        <span class="tl-who" title={who.map((u) => (u === ME ? $t("you") : "@" + u)).join(", ")}>
          {#each who.slice(0, 3) as u (u)}
            <span class="tl-av" style:background-image={u !== ME && $avatars[u] ? `url("${$avatars[u]}")` : undefined}>{u === ME ? "★" : $avatars[u] ? "" : u.slice(0, 1).toUpperCase()}</span>
          {/each}
          {#if need > 1}<span class="tl-n">{who.length}/{need}</span>{/if}
        </span>
      </li>
    {/each}
  </ul>
  {#if tl.open}
    <div class="tl-add">
      <input placeholder={$t("Add a task…")} bind:value={newText} maxlength="300" onkeydown={(e) => { if (e.key === "Enter") void add(); }} />
      <button onclick={add} disabled={!newText.trim()}>＋</button>
    </div>
  {/if}
</div>

<style>
  .tl { width: min(360px, 74vw); border-radius: var(--bubble-r, 14px); border: 1px solid var(--border); background: var(--bg-bubble-in); padding: 12px; display: flex; flex-direction: column; gap: 6px; }
  .tl.out { border-color: color-mix(in srgb, var(--accent) 30%, transparent); background: linear-gradient(135deg, color-mix(in srgb, var(--accent) 14%, transparent), transparent), var(--bg-bubble-in); }
  .tl-head { display: flex; justify-content: space-between; align-items: center; }
  .tl-kicker { font-size: 11px; font-weight: 700; color: var(--accent); text-transform: uppercase; letter-spacing: 0.03em; }
  .tl-prog { font-size: 12px; font-weight: 700; color: var(--fg-muted); font-variant-numeric: tabular-nums; }
  .tl-title { font-size: 15.5px; font-weight: 800; color: var(--fg); }
  .tl-bar { height: 4px; border-radius: 2px; background: var(--bg-hover); overflow: hidden; }
  .tl-bar i { display: block; height: 100%; background: linear-gradient(90deg, var(--accent), var(--accent-2)); transition: width 0.3s ease; }
  .tl-rule { font-size: 11.5px; color: var(--fg-dim); }
  .tl-items { list-style: none; margin: 4px 0 0; padding: 0; display: flex; flex-direction: column; gap: 2px; }
  .tl-items li { display: flex; align-items: center; gap: 9px; padding: 6px 4px; border-radius: calc(8px * var(--rk, 1)); transition: background 0.12s, opacity 0.2s; }
  .tl-items li:hover { background: var(--bg-hover); }
  .tl-items li.lock { opacity: 0.45; }
  .tl-items li.done .tl-t { text-decoration: line-through; color: var(--fg-dim); }
  .tl-check { width: 22px; height: 22px; border-radius: calc(7px * var(--rk, 1)); border: 2px solid var(--border-strong); background: none; color: #06070C; font-size: 13px; font-weight: 900; display: grid; place-items: center; cursor: pointer; flex-shrink: 0; transition: background 0.12s, border-color 0.12s, transform 0.1s; }
  .tl-check.on { background: var(--accent); border-color: var(--accent); }
  .tl-check:active:not(:disabled) { transform: scale(0.9); }
  .tl-check:disabled { cursor: not-allowed; }
  .tl-t { flex: 1; min-width: 0; font-size: 13.5px; color: var(--fg); word-wrap: break-word; }
  .tl-who { display: flex; align-items: center; flex-shrink: 0; }
  .tl-av { width: 18px; height: 18px; border-radius: 50%; margin-left: -5px; border: 2px solid var(--bg-bubble-in); display: grid; place-items: center; font-size: 9px; font-weight: 800; color: #06070C; background: linear-gradient(135deg, var(--accent), var(--accent-2)); background-size: cover; background-position: center; }
  .tl-n { margin-left: 5px; font-size: 11px; color: var(--fg-dim); font-variant-numeric: tabular-nums; }
  .tl-add { display: flex; gap: 6px; margin-top: 4px; }
  .tl-add input { flex: 1; min-width: 0; padding: 7px 10px; border-radius: calc(9px * var(--rk, 1)); border: 1px solid var(--border); background: var(--bg-input); color: var(--fg); font: inherit; font-size: 13px; outline: none; }
  .tl-add input:focus { border-color: var(--accent); }
  .tl-add button { width: 34px; border-radius: calc(9px * var(--rk, 1)); border: none; background: var(--accent); color: #06070C; font-size: 16px; font-weight: 800; cursor: pointer; }
  .tl-add button:disabled { opacity: 0.5; }
</style>
