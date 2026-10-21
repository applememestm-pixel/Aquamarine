<script lang="ts">
  /** Someone asked to open a sealed message of mine: say no while I can. */
  import { t, tr, lang } from "../i18n";
  import { myCapsules, stopCapsule } from "../capsules";
  import { showToast } from "./Toast.svelte";

  let dismissed = $state<Set<string>>(new Set());
  let pending = $derived($myCapsules.filter((c) => c.requested && !c.released && !c.cancelled && !dismissed.has(c.id + c.requested)));
  const until = (c: { requested: number; grace_h: number }) => new Date(c.requested + c.grace_h * 3_600_000).toLocaleString($lang === "ru" ? "ru-RU" : undefined, { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" });
  async function deny(id: string) {
    try { await stopCapsule(id, false); showToast(tr("Denied — it stays sealed")); } catch (e) { showToast(String(e)); }
  }
</script>

{#if pending.length}
  <div class="ca">
    {#each pending as c (c.id)}
      <div class="ca-card" role="alert">
        <b>✉️ {tr("@{u} asked to open your sealed message", { u: c.recipient })}</b>
        <span>{tr("It opens on {d} unless you say no.", { d: until(c) })}</span>
        <div class="ca-btns">
          <button class="ca-btn" onclick={() => deny(c.id)}>{$t("Say no")}</button>
          <button class="ca-btn ghost" onclick={() => { dismissed = new Set([...dismissed, c.id + c.requested]); }}>{$t("Let it open")}</button>
        </div>
      </div>
    {/each}
  </div>
{/if}

<style>
  .ca { position: fixed; left: 16px; bottom: 16px; z-index: 975; display: flex; flex-direction: column; gap: 8px; width: min(360px, calc(100vw - 32px)); }
  .ca-card { padding: 12px 14px; border-radius: 14px; background: var(--bg-shell); border: 1px solid #ffb84d; display: flex; flex-direction: column; gap: 4px; box-shadow: 0 16px 40px -16px rgba(0,0,0,0.6); }
  .ca-card b { font-size: 13.5px; }
  .ca-card span { font-size: 12.5px; color: var(--fg-muted); }
  .ca-btns { display: flex; gap: 6px; margin-top: 6px; }
  .ca-btn { border: none; border-radius: 9px; padding: 7px 12px; background: #ffb84d; color: #1a1205; font: inherit; font-weight: 700; cursor: pointer; }
  .ca-btn.ghost { background: var(--bg-input); color: var(--fg); }
</style>
