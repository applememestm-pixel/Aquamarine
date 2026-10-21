<script lang="ts">
  import { t, tr, lang } from "../i18n";
  import { rsvps, answer, remember, myReminders, remindersFor, REMIND_PRESETS, remindLabel, toIcs, ME, type EventMsg } from "../planner";
  import { avatars } from "../people";
  import { api } from "../tauri";
  import { showToast } from "./Toast.svelte";
  import { save } from "@tauri-apps/plugin-dialog";

  let { ev, chat, outgoing = false }: { ev: EventMsg; chat: string; outgoing?: boolean } = $props();

  $effect(() => { remember(chat, ev); });

  let mine = $derived($rsvps[ev.id]?.[ME]);
  let who = $derived(Object.entries($rsvps[ev.id] ?? {}));
  let going = $derived(who.filter(([, s]) => s === "yes").map(([u]) => u));
  let maybe = $derived(who.filter(([, s]) => s === "maybe").length);
  let past = $derived(ev.at + (ev.dur ?? 60) * 60_000 < Date.now());
  let rem = $derived($myReminders[ev.id] ?? remindersFor(ev));
  let showRem = $state(false);

  const loc = () => ($lang === "ru" ? "ru-RU" : undefined);
  let d = $derived(new Date(ev.at));
  let month = $derived(d.toLocaleDateString(loc(), { month: "short" }).replace(".", ""));
  let when = $derived(
    d.toLocaleDateString(loc(), { weekday: "long", day: "numeric", month: "long" }) + " · " +
    d.toLocaleTimeString(loc(), { hour: "2-digit", minute: "2-digit" }) +
    (ev.dur ? "–" + new Date(ev.at + ev.dur * 60_000).toLocaleTimeString(loc(), { hour: "2-digit", minute: "2-digit" }) : ""),
  );
  let countdown = $derived.by(() => {
    const ms = ev.at - Date.now();
    if (ms <= 0) return past ? $t("Finished") : $t("Now");
    const h = Math.floor(ms / 3_600_000);
    if (h >= 48) return tr("in {n} days", { n: Math.round(h / 24) });
    if (h >= 1) return tr("in {n} h", { n: h });
    return tr("in {n} min", { n: Math.max(1, Math.round(ms / 60_000)) });
  });

  function toggleRem(min: number) {
    const cur = remindersFor(ev);
    const next = cur.includes(min) ? cur.filter((x) => x !== min) : [...cur, min].sort((a, b) => b - a);
    myReminders.update((r) => ({ ...r, [ev.id]: next }));
  }

  /** A temporary group for this event: the people going (or the chat
   *  partner), closing and archiving itself a day after the event. */
  let roomBusy = $state(false);
  async function makeRoom() {
    if (roomBusy) return;
    roomBusy = true;
    try {
      const members = chat.startsWith("grp_") ? going.filter((u) => u !== ME) : [chat];
      const g = await api.createGroup("group", ev.title.slice(0, 60), [when, ev.place ?? ""].filter(Boolean).join(" · "), members);
      g.settings.ends_at = ev.at + (ev.dur ?? 60) * 60_000 + 24 * 3_600_000;
      g.settings.end_action = "archive";
      await api.updateGroup(g);
      await api.groupSend(g.id, "[event:v1]" + JSON.stringify(ev));
      const { refreshGroups } = await import("../groups");
      await refreshGroups();
      const st = await import("../stores");
      st.chats.set(await api.listChats());
      await st.selectChat(g.id);
      showToast(tr("Event room created — it closes a day after the event"));
    } catch (e) { showToast(String(e)); }
    finally { roomBusy = false; }
  }

  async function exportIcs() {
    try {
      const path = await save({ title: "Calendar", defaultPath: `${ev.title.replace(/[^\p{L}\p{N} _-]/gu, "").slice(0, 40) || "event"}.ics`, filters: [{ name: "iCalendar", extensions: ["ics"] }] });
      if (!path) return;
      await api.writeTextFile(path, toIcs(ev));
      showToast(tr("Saved — open it to add the event to your calendar"));
    } catch (e) { showToast(String(e)); }
  }
</script>

<div class="ev" class:out={outgoing} class:past>
  <div class="ev-top">
    <div class="ev-date"><b>{d.getDate()}</b><span>{month}</span></div>
    <div class="ev-main">
      <div class="ev-kicker">📅 {$t("Event")} · <span class="ev-cd">{countdown}</span></div>
      <div class="ev-title">{ev.title}</div>
      <div class="ev-when">{when}</div>
      {#if ev.place}<div class="ev-place">📍 {ev.place}</div>{/if}
    </div>
  </div>
  {#if ev.note}<div class="ev-note">{ev.note}</div>{/if}
  <div class="ev-who">
    <span class="ev-avs">
      {#each going.slice(0, 5) as u (u)}
        <span class="ev-av" style:background-image={u !== ME && $avatars[u] ? `url("${$avatars[u]}")` : undefined}>{u === ME ? "★" : $avatars[u] ? "" : u.slice(0, 1).toUpperCase()}</span>
      {/each}
    </span>
    <span class="ev-count">{tr("{n} going", { n: going.length })}{maybe ? " · " + tr("{n} maybe", { n: maybe }) : ""}</span>
  </div>
  {#if !past}
    <div class="ev-btns">
      <button class:on={mine === "yes"} onclick={() => answer(chat, ev, "yes")}>✓ {$t("Going")}</button>
      <button class:on={mine === "maybe"} onclick={() => answer(chat, ev, "maybe")}>? {$t("Maybe")}</button>
      <button class:on={mine === "no"} onclick={() => answer(chat, ev, "no")}>✕ {$t("Can't")}</button>
    </div>
    <div class="ev-foot">
      <button class="ev-link" onclick={() => (showRem = !showRem)}>🔔 {rem.length ? rem.map((m) => remindLabel(m)).join(", ") : $t("No reminders")}</button>
      <button class="ev-link" onclick={exportIcs}>{$t("To calendar")}</button>
      {#if !chat.startsWith("grp_") || going.length > 1}<button class="ev-link" onclick={makeRoom} disabled={roomBusy}>{$t("Event room")}</button>{/if}
    </div>
    {#if showRem}
      <div class="ev-rem">
        {#each [0, ...REMIND_PRESETS] as m (m)}
          <button class:on={rem.includes(m)} onclick={() => toggleRem(m)}>{remindLabel(m)}</button>
        {/each}
      </div>
    {/if}
  {/if}
</div>

<style>
  .ev { width: min(340px, 72vw); border-radius: var(--bubble-r, 14px); border: 1px solid var(--border); background: var(--bg-bubble-in); padding: 12px; display: flex; flex-direction: column; gap: 10px; }
  .ev.out { border-color: color-mix(in srgb, var(--accent) 30%, transparent); background: linear-gradient(135deg, color-mix(in srgb, var(--accent) 14%, transparent), transparent), var(--bg-bubble-in); }
  .ev.past { opacity: 0.75; }
  .ev-top { display: flex; gap: 12px; }
  .ev-date { width: 50px; height: 54px; flex-shrink: 0; border-radius: 12px; display: flex; flex-direction: column; align-items: center; justify-content: center; background: linear-gradient(160deg, var(--accent), var(--accent-2)); color: #06070C; }
  .ev-date b { font-size: 22px; line-height: 1; }
  .ev-date span { font-size: 11px; font-weight: 700; text-transform: uppercase; }
  .ev-main { min-width: 0; display: flex; flex-direction: column; gap: 2px; }
  .ev-kicker { font-size: 11px; font-weight: 700; color: var(--accent); text-transform: uppercase; letter-spacing: 0.03em; }
  .ev-cd { color: var(--fg-muted); text-transform: none; letter-spacing: 0; }
  .ev-title { font-size: 15.5px; font-weight: 800; color: var(--fg); word-wrap: break-word; }
  .ev-when { font-size: 12.5px; color: var(--fg-muted); }
  .ev-place { font-size: 12.5px; color: var(--fg-muted); }
  .ev-note { font-size: 13px; color: var(--fg); line-height: 1.45; white-space: pre-wrap; }
  .ev-who { display: flex; align-items: center; gap: 8px; }
  .ev-avs { display: flex; }
  .ev-av { width: 22px; height: 22px; border-radius: 50%; margin-left: -6px; border: 2px solid var(--bg-bubble-in); display: grid; place-items: center; font-size: 10px; font-weight: 800; color: #06070C; background: linear-gradient(135deg, var(--accent), var(--accent-2)); background-size: cover; background-position: center; }
  .ev-av:first-child { margin-left: 0; }
  .ev-count { font-size: 12px; color: var(--fg-muted); }
  .ev-btns { display: grid; grid-template-columns: repeat(3, 1fr); gap: 6px; }
  .ev-btns button { border: 1px solid var(--border); border-radius: 9px; background: var(--bg-input); color: var(--fg); font: inherit; font-size: 12.5px; font-weight: 700; padding: 7px 4px; cursor: pointer; transition: background 0.12s, border-color 0.12s; }
  .ev-btns button:hover { border-color: var(--border-strong); }
  .ev-btns button.on { background: var(--accent); border-color: var(--accent); color: #06070C; }
  .ev-foot { display: flex; flex-wrap: wrap; justify-content: space-between; gap: 6px 10px; }
  .ev-link { border: none; background: none; padding: 0; color: var(--accent); font: inherit; font-size: 12px; cursor: pointer; text-align: left; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .ev-link:hover { text-decoration: underline; }
  .ev-rem { display: flex; flex-wrap: wrap; gap: 5px; }
  .ev-rem button { border: 1px solid var(--border); border-radius: 999px; background: none; color: var(--fg-muted); font: inherit; font-size: 11.5px; padding: 4px 9px; cursor: pointer; }
  .ev-rem button.on { border-color: var(--accent); color: var(--accent); background: var(--accent-soft); }
</style>
