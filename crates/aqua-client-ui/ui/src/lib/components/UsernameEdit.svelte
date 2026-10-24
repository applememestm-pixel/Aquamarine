<script lang="ts">
  /** My @username with a "Change" button (Settings → Account, profile editor). */
  import { info } from "../stores";
  import { t, tr } from "../i18n";
  import { changeMyUsername, USERNAME_RE } from "../rename";
  import { showToast } from "./Toast.svelte";

  let editing = $state(false);
  let value = $state("");
  let busy = $state(false);
  let err = $state<string | null>(null);
  const clean = $derived(value.trim().replace(/^@+/, "").toLowerCase());
  const valid = $derived(USERNAME_RE.test(clean));

  function start() { value = $info?.username ?? ""; err = null; editing = true; }
  async function save() {
    if (!valid || busy) return;
    if (clean === $info?.username) { editing = false; return; }
    busy = true; err = null;
    try {
      const got = await changeMyUsername(clean);
      editing = false;
      showToast(tr("Your username is now @{u}", { u: got }));
    } catch (e) {
      const m = String(e);
      err = /taken/i.test(m) ? tr("This username is taken.")
        : /once a day/i.test(m) ? tr("The username can be changed once a day.")
        : /not connected/i.test(m) ? tr("Connect to your server first.")
        : tr("Could not change the username: {e}", { e: m });
    } finally { busy = false; }
  }
</script>

{#if !editing}
  <div class="ue-row">
    <span class="ue-at">@{$info?.username ?? "—"}</span>
    <button class="ue-btn" onclick={start} disabled={!$info?.connected}>{$t("Change")}</button>
  </div>
{:else}
  <form class="ue-form" onsubmit={(e) => { e.preventDefault(); void save(); }}>
    <div class="ue-in"><i>@</i><input bind:value placeholder="username" spellcheck="false" autocomplete="off" autocapitalize="off"
      oninput={() => (value = value.toLowerCase())} /></div>
    <div class="ue-acts">
      <button type="button" class="ue-btn ghost" onclick={() => (editing = false)} disabled={busy}>{$t("Cancel")}</button>
      <button type="submit" class="ue-btn primary" disabled={!valid || busy}>{busy ? "…" : $t("Save")}</button>
    </div>
  </form>
  <div class="ue-note" class:bad={!!value && !valid}>
    {#if err}{err}{:else}{$t("3–32 characters: a–z, 0–9, _ and dot. People find you as @{u}; the old name leads to you for 30 days, and your contacts see the new one by themselves.", { u: clean || "username" })}{/if}
  </div>
{/if}

<style>
  .ue-row { display: flex; align-items: center; gap: 10px; }
  .ue-at { flex: 1; min-width: 0; font-weight: 600; overflow: hidden; text-overflow: ellipsis; }
  .ue-btn { border: 1px solid var(--border); background: transparent; color: var(--accent); border-radius: 9px; padding: 6px 12px; font: 600 13px var(--font-ui); cursor: pointer; }
  .ue-btn:disabled { opacity: .5; cursor: default; }
  .ue-btn.primary { background: var(--accent); color: #06070c; border-color: transparent; }
  .ue-btn.ghost { color: var(--fg-muted); }
  .ue-form { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; }
  .ue-in { flex: 1 1 180px; display: flex; align-items: center; gap: 2px; background: var(--bg-input); border: 1px solid var(--border); border-radius: 10px; padding: 0 10px; }
  .ue-in:focus-within { border-color: var(--accent); }
  .ue-in i { font-style: normal; color: var(--fg-dim); font-weight: 700; }
  .ue-in input { flex: 1; min-width: 0; border: none; background: transparent; color: var(--fg); font: inherit; padding: 8px 0; outline: none; }
  .ue-acts { display: flex; gap: 6px; }
  .ue-note { margin-top: 6px; font-size: 12px; color: var(--fg-dim); line-height: 1.45; }
  .ue-note.bad { color: var(--danger, #FF4D6D); }
</style>
