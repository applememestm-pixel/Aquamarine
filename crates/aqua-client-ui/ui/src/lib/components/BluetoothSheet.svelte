<script lang="ts">
  /** "Send via Bluetooth": choose a nearby device, send files, watch progress. */
  import { t, tr } from "../i18n";
  import { btSheet, btStatus, btTransfers, btGetStatus, btDevices, btPower, btSend, clearDone, type BtDevice } from "../bluetooth";
  import { openFileDialog } from "../dialog";
  import { showToast } from "./Toast.svelte";

  let files = $state<string[]>([]);
  let devices = $state<BtDevice[]>([]);
  let scanning = $state(false);
  let loading = $state(false);
  let sendId = $state<string | null>(null);

  $effect(() => {
    const f = $btSheet;
    if (f === null) return;
    files = [...f];
    sendId = null;
    void refresh();
  });

  async function refresh(scan = 0) {
    loading = true;
    try {
      const s = await btGetStatus();
      if (s.supported && s.adapter && s.powered) devices = await btDevices(scan);
    } catch (e) { showToast(String(e)); }
    finally { loading = false; }
  }
  async function scan() {
    scanning = true;
    await refresh(8);
    scanning = false;
  }
  async function powerOn() {
    try { await btPower(true); await new Promise((r) => setTimeout(r, 900)); await refresh(); } catch (e) { showToast(String(e)); }
  }
  async function pick() {
    const r = await openFileDialog({ multiple: true, title: tr("Files to send") });
    if (!r) return;
    const list = (Array.isArray(r) ? r : [r]).map(String);
    files = [...new Set([...files, ...list])].slice(0, 50);
  }
  async function send(d: BtDevice) {
    if (!files.length) { await pick(); if (!files.length) return; }
    try { sendId = await btSend(d.address, files); } catch (e) { showToast(String(e)); }
  }
  const base = (p: string) => p.split(/[\\/]/).pop() ?? p;
  const mb = (n: number) => (n >= 1048576 ? (n / 1048576).toFixed(1) + " MB" : Math.max(1, Math.round(n / 1024)) + " KB");
  const devIcon = (i: string) => (i.startsWith("phone") ? "📱" : i.startsWith("computer") ? "💻" : i.includes("audio") ? "🎧" : "📶");
  let mine = $derived(sendId ? Object.entries($btTransfers).filter(([k]) => k.startsWith(sendId + "-")).map(([, v]) => v) : []);
  let failed = $derived(sendId ? $btTransfers[sendId] : undefined);
  const close = () => { btSheet.set(null); clearDone(); };
</script>

{#if $btSheet !== null}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="bt-veil" onclick={(e) => { if (e.target === e.currentTarget) close(); }}>
    <div class="bt" role="dialog" aria-modal="true" aria-label={$t("Send via Bluetooth")}>
      <header class="bt-head">
        <span class="bt-logo"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="7 7 17 17 12 22 12 2 17 7 7 17"/></svg></span>
        <b>{$t("Send via Bluetooth")}</b>
        <button class="bt-x" onclick={close} aria-label={$t("Close")}>✕</button>
      </header>
      <div class="bt-body">
        <div class="bt-files">
          {#each files as f (f)}
            <span class="bt-file">{base(f)}<button onclick={() => (files = files.filter((x) => x !== f))} aria-label={$t("Remove")}>×</button></span>
          {/each}
          <button class="bt-add" onclick={pick}>＋ {files.length ? $t("More files") : $t("Choose files")}</button>
        </div>

        {#if sendId}
          <div class="bt-lbl">{$t("Sending")}</div>
          {#each mine as tr_ (tr_.id)}
            <div class="bt-tr" class:done={tr_.status === "complete"} class:err={tr_.status === "error"}>
              <span class="bt-tn">{tr_.name}</span>
              <span class="bt-ts">{tr_.status === "complete" ? "✓" : tr_.status === "error" ? "✕" : tr_.size ? Math.round((tr_.sent / tr_.size) * 100) + "%" : "…"}</span>
              <span class="bt-bar"><i style:width="{tr_.size ? Math.min(100, (tr_.sent / tr_.size) * 100) : tr_.status === 'complete' ? 100 : 4}%"></i></span>
            </div>
          {/each}
          {#if failed?.status === "failed"}<div class="bt-warn">{failed.error}</div>{/if}
          {#if !mine.length && !failed}<div class="bt-note">{$t("Waiting for the other device to accept…")}</div>{/if}
          <button class="bt-ghost" onclick={() => (sendId = null)}>{$t("Send to another device")}</button>
        {:else if !$btStatus}
          <div class="bt-note">{$t("Looking for Bluetooth…")}</div>
        {:else if !$btStatus.supported}
          <div class="bt-warn">{$t("Bluetooth transfers work in the Linux app for now.")}</div>
        {:else if !$btStatus.adapter}
          <div class="bt-warn">{$btStatus.error ?? $t("No Bluetooth adapter found.")}</div>
        {:else if !$btStatus.powered}
          <div class="bt-note">{$t("Bluetooth is off.")}</div>
          <button class="bt-btn" onclick={powerOn}>{$t("Turn Bluetooth on")}</button>
        {:else}
          {#if !$btStatus.obex}<div class="bt-warn">{$t("File transfer service (obexd) isn't running. On Arch: sudo pacman -S bluez-obex, then restart Bluetooth.")}</div>{/if}
          <div class="bt-lbl">{$t("Devices")} <button class="bt-scan" onclick={scan} disabled={scanning}>{scanning ? $t("Searching…") : $t("Search")}</button></div>
          {#if loading && !devices.length}<div class="bt-note">…</div>{/if}
          {#if !loading && !devices.length}<div class="bt-note">{$t("No devices yet. Make your phone visible and press Search.")}</div>{/if}
          <div class="bt-devs">
            {#each devices as d (d.address)}
              <button class="bt-dev" onclick={() => send(d)} disabled={!$btStatus.obex}>
                <span class="bt-di">{devIcon(d.icon)}</span>
                <span class="bt-dn"><b>{d.name}</b><small>{d.paired ? $t("Paired") : $t("Not paired")}{d.connected ? " · " + $t("connected") : ""}</small></span>
                <span class="bt-go">{$t("Send")}</span>
              </button>
            {/each}
          </div>
        {/if}
        {#if $btStatus?.receiving}<div class="bt-foot">{$t("Files sent to this computer appear here to accept and are saved to Downloads / Aquamarine Bluetooth.")}</div>{/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .bt-veil { position: fixed; inset: 0; z-index: 905; display: grid; place-items: center; background: rgba(4, 6, 12, 0.5); }
  .bt { width: min(460px, calc(100vw - 20px)); max-height: calc(100dvh - 24px); display: flex; flex-direction: column; background: var(--bg-shell); border: 1px solid var(--border-strong); border-radius: 18px; overflow: hidden; animation: bt-pop 0.18s cubic-bezier(.2,.9,.3,1.12); }
  @keyframes bt-pop { from { opacity: 0; transform: translateY(12px) scale(0.97); } }
  .bt-head { display: flex; align-items: center; gap: 10px; padding: 14px 16px 12px; border-bottom: 1px solid var(--border); }
  .bt-head b { flex: 1; font-size: 15.5px; }
  .bt-logo { width: 30px; height: 30px; border-radius: 9px; display: grid; place-items: center; background: #1f6fff; color: #fff; }
  .bt-logo svg { width: 17px; height: 17px; }
  .bt-x { width: 30px; height: 30px; border-radius: 50%; border: none; background: var(--bg-hover); color: var(--fg-muted); cursor: pointer; }
  .bt-body { padding: 14px 16px 16px; display: flex; flex-direction: column; gap: 10px; overflow-y: auto; }
  .bt-files { display: flex; flex-wrap: wrap; gap: 6px; }
  .bt-file { display: inline-flex; align-items: center; gap: 4px; max-width: 100%; padding: 5px 6px 5px 10px; border-radius: 9px; background: var(--bg-input); font-size: 12.5px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .bt-file button { border: none; background: none; color: var(--fg-dim); cursor: pointer; font-size: 15px; }
  .bt-add { border: 1px dashed var(--border-strong); border-radius: 9px; background: none; color: var(--fg-muted); font: inherit; font-size: 12.5px; font-weight: 600; padding: 5px 10px; cursor: pointer; }
  .bt-add:hover { color: var(--accent); border-color: var(--accent); }
  .bt-lbl { display: flex; align-items: center; justify-content: space-between; font-size: 11.5px; font-weight: 800; color: var(--fg-muted); text-transform: uppercase; letter-spacing: 0.04em; margin-top: 4px; }
  .bt-scan { border: none; background: none; color: var(--accent); font: inherit; font-size: 12px; font-weight: 700; text-transform: none; letter-spacing: 0; cursor: pointer; }
  .bt-devs { display: flex; flex-direction: column; gap: 4px; }
  .bt-dev { display: flex; align-items: center; gap: 10px; padding: 9px 10px; border-radius: 11px; border: 1px solid var(--border); background: var(--bg-input); color: var(--fg); font: inherit; text-align: left; cursor: pointer; }
  .bt-dev:hover:not(:disabled) { border-color: var(--accent); }
  .bt-dev:disabled { opacity: 0.55; cursor: default; }
  .bt-di { font-size: 20px; }
  .bt-dn { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .bt-dn b { font-size: 13.5px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .bt-dn small { font-size: 11.5px; color: var(--fg-dim); }
  .bt-go { font-size: 12px; font-weight: 800; color: var(--accent); }
  .bt-note { font-size: 12.5px; color: var(--fg-dim); line-height: 1.5; }
  .bt-warn { font-size: 12.5px; color: #ffb84d; line-height: 1.5; }
  .bt-foot { font-size: 11.5px; color: var(--fg-dim); line-height: 1.45; border-top: 1px solid var(--border); padding-top: 10px; }
  .bt-btn { border: none; border-radius: 10px; padding: 9px 16px; background: var(--accent); color: #06070C; font: inherit; font-weight: 700; cursor: pointer; align-self: flex-start; }
  .bt-ghost { border: 1px solid var(--border); border-radius: 10px; padding: 8px 12px; background: none; color: var(--fg-muted); font: inherit; font-size: 12.5px; font-weight: 600; cursor: pointer; align-self: flex-start; }
  .bt-tr { display: grid; grid-template-columns: 1fr auto; gap: 4px 8px; font-size: 12.5px; }
  .bt-tn { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .bt-ts { color: var(--fg-muted); font-variant-numeric: tabular-nums; }
  .bt-tr.done .bt-ts { color: var(--online, #3ddc84); }
  .bt-tr.err .bt-ts { color: #ff6b6b; }
  .bt-bar { grid-column: 1 / -1; height: 4px; border-radius: 2px; background: var(--bg-hover); overflow: hidden; }
  .bt-bar i { display: block; height: 100%; background: linear-gradient(90deg, var(--accent), var(--accent-2)); transition: width 0.25s; }
</style>
