<script lang="ts">
  import { t, tr } from "../i18n";
  import { shortcutsOpen, rebinding } from "../shortcuts";
  import { ACTIONS, FIXED, bindings, keyOverrides, prettyCombo, comboOf, setBinding, resetBinding, resetAllBindings, type ActionId } from "../keybindings";
  import { showToast } from "./Toast.svelte";
  import { invoke } from "@tauri-apps/api/core";

  const groups = [...new Set([...ACTIONS.map((a) => a.group), ...FIXED.map((f) => f.group)])];

  /** Waiting for a combo for this action ("add" keeps the old ones). */
  let mode = $state<"replace" | "add">("replace");

  function startRebind(id: ActionId, add = false) { mode = add ? "add" : "replace"; rebinding.set(id); }
  function stopRebind() { rebinding.set(null); }

  function onKey(e: KeyboardEvent) {
    if (!$shortcutsOpen) return;
    const id = $rebinding as ActionId | null;
    if (!id) {
      if (e.key === "Escape") shortcutsOpen.set(false);
      return;
    }
    e.preventDefault();
    e.stopPropagation();
    if (e.key === "Escape" && !e.ctrlKey && !e.altKey && !e.shiftKey) { stopRebind(); return; }
    const c = comboOf(e);
    if (!c) return; // a modifier alone: keep waiting
    if (!/^(Ctrl|Alt)\+/.test(c) && !/^(Shift\+)?F\d{1,2}$/.test(c)) {
      showToast(tr("Use Ctrl or Alt with a key (or an F-key) — plain keys are for typing"));
      return;
    }
    const combos = mode === "add" ? [...new Set([...$bindings[id], c])] : [c];
    const taken = setBinding(id, combos);
    stopRebind();
    if (taken.length) {
      const names = taken.map((x) => tr(ACTIONS.find((a) => a.id === x)?.what ?? x)).join(", ");
      showToast(tr("{k} was taken from: {a}", { k: prettyCombo(c).join("+"), a: names }));
    }
  }

  async function openConfig() {
    try { await invoke("open_keybindings_file"); }
    catch (e) { showToast(String(e)); }
  }

  $effect(() => { if (!$shortcutsOpen) rebinding.set(null); });
</script>

<svelte:window onkeydown={onKey} />

{#if $shortcutsOpen}
  <div class="backdrop" role="button" tabindex="-1" aria-label={$t("Close")}
    onclick={(e) => { if ((e.target as HTMLElement).classList.contains("backdrop")) shortcutsOpen.set(false); }}
    onkeydown={() => {}}>
    <div class="sheet" role="dialog" aria-modal="true" aria-label={$t("Keyboard shortcuts")} tabindex="-1">
      <div class="head">
        <h2>{$t("Keyboard shortcuts")}</h2>
        <span class="hint">{$t("Click a shortcut to change it")}</span>
        <span class="grow"></span>
        <button class="hbtn" onclick={openConfig} title="~/.config/aquamarine/keybindings.json">{$t("Config file")}</button>
        {#if Object.keys($keyOverrides).length}<button class="hbtn" onclick={resetAllBindings}>{$t("Reset all")}</button>{/if}
        <button data-x class="close" onclick={() => shortcutsOpen.set(false)} title={$t("Close (Esc)")}>×</button>
      </div>
      <div class="cols">
        {#each groups as g}
          <section>
            <div class="g">{$t(g)}</div>
            {#each ACTIONS.filter((a) => a.group === g) as a (a.id)}
              {@const custom = !!$keyOverrides[a.id]}
              <div class="row" class:custom class:capturing={$rebinding === a.id}>
                <span class="what">{$t(a.what)}</span>
                <span class="keys">
                  {#if $rebinding === a.id}
                    <span class="press">{$t("Press keys…")}</span>
                    <button class="mini" onclick={stopRebind} title="Esc">✕</button>
                  {:else}
                    <button class="combo-btn" onclick={() => startRebind(a.id)} title={$t("Change")}>
                      {#each $bindings[a.id] as c, ci (c)}
                        {#if ci > 0}<span class="or">/</span>{/if}
                        {#each prettyCombo(c) as k}<kbd>{k}</kbd>{/each}
                      {:else}
                        <span class="none">{$t("not set")}</span>
                      {/each}
                    </button>
                    <button class="mini" onclick={() => startRebind(a.id, true)} title={$t("Add another shortcut")}>＋</button>
                    {#if custom}<button class="mini" onclick={() => resetBinding(a.id)} title={$t("Back to default")}>↺</button>{/if}
                  {/if}
                </span>
              </div>
            {/each}
            {#each FIXED.filter((f) => f.group === g) as f (f.what)}
              <div class="row fixed">
                <span class="what">{$t(f.what)}</span>
                <span class="keys">
                  {#each f.keys as alt, ai}
                    {#if ai > 0}<span class="or">/</span>{/if}
                    {#each alt as k}<kbd>{k.length > 4 ? $t(k) : k}</kbd>{/each}
                  {/each}
                </span>
              </div>
            {/each}
          </section>
        {/each}
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed; inset: 0; z-index: 270;
    background: rgba(6, 7, 12, 0.6); backdrop-filter: blur(4px);
    display: grid; place-items: center; padding: 20px;
  }
  .sheet {
    width: min(820px, 96vw); max-height: 86vh; overflow: auto;
    background: var(--bg-shell); border: 1px solid var(--border); border-radius: calc(16px * var(--rk, 1));
    box-shadow: 0 32px 80px -16px rgba(0, 0, 0, 0.6);
    padding: 18px 22px 22px;
   animation: aq-pop .3s var(--ease-spring, ease-out); }
  .head { display: flex; align-items: center; justify-content: space-between; margin-bottom: 8px; }
  h2 { margin: 0; font-size: 18px; color: var(--fg); }
  .close { width: 30px; height: 30px; border-radius: calc(8px * var(--rk, 1)); border: 1px solid var(--border); background: transparent; color: var(--fg-dim); font-size: 18px; cursor: pointer; }
  .cols { columns: 2 340px; column-gap: 28px; }
  section { break-inside: avoid; margin-top: 12px; }
  .g { font-size: 11px; font-weight: 800; text-transform: uppercase; letter-spacing: 0.08em; color: var(--accent); margin-bottom: 6px; }
  .row { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 6px 0; border-bottom: 1px solid var(--border); }
  .what { font-size: 13px; color: var(--fg); }
  .keys { display: flex; align-items: center; gap: 4px; flex-shrink: 0; }
  .hint { font-size: 12px; color: var(--fg-dim); margin-left: 12px; }
  .grow { flex: 1; }
  .head { gap: 8px; }
  .hbtn { border: 1px solid var(--border-strong); background: transparent; color: var(--fg-muted); border-radius: calc(8px * var(--rk, 1)); padding: 5px 10px; font-size: 12px; font-weight: 600; cursor: pointer; }
  .hbtn:hover { color: var(--accent); border-color: var(--accent); }
  .combo-btn { display: flex; align-items: center; gap: 4px; border: 1px solid transparent; background: transparent; border-radius: calc(8px * var(--rk, 1)); padding: 2px 4px; cursor: pointer; }
  .combo-btn:hover { border-color: var(--accent); background: var(--accent-soft); }
  .mini { width: 22px; height: 22px; border-radius: calc(6px * var(--rk, 1)); border: 1px solid var(--border); background: transparent; color: var(--fg-dim); font-size: 12px; cursor: pointer; opacity: 0; transition: opacity .15s; }
  .row:hover .mini, .row.capturing .mini, .mini:focus-visible { opacity: 1; }
  .mini:hover { color: var(--accent); border-color: var(--accent); }
  .row.custom .what::after { content: " •"; color: var(--accent); }
  .row.capturing { background: var(--accent-soft); border-radius: calc(8px * var(--rk, 1)); padding-left: 8px; padding-right: 4px; }
  .press { font-size: 12px; font-weight: 700; color: var(--accent); animation: blink 1s ease-in-out infinite; }
  @keyframes blink { 50% { opacity: .45; } }
  .none { font-size: 11.5px; color: var(--fg-dim); font-style: italic; }
  .row.fixed .what { color: var(--fg-muted); }
  .or { color: var(--fg-dim); font-size: 11px; margin: 0 2px; }
  kbd {
    font-family: var(--font-mono, monospace); font-size: 11px; font-weight: 700;
    padding: 2px 6px; border-radius: calc(6px * var(--rk, 1));
    background: var(--bg-input); color: var(--fg-muted);
    border: 1px solid var(--border-strong); border-bottom-width: 2px;
  }
</style>
