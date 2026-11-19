<script module lang="ts">
  import { writable } from "svelte/store";
  /** The bot whose constructor is open (its @name), or null. */
  export const botMakerFor = writable<string | null>(null);
</script>

<script lang="ts">
  /**
   * Bot constructor: a bot without code. A greeting (/start), commands with
   * their answers, answers to words, buttons that lead to another answer or
   * open a link, and what to say to anything else. The relay answers for the
   * bot (aqua-server botmaker.rs); saved with the bot's token.
   */
  import { t, tr } from "../i18n";
  import { api } from "../tauri";
  import { showToast } from "./Toast.svelte";
  import Select from "./Select.svelte";
  import { onBack } from "../back";

  interface Btn { text: string; goto: string; url: string }
  interface Reply { text: string; buttons: Btn[][] }
  interface Cmd { command: string; description: string; reply: Reply }
  interface Kw { words: string; reply: Reply }
  interface Spec { about: string; start: Reply; commands: Cmd[]; keywords: Kw[]; fallback?: Reply | null }

  const empty = (text = ""): Reply => ({ text, buttons: [] });
  let spec = $state<Spec>({ about: "", start: empty(), commands: [], keywords: [] });
  let hasFallback = $state(false);
  let fallback = $state<Reply>(empty());
  let loading = $state(false);
  let saving = $state(false);
  let on = $state(false);
  /** The saved spec didn't load: saving now would replace it with this draft. */
  let loadFailed = $state(false);

  $effect(() => {
    const bot = $botMakerFor;
    if (!bot) return;
    loading = true;
    on = false; loadFailed = false;
    spec = { about: "", start: empty(tr("Hi! I'm a bot.")), commands: [], keywords: [] };
    hasFallback = false; fallback = empty();
    api.botSpec(bot, "get").then((v) => {
      if ($botMakerFor !== bot || !v || typeof v !== "object") return;
      const s = v as Spec;
      const fix = (r?: Reply | null): Reply => ({ text: r?.text ?? "", buttons: (r?.buttons ?? []).map((row) => row.map((b) => ({ text: b.text ?? "", goto: b.goto ?? "", url: b.url ?? "" }))) });
      spec = { about: s.about ?? "", start: fix(s.start), commands: (s.commands ?? []).map((c) => ({ command: c.command, description: c.description ?? "", reply: fix(c.reply) })), keywords: (s.keywords ?? []).map((k) => ({ words: k.words, reply: fix(k.reply) })) };
      hasFallback = !!s.fallback; fallback = fix(s.fallback);
      on = true;
    }).catch((e) => { loadFailed = true; showToast(String(e), 6000); }).finally(() => { loading = false; });
  });
  $effect(() => { if ($botMakerFor) return onBack(() => botMakerFor.set(null)); });

  /** Where a button can lead: the greeting or a command. */
  let targets = $derived([
    { value: "start", label: "/start" },
    ...spec.commands.filter((c) => c.command.trim()).map((c) => ({ value: c.command.trim().replace(/^\//, "").toLowerCase(), label: "/" + c.command.trim().replace(/^\//, "").toLowerCase() })),
    { value: "@url", label: tr("A link…") },
  ]);

  /** Renaming a command takes its buttons along (they pointed at the old name). */
  function renameCmd(c: Cmd, next: string) {
    const norm = (x: string) => x.trim().replace(/^\//, "").toLowerCase();
    const was = norm(c.command), now = norm(next);
    c.command = next;
    if (!was || was === now) return;
    const all = [spec.start, ...spec.commands.map((x) => x.reply), ...spec.keywords.map((k) => k.reply), ...(hasFallback ? [fallback] : [])];
    for (const r of all) for (const row of r.buttons) for (const b of row) if (b.goto === was) b.goto = now;
  }
  function addButton(r: Reply) { if (r.buttons.length < 8) r.buttons.push([{ text: "", goto: "start", url: "" }]); }
  function setTarget(b: Btn, v: string) { if (v === "@url") { b.goto = ""; b.url = b.url || "https://"; } else { b.goto = v; b.url = ""; } }

  function payload(): Spec {
    const out: Spec = $state.snapshot(spec) as Spec;
    out.commands = out.commands.map((c) => ({ ...c, command: c.command.trim().replace(/^\//, "").toLowerCase() }));
    out.fallback = hasFallback ? ($state.snapshot(fallback) as Reply) : undefined;
    const strip = (r: Reply) => { r.buttons = r.buttons.filter((row) => row.length && row[0].text.trim()); };
    strip(out.start); out.commands.forEach((c) => strip(c.reply)); out.keywords.forEach((k) => strip(k.reply)); if (out.fallback) strip(out.fallback);
    return out;
  }

  async function save() {
    const bot = $botMakerFor;
    if (!bot || loadFailed) return;
    saving = true;
    try {
      await api.botSpec(bot, "put", payload());
      on = true;
      showToast(tr("Saved — the bot answers now"));
    } catch (e) {
      showToast(tr("Couldn't save: {e}", { e: String(e) }), 7000);
    } finally { saving = false; }
  }
  async function turnOff() {
    const bot = $botMakerFor;
    if (!bot) return;
    try { await api.botSpec(bot, "delete"); on = false; showToast(tr("The constructor is off — the bot is silent until a program runs it")); }
    catch (e) { showToast(String(e), 6000); }
  }
</script>

{#snippet replyEd(r: Reply, ph: string)}
  <textarea class="bm-text" rows="3" maxlength="4000" placeholder={$t(ph)} bind:value={r.text}></textarea>
  {#each r.buttons as row, ri (ri)}
    {@const b = row[0]}
    <div class="bm-btn">
      <input class="bm-in" maxlength="64" placeholder={$t("Button")} bind:value={b.text} />
      <Select class="bm-sel" value={b.url ? "@url" : b.goto} options={targets} onchange={(v: string) => setTarget(b, v)} />
      {#if b.url}<input class="bm-in" placeholder="https://" bind:value={b.url} />{/if}
      <button class="bm-x" onclick={() => r.buttons.splice(ri, 1)} aria-label={$t("Remove")}>×</button>
    </div>
  {/each}
  {#if r.buttons.length < 8}<button class="bm-add small" onclick={() => addButton(r)}>＋ {$t("Button")}</button>{/if}
{/snippet}

{#if $botMakerFor}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="bm-veil" onclick={(e) => { if (e.target === e.currentTarget) botMakerFor.set(null); }}>
    <div class="bm" role="dialog" aria-modal="true" aria-label={$t("Bot constructor")}>
      <div class="bm-head">
        <div class="bm-title">{$t("Bot constructor")} <span class="bm-at">@{$botMakerFor}</span></div>
        <button data-x class="bm-close" onclick={() => botMakerFor.set(null)} aria-label={$t("Close")}>×</button>
      </div>
      {#if loading}
        <div class="bm-note">{$t("Loading…")}</div>
      {:else}
      <div class="bm-body">
        {#if loadFailed}<div class="bm-warn">{$t("Couldn't load what the bot answers now — saving is off so it isn't replaced. Close and open the constructor again.")}</div>{/if}
        <div class="bm-note">{$t("A bot without code: write what it answers, the server does the rest. Buttons lead to another answer or open a link.")}{#if on} <b class="bm-on">{$t("On")}</b>{/if}</div>

        <div class="bm-sec">{$t("About the bot")}</div>
        <textarea class="bm-text" rows="2" maxlength="512" placeholder={$t("What it does — shown in its profile")} bind:value={spec.about}></textarea>

        <div class="bm-sec">/start — {$t("greeting")}</div>
        {@render replyEd(spec.start, "What the bot says first")}

        <div class="bm-sec">{$t("Commands")}</div>
        {#each spec.commands as c, ci (ci)}
          <div class="bm-card">
            <div class="bm-row">
              <span class="bm-slash">/</span>
              <input class="bm-in cmd" maxlength="32" placeholder="prices" value={c.command} oninput={(e) => renameCmd(c, (e.currentTarget as HTMLInputElement).value)} />
              <input class="bm-in" maxlength="256" placeholder={$t("Description in the / menu")} bind:value={c.description} />
              <button class="bm-x" onclick={() => spec.commands.splice(ci, 1)} aria-label={$t("Remove")}>×</button>
            </div>
            {@render replyEd(c.reply, "The answer")}
          </div>
        {/each}
        {#if spec.commands.length < 50}<button class="bm-add" onclick={() => spec.commands.push({ command: "", description: "", reply: empty() })}>＋ {$t("Command")}</button>{/if}

        <div class="bm-sec">{$t("Answers to words")}</div>
        {#each spec.keywords as k, ki (ki)}
          <div class="bm-card">
            <div class="bm-row">
              <input class="bm-in" maxlength="300" placeholder={$t("Words, comma-separated: price, cost")} bind:value={k.words} />
              <button class="bm-x" onclick={() => spec.keywords.splice(ki, 1)} aria-label={$t("Remove")}>×</button>
            </div>
            {@render replyEd(k.reply, "The answer")}
          </div>
        {/each}
        {#if spec.keywords.length < 100}<button class="bm-add" onclick={() => spec.keywords.push({ words: "", reply: empty() })}>＋ {$t("Answer to words")}</button>{/if}

        <div class="bm-sec">{$t("Anything else")}</div>
        <label class="bm-check"><input type="checkbox" bind:checked={hasFallback} /> {$t("Answer messages it doesn't know")}</label>
        {#if hasFallback}{@render replyEd(fallback, "E.g. “I don't understand — try /start”")}{/if}
      </div>
      <div class="bm-foot">
        {#if on}<button class="bm-off" onclick={turnOff}>{$t("Turn off")}</button>{/if}
        <span class="bm-sp"></span>
        <button class="bm-save" onclick={save} disabled={saving || loadFailed}>{$t(saving ? "Saving…" : "Save")}</button>
      </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .bm-veil { position: fixed; inset: 0; z-index: 270; background: rgba(0, 0, 0, .45); display: grid; place-items: center; }
  .bm { width: min(640px, 96vw); max-height: min(88dvh, 900px); display: flex; flex-direction: column; background: var(--bg-surface); color: var(--fg);
    border: 1px solid var(--border-strong); border-radius: calc(18px * var(--rk, 1)); box-shadow: 0 30px 70px -20px rgba(0, 0, 0, .8); overflow: hidden; }
  .bm-head { display: flex; align-items: center; gap: 8px; padding: 14px 16px; border-bottom: 1px solid var(--border); }
  .bm-title { font-weight: 700; font-size: 16px; flex: 1; min-width: 0; }
  .bm-at { color: var(--fg-dim); font-weight: 500; }
  .bm-close { width: 32px; height: 32px; border: none; border-radius: 50%; background: transparent; color: var(--fg-muted); font-size: 22px; cursor: pointer; }
  .bm-body { overflow-y: auto; padding: 12px 16px 16px; display: flex; flex-direction: column; gap: 8px; }
  .bm-note { font-size: 13px; color: var(--fg-muted); padding: 12px 16px 0; }
  .bm-body .bm-note { padding: 0; }
  .bm-on { color: var(--accent); }
  .bm-warn { font-size: 13px; color: var(--danger); padding: 8px 10px; border-radius: calc(10px * var(--rk, 1)); background: color-mix(in srgb, var(--danger) 12%, transparent); }
  .bm-sec { margin-top: 10px; font-size: 12px; font-weight: 700; letter-spacing: .04em; text-transform: uppercase; color: var(--fg-dim); }
  .bm-text, .bm-in { width: 100%; box-sizing: border-box; border: 1px solid var(--border); background: var(--bg-input); color: var(--fg); border-radius: calc(10px * var(--rk, 1));
    padding: 8px 10px; font: 14px var(--font-ui, inherit); resize: vertical; min-width: 0; }
  .bm-card { display: flex; flex-direction: column; gap: 6px; padding: 10px; border: 1px solid var(--border); border-radius: calc(12px * var(--rk, 1)); background: var(--bg-surface-2); }
  .bm-row, .bm-btn { display: flex; gap: 6px; align-items: center; }
  .bm-btn { flex-wrap: wrap; }
  .bm-btn .bm-in { flex: 1 1 140px; }
  .bm-btn :global(.bm-sel) { flex: 0 1 160px; min-width: 0; }
  .bm-slash { color: var(--fg-dim); font-weight: 700; }
  .bm-in.cmd { flex: 0 0 120px; }
  .bm-x { flex-shrink: 0; width: 30px; height: 30px; border: none; border-radius: 50%; background: transparent; color: var(--fg-dim); font-size: 18px; cursor: pointer; }
  .bm-x:hover { color: var(--danger); background: var(--bg-hover); }
  .bm-add { align-self: flex-start; border: none; background: var(--accent-soft); color: var(--accent); border-radius: calc(10px * var(--rk, 1)); padding: 7px 12px; font: 600 13px var(--font-ui, inherit); cursor: pointer; }
  .bm-add.small { padding: 5px 10px; font-size: 12px; }
  .bm-check { display: flex; align-items: center; gap: 8px; font-size: 14px; }
  .bm-foot { display: flex; align-items: center; gap: 8px; padding: 12px 16px; border-top: 1px solid var(--border); }
  .bm-sp { flex: 1; }
  .bm-save { border: none; border-radius: calc(10px * var(--rk, 1)); padding: 9px 18px; font: 700 14px var(--font-ui, inherit); cursor: pointer;
    background: linear-gradient(135deg, var(--accent), var(--accent-2)); color: var(--bg-shell); }
  .bm-save:disabled { opacity: .6; }
  .bm-off { border: 1px solid var(--border-strong); background: transparent; color: var(--fg-muted); border-radius: calc(10px * var(--rk, 1)); padding: 8px 14px; font: 600 13px var(--font-ui, inherit); cursor: pointer; }
</style>
