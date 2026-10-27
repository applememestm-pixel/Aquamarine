<script module lang="ts">
  import { writable } from "svelte/store";
  /** Create an event or a checklist in a chat. `day` preselects a date. */
  export const plannerComposer = writable<{ kind: "event" | "tasks"; chat: string; day?: number } | null>(null);
  export function openPlanner(kind: "event" | "tasks", chat: string, day?: number) { plannerComposer.set({ kind, chat, day }); }
</script>

<script lang="ts">
  import Select from "./Select.svelte";
  import { t, tr } from "../i18n";
  import { api } from "../tauri";
  import { chats, activeId, loadActiveMessages } from "../stores";
  import { showToast } from "./Toast.svelte";
  import { newId, remember, REMIND_PRESETS, remindLabel, type EventMsg, type TasksMsg } from "../planner";
  import MonthGrid from "./MonthGrid.svelte";
  import { get } from "svelte/store";

  // Event
  let title = $state("");
  let date = $state(new Date());
  let hh = $state(19);
  let mm = $state(0);
  let dur = $state(60);
  let place = $state("");
  let note = $state("");
  let remind = $state<number[]>([60, 5]);
  // Checklist
  let items = $state<string[]>(["", ""]);
  let mode = $state<"all" | "k" | "pct">("all");
  let n = $state(2);
  let seq = $state(false);
  let open = $state(true);
  let busy = $state(false);

  $effect(() => {
    const p = $plannerComposer;
    if (!p) return;
    title = ""; place = ""; note = ""; items = ["", ""]; mode = "all"; n = 2; seq = false; open = true; remind = [60, 5]; dur = 60;
    const base = p.day ? new Date(p.day) : new Date(Date.now() + 86400_000);
    date = base; hh = p.day ? 19 : 19; mm = 0;
  });

  function close() { plannerComposer.set(null); }

  async function sendText(chat: string, text: string) {
    if (chat.startsWith("grp_")) await api.groupSend(chat, text);
    else await api.sendMessage(chat, text);
    if (get(activeId) === chat) await loadActiveMessages();
    chats.set(await api.listChats());
  }

  async function submit() {
    const p = $plannerComposer;
    if (!p || busy) return;
    if (!title.trim()) { showToast(tr("Give it a title")); return; }
    busy = true;
    try {
      if (p.kind === "event") {
        const at = new Date(date.getFullYear(), date.getMonth(), date.getDate(), hh, mm).getTime();
        const ev: EventMsg = { id: newId(), title: title.trim().slice(0, 120), at, dur: dur || undefined, place: place.trim() || undefined, note: note.trim() || undefined, remind };
        await sendText(p.chat, "[event:v1]" + JSON.stringify(ev));
        remember(p.chat, ev);
      } else {
        const list = items.map((x) => x.trim()).filter(Boolean);
        if (!list.length) { showToast(tr("Add at least one task")); busy = false; return; }
        const tl: TasksMsg = { id: newId(), title: title.trim().slice(0, 120), items: list.map((t, i) => ({ id: String(i + 1), t: t.slice(0, 300) })), need: { mode, n: mode === "all" ? 1 : n }, seq, open };
        await sendText(p.chat, "[tasks:v1]" + JSON.stringify(tl));
      }
      close();
    } catch (e) { showToast(String(e)); }
    finally { busy = false; }
  }

  const pad = (x: number) => String(x).padStart(2, "0");
</script>

{#if $plannerComposer}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="pc-veil" onclick={(e) => { if (e.target === e.currentTarget) close(); }}>
    <div class="pc" role="dialog" aria-modal="true" aria-label={$t($plannerComposer.kind === "event" ? "New event" : "New checklist")}>
      <header class="pc-head">
        <b>{$plannerComposer.kind === "event" ? "📅 " + $t("New event") : "☑️ " + $t("New checklist")}</b>
        <button class="pc-x" onclick={close} aria-label={$t("Close")}>✕</button>
      </header>
      <div class="pc-body">
        <input class="pc-title" placeholder={$plannerComposer.kind === "event" ? $t("What's happening?") : $t("Checklist title")} bind:value={title} maxlength="120" />
        {#if $plannerComposer.kind === "event"}
          <div class="pc-cols">
            <MonthGrid bind:selected={date} />
            <div class="pc-side">
              <div class="pc-lbl">{$t("Time")}</div>
              <div class="pc-time">
                <Select compact bind:value={hh} options={Array.from({ length: 24 }, (_, h) => ({ value: h, label: pad(h) }))} />
                <span>:</span>
                <Select compact bind:value={mm} options={Array.from({ length: 12 }, (_, i) => ({ value: i * 5, label: pad(i * 5) }))} />
              </div>
              <div class="pc-lbl">{$t("Duration")}</div>
              <Select bind:value={dur} options={([[0, "—"], [15, "15 min"], [30, "30 min"], [60, "1 h"], [90, "1.5 h"], [120, "2 h"], [180, "3 h"], [240, "4 h"], [480, "8 h"], [1440, "1 d"]] as [number, string][]).map(([v, l]) => ({ value: v, label: $t(l) }))} />
              <div class="pc-lbl">{$t("Remind everyone")}</div>
              <div class="pc-chips">
                {#each [0, ...REMIND_PRESETS] as m (m)}
                  <button class:on={remind.includes(m)} onclick={() => (remind = remind.includes(m) ? remind.filter((x) => x !== m) : [...remind, m].sort((a, b) => b - a))}>{remindLabel(m)}</button>
                {/each}
              </div>
            </div>
          </div>
          <input placeholder={$t("Place or link (optional)")} bind:value={place} maxlength="200" />
          <textarea rows="2" placeholder={$t("Details (optional)")} bind:value={note} maxlength="1000"></textarea>
          <div class="pc-note">{$t("Everyone in the chat gets reminders at these times (each person can change theirs) — a pop-up with its own sound.")}</div>
        {:else}
          <div class="pc-items">
            {#each items as _, i (i)}
              <div class="pc-item">
                <span class="pc-num">{i + 1}</span>
                <input placeholder={$t("Task")} bind:value={items[i]} maxlength="300"
                  onkeydown={(e) => { if (e.key === "Enter" && i === items.length - 1 && items[i].trim()) items = [...items, ""]; }} />
                {#if items.length > 1}<button class="pc-rm" onclick={() => (items = items.filter((_, k) => k !== i))} aria-label={$t("Delete")}>✕</button>{/if}
              </div>
            {/each}
            {#if items.length < 50}<button class="pc-addi" onclick={() => (items = [...items, ""])}>＋ {$t("Add a task")}</button>{/if}
          </div>
          <div class="pc-lbl">{$t("A task is done when")} <span class="pc-exp">{$t("experimental")}</span></div>
          <div class="pc-seg">
            <button class:on={mode === "all"} onclick={() => (mode = "all")}>{$t("everyone ticks it")}</button>
            <button class:on={mode === "k"} onclick={() => (mode = "k")}>{$t("k people")}</button>
            <button class:on={mode === "pct"} onclick={() => (mode = "pct")}>{$t("n % of the chat")}</button>
          </div>
          {#if mode !== "all"}
            <div class="pc-n">
              <input type="range" min="1" max={mode === "pct" ? 100 : 20} bind:value={n} />
              <b>{mode === "pct" ? `${n}%` : tr("{n} people", { n })}</b>
            </div>
          {/if}
          <button class="pc-toggle" onclick={() => (seq = !seq)}>
            <span><b>{$t("One after another")}</b><i>{$t("The next task opens only when the previous one is done")}</i></span>
            <span class="pc-sw" class:on={seq}></span>
          </button>
          <button class="pc-toggle" onclick={() => (open = !open)}>
            <span><b>{$t("Others can add tasks")}</b><i>{$t("Everyone in the chat may extend the list")}</i></span>
            <span class="pc-sw" class:on={open}></span>
          </button>
        {/if}
      </div>
      <footer class="pc-foot">
        <button class="pc-btn ghost" onclick={close}>{$t("Cancel")}</button>
        <button class="pc-btn" onclick={submit} disabled={busy}>{busy ? "…" : $t("Send")}</button>
      </footer>
    </div>
  </div>
{/if}

<style>
  .pc-veil { position: fixed; inset: 0; z-index: 900; display: grid; place-items: center; background: rgba(4, 6, 12, 0.5); animation: pc-fade 0.14s ease-out; }
  .pc { width: min(560px, calc(100vw - 20px)); max-height: calc(100dvh - 24px); display: flex; flex-direction: column; background: var(--bg-shell); border: 1px solid var(--border-strong); border-radius: calc(18px * var(--rk, 1)); overflow: hidden; animation: pc-pop 0.18s cubic-bezier(.2,.9,.3,1.15); }
  .pc-head { display: flex; align-items: center; padding: 14px 16px 10px; border-bottom: 1px solid var(--border); }
  .pc-head b { flex: 1; font-size: 15.5px; }
  .pc-x { width: 30px; height: 30px; border-radius: 50%; border: none; background: var(--bg-hover); color: var(--fg-muted); cursor: pointer; }
  .pc-body { padding: 14px 16px; display: flex; flex-direction: column; gap: 10px; overflow-y: auto; }
  .pc-body input:not([type=range]), .pc-body textarea, .pc-body select { padding: 9px 11px; border-radius: calc(10px * var(--rk, 1)); border: 1px solid var(--border); background: var(--bg-input); color: var(--fg); font: inherit; font-size: 14px; outline: none; resize: vertical; }
  .pc-body input:focus, .pc-body textarea:focus { border-color: var(--accent); }
  .pc-title { font-size: 16px !important; font-weight: 700; }
  .pc-cols { display: grid; grid-template-columns: 1fr 190px; gap: 14px; }
  @media (max-width: 520px) { .pc-cols { grid-template-columns: 1fr; } }
  .pc-side { display: flex; flex-direction: column; gap: 6px; }
  .pc-lbl { font-size: 11.5px; font-weight: 700; color: var(--fg-muted); text-transform: uppercase; letter-spacing: 0.04em; margin-top: 4px; }
  .pc-exp { font-size: 10px; text-transform: none; letter-spacing: 0; color: var(--accent); border: 1px solid var(--accent); border-radius: 999px; padding: 0 6px; margin-left: 4px; }
  .pc-time { display: flex; align-items: center; gap: 4px; }
  .pc-time :global(.aq-select) { flex: 1; justify-content: space-between; }
  .pc-chips { display: flex; flex-wrap: wrap; gap: 5px; }
  .pc-chips button { border: 1px solid var(--border); border-radius: 999px; background: none; color: var(--fg-muted); font: inherit; font-size: 11.5px; padding: 4px 9px; cursor: pointer; }
  .pc-chips button.on { border-color: var(--accent); color: var(--accent); background: var(--accent-soft); }
  .pc-note { font-size: 12px; color: var(--fg-dim); line-height: 1.45; }
  .pc-items { display: flex; flex-direction: column; gap: 6px; }
  .pc-item { display: flex; align-items: center; gap: 8px; }
  .pc-item input { flex: 1; min-width: 0; }
  .pc-num { width: 20px; text-align: right; font-size: 12px; color: var(--fg-dim); font-variant-numeric: tabular-nums; }
  .pc-rm { width: 28px; height: 28px; border-radius: calc(8px * var(--rk, 1)); border: none; background: none; color: var(--fg-dim); cursor: pointer; }
  .pc-rm:hover { background: var(--bg-hover); color: var(--fg); }
  .pc-addi { align-self: flex-start; border: none; background: none; color: var(--accent); font: inherit; font-weight: 600; font-size: 13px; cursor: pointer; padding: 4px 28px; }
  .pc-seg { display: flex; gap: 4px; padding: 3px; border-radius: calc(10px * var(--rk, 1)); background: var(--bg-input); }
  .pc-seg button { flex: 1; border: none; background: none; color: var(--fg-muted); font: inherit; font-size: 12.5px; font-weight: 600; padding: 7px 4px; border-radius: calc(8px * var(--rk, 1)); cursor: pointer; }
  .pc-seg button.on { background: var(--bg-shell); color: var(--accent); box-shadow: 0 0 0 1px var(--border-strong); }
  .pc-n { display: flex; align-items: center; gap: 10px; }
  .pc-n input { flex: 1; }
  .pc-n b { min-width: 74px; text-align: right; font-size: 13px; }
  .pc-toggle { display: flex; align-items: center; gap: 12px; padding: 6px 0; border: none; background: none; color: var(--fg); font: inherit; text-align: left; cursor: pointer; }
  .pc-toggle > span:first-child { flex: 1; display: flex; flex-direction: column; gap: 2px; }
  .pc-toggle b { font-size: 13.5px; }
  .pc-toggle i { font-style: normal; font-size: 12px; color: var(--fg-dim); }
  .pc-sw { width: 36px; height: 21px; border-radius: calc(11px * var(--rk, 1)); background: var(--bg-hover); border: 1px solid var(--border-strong); position: relative; flex-shrink: 0; }
  .pc-sw::after { content: ""; position: absolute; top: 2px; left: 2px; width: 15px; height: 15px; border-radius: 50%; background: var(--fg-muted); transition: transform 0.15s; }
  .pc-sw.on { background: var(--accent); border-color: transparent; }
  .pc-sw.on::after { transform: translateX(15px); background: #fff; }
  .pc-foot { display: flex; justify-content: flex-end; gap: 8px; padding: 12px 16px; border-top: 1px solid var(--border); }
  .pc-btn { border: none; border-radius: calc(10px * var(--rk, 1)); padding: 9px 18px; background: var(--accent); color: #06070C; font: inherit; font-weight: 700; cursor: pointer; }
  .pc-btn.ghost { background: var(--bg-input); color: var(--fg); }
  .pc-btn:disabled { opacity: 0.6; }
  @keyframes pc-fade { from { opacity: 0; } }
  @keyframes pc-pop { from { opacity: 0; transform: translateY(12px) scale(0.97); } }
</style>
