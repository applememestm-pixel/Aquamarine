<script module lang="ts">
  import { writable } from "svelte/store";
  import type { GroupKind } from "../types";

  interface WizState { open: boolean; kind: GroupKind; }
  export const newGroupState = writable<WizState>({ open: false, kind: "group" });

  /** Open the window for one kind (group, server or channel). */
  export function openNewGroup(kind: GroupKind = "group") {
    newGroupState.set({ open: true, kind });
  }
  export function closeNewGroup() {
    newGroupState.update((s) => ({ ...s, open: false }));
  }
</script>

<script lang="ts">
  import { t, tr } from "../i18n";
  import { chats, selectChat } from "../stores";
  import { refreshGroups } from "../groups";
  import { DEFAULT_ROOMS } from "../groupRules";
  import { api } from "../tauri";
  import { showToast } from "./Toast.svelte";

  let kind = $state<GroupKind>("group");
  let title = $state("");
  let description = $state("");
  let picked = $state<Set<string>>(new Set());
  let manual = $state("");
  let busy = $state(false);

  // Seed the kind each time the wizard opens; reset the form.
  let wasOpen = false;
  $effect(() => {
    const s = $newGroupState;
    if (s.open && !wasOpen) {
      kind = s.kind;
      title = ""; description = ""; manual = "";
      picked = new Set();
    }
    wasOpen = s.open;
  });

  // Candidate members = real 1:1 peers (exclude groups, saved, welcome, bots).
  let candidates = $derived(
    $chats.filter(
      (c) =>
        !c.id.startsWith("grp_") &&
        c.id !== "__saved__" &&
        c.id !== "welcome" &&
        c.has_lock,
    ),
  );

  function toggle(id: string) {
    const next = new Set(picked);
    if (next.has(id)) next.delete(id); else next.add(id);
    picked = next;
  }

  async function create() {
    const t = title.trim();
    if (!t) { showToast(tr("Give it a name first.")); return; }
    const members = [...picked];
    for (const u of manual.split(/[,\s]+/)) {
      const v = u.trim().toLowerCase();
      if (v && !members.includes(v)) members.push(v);
    }
    busy = true;
    try {
      let g = await api.createGroup(kind, t, description.trim(), members);
      if (kind === "supergroup") {
        // A server: starts with Discord-style rooms.
        const next = JSON.parse(JSON.stringify(g)) as typeof g;
        next.settings.rooms_enabled = true;
        next.rooms = DEFAULT_ROOMS.map((r) => ({ ...r }));
        try { g = await api.updateGroup(next); } catch { /* rooms can be turned on later */ }
      }
      await refreshGroups();
      closeNewGroup();
      await selectChat(g.id);
      showToast(tr(kind === "channel" ? "Channel “{name}” created" : kind === "supergroup" ? "Server “{name}” created" : "Group “{name}” created", { name: t }));
    } catch (e) {
      showToast(tr("Could not create: {e}", { e: String(e) }), 5000);
    } finally {
      busy = false;
    }
  }
</script>

{#if $newGroupState.open}
  <div class="backdrop" role="button" tabindex="-1"
       onclick={(e) => { if ((e.target as HTMLElement).classList.contains('backdrop')) closeNewGroup(); }}
       onkeydown={(e) => { if (e.key === 'Escape') closeNewGroup(); }}>
    <div class="sheet" role="dialog" aria-modal="true">
      <!-- One window per kind (the pencil menu picks it): its own title,
           what it is, and its own button — no switch between them here. -->
      <div class="head">
        <span class="k-ic {kind}">
          {#if kind === "channel"}<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 10v4a1 1 0 0 0 1 1h3l6 4V5L7 9H4a1 1 0 0 0-1 1z"/><path d="M17 8.5a5 5 0 0 1 0 7"/></svg>
          {:else if kind === "supergroup"}<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="3"/><path d="M8 9h8M8 13h5"/></svg>
          {:else}<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="9" cy="8" r="3.2"/><path d="M3 19c.6-3.2 3-5 6-5s5.4 1.8 6 5"/><path d="M16 5.2a3 3 0 0 1 0 5.6M18 14.2c1.6.6 2.7 2.2 3 4.8"/></svg>{/if}
        </span>
        <h2>{$t(kind === "channel" ? "New channel" : kind === "supergroup" ? "New server" : "New group")}</h2>
        <button data-x class="x" onclick={closeNewGroup} aria-label={$t("Close")}>×</button>
      </div>
      <p class="hint">
        {$t(kind === "channel" ? "Broadcast to subscribers — only admins post. Everyone else reads."
          : kind === "supergroup" ? "A server: rooms like Discord channels (#general, announcements, voice), public link, server tag and full admin tools."
          : "A private group chat. Everyone can post by default.")}
      </p>

      <label class="field"><span>{$t("Name")}</span>
        <input bind:value={title} placeholder={$t(kind === "channel" ? "My Channel" : kind === "supergroup" ? "My server" : "My Group")} maxlength="64" />
      </label>
      <label class="field"><span>{$t("Description")}</span>
        <textarea bind:value={description} rows="2" placeholder={$t("What's this about? (optional)")}></textarea>
      </label>

      <div class="field">
        <span>{$t(kind === "channel" ? "Subscribers" : "Members")}</span>
        {#if candidates.length === 0}
          <div class="empty">{$t("No contacts yet — add usernames below.")}</div>
        {:else}
          <div class="members">
            {#each candidates as c (c.id)}
              <button class="member" class:on={picked.has(c.id)} onclick={() => toggle(c.id)}>
                <span class="m-ava" style="background: {c.gradient}">{c.letter}</span>
                <span class="m-name">{c.name}</span>
                <span class="m-check">{picked.has(c.id) ? "✓" : ""}</span>
              </button>
            {/each}
          </div>
        {/if}
        <input class="manual" bind:value={manual} placeholder={$t("Add by username: ivan, maria …")} />
      </div>

      <div class="actions">
        <button class="ghost" onclick={closeNewGroup}>{$t("Cancel")}</button>
        <button class="primary" onclick={create} disabled={busy}>
          {$t(busy ? "Creating…" : kind === "channel" ? "Create channel" : kind === "supergroup" ? "Create server" : "Create group")}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop { position: fixed; inset: 0; background: rgba(6,7,12,0.6); backdrop-filter: blur(4px);
              display: grid; place-items: center; z-index: 130; padding: 20px; }
  .sheet { width: min(460px, 94vw); max-height: 88vh; overflow-y: auto; background: var(--bg-shell);
           border: 1px solid var(--border-strong); border-radius: calc(18px * var(--rk, 1)); padding: 20px;
           box-shadow: 0 40px 90px -24px rgba(0,0,0,0.7);  animation: aq-pop .3s var(--ease-spring, ease-out); }
  .head { display: flex; align-items: center; justify-content: space-between; margin-bottom: 14px; }
  .head h2 { margin: 0; font-size: 18px; font-weight: 800; }
  .x { width: 30px; height: 30px; border-radius: calc(8px * var(--rk, 1)); border: 1px solid var(--border);
       background: transparent; color: var(--fg-dim); font-size: 18px; cursor: pointer; }
  .x:hover { color: var(--fg); background: var(--bg-hover); }

  .k-ic { width: 38px; height: 38px; flex-shrink: 0; border-radius: calc(12px * var(--rk, 1)); display: grid; place-items: center;
          background: var(--accent-soft); color: var(--accent); margin-right: 12px; }
  .k-ic svg { width: 20px; height: 20px; }
  .head h2 { flex: 1; }
  .hint { font-size: 12.5px; color: var(--fg-dim); margin: -4px 2px 16px; line-height: 1.5; }

  .field { display: flex; flex-direction: column; gap: 5px; margin-bottom: 14px; }
  .field > span { font-size: 11px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.06em; color: var(--fg-dim); }
  .field input, .field textarea { background: var(--bg-input); border: 1px solid var(--border);
    border-radius: calc(10px * var(--rk, 1)); color: var(--fg); font: inherit; font-size: 14px; padding: 9px 12px; outline: none; resize: vertical; }
  .field input:focus, .field textarea:focus { border-color: var(--accent); }

  .members { display: flex; flex-direction: column; gap: 4px; max-height: 200px; overflow-y: auto;
             border: 1px solid var(--border); border-radius: calc(10px * var(--rk, 1)); padding: 6px; }
  .member { display: flex; align-items: center; gap: 10px; padding: 7px 8px; border-radius: calc(8px * var(--rk, 1));
            border: none; background: transparent; cursor: pointer; text-align: left; }
  .member:hover { background: var(--bg-hover); }
  .member.on { background: var(--accent-soft); }
  .m-ava { width: 28px; height: 28px; border-radius: var(--ava-r, 50%); display: grid; place-items: center;
           color: #06070C; font-weight: 700; font-size: 12px; flex-shrink: 0; }
  .m-name { flex: 1; font-size: 13.5px; color: var(--fg); }
  .m-check { color: var(--accent); font-weight: 700; }
  .manual { margin-top: 8px; }
  .empty { font-size: 13px; color: var(--fg-dim); padding: 8px 2px; }

  .actions { display: flex; justify-content: flex-end; gap: 10px; margin-top: 8px; }
  .ghost, .primary { padding: 9px 18px; border-radius: calc(10px * var(--rk, 1)); font-size: 13px; font-weight: 700; cursor: pointer; border: none; }
  .ghost { background: transparent; border: 1px solid var(--border); color: var(--fg-muted); }
  .primary { background: linear-gradient(135deg, var(--accent), var(--accent-2)); color: #06070C; }
  .primary:disabled { opacity: 0.5; cursor: default; }
</style>
