<script lang="ts">
  /**
   * Settings → Privacy (Telegram-style): one row per item with its current
   * audience; tapping opens the rule — Everybody / My contacts / Nobody plus
   * "Always allow" and "Never allow" lists. Per-person exceptions made from
   * someone's profile land in the same lists.
   */
  import {
    privacy, PRIV_ITEMS, setAudience, setException, audienceLabel,
    type PrivKey, type Audience,
  } from "../privacy";
  import { avatars } from "../people";
  import { openProfile } from "./ProfileModal.svelte";
  import { t, tn } from "../i18n";

  let open = $state<PrivKey | null>(null);
  let addAllow = $state(""), addDeny = $state("");
  const item = $derived(PRIV_ITEMS.find((i) => i.key === open));
  const rule = $derived(open ? $privacy[open] : null);

  /** People with any exception, for the "per person" overview. */
  const people = $derived.by(() => {
    const m = new Map<string, number>();
    for (const k of Object.keys($privacy) as PrivKey[]) {
      for (const u of [...$privacy[k].allow, ...$privacy[k].deny]) m.set(u, (m.get(u) ?? 0) + 1);
    }
    return [...m.entries()].sort((a, b) => a[0].localeCompare(b[0]));
  });

  function add(kind: "allow" | "deny") {
    if (!open) return;
    const raw = kind === "allow" ? addAllow : addDeny;
    for (const u of raw.split(/[\s,]+/)) if (u.trim()) setException(open, u.trim().toLowerCase(), kind);
    if (kind === "allow") addAllow = ""; else addDeny = "";
  }
  const AUD: [Audience, string][] = [["everybody", "Everybody"], ["contacts", "My contacts"], ["nobody", "Nobody"]];
</script>

{#if !open}
  <section class="pr-group">
    <div class="pr-label">{$t("Privacy")}</div>
    {#each PRIV_ITEMS as it (it.key)}
      <button class="pr-row" onclick={() => (open = it.key)}>
        <span class="pr-name">{$t(it.label)}</span>
        <span class="pr-val">{$t(audienceLabel($privacy[it.key]))}</span>
        <span class="pr-chev">›</span>
      </button>
    {/each}
    <div class="pr-note">{$t("“My contacts” means people in your contact book and everyone you have a chat with.")}</div>
  </section>
  {#if people.length}
    <section class="pr-group">
      <div class="pr-label">{$t("Exceptions for people")}</div>
      {#each people as [u, n] (u)}
        <button class="pr-row" onclick={() => openProfile(u)}>
          <span class="pr-ava">{#if $avatars[u]}<img src={$avatars[u]} alt="" />{:else}{u.charAt(0).toUpperCase()}{/if}</span>
          <span class="pr-name">@{u}</span>
          <span class="pr-val">{$tn(n, "rule")}</span>
          <span class="pr-chev">›</span>
        </button>
      {/each}
    </section>
  {/if}
{:else if item && rule}
  <section class="pr-group">
    <button class="pr-back" onclick={() => (open = null)}>‹ {$t("Privacy")}</button>
    <div class="pr-q">{$t(item.q)}</div>
    <div class="pr-aud">
      {#each AUD as [id, lbl] (id)}
        <button class="pr-opt" class:on={rule.who === id} onclick={() => setAudience(item.key, id)}>
          <span class="radio" class:on={rule.who === id}></span>{$t(lbl)}
        </button>
      {/each}
    </div>
  </section>

  {#if rule.who !== "nobody"}
    <section class="pr-group">
      <div class="pr-label">{$t("Never allow")}</div>
      {#each rule.deny as u (u)}
        <div class="pr-person">
          <span class="pr-ava">{#if $avatars[u]}<img src={$avatars[u]} alt="" />{:else}{u.charAt(0).toUpperCase()}{/if}</span>
          <span class="pr-name">@{u}</span>
          <button class="pr-x" onclick={() => setException(item.key, u, null)} title={$t("Remove")}>×</button>
        </div>
      {/each}
      <div class="pr-add">
        <input placeholder={$t("Add username…")} bind:value={addDeny} onkeydown={(e) => { if (e.key === "Enter") add("deny"); }} />
        <button onclick={() => add("deny")}>{$t("Add")}</button>
      </div>
    </section>
  {/if}
  {#if rule.who !== "everybody"}
    <section class="pr-group">
      <div class="pr-label">{$t("Always allow")}</div>
      {#each rule.allow as u (u)}
        <div class="pr-person">
          <span class="pr-ava">{#if $avatars[u]}<img src={$avatars[u]} alt="" />{:else}{u.charAt(0).toUpperCase()}{/if}</span>
          <span class="pr-name">@{u}</span>
          <button class="pr-x" onclick={() => setException(item.key, u, null)} title={$t("Remove")}>×</button>
        </div>
      {/each}
      <div class="pr-add">
        <input placeholder={$t("Add username…")} bind:value={addAllow} onkeydown={(e) => { if (e.key === "Enter") add("allow"); }} />
        <button onclick={() => add("allow")}>{$t("Add")}</button>
      </div>
    </section>
  {/if}
  <div class="pr-note pad">{$t("You can also set these for one person from their profile → Privacy for this person.")}</div>
{/if}

<style>
  .pr-group { background: var(--bg-surface); border-radius: 14px; padding: 6px; margin: 0 18px 12px; }
  .pr-label { font-size: 11.5px; font-weight: 800; color: var(--accent); text-transform: uppercase; letter-spacing: 0.06em; padding: 8px 10px 6px; }
  .pr-row, .pr-person {
    display: flex; align-items: center; gap: 10px; width: 100%; padding: 10px; border-radius: 10px;
    background: none; border: none; color: var(--fg); font: inherit; font-size: 14px; text-align: left; cursor: pointer;
  }
  .pr-person { cursor: default; }
  .pr-row:hover { background: var(--bg-hover); }
  .pr-name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .pr-val { font-size: 13px; color: var(--accent); white-space: nowrap; }
  .pr-chev { color: var(--fg-dim); font-size: 18px; line-height: 1; }
  .pr-ava {
    width: 30px; height: 30px; border-radius: 50%; flex-shrink: 0; overflow: hidden; display: grid; place-items: center;
    background: linear-gradient(135deg, var(--accent), var(--accent-2)); color: #06070C; font-weight: 800; font-size: 13px;
  }
  .pr-ava img { width: 100%; height: 100%; object-fit: cover; }
  .pr-note { font-size: 12px; color: var(--fg-dim); padding: 6px 10px 8px; line-height: 1.5; }
  .pr-note.pad { padding: 0 28px; }
  .pr-back { background: none; border: none; color: var(--accent); font: inherit; font-weight: 700; font-size: 13.5px; padding: 8px 10px; cursor: pointer; }
  .pr-q { font-size: 14px; font-weight: 700; padding: 4px 10px 10px; color: var(--fg); }
  .pr-aud { display: flex; flex-direction: column; }
  .pr-opt {
    display: flex; align-items: center; gap: 12px; padding: 11px 10px; border-radius: 10px;
    background: none; border: none; color: var(--fg); font: inherit; font-size: 14px; text-align: left; cursor: pointer;
  }
  .pr-opt:hover { background: var(--bg-hover); }
  .radio { width: 18px; height: 18px; border-radius: 50%; border: 2px solid var(--fg-dim); flex-shrink: 0; position: relative; }
  .radio.on { border-color: var(--accent); }
  .radio.on::after { content: ""; position: absolute; inset: 3px; border-radius: 50%; background: var(--accent); box-shadow: 0 0 8px var(--accent-glow); }
  .pr-x { background: none; border: none; color: var(--fg-dim); font-size: 20px; cursor: pointer; padding: 0 6px; }
  .pr-x:hover { color: var(--danger, #ff6b6b); }
  .pr-add { display: flex; gap: 6px; padding: 6px 10px 8px; }
  .pr-add input {
    flex: 1; min-width: 0; background: var(--bg-input); border: 1px solid var(--border); border-radius: 8px;
    color: var(--fg); font: inherit; font-size: 13px; padding: 7px 10px; outline: none;
  }
  .pr-add input:focus { border-color: var(--accent); }
  .pr-add button { border: none; border-radius: 8px; padding: 7px 12px; background: var(--accent-soft); color: var(--accent); font: inherit; font-weight: 700; font-size: 13px; cursor: pointer; }
</style>
