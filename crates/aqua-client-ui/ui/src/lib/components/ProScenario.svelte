<script module lang="ts">
  export type Choice = { label: string; next: string };
  export type Step =
    | { type: "message"; text: string; next: string }
    | { type: "buttons"; text: string; choices: Choice[] }
    | { type: "wait"; next: string }
    | { type: "branch"; equals: string; yes: string; no: string };
  export type Scenario = { entry: string; steps: Record<string, Step> };
</script>

<script lang="ts">
  import { t } from "../i18n";
  let { scenario = $bindable(), enabled }: { scenario?: Scenario | null; enabled: boolean } = $props();
  const names = { message: "Message", buttons: "Buttons", wait: "Wait for reply", branch: "Match reply" } as const;
  const factories: Record<Step["type"], () => Step> = {
    message: () => ({ type: "message", text: "", next: "" }),
    buttons: () => ({ type: "buttons", text: "", choices: [{ label: "", next: "" }] }),
    wait: () => ({ type: "wait", next: "" }),
    branch: () => ({ type: "branch", equals: "", yes: "", no: "" }),
  };
  let type = $state<Step["type"]>("message");
  let cursor = $state("");
  let messages = $state<string[]>([]);
  let answer = $state("");
  let current = $derived(scenario?.steps[cursor]);
  function add() {
    if (!scenario) return;
    let n = 1;
    while (Object.hasOwn(scenario.steps, "step_" + n)) n++;
    scenario.steps["step_" + n] = factories[type]();
  }
  function remove(id: string) {
    if (!scenario) return;
    delete scenario.steps[id];
    for (const step of Object.values(scenario.steps)) {
      if ("next" in step && step.next === id) step.next = "";
      if (step.type === "branch") {
        if (step.yes === id) step.yes = "";
        if (step.no === id) step.no = "";
      }
      if (step.type === "buttons") for (const choice of step.choices) {
        if (choice.next === id) choice.next = "";
      }
    }
  }
  function preview(next: string, input = "") {
    cursor = next;
    for (let i = 0; i < 16; i++) {
      const step: Step | undefined = scenario?.steps[cursor];
      if (!step) break;
      if (step.type === "wait" || step.type === "buttons") return;
      if (step.type === "message") { messages.push(step.text); cursor = step.next; }
      else cursor = input.trim().toLowerCase() === step.equals.trim().toLowerCase() ? step.yes : step.no;
    }
    cursor = "";
  }
</script>

{#snippet target(value: string, change: (value: string) => void, entry = false)}
  <select value={value} aria-label={$t(entry ? "First step" : "Next step")} onchange={(e) => change(e.currentTarget.value)}>
    {#if !entry}<option value="">{$t("End scenario")}</option>{/if}
    {#each Object.keys(scenario?.steps ?? {}) as id (id)}<option value={id}>{id}</option>{/each}
  </select>
{/snippet}

<details>
  <summary>{$t("Pro scenario")}</summary>
  {#if !enabled}<p>{$t("A Stars subscription is required to run a Pro scenario.")}</p>{/if}
  {#if !scenario}
    <button type="button" disabled={!enabled} onclick={() => scenario = { entry: "step_1", steps: { step_1: factories.message() } }}>{$t("Create scenario")}</button>
  {:else}
    <fieldset disabled={!enabled}>
      <label>{$t("First step")}{@render target(scenario.entry, (v) => { if (scenario) scenario.entry = v; }, true)}</label>
      {#each Object.entries(scenario.steps) as [id, step] (id)}
        <div class="step">
          <strong>{id} · {$t(names[step.type])}</strong>
          {#if step.type === "message" || step.type === "buttons"}
            <textarea rows="2" maxlength="4000" aria-label={$t("Message")} bind:value={step.text}></textarea>
          {/if}
          {#if "next" in step}
            {@render target(step.next, (v) => step.next = v)}
          {:else if step.type === "buttons"}
            {#each step.choices as choice, i (i)}
              <div class="row"><input maxlength="64" aria-label={$t("Button")} bind:value={choice.label} />{@render target(choice.next, (v) => choice.next = v)}
                <button type="button" disabled={step.choices.length === 1} onclick={() => step.choices.splice(i, 1)}>{$t("Remove")}</button></div>
            {/each}
            <button type="button" disabled={step.choices.length >= 8} onclick={() => step.choices.push({ label: "", next: "" })}>{$t("Add button")}</button>
          {:else}
            <input maxlength="200" aria-label={$t("Match reply")} bind:value={step.equals} />
            <label>{$t("Matches")}{@render target(step.yes, (v) => step.yes = v)}</label>
            <label>{$t("Otherwise")}{@render target(step.no, (v) => step.no = v)}</label>
          {/if}
          {#if id !== scenario.entry}<button type="button" onclick={() => remove(id)}>{$t("Remove step")}</button>{/if}
        </div>
      {/each}
      <div class="row">
        <select aria-label={$t("Step type")} bind:value={type}>
          {#each Object.entries(names) as [id, label] (id)}<option value={id}>{$t(label)}</option>{/each}
        </select>
        <button type="button" disabled={Object.keys(scenario.steps).length >= 50} onclick={add}>{$t("Add step")}</button>
        <button type="button" onclick={() => scenario = null}>{$t("Remove scenario")}</button>
      </div>
      <button type="button" onclick={() => { messages = []; if (scenario) preview(scenario.entry); }}>{$t("Preview scenario")}</button>
      <div class="preview" aria-live="polite">
        {#each messages as line, i (i)}<p>{line}</p>{/each}
        {#if current?.type === "buttons"}
          <p>{current.text}</p>
          {#each current.choices as choice, i (i)}<button type="button" onclick={() => preview(choice.next)}>{choice.label}</button>{/each}
        {:else if current?.type === "wait"}
          {@const next = current.next}
          <input aria-label={$t("Reply")} bind:value={answer} />
          <button type="button" onclick={() => { preview(next, answer); answer = ""; }}>{$t("Send")}</button>
        {/if}
      </div>
    </fieldset>
  {/if}
</details>

<style>
  details { padding: 12px; background: var(--bg-input); border: 1px solid var(--border); border-radius: 12px; }
  summary { cursor: pointer; font-weight: 600; }
  fieldset, .step { display: grid; gap: 8px; padding: 8px; margin: 0; min-inline-size: 0; border: 1px solid var(--border); border-radius: 8px; }
  label, .row { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; }
  textarea, select, input, button { background: var(--bg-surface); color: var(--fg); border: 1px solid var(--border); border-radius: 6px; padding: 6px; min-width: 0; max-width: 100%; font: inherit; }
  button { cursor: pointer; }
  .preview p { white-space: pre-wrap; overflow-wrap: anywhere; }
</style>
