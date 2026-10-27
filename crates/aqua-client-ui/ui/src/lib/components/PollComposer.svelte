<script module lang="ts">
  import { writable } from "svelte/store";
  export const pollComposerOpen = writable(false);
  export function openPollComposer() { pollComposerOpen.set(true); }
  export function closePollComposer() { pollComposerOpen.set(false); }
</script>

<script lang="ts">
  import { onMount } from "svelte";
  import { sendCurrent, loadActiveMessages, activeChat } from "../stores";
  import { showToast } from "./Toast.svelte";
  import { t, tr } from "../i18n";

  let question = $state("");
  let options = $state<string[]>(["", ""]);
  let multi = $state(false);
  let anonymous = $state(true);
  let quiz = $state(false);
  let correct = $state<number | null>(null);
  let explanation = $state("");
  let closeIn = $state(0); // minutes, 0 = never
  let busy = $state(false);
  const MAX = 12;

  const CLOSE_OPTS: [number, string][] = [[0, "Never"], [5, "5 min"], [60, "1 hour"], [360, "6 hours"], [1440, "1 day"], [10080, "1 week"]];

  function addOption() { if (options.length < MAX) options = [...options, ""]; }
  function removeOption(i: number) {
    if (options.length <= 2) return;
    options = options.filter((_, j) => j !== i);
    if (correct === i) correct = null; else if (correct !== null && correct > i) correct--;
  }
  function move(i: number, d: -1 | 1) {
    const j = i + d;
    if (j < 0 || j >= options.length) return;
    const next = [...options];
    [next[i], next[j]] = [next[j], next[i]];
    options = next;
    if (correct === i) correct = j; else if (correct === j) correct = i;
  }
  function onOptInput(i: number) {
    // Telegram: typing into the last box adds a fresh empty one.
    if (i === options.length - 1 && options[i].trim() && options.length < MAX) options = [...options, ""];
  }

  function reset() {
    question = ""; options = ["", ""]; multi = false; anonymous = true; quiz = false;
    correct = null; explanation = ""; closeIn = 0;
  }

  async function create() {
    const q = question.trim();
    // Keep original indices so `correct` still points at the right option.
    const filled = options.map((o, i) => ({ o: o.trim(), i })).filter((x) => x.o);
    if (!q) { showToast(tr("Enter a question")); return; }
    if (filled.length < 2) { showToast(tr("At least two options")); return; }
    let correctIdx: number | undefined;
    if (quiz) {
      const k = filled.findIndex((x) => x.i === correct);
      if (k < 0) { showToast(tr("Quiz: tap the circle next to the right answer")); return; }
      correctIdx = k;
    }
    if (!$activeChat?.live) { showToast(tr("Open a live chat first")); return; }
    busy = true;
    const id = "poll-" + Math.random().toString(36).slice(2, 10);
    const body: Record<string, unknown> = { id, question: q, options: filled.map((x) => x.o), multi: multi && !quiz };
    if (quiz) { body.quiz = true; body.correct = correctIdx; if (explanation.trim()) body.explanation = explanation.trim(); }
    if (!anonymous) body.public = true;
    if (closeIn > 0) body.close_at = Date.now() + closeIn * 60_000;
    const err = await sendCurrent("[poll:v1]" + JSON.stringify(body));
    busy = false;
    if (err) { showToast(err); return; }
    await loadActiveMessages();
    reset();
    closePollComposer();
  }

  onMount(() => {
    const h = (e: KeyboardEvent) => { if (e.key === "Escape") closePollComposer(); };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  });
</script>

{#if $pollComposerOpen}
  <div class="pc-backdrop" role="button" tabindex="-1" aria-label={$t("Close")}
       onclick={(e) => { if ((e.target as HTMLElement).classList.contains("pc-backdrop")) closePollComposer(); }}
       onkeydown={(e) => { if (e.key === "Escape") closePollComposer(); }}>
    <div class="pc-sheet" role="dialog" aria-modal="true" tabindex="-1">
      <div class="pc-head">
        <h3>{quiz ? $t("New quiz") : $t("New poll")}</h3>
        <button class="pc-close" onclick={closePollComposer} title={$t("Close (Esc)")}>×</button>
      </div>
      <div class="pc-body">
        <label class="pc-field">
          <span>{$t("Question")}</span>
          <input bind:value={question} maxlength="300" placeholder={$t("Ask something…")} />
        </label>
        <div class="pc-opts">
          <span class="pc-lbl">{$t("Options")} <em>{options.filter((o) => o.trim()).length}/{MAX}</em></span>
          {#each options as _, i (i)}
            <div class="pc-opt" class:right={quiz && correct === i}>
              {#if quiz}
                <button class="pc-correct" class:on={correct === i} onclick={() => (correct = i)} title={$t("Right answer")} aria-label={$t("Right answer")}></button>
              {/if}
              <input bind:value={options[i]} maxlength="100" placeholder={$t("Option {n}", { n: i + 1 })}
                     oninput={() => onOptInput(i)}
                     onkeydown={(e) => { if (e.key === "Enter") { e.preventDefault(); addOption(); } }} />
              <div class="pc-move">
                <button onclick={() => move(i, -1)} disabled={i === 0} title={$t("Move up")}>▲</button>
                <button onclick={() => move(i, 1)} disabled={i === options.length - 1} title={$t("Move down")}>▼</button>
              </div>
              {#if options.length > 2}
                <button class="pc-rm" onclick={() => removeOption(i)} title={$t("Remove")}>×</button>
              {/if}
            </div>
          {/each}
          {#if options.length < MAX}
            <button class="pc-add" onclick={addOption}>+ {$t("Add option")}</button>
          {/if}
        </div>

        <div class="pc-settings">
          <button class="pc-toggle" onclick={() => (anonymous = !anonymous)}>
            <span><b>{$t("Anonymous voting")}</b><small>{$t("Nobody sees who voted for what")}</small></span>
            <span class="sw" class:on={anonymous}></span>
          </button>
          <button class="pc-toggle" onclick={() => (multi = !multi)} disabled={quiz}>
            <span><b>{$t("Multiple answers")}</b><small>{$t("Voters can pick several options")}</small></span>
            <span class="sw" class:on={multi && !quiz}></span>
          </button>
          <button class="pc-toggle" onclick={() => { quiz = !quiz; if (quiz) multi = false; }}>
            <span><b>{$t("Quiz mode")}</b><small>{$t("One right answer, revealed after voting")}</small></span>
            <span class="sw" class:on={quiz}></span>
          </button>
          {#if quiz}
            <label class="pc-field">
              <span>{$t("Explanation (optional)")}</span>
              <input bind:value={explanation} maxlength="200" placeholder={$t("Shown after someone answers")} />
            </label>
          {/if}
          <div class="pc-field">
            <span>{$t("Close automatically")}</span>
            <div class="pc-seg">
              {#each CLOSE_OPTS as [m, lbl] (m)}
                <button class:on={closeIn === m} onclick={() => (closeIn = m)}>{$t(lbl)}</button>
              {/each}
            </div>
          </div>
        </div>
      </div>
      <div class="pc-foot">
        <button class="pc-ghost" onclick={closePollComposer}>{$t("Cancel")}</button>
        <button class="pc-create" onclick={create} disabled={busy}>{busy ? $t("Sending…") : quiz ? $t("Create quiz") : $t("Create poll")}</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .pc-backdrop {
    position: fixed; inset: 0; background: rgba(6,7,12,0.55);
    backdrop-filter: blur(4px); display: grid; place-items: center;
    z-index: 115; padding: 20px; animation: pc-fade 0.15s ease-out;
  }
  @keyframes pc-fade { from { opacity: 0; } to { opacity: 1; } }
  .pc-sheet {
    width: min(460px, 94vw); max-height: 88vh; display: flex; flex-direction: column;
    background: var(--bg-shell); border: 1px solid var(--border);
    border-radius: calc(16px * var(--rk, 1)); overflow: hidden; box-shadow: 0 32px 80px -16px rgba(0,0,0,0.6);
  }
  .pc-head { display: flex; align-items: center; justify-content: space-between; padding: 14px 16px; border-bottom: 1px solid var(--border); }
  .pc-head h3 { margin: 0; font-size: 14.5px; font-weight: 700; }
  .pc-close { width: 28px; height: 28px; background: transparent; border: 1px solid var(--border); border-radius: calc(8px * var(--rk, 1)); color: var(--fg-dim); font-size: 17px; cursor: pointer; }
  .pc-close:hover { color: var(--fg); background: var(--bg-hover); }
  .pc-body { padding: 14px 16px; overflow-y: auto; display: flex; flex-direction: column; gap: 14px; }
  .pc-field { display: flex; flex-direction: column; gap: 5px; }
  .pc-field span, .pc-lbl { font-size: 11px; font-weight: 700; color: var(--fg-dim); text-transform: uppercase; letter-spacing: 0.06em; }
  .pc-field input, .pc-opt input {
    background: var(--bg-input); border: 1px solid var(--border); border-radius: calc(9px * var(--rk, 1));
    color: var(--fg); font-family: inherit; font-size: 13.5px; padding: 9px 11px; outline: none; width: 100%;
  }
  .pc-field input:focus, .pc-opt input:focus { border-color: var(--accent); }
  .pc-opts { display: flex; flex-direction: column; gap: 7px; }
  .pc-opt { display: flex; align-items: center; gap: 6px; }
  .pc-rm { width: 30px; height: 34px; flex-shrink: 0; background: transparent; border: 1px solid var(--border); border-radius: calc(8px * var(--rk, 1)); color: var(--danger); font-size: 15px; cursor: pointer; }
  .pc-add { align-self: flex-start; background: transparent; border: none; color: var(--accent); font-size: 12.5px; font-weight: 600; cursor: pointer; padding: 2px 0; }
  .pc-lbl em { font-style: normal; color: var(--fg-dim); font-weight: 600; margin-left: 4px; }
  .pc-opt.right input { border-color: #3ddc84; }
  .pc-correct { width: 20px; height: 20px; flex-shrink: 0; border-radius: 50%; border: 2px solid var(--fg-dim); background: none; cursor: pointer; position: relative; }
  .pc-correct.on { border-color: #3ddc84; background: #3ddc84; box-shadow: inset 0 0 0 3px var(--bg-shell); }
  .pc-move { display: flex; flex-direction: column; gap: 1px; }
  .pc-move button { background: none; border: none; color: var(--fg-dim); font-size: 8px; line-height: 1; padding: 2px 3px; cursor: pointer; }
  .pc-move button:hover:not(:disabled) { color: var(--accent); }
  .pc-move button:disabled { opacity: 0.25; cursor: default; }
  .pc-settings { display: flex; flex-direction: column; gap: 4px; border-top: 1px solid var(--border); padding-top: 10px; }
  .pc-toggle {
    display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 8px 4px;
    background: none; border: none; color: var(--fg); font: inherit; text-align: left; cursor: pointer; border-radius: calc(8px * var(--rk, 1));
  }
  .pc-toggle:hover:not(:disabled) { background: var(--bg-hover); }
  .pc-toggle:disabled { opacity: 0.45; cursor: default; }
  .pc-toggle b { display: block; font-size: 13.5px; font-weight: 600; }
  .pc-toggle small { display: block; font-size: 11.5px; color: var(--fg-dim); }
  .sw { width: 36px; height: 20px; border-radius: 100px; background: var(--border-strong); position: relative; flex-shrink: 0; transition: background 0.15s; }
  .sw::after { content: ""; position: absolute; top: 3px; left: 3px; width: 14px; height: 14px; border-radius: 50%; background: var(--fg-dim); transition: transform 0.15s, background 0.15s; }
  .sw.on { background: var(--accent-soft); }
  .sw.on::after { transform: translateX(16px); background: var(--accent); }
  .pc-seg { display: flex; flex-wrap: wrap; gap: 5px; }
  .pc-seg button { border: 1px solid var(--border); background: var(--bg-input); color: var(--fg-muted); border-radius: 100px; padding: 5px 10px; font: inherit; font-size: 12px; font-weight: 600; cursor: pointer; }
  .pc-seg button.on { border-color: var(--accent); color: var(--accent); background: var(--accent-soft); }
  .pc-check { display: flex; align-items: center; gap: 8px; cursor: pointer; }
  .pc-check input { accent-color: var(--accent); width: 16px; height: 16px; }
  .pc-check span { font-size: 13px; color: var(--fg-muted); }
  .pc-foot { display: flex; justify-content: flex-end; gap: 10px; padding: 12px 16px; border-top: 1px solid var(--border); }
  .pc-ghost, .pc-create { padding: 8px 16px; border-radius: calc(9px * var(--rk, 1)); font-size: 13px; font-weight: 700; cursor: pointer; border: none; }
  .pc-ghost { background: transparent; border: 1px solid var(--border); color: var(--fg-muted); }
  .pc-ghost:hover { color: var(--fg); border-color: var(--border-strong); }
  .pc-create { background: linear-gradient(135deg, var(--accent), var(--accent-2)); color: #06070C; }
  .pc-create:disabled { opacity: 0.5; cursor: not-allowed; }
</style>
