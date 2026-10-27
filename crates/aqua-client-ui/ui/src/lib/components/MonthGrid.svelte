<script lang="ts">
  /** A month calendar: pick a day; days with something on them get a dot. */
  import { lang } from "../i18n";

  let { selected = $bindable(new Date()), marks = new Set<string>(), onpick }: {
    selected?: Date; marks?: Set<string>; onpick?: (d: Date) => void;
  } = $props();

  let view = $state(new Date(selected.getFullYear(), selected.getMonth(), 1));
  const key = (d: Date) => `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
  const loc = () => ($lang === "ru" ? "ru-RU" : undefined);

  let title = $derived(view.toLocaleDateString(loc(), { month: "long", year: "numeric" }));
  let weekdays = $derived.by(() => {
    // Monday first.
    const base = new Date(2024, 0, 1); // a Monday
    return Array.from({ length: 7 }, (_, i) => new Date(base.getTime() + i * 86400_000).toLocaleDateString(loc(), { weekday: "short" }).slice(0, 2));
  });
  let cells = $derived.by(() => {
    const first = new Date(view.getFullYear(), view.getMonth(), 1);
    const shift = (first.getDay() + 6) % 7;
    const start = new Date(first.getFullYear(), first.getMonth(), 1 - shift);
    return Array.from({ length: 42 }, (_, i) => new Date(start.getFullYear(), start.getMonth(), start.getDate() + i));
  });
  const today = key(new Date());

  function pick(d: Date) {
    selected = new Date(d.getFullYear(), d.getMonth(), d.getDate(), selected.getHours(), selected.getMinutes());
    onpick?.(selected);
  }
</script>

<div class="mg">
  <div class="mg-head">
    <button class="mg-nav" onclick={() => (view = new Date(view.getFullYear(), view.getMonth() - 1, 1))} aria-label="‹">‹</button>
    <span class="mg-title">{title}</span>
    <button class="mg-nav" onclick={() => (view = new Date(view.getFullYear(), view.getMonth() + 1, 1))} aria-label="›">›</button>
  </div>
  <div class="mg-grid">
    {#each weekdays as w, i (i)}<span class="mg-wd">{w}</span>{/each}
    {#each cells as d (d.getTime())}
      <button class="mg-day" class:other={d.getMonth() !== view.getMonth()} class:today={key(d) === today}
        class:sel={key(d) === key(selected)} class:mark={marks.has(key(d))} onclick={() => pick(d)}>{d.getDate()}</button>
    {/each}
  </div>
</div>

<style>
  .mg { display: flex; flex-direction: column; gap: 6px; user-select: none; }
  .mg-head { display: flex; align-items: center; gap: 6px; }
  .mg-title { flex: 1; text-align: center; font-weight: 700; font-size: 13.5px; text-transform: capitalize; }
  .mg-nav { width: 28px; height: 28px; border-radius: calc(8px * var(--rk, 1)); border: none; background: var(--bg-hover); color: var(--fg); font-size: 16px; cursor: pointer; }
  .mg-nav:hover { background: var(--bg-active); }
  .mg-grid { display: grid; grid-template-columns: repeat(7, 1fr); gap: 2px; }
  .mg-wd { text-align: center; font-size: 10.5px; font-weight: 700; color: var(--fg-dim); text-transform: uppercase; padding: 2px 0 4px; }
  .mg-day { position: relative; aspect-ratio: 1; border: none; border-radius: calc(9px * var(--rk, 1)); background: none; color: var(--fg); font: inherit; font-size: 12.5px; cursor: pointer; }
  .mg-day:hover { background: var(--bg-hover); }
  .mg-day.other { color: var(--fg-dim); opacity: 0.6; }
  .mg-day.today { color: var(--accent); font-weight: 800; }
  .mg-day.sel { background: var(--accent); color: #06070C; font-weight: 800; }
  .mg-day.mark::after { content: ""; position: absolute; left: 50%; bottom: 3px; width: 4px; height: 4px; margin-left: -2px; border-radius: 50%; background: var(--accent); }
  .mg-day.sel.mark::after { background: #06070C; }
</style>
