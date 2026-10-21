<script lang="ts">
  import { t, tr, lang } from "../i18n";
  import { tryOpen, openedCapsules, myCapsules, stopCapsule, type Capsule } from "../capsules";
  import { showToast } from "./Toast.svelte";
  import { askConfirm } from "./Confirm.svelte";
  import Markup from "./Markup.svelte";

  let { c, outgoing = false }: { c: Capsule; outgoing?: boolean } = $props();
  let st = $state<string>("");
  let until = $state(0);
  let busy = $state(false);
  let text = $derived($openedCapsules[c.id]);
  let mine = $derived($myCapsules.find((x) => x.id === c.id));

  const loc = () => ($lang === "ru" ? "ru-RU" : undefined);
  const when = (ms: number) => new Date(ms).toLocaleString(loc(), { day: "numeric", month: "long", year: "numeric", hour: "2-digit", minute: "2-digit" });
  let cond = $derived(c.kind === "date" ? tr("Opens on {d}", { d: when(c.open_at ?? 0) }) : tr("Opens if {u} is away for {n} days", { u: c.from ? "@" + c.from : tr("the sender"), n: c.days ?? 0 }));

  async function open() {
    busy = true;
    try {
      const r = await tryOpen(c);
      st = r.state;
      if (r.state === "waiting") until = r.until;
      if (r.state === "sealed") showToast(tr("Still sealed"));
    } catch (e) { showToast(String(e)); }
    finally { busy = false; }
  }
  async function cancel() {
    if (!(await askConfirm({ title: tr("Cancel this sealed message?"), text: tr("Its key is deleted from the server — the message can never be opened."), ok: tr("Cancel it"), danger: true }))) return;
    try { await stopCapsule(c.id, true); showToast(tr("Cancelled")); } catch (e) { showToast(String(e)); }
  }
</script>

<div class="cp" class:out={outgoing} class:open={text !== undefined}>
  <div class="cp-head">
    <span class="cp-ic">{text !== undefined ? "📬" : "✉️"}</span>
    <div class="cp-main">
      <span class="cp-kick">{$t("Sealed message")}</span>
      {#if c.hint}<b>{c.hint}</b>{/if}
      <span class="cp-cond">{cond}{c.grace_h ? " · " + tr("the sender has {n} h to say no", { n: c.grace_h }) : ""}</span>
    </div>
  </div>
  {#if text !== undefined}
    <div class="cp-text"><Markup src={text} /></div>
  {:else if outgoing}
    {#if mine?.cancelled}
      <div class="cp-state">{$t("Cancelled")}</div>
    {:else if mine?.released}
      <div class="cp-state">{tr("Opened {d}", { d: when(mine.released) })}</div>
    {:else}
      {#if mine?.requested}<div class="cp-state warn">{tr("Asked to open on {d}", { d: when(mine.requested) })}</div>{/if}
      <button class="cp-btn ghost" onclick={cancel}>{$t("Cancel the message")}</button>
    {/if}
  {:else}
    {#if st === "waiting"}<div class="cp-state">{tr("Opens on {d} unless the sender says no", { d: when(until) })}</div>{/if}
    {#if st === "cancelled"}<div class="cp-state">{$t("The sender cancelled it")}</div>{/if}
    {#if st !== "cancelled"}<button class="cp-btn" onclick={open} disabled={busy}>{busy ? "…" : $t("Try to open")}</button>{/if}
  {/if}
</div>

<style>
  .cp { width: min(340px, 72vw); border-radius: var(--bubble-r, 14px); border: 1px solid var(--border); padding: 12px; display: flex; flex-direction: column; gap: 10px;
    background: repeating-linear-gradient(135deg, transparent 0 10px, color-mix(in srgb, var(--accent) 5%, transparent) 10px 20px), var(--bg-bubble-in); }
  .cp.out { border-color: color-mix(in srgb, var(--accent) 30%, transparent); }
  .cp.open { background: var(--bg-bubble-in); }
  .cp-head { display: flex; gap: 10px; }
  .cp-ic { font-size: 28px; line-height: 1; }
  .cp-main { min-width: 0; display: flex; flex-direction: column; gap: 2px; }
  .cp-kick { font-size: 11px; font-weight: 700; color: var(--accent); text-transform: uppercase; letter-spacing: 0.03em; }
  .cp-main b { font-size: 14.5px; }
  .cp-cond { font-size: 12px; color: var(--fg-muted); line-height: 1.4; }
  .cp-text { font-size: 14px; line-height: 1.5; padding-top: 8px; border-top: 1px dashed var(--border-strong); animation: cp-reveal 0.5s ease-out; }
  @keyframes cp-reveal { from { opacity: 0; filter: blur(6px); } }
  .cp-state { font-size: 12.5px; color: var(--fg-muted); }
  .cp-state.warn { color: #ffb84d; }
  .cp-btn { border: none; border-radius: 10px; padding: 8px 14px; background: var(--accent); color: #06070C; font: inherit; font-weight: 700; cursor: pointer; align-self: flex-start; }
  .cp-btn.ghost { background: var(--bg-input); color: var(--fg); }
  .cp-btn:disabled { opacity: 0.6; }
</style>
