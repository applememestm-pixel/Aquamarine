<script module lang="ts">
  import { writable } from "svelte/store";
  export const capsuleComposer = writable<string | null>(null);
</script>

<script lang="ts">
  /** Write a sealed message to the person in this chat. */
  import { t, tr } from "../i18n";
  import { api } from "../tauri";
  import { loadActiveMessages, chats } from "../stores";
  import { seal, refreshMyCapsules } from "../capsules";
  import { showToast } from "./Toast.svelte";
  import MonthGrid from "./MonthGrid.svelte";

  let text = $state("");
  let hint = $state("");
  let kind = $state<"date" | "inactive">("date");
  let date = $state(new Date(Date.now() + 365 * 86_400_000));
  let hh = $state(12);
  let days = $state(30);
  let grace = $state(48);
  let busy = $state(false);

  $effect(() => { if ($capsuleComposer) { text = ""; hint = ""; kind = "date"; date = new Date(Date.now() + 365 * 86_400_000); days = 30; grace = 48; } });
  const close = () => capsuleComposer.set(null);

  async function send() {
    const peer = $capsuleComposer;
    if (!peer || busy) return;
    if (!text.trim()) { showToast(tr("Write the message first")); return; }
    const at = new Date(date.getFullYear(), date.getMonth(), date.getDate(), hh, 0).getTime();
    if (kind === "date" && at <= Date.now()) { showToast(tr("Pick a date in the future")); return; }
    busy = true;
    try {
      const msg = await seal(peer, text.slice(0, 20000), hint.trim(), kind === "date" ? { kind: "date", open_at: at } : { kind: "inactive", days }, grace);
      await api.sendMessage(peer, msg);
      await loadActiveMessages();
      chats.set(await api.listChats());
      void refreshMyCapsules();
      close();
      showToast(tr("Sealed and sent"));
    } catch (e) { showToast(String(e)); }
    finally { busy = false; }
  }
</script>

{#if $capsuleComposer}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="cc-veil" onclick={(e) => { if (e.target === e.currentTarget) close(); }}>
    <div class="cc" role="dialog" aria-modal="true" aria-label={$t("Sealed message")}>
      <header class="cc-head"><b>✉️ {$t("Sealed message")}</b><button class="cc-x" onclick={close} aria-label={$t("Close")}>✕</button></header>
      <div class="cc-body">
        <p class="cc-note">{$t("It reaches them now but stays sealed until the condition is met. The text is encrypted here; the server keeps only the key and gives it to them when the time comes — neither can read it alone before.")}</p>
        <textarea rows="5" placeholder={$t("Your message…")} bind:value={text} maxlength="20000"></textarea>
        <input placeholder={$t("What's on the envelope (optional)")} bind:value={hint} maxlength="200" />
        <div class="cc-seg">
          <button class:on={kind === "date"} onclick={() => (kind = "date")}>{$t("On a date")}</button>
          <button class:on={kind === "inactive"} onclick={() => (kind = "inactive")}>{$t("If I'm away")}</button>
        </div>
        {#if kind === "date"}
          <div class="cc-date">
            <MonthGrid bind:selected={date} />
            <label class="cc-time">{$t("Time")} <select bind:value={hh}>{#each Array(24) as _, h (h)}<option value={h}>{String(h).padStart(2, "0")}:00</option>{/each}</select></label>
          </div>
        {:else}
          <div class="cc-seg">
            {#each [7, 30, 90, 180, 365] as d (d)}<button class:on={days === d} onclick={() => (days = d)}>{tr("{n} days", { n: d })}</button>{/each}
          </div>
          <p class="cc-note">{$t("Opens if none of your devices connected to the server for that long.")}</p>
        {/if}
        <div class="cc-lbl">{$t("Before it opens, give me time to say no")}</div>
        <div class="cc-seg">
          {#each [[0, "No"], [24, "24 h"], [48, "48 h"], [168, "7 days"]] as [g, l] (g)}<button class:on={grace === g} onclick={() => (grace = Number(g))}>{$t(String(l))}</button>{/each}
        </div>
      </div>
      <footer class="cc-foot">
        <button class="cc-btn ghost" onclick={close}>{$t("Cancel")}</button>
        <button class="cc-btn" onclick={send} disabled={busy}>{busy ? "…" : $t("Seal and send")}</button>
      </footer>
    </div>
  </div>
{/if}

<style>
  .cc-veil { position: fixed; inset: 0; z-index: 900; display: grid; place-items: center; background: rgba(4, 6, 12, 0.5); }
  .cc { width: min(520px, calc(100vw - 20px)); max-height: calc(100dvh - 24px); display: flex; flex-direction: column; background: var(--bg-shell); border: 1px solid var(--border-strong); border-radius: 18px; overflow: hidden; animation: cc-pop 0.18s cubic-bezier(.2,.9,.3,1.12); }
  @keyframes cc-pop { from { opacity: 0; transform: translateY(12px) scale(0.97); } }
  .cc-head { display: flex; align-items: center; padding: 14px 16px 10px; border-bottom: 1px solid var(--border); }
  .cc-head b { flex: 1; font-size: 15.5px; }
  .cc-x { width: 30px; height: 30px; border-radius: 50%; border: none; background: var(--bg-hover); color: var(--fg-muted); cursor: pointer; }
  .cc-body { padding: 14px 16px; display: flex; flex-direction: column; gap: 10px; overflow-y: auto; }
  .cc-note { margin: 0; font-size: 12.5px; color: var(--fg-dim); line-height: 1.5; }
  .cc-body textarea, .cc-body input, .cc-body select { padding: 9px 11px; border-radius: 10px; border: 1px solid var(--border); background: var(--bg-input); color: var(--fg); font: inherit; font-size: 14px; outline: none; resize: vertical; }
  .cc-body textarea:focus, .cc-body input:focus { border-color: var(--accent); }
  .cc-seg { display: flex; gap: 4px; padding: 3px; border-radius: 10px; background: var(--bg-input); flex-wrap: wrap; }
  .cc-seg button { flex: 1; border: none; background: none; color: var(--fg-muted); font: inherit; font-size: 12.5px; font-weight: 600; padding: 7px 6px; border-radius: 8px; cursor: pointer; white-space: nowrap; }
  .cc-seg button.on { background: var(--bg-shell); color: var(--accent); box-shadow: 0 0 0 1px var(--border-strong); }
  .cc-date { display: flex; flex-direction: column; gap: 8px; }
  .cc-time { display: flex; align-items: center; gap: 8px; font-size: 13px; color: var(--fg-muted); }
  .cc-lbl { font-size: 11.5px; font-weight: 700; color: var(--fg-muted); text-transform: uppercase; letter-spacing: 0.04em; margin-top: 4px; }
  .cc-foot { display: flex; justify-content: flex-end; gap: 8px; padding: 12px 16px; border-top: 1px solid var(--border); }
  .cc-btn { border: none; border-radius: 10px; padding: 9px 18px; background: var(--accent); color: #06070C; font: inherit; font-weight: 700; cursor: pointer; }
  .cc-btn.ghost { background: var(--bg-input); color: var(--fg); }
  .cc-btn:disabled { opacity: 0.6; }
</style>
