<script lang="ts">
  /** "A new build is out" — a card at the top: update now or later. */
  import { t } from "../i18n";
  import { updateState, installUpdate, skipUpdate, BUILD_SHORT, BUILD } from "../updates";

  const st = $derived($updateState);
  const shown = $derived(st.s === "available" || st.s === "downloading" || st.s === "installing" || st.s === "permission" || (st.s === "error" && !!st.id));
  const label = $derived("info" in st && st.info ? (st.info.version ? `v${st.info.version}` : st.info.commit ?? st.info.tag) : "");
  const mb = (n: number) => (n / 1048576).toFixed(1);
</script>

{#if shown}
  <div class="upd" role="status">
    <div class="upd-ic">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3v12"/><path d="m7 10 5 5 5-5"/><path d="M5 21h14"/></svg>
    </div>
    <div class="upd-body">
      {#if st.s === "available"}
        <b>{$t("Update available")}</b>
        <span>{[label, BUILD_SHORT || BUILD.version ? $t("you have {v}", { v: BUILD.channel === "stable" ? "v" + BUILD.version : BUILD_SHORT || BUILD.version }) : "", st.info.asset ? `${mb(st.info.asset.size)} ${$t("MB")}` : ""].filter(Boolean).join(" · ")}</span>
      {:else if st.s === "downloading"}
        <b>{$t("Downloading the update…")}</b>
        <span class="bar"><i style:width="{st.total ? Math.min(100, (st.done / st.total) * 100) : 0}%"></i></span>
        <span>{mb(st.done)} / {mb(st.total)} {$t("MB")}</span>
      {:else if st.s === "installing"}
        <b>{$t("Installing…")}</b>
        <span>{$t("Confirm the installation in the window that opens.")}</span>
      {:else if st.s === "permission"}
        <b>{$t("Allow installing updates")}</b>
        <span>{$t("Turn on “Allow from this source” for Aquamarine and come back — the update continues by itself.")}</span>
      {:else if st.s === "error"}
        <b>{$t("The update didn't work")}</b>
        <span class="err">{st.error}</span>
      {/if}
    </div>
    <div class="upd-act">
      {#if st.s === "available" || st.s === "error"}
        <button class="go" onclick={installUpdate}>{$t(st.s === "error" ? "Try again" : "Update")}</button>
        <button class="later" onclick={skipUpdate}>{$t("Later")}</button>
      {:else if st.s === "permission"}
        <button class="go" onclick={installUpdate}>{$t("Continue")}</button>
      {/if}
    </div>
  </div>
{/if}

<style>
  .upd {
    position: fixed; z-index: 290; left: 50%; transform: translateX(-50%);
    top: calc(var(--tb-h, 0px) + env(safe-area-inset-top, 0px) + 10px);
    width: min(520px, calc(100vw - 20px)); box-sizing: border-box;
    display: flex; align-items: center; gap: 12px; padding: 12px 12px 12px 14px; border-radius: 18px;
    background: color-mix(in srgb, var(--bg-surface) 80%, transparent);
    backdrop-filter: blur(22px) saturate(1.5); -webkit-backdrop-filter: blur(22px) saturate(1.5);
    border: 1px solid var(--border-strong); box-shadow: 0 18px 44px -18px rgba(0,0,0,.85);
    animation: upd-in .4s cubic-bezier(.22,1,.36,1);
  }
  @keyframes upd-in { from { opacity: 0; transform: translateX(-50%) translateY(-14px); } }
  .upd-ic { width: 38px; height: 38px; flex-shrink: 0; border-radius: 12px; display: grid; place-items: center; color: #06070c;
    background: linear-gradient(135deg, var(--accent), var(--accent-2)); }
  .upd-ic svg { width: 20px; height: 20px; }
  .upd-body { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 3px; font-size: 13px; color: var(--fg-muted); }
  .upd-body b { color: var(--fg); font-size: 14.5px; }
  .upd-body span { overflow: hidden; text-overflow: ellipsis; }
  .err { color: var(--danger, #FF4D6D); white-space: normal; }
  .bar { display: block; height: 5px; border-radius: 3px; background: var(--bg-input); overflow: hidden; margin: 2px 0; }
  .bar i { display: block; height: 100%; background: linear-gradient(90deg, var(--accent), var(--accent-2)); transition: width .25s ease; }
  .upd-act { display: flex; flex-direction: column; gap: 6px; flex-shrink: 0; }
  .upd-act button { border: none; border-radius: 12px; padding: 8px 14px; font: 700 13.5px var(--font-ui); cursor: pointer; }
  .go { color: #06070c; background: linear-gradient(135deg, var(--accent), var(--accent-2)); }
  .later { color: var(--fg-muted); background: transparent; }
</style>
