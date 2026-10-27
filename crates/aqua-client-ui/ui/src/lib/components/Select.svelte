<script lang="ts" generics="V extends string | number">
  /**
   * Our own drop-down list instead of <select>: the system one is a grey
   * GTK / Windows menu that ignores the theme. The list opens over
   * everything (attached to <body>), so dialogs with overflow don't clip it.
   */
  import { onDestroy, tick } from "svelte";

  interface Opt { value: V; label: string }
  let {
    value = $bindable(),
    options,
    onchange,
    label = "",
    class: cls = "",
    disabled = false,
    compact = false,
  }: {
    value?: V;
    options: Opt[];
    onchange?: (v: V) => void;
    label?: string;
    class?: string;
    disabled?: boolean;
    compact?: boolean;
  } = $props();

  let open = $state(false);
  let btn = $state<HTMLButtonElement | null>(null);
  let list = $state<HTMLDivElement | null>(null);
  let hi = $state(0);
  let pos = $state({ x: 0, y: 0, w: 0, up: false, max: 280 });
  const current = $derived(options.find((o) => o.value === value));

  function place() {
    if (!btn) return;
    const r = btn.getBoundingClientRect();
    const below = window.innerHeight - r.bottom - 8;
    const above = r.top - 8;
    const up = below < 200 && above > below;
    pos = { x: r.left, y: up ? r.top - 4 : r.bottom + 4, w: r.width, up, max: Math.max(120, Math.min(320, up ? above : below)) };
  }
  async function show() {
    if (disabled) return;
    place();
    hi = Math.max(0, options.findIndex((o) => o.value === value));
    open = true;
    await tick();
    if (list) document.body.appendChild(list);
    list?.querySelector<HTMLElement>(`[data-i="${hi}"]`)?.scrollIntoView({ block: "center" });
    list?.focus();
  }
  function hide(refocus = true) {
    open = false;
    if (refocus) btn?.focus();
  }
  function pick(o: Opt) {
    value = o.value;
    onchange?.(o.value);
    hide();
  }
  function key(e: KeyboardEvent) {
    if (!open) {
      if (["Enter", " ", "ArrowDown", "ArrowUp"].includes(e.key)) { e.preventDefault(); void show(); }
      return;
    }
    if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); hide(); }
    else if (e.key === "ArrowDown") { e.preventDefault(); hi = Math.min(options.length - 1, hi + 1); scrollHi(); }
    else if (e.key === "ArrowUp") { e.preventDefault(); hi = Math.max(0, hi - 1); scrollHi(); }
    else if (e.key === "Enter" || e.key === " ") { e.preventDefault(); if (options[hi]) pick(options[hi]); }
    else if (e.key === "Tab") hide(false);
  }
  function scrollHi() { void tick().then(() => list?.querySelector<HTMLElement>(`[data-i="${hi}"]`)?.scrollIntoView({ block: "nearest" })); }

  $effect(() => {
    if (!open) return;
    const away = (e: Event) => {
      const t = e.target as Node;
      if (list?.contains(t) || btn?.contains(t)) return;
      hide(false);
    };
    const re = () => place();
    window.addEventListener("pointerdown", away, true);
    window.addEventListener("resize", re);
    window.addEventListener("scroll", re, true);
    return () => {
      window.removeEventListener("pointerdown", away, true);
      window.removeEventListener("resize", re);
      window.removeEventListener("scroll", re, true);
    };
  });
  onDestroy(() => list?.remove());
</script>

<button bind:this={btn} type="button" class="aq-select {cls}" class:compact class:open {disabled} aria-haspopup="listbox" aria-expanded={open} aria-label={label || undefined}
  onclick={() => (open ? hide() : void show())} onkeydown={key}>
  <span class="cur">{current?.label ?? ""}</span>
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="m6 9 6 6 6-6"/></svg>
</button>
{#if open}
  <div bind:this={list} class="aq-list" class:up={pos.up} role="listbox" tabindex="-1" onkeydown={key}
    style:left="{pos.x}px" style:top={pos.up ? "auto" : `${pos.y}px`} style:bottom={pos.up ? `${window.innerHeight - pos.y}px` : "auto"}
    style:min-width="{pos.w}px" style:max-height="{pos.max}px">
    {#each options as o, i (o.value)}
      <button type="button" class="aq-opt" class:on={o.value === value} class:hi={i === hi} data-i={i} role="option" aria-selected={o.value === value}
        onpointerenter={() => (hi = i)} onclick={() => pick(o)}>
        <span>{o.label}</span>
        {#if o.value === value}<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12.5l4.5 4.5L19 7.5"/></svg>{/if}
      </button>
    {/each}
  </div>
{/if}

<style>
  .aq-select {
    display: inline-flex; align-items: center; gap: 8px; justify-content: space-between;
    min-width: 0; max-width: 100%; padding: 8px 10px 8px 12px;
    background: var(--bg-input); color: var(--fg); border: 1px solid var(--border);
    border-radius: calc(10px * var(--rk, 1)); font: inherit; font-size: 13.5px; cursor: pointer; text-align: left;
    transition: border-color .15s, box-shadow .15s;
  }
  .aq-select.compact { padding: 5px 8px 5px 10px; font-size: 13px; }
  .aq-select:hover:not(:disabled) { border-color: var(--border-strong); }
  .aq-select:focus-visible, .aq-select.open { outline: none; border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  .aq-select:disabled { opacity: .5; cursor: default; }
  .cur { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .aq-select svg { width: 15px; height: 15px; flex-shrink: 0; color: var(--fg-dim); transition: transform .15s; }
  .aq-select.open svg { transform: rotate(180deg); color: var(--accent); }
  :global(.aq-list) {
    position: fixed; z-index: 10000; overflow-y: auto; padding: 4px;
    background: color-mix(in srgb, var(--accent) 5%, var(--bg-shell, #0d1117));
    border: 1px solid color-mix(in srgb, var(--accent) 22%, var(--border-strong, #2a2f3a));
    border-radius: calc(12px * var(--rk, 1));
    box-shadow: 0 18px 44px -12px rgba(0, 0, 0, .65);
    animation: aq-pop .12s ease-out; outline: none; scrollbar-width: thin;
  }
  :global(.aq-list.up) { animation-name: aq-pop-up; }
  @keyframes -global-aq-pop { from { opacity: 0; transform: translateY(-4px); } }
  @keyframes -global-aq-pop-up { from { opacity: 0; transform: translateY(4px); } }
  :global(.aq-opt) {
    display: flex; align-items: center; gap: 10px; justify-content: space-between; width: 100%;
    padding: 7px 10px; border: none; border-radius: calc(8px * var(--rk, 1)); background: none;
    color: var(--fg, #e6e8ee); font: inherit; font-size: 13.5px; text-align: left; cursor: pointer; white-space: nowrap;
  }
  :global(.aq-opt.hi) { background: var(--accent-soft); }
  :global(.aq-opt.on) { color: var(--accent); font-weight: 600; }
  :global(.aq-opt svg) { width: 15px; height: 15px; flex-shrink: 0; }
</style>
