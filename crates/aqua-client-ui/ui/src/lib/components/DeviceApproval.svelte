<script module lang="ts">
  import { writable } from "svelte/store";
  import type { DeviceTrust } from "../tauri";
  /** Devices of my account waiting for approval (shared with Settings → Devices). */
  export const deviceTrust = writable<DeviceTrust | null>(null);
</script>

<script lang="ts">
  /**
   * "New device signed in: it's me / sign it out". A new device of my
   * account gets nothing from this one — no copies of what I send, no
   * history key — until it's approved here or on another approved device
   * (src/devtrust.rs).
   */
  import { onMount } from "svelte";
  import { api, events } from "../tauri";
  import { t, tr } from "../i18n";
  import { showToast } from "./Toast.svelte";
  import { askConfirm } from "./Confirm.svelte";
  import { openSettings } from "../panels/settings";
  import { info } from "../stores";

  /** "Later" for this run of the app. */
  let later = $state<Set<string>>(new Set());
  let busy = $state(false);

  async function load() {
    try { deviceTrust.set(await api.deviceTrust()); } catch { /* not signed in yet */ }
  }
  onMount(() => {
    void load();
    let un: (() => void) | null = null;
    void events.onDevicePending(() => void load()).then((u) => (un = u)).catch(() => {});
    return () => un?.();
  });

  /** This device isn't on my account's signed list: people's messages
   *  don't reach it and its own are ignored (devlists.rs). The list heals
   *  by itself when the old devices are gone; otherwise say so. */
  let unlisted = $state(false);
  let unlistedSeen = $state(false);
  let checkT: ReturnType<typeof setTimeout> | undefined;
  async function checkListed() {
    try { const st = await api.devlistStatus(); unlisted = st.published && !st.this_listed; } catch { /* offline */ }
  }
  $effect(() => {
    if (!$info?.connected) return;
    clearTimeout(checkT);
    // After the app had its chance to publish / repair the list.
    checkT = setTimeout(() => void checkListed(), 12_000);
    return () => clearTimeout(checkT);
  });

  const cur = $derived(($deviceTrust?.pending ?? []).find((p) => !later.has(p.id)) ?? null);

  function when(sec: number): string {
    if (!sec) return "";
    const d = new Date(sec * 1000);
    return d.toLocaleString(undefined, { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" });
  }

  async function approve() {
    if (!cur) return;
    busy = true;
    try { await api.deviceApprove(cur.id); showToast(tr("{d} can now see your chats", { d: cur.name || cur.id })); }
    catch (e) { showToast(tr("Couldn't approve: {e}", { e: String(e) })); }
    busy = false;
    await load();
  }
  async function decline() {
    if (!cur) return;
    const ok = await askConfirm({
      title: tr("Sign {d} out?", { d: cur.name || cur.id }),
      text: tr("It's signed out of your account. Change your password afterwards — whoever signed in knows it."),
      ok: tr("Sign out"),
      danger: true,
    });
    if (!ok) return;
    busy = true;
    try { await api.deviceDecline(cur.id); showToast(tr("Device signed out. Change your password in Settings → Account.")); }
    catch (e) { showToast(tr("Couldn't sign it out: {e}", { e: String(e) })); }
    busy = false;
    await load();
  }
</script>

{#if unlisted && !unlistedSeen && !cur}
  <div class="da-bar" role="status">
    <span>{$t("This device isn't confirmed by your account yet: people don't receive your messages from it, and theirs don't reach it. Confirm it on another device of yours (“It's me”) — or, if there is none, start the device list over.")}</span>
    <button class="da-btn ok" onclick={() => { unlistedSeen = true; openSettings("devices"); }}>{$t("Devices")}</button>
    <button class="da-later" data-x onclick={() => (unlistedSeen = true)} aria-label={$t("Close")}>✕</button>
  </div>
{/if}

{#if cur}
  <div class="da-back">
    <div class="da" role="dialog" aria-modal="true" aria-label={$t("New device signed in")}>
      <div class="da-ic" aria-hidden="true">
        <svg viewBox="0 0 24 24" width="30" height="30" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="4" width="13" height="10" rx="2"/><line x1="5" y1="18" x2="12" y2="18"/><rect x="17" y="8" width="5" height="12" rx="1"/></svg>
      </div>
      <h3>{$t("New device signed in")}</h3>
      <div class="da-dev">
        <b>{cur.name || $t("Device {id}", { id: cur.id.slice(0, 6) })}</b>
        <span>{when(cur.added || cur.at)}{cur.added || cur.at ? " · " : ""}{cur.id}</span>
      </div>
      <p>{$t("Someone signed in to your account on this device. If it's you, allow it — it gets your chats and their history. Until then it gets nothing from your other devices.")}</p>
      <p class="warn">{$t("Not you? Sign it out and change your password: whoever did it knows it. Turn on two-step verification in Settings → Privacy & Security.")}</p>
      <div class="da-row">
        <button class="da-btn danger" disabled={busy} onclick={decline}>{$t("Not me — sign out")}</button>
        <button class="da-btn ok" disabled={busy} onclick={approve}>{$t("It's me")}</button>
      </div>
      <button class="da-later cancel" onclick={() => (later = new Set([...later, cur!.id]))}>{$t("Decide later")}</button>
    </div>
  </div>
{/if}

<style>
  .da-back { position: fixed; inset: 0; z-index: 10050; display: grid; place-items: center; background: rgba(0, 0, 0, 0.55); padding: 16px; }
  .da { width: min(400px, 100%); padding: 22px 20px 14px; border-radius: calc(18px * var(--rk, 1)); background: var(--bg-shell); border: 1px solid var(--border-strong); box-shadow: 0 24px 60px rgba(0, 0, 0, 0.5); display: flex; flex-direction: column; align-items: center; text-align: center; gap: 8px; }
  .da-ic { width: 56px; height: 56px; border-radius: 50%; display: grid; place-items: center; background: color-mix(in srgb, var(--accent) 16%, transparent); color: var(--accent); }
  h3 { margin: 4px 0 0; font-size: 17px; font-weight: 700; }
  .da-dev { display: flex; flex-direction: column; gap: 2px; padding: 8px 14px; border-radius: calc(12px * var(--rk, 1)); background: var(--bg-input); border: 1px solid var(--border); width: 100%; }
  .da-dev b { font-size: 14.5px; }
  .da-dev span { font-size: 12px; color: var(--fg-muted); font-family: var(--font-mono, ui-monospace, monospace); }
  p { margin: 0; font-size: 13.5px; color: var(--fg-muted); line-height: 1.45; }
  p.warn { font-size: 12.5px; }
  .da-row { display: flex; gap: 8px; width: 100%; margin-top: 8px; }
  .da-btn { flex: 1; min-height: 40px; padding: 0 12px; border-radius: calc(11px * var(--rk, 1)); border: 1px solid var(--border-strong); background: transparent; color: var(--fg); font: inherit; font-size: 13.5px; font-weight: 700; cursor: pointer; }
  .da-btn.ok { border-color: transparent; background: var(--accent); color: #06070c; }
  .da-btn.danger { color: #e5484d; border-color: color-mix(in srgb, #e5484d 50%, transparent); }
  .da-btn:disabled { opacity: 0.6; cursor: default; }
  .da-later { border: none; background: none; color: var(--fg-muted); font: inherit; font-size: 12.5px; padding: 6px; cursor: pointer; }
  .da-later:hover { color: var(--fg); }
  .da-bar { position: fixed; z-index: 10040; left: 50%; top: calc(44px + env(safe-area-inset-top, 0px)); transform: translateX(-50%); width: min(560px, calc(100% - 24px)); display: flex; align-items: center; gap: 10px; padding: 10px 12px; border-radius: calc(14px * var(--rk, 1)); background: var(--bg-shell); border: 1px solid color-mix(in srgb, var(--danger) 55%, var(--border-strong)); box-shadow: 0 16px 40px rgba(0, 0, 0, 0.45); font-size: 13px; line-height: 1.4; color: var(--fg); }
  .da-bar span { flex: 1; min-width: 0; }
  .da-bar .da-btn { flex: none; min-height: 34px; }
</style>
