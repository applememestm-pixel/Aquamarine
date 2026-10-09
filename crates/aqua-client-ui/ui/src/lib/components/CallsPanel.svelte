<script lang="ts">
  import { t } from "../i18n";
  import { callHistory, clearCallHistory, type CallRecord } from "../callHistory";
  import { chats, activeId, activeTab, selectChat } from "../stores";
  import { displayName } from "../contacts";
  import { icons } from "../icons";

  function fmtDuration(secs: number): string {
    if (secs <= 0) return "—";
    const m = Math.floor(secs / 60);
    const s = secs % 60;
    return `${m}:${String(s).padStart(2, "0")}`;
  }

  function fmtWhen(ts: number): string {
    const d = new Date(ts);
    const today = new Date();
    const sameDay = d.toDateString() === today.toDateString();
    if (sameDay) {
      return d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
    }
    const yd = new Date(today.getTime() - 86400000);
    if (d.toDateString() === yd.toDateString()) return "yesterday";
    return d.toLocaleDateString(undefined, { day: "numeric", month: "short" });
  }

  function statusLabel(c: CallRecord): string {
    switch (c.status) {
      case "completed": return c.direction === "in" ? "Incoming" : "Outgoing";
      case "missed":    return "Missed";
      case "rejected":  return "Declined";
      case "failed":    return "Failed";
    }
  }

  function openPeer(peerId: string) {
    activeTab.set("chats");
    void selectChat(peerId);
  }

  // Sorted newest first.
  let sorted = $derived([...$callHistory].sort((a, b) => b.started_at - a.started_at));

  // Enriched with display names.
  function nameFor(peerId: string): string {
    const c = $chats.find((x) => x.id === peerId);
    const fallback = c?.name ?? peerId;
    return displayName(peerId, fallback);
  }
</script>

<aside class="panel">
  <div class="head">
    <h2>{$t("Calls")}</h2>
    {#if sorted.length > 0}
      <button class="clear" onclick={() => confirm("Clear all call history?") && clearCallHistory()}>
        {$t("Clear history")}
      </button>
    {/if}
  </div>

  <div class="list">
    {#if sorted.length === 0}
      <div class="empty">
        <div class="empty-icon">{@html icons.phone}</div>
        <div class="empty-title">{$t("No calls yet")}</div>
        <div class="empty-sub">{$t("Voice calls will show up here. Click the phone icon in any chat to start one.")}</div>
      </div>
    {:else}
      {#each sorted as c (c.id)}
        <button class="row" onclick={() => openPeer(c.peer_id)}>
          <div class="icon" class:miss={c.status === "missed"} class:ok={c.status === "completed"}>
            {@html icons.phone}
            {#if c.direction === "in"}
              <span class="dir in" title={$t("Incoming")}>↙</span>
            {:else}
              <span class="dir out" title={$t("Outgoing")}>↗</span>
            {/if}
          </div>
          <div class="meta">
            <div class="r1">
              <span class="name">{nameFor(c.peer_id)}</span>
              <span class="when">{fmtWhen(c.started_at)}</span>
            </div>
            <div class="r2">
              <span class="status" class:miss={c.status === "missed"}>{statusLabel(c)}</span>
              <span class="dot">·</span>
              <span class="dur">{fmtDuration(c.duration_seconds)}</span>
            </div>
          </div>
        </button>
      {/each}
    {/if}
  </div>
</aside>

<style>
  .panel {
    background: var(--bg-list);
    border-right: 1px solid var(--border);
    display: flex; flex-direction: column;
    min-width: 0;
  }
  .head {
    padding: 14px 18px 10px;
    display: flex; align-items: center; justify-content: space-between;
    border-bottom: 1px solid var(--border);
  }
  .head h2 { margin: 0; font-size: 15.5px; font-weight: 700; letter-spacing: -0.01em; }
  .clear {
    padding: 5px 10px;
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 8px;
    color: var(--fg-dim);
    font-size: 11.5px;
    cursor: pointer;
  }
  .clear:hover { color: var(--danger); border-color: var(--danger); }

  .list { flex: 1; overflow-y: auto; padding: 6px; }
  .empty {
    padding: 60px 22px;
    text-align: center;
    color: var(--fg-dim);
  }
  .empty-icon {
    display: inline-grid; place-items: center;
    width: 56px; height: 56px;
    border-radius: 50%;
    background: var(--accent-soft);
    color: var(--accent);
    margin-bottom: 14px;
  }
  .empty-icon :global(svg) { width: 24px; height: 24px; }
  .empty-title { font-size: 14px; font-weight: 700; color: var(--fg); margin-bottom: 4px; }
  .empty-sub { font-size: 12.5px; line-height: 1.5; }

  .row {
    width: 100%;
    display: grid;
    grid-template-columns: 42px 1fr;
    gap: 12px;
    align-items: center;
    padding: 10px 10px;
    background: transparent;
    border: none;
    border-radius: 10px;
    text-align: left;
    color: inherit;
    cursor: pointer;
    transition: background 0.1s ease;
  }
  .row:hover { background: var(--bg-hover); }
  .icon {
    position: relative;
    width: 42px; height: 42px;
    border-radius: 12px;
    display: grid; place-items: center;
    background: var(--bg-input);
    color: var(--fg-muted);
    border: 1px solid var(--border);
  }
  .icon.ok { color: var(--accent); border-color: var(--accent-soft); }
  .icon.miss { color: var(--danger); border-color: rgba(255, 77, 109, 0.3); }
  .icon :global(svg) { width: 16px; height: 16px; }
  .dir {
    position: absolute;
    bottom: -4px; right: -4px;
    width: 18px; height: 18px;
    border-radius: 50%;
    background: var(--bg-list);
    display: grid; place-items: center;
    font-size: 11px;
    font-weight: 800;
    color: var(--fg-muted);
    border: 1px solid var(--border);
  }
  .dir.in  { color: var(--online); }
  .dir.out { color: var(--accent); }

  .meta { min-width: 0; }
  .r1 { display: flex; justify-content: space-between; align-items: baseline; gap: 10px; }
  .name {
    font-weight: 600;
    font-size: 14px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .when { font-size: 11.5px; color: var(--fg-dim); font-variant-numeric: tabular-nums; flex-shrink: 0; }
  .r2 {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--fg-muted);
    margin-top: 2px;
  }
  .status.miss { color: var(--danger); }
  .dot { color: var(--fg-dim); }
  .dur { font-variant-numeric: tabular-nums; }
</style>
