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
  }
</script>

<script lang="ts">
  /** A message with everything said about it: replies (groups, chats) or
   *  comments (channel posts) — with answers to answers. */
  import { t, tr } from "../i18n";
  import { avatars } from "../people";
  import Markup from "./Markup.svelte";

  let { title, rootAuthor, rootText, rootTime, items, canWrite = true, onSend, onClose, onJump }: {
    title: string;
    rootAuthor: string;
    rootText: string;
    rootTime: string;
    items: ThreadItem[];
    canWrite?: boolean;
    onSend: (text: string, re: { a: string; t: string } | null) => Promise<void>;
    onClose: () => void;
    onJump?: (id: string) => void;
  } = $props();

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

<aside class="th" aria-label={title}>
  <header class="th-head">
    <b>{title}</b>
    <span class="th-n">{items.length}</span>
    <button class="th-x" onclick={onClose} aria-label={$t("Close")}>✕</button>
  </header>
  <div class="th-list" bind:this={listEl}>
    <div class="th-root">
      <div class="th-meta"><b>{rootAuthor}</b><span>{rootTime}</span></div>
      <div class="th-text"><Markup src={rootText.slice(0, 2000)} compact /></div>
    </div>
    <div class="th-sep"><span>{items.length ? tr("{n} in the thread", { n: items.length }) : $t("No replies yet — start the discussion")}</span></div>
    {#each items as it (it.id)}
      <div class="th-item" class:out={it.outgoing} style:margin-left="{Math.min(it.depth, 3) * 14}px">
        <span class="th-av" style:background-image={$avatars[it.author] ? `url("${$avatars[it.author]}")` : undefined}>{$avatars[it.author] ? "" : it.author.slice(0, 1).toUpperCase()}</span>
        <div class="th-body">
          <div class="th-meta"><b>{it.outgoing ? $t("You") : it.author}</b><span>{it.time}</span></div>
          {#if it.re}<div class="th-re"><b>{it.re.a}</b> {it.re.t}</div>{/if}
          <div class="th-text"><Markup src={it.text} compact /></div>
          <div class="th-acts">
            {#if canWrite}<button onclick={() => (re = { a: it.author, t: it.text.slice(0, 120) })}>{$t("Reply")}</button>{/if}
            {#if onJump}<button onclick={() => onJump?.(it.id)}>{$t("Show in chat")}</button>{/if}
          </div>
        </div>
      </div>
    {/each}
  </div>
  {#if canWrite}
    <footer class="th-foot">
      {#if re}
        <div class="th-reply"><span>↩ <b>{re.a}</b> {re.t}</span><button onclick={() => (re = null)} aria-label={$t("Cancel")}>✕</button></div>
      {/if}
      <div class="th-row">
        <textarea rows="1" placeholder={$t("Write a comment…")} bind:value={text}
          onkeydown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); void send(); } }}></textarea>
        <button class="th-send" onclick={send} disabled={busy || !text.trim()} aria-label={$t("Send")}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><line x1="22" y1="2" x2="11" y2="13"/><polygon points="22 2 15 22 11 13 2 9 22 2"/></svg>
        </button>
      </div>
    </footer>
  {/if}
</aside>

<style>
  .th { position: absolute; top: 0; right: 0; bottom: 0; width: min(380px, 100%); z-index: 25; display: flex; flex-direction: column; background: var(--bg-shell); border-left: 1px solid var(--border-strong); box-shadow: -16px 0 40px -22px rgba(0,0,0,0.6); animation: th-in 0.2s cubic-bezier(.2,.9,.3,1); }
  @keyframes th-in { from { transform: translateX(24px); opacity: 0; } }
  .th-head { display: flex; align-items: center; gap: 8px; padding: 14px 12px 12px 16px; border-bottom: 1px solid var(--border); }
  .th-head b { font-size: 15px; }
  .th-n { flex: 1; font-size: 12px; color: var(--fg-dim); }
  .th-x { width: 28px; height: 28px; border-radius: 50%; border: none; background: var(--bg-hover); color: var(--fg-muted); cursor: pointer; }
  .th-list { flex: 1; overflow-y: auto; padding: 12px 14px; display: flex; flex-direction: column; gap: 10px; }
  .th-root { padding: 12px; border-radius: calc(12px * var(--rk, 1)); background: var(--bg-input); border-left: 3px solid var(--accent); }
  .th-meta { display: flex; align-items: baseline; gap: 8px; font-size: 12px; }
  .th-meta b { color: var(--accent); font-size: 12.5px; }
  .th-meta span { color: var(--fg-dim); font-size: 11px; }
  .th-text { font-size: 13.5px; line-height: 1.45; color: var(--fg); word-wrap: break-word; }
  .th-sep { display: flex; align-items: center; gap: 10px; font-size: 11px; color: var(--fg-dim); text-transform: uppercase; letter-spacing: 0.04em; font-weight: 700; }
  .th-sep::before, .th-sep::after { content: ""; flex: 1; height: 1px; background: var(--border); }
  .th-item { display: flex; gap: 9px; animation: th-in 0.18s ease-out; }
  .th-av { width: 28px; height: 28px; border-radius: 50%; flex-shrink: 0; display: grid; place-items: center; font-size: 12px; font-weight: 800; color: #06070C; background: linear-gradient(135deg, var(--accent), var(--accent-2)); background-size: cover; background-position: center; }
  .th-body { min-width: 0; flex: 1; padding: 8px 10px; border-radius: 4px 12px 12px 12px; background: var(--bg-bubble-in); border: 1px solid var(--border); }
  .th-item.out .th-body { border-color: color-mix(in srgb, var(--accent) 25%, transparent); }
  .th-re { font-size: 12px; color: var(--fg-muted); border-left: 2px solid var(--accent); padding-left: 6px; margin: 3px 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .th-re b { color: var(--accent); }
  .th-acts { display: flex; gap: 10px; margin-top: 4px; }
  .th-acts button { border: none; background: none; padding: 0; color: var(--fg-dim); font: inherit; font-size: 11.5px; font-weight: 600; cursor: pointer; }
  .th-acts button:hover { color: var(--accent); }
  .th-foot { padding: 10px 12px; border-top: 1px solid var(--border); display: flex; flex-direction: column; gap: 6px; }
  .th-reply { display: flex; align-items: center; gap: 8px; font-size: 12px; color: var(--fg-muted); }
  .th-reply span { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .th-reply b { color: var(--accent); }
  .th-reply button { border: none; background: none; color: var(--fg-dim); cursor: pointer; }
  .th-row { display: flex; gap: 8px; align-items: flex-end; }
  .th-row textarea { flex: 1; min-width: 0; resize: none; max-height: 120px; padding: 9px 12px; border-radius: calc(12px * var(--rk, 1)); border: 1px solid var(--border); background: var(--bg-input); color: var(--fg); font: inherit; font-size: 13.5px; outline: none; }
  .th-row textarea:focus { border-color: var(--accent); }
  .th-send { width: 38px; height: 38px; border-radius: 50%; border: none; background: var(--accent); color: #06070C; display: grid; place-items: center; cursor: pointer; flex-shrink: 0; }
  .th-send svg { width: 16px; height: 16px; }
  .th-send:disabled { opacity: 0.5; }
</style>
