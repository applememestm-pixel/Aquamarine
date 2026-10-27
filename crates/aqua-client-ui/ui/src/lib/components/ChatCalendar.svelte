<script module lang="ts">
  import { writable } from "svelte/store";
  /** The calendar side panel (null = closed). */
  export const calendarFor = writable<string | null>(null);
</script>

<script lang="ts">
  import { t, tr, lang } from "../i18n";
  import { activeMessages, chats, selectChat } from "../stores";
  import { knownEvents, parseSpecial, rsvps, ME, type EventMsg } from "../planner";
  import { openPlanner } from "./PlannerComposer.svelte";
  import MonthGrid from "./MonthGrid.svelte";

  let all = $state(false);
  let day = $state(new Date());
  const key = (d: Date) => `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
  const loc = () => ($lang === "ru" ? "ru-RU" : undefined);

  /** Events of this chat (from its messages) or of every chat. */
  let list = $derived.by<(EventMsg & { chat: string; msg?: string })[]>(() => {
    const chat = $calendarFor;
    if (!chat) return [];
    const out = new Map<string, EventMsg & { chat: string; msg?: string }>();
    if (all) for (const e of Object.values($knownEvents)) out.set(e.id, e);
    else for (const e of Object.values($knownEvents)) if (e.chat === chat) out.set(e.id, e);
    for (const m of $activeMessages) {
      if (m.kind.type !== "text") continue;
      const sp = parseSpecial(m.kind.content.text);
      if (sp?.kind === "event") out.set(sp.ev.id, { ...sp.ev, chat, msg: m.id });
    }
    return [...out.values()].sort((a, b) => a.at - b.at);
  });
  let marks = $derived(new Set(list.map((e) => key(new Date(e.at)))));
  let onDay = $derived(list.filter((e) => key(new Date(e.at)) === key(day)));
  let upcoming = $derived(list.filter((e) => e.at + (e.dur ?? 60) * 60_000 > Date.now()).slice(0, 8));

  function go(e: EventMsg & { chat: string; msg?: string }) {
    if (e.chat !== $calendarFor) { void selectChat(e.chat); calendarFor.set(e.chat); return; }
    if (e.msg) window.dispatchEvent(new CustomEvent("aqua:jump", { detail: e.msg }));
  }
  const time = (at: number) => new Date(at).toLocaleTimeString(loc(), { hour: "2-digit", minute: "2-digit" });
  const date = (at: number) => new Date(at).toLocaleDateString(loc(), { day: "numeric", month: "short", weekday: "short" });
</script>

{#if $calendarFor}
  <aside class="cal" data-back aria-label={$t("Calendar")}>
    <header class="cal-head">
      <b>📅 {$t("Calendar")}</b>
      <div class="cal-seg">
        <button class:on={!all} onclick={() => (all = false)}>{$t("This chat")}</button>
        <button class:on={all} onclick={() => (all = true)}>{$t("All chats")}</button>
      </div>
      <button data-x class="cal-x" onclick={() => calendarFor.set(null)} aria-label={$t("Close")}>✕</button>
    </header>
    <div class="cal-body">
      <MonthGrid bind:selected={day} {marks} />
      <button class="cal-new" onclick={() => openPlanner("event", $calendarFor!, new Date(day.getFullYear(), day.getMonth(), day.getDate()).getTime())}>
        ＋ {tr("Event on {d}", { d: day.toLocaleDateString(loc(), { day: "numeric", month: "long" }) })}
      </button>
      {#if onDay.length}
        <div class="cal-lbl">{day.toLocaleDateString(loc(), { weekday: "long", day: "numeric", month: "long" })}</div>
        {#each onDay as e (e.id)}
          <button class="cal-ev" onclick={() => go(e)}>
            <span class="cal-t">{time(e.at)}</span>
            <span class="cal-m"><b>{e.title}</b>{#if all || e.chat !== $calendarFor}<i>{$chats.find((c) => c.id === e.chat)?.name ?? e.chat}</i>{/if}</span>
            {#if $rsvps[e.id]?.[ME] === "yes"}<span class="cal-ok">✓</span>{/if}
          </button>
        {/each}
      {/if}
      <div class="cal-lbl">{$t("Upcoming")}</div>
      {#if upcoming.length === 0}
        <div class="cal-empty">{$t("Nothing planned. Pick a day and add an event — everyone in the chat gets reminders.")}</div>
      {/if}
      {#each upcoming as e (e.id)}
        <button class="cal-ev" onclick={() => { day = new Date(e.at); go(e); }}>
          <span class="cal-t">{date(e.at)}<br />{time(e.at)}</span>
          <span class="cal-m"><b>{e.title}</b>{#if all || e.chat !== $calendarFor}<i>{$chats.find((c) => c.id === e.chat)?.name ?? e.chat}</i>{/if}{#if e.place}<i>📍 {e.place}</i>{/if}</span>
          {#if $rsvps[e.id]?.[ME] === "yes"}<span class="cal-ok">✓</span>{/if}
        </button>
      {/each}
    </div>
  </aside>
{/if}

<style>
  .cal { position: fixed; top: 40px; right: 0; bottom: 0; width: min(340px, 100vw); z-index: 60; display: flex; flex-direction: column; background: var(--bg-shell); border-left: 1px solid var(--border-strong); box-shadow: -18px 0 40px -20px rgba(0,0,0,0.6); animation: cal-in 0.2s cubic-bezier(.2,.9,.3,1); }
  @keyframes cal-in { from { transform: translateX(30px); opacity: 0; } }
  .cal-head { display: flex; align-items: center; gap: 8px; padding: 12px 12px 10px 16px; border-bottom: 1px solid var(--border); }
  .cal-head b { font-size: 15px; }
  .cal-seg { flex: 1; display: flex; gap: 2px; padding: 2px; border-radius: calc(9px * var(--rk, 1)); background: var(--bg-input); margin-left: 6px; }
  .cal-seg button { flex: 1; border: none; background: none; color: var(--fg-muted); font: inherit; font-size: 11.5px; font-weight: 600; padding: 5px 4px; border-radius: calc(7px * var(--rk, 1)); cursor: pointer; }
  .cal-seg button.on { background: var(--bg-shell); color: var(--accent); }
  .cal-x { width: 28px; height: 28px; border-radius: 50%; border: none; background: var(--bg-hover); color: var(--fg-muted); cursor: pointer; }
  .cal-body { flex: 1; overflow-y: auto; padding: 12px 14px 20px; display: flex; flex-direction: column; gap: 8px; }
  .cal-new { border: 1px dashed var(--accent); border-radius: calc(10px * var(--rk, 1)); background: var(--accent-soft); color: var(--accent); font: inherit; font-weight: 700; font-size: 13px; padding: 9px; cursor: pointer; }
  .cal-lbl { margin-top: 8px; font-size: 11.5px; font-weight: 800; color: var(--fg-muted); text-transform: uppercase; letter-spacing: 0.04em; }
  .cal-empty { font-size: 12.5px; color: var(--fg-dim); line-height: 1.5; }
  .cal-ev { display: flex; align-items: center; gap: 10px; padding: 9px 10px; border-radius: calc(11px * var(--rk, 1)); border: 1px solid var(--border); background: var(--bg-input); color: var(--fg); font: inherit; text-align: left; cursor: pointer; }
  .cal-ev:hover { border-color: var(--accent); }
  .cal-t { width: 62px; flex-shrink: 0; font-size: 11.5px; font-weight: 700; color: var(--accent); line-height: 1.35; }
  .cal-m { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 1px; }
  .cal-m b { font-size: 13.5px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .cal-m i { font-style: normal; font-size: 11.5px; color: var(--fg-dim); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .cal-ok { color: var(--accent); font-weight: 900; }
</style>
