<script lang="ts">
  /** Edit a chat's shared document; saving sends the new revision to everyone. */
  import { t, tr } from "../i18n";
  import { docEditor } from "./DocCard.svelte";
  import { docState, docNow, saveDoc } from "../workspace";
  import { api } from "../tauri";
  import { showToast } from "./Toast.svelte";
  import { save } from "@tauri-apps/plugin-dialog";
  import Markup from "./Markup.svelte";
  import { askConfirm } from "./Confirm.svelte";

  let title = $state("");
  let body = $state("");
  let baseRev = $state(0);
  let preview = $state(false);
  let busy = $state(false);

  $effect(() => {
    const e = $docEditor;
    if (!e) return;
    const n = docNow(e.doc);
    title = n.title; body = n.body; baseRev = n.rev; preview = false;
  });
  /** Someone saved a newer version while this one is open. */
  let newer = $derived.by(() => {
    const e = $docEditor;
    if (!e) return null;
    const r = $docState[e.doc.id];
    return r && r.rev > baseRev ? r : null;
  });
  let dirty = $derived.by(() => { const e = $docEditor; if (!e) return false; const n = docNow(e.doc); return n.title !== title || n.body !== body; });

  function reload() {
    const e = $docEditor;
    if (!e) return;
    const n = docNow(e.doc);
    title = n.title; body = n.body; baseRev = n.rev;
  }
  async function submit() {
    const e = $docEditor;
    if (!e || busy) return;
    busy = true;
    try { await saveDoc(e.chat, e.doc, title.trim() || e.doc.title, body); baseRev = docNow(e.doc).rev; showToast(tr("Saved for everyone in the chat")); }
    catch (err) { showToast(String(err)); }
    finally { busy = false; }
  }
  async function exportMd() {
    try {
      const path = await save({ title: "Markdown", defaultPath: `${title.replace(/[^\p{L}\p{N} _-]/gu, "").slice(0, 40) || "doc"}.md`, filters: [{ name: "Markdown", extensions: ["md"] }] });
      if (path) { await api.writeTextFile(path, `# ${title}\n\n${body}`); showToast(tr("Saved")); }
    } catch (err) { showToast(String(err)); }
  }
  async function close() {
    if (dirty && !(await askConfirm({ title: tr("Close without saving?"), text: tr("Your changes to this document will be lost."), ok: tr("Close"), danger: true }))) return;
    docEditor.set(null);
  }
</script>

{#if $docEditor}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="de-veil" onclick={(e) => { if (e.target === e.currentTarget) close(); }}>
    <div class="de" role="dialog" aria-modal="true" aria-label={$t("Shared document")}>
      <header class="de-head">
        <input class="de-title" bind:value={title} maxlength="120" placeholder={$t("Title")} />
        <div class="de-seg">
          <button class:on={!preview} onclick={() => (preview = false)}>{$t("Edit")}</button>
          <button class:on={preview} onclick={() => (preview = true)}>{$t("Preview")}</button>
        </div>
        <button data-x class="de-x" onclick={close} aria-label={$t("Close")}>✕</button>
      </header>
      {#if newer}
        <div class="de-newer">{tr("{u} saved a newer version.", { u: newer.by })} <button onclick={reload}>{$t("Load it")}</button></div>
      {/if}
      <div class="de-body">
        {#if preview}
          <div class="de-prev"><Markup src={body || "…"} /></div>
        {:else}
          <textarea bind:value={body} maxlength="20000" spellcheck="true" placeholder={$t("Write together: **bold**, __italic__, # headings, - lists, [links](https://…)")}></textarea>
        {/if}
      </div>
      <footer class="de-foot">
        <span class="de-info">{body.length}/20000</span>
        <button class="de-btn ghost" onclick={exportMd}>{$t("Export .md")}</button>
        <button class="de-btn" onclick={submit} disabled={busy || !dirty}>{busy ? "…" : $t("Save for everyone")}</button>
      </footer>
    </div>
  </div>
{/if}

<style>
  .de-veil { position: fixed; inset: 0; z-index: 900; display: grid; place-items: center; background: rgba(4, 6, 12, 0.5); animation: de-fade 0.14s ease-out; }
  .de { width: min(820px, calc(100vw - 20px)); height: min(720px, calc(100dvh - 24px)); display: flex; flex-direction: column; background: var(--bg-shell); border: 1px solid var(--border-strong); border-radius: calc(18px * var(--rk, 1)); overflow: hidden; animation: de-pop 0.18s cubic-bezier(.2,.9,.3,1.12); }
  .de-head { display: flex; align-items: center; gap: 10px; padding: 12px 14px; border-bottom: 1px solid var(--border); }
  .de-title { flex: 1; min-width: 0; border: none; background: none; color: var(--fg); font: inherit; font-size: 18px; font-weight: 800; outline: none; }
  .de-seg { display: flex; gap: 2px; padding: 2px; border-radius: calc(9px * var(--rk, 1)); background: var(--bg-input); }
  .de-seg button { border: none; background: none; color: var(--fg-muted); font: inherit; font-size: 12.5px; font-weight: 600; padding: 6px 10px; border-radius: calc(7px * var(--rk, 1)); cursor: pointer; }
  .de-seg button.on { background: var(--bg-shell); color: var(--accent); }
  .de-x { width: 30px; height: 30px; border-radius: 50%; border: none; background: var(--bg-hover); color: var(--fg-muted); cursor: pointer; }
  .de-newer { padding: 8px 14px; background: var(--accent-soft); color: var(--fg); font-size: 13px; }
  .de-newer button { border: none; background: none; color: var(--accent); font: inherit; font-weight: 700; cursor: pointer; }
  .de-body { flex: 1; min-height: 0; display: flex; }
  .de-body textarea { flex: 1; resize: none; border: none; outline: none; padding: 16px 20px; background: none; color: var(--fg); font: inherit; font-size: 14.5px; line-height: 1.6; }
  .de-prev { flex: 1; overflow-y: auto; padding: 16px 20px; font-size: 14.5px; line-height: 1.6; }
  .de-foot { display: flex; align-items: center; gap: 8px; padding: 10px 14px; border-top: 1px solid var(--border); }
  .de-info { flex: 1; font-size: 11.5px; color: var(--fg-dim); font-variant-numeric: tabular-nums; }
  .de-btn { border: none; border-radius: calc(10px * var(--rk, 1)); padding: 9px 16px; background: var(--accent); color: #06070C; font: inherit; font-weight: 700; cursor: pointer; }
  .de-btn.ghost { background: var(--bg-input); color: var(--fg); }
  .de-btn:disabled { opacity: 0.55; }
  @keyframes de-fade { from { opacity: 0; } }
  @keyframes de-pop { from { opacity: 0; transform: translateY(12px) scale(0.98); } }
</style>
