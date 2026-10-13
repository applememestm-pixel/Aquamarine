<script module lang="ts">
  import { writable, derived } from "svelte/store";

  export interface GalleryItem {
    url: string;
    kind: "image" | "video";
    name?: string;
    path?: string | null;
    size?: number;
    from?: string;
    time?: string;
  }
  /** Back-compat alias for single-item callers. */
  export type LightboxItem = GalleryItem;

  interface GalleryState { items: GalleryItem[]; index: number }

  export const gallery = writable<GalleryState | null>(null);
  /** Currently shown item (or null when closed). */
  export const lightbox = derived(gallery, (g) => (g ? g.items[g.index] ?? null : null));

  let enteredFullscreen = false;

  export function openGallery(items: GalleryItem[], index: number): void {
    if (!items || items.length === 0) return;
    const i = Math.max(0, Math.min(items.length - 1, Math.floor(index) || 0));
    gallery.set({ items: [...items], index: i });
  }

  export function openLightbox(item: GalleryItem): void {
    openGallery([item], 0);
  }

  export function closeLightbox(): void {
    gallery.set(null);
    if (enteredFullscreen && typeof document !== "undefined" && document.fullscreenElement) {
      document.exitFullscreen().catch(() => {});
    }
    enteredFullscreen = false;
  }

  function setFullscreenFlag(v: boolean) { enteredFullscreen = v; }
</script>

<script lang="ts">
  import { onMount, tick } from "svelte";
  import { save } from "@tauri-apps/plugin-dialog";
  import MediaPlayer from "./MediaPlayer.svelte";
  import { showToast } from "./Toast.svelte";
  import { api } from "../tauri";
  import { t, tr } from "../i18n";

  const ZOOM_STEP = 1.15;
  const BTN_STEP = 1.25;
  const MAX_ZOOM = 8;
  const MIN_ZOOM = 0.1;
  const FIT_PAD = 32;

  // ── Current item ────────────────────────────────────────────
  const items = $derived($gallery?.items ?? []);
  const index = $derived($gallery?.index ?? 0);
  const total = $derived(items.length);
  const cur = $derived<GalleryItem | null>($gallery ? items[index] ?? null : null);
  const isImage = $derived(cur?.kind === "image");

  // ── Per-item view state ─────────────────────────────────────
  let loading = $state(true);
  let failed = $state(false);
  let settled = $state(false);       // enables transform transitions after first layout
  let natW = $state(0);
  let natH = $state(0);
  let rot = $state(0);               // cumulative degrees (multiples of 90)
  let fitMode = $state(true);
  let zoomState = $state(1);
  let tx = $state(0);
  let ty = $state(0);
  let dragging = $state(false);
  let isFs = $state(false);
  let busy = $state(false);

  let stageEl = $state<HTMLDivElement | null>(null);
  let stripEl = $state<HTMLDivElement | null>(null);
  let stageW = $state(0);
  let stageH = $state(0);

  const quarter = $derived((((rot / 90) % 4) + 4) % 4);
  const sideways = $derived(quarter % 2 === 1);
  const rotW = $derived(sideways ? natH : natW);
  const rotH = $derived(sideways ? natW : natH);
  const fitZoom = $derived(
    natW && natH && stageW && stageH
      ? Math.min(1, Math.max(0.01, (stageW - FIT_PAD) / rotW), Math.max(0.01, (stageH - FIT_PAD) / rotH))
      : 1,
  );
  const minZoom = $derived(Math.min(MIN_ZOOM, fitZoom));
  const zoom = $derived(fitMode ? fitZoom : zoomState);
  const maxX = $derived(Math.max(0, (rotW * zoom - stageW) / 2));
  const maxY = $derived(Math.max(0, (rotH * zoom - stageH) / 2));
  const panX = $derived(clamp(tx, -maxX, maxX));
  const panY = $derived(clamp(ty, -maxY, maxY));
  const canPan = $derived(isImage && !loading && !failed && (maxX > 0.5 || maxY > 0.5));
  const zoomPct = $derived(Math.round(zoom * 100));

  function clamp(v: number, lo: number, hi: number) { return v < lo ? lo : v > hi ? hi : v; }

  // Reset whenever the gallery (re)opens or the index changes — every store set is a new object.
  $effect(() => {
    const g = $gallery;
    loading = true;
    failed = false;
    settled = false;
    natW = 0; natH = 0;
    rot = 0;
    fitMode = true;
    zoomState = 1;
    tx = 0; ty = 0;
    dragging = false;
    drag = null;
    if (g && g.items[g.index]?.kind === "video") loading = false;
  });

  // Preload neighbours.
  $effect(() => {
    if (!$gallery || total < 2) return;
    for (const d of [-1, 1]) {
      const it = items[(index + d + total) % total];
      if (it && it.kind === "image") { const im = new Image(); im.decoding = "async"; im.src = it.url; }
    }
  });

  // Keep the active thumbnail in view.
  $effect(() => {
    const i = index;
    const strip = stripEl;
    if (!strip) return;
    tick().then(() => {
      const el = strip.children[i] as HTMLElement | undefined;
      el?.scrollIntoView({ behavior: "smooth", block: "nearest", inline: "center" });
    });
  });

  // Non-passive wheel listener so we can preventDefault.
  $effect(() => {
    const el = stageEl;
    if (!el) return;
    const h = (e: WheelEvent) => onWheel(e);
    el.addEventListener("wheel", h, { passive: false });
    return () => el.removeEventListener("wheel", h);
  });

  function onImgLoad(e: Event) {
    const img = e.currentTarget as HTMLImageElement;
    natW = img.naturalWidth || 1;
    natH = img.naturalHeight || 1;
    loading = false;
    failed = false;
    requestAnimationFrame(() => requestAnimationFrame(() => { settled = true; }));
  }
  function onImgError() { loading = false; failed = true; }

  // ── Navigation ──────────────────────────────────────────────
  function go(delta: number) {
    const g = $gallery;
    if (!g || g.items.length < 2) return;
    const n = g.items.length;
    gallery.set({ items: g.items, index: (g.index + delta + n) % n });
  }
  function jump(i: number) {
    const g = $gallery;
    if (!g || i === g.index) return;
    gallery.set({ items: g.items, index: i });
  }

  // ── Zoom / pan ──────────────────────────────────────────────
  /** Zoom to `nz` keeping the stage point (px,py) — relative to stage centre — fixed. */
  function zoomTo(nz: number, px = 0, py = 0) {
    if (!isImage || !natW) return;
    const z = zoom;
    nz = clamp(nz, minZoom, MAX_ZOOM);
    const k = nz / z;
    const ntx = px - (px - panX) * k;
    const nty = py - (py - panY) * k;
    const mx = Math.max(0, (rotW * nz - stageW) / 2);
    const my = Math.max(0, (rotH * nz - stageH) / 2);
    zoomState = nz;
    fitMode = false;
    tx = clamp(ntx, -mx, mx);
    ty = clamp(nty, -my, my);
  }
  function zoomBy(f: number) { zoomTo(zoom * f); }
  function zoomActual() { zoomTo(1); }
  function fit() { fitMode = true; tx = 0; ty = 0; }

  function stagePoint(e: { clientX: number; clientY: number }) {
    const r = stageEl!.getBoundingClientRect();
    return { x: e.clientX - r.left - r.width / 2, y: e.clientY - r.top - r.height / 2 };
  }

  function onWheel(e: WheelEvent) {
    if (!isImage || loading || failed) return;
    e.preventDefault();
    if (e.deltaY === 0) return;
    const p = stagePoint(e);
    zoomTo(zoom * (e.deltaY < 0 ? ZOOM_STEP : 1 / ZOOM_STEP), p.x, p.y);
  }

  function onDblClick(e: MouseEvent) {
    if (!isImage || loading || failed) return;
    e.preventDefault();
    if (!fitMode && Math.abs(zoom - fitZoom) > 0.01) { fit(); return; }
    const p = stagePoint(e);
    zoomTo(2, p.x, p.y);
  }

  function rotate() {
    if (!isImage || loading || failed) return;
    rot += 90;
    if (!fitMode) { tx = panX; ty = panY; }
  }

  // Drag-to-pan
  let drag: { id: number; x: number; y: number; tx: number; ty: number; moved: boolean } | null = null;
  let suppressClick = false;

  function onPointerDown(e: PointerEvent) {
    suppressClick = false;
    if (e.button !== 0 || !canPan) return;
    if ((e.target as HTMLElement).closest("button")) return;
    drag = { id: e.pointerId, x: e.clientX, y: e.clientY, tx: panX, ty: panY, moved: false };
    try { stageEl?.setPointerCapture(e.pointerId); } catch { /* ignore */ }
    dragging = true;
    e.preventDefault();
  }
  function onPointerMove(e: PointerEvent) {
    if (!drag || e.pointerId !== drag.id) return;
    const dx = e.clientX - drag.x, dy = e.clientY - drag.y;
    if (!drag.moved && Math.abs(dx) + Math.abs(dy) > 3) drag.moved = true;
    tx = clamp(drag.tx + dx, -maxX, maxX);
    ty = clamp(drag.ty + dy, -maxY, maxY);
  }
  function onPointerUp(e: PointerEvent) {
    if (!drag || e.pointerId !== drag.id) return;
    suppressClick = drag.moved;
    try { stageEl?.releasePointerCapture(e.pointerId); } catch { /* ignore */ }
    drag = null;
    dragging = false;
  }
  function onStageClick(e: MouseEvent) {
    if (suppressClick) { suppressClick = false; return; }
    const tgt = e.target as HTMLElement;
    if (tgt === stageEl || tgt.classList.contains("lb-center")) closeLightbox();
  }

  // ── Actions ─────────────────────────────────────────────────
  function fileName(it: GalleryItem): string {
    if (it.name) return it.name;
    if (it.path) return it.path.split(/[\\/]/).pop() || "file";
    try { return decodeURIComponent(new URL(it.url).pathname.split("/").pop() || "") || "file"; }
    catch { return it.kind === "video" ? "video.mp4" : "image.png"; }
  }

  async function saveAs() {
    const it = cur;
    if (!it || busy) return;
    busy = true;
    try {
      const name = fileName(it);
      if (it.path) {
        const dest = await save({ defaultPath: name });
        if (!dest) return;
        await api.copyFileTo(it.path, dest);
        showToast(tr("Saved to {path}", { path: dest }));
      } else {
        const blob = await (await fetch(it.url)).blob();
        const href = URL.createObjectURL(blob);
        const a = document.createElement("a");
        a.href = href;
        a.download = name;
        document.body.appendChild(a);
        a.click();
        a.remove();
        setTimeout(() => URL.revokeObjectURL(href), 4000);
        showToast(tr("Download started"));
      }
    } catch (err) {
      console.error("lightbox save failed", err);
      showToast(tr("Could not save file"));
    } finally {
      busy = false;
    }
  }

  async function renderPng(url: string, quarterTurns: number): Promise<Blob> {
    const src = await (await fetch(url)).blob();
    const obj = URL.createObjectURL(src);
    try {
      const img = new Image();
      img.src = obj;
      await img.decode();
      const w = img.naturalWidth, h = img.naturalHeight;
      const side = quarterTurns % 2 === 1;
      const c = document.createElement("canvas");
      c.width = side ? h : w;
      c.height = side ? w : h;
      const ctx = c.getContext("2d");
      if (!ctx) throw new Error("no 2d context");
      ctx.translate(c.width / 2, c.height / 2);
      ctx.rotate((quarterTurns * Math.PI) / 2);
      ctx.drawImage(img, -w / 2, -h / 2);
      return await new Promise<Blob>((res, rej) =>
        c.toBlob((b) => (b ? res(b) : rej(new Error("toBlob failed"))), "image/png"));
    } finally {
      URL.revokeObjectURL(obj);
    }
  }

  async function copyImage() {
    const it = cur;
    if (!it || it.kind !== "image" || busy) return;
    busy = true;
    try {
      // Pass a promise so the clipboard call happens inside the user gesture (WebKit requirement).
      const pending = renderPng(it.url, quarter);
      await navigator.clipboard.write([new ClipboardItem({ "image/png": pending })]);
      showToast(tr("Image copied"));
    } catch (err) {
      console.error("lightbox copy failed", err);
      showToast(tr("Could not copy image"));
    } finally {
      busy = false;
    }
  }

  async function toggleFullscreen() {
    try {
      if (document.fullscreenElement) {
        await document.exitFullscreen();
        setFullscreenFlag(false);
      } else {
        await document.documentElement.requestFullscreen();
        setFullscreenFlag(true);
      }
    } catch (err) {
      console.error("fullscreen toggle failed", err);
    }
  }

  function humanSize(n?: number): string {
    if (n == null || !isFinite(n) || n < 0) return "";
    if (n < 1024) return `${n} B`;
    const u = ["KB", "MB", "GB", "TB"];
    let v = n / 1024, i = 0;
    while (v >= 1024 && i < u.length - 1) { v /= 1024; i++; }
    return `${v < 10 ? v.toFixed(1) : Math.round(v)} ${u[i]}`;
  }

  // ── Keyboard ────────────────────────────────────────────────
  function onKey(e: KeyboardEvent) {
    if (!$gallery) return;
    const mod = e.ctrlKey || e.metaKey;
    let handled = true;
    switch (e.key) {
      case "Escape": closeLightbox(); break;
      case "ArrowLeft": if (e.altKey || mod) { handled = false; break; } go(-1); break;
      case "ArrowRight": if (e.altKey || mod) { handled = false; break; } go(1); break;
      case "+": case "=": if (isImage) zoomBy(BTN_STEP); else handled = false; break;
      case "-": case "_": if (isImage) zoomBy(1 / BTN_STEP); else handled = false; break;
      case "0": if (isImage && !mod) zoomActual(); else handled = false; break;
      case "f": case "F": if (isImage && !mod) fit(); else handled = false; break;
      case "r": case "R": if (isImage && !mod) rotate(); else handled = false; break;
      case "F11": toggleFullscreen(); break;
      case "c": case "C":
        if (mod && isImage && !String(window.getSelection() ?? "")) copyImage(); else handled = false;
        break;
      case "s": case "S": if (mod) saveAs(); else handled = false; break;
      default: handled = false;
    }
    if (handled) { e.preventDefault(); e.stopPropagation(); }
  }

  onMount(() => {
    const onFs = () => { isFs = !!document.fullscreenElement; if (!isFs) setFullscreenFlag(false); };
    isFs = !!document.fullscreenElement;
    window.addEventListener("keydown", onKey, true);
    document.addEventListener("fullscreenchange", onFs);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      document.removeEventListener("fullscreenchange", onFs);
    };
  });

  const imgStyle = $derived(
    natW
      ? `width:${natW}px;height:${natH}px;` +
        `transform:translate(-50%,-50%) translate(${panX}px,${panY}px) rotate(${rot}deg) scale(${zoom});` +
        `transition:${dragging || !settled ? "none" : "transform 0.18s cubic-bezier(.2,.7,.2,1)"};`
      : "transform:translate(-50%,-50%);",
  );
</script>

{#snippet ico(d: string, size = 18)}
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor"
       stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path {d} /></svg>
{/snippet}

{#if $gallery && cur}
  <div class="lb" role="dialog" aria-modal="true" aria-label={fileName(cur)}>
    <!-- Top bar -->
    <header class="lb-top">
      <div class="lb-info">
        {#if total > 1}
          <span class="lb-count" title={$t("{n} of {total}", { n: index + 1, total })}>{index + 1} / {total}</span>
          <span class="dot">·</span>
        {/if}
        <span class="lb-name" title={fileName(cur)}>{fileName(cur)}</span>
        {#if humanSize(cur.size)}<span class="dot">·</span><span class="meta">{humanSize(cur.size)}</span>{/if}
        {#if cur.from}<span class="dot">·</span><span class="meta from">{cur.from}</span>{/if}
        {#if cur.time}<span class="dot">·</span><span class="meta">{cur.time}</span>{/if}
      </div>
      <div class="lb-actions">
        {#if isImage}
          <button class="gbtn" onclick={rotate} disabled={loading || failed} title={$t("Rotate (R)")} aria-label={$t("Rotate")}>
            {@render ico("M20 11a8 8 0 1 0-2.34 5.66M20 4v7h-7")}
          </button>
        {/if}
        <button class="gbtn" onclick={saveAs} disabled={busy} title={$t("Save as…")} aria-label={$t("Save as…")}>
          {@render ico("M12 4v11M7 10l5 5 5-5M5 20h14")}
        </button>
        {#if isImage}
          <button class="gbtn" onclick={copyImage} disabled={busy || loading || failed} title={$t("Copy image")} aria-label={$t("Copy image")}>
            {@render ico("M9 9h11v11H9zM5 15H4V4h11v1")}
          </button>
        {/if}
        <button class="gbtn" onclick={toggleFullscreen}
                title={isFs ? $t("Exit fullscreen (F11)") : $t("Fullscreen (F11)")}
                aria-label={isFs ? $t("Exit fullscreen") : $t("Fullscreen")}>
          {#if isFs}
            {@render ico("M9 4v5H4M15 4v5h5M9 20v-5H4M15 20v-5h5")}
          {:else}
            {@render ico("M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5")}
          {/if}
        </button>
        <button class="gbtn close" onclick={closeLightbox} title={$t("Close (Esc)")} aria-label={$t("Close")}>
          {@render ico("M6 6l12 12M18 6L6 18")}
        </button>
      </div>
    </header>

    <!-- Stage -->
    <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
    <div
      class="lb-stage"
      class:pannable={canPan}
      class:dragging
      bind:this={stageEl}
      bind:clientWidth={stageW}
      bind:clientHeight={stageH}
      onpointerdown={onPointerDown}
      onpointermove={onPointerMove}
      onpointerup={onPointerUp}
      onpointercancel={onPointerUp}
      onclick={onStageClick}
    >
      {#if cur.kind === "image"}
        {#key cur.url + "#" + index}
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
          <img
            class="lb-img"
            class:hidden={loading || failed}
            src={cur.url}
            alt={fileName(cur)}
            draggable="false"
            style={imgStyle}
            onload={onImgLoad}
            onerror={onImgError}
            ondblclick={onDblClick}
          />
        {/key}
        {#if loading}
          <div class="lb-center"><div class="spinner" aria-label={$t("Loading…")}></div></div>
        {:else if failed}
          <div class="lb-center">
            <div class="lb-error">
              {@render ico("M12 9v4M12 16.5v.01M10.3 3.9L2.4 18a2 2 0 0 0 1.7 3h15.8a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z", 34)}
              <div>{$t("Could not load image")}</div>
            </div>
          </div>
        {/if}
      {:else}
        <!-- Video fills the whole viewer; the player has window / full-screen modes too. -->
        <div class="lb-video">
          {#key cur.url + "#" + index}
            <MediaPlayer src={cur.url} kind="video" fill autoplay name={fileName(cur)} size={cur.size} />
          {/key}
        </div>
      {/if}

      {#if total > 1}
        <button class="side left" onclick={() => go(-1)} title={$t("Previous (←)")} aria-label={$t("Previous")}>
          {@render ico("M15 5l-7 7 7 7", 28)}
        </button>
        <button class="side right" onclick={() => go(1)} title={$t("Next (→)")} aria-label={$t("Next")}>
          {@render ico("M9 5l7 7-7 7", 28)}
        </button>
      {/if}
    </div>

    <!-- Thumbnails -->
    {#if total > 1}
      <div class="lb-strip" bind:this={stripEl}>
        {#each items as it, i (i)}
          <button class="thumb" class:active={i === index} onclick={() => jump(i)} title={fileName(it)} aria-label={fileName(it)}>
            {#if it.kind === "image"}
              <img src={it.url} alt="" loading="lazy" draggable="false" />
            {:else}
              <span class="vthumb">
                <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M8 5v14l11-7z" /></svg>
              </span>
            {/if}
          </button>
        {/each}
      </div>
    {/if}

    <!-- Bottom bar -->
    {#if isImage || total > 1}
    <footer class="lb-bottom">
      {#if total > 1}
        <button class="gbtn" onclick={() => go(-1)} title={$t("Previous (←)")} aria-label={$t("Previous")}>
          {@render ico("M15 5l-7 7 7 7")}
        </button>
      {/if}
      {#if isImage}
        <div class="zoombox">
          <button class="gbtn" onclick={() => zoomBy(1 / BTN_STEP)} disabled={loading || failed || zoom <= minZoom + 1e-6} title={$t("Zoom out (−)")} aria-label={$t("Zoom out")}>
            {@render ico("M5 12h14")}
          </button>
          <span class="pill" title={$t("Zoom")}>{zoomPct}%</span>
          <button class="gbtn" onclick={() => zoomBy(BTN_STEP)} disabled={loading || failed || zoom >= MAX_ZOOM - 1e-6} title={$t("Zoom in (+)")} aria-label={$t("Zoom in")}>
            {@render ico("M12 5v14M5 12h14")}
          </button>
          <button class="gbtn txt" onclick={zoomActual} disabled={loading || failed} title={$t("Actual size (0)")} aria-label={$t("Actual size")}>1:1</button>
          <button class="gbtn" class:on={fitMode} onclick={fit} disabled={loading || failed} title={$t("Fit to window (F)")} aria-label={$t("Fit to window")}>
            {@render ico("M3 7V3h4M17 3h4v4M21 17v4h-4M7 21H3v-4M8 8h8v8H8z")}
          </button>
        </div>
      {/if}
      {#if total > 1}
        <button class="gbtn" onclick={() => go(1)} title={$t("Next (→)")} aria-label={$t("Next")}>
          {@render ico("M9 5l7 7-7 7")}
        </button>
      {/if}
    </footer>
    {/if}
  </div>
{/if}

<style>
  .lb {
    position: fixed; inset: var(--tb-h, 0px) 0 0 0;
    z-index: 200;
    display: flex; flex-direction: column;
    background: rgba(2, 3, 6, 0.94);
    backdrop-filter: blur(10px);
    -webkit-backdrop-filter: blur(10px);
    color: #fff;
    animation: lb-fade 0.16s ease-out;
    user-select: none;
    -webkit-user-select: none;
  }
  @keyframes lb-fade { from { opacity: 0; } to { opacity: 1; } }

  /* ── Bars ─────────────────────────────── */
  .lb-top {
    flex: none;
    display: flex; align-items: center; justify-content: space-between; gap: 16px;
    padding: 12px 16px;
    background: linear-gradient(to bottom, rgba(2, 3, 6, 0.7), rgba(2, 3, 6, 0));
    position: relative; z-index: 3;
  }
  .lb-info {
    min-width: 0;
    display: flex; align-items: center; gap: 8px;
    font-size: 13px;
    color: rgba(255, 255, 255, 0.82);
    white-space: nowrap;
    overflow: hidden;
  }
  .lb-count {
    flex: none;
    color: var(--accent);
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    text-shadow: 0 0 10px var(--accent-glow);
  }
  .lb-name { min-width: 0; overflow: hidden; text-overflow: ellipsis; color: #fff; font-weight: 500; }
  .meta { flex: none; color: rgba(255, 255, 255, 0.55); font-variant-numeric: tabular-nums; }
  .meta.from { flex: 0 1 auto; overflow: hidden; text-overflow: ellipsis; max-width: 22ch; }
  .dot { flex: none; color: rgba(255, 255, 255, 0.3); }
  .lb-actions { flex: none; display: flex; gap: 8px; }

  .lb-bottom {
    flex: none;
    display: flex; align-items: center; justify-content: center; gap: 14px;
    padding: 10px 16px 14px;
    position: relative; z-index: 3;
  }
  .zoombox { display: flex; align-items: center; gap: 6px; }
  .pill {
    min-width: 58px;
    padding: 6px 10px;
    border-radius: 999px;
    background: rgba(255, 255, 255, 0.06);
    border: 1px solid rgba(255, 255, 255, 0.1);
    font-size: 12px; font-weight: 600;
    text-align: center;
    font-variant-numeric: tabular-nums;
    color: rgba(255, 255, 255, 0.85);
  }

  /* ── Glass buttons ────────────────────── */
  .gbtn {
    display: inline-grid; place-items: center;
    min-width: 36px; height: 36px;
    padding: 0 8px;
    border-radius: 10px;
    border: 1px solid rgba(255, 255, 255, 0.1);
    background: rgba(255, 255, 255, 0.08);
    color: rgba(255, 255, 255, 0.88);
    cursor: pointer;
    backdrop-filter: blur(6px);
    -webkit-backdrop-filter: blur(6px);
    transition: color 0.12s, box-shadow 0.15s, border-color 0.15s, background 0.15s;
  }
  .gbtn.txt { font-size: 12px; font-weight: 700; letter-spacing: 0.02em; }
  .gbtn:hover:not(:disabled), .gbtn:focus-visible {
    color: var(--accent);
    border-color: var(--accent-soft, rgba(255, 255, 255, 0.2));
    box-shadow: 0 0 14px var(--accent-glow);
    outline: none;
  }
  .gbtn.on { color: var(--accent); }
  .gbtn:disabled { opacity: 0.35; cursor: default; }
  .gbtn.close:hover { background: rgba(255, 255, 255, 0.12); }

  /* ── Stage ────────────────────────────── */
  .lb-stage {
    flex: 1 1 auto;
    min-height: 0;
    position: relative;
    overflow: hidden;
    touch-action: none;
  }
  .lb-stage.pannable { cursor: grab; }
  .lb-stage.dragging { cursor: grabbing; }
  .lb-img {
    position: absolute; left: 50%; top: 50%;
    max-width: none; max-height: none;
    transform-origin: center center;
    will-change: transform;
    border-radius: 4px;
    box-shadow: 0 10px 60px rgba(0, 0, 0, 0.55);
    -webkit-user-drag: none;
  }
  .lb-img.hidden { opacity: 0; pointer-events: none; }
  .lb-center {
    position: absolute; inset: 0;
    display: grid; place-items: center;
    padding: 16px;
  }
  .lb-video { position: absolute; inset: 0; pointer-events: auto; }
  .spinner {
    width: 38px; height: 38px;
    border-radius: 50%;
    border: 3px solid rgba(255, 255, 255, 0.12);
    border-top-color: var(--accent);
    box-shadow: 0 0 18px var(--accent-glow);
    animation: lb-spin 0.8s linear infinite;
  }
  @keyframes lb-spin { to { transform: rotate(360deg); } }
  .lb-error {
    display: flex; flex-direction: column; align-items: center; gap: 10px;
    color: rgba(255, 255, 255, 0.6);
    font-size: 13px;
  }

  .side {
    position: absolute; top: 50%;
    transform: translateY(-50%);
    width: 56px; height: 96px;
    display: grid; place-items: center;
    border: none;
    border-radius: 14px;
    background: rgba(255, 255, 255, 0.05);
    color: rgba(255, 255, 255, 0.75);
    cursor: pointer;
    opacity: 0;
    transition: opacity 0.18s, color 0.12s, background 0.15s, box-shadow 0.15s;
    z-index: 2;
  }
  .side.left { left: 14px; }
  .side.right { right: 14px; }
  .lb-stage:hover .side { opacity: 1; }
  .lb-stage.dragging .side { opacity: 0; }
  .side:hover, .side:focus-visible {
    opacity: 1;
    color: var(--accent);
    background: rgba(255, 255, 255, 0.1);
    box-shadow: 0 0 18px var(--accent-glow);
    outline: none;
  }

  /* ── Thumbnail strip ──────────────────── */
  .lb-strip {
    flex: none;
    display: flex; gap: 8px;
    padding: 8px 16px 2px;
    overflow-x: auto;
    overflow-y: hidden;
    scrollbar-width: thin;
    scrollbar-color: rgba(255, 255, 255, 0.15) transparent;
    position: relative; z-index: 3;
  }
  .thumb {
    flex: none;
    width: 56px; height: 56px;
    padding: 0;
    border-radius: 8px;
    overflow: hidden;
    border: 1px solid rgba(255, 255, 255, 0.1);
    background: rgba(255, 255, 255, 0.06);
    cursor: pointer;
    opacity: 0.5;
    transition: opacity 0.15s, border-color 0.15s, box-shadow 0.15s, transform 0.15s;
  }
  .thumb:first-child { margin-left: auto; }
  .thumb:last-child { margin-right: auto; }
  .thumb:hover { opacity: 0.85; }
  .thumb.active {
    opacity: 1;
    border-color: var(--accent);
    box-shadow: 0 0 12px var(--accent-glow);
    transform: translateY(-2px);
  }
  .thumb img { width: 100%; height: 100%; object-fit: cover; display: block; }
  .vthumb {
    width: 100%; height: 100%;
    display: grid; place-items: center;
    color: rgba(255, 255, 255, 0.8);
    background: linear-gradient(135deg, rgba(255, 255, 255, 0.08), rgba(255, 255, 255, 0.02));
  }

  @media (max-width: 560px) {
    .meta { display: none; }
    .dot { display: none; }
    .lb-count + .dot { display: inline; }
    .lb-actions { gap: 4px; }
    .side { width: 40px; height: 72px; }
  }
</style>
