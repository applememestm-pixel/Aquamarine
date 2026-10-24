<script lang="ts">
  /**
   * Server field with a drop-down of every relay the app can find: the
   * project's public list (site/servers.json — so mobile data, with no local
   * network to search, still shows servers), the defaults, remembered ones,
   * relays found on the local network, and every relay those know about
   * (their own outside address and their federation, two hops deep) — each
   * with its full address and port, IP, rough location, ping and who's
   * online. Tap one to use it.
   */
  import { onMount } from "svelte";
  import { api, type ServerProbe } from "../tauri";
  import { knownServers, aliasesOf, rememberAlias } from "../servers";
  import { t, tr } from "../i18n";

  // inline: the list is always shown in place (the server sheet on the main
  // page), not as a drop-down under the field.
  let { value = $bindable(""), defaults = [] as string[], placeholder = "", inline = false } = $props();

  let open = $state(false);
  let probes = $state<ServerProbe[]>([]);
  let probing = $state(false);
  let scanning = $state(false);
  let wrap = $state<HTMLElement | null>(null);
  let lastRun = 0;
  let aliasTick = $state(0);
  // The public list, kept for offline starts.
  const PUB_KEY = "aqua.publicServers.v1";
  type Pub = { url: string; name: string | null; note: string | null };
  let pub = $state<Pub[]>((() => { try { return JSON.parse(localStorage.getItem(PUB_KEY) || "[]"); } catch { return []; } })());
  const pubOf = (url: string) => pub.find((x) => norm(x.url) === norm(url));

  const norm = (u: string) => u.trim().replace(/\/+$/, "");

  function candidates(extra: string[] = []): string[] {
    const list = [value, ...pub.map((x) => x.url), ...defaults, ...knownServers(), ...extra].map(norm).filter(Boolean);
    return [...new Set(list)];
  }

  async function probe(urls: string[]) {
    if (!urls.length) return;
    probing = true;
    try {
      const got = await api.probeServers(urls);
      const by = new Map(probes.map((p) => [p.url, p]));
      for (const p of got) by.set(p.url, p);
      probes = [...by.values()];
    } catch { /* browser preview / old backend */ }
    probing = false;
  }

  /** Relays the answering ones know about: their own outside address (a
   *  home relay found on Wi-Fi tells its white IP) and their federation —
   *  and what those know, two hops deep. */
  async function learn(depth = 2) {
    const asked = new Set<string>();
    for (let hop = 0; hop < depth; hop++) {
      const ok = probes.filter((p) => p.ok && !asked.has(norm(p.url))).slice(0, 8);
      if (!ok.length) return;
      const more: string[] = [];
      await Promise.all(ok.map(async (p) => {
        asked.add(norm(p.url));
        try {
          const r = await api.fedPeers(p.url);
          if (r.self) { rememberAlias(p.url, r.self); aliasTick++; }
          more.push(...(r.self ? [r.self] : []), ...(r.servers ?? []));
        } catch { /* not federated */ }
      }));
      const fresh = [...new Set(more.map(norm))].filter((u) => u && !probes.some((p) => norm(p.url) === u || norm(p.url).replace(/:(80|443)$/, "") === u));
      if (!fresh.length) return;
      await probe(fresh);
    }
  }

  async function loadPublic() {
    try {
      const list = await api.publicServers();
      pub = list;
      try { localStorage.setItem(PUB_KEY, JSON.stringify(list)); } catch { /* storage blocked */ }
      const fresh = list.map((x) => norm(x.url)).filter((u) => !probes.some((p) => norm(p.url) === u));
      await probe(fresh);
    } catch { /* offline: the cached list was probed already */ }
  }

  async function scan() {
    scanning = true;
    try {
      const found = await api.scanLanRelays();
      await probe(found);
    } catch { /* no network */ }
    scanning = false;
  }

  async function refresh() {
    lastRun = Date.now();
    await probe(candidates());
    void learn();
    void scan().then(() => learn());
    void loadPublic().then(() => learn());
  }

  function show() {
    open = true;
    if (Date.now() - lastRun > 30_000) void refresh();
  }
  function pick(p: ServerProbe) {
    // Keep the scheme the relay answers on; drop a default port for neatness.
    value = p.url.replace(/^(https:\/\/[^/]+):443$/, "$1").replace(/^(http:\/\/[^/]+):80$/, "$1");
    if (!inline) open = false;
  }
  onMount(() => { if (inline) { open = true; void refresh(); } });
  function onFocusOut(e: FocusEvent) {
    if (inline) return;
    const to = e.relatedTarget as Node | null;
    if (!to || !wrap?.contains(to)) open = false;
  }

  const flag = (cc: string | null) =>
    cc && /^[A-Za-z]{2}$/.test(cc) ? String.fromCodePoint(...[...cc.toUpperCase()].map((c) => 0x1f1a5 + c.charCodeAt(0))) : "";

  function where(p: ServerProbe): string {
    if (p.scope === "loopback") return tr("this device");
    if (p.scope === "emulator") return tr("the PC hosting the emulator");
    if (p.scope === "lan") return tr("local network");
    if (p.country) return [flag(p.country_code), p.city, p.country].filter(Boolean).join(" ");
    return p.ip ? "" : tr("address not found");
  }

  /** The other address of the same relay that answers (the home address of
   *  a white IP the router doesn't loop back inside). */
  function twin(p: ServerProbe, all: ServerProbe[]): ServerProbe | undefined {
    void aliasTick;
    const al = aliasesOf(p.url);
    return all.find((q) => q !== p && al.includes(norm(q.url)));
  }
  const nameOf = (p: ServerProbe) => { const t2 = twin(p, probes); return pubOf(p.url) ?? (t2 && pubOf(t2.url)); };
  // A relay seen at both its addresses is one row: the one that answers here.
  let sorted = $derived(
    probes.filter((p) => p.ok || !twin(p, probes)?.ok).sort((a, b) => Number(b.ok) - Number(a.ok) || (a.ms ?? 1e9) - (b.ms ?? 1e9) || a.url.localeCompare(b.url)),
  );
  let current = $derived(norm(value));
  const isCur = (p: ServerProbe) => [p.url, ...(twin(p, probes)?.ok ? [twin(p, probes)!.url] : [])]
    .some((u) => norm(u) === current || norm(u).replace(/:(80|443)$/, "") === current);
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="sp" class:inline bind:this={wrap} onfocusout={onFocusOut}>
  <div class="sp-in">
    <input bind:value {placeholder} onfocus={show} onclick={show} autocomplete="off" spellcheck="false"
           onkeydown={(e) => { if (e.key === "Escape") open = false; else if (e.key === "ArrowDown") show(); }} />
    <button type="button" class="sp-tg" class:on={open} tabindex="-1" aria-label={$t("Known servers")} title={$t("Known servers")}
            onclick={() => (open ? (open = false) : show())}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
    </button>
  </div>
  {#if open}
    <div class="sp-list" role="listbox">
      <div class="sp-head">
        <span>{$t("Servers")}</span>
        {#if probing || scanning}<span class="sp-busy">{$t(scanning ? "Searching the local network…" : "Checking…")}</span>{/if}
        <button type="button" class="sp-re" onclick={refresh} disabled={probing || scanning} title={$t("Check again")}>↻</button>
      </div>
      {#each sorted as p (p.url)}
        <button type="button" class="sp-row" class:cur={isCur(p)} class:down={!p.ok} role="option" aria-selected={isCur(p)} onclick={() => pick(p)}>
          <span class="dot" class:ok={p.ok}></span>
          <span class="sp-main">
            <span class="sp-url">{#if nameOf(p)?.name}<b class="sp-name">{nameOf(p)?.name}</b>{/if}{p.url.replace(/^https?:\/\//, "")}</span>
            <span class="sp-sub">
              {#if p.ip && p.ip !== p.host}<span class="mono">{p.ip}</span>{/if}
              {#if nameOf(p)}<span class="pub">{nameOf(p)?.note || $t("public server")}</span>{/if}
              {#if p.ok && twin(p, probes) && !twin(p, probes)?.ok}<span>{$t("from outside: {a}", { a: twin(p, probes)!.url.replace(/^https?:\/\//, "") })}</span>{/if}
              {#if where(p)}<span>{where(p)}</span>{/if}
              {#if p.url.startsWith("https")}<span class="tls">🔒 TLS</span>{/if}
            </span>
          </span>
          <span class="sp-meta">
            {#if p.ok}
              <span class="ms" class:slow={(p.ms ?? 0) > 400}>{p.ms} {$t("ms")}</span>
              {#if p.online != null}<span class="on">{p.online} {$t("online")}</span>{/if}
            {:else}
              <span class="off">{$t(probing ? "…" : "no answer")}</span>
            {/if}
          </span>
        </button>
      {:else}
        <div class="sp-empty">{$t(probing ? "Checking…" : "No servers yet — type an address")}</div>
      {/each}
      <div class="sp-foot">{$t("Location is approximate (by IP).")}</div>
    </div>
  {/if}
</div>

<style>
  .sp { position: relative; }
  .sp.inline .sp-list { position: static; margin-top: 8px; max-height: min(320px, 42vh); box-shadow: none; animation: none;
    background: color-mix(in srgb, var(--bg-surface) 60%, transparent); }
  .sp.inline .sp-tg { display: none; }
  .sp-in { position: relative; display: flex; }
  .sp-in input {
    flex: 1; min-width: 0; box-sizing: border-box;
    background: var(--bg-input); border: 1px solid var(--border); border-radius: 10px;
    color: var(--fg); font-family: inherit; font-size: 14px; padding: 9px 36px 9px 12px; outline: none;
  }
  .sp-in input:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  .sp-tg {
    position: absolute; right: 4px; top: 50%; transform: translateY(-50%);
    width: 28px; height: 28px; border: none; border-radius: 8px; background: transparent;
    color: var(--fg-dim); cursor: pointer; display: grid; place-items: center; transition: transform .18s, color .15s;
  }
  .sp-tg svg { width: 16px; height: 16px; }
  .sp-tg.on { color: var(--accent); transform: translateY(-50%) rotate(180deg); }
  .sp-list {
    position: absolute; left: 0; right: 0; top: calc(100% + 6px); z-index: 40;
    max-height: min(360px, 55vh); overflow-y: auto; padding: 6px;
    background: var(--bg-shell, var(--bg-surface)); border: 1px solid var(--border-strong); border-radius: 14px;
    box-shadow: 0 22px 50px -16px rgba(0, 0, 0, 0.65);
    animation: sp-in .18s cubic-bezier(.2, .9, .25, 1.1);
  }
  @keyframes sp-in { from { opacity: 0; transform: translateY(-6px) scale(.98); } }
  .sp-head { display: flex; align-items: center; gap: 8px; padding: 4px 8px 6px; font-size: 11.5px; font-weight: 700; color: var(--fg-dim); text-transform: uppercase; letter-spacing: .04em; }
  .sp-busy { text-transform: none; letter-spacing: 0; font-weight: 500; color: var(--accent); }
  .sp-re { margin-left: auto; border: none; background: none; color: var(--fg-dim); cursor: pointer; font-size: 15px; }
  .sp-re:hover:not(:disabled) { color: var(--accent); }
  .sp-row {
    width: 100%; display: flex; align-items: center; gap: 10px; padding: 9px 10px; border: none; border-radius: 10px;
    background: transparent; color: var(--fg); font: inherit; text-align: left; cursor: pointer;
  }
  .sp-row:hover { background: var(--bg-hover); }
  .sp-row.cur { background: var(--accent-soft); }
  .sp-row.down .sp-url { color: var(--fg-muted); }
  .dot { width: 8px; height: 8px; border-radius: 50%; background: var(--fg-dim); flex-shrink: 0; opacity: .6; }
  .dot.ok { background: #3BD671; opacity: 1; box-shadow: 0 0 8px #3BD67199; }
  .sp-main { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 2px; }
  .sp-url { font-weight: 600; font-size: 13.5px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sp-sub { display: flex; flex-wrap: wrap; gap: 3px 10px; font-size: 11.5px; color: var(--fg-dim); }
  .mono { font-family: var(--mono, ui-monospace, monospace); }
  .tls { color: var(--accent); }
  .pub { color: var(--accent); }
  .sp-name { margin-right: 8px; }
  .sp-meta { display: flex; flex-direction: column; align-items: flex-end; gap: 2px; font-size: 11.5px; flex-shrink: 0; }
  .ms { color: #3BD671; font-weight: 700; }
  .ms.slow { color: #F0B232; }
  .on { color: var(--fg-dim); }
  .off { color: var(--fg-dim); }
  .sp-empty { padding: 14px 10px; font-size: 12.5px; color: var(--fg-dim); text-align: center; }
  .sp-foot { padding: 6px 10px 2px; font-size: 10.5px; color: var(--fg-dim); }
</style>
