<script lang="ts">
  import Select from "./Select.svelte";
  /**
   * Birthday: type it, or pick day / month / year from a small menu.
   * The value stays a readable string ("15 марта 1998" / "15 March 1998").
   */
  import { lang, t } from "../i18n";

  let { value = $bindable("") }: { value?: string } = $props();

  const RU = ["января", "февраля", "марта", "апреля", "мая", "июня", "июля", "августа", "сентября", "октября", "ноября", "декабря"];
  const RU_SHORT = ["янв", "фев", "мар", "апр", "май", "июн", "июл", "авг", "сен", "окт", "ноя", "дек"];
  const EN = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
  const now = new Date().getFullYear();
  const YEARS = Array.from({ length: 100 }, (_, i) => now - i);

  let open = $state(false);
  let day = $state(0), month = $state(-1), year = $state(0);
  let wrap = $state<HTMLDivElement | null>(null);

  function parse(s: string) {
    const v = s.trim().toLowerCase();
    const d = /\b(\d{1,2})\b/.exec(v);
    const y = /\b(\d{4})\b/.exec(v);
    let m = -1;
    [RU, EN, RU_SHORT].forEach((arr) => arr.forEach((name, i) => { if (m < 0 && v.includes(name.toLowerCase().slice(0, 3))) m = i; }));
    const num = /^(\d{1,2})[./-](\d{1,2})(?:[./-](\d{2,4}))?$/.exec(v);
    if (num) { day = +num[1]; month = +num[2] - 1; year = num[3] ? +(num[3].length === 2 ? "19" + num[3] : num[3]) : 0; return; }
    day = d ? +d[1] : 0; month = m; year = y ? +y[1] : 0;
  }
  function compose() {
    if (!day || month < 0) return;
    const names = $lang === "ru" ? RU : EN;
    value = `${day} ${names[month]}${year ? " " + year : ""}`;
  }
  function daysIn(m: number, y: number) { return m < 0 ? 31 : new Date(y || 2000, m + 1, 0).getDate(); }

  function toggle() { if (!open) parse(value); open = !open; }
  $effect(() => {
    if (!open) return;
    const h = (e: PointerEvent) => { if (wrap && !wrap.contains(e.target as Node) && !(e.target as Element).closest?.(".aq-list")) open = false; };
    const k = (e: KeyboardEvent) => { if (e.key === "Escape") { e.stopPropagation(); open = false; } };
    document.addEventListener("pointerdown", h, true);
    document.addEventListener("keydown", k, true);
    return () => { document.removeEventListener("pointerdown", h, true); document.removeEventListener("keydown", k, true); };
  });
</script>

<div class="bd" bind:this={wrap}>
  <input bind:value placeholder={$t("15 March 1998")} />
  <button type="button" class="bd-btn" onclick={toggle} title={$t("Pick a date")} aria-label={$t("Pick a date")} aria-expanded={open}>📅</button>
  {#if open}
    <div class="bd-pop" role="dialog">
      <div class="bd-months">
        {#each ($lang === "ru" ? RU_SHORT : EN.map((m) => m.slice(0, 3))) as m, i (i)}
          <button type="button" class:on={month === i} onclick={() => { month = i; if (day > daysIn(i, year)) day = daysIn(i, year); compose(); }}>{m}</button>
        {/each}
      </div>
      <div class="bd-days">
        {#each Array.from({ length: daysIn(month, year) }, (_, i) => i + 1) as d (d)}
          <button type="button" class:on={day === d} onclick={() => { day = d; compose(); }}>{d}</button>
        {/each}
      </div>
      <div class="bd-foot">
        <Select compact value={year} options={[{ value: 0, label: $t("No year") }, ...YEARS.map((y) => ({ value: y, label: String(y) }))]}
          onchange={(v) => { year = v; compose(); }} />
        <button type="button" class="bd-ok" onclick={() => { compose(); open = false; }}>{$t("Done")}</button>
      </div>
    </div>
  {/if}
</div>

<style>
  .bd { position: relative; display: flex; gap: 6px; }
  .bd input { flex: 1; min-width: 0; background: var(--bg-input); border: 1px solid var(--border); border-radius: calc(9px * var(--rk, 1)); color: var(--fg);
    font: inherit; font-size: 13.5px; padding: 9px 12px; outline: none; }
  .bd input:focus { border-color: var(--accent); }
  .bd-btn { flex-shrink: 0; width: 38px; border-radius: calc(9px * var(--rk, 1)); border: 1px solid var(--border); background: var(--bg-input); cursor: pointer; font-size: 16px; }
  .bd-btn:hover { border-color: var(--accent); }
  .bd-pop { position: absolute; z-index: 50; right: 0; top: calc(100% + 6px); width: 280px; padding: 10px; border-radius: calc(14px * var(--rk, 1));
    background: var(--bg-shell); border: 1px solid var(--border-strong); box-shadow: 0 20px 50px -14px rgba(0,0,0,.7);
    display: flex; flex-direction: column; gap: 8px; animation: bd-in .2s cubic-bezier(.2,.9,.25,1.1); }
  @keyframes bd-in { from { opacity: 0; transform: translateY(-4px) scale(.98); } }
  .bd-months { display: grid; grid-template-columns: repeat(4, 1fr); gap: 4px; }
  .bd-days { display: grid; grid-template-columns: repeat(7, 1fr); gap: 3px; }
  .bd-pop button { border: 1px solid transparent; background: var(--bg-input); color: var(--fg); border-radius: calc(7px * var(--rk, 1)); padding: 5px 0; font: inherit; font-size: 12px; cursor: pointer; }
  .bd-pop button:hover { border-color: var(--border-strong); }
  .bd-pop button.on { background: var(--accent); color: #06070c; font-weight: 800; }
  .bd-foot { display: flex; gap: 6px; }
  .bd-foot :global(.aq-select) { flex: 1; }
  .bd-pop .bd-ok { padding: 5px 14px; background: var(--accent-soft); color: var(--accent); font-weight: 800; }
</style>
