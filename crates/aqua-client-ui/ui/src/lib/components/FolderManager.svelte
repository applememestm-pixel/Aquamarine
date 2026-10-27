<script module lang="ts">
  import { writable } from "svelte/store";
  export const folderManagerOpen = writable(false);
  export function openFolderManager() { folderManagerOpen.set(true); }
  export function closeFolderManager() { folderManagerOpen.set(false); }
</script>

<script lang="ts">
  import { t } from "../i18n";
  import { onMount } from "svelte";
  import { chats } from "../stores";
  import { folders, createFolder, renameFolder, deleteFolder, toggleChatInFolder } from "../folders";

  let newName = $state("");
  let editId = $state<string | null>(null); // folder whose membership is open

  function add() {
    const n = newName.trim();
    if (!n) return;
    const id = createFolder(n);
    newName = "";
    editId = id;
  }

  function onBackdrop(e: MouseEvent) {
    if ((e.target as HTMLElement).classList.contains("fm-backdrop")) closeFolderManager();
  }
  onMount(() => {
    const h = (e: KeyboardEvent) => { if (e.key === "Escape") closeFolderManager(); };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  });
</script>

{#if $folderManagerOpen}
  <div class="fm-backdrop" role="button" tabindex="-1" aria-label={$t("Close")} onclick={onBackdrop}
       onkeydown={(e) => { if (e.key === "Escape") closeFolderManager(); }}>
    <div class="fm-sheet" role="dialog" aria-modal="true" tabindex="-1">
      <div class="fm-head">
        <h3>{$t("Chat folders")}</h3>
        <button data-x class="fm-close" onclick={closeFolderManager} title={$t("Close (Esc)")}>×</button>
      </div>

      <div class="fm-create">
        <input placeholder={$t("New folder name")} bind:value={newName}
               onkeydown={(e) => { if (e.key === 'Enter') add(); }} />
        <button class="fm-add" onclick={add}>{$t("Create")}</button>
      </div>

      <div class="fm-list">
        {#if $folders.length === 0}
          <div class="fm-empty">{$t("No folders yet. Create one above, then add chats to it.")}</div>
        {:else}
          {#each $folders as f (f.id)}
            <div class="fm-folder">
              <div class="fm-folder-head">
                <input class="fm-name" value={f.name} oninput={(e) => renameFolder(f.id, (e.target as HTMLInputElement).value)} />
                <span class="fm-count">{f.chatIds.length}</span>
                <button class="fm-toggle" onclick={() => (editId = editId === f.id ? null : f.id)}>
                  {editId === f.id ? "Done" : "Chats"}
                </button>
                <button class="fm-del" onclick={() => deleteFolder(f.id)} title={$t("Delete folder")}>×</button>
              </div>
              {#if editId === f.id}
                <div class="fm-chats">
                  {#each $chats as c (c.id)}
                    <label class="fm-chat">
                      <input type="checkbox" checked={f.chatIds.includes(c.id)} onchange={() => toggleChatInFolder(f.id, c.id)} />
                      <span class="fm-av" style="--gradient: {c.gradient}">{c.letter}</span>
                      <span class="fm-chat-name">{c.name}</span>
                    </label>
                  {/each}
                </div>
              {/if}
            </div>
          {/each}
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .fm-backdrop {
    position: fixed; inset: 0;
    background: rgba(6, 7, 12, 0.55);
    backdrop-filter: blur(4px);
    display: grid; place-items: center;
    z-index: 115; padding: 20px;
    animation: fm-fade 0.15s ease-out;
  }
  @keyframes fm-fade { from { opacity: 0; } to { opacity: 1; } }
  .fm-sheet {
    width: min(460px, 94vw); max-height: 82vh;
    display: flex; flex-direction: column;
    background: var(--bg-shell); border: 1px solid var(--border);
    border-radius: calc(16px * var(--rk, 1)); overflow: hidden;
    box-shadow: 0 32px 80px -16px rgba(0,0,0,0.6);
  }
  .fm-head {
    display: flex; align-items: center; justify-content: space-between;
    padding: 14px 16px; border-bottom: 1px solid var(--border);
  }
  .fm-head h3 { margin: 0; font-size: 14.5px; font-weight: 700; }
  .fm-close {
    width: 28px; height: 28px; background: transparent;
    border: 1px solid var(--border); border-radius: calc(8px * var(--rk, 1));
    color: var(--fg-dim); font-size: 17px; cursor: pointer;
  }
  .fm-close:hover { color: var(--fg); background: var(--bg-hover); }
  .fm-create { display: flex; gap: 6px; padding: 12px 14px; border-bottom: 1px solid var(--border); }
  .fm-create input {
    flex: 1; padding: 8px 11px; background: var(--bg-input);
    border: 1px solid var(--border); border-radius: calc(9px * var(--rk, 1)); color: var(--fg);
    font-family: inherit; font-size: 13px; outline: none;
  }
  .fm-create input:focus { border-color: var(--accent); }
  .fm-add {
    padding: 8px 16px; border: none; border-radius: calc(9px * var(--rk, 1));
    background: linear-gradient(135deg, var(--accent), var(--accent-2));
    color: #06070C; font-weight: 700; font-size: 13px; cursor: pointer;
  }
  .fm-list { overflow-y: auto; padding: 10px 14px 14px; }
  .fm-empty { padding: 24px 8px; text-align: center; color: var(--fg-dim); font-size: 12.5px; line-height: 1.5; }
  .fm-folder { border: 1px solid var(--border); border-radius: calc(11px * var(--rk, 1)); margin-bottom: 8px; overflow: hidden; }
  .fm-folder-head { display: flex; align-items: center; gap: 8px; padding: 8px 10px; }
  .fm-name {
    flex: 1; min-width: 0; background: transparent; border: none;
    color: var(--fg); font-size: 13.5px; font-weight: 600; outline: none;
    border-bottom: 1px solid transparent; padding: 2px 0;
  }
  .fm-name:focus { border-bottom-color: var(--accent); }
  .fm-count {
    font-size: 11px; color: var(--fg-dim); font-variant-numeric: tabular-nums;
    background: var(--bg-input); border-radius: 100px; padding: 1px 8px;
  }
  .fm-toggle {
    padding: 4px 10px; border: 1px solid var(--border); border-radius: calc(7px * var(--rk, 1));
    background: transparent; color: var(--fg-muted); font-size: 11.5px; font-weight: 600; cursor: pointer;
  }
  .fm-toggle:hover { color: var(--accent); border-color: var(--accent-soft); }
  .fm-del {
    width: 26px; height: 26px; background: transparent;
    border: 1px solid var(--border); border-radius: calc(7px * var(--rk, 1));
    color: var(--danger); font-size: 15px; cursor: pointer; flex-shrink: 0;
  }
  .fm-del:hover { background: color-mix(in srgb, var(--danger) 14%, transparent); }
  .fm-chats { border-top: 1px solid var(--border); max-height: 260px; overflow-y: auto; padding: 6px; }
  .fm-chat {
    display: flex; align-items: center; gap: 9px;
    padding: 6px 8px; border-radius: calc(8px * var(--rk, 1)); cursor: pointer;
  }
  .fm-chat:hover { background: var(--bg-hover); }
  .fm-chat input { accent-color: var(--accent); width: 16px; height: 16px; }
  .fm-av {
    width: 28px; height: 28px; border-radius: 50%;
    display: grid; place-items: center; font-weight: 700; font-size: 12px;
    color: rgba(6,7,12,0.85); background: var(--gradient, linear-gradient(135deg,#666,#999));
    flex-shrink: 0;
  }
  .fm-chat-name { font-size: 13px; color: var(--fg); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
</style>
