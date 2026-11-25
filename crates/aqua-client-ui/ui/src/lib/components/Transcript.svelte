<script lang="ts">
  /** "To text" under a voice message, a circle or an audio file: the
   *  transcript appears below it (made on this device — lib/stt.ts). */
  import { t, tr } from "../i18n";
  import { transcribe, cachedTranscript, sttProgress } from "../stt";
  import { processing, refreshProcessing, consentToProcessing, processSpeech } from "../paid";
  import { decode16k } from "../stt";
  import { showToast } from "./Toast.svelte";

  let { src, id, compact = false }: { src: string; id: string; compact?: boolean } = $props();

  let text = $state<string | null>(cachedTranscript(id));
  let open = $state(false);
  let busy = $state(false);

  void refreshProcessing();

  async function server() {
    if (!(await consentToProcessing())) return;
    busy = true;
    try {
      const response = await fetch(src);
      if (!response.ok) throw new Error(String(response.status));
      text = await processSpeech(await decode16k(await response.arrayBuffer(), 600));
      open = true;
    } catch (e) {
      showToast(tr("Couldn't turn it into text: {e}", { e: String(e) }));
    } finally { busy = false; }
  }

  async function go() {
    if (text != null) { open = !open; return; }
    busy = true;
    try {
      const r = await transcribe(src, id);
      if (r != null) { text = r; open = true; }
    } catch (e) {
      showToast(tr("Couldn't turn it into text: {e}", { e: String((e as Error)?.message ?? e) }), 4000);
    } finally { busy = false; }
  }
</script>

<div class="tr" class:compact>
  <button type="button" class="tr-btn" class:on={open} onclick={go} disabled={busy}
    title={$t(text != null ? (open ? "Hide the text" : "Show the text") : "Turn into text")} aria-label={$t("Turn into text")}>
    {#if busy}
      <span class="spin"></span>
      {#if $sttProgress != null}<span class="pct">{Math.round($sttProgress * 100)}%</span>{/if}
    {:else}
      <svg viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M4 7V5h11v2M9.5 5v14M7 19h5M14 13v-1.5h7V13M17.5 11.5V19M16 19h3"/></svg>
    {/if}
  </button>
  {#if $processing.speech}
    <button type="button" class="tr-btn" onclick={server} disabled={busy}>{$t("Transcribe on server")}</button>
  {/if}
  {#if open && text != null}
    <div class="tr-text" class:empty={!text}>{text || $t("No speech recognized.")}</div>
  {/if}
</div>

<style>
  .tr { display: flex; flex-direction: column; align-items: flex-start; gap: 6px; }
  .tr-btn {
    display: inline-flex; align-items: center; gap: 5px; height: 26px; min-width: 26px; padding: 0 6px;
    border: none; border-radius: calc(8px * var(--rk, 1)); cursor: pointer;
    background: var(--accent-soft); color: var(--accent);
  }
  .tr-btn.on { background: var(--accent); color: var(--bg-void); }
  .tr-btn:disabled { cursor: progress; }
  .pct { font-size: 11px; font-variant-numeric: tabular-nums; }
  .spin { width: 12px; height: 12px; border-radius: 50%; border: 2px solid currentColor; border-right-color: transparent; animation: sp 0.8s linear infinite; }
  @keyframes sp { to { transform: rotate(1turn); } }
  .tr-text { font-size: 14px; line-height: 1.4; white-space: pre-wrap; word-break: break-word; user-select: text; color: var(--fg); max-width: 100%; }
  .tr-text.empty { color: var(--fg-muted); font-style: italic; }
  .compact .tr-text { font-size: 13px; }
</style>
