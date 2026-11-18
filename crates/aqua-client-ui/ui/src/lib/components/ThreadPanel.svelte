<script module lang="ts">
  export interface ThreadItem {
    id: string;
    author: string;
    time: string;
    text: string;
    outgoing: boolean;
    /** Answer to another comment in the thread. */
    re?: { a: string; t: string } | null;
    depth: number;
    /** Sent as a group / channel (checked, sendAs.ts). */
    as?: { gid: string; title: string } | null;
  }
</script>

<script lang="ts">
  import { sideResize } from "../sideResize";
  /** A message with everything said about it: replies (groups, chats) or
   *  comments (channel posts) — with answers to answers. */
  import { t, tr } from "../i18n";
  import { avatars } from "../people";
  import Markup from "./Markup.svelte";
  import { icons } from "../icons";
  import { IS_MOBILE } from "../platform";
  import { info } from "../stores";
  const me = $derived($info?.username ?? "?");
  const CLOSE = `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="M6 6l12 12M18 6 6 18"/></svg>`;

  let { title, rootAuthor, rootText, rootTime, items, canWrite = true, onSend, onClose, onJump, asChoices = [], asPick = "", onAs }: {
    title: string;
    rootAuthor: string;
    rootText: string;
    rootTime: string;
    items: ThreadItem[];
    canWrite?: boolean;
    onSend: (text: string, re: { a: string; t: string } | null) => Promise<void>;
    onClose: () => void;
    onJump?: (id: string) => void;
    /** "Send message as…": groups / channels I run, the current pick ("" = me). */
    asChoices?: { id: string; title: string; gradient: string }[];
    asPick?: string;
    onAs?: (gid: string) => void;
  } = $props();
  let asOpen = $state(false);
  const picked = $derived(asChoices.find((c) => c.id === asPick));

  let text = $state("");
  let re = $state<{ a: string; t: string } | null>(null);
  let busy = $state(false);
  let listEl = $state<HTMLDivElement | null>(null);

  $effect(() => {
    void items.length;
    queueMicrotask(() => { if (listEl) listEl.scrollTop = listEl.scrollHeight; });
  });

  async function send() {
    const v = text.trim();
    if (!v || busy) return;
    busy = true;
    try { await onSend(v, re); text = ""; re = null; }
    finally { busy = false; }
  }
</script>

<aside class="th" class:phone={IS_MOBILE} use:sideResize={"threadpanel"} data-back aria-label={title}>
  <header class="th-head">
    <button data-x data-keep-x class="th-x" onclick={onClose} aria-label={$t(IS_MOBILE ? "Back" : "Close")}>{@html IS_MOBILE ? icons.back : CLOSE}</button>
    <div class="th-title"><b>{title}</b>{#if items.length}<span>{tr("{n} in the thread", { n: items.length })}</span>{/if}</div>
  </header>
  <div class="th-list" bind:this={listEl}>
    <!-- The post itself on top, like Telegram's comments. -->
    <div class="th-root">
      <div class="th-name">{rootAuthor}</div>
      <div class="th-text"><Markup src={rootText.slice(0, 2000)} compact /></div>
      <div class="th-time">{rootTime}</div>
    </div>
    <div class="th-sep"><span>{$t(items.length ? "Discussion started" : "No replies yet — start the discussion")}</span></div>
    {#each items as it (it.id)}
      {@const face = it.as ? it.as.gid : it.author}
      <div class="th-item" class:out={it.outgoing} style:padding-left="{Math.min(it.depth, 3) * 12}px">
        {#if !it.outgoing}<span class="th-av" style:background-image={$avatars[face] ? `url("${$avatars[face]}")` : undefined}>{$avatars[face] ? "" : (it.as?.title ?? it.author).slice(0, 1).toUpperCase()}</span>{/if}
        <div class="th-bubble">
          {#if !it.outgoing}<div class="th-name">{it.as?.title ?? it.author}</div>{/if}
          {#if it.re}<div class="th-re"><b>{it.re.a}</b><span>{it.re.t}</span></div>{/if}
          <div class="th-text"><Markup src={it.text} compact /></div>
          <div class="th-time">{it.time}</div>
        </div>
        <div class="th-acts">
          {#if canWrite}<button onclick={() => (re = { a: it.author, t: it.text.slice(0, 120) })} title={$t("Reply")} aria-label={$t("Reply")}>{@html icons.reply}</button>{/if}
          {#if onJump}<button onclick={() => onJump?.(it.id)} title={$t("Show in chat")} aria-label={$t("Show in chat")}>{@html icons.forward}</button>{/if}
        </div>
      </div>
    {/each}
  </div>
  {#if canWrite}
    <footer class="th-foot">
      {#if re}
        <div class="th-reply"><span class="th-rbar"></span><span class="th-rtx"><b>{re.a}</b>{re.t}</span><button onclick={() => (re = null)} aria-label={$t("Cancel")}>{@html CLOSE}</button></div>
      {/if}
      <div class="th-row">
        {#if asChoices.length && onAs}
          <div class="th-as">
            <button class="th-asb" onclick={() => (asOpen = !asOpen)} title={$t("Send message as…")} aria-label={$t("Send message as…")}
              style:background={picked ? ($avatars[picked.id] ? undefined : picked.gradient) : undefined}
              style:background-image={picked ? ($avatars[picked.id] ? `url("${$avatars[picked.id]}")` : undefined) : ($avatars[me] ? `url("${$avatars[me]}")` : undefined)}>
              {picked ? ($avatars[picked.id] ? "" : picked.title.charAt(0).toUpperCase()) : ($avatars[me] ? "" : me.charAt(0).toUpperCase())}
            </button>
            {#if asOpen}
              <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
              <div class="th-asveil" onclick={() => (asOpen = false)}></div>
              <div class="th-asmenu" role="menu">
                <div class="th-astitle">{$t("Send message as…")}</div>
                <button role="menuitem" class:on={!picked} onclick={() => { onAs(""); asOpen = false; }}><span class="th-asav" style:background-image={$avatars[me] ? `url("${$avatars[me]}")` : undefined}>{$avatars[me] ? "" : me.charAt(0).toUpperCase()}</span><b>{me}</b></button>
                {#each asChoices as c (c.id)}
                  <button role="menuitem" class:on={picked?.id === c.id} onclick={() => { onAs(c.id); asOpen = false; }}><span class="th-asav" style:background={$avatars[c.id] ? undefined : c.gradient} style:background-image={$avatars[c.id] ? `url("${$avatars[c.id]}")` : undefined}>{$avatars[c.id] ? "" : c.title.charAt(0).toUpperCase()}</span><b>{c.title}</b></button>
                {/each}
              </div>
            {/if}
          </div>
        {/if}
        <textarea rows="1" placeholder={$t("Write a comment…")} bind:value={text}
          onkeydown={(e) => { if (e.key === "Enter" && !e.shiftKey && !IS_MOBILE) { e.preventDefault(); void send(); } }}></textarea>
        <button class="th-send" onclick={send} disabled={busy || !text.trim()} aria-label={$t("Send")}>{@html icons.send}</button>
      </div>
    </footer>
  {/if}
</aside>

<style>
  .th { position: absolute; top: 0; right: 0; bottom: 0; width: min(380px, 100%); z-index: 25; display: flex; flex-direction: column; background: var(--bg-shell); border-left: 1px solid var(--border-strong); box-shadow: -16px 0 40px -22px rgba(0,0,0,0.6); animation: th-in 0.2s cubic-bezier(.2,.9,.3,1); }
  /* Phones: a screen of its own over everything (Telegram), not a strip. */
  .th.phone { position: fixed; inset: 0; width: auto; z-index: 300; border: none; box-shadow: none;
    padding: var(--sat, env(safe-area-inset-top, 0px)) 0 var(--sab, env(safe-area-inset-bottom, 0px)); animation: th-in-ph 0.22s cubic-bezier(.2,.9,.3,1); }
  @keyframes th-in { from { transform: translateX(24px); opacity: 0; } }
  @keyframes th-in-ph { from { transform: translateX(40%); opacity: 0; } }
  .th-head { display: flex; align-items: center; gap: 6px; min-height: 56px; padding: 0 12px 0 6px; border-bottom: 1px solid var(--border); background: var(--bg-list); }
  .th-x { width: 44px; height: 44px; flex-shrink: 0; display: grid; place-items: center; border-radius: 50%; border: none; background: transparent; color: var(--fg); cursor: pointer; }
  .th-x :global(svg) { width: 22px; height: 22px; }
  .th-x:hover { background: color-mix(in srgb, var(--fg) 8%, transparent); }
  .th-title { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .th-title b { font-size: 17px; font-weight: 600; }
  .th-title span { font-size: 13px; color: var(--fg-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .th-list { flex: 1; overflow-y: auto; padding: 12px 10px; display: flex; flex-direction: column; gap: 6px; }
  .th-root { align-self: stretch; padding: 10px 12px 6px; border-radius: calc(16px * var(--rk, 1)); background: var(--bg-bubble-in); border: 1px solid var(--border); }
  .th-name { font-size: 13.5px; font-weight: 600; color: var(--accent); margin-bottom: 2px; }
  .th-text { font-size: 15px; line-height: 1.4; color: var(--fg); word-wrap: break-word; }
  .th-time { text-align: right; font-size: 11.5px; color: var(--fg-dim); margin-top: 2px; font-variant-numeric: tabular-nums; }
  .th-sep { align-self: center; margin: 8px 0; padding: 4px 12px; border-radius: 999px; font-size: 12.5px; font-weight: 600; color: var(--fg-muted); background: color-mix(in srgb, var(--bg-surface-2) 85%, transparent); }
  .th-item { display: flex; align-items: flex-end; gap: 8px; animation: th-in 0.18s ease-out; }
  .th-item.out { flex-direction: row-reverse; }
  .th-av { width: 34px; height: 34px; border-radius: 50%; flex-shrink: 0; display: grid; place-items: center; font-size: 14px; font-weight: 700; color: var(--bg-shell); background: linear-gradient(135deg, var(--accent), var(--accent-2)); background-size: cover; background-position: center; }
  .th-bubble { min-width: 64px; max-width: 78%; padding: 7px 11px 5px; border-radius: calc(16px * var(--rk, 1)) calc(16px * var(--rk, 1)) calc(16px * var(--rk, 1)) calc(4px * var(--rk, 1)); background: var(--bg-bubble-in); border: 1px solid var(--border); }
  .th-item.out .th-bubble { border-radius: calc(16px * var(--rk, 1)) calc(16px * var(--rk, 1)) calc(4px * var(--rk, 1)) calc(16px * var(--rk, 1)); background: linear-gradient(135deg, color-mix(in srgb, var(--accent) 16%, transparent), color-mix(in srgb, var(--accent-2) 14%, transparent)), var(--bg-bubble-in);
    border-color: color-mix(in srgb, var(--accent) 22%, transparent); }
  .th-re { display: flex; flex-direction: column; font-size: 13px; border-left: 3px solid var(--accent); padding: 2px 8px; margin: 2px 0 4px; border-radius: calc(4px * var(--rk, 1)); background: color-mix(in srgb, var(--accent) 10%, transparent); }
  .th-re b { color: var(--accent); font-weight: 600; }
  .th-re span { color: var(--fg-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .th-acts { display: flex; flex-direction: column; gap: 2px; align-self: center; opacity: .55; }
  .th-acts button { width: 30px; height: 30px; display: grid; place-items: center; border: none; border-radius: 50%; background: transparent; color: var(--fg-muted); cursor: pointer; }
  .th-acts button :global(svg) { width: 18px; height: 18px; }
  .th-acts button:hover { opacity: 1; color: var(--accent); background: color-mix(in srgb, var(--fg) 8%, transparent); }
  .th-foot { padding: 8px 10px 10px; display: flex; flex-direction: column; gap: 6px; background: var(--bg-list); border-top: 1px solid var(--border); }
  .th-reply { display: flex; align-items: center; gap: 10px; font-size: 13px; padding: 0 4px; }
  .th-rbar { width: 3px; align-self: stretch; border-radius: 2px; background: var(--accent); }
  .th-rtx { flex: 1; min-width: 0; display: flex; flex-direction: column; overflow: hidden; }
  .th-rtx b { color: var(--accent); }
  .th-reply button { width: 30px; height: 30px; display: grid; place-items: center; border: none; background: none; color: var(--fg-muted); cursor: pointer; }
  .th-reply button :global(svg) { width: 16px; height: 16px; }
  .th-as { position: relative; flex-shrink: 0; }
  .th-asb { width: 44px; height: 44px; border-radius: 50%; border: 2px solid var(--accent); display: grid; place-items: center; font-weight: 800; color: var(--bg-shell); background: var(--accent-soft) center / cover no-repeat; cursor: pointer; }
  .th-asveil { position: fixed; inset: 0; z-index: 30; }
  .th-asmenu { position: absolute; left: 0; bottom: calc(100% + 10px); z-index: 31; width: min(300px, calc(100vw - 24px)); padding: 6px 0; display: flex; flex-direction: column;
    background: var(--bg-surface-2); border: 1px solid var(--border); border-radius: calc(18px * var(--rk, 1)); box-shadow: 0 14px 36px -12px rgba(0,0,0,.55); }
  .th-astitle { padding: 8px 18px 6px; font-size: 15px; font-weight: 600; color: var(--accent); }
  .th-asmenu button { display: flex; align-items: center; gap: 14px; min-height: 54px; padding: 6px 16px; border: none; background: transparent; color: var(--fg); font: inherit; font-size: 15.5px; text-align: left; cursor: pointer; }
  .th-asmenu button b { font-weight: 500; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .th-asmenu button.on .th-asav { box-shadow: 0 0 0 2px var(--bg-surface-2), 0 0 0 4px var(--accent); }
  .th-asav { width: 40px; height: 40px; border-radius: 50%; flex-shrink: 0; display: grid; place-items: center; font-weight: 700; color: var(--bg-shell); background: var(--accent-soft) center / cover no-repeat; }
  .th-row { display: flex; gap: 8px; align-items: flex-end; }
  .th-row textarea { flex: 1; min-width: 0; resize: none; max-height: 140px; min-height: 44px; box-sizing: border-box; padding: 11px 16px; border-radius: calc(22px * var(--rk, 1)); border: 1px solid var(--border); background: var(--bg-input); color: var(--fg); font: inherit; font-size: 15px; outline: none; }
  .th-row textarea:focus { border-color: var(--accent); }
  .th-send { width: 44px; height: 44px; border-radius: 50%; border: none; background: var(--accent); color: var(--bg-shell); display: grid; place-items: center; cursor: pointer; flex-shrink: 0; transition: transform .15s, opacity .15s; }
  .th-send :global(svg) { width: 20px; height: 20px; }
  .th-send:disabled { opacity: 0.45; transform: scale(.92); }
</style>
