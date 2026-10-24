<script lang="ts">
  /**
   * The rail: a column of sections on desktop; on phones a floating tab bar
   * like the new Telegram — equal buttons, a highlight that slides between
   * them, press-and-hold to drag the highlight and drop it on a section, and
   * (as in exteraGram) it slips away while you scroll down and comes back
   * when you scroll up.
   */
  import { onMount } from "svelte";
  import { appIconUrl } from "../appIcon";
  import { t } from "../i18n";
  import { icons } from "../icons";
  import { activeTab, type NavTab } from "../stores";
  import { openSettings } from "../panels/settings";

  type Item = { id: string; label: string; svg: string; tab?: NavTab; mobileOnly?: boolean; desktopOnly?: boolean };
  // Phones get four: Chats, Files, Contacts (with recent calls inside, like
  // Telegram) and Settings; your profile is the avatar on the main page.
  const all: Item[] = [
    { id: "chats", label: "Chats", svg: icons.chats, tab: "chats" },
    { id: "calls", label: "Calls", svg: icons.phone, tab: "calls", desktopOnly: true },
    { id: "files", label: "Files", svg: icons.files, tab: "files" },
    { id: "contacts", label: "Contacts", svg: icons.contacts, tab: "contacts" },
    { id: "settings", label: "Settings", svg: icons.settings, mobileOnly: true },
  ];
  const mq = typeof window !== "undefined" ? matchMedia("(max-width: 720px)") : null;
  let phone = $state(!!mq?.matches);
  onMount(() => {
    const on = () => (phone = !!mq?.matches);
    mq?.addEventListener("change", on);
    return () => mq?.removeEventListener("change", on);
  });
  const items = $derived(all.filter((i) => (phone ? !i.desktopOnly : !i.mobileOnly)));

  function activate(it: Item) {
    if (it.tab) activeTab.set(it.tab);
    else if (it.id === "settings") openSettings();
  }
  // On phones the calls list lives under Contacts.
  const shownTab = $derived(phone && $activeTab === "calls" ? "contacts" : $activeTab);
  const activeIndex = $derived(Math.max(0, items.findIndex((i) => i.tab === shownTab)));

  // ---- press and hold: drag the highlight, drop it on a section ----
  let bar: HTMLElement | undefined = $state();
  let dragX = $state<number | null>(null);   // pill centre while dragging (px in the bar)
  let holdTimer: ReturnType<typeof setTimeout> | null = null;
  let pressX = 0, pressY = 0, swallowClick = false, lastIdx = -1;
  const mobile = () => phone;
  function idxAt(x: number): number {
    if (!bar) return 0;
    const r = bar.getBoundingClientRect();
    const w = (r.width - 8) / items.length;
    return Math.max(0, Math.min(items.length - 1, Math.floor((x - r.left - 4) / w)));
  }
  function down(e: PointerEvent) {
    if (!mobile() || !bar) return;
    pressX = e.clientX; pressY = e.clientY;
    holdTimer = setTimeout(() => {
      holdTimer = null;
      const r = bar!.getBoundingClientRect();
      dragX = pressX - r.left;
      lastIdx = idxAt(pressX);
      try { navigator.vibrate?.(12); } catch { /* none */ }
      try { bar!.setPointerCapture(e.pointerId); } catch { /* gone */ }
    }, 260);
  }
  function move(e: PointerEvent) {
    if (holdTimer && Math.hypot(e.clientX - pressX, e.clientY - pressY) > 10) { clearTimeout(holdTimer); holdTimer = null; }
    if (dragX == null || !bar) return;
    e.preventDefault();
    const r = bar.getBoundingClientRect();
    dragX = Math.max(30, Math.min(r.width - 30, e.clientX - r.left));
    const i = idxAt(e.clientX);
    if (i !== lastIdx) { lastIdx = i; try { navigator.vibrate?.(6); } catch { /* none */ } }
  }
  function up(e: PointerEvent) {
    if (holdTimer) { clearTimeout(holdTimer); holdTimer = null; }
    if (dragX == null) return;
    const it = items[idxAt(e.clientX)];
    dragX = null;
    swallowClick = true;
    setTimeout(() => (swallowClick = false), 0);
    activate(it);
  }
  function cancel() { if (holdTimer) clearTimeout(holdTimer); holdTimer = null; dragX = null; }

  // ---- exteraGram: hide while scrolling down, show when scrolling up ----
  let hidden = $state(false);
  onMount(() => {
    const last = new WeakMap<EventTarget, number>();
    const onScroll = (e: Event) => {
      if (!mobile()) return;
      const el = e.target as HTMLElement;
      if (!el || !(el instanceof HTMLElement) || el.closest(".main-pane, .rail, [role=dialog]")) return;
      const y = el.scrollTop;
      const prev = last.get(el) ?? y;
      last.set(el, y);
      if (y < 40) hidden = false;
      else if (y - prev > 6) hidden = true;
      else if (prev - y > 6) hidden = false;
    };
    document.addEventListener("scroll", onScroll, { capture: true, passive: true });
    return () => document.removeEventListener("scroll", onScroll, { capture: true });
  });
  // A new section starts with the bar in view.
  $effect(() => { void $activeTab; hidden = false; });
  $effect(() => { document.documentElement.classList.toggle("dock-hidden", hidden); });
</script>

<aside
  class="rail"
  class:hidden
  class:dragging={dragX != null}
  bind:this={bar}
  style="--n: {items.length}; --i: {activeIndex}; {dragX != null ? `--drag-x: ${dragX}px;` : ''}"
  onpointerdown={down}
  onpointermove={move}
  onpointerup={up}
  onpointercancel={cancel}
  oncontextmenu={(e) => { if (mobile()) e.preventDefault(); }}
>
  <img class="brand" src={$appIconUrl} alt={$t("Aquamarine")} title={$t("Aquamarine")} draggable="false" />
  <div class="rail-divider"></div>
  <span class="pill" aria-hidden="true"></span>

  {#each items as item, i (item.id)}
    <button
      class="nav-btn"
      class:mobile-only={item.mobileOnly}
      class:active={item.tab ? shownTab === item.tab : false}
      class:hover={dragX != null && lastIdx === i}
      data-label={$t(item.label)}
      onclick={(e) => { if (swallowClick) { e.preventDefault(); return; } activate(item); }}
    >
      {@html item.svg}
      <span class="nav-lbl">{$t(item.label)}</span>
    </button>
    {#if item.id === "contacts"}<div class="rail-spacer"></div>{/if}
  {/each}
</aside>

<style>
  .nav-btn.mobile-only { display: none; }
  .nav-btn.mobile-only:first-of-type { margin-top: auto; }
  .rail {
    background: var(--bg-rail);
    border-right: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 14px 0;
    gap: 4px;
    position: relative;
  }
  .pill { display: none; }
  .brand {
    width: 40px; height: 40px;
    border-radius: 12px;
    display: block; object-fit: cover;
    margin-bottom: 10px;
    box-shadow: 0 0 24px -4px var(--accent-glow);
  }
  .rail-divider { width: 32px; height: 1px; background: var(--border); margin: 4px 0 10px; }
  .nav-btn {
    width: 44px; height: 44px;
    display: grid; place-items: center;
    background: transparent;
    border: none;
    border-radius: 12px;
    color: var(--fg-muted);
    cursor: pointer;
    transition: background-color .15s ease, color .15s ease;
    position: relative;
  }
  .nav-btn::before {
    content: '';
    position: absolute;
    left: -14px; top: 50%;
    width: 3px; height: 0;
    background: var(--accent);
    border-radius: 0 3px 3px 0;
    transform: translateY(-50%);
    transition: height 0.2s ease;
    box-shadow: 0 0 8px var(--accent-glow);
  }
  .nav-btn:hover { background: var(--bg-hover); color: var(--fg); }
  .nav-btn.active { color: var(--accent); background: var(--accent-soft); }
  .nav-btn.active::before { height: 22px; }
  .nav-btn :global(svg) { width: 20px; height: 20px; }
  .rail-spacer { flex: 1; }
  .nav-btn[data-label]:hover::after {
    content: attr(data-label);
    position: absolute;
    left: calc(100% + 14px);
    top: 50%;
    transform: translateY(-50%);
    background: var(--bg-surface-2);
    border: 1px solid var(--border-strong);
    color: var(--fg);
    padding: 6px 10px;
    border-radius: 8px;
    font-size: 12px;
    font-weight: 500;
    white-space: nowrap;
    box-shadow: var(--shadow-lift);
    z-index: 100;
    pointer-events: none;
  }
  .nav-lbl { display: none; }

  /* ---- Phones: a floating tab bar, like the new Telegram ---- */
  @media (max-width: 720px) {
    .rail {
      flex-direction: row;
      align-items: stretch;
      position: fixed;
      left: 50%; width: min(340px, calc(100% - 48px)); transform: translateX(-50%);
      bottom: calc(8px + env(safe-area-inset-bottom, 0px));
      height: 48px;
      padding: 4px;
      gap: 0;
      border: 1px solid var(--border-strong);
      border-radius: 24px;
      /* Frosted glass: the list shows through, softly. */
      background: color-mix(in srgb, var(--bg-surface, #12151d) 58%, transparent);
      backdrop-filter: blur(22px) saturate(1.6);
      -webkit-backdrop-filter: blur(22px) saturate(1.6);
      box-shadow: 0 18px 40px -18px rgba(0,0,0,.8), 0 1px 0 rgba(255,255,255,.05) inset;
      z-index: 60;
      touch-action: pan-y;
      user-select: none;
      -webkit-user-select: none;
      transition: transform .38s cubic-bezier(.22,1,.36,1), opacity .3s ease;
    }
    .rail.hidden:not(.dragging) { transform: translateX(-50%) translateY(calc(100% + 24px)); opacity: 0; pointer-events: none; }
    .brand, .rail-divider, .rail-spacer { display: none; }
    .nav-btn.mobile-only { display: flex; margin-top: 0; }
    /* Every button the same size: the whole slot is the hitbox. */
    .nav-btn {
      flex: 1 1 0; min-width: 0;
      width: auto; height: 100%;
      flex-direction: column; justify-content: center; align-items: center;
      display: flex; gap: 1px;
      padding: 0;
      border-radius: 20px;
      background: transparent !important;
      z-index: 1;
    }
    .nav-btn::before, .nav-btn[data-label]:hover::after { display: none; }
    .nav-btn :global(svg) { width: 19px; height: 19px; }
    .nav-btn.active, .nav-btn.hover { color: var(--accent); }
    .nav-lbl { display: block; font-size: 10px; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; max-width: 100%; }
    /* The highlight slides under the active button… */
    .pill {
      display: block; position: absolute; top: 4px; bottom: 4px;
      width: calc((100% - 8px) / var(--n));
      left: calc(4px + var(--i) * (100% - 8px) / var(--n));
      border-radius: 20px;
      background: var(--accent-soft);
      box-shadow: 0 0 0 1px color-mix(in srgb, var(--accent) 22%, transparent) inset;
      transition: left .42s cubic-bezier(.22,1,.36,1), transform .25s cubic-bezier(.22,1,.36,1), width .2s ease;
    }
    /* …and follows the finger while held. */
    .rail.dragging .pill {
      left: calc(var(--drag-x) - (100% - 8px) / var(--n) / 2);
      transition: transform .25s cubic-bezier(.22,1,.36,1);
      transform: scale(1.08, 1.12);
      background: color-mix(in srgb, var(--accent) 24%, transparent);
      box-shadow: 0 8px 24px -8px var(--accent-glow), 0 0 0 1px color-mix(in srgb, var(--accent) 40%, transparent) inset;
    }
  }
</style>
