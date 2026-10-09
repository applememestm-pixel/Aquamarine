<script module lang="ts">
  import { writable } from "svelte/store";

  export interface PromptField {
    label: string;
    placeholder?: string;
    value?: string;
    monospace?: boolean;
  }

  export interface PromptSpec {
    title: string;
    description?: string;
    fields: PromptField[];
    submitLabel: string;
    onSubmit: (values: string[]) => Promise<string | null>;
  }

  export const promptOpen  = writable<boolean>(false);
  export const promptSpec  = writable<PromptSpec | null>(null);
  export const promptBusy  = writable<boolean>(false);
  export const promptError = writable<string | null>(null);

  export function askInput(s: PromptSpec) {
    promptSpec.set(s);
    promptError.set(null);
    promptBusy.set(false);
    promptOpen.set(true);
  }
  export function closeInput() {
    promptOpen.set(false);
    promptSpec.set(null);
  }
</script>

<script lang="ts">
  import { t } from "../i18n";
  let values: string[] = $state([]);

  $effect(() => {
    const s = $promptSpec;
    if (s) values = s.fields.map((f) => f.value ?? "");
  });

  async function submit() {
    const s = $promptSpec;
    if (!s) return;
    promptBusy.set(true);
    promptError.set(null);
    const err = await s.onSubmit(values);
    promptBusy.set(false);
    if (err) promptError.set(err);
    else closeInput();
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") closeInput();
    if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); submit(); }
  }
</script>

{#if $promptOpen && $promptSpec}
  <div
    class="bg"
    role="button" tabindex="-1"
    aria-label={$t("Close dialog")}
    onclick={(e) => { if (e.target === e.currentTarget) closeInput(); }}
    onkeydown={(e) => { if (e.key === "Escape" && e.target === e.currentTarget) closeInput(); }}
  >
    <div class="dialog" role="dialog" aria-modal="true">
      <h2>{$promptSpec.title}</h2>
      {#if $promptSpec.description}<p>{$promptSpec.description}</p>{/if}

      {#each $promptSpec.fields as f, i (i)}
        <label class="field">
          <span class="label">{f.label}</span>
          <input
            type="text"
            placeholder={f.placeholder ?? ""}
            bind:value={values[i]}
            onkeydown={onKey}
            class:mono={!!f.monospace}
            autofocus={i === 0}
          />
        </label>
      {/each}

      {#if $promptError}<div class="error">{$promptError}</div>{/if}

      <div class="actions">
        <button class="btn" onclick={closeInput} disabled={$promptBusy}>{$t("Cancel")}</button>
        <button class="btn primary" onclick={submit} disabled={$promptBusy}>
          {#if $promptBusy}...{:else}{$promptSpec.submitLabel}{/if}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .bg {
    position: fixed;
    inset: 0;
    background: rgba(6,7,12,0.6);
    backdrop-filter: blur(6px);
    display: grid; place-items: center;
    z-index: 200;
    padding: 20px;
  }
  .dialog {
    background: var(--bg-surface);
    border: 1px solid var(--border-strong);
    border-radius: 16px;
    padding: 24px;
    max-width: 420px;
    width: 100%;
    box-shadow: 0 32px 64px -16px rgba(0,0,0,0.6), var(--shadow-accent);
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  h2 { margin: 0; font-size: 17px; font-weight: 700; letter-spacing: -0.01em; }
  p  { margin: 0; color: var(--fg-muted); font-size: 13px; line-height: 1.5; }
  .field { display: flex; flex-direction: column; gap: 6px; }
  .label { font-size: 12px; color: var(--fg-muted); font-weight: 500; }
  input {
    background: var(--bg-input);
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 10px 12px;
    color: var(--fg);
    font-family: inherit;
    font-size: 14px;
    outline: none;
    transition: border-color 0.15s ease, box-shadow 0.15s ease;
  }
  input.mono { font-family: var(--font-mono); font-size: 13px; }
  input:focus {
    border-color: var(--accent-soft);
    box-shadow: 0 0 0 3px var(--accent-soft);
  }
  .error {
    color: var(--danger);
    font-size: 12.5px;
    padding: 8px 10px;
    background: rgba(255,77,109,0.08);
    border-radius: 8px;
    word-break: break-word;
  }
  .actions { display: flex; gap: 10px; justify-content: flex-end; margin-top: 4px; }
  .btn {
    padding: 9px 16px;
    border-radius: 10px;
    border: 1px solid var(--border-strong);
    background: var(--bg-input);
    color: var(--fg);
    font-family: inherit;
    font-weight: 600;
    font-size: 13.5px;
    cursor: pointer;
    transition: all 0.12s ease;
  }
  .btn:disabled { opacity: 0.5; cursor: not-allowed; }
  .btn:hover:not(:disabled) { background: var(--bg-hover); border-color: var(--fg-dim); }
  .btn.primary {
    background: linear-gradient(135deg, var(--accent), var(--accent-2));
    color: #06070C;
    border-color: transparent;
    box-shadow: 0 0 16px -4px var(--accent-glow);
  }
  .btn.primary:hover:not(:disabled) { box-shadow: 0 0 24px -2px var(--accent-glow); }
</style>
