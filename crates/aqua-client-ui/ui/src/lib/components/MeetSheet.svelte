<script module lang="ts">
  import { writable } from "svelte/store";
  import { api } from "../tauri";
  import { selectChat, activeTab } from "../stores";
  import { tr } from "../i18n";
  import { showToast } from "./Toast.svelte";
  /** Open "Meet nearby" from anywhere (New message, Settings → Nearby). */
  export const meetOpen = writable(false);
  /** A scanned meet code (aqua:meet?…): start the chat and open it. Throws on a bad code. */
  export async function acceptMeet(text: string): Promise<void> {
    const user = await api.meetAccept(text);
    meetOpen.set(false);
    activeTab.set("chats");
    await selectChat(user);
    showToast(tr("Chat started. Messages go over Bluetooth or the same Wi-Fi while you're close."), 5000);
  }
</script>

<script lang="ts">
  /**
   * Meet someone without the internet: my QR carries my keys and a 15-minute
   * pass (commands.rs::meet_qr); scanning theirs starts an encrypted chat at
   * once (meet_accept). Messages go over Bluetooth / the same Wi-Fi while
   * you're close — or through a server, once both are on one.
   */
  import { onBack } from "../back";
  import { t } from "../i18n";
  import { profile } from "../profile";
  import QrCode from "./QrCode.svelte";
  import { scanQr } from "./QrScanner.svelte";

  let code = $state("");
  let err = $state("");
  let busy = $state(false);

  $effect(() => {
    if (!$meetOpen) return;
    code = ""; err = "";
    void api.meetQr($profile?.display_name ?? "").then((c) => (code = c)).catch((e) => (err = String(e)));
    // A new pass every 14 minutes while it's open (each lives 15).
    const id = setInterval(() => void api.meetQr($profile?.display_name ?? "").then((c) => (code = c)).catch(() => {}), 14 * 60_000);
    const release = onBack(() => meetOpen.set(false));
    return () => { clearInterval(id); release(); };
  });

  async function scan() {
    const text = await scanQr();
    if (!text) return;
    if (!/^(aqua:meet\?|AQUA:M2:)/i.test(text)) { err = tr("That's not a “meet nearby” code — ask them to open Meet nearby too."); return; }
    busy = true; err = "";
    try { await acceptMeet(text); } catch (e) { err = String(e); } finally { busy = false; }
  }
</script>

{#if $meetOpen}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="mt-back" onclick={(e) => { if (e.target === e.currentTarget) meetOpen.set(false); }}>
    <div class="mt" role="dialog" aria-modal="true" aria-label={$t("Meet nearby")} tabindex="-1">
      <header><h3>{$t("Meet nearby")}</h3><button data-x class="x" onclick={() => meetOpen.set(false)} aria-label={$t("Close")}>×</button></header>
      <p class="note">{$t("No internet and no pairing needed, like Briar. One of you shows the code, the other scans it: the code carries the keys and the Bluetooth address, so the devices connect directly over Bluetooth or the same Wi-Fi. Encrypted end to end, like any chat.")}</p>
      <div class="qr">
        {#if code}<QrCode text={code} size={260} plain />{:else if !err}<div class="ph">…</div>{/if}
      </div>
      <p class="hint">{$t("The code works for 15 minutes.")}</p>
      <button class="btn ok" onclick={scan} disabled={busy}>{busy ? "…" : $t("Scan their code")}</button>
      {#if err}<p class="err">{err}</p>{/if}
    </div>
  </div>
{/if}

<style>
  .mt-back { position: fixed; inset: 0; z-index: 10085; display: grid; place-items: center; background: rgba(0, 0, 0, .5); }
  :global(html.phone) .mt-back { place-items: end stretch; }
  .mt { width: min(400px, 94vw); max-height: 92vh; overflow-y: auto; display: flex; flex-direction: column; align-items: stretch; gap: 10px; padding: 18px;
    background: var(--bg-surface-2, var(--bg-list)); color: var(--fg); border: 1px solid var(--border-strong); border-radius: calc(16px * var(--rk, 1));
    animation: mt-in .2s cubic-bezier(.16, 1, .3, 1); }
  :global(html.phone) .mt { width: 100%; border-radius: calc(18px * var(--rk, 1)) calc(18px * var(--rk, 1)) 0 0; padding-bottom: calc(18px + var(--sab, env(safe-area-inset-bottom, 0px))); }
  @keyframes mt-in { from { opacity: 0; transform: translateY(16px); } }
  header { display: flex; align-items: center; }
  h3 { margin: 0; font-size: 17px; flex: 1; }
  .x { border: none; background: var(--bg-input); color: var(--fg); width: 28px; height: 28px; border-radius: 50%; cursor: pointer; font-size: 16px; }
  .note { margin: 0; font-size: 13px; color: var(--fg-muted); line-height: 1.45; }
  .qr { display: grid; place-items: center; min-height: 240px; }
  .ph { color: var(--fg-dim); }
  .hint { margin: 0; text-align: center; font-size: 12px; color: var(--fg-dim); }
  .err { margin: 0; font-size: 13px; color: #ff6b81; }
  .btn { border: 1px solid var(--border-strong); background: var(--bg-input); color: var(--fg); font: inherit; font-weight: 700; padding: 11px 14px; border-radius: calc(12px * var(--rk, 1)); cursor: pointer; }
  .btn.ok { background: var(--accent); border-color: var(--accent); color: #06231f; }
</style>
