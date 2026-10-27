<script module lang="ts">
  import { writable, get as getStore } from "svelte/store";

  /** Who wrote the original (shown as "Forwarded from …"). */
  export interface ForwardAuthor {
    /** Username — omitted when the author doesn't allow linking to them. */
    from?: string;
    name: string;
    /** Original time ("12:30"). */
    at?: string;
    /** Group / channel it came from. */
    chat?: string;
  }

  /** One forwardable payload, extracted from a chat message. */
  export type ForwardItem =
    | { kind: "text"; text: string; author?: ForwardAuthor }
    | { kind: "file"; path: string; name: string; author?: ForwardAuthor };

  interface ForwardState {
    open: boolean;
    items: ForwardItem[];
  }
  export const forwardState = writable<ForwardState>({ open: false, items: [] });

  /** Open the forward picker for a set of messages. */
  export function openForward(items: ForwardItem[]) {
    if (items.length === 0) return;
    forwardState.set({ open: true, items });
  }
  export function closeForward() {
    forwardState.set({ open: false, items: [] });
  }

  /** The `[fwd:v1]` header for an author (null = no header). */
  export function fwdHeader(a: ForwardAuthor | undefined): string | null {
    if (!a?.name) return null;
    const h: Record<string, string> = { name: a.name.slice(0, 80) };
    if (a.from) h.from = a.from;
    if (a.at) h.at = a.at;
    if (a.chat) h.chat = a.chat.slice(0, 80);
    return JSON.stringify(h);
  }
</script>

<script lang="ts">
  import { t, tr } from "../i18n";
  import { onMount } from "svelte";
  import { chats, activeId, loadActiveMessages } from "../stores";
  import { api } from "../tauri";
  import { showToast } from "./Toast.svelte";
  import { avatars } from "../people";

  const firstLetter = (s: string) => (s.trim()[0] ?? "?").toUpperCase();

  let query = $state("");
  let busy = $state(false);
  /** Telegram's forward options. */
  let showNames = $state(true);
  let comment = $state("");
  let picked = $state<string[]>([]);

  let items = $derived($forwardState.items);
  let authors = $derived([...new Set(items.map((i) => i.author?.name).filter(Boolean))] as string[]);

  $effect(() => { if ($forwardState.open) { showNames = true; comment = ""; picked = []; query = ""; } });

  // Only live chats can receive a forward (and Saved Messages).
  let targets = $derived(
    $chats
      .filter((c) => c.live || c.id === "__saved__")
      .filter((c) => c.name.toLowerCase().includes(query.trim().toLowerCase())),
  );

  async function sendText(peerId: string, text: string) {
    if (peerId.startsWith("grp_")) await api.groupSend(peerId, text);
    else await api.sendMessage(peerId, text);
  }

  async function forwardTo(peerIds: string[]) {
    if (busy || !peerIds.length) return;
    busy = true;
    const list = getStore(forwardState).items;
    let ok = 0;
    let skipped = 0;
    try {
      for (const peerId of peerIds) {
        if (comment.trim()) { try { await sendText(peerId, comment.trim()); } catch { /* ignore */ } }
        for (const it of list) {
          // Bots get plain text (Telegram's forward_origin isn't mapped).
          const bot = $chats.find((c) => c.id === peerId && c.live && !c.has_lock && !c.id.startsWith("grp_"));
          const h = showNames && !bot ? fwdHeader(it.author) : null;
          try {
            if (it.kind === "text") {
              await sendText(peerId, h ? `[fwd:v1]${h}\n${it.text}` : it.text);
            } else if (h) {
              await api.forwardFile(peerId, it.path, h);
            } else {
              await api.attachFile(peerId, it.path);
            }
            ok++;
          } catch {
            skipped++;
          }
        }
      }
      if (peerIds.includes(getStore(activeId) ?? "")) await loadActiveMessages();
      chats.set(await api.listChats());
      closeForward();
      const names = peerIds.map((id) => $chats.find((c) => c.id === id)?.name ?? id).join(", ");
      let msg = tr("Forwarded to {to}", { to: names });
      if (skipped) msg += " " + tr("({n} skipped — not downloaded)", { n: skipped });
      showToast(msg);
    } finally {
      busy = false;
    }
  }

  function toggle(id: string) {
    picked = picked.includes(id) ? picked.filter((x) => x !== id) : [...picked, id];
  }

  function onEsc(e: KeyboardEvent) { if (e.key === "Escape" && getStore(forwardState).open) closeForward(); }
  onMount(() => {
    window.addEventListener("keydown", onEsc);
    return () => window.removeEventListener("keydown", onEsc);
  });
</script>

{#if $forwardState.open}
  <div
    class="fw-backdrop"
    role="button"
    tabindex="-1"
    aria-label={$t("Close")}
    onclick={(e) => { if ((e.target as HTMLElement).classList.contains("fw-backdrop")) closeForward(); }}
    onkeydown={(e) => { if (e.key === "Escape") closeForward(); }}
  >
    <div class="fw-sheet" role="dialog" aria-modal="true" tabindex="-1">
      <div class="fw-head">
        <h3>{$t("Forward to…")}</h3>
        <span class="fw-count">{tr("{n} selected", { n: items.length })}</span>
        <button class="fw-close" onclick={closeForward} title={$t("Close (Esc)")}>×</button>
      </div>

      <!-- Preview of what goes out, with the author in the corner (Telegram). -->
      <div class="fw-prev">
        {#if showNames && items[0]?.author}
          {@const a = items[0].author}
          <span class="fw-pav" style:background-image={a.from && $avatars[a.from] ? `url("${$avatars[a.from]}")` : undefined}>{a.from && $avatars[a.from] ? "" : firstLetter(a.name)}</span>
          <span class="fw-ptext">
            <b>{authors.length > 1 ? tr("From {a} and others", { a: authors[0] }) : tr("Forwarded from {a}", { a: a.name })}</b>
            <span>{items[0].kind === "text" ? items[0].text.replace(/^\[\w+:v1\]/, "").slice(0, 90) : `📎 ${items[0].name}`}{items.length > 1 ? ` · +${items.length - 1}` : ""}</span>
          </span>
        {:else}
          <span class="fw-pav anon">↪</span>
          <span class="fw-ptext">
            <b>{$t("Sent as your own message")}</b>
            <span>{items[0]?.kind === "text" ? items[0].text.slice(0, 90) : `📎 ${items[0]?.name ?? ""}`}</span>
          </span>
        {/if}
      </div>
      <div class="fw-opts">
        <button class="fw-seg" class:on={showNames} onclick={() => (showNames = true)}>{$t("Show sender's name")}</button>
        <button class="fw-seg" class:on={!showNames} onclick={() => (showNames = false)}>{$t("Hide sender's name")}</button>
      </div>

      <input class="fw-search" placeholder={$t("Search chats")} bind:value={query} />
      <div class="fw-list">
        {#if targets.length === 0}
          <div class="fw-empty">{$t("No chats to forward to.")}</div>
        {:else}
          {#each targets as c (c.id)}
            <button class="fw-row" class:on={picked.includes(c.id)} disabled={busy}
              onclick={(e) => { if (e.ctrlKey || e.metaKey || picked.length) toggle(c.id); else void forwardTo([c.id]); }}>
              <span class="fw-av" style="--gradient: {c.gradient}">{firstLetter(c.name)}
                {#if $avatars[c.id]}<img src={$avatars[c.id]} alt="" />{/if}
              </span>
              <span class="fw-name">{c.name}</span>
              <span class="fw-check" class:on={picked.includes(c.id)} role="checkbox" aria-checked={picked.includes(c.id)} tabindex="-1"
                onclick={(e) => { e.stopPropagation(); toggle(c.id); }} onkeydown={() => {}}></span>
            </button>
          {/each}
        {/if}
      </div>
      <div class="fw-foot">
        <input class="fw-comment" placeholder={$t("Add a comment…")} bind:value={comment}
          onkeydown={(e) => { if (e.key === "Enter" && picked.length) void forwardTo(picked); }} />
        {#if picked.length}
          <button class="fw-send" disabled={busy} onclick={() => forwardTo(picked)}>{tr("Send to {n}", { n: picked.length })}</button>
        {/if}
      </div>
      <div class="fw-hint">{$t("Click a chat to send. Tick several (or Ctrl+click) to send to all of them at once.")}</div>
    </div>
  </div>
{/if}

<style>
  .fw-backdrop {
    position: fixed; inset: 0;
    background: rgba(6, 7, 12, 0.55);
    display: grid; place-items: center;
    z-index: 115;
    animation: fw-fade 0.15s ease-out;
  }
  @keyframes fw-fade { from { opacity: 0; } to { opacity: 1; } }
  @keyframes fw-pop { from { opacity: 0; transform: translateY(10px) scale(0.98); } }
  .fw-sheet {
    width: min(440px, 94vw);
    max-height: 80vh;
    display: flex; flex-direction: column;
    background: var(--bg-shell);
    border: 1px solid var(--border-strong);
    border-radius: calc(16px * var(--rk, 1));
    overflow: hidden;
    animation: fw-pop 0.18s cubic-bezier(.2,.9,.3,1.1);
  }
  .fw-head { display: flex; align-items: center; gap: 10px; padding: 14px 16px; border-bottom: 1px solid var(--border); }
  .fw-head h3 { margin: 0; font-size: 14.5px; font-weight: 700; flex: 1; }
  .fw-count { font-size: 11.5px; color: var(--fg-dim); }
  .fw-close { width: 28px; height: 28px; background: transparent; border: 1px solid var(--border); border-radius: calc(8px * var(--rk, 1)); color: var(--fg-dim); font-size: 17px; cursor: pointer; }
  .fw-close:hover { color: var(--fg); background: var(--bg-hover); }
  .fw-prev { display: flex; align-items: center; gap: 10px; margin: 12px 14px 0; padding: 10px 12px; border-radius: calc(12px * var(--rk, 1)); background: var(--bg-input); border-left: 3px solid var(--accent); }
  .fw-pav { width: 34px; height: 34px; border-radius: 50%; flex-shrink: 0; display: grid; place-items: center; font-weight: 800; font-size: 14px; color: #06070C; background: linear-gradient(135deg, var(--accent), var(--accent-2)); background-size: cover; background-position: center; }
  .fw-pav.anon { background: var(--bg-hover); color: var(--fg-muted); }
  .fw-ptext { min-width: 0; display: flex; flex-direction: column; gap: 2px; }
  .fw-ptext b { font-size: 12.5px; color: var(--accent); }
  .fw-ptext span { font-size: 12.5px; color: var(--fg-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .fw-opts { display: flex; gap: 4px; margin: 10px 14px 0; padding: 3px; border-radius: calc(10px * var(--rk, 1)); background: var(--bg-input); }
  .fw-seg { flex: 1; border: none; background: none; color: var(--fg-muted); font: inherit; font-size: 12.5px; font-weight: 600; padding: 7px; border-radius: calc(8px * var(--rk, 1)); cursor: pointer; }
  .fw-seg.on { background: var(--bg-shell); color: var(--accent); box-shadow: 0 0 0 1px var(--border-strong); }
  .fw-search { margin: 10px 14px 6px; padding: 8px 11px; background: var(--bg-input); border: 1px solid var(--border); border-radius: calc(9px * var(--rk, 1)); color: var(--fg); font-family: inherit; font-size: 13px; outline: none; }
  .fw-search:focus { border-color: var(--accent); }
  .fw-list { overflow-y: auto; padding: 4px 8px 6px; min-height: 120px; }
  .fw-empty { padding: 24px; text-align: center; color: var(--fg-dim); font-size: 13px; }
  .fw-row { display: flex; align-items: center; gap: 11px; width: 100%; padding: 7px 10px; background: transparent; border: none; border-radius: calc(10px * var(--rk, 1)); cursor: pointer; text-align: left; color: var(--fg); }
  .fw-row:hover:not(:disabled) { background: var(--bg-hover); }
  .fw-row.on { background: var(--accent-soft); }
  .fw-row:disabled { opacity: 0.5; cursor: default; }
  .fw-av { position: relative; overflow: hidden; width: 36px; height: 36px; border-radius: 50%; display: grid; place-items: center; font-weight: 700; font-size: 14px; color: rgba(6,7,12,0.85); background: var(--gradient, linear-gradient(135deg, #666, #999)); flex-shrink: 0; }
  .fw-av img { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: cover; }
  .fw-name { flex: 1; font-size: 13.5px; font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .fw-check { width: 20px; height: 20px; border-radius: 50%; border: 2px solid var(--border-strong); flex-shrink: 0; position: relative; transition: background 0.12s, border-color 0.12s; }
  .fw-check.on { background: var(--accent); border-color: var(--accent); }
  .fw-check.on::after { content: ""; position: absolute; left: 5px; top: 1px; width: 5px; height: 10px; border: solid #06070C; border-width: 0 2px 2px 0; transform: rotate(45deg); }
  .fw-foot { display: flex; gap: 8px; padding: 8px 14px 4px; border-top: 1px solid var(--border); }
  .fw-comment { flex: 1; padding: 9px 11px; background: var(--bg-input); border: 1px solid var(--border); border-radius: calc(9px * var(--rk, 1)); color: var(--fg); font: inherit; font-size: 13px; outline: none; }
  .fw-comment:focus { border-color: var(--accent); }
  .fw-send { border: none; border-radius: calc(9px * var(--rk, 1)); padding: 0 14px; background: var(--accent); color: #06070C; font: inherit; font-weight: 700; font-size: 13px; cursor: pointer; }
  .fw-hint { padding: 2px 14px 10px; font-size: 11px; color: var(--fg-dim); }
</style>
