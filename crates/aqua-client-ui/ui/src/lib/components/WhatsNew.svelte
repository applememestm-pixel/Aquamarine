<script module lang="ts">
  import { writable, get } from "svelte/store";
  declare const __AQ_NOTES__: { version: string; date: string; md: string };
  export const NOTES = typeof __AQ_NOTES__ !== "undefined" ? __AQ_NOTES__ : { version: "", date: "", md: "" };
  const open = writable(false);
  const SEEN = "aqua.whatsNew.seen";
  export function openWhatsNew() { if (NOTES.md) open.set(true); }
  /** After an update (not on the very first start): show this version's
   *  notes once, unless turned off in Settings → Updates. */
  export function maybeShowWhatsNew(enabled: boolean) {
    if (!NOTES.version || document.documentElement.classList.contains("popout")) return;
    let seen = "";
    try { seen = localStorage.getItem(SEEN) ?? ""; localStorage.setItem(SEEN, NOTES.version); } catch { return; }
    if (seen && seen !== NOTES.version && enabled && NOTES.md) setTimeout(() => open.set(true), 1200);
  }
  export function whatsNewOpen() { return get(open); }
</script>

<script lang="ts">
  import { t, lang } from "../i18n";
  import { onBack } from "../back";
  import { api } from "../tauri";
  import { openSettings } from "../panels/settings";
  import { LINKS as ALL } from "../links";
  // One of each place (GitHub, Telegram, X, YouTube).
  const LINKS = ALL.filter((l, i) => ALL.findIndex((x) => x.label === l.label) === i);

  function close() { open.set(false); }
  $effect(() => { if ($open) return onBack(close); });
  $effect(() => {
    if (!$open) return;
    const k = (e: KeyboardEvent) => { if (e.key === "Escape") close(); };
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  });

  /** The release notes' Markdown, just the parts they use: headings, lists
   *  (nested by two spaces), **bold**, `code`, [links](https://…). */
  function md(src: string): string {
    const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
    const inline = (s: string) => esc(s)
      .replace(/`([^`]+)`/g, "<code>$1</code>")
      .replace(/\*\*([^*]+)\*\*/g, "<b>$1</b>")
      .replace(/\[([^\]]+)\]\((https?:\/\/[^)\s]+)\)/g, '<a href="$2" data-ext>$1</a>');
    const out: string[] = [];
    let depth = 0;
    const closeLists = (to: number) => { while (depth > to) { out.push("</ul>"); depth--; } };
    for (const raw of src.split("\n")) {
      const line = raw.replace(/\s+$/, "");
      const li = /^(\s*)- (.*)$/.exec(line);
      if (li) {
        const d = Math.floor(li[1].length / 2) + 1;
        while (depth < d) { out.push("<ul>"); depth++; }
        closeLists(d);
        out.push(`<li>${inline(li[2])}</li>`);
        continue;
      }
      closeLists(0);
      if (/^# /.test(line)) continue; // the title is in the header
      const h = /^(#{2,3}) (.*)$/.exec(line);
      if (h) { out.push(`<h${h[1].length + 1}>${inline(h[2])}</h${h[1].length + 1}>`); continue; }
      if (line.trim()) out.push(`<p>${inline(line)}</p>`);
    }
    closeLists(0);
    return out.join("");
  }
  const html = $derived(md(NOTES.md));
  const date = $derived(NOTES.date ? new Date(NOTES.date + "T12:00:00").toLocaleDateString($lang === "ru" ? "ru-RU" : "en-GB", { day: "numeric", month: "long", year: "numeric" }) : "");
  function click(e: MouseEvent) {
    const a = (e.target as Element).closest?.("a[data-ext]") as HTMLAnchorElement | null;
    if (a) { e.preventDefault(); void api.openExternal(a.href).catch(() => {}); }
  }

</script>

{#if $open}
  <div class="wn-bg" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) close(); }}>
    <div class="wn" data-phone-full role="dialog" aria-modal="true" aria-label={$t("What's new")}>
      <header>
        <button data-x class="x" onclick={close} aria-label={$t("Close")}>×</button>
        <div class="ttl"><b>{$t("What's new")}</b>{#if date}<small>{date}</small>{/if}</div>
      </header>
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <div class="body" onclick={click}>
        <div class="art">
          <span class="v">{NOTES.version}</span>
          <span class="s">Aquamarine</span>
          <i class="b1"></i><i class="b2"></i><i class="b3"></i>
        </div>
        <div class="md">{@html html}</div>
        <p class="off">{$t("Don't want this window after updates? Turn it off in Settings → Updates.")}
          <button class="lnk" onclick={() => { close(); openSettings("updates"); }}>{$t("Open settings")}</button></p>
      </div>
      <footer>
        {#each LINKS as l (l.url)}
          <button onclick={() => void api.openExternal(l.url).catch(() => {})} title={l.label} aria-label={l.label}>
            <svg viewBox="0 0 24 24">{@html l.icon}</svg>
          </button>
        {/each}
      </footer>
    </div>
  </div>
{/if}

<style>
  .wn-bg { position: fixed; inset: 0; z-index: 900; background: rgba(4, 6, 10, .62); backdrop-filter: blur(6px); display: grid; place-items: center; padding: 16px; animation: wn-fade .2s ease-out; }
  @keyframes wn-fade { from { opacity: 0; } }
  .wn { width: min(560px, 100%); max-height: min(86vh, 900px); display: flex; flex-direction: column; overflow: hidden;
    background: var(--bg-shell); border: 1px solid var(--border-strong); border-radius: calc(18px * var(--rk, 1));
    box-shadow: 0 40px 90px -20px rgba(0,0,0,.75); animation: wn-pop .3s cubic-bezier(.2,.9,.25,1.05); }
  @keyframes wn-pop { from { opacity: 0; transform: translateY(16px) scale(.97); } }
  header { position: relative; padding: 14px 52px; border-bottom: 1px solid var(--border); text-align: center; }
  .ttl { display: flex; flex-direction: column; } .ttl b { font-size: 18px; } .ttl small { color: var(--fg-dim); font-size: 12.5px; }
  .x { position: absolute; left: 12px; top: 50%; transform: translateY(-50%); width: 34px; height: 34px; border: none; background: none; color: var(--fg); font-size: 26px; cursor: pointer; border-radius: 50%; }
  .x:hover { background: var(--bg-hover); }
  .body { overflow-y: auto; padding: 18px 22px 8px; overscroll-behavior: contain; }
  .art { position: relative; height: 170px; border-radius: calc(14px * var(--rk, 1)); overflow: hidden; display: flex; flex-direction: column; align-items: center; justify-content: center;
    background: radial-gradient(120% 140% at 0% 0%, var(--accent) 0%, transparent 55%), radial-gradient(120% 140% at 100% 100%, var(--accent-2) 0%, transparent 60%), #0b1020; color: #fff; }
  .art .v { font-size: 54px; font-weight: 900; letter-spacing: -0.03em; text-shadow: 0 8px 30px rgba(0,0,0,.35); z-index: 1; }
  .art .s { font-size: 13px; font-weight: 700; letter-spacing: .3em; text-transform: uppercase; opacity: .85; z-index: 1; }
  .art i { position: absolute; border-radius: 50%; filter: blur(2px); opacity: .5; animation: float 7s ease-in-out infinite; }
  .art .b1 { width: 90px; height: 90px; left: 8%; top: 18%; background: rgba(255,255,255,.18); }
  .art .b2 { width: 50px; height: 50px; right: 12%; top: 12%; background: rgba(255,255,255,.22); animation-delay: -2s; }
  .art .b3 { width: 130px; height: 130px; right: -30px; bottom: -40px; background: rgba(255,255,255,.12); animation-delay: -4s; }
  @keyframes float { 50% { transform: translateY(-10px) scale(1.06); } }
  .md { font-size: 15px; line-height: 1.55; color: var(--fg-muted); }
  .md :global(h3) { color: var(--accent); text-transform: uppercase; font-size: 17px; letter-spacing: .02em; margin: 22px 0 8px; }
  .md :global(h4) { color: var(--fg); font-size: 15px; margin: 16px 0 6px; }
  .md :global(p) { margin: 12px 0; }
  .md :global(ul) { margin: 6px 0; padding-left: 20px; } .md :global(li) { margin: 4px 0; }
  .md :global(b) { color: var(--fg); }
  .md :global(code) { font-family: var(--font-mono); font-size: 12.5px; background: var(--bg-input); padding: 1px 5px; border-radius: 5px; }
  .md :global(a) { color: var(--accent); }
  .off { font-size: 12.5px; color: var(--fg-dim); border-top: 1px solid var(--border); padding-top: 12px; margin-top: 18px; }
  .lnk { border: none; background: none; color: var(--accent); font: inherit; cursor: pointer; padding: 0; text-decoration: underline; }
  footer { display: flex; justify-content: center; gap: 28px; padding: 12px; border-top: 1px solid var(--border); }
  footer button { width: 40px; height: 40px; border-radius: 50%; border: none; background: none; color: var(--fg-muted); cursor: pointer; display: grid; place-items: center; transition: color .15s, transform .15s; }
  footer button:hover { color: var(--accent); transform: translateY(-1px); }
  footer svg { width: 24px; height: 24px; }
  @media (max-width: 720px) {
    .wn-bg { padding: 0; }
    .wn { width: 100%; height: 100%; max-height: none; border-radius: 0; border: none; padding-top: env(safe-area-inset-top, 0px); padding-bottom: env(safe-area-inset-bottom, 0px); }
  }
</style>
