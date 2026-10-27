<script lang="ts">
  /** Settings → Network & proxy (lib/network.ts). */
  import { onMount } from "svelte";
  import { info } from "../stores";
  import { t, tr } from "../i18n";
  import { netPrefs, serverPings, proxyPings, serverCandidates, measureServers, measureProxies, switchServer, applyProxy, normProxy } from "../network";
  import { showToast } from "./Toast.svelte";
  import { addManualServer, forgetServer, aliasesOf, federationOf } from "../servers";
  import { api } from "../tauri";

  // ---- Add a server by hand / forget one ----
  let newServer = $state("");
  let srvErr = $state<string | null>(null);
  let srvBusy = $state(false);
  async function addServer() {
    srvErr = null;
    let u = newServer.trim().replace(/\/+$/, "");
    if (!u) return;
    if (!/^https?:\/\//i.test(u)) u = "http://" + u;
    if (!/:\d+$/.test(u.replace(/^https?:\/\//, "")) && u.startsWith("http://")) u += ":7777";
    srvBusy = true;
    try {
      const ms = await api.serverPing(u);
      if (ms == null) throw new Error(tr("no answer"));
      addManualServer(u);
      newServer = "";
      servers = serverCandidates();
      void measureServers();
      showToast(tr("Added {u}", { u: short(u) }));
    } catch (e) { srvErr = tr("{u} doesn't answer: {e}", { u: short(u), e: String(e).replace(/^Error: /, "") }); }
    srvBusy = false;
  }
  function forget(u: string) {
    forgetServer(u);
    servers = serverCandidates();
  }
  const kindOf = (u: string) => (aliasesOf(cur).includes(u) ? "alias" : federationOf(cur).includes(u) ? "fed" : "manual");

  let servers = $state<string[]>([]);
  let busy = $state<string | null>(null);
  let adding = $state("");
  let addErr = $state<string | null>(null);
  const cur = $derived(($info?.server_url ?? "").replace(/\/+$/, ""));
  const curProxy = $derived($info?.proxy ?? "");
  const locked = $derived(($netPrefs.lockServer ?? "").replace(/\/+$/, ""));

  async function refresh() {
    servers = serverCandidates();
    await Promise.all([measureServers(), measureProxies()]);
    servers = serverCandidates();
  }
  onMount(() => {
    void refresh();
    const iv = setInterval(refresh, 15_000);
    return () => clearInterval(iv);
  });

  const msLabel = (ms: number | null | undefined) => (ms === undefined ? "…" : ms === null ? tr("no answer") : `${ms} ${tr("ms")}`);
  const msClass = (ms: number | null | undefined) => (ms == null ? "off" : ms > 400 ? "slow" : ms > 150 ? "mid" : "ok");
  const short = (u: string) => u.replace(/^\w+:\/\//, "");

  async function pickServer(u: string) {
    if (u === cur || busy) return;
    busy = u;
    const err = await switchServer(u);
    busy = null;
    showToast(err ? tr("Could not connect: {e}", { e: err }) : tr("Connected to {u}", { u: short(u) }));
  }

  async function setUseProxy(on: boolean) {
    netPrefs.update((p) => ({ ...p, useProxy: on }));
    if (!on) { if (curProxy) await applyProxy(""); return; }
    const first = $netPrefs.proxies[0];
    if (first && !curProxy) await applyProxy(first);
  }
  async function pickProxy(p: string) {
    if (busy) return;
    busy = p;
    netPrefs.update((x) => ({ ...x, useProxy: true }));
    const err = await applyProxy(p);
    busy = null;
    showToast(err ? tr("Could not connect: {e}", { e: err }) : tr("Connected through {p}", { p: short(p) }));
  }
  function addProxy() {
    addErr = null;
    const p = normProxy(adding);
    if (!/^(socks5h?|socks4a?|https?):\/\/[^\s/]+:\d+\/?$/i.test(p.replace(/\/\/[^@]*@/, "//"))) { addErr = tr("Use socks5://host:port or http://host:port (login:password@ before the host if needed)."); return; }
    netPrefs.update((x) => ({ ...x, proxies: [...new Set([...x.proxies, p])] }));
    adding = "";
    void measureProxies();
  }
  async function removeProxy(p: string) {
    netPrefs.update((x) => ({ ...x, proxies: x.proxies.filter((y) => y !== p) }));
    if (curProxy === p) await applyProxy($netPrefs.useProxy && $netPrefs.proxies[0] ? $netPrefs.proxies[0] : "");
  }
</script>

<section class="group">
  <div class="label">{$t("Servers")}</div>
  <div class="ns-list">
    {#each servers as u (u)}
      {@const ms = $serverPings[u]}
      <div class="ns-row as-div" class:cur={u === cur}>
        <button class="ns-pick" onclick={() => pickServer(u)} disabled={!!busy}>
          <span class="ns-dot {msClass(ms)}"></span>
          <span class="ns-main">
            <span class="ns-addr">{short(u)}</span>
            <span class="ns-sub">{u === cur ? $t($info?.connected ? "Connected" : "Connecting…") : kindOf(u) === "alias" ? $t("This server's other address") : kindOf(u) === "fed" ? $t("Your account works here too") : $t("Added by you")}</span>
          </span>
          <span class="ns-ms {msClass(ms)}">{busy === u ? "…" : msLabel(ms)}</span>
          {#if u === locked}<span class="ns-lock" title={$t("Locked")}>🔒</span>{:else if u === cur}<span class="ns-tick">✓</span>{/if}
        </button>
        {#if u !== cur}<button class="ns-x" onclick={() => forget(u)} title={$t("Forget this server")} aria-label={$t("Forget this server")}>×</button>{/if}
      </div>
    {:else}
      <div class="ns-empty">{$t("Not connected to a server yet.")}</div>
    {/each}
  </div>
  <form class="ns-add" onsubmit={(e) => { e.preventDefault(); void addServer(); }}>
    <input bind:value={newServer} placeholder="192.168.1.39:7777" spellcheck="false" autocomplete="off" autocapitalize="off" />
    <button type="submit" disabled={!newServer.trim() || srvBusy}>{srvBusy ? "…" : $t("Add server")}</button>
  </form>
  {#if srvErr}<div class="ns-note bad">{srvErr}</div>{/if}
  <label class="ns-switch">
    <span><b>{$t("Stay on this server")}</b><small>{locked ? $t("Locked to {u}. The app never moves to another server — if this one goes down, you're offline until it's back.", { u: short(locked) }) : $t("Never move to another server by itself, not even when this one stops answering.")}</small></span>
    <input type="checkbox" checked={!!locked} disabled={!cur && !locked} onchange={(e) => netPrefs.update((p) => ({ ...p, lockServer: (e.currentTarget as HTMLInputElement).checked ? cur : "" }))} />
  </label>
  <label class="ns-switch" class:off={!!locked}>
    <span><b>{$t("Pick the fastest server by itself")}</b><small>{$t("Moves to a server of your federation that answers clearly faster, or off one that stopped answering. Your chats and account are the same on all of them.")}</small></span>
    <input type="checkbox" checked={$netPrefs.autoServer && !locked} disabled={!!locked} onchange={(e) => netPrefs.update((p) => ({ ...p, autoServer: (e.currentTarget as HTMLInputElement).checked }))} />
  </label>
</section>

<section class="group">
  <div class="label">{$t("Proxy")}</div>
  <label class="ns-switch">
    <span><b>{$t("Use proxy")}</b><small>{$t("All traffic of the app goes through the chosen proxy.")}</small></span>
    <input type="checkbox" checked={$netPrefs.useProxy} onchange={(e) => void setUseProxy((e.currentTarget as HTMLInputElement).checked)} />
  </label>
  <div class="ns-list">
    {#each $netPrefs.proxies as p (p)}
      {@const ms = $proxyPings[p]}
      <div class="ns-row as-div" class:cur={p === curProxy}>
        <button class="ns-pick" onclick={() => pickProxy(p)} disabled={!!busy}>
          <span class="ns-dot {msClass(ms)}"></span>
          <span class="ns-main">
            <span class="ns-addr">{short(p).replace(/^[^@]*@/, "")}</span>
            <span class="ns-sub">{p.split("://")[0].toUpperCase()}{p === curProxy ? " · " + $t("in use") : ""}</span>
          </span>
          <span class="ns-ms {msClass(ms)}">{busy === p ? "…" : msLabel(ms)}</span>
        </button>
        <button class="ns-x" onclick={() => removeProxy(p)} title={$t("Remove")} aria-label={$t("Remove")}>×</button>
      </div>
    {/each}
  </div>
  <form class="ns-add" onsubmit={(e) => { e.preventDefault(); addProxy(); }}>
    <input bind:value={adding} placeholder="socks5://127.0.0.1:1080" spellcheck="false" autocomplete="off" autocapitalize="off" />
    <button type="submit" disabled={!adding.trim()}>{$t("Add proxy")}</button>
  </form>
  {#if addErr}<div class="ns-note bad">{addErr}</div>{/if}
  <label class="ns-switch">
    <span><b>{$t("Switch proxies by itself")}</b><small>{$t("When the proxy in use stops answering, the app moves to another one from the list that works.")}</small></span>
    <input type="checkbox" checked={$netPrefs.autoProxy} onchange={(e) => netPrefs.update((p) => ({ ...p, autoProxy: (e.currentTarget as HTMLInputElement).checked }))} />
  </label>
  <div class="ns-note">{$t("The ping is the time to your server through each proxy. Aquamarine doesn't run proxies itself: point it at one you have (a SOCKS5 tunnel, a VPN client's local port, a proxy from a friend). Messages are end-to-end encrypted either way.")}</div>
</section>

<style>
  .ns-list { display: flex; flex-direction: column; gap: 2px; margin: 4px 0 8px; }
  .ns-row { display: flex; align-items: center; gap: 10px; width: 100%; padding: 9px 10px; border: none; border-radius: calc(12px * var(--rk, 1)); background: transparent; color: var(--fg); font: inherit; text-align: left; cursor: pointer; }
  .ns-row.as-div { padding: 0 4px 0 0; cursor: default; }
  .ns-pick { flex: 1; min-width: 0; display: flex; align-items: center; gap: 10px; padding: 9px 10px; border: none; border-radius: calc(12px * var(--rk, 1)); background: transparent; color: var(--fg); font: inherit; text-align: left; cursor: pointer; }
  .ns-row:hover:not(.as-div), .ns-pick:hover { background: var(--bg-hover); }
  .ns-row.cur { background: var(--accent-soft); }
  .ns-dot { width: 9px; height: 9px; border-radius: 50%; flex-shrink: 0; background: var(--fg-dim); }
  .ns-dot.ok { background: #3BD671; box-shadow: 0 0 8px #3BD67188; }
  .ns-dot.mid { background: #9BD63B; }
  .ns-dot.slow { background: #F0B232; }
  .ns-dot.off { background: var(--danger, #FF4D6D); opacity: .8; }
  .ns-main { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 1px; }
  .ns-addr { font-weight: 600; font-size: 14px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .ns-sub { font-size: 12px; color: var(--fg-dim); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .ns-ms { font-size: 12.5px; font-weight: 700; font-variant-numeric: tabular-nums; white-space: nowrap; }
  .ns-ms.ok { color: #3BD671; } .ns-ms.mid { color: #9BD63B; } .ns-ms.slow { color: #F0B232; } .ns-ms.off { color: var(--fg-dim); font-weight: 600; }
  .ns-tick { color: var(--accent); font-weight: 800; }
  .ns-lock { font-size: 13px; }
  .ns-switch.off { opacity: .5; cursor: default; }
  .ns-x { width: 32px; height: 32px; border: none; border-radius: 50%; background: transparent; color: var(--fg-dim); font-size: 18px; cursor: pointer; flex-shrink: 0; }
  .ns-x:hover { color: var(--danger, #FF4D6D); background: var(--bg-hover); }
  .ns-empty { padding: 10px; font-size: 13px; color: var(--fg-dim); }
  .ns-switch { display: flex; align-items: center; gap: 12px; padding: 10px 4px; cursor: pointer; }
  .ns-switch > span { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 2px; }
  .ns-switch b { font-size: 14px; font-weight: 600; color: var(--fg); }
  .ns-switch small { font-size: 12px; color: var(--fg-dim); line-height: 1.4; }
  .ns-switch input { width: 18px; height: 18px; accent-color: var(--accent); flex-shrink: 0; }
  .ns-add { display: flex; gap: 8px; margin: 4px 0; }
  .ns-add input { flex: 1; min-width: 0; background: var(--bg-input); border: 1px solid var(--border); border-radius: calc(10px * var(--rk, 1)); color: var(--fg); font: 13px var(--font-mono, monospace); padding: 8px 10px; outline: none; }
  .ns-add input:focus { border-color: var(--accent); }
  .ns-add button { border: none; border-radius: calc(10px * var(--rk, 1)); padding: 8px 12px; background: var(--accent-soft); color: var(--accent); font: 600 13px var(--font-ui); cursor: pointer; white-space: nowrap; }
  .ns-add button:disabled { opacity: .5; }
  .ns-note { font-size: 12px; color: var(--fg-dim); line-height: 1.45; margin-top: 6px; }
  .ns-note.bad { color: var(--danger, #FF4D6D); }
</style>
