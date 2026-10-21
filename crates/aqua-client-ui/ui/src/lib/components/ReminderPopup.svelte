<script lang="ts">
  /** Calendar reminders: cards in the corner, with the chat one click away. */
  import { t, tr, lang } from "../i18n";
  import { reminders, answer, type Reminder } from "../planner";
  import { ttlLabel } from "../ttl";
  import { selectChat, chats } from "../stores";

  function dismiss(r: Reminder) { reminders.update((l) => l.filter((x) => x.key !== r.key)); }
  async function open(r: Reminder) { dismiss(r); await selectChat(r.chat); }
  const when = (at: number) => new Date(at).toLocaleTimeString($lang === "ru" ? "ru-RU" : undefined, { hour: "2-digit", minute: "2-digit" });
</script>

{#if $reminders.length}
  <div class="rp-stack" aria-live="assertive">
    {#each $reminders as r (r.key)}
      <div class="rp" role="alert">
        <div class="rp-ic">🔔</div>
        <div class="rp-main">
          <div class="rp-kick">{r.ev.at - Date.now() > 45_000 ? tr("In {t}", { t: ttlLabel(Math.round((r.ev.at - Date.now()) / 60_000) * 60) }) : $t("Starting now")} · {when(r.ev.at)}</div>
          <div class="rp-title">{r.ev.title}</div>
          <div class="rp-sub">{$chats.find((c) => c.id === r.chat)?.name ?? r.chat}{r.ev.place ? " · 📍 " + r.ev.place : ""}</div>
          <div class="rp-btns">
            <button class="rp-btn" onclick={() => open(r)}>{$t("Open chat")}</button>
            <button class="rp-btn ghost" onclick={() => { void answer(r.chat, r.ev, "no"); dismiss(r); }}>{$t("Can't go")}</button>
            <button class="rp-btn ghost" onclick={() => dismiss(r)}>{$t("OK")}</button>
          </div>
        </div>
      </div>
    {/each}
  </div>
{/if}

<style>
  .rp-stack { position: fixed; top: 46px; right: 16px; z-index: 980; display: flex; flex-direction: column; gap: 10px; width: min(340px, calc(100vw - 32px)); }
  .rp { display: flex; gap: 12px; padding: 14px; border-radius: 16px; background: var(--bg-shell); border: 1px solid var(--accent); box-shadow: 0 18px 44px -14px rgba(0,0,0,0.6), 0 0 0 4px var(--accent-soft); animation: rp-in 0.32s cubic-bezier(.2,.9,.3,1.25); }
  .rp-ic { font-size: 22px; animation: rp-ring 1.2s ease-in-out 2; transform-origin: top center; }
  .rp-main { min-width: 0; flex: 1; display: flex; flex-direction: column; gap: 3px; }
  .rp-kick { font-size: 11.5px; font-weight: 700; color: var(--accent); text-transform: uppercase; letter-spacing: 0.03em; }
  .rp-title { font-size: 15px; font-weight: 800; color: var(--fg); }
  .rp-sub { font-size: 12.5px; color: var(--fg-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .rp-btns { display: flex; gap: 6px; margin-top: 8px; flex-wrap: wrap; }
  .rp-btn { border: none; border-radius: 9px; padding: 7px 12px; background: var(--accent); color: #06070C; font: inherit; font-size: 12.5px; font-weight: 700; cursor: pointer; }
  .rp-btn.ghost { background: var(--bg-input); color: var(--fg); }
  @keyframes rp-in { from { opacity: 0; transform: translateX(30px) scale(0.96); } }
  @keyframes rp-ring { 0%, 100% { transform: rotate(0); } 20% { transform: rotate(16deg); } 40% { transform: rotate(-14deg); } 60% { transform: rotate(10deg); } 80% { transform: rotate(-6deg); } }
</style>
