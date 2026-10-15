<script lang="ts">
  import { t } from "../i18n";
  import { shortcutsOpen, SHORTCUTS } from "../shortcuts";

  const groups = [...new Set(SHORTCUTS.map((s) => s.group))];
  function keysOf(k: string): string[][] {
    // "Ctrl+Tab / Ctrl+Shift+Tab" → [["Ctrl","Tab"], ["Ctrl","Shift","Tab"]]
    return k.split(" / ").map((alt) => alt.split("+").map((x) => x.trim()));
  }
</script>

<svelte:window onkeydown={(e) => { if (e.key === "Escape" && $shortcutsOpen) shortcutsOpen.set(false); }} />

{#if $shortcutsOpen}
  <div class="backdrop" role="button" tabindex="-1" aria-label={$t("Close")}
    onclick={(e) => { if ((e.target as HTMLElement).classList.contains("backdrop")) shortcutsOpen.set(false); }}
    onkeydown={() => {}}>
    <div class="sheet" role="dialog" aria-modal="true" aria-label={$t("Keyboard shortcuts")} tabindex="-1">
      <div class="head">
        <h2>{$t("Keyboard shortcuts")}</h2>
        <button class="close" onclick={() => shortcutsOpen.set(false)} title={$t("Close (Esc)")}>×</button>
      </div>
      <div class="cols">
        {#each groups as g}
          <section>
            <div class="g">{g}</div>
            {#each SHORTCUTS.filter((s) => s.group === g) as s}
              <div class="row">
                <span class="what">{s.what}</span>
                <span class="keys">
                  {#each keysOf(s.keys) as alt, ai}
                    {#if ai > 0}<span class="or">/</span>{/if}
                    {#each alt as k}<kbd>{k}</kbd>{/each}
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
    background: var(--bg-shell); border: 1px solid var(--border); border-radius: 16px;
    box-shadow: 0 32px 80px -16px rgba(0, 0, 0, 0.6);
    padding: 18px 22px 22px;
   animation: aq-pop .3s var(--ease-spring, ease-out); }
  .head { display: flex; align-items: center; justify-content: space-between; margin-bottom: 8px; }
  h2 { margin: 0; font-size: 18px; color: var(--fg); }
  .close { width: 30px; height: 30px; border-radius: 8px; border: 1px solid var(--border); background: transparent; color: var(--fg-dim); font-size: 18px; cursor: pointer; }
  .cols { columns: 2 340px; column-gap: 28px; }
  section { break-inside: avoid; margin-top: 12px; }
  .g { font-size: 11px; font-weight: 800; text-transform: uppercase; letter-spacing: 0.08em; color: var(--accent); margin-bottom: 6px; }
  .row { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 6px 0; border-bottom: 1px solid var(--border); }
  .what { font-size: 13px; color: var(--fg); }
  .keys { display: flex; align-items: center; gap: 4px; flex-shrink: 0; }
  .or { color: var(--fg-dim); font-size: 11px; margin: 0 2px; }
  kbd {
    font-family: var(--font-mono, monospace); font-size: 11px; font-weight: 700;
    padding: 2px 6px; border-radius: 6px;
    background: var(--bg-input); color: var(--fg-muted);
    border: 1px solid var(--border-strong); border-bottom-width: 2px;
  }
</style>
