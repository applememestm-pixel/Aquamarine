<script module lang="ts">
  import { writable } from "svelte/store";
  import type { DocMsg } from "../workspace";
  /** The shared document being edited (null = closed). */
  export const docEditor = writable<{ chat: string; doc: DocMsg } | null>(null);
</script>

<script lang="ts">
  import { t, tr } from "../i18n";
  import { docState } from "../workspace";

  let { doc, chat, outgoing = false }: { doc: DocMsg; chat: string; outgoing?: boolean } = $props();
  let now = $derived.by(() => {
    const r = $docState[doc.id];
    return r ? { title: r.title ?? doc.title, body: r.body, rev: r.rev, by: r.by } : { title: doc.title, body: doc.body, rev: 0, by: undefined as string | undefined };
  });
  let excerpt = $derived(now.body.replace(/[#*_`>~|[\]()]/g, "").replace(/\s+/g, " ").trim().slice(0, 160));
  let words = $derived(now.body.trim() ? now.body.trim().split(/\s+/).length : 0);
</script>

<button class="dc" class:out={outgoing} onclick={() => docEditor.set({ chat, doc })}>
  <span class="dc-ic">📄</span>
  <span class="dc-main">
    <span class="dc-kick">{$t("Shared document")}{now.rev ? " · " + tr("edited by {u}", { u: now.by ?? "?" }) : ""}</span>
    <b>{now.title}</b>
    {#if excerpt}<span class="dc-ex">{excerpt}{now.body.length > 160 ? "…" : ""}</span>{/if}
    <span class="dc-foot">{tr("{n} words", { n: words })} · {$t("Everyone in the chat can edit")}</span>
  </span>
</button>

<style>
  .dc { width: min(360px, 74vw); display: flex; gap: 12px; padding: 12px; text-align: left; border-radius: var(--bubble-r, 14px); border: 1px solid var(--border); background: var(--bg-bubble-in); color: var(--fg); font: inherit; cursor: pointer; transition: border-color 0.12s, transform 0.1s; }
  .dc:hover { border-color: var(--accent); }
  .dc:active { transform: scale(0.99); }
  .dc.out { border-color: color-mix(in srgb, var(--accent) 30%, transparent); }
  .dc-ic { font-size: 26px; line-height: 1; }
  .dc-main { min-width: 0; display: flex; flex-direction: column; gap: 3px; }
  .dc-kick { font-size: 11px; font-weight: 700; color: var(--accent); text-transform: uppercase; letter-spacing: 0.03em; }
  .dc-main b { font-size: 15px; }
  .dc-ex { font-size: 12.5px; color: var(--fg-muted); line-height: 1.45; }
  .dc-foot { font-size: 11px; color: var(--fg-dim); }
</style>
