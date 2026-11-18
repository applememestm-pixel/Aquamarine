<script module lang="ts">
  import { writable, get } from "svelte/store";
  import type { FingerprintView } from "../types";

  // The peer's marks are heard app-wide (on a phone the chat screen exists
  // only while a chat is open — a mark sent meanwhile was lost: half a sun forever).
  let listening = false;
  export function listenKeyOk() {
    if (listening) return;
    listening = true;
    void import("../tauri").then(({ events }) => events.onKeyOk((p) => noteKeyOk(p.from, p.code, p.on))).catch(() => { listening = false; });
  }

  const open = writable<boolean>(false);
  const view = writable<FingerprintView | null>(null);
  const peer = writable<string>("");

  /** peer id → the identity key (hex) the user checked against the words. */
  const KEY = "aqua.verified.v1";
  function loadVerified(): Record<string, string> {
    try { return JSON.parse(localStorage.getItem(KEY) || "{}"); } catch { return {}; }
  }
  export const verifiedKeys = writable<Record<string, string>>(loadVerified());
  verifiedKeys.subscribe((v) => { try { localStorage.setItem(KEY, JSON.stringify(v)); } catch { /* storage blocked */ } });

  /** "verified": checked and unchanged; "changed": checked, but the key is different now. */
  export function keyState(peerId: string, key: string | null | undefined, known: Record<string, string>): "verified" | "changed" | "unverified" {
    const k = known[peerId];
    if (!k || !key) return "unverified";
    return k === key ? "verified" : "changed";
  }

  /** peer id → the safety code (hex) THEY compared and marked ([keyok:v1]).
   *  Both marked, same code as now = a whole sun in the chat header. */
  const BY_KEY = "aqua.verifiedBy.v1";
  export const verifiedBy = writable<Record<string, string>>((() => { try { return JSON.parse(localStorage.getItem(BY_KEY) || "{}"); } catch { return {}; } })());
  verifiedBy.subscribe((v) => { try { localStorage.setItem(BY_KEY, JSON.stringify(v)); } catch { /* storage blocked */ } });
  export function noteKeyOk(from: string, code: string, on: boolean) {
    const c = (code || "").replace(/\s+/g, "").toLowerCase();
    const isNew = on && !!c && get(verifiedBy)[from] !== c;
    verifiedBy.update((m) => { const n = { ...m }; if (on && c) n[from] = c; else delete n[from]; return n; });
    // They just marked our code: if I marked it too, tell them now — so the
    // sun turns whole on both sides at once, not only after I happen to open
    // the chat. Only on news (no ping-pong: their echo is no longer new).
    if (isNew) {
      void import("../tauri").then(async ({ api }) => {
        const v = await api.verifyFingerprint(from);
        const mine = v.hex.replace(/\s+/g, "").toLowerCase();
        if (mine === c && keyState(from, v.peer_key, get(verifiedKeys)) === "verified") sendKeyOk(from, v.hex, true);
      }).catch(() => {});
    }
  }
  /** Tell the peer I marked (or unmarked) our code — their sun follows. */
  export function sendKeyOk(peerId: string, hex: string, on: boolean) {
    const body = JSON.stringify({ c: hex.replace(/\s+/g, "").toLowerCase(), on });
    void import("../tauri").then(({ api }) => api.sendControl(peerId, "[keyok:v1]" + body)).catch(() => {});
  }
  /** Sun: "none" not compared, "mine" I compared, "both" both did (same code), "changed" the key changed. */
  export function sunState(peerId: string, key: string | null | undefined, hex: string | null | undefined, known: Record<string, string>, by: Record<string, string>): "none" | "mine" | "both" | "changed" {
    const k = keyState(peerId, key, known);
    if (k === "changed") return "changed";
    if (k !== "verified") return "none";
    return hex && by[peerId] === hex.replace(/\s+/g, "").toLowerCase() ? "both" : "mine";
  }

  export function openFingerprint(v: FingerprintView, peerId = "") {
    view.set(v);
    peer.set(peerId);
    open.set(true);
  }
  export function closeFingerprint() { open.set(false); }
  export { open as fpOpen, view as fpView };
</script>

<script lang="ts">
  import { t, tr } from "../i18n";
  import { onMount } from "svelte";
  import { showToast } from "./Toast.svelte";
  import QrCode from "./QrCode.svelte";
  import { scanQr } from "./QrScanner.svelte";
  import { keyQr, parseQr } from "../qr";
  import { info } from "../stores";

  let showQr = $state(false);
  /** Signal-style: scan the code on their screen; it must be the same. */
  async function scanTheirs() {
    const text = await scanQr();
    const v = get(view);
    if (!text || !v) return;
    const r = parseQr(text);
    if (!r || r.kind !== "key") { showToast(tr("That QR code isn't a key check")); return; }
    if (r.code.toLowerCase() === v.hex.replace(/\s+/g, "").toLowerCase()) markVerified();
    else showToast(tr("The codes are different — this is not the same key. Don't trust this chat until you find out why."), 8000);
  }

  onMount(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") closeFingerprint(); };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  });

  const kstate = $derived($view && $peer ? keyState($peer, $view.peer_key, $verifiedKeys) : "unverified");

  function markVerified() {
    const v = get(view), id = get(peer);
    if (v && id) { verifiedKeys.update((m) => ({ ...m, [id]: v.peer_key })); sendKeyOk(id, v.hex, true); }
    closeFingerprint();
    showToast(tr("{name}'s key marked verified", { name: v?.peer_name ?? "" }));
  }
  function unverify() {
    const id = get(peer), v = get(view);
    verifiedKeys.update((m) => { const n = { ...m }; delete n[id]; return n; });
    if (id && v) sendKeyOk(id, v.hex, false);
    closeFingerprint();
  }
</script>

{#if $open && $view}
  <div
    class="modal-bg"
    role="button"
    tabindex="-1"
    aria-label={$t("Close verification dialog")}
    onclick={(e) => { if (e.target === e.currentTarget) closeFingerprint(); }}
    onkeydown={(e) => { if (e.key === "Escape" && e.target === e.currentTarget) closeFingerprint(); }}
  >
    <div class="modal" role="dialog" aria-modal="true">
      <h2>{$t("Verify {name}'s key", { name: $view.peer_name })}</h2>
      {#if kstate === "changed"}
        <p class="fp-warn">{$t("{name}'s key has changed since you verified it. That happens after reinstalling the app or signing in anew — or when someone is trying to read the chat. Compare the words again.", { name: $view.peer_name })}</p>
      {:else if kstate === "verified"}
        <p class="fp-ok">{$t("Verified: these words matched on both phones.")}</p>
        {#if $peer && $verifiedBy[$peer] === $view.hex.replace(/\s+/g, "").toLowerCase()}
          <p class="fp-ok">☀ {$t("{name} verified your key too — the sun is whole.", { name: $view.peer_name })}</p>
        {:else}
          <p>{$t("When {name} marks it on their side too, the sun becomes whole for both of you.", { name: $view.peer_name })}</p>
        {/if}
      {/if}
      <p>{$t("Open this window in the same chat on {name}'s device — in person, or over a call where you know the voice. If the words are the same on both screens, nobody stands between you.", { name: $view.peer_name })}</p>
      <div class="fp-grid">
        {#each $view.words as w, i (i)}
          <div class="word">{w}</div>
        {/each}
      </div>
      {#if showQr}
        <div class="fp-qr"><QrCode text={keyQr($info?.username ?? "", $view.hex)} size={200} letter="🔒" /></div>
      {/if}
      <div class="fp-qr-btns">
        <button class="btn" onclick={() => (showQr = !showQr)}>{showQr ? $t("Hide my QR") : $t("Show my QR")}</button>
        <button class="btn" onclick={scanTheirs}>{$t("Scan their QR")}</button>
      </div>
      <div class="fp-instruction">
        {$t("Safety code:")}
        <span class="hex">{$view.hex}</span>
      </div>
      <div class="modal-actions">
        {#if kstate === "verified"}
          <button class="btn" onclick={unverify}>{$t("Mark as not verified")}</button>
          <button class="btn primary" onclick={closeFingerprint}>{$t("Done")}</button>
        {:else}
          <button class="btn" onclick={closeFingerprint}>{$t("Cancel")}</button>
          <button class="btn primary" onclick={markVerified}>{$t("They match")}</button>
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .fp-qr { display: grid; place-items: center; margin: -6px 0 14px; }
  .fp-qr-btns { display: flex; gap: 8px; margin-bottom: 16px; }
  .fp-qr-btns .btn { flex: 1; }
  .fp-warn { color: #F0B232; font-weight: 600; }
  .fp-ok { color: #3BD671; font-weight: 600; }
  .modal-bg {
    position: fixed;
    inset: 0;
    background: rgba(6,7,12,0.6);
    backdrop-filter: blur(6px);
    display: grid;
    place-items: center;
    z-index: 200;
    padding: 20px;
  }
  .modal {
    background: var(--bg-surface);
    border: 1px solid var(--border-strong);
    border-radius: calc(16px * var(--rk, 1));
    padding: 28px;
    max-width: 420px;
    width: 100%;
    box-shadow: 0 32px 64px -16px rgba(0,0,0,0.6), var(--shadow-accent);
  }
  h2 { margin: 0 0 4px; font-size: 18px; font-weight: 700; letter-spacing: -0.01em; }
  p  { margin: 0 0 20px; color: var(--fg-muted); font-size: 13.5px; }
  .fp-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
    padding: 16px;
    background: var(--bg-input);
    border: 1px solid var(--border);
    border-radius: calc(12px * var(--rk, 1));
    font-family: var(--font-mono);
    font-size: 15px;
    letter-spacing: 0.03em;
    margin-bottom: 20px;
  }
  .word {
    color: var(--accent);
    font-weight: 500;
    padding: 4px 10px;
    background: color-mix(in srgb, var(--accent) 6%, transparent);
    border-radius: calc(6px * var(--rk, 1));
    text-align: center;
  }
  .word:nth-child(even) { color: var(--accent-2); background: color-mix(in srgb, var(--accent-2) 6%, transparent); }
  .fp-instruction {
    font-size: 12.5px;
    color: var(--fg-muted);
    padding: 10px 12px;
    background: rgba(0,0,0,0.2);
    border-radius: calc(8px * var(--rk, 1));
    border-left: 2px solid var(--accent);
    margin-bottom: 20px;
    word-break: break-all;
  }
  .fp-instruction .hex {
    font-family: var(--font-mono);
    color: var(--accent);
    font-size: 12px;
  }
  .modal-actions { display: flex; gap: 10px; justify-content: flex-end; }
  .btn {
    padding: 10px 18px;
    border-radius: calc(10px * var(--rk, 1));
    border: 1px solid var(--border-strong);
    background: var(--bg-input);
    color: var(--fg);
    font-family: inherit;
    font-weight: 600;
    font-size: 13.5px;
    cursor: pointer;
    transition: all 0.12s ease;
  }
  .btn:hover { background: var(--bg-hover); border-color: var(--fg-dim); }
  .btn.primary {
    background: linear-gradient(135deg, var(--accent), var(--accent-2));
    color: #06070C;
    border-color: transparent;
    box-shadow: 0 0 16px -4px var(--accent-glow);
  }
  .btn.primary:hover { box-shadow: 0 0 24px -2px var(--accent-glow); }
</style>
