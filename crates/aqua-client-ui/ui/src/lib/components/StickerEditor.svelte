<script module lang="ts">
  import { writable } from "svelte/store";

  interface State { imageBytes: number[]; mime: string; sourceName: string; pack?: string }
  export const stickerEditorState = writable<State | null>(null);
  /** Bumped after a sticker is saved, so libraries rescan. */
  export const stickerLibraryTick = writable(0);

  /** Open the editor with a picture (or empty bytes for a blank sticker:
   *  text and drawing only). `pack` preselects the target pack. */
  export function openStickerEditor(imageBytes: number[], mime: string, sourceName: string, pack?: string) {
    stickerEditorState.set({ imageBytes, mime, sourceName, pack });
  }
  export function closeStickerEditor() { stickerEditorState.set(null); }
</script>

<script lang="ts">
  /**
   * Telegram-style sticker editor.
   *   Move    — drag the picture, wheel / slider to zoom, slider to rotate, flip
   *   Cut out — tap a colour to remove it (flood fill with tolerance), or
   *             "Remove background" from the edges in one go
   *   Erase / Restore — brushes on the picture's own mask (stays put when
   *             the picture is moved afterwards)
   *   Draw    — freehand strokes
   *   Text    — captions in three styles, draggable
   * plus a white (or any colour) outline around everything, undo/redo, up
   * to three emoji tags and a choice of your packs. Output: 512×512 WebP
   * (PNG where the WebView can't encode WebP).
   */
  import { t, tr } from "../i18n";
  import { onMount, tick } from "svelte";
  import { get } from "svelte/store";
  import { api, bytesToDataUrl } from "../tauri";
  import { showToast } from "./Toast.svelte";
  import { EMOJI_CATS } from "../emojiData";

  const S = 512;          // output / stage size
  const SRC_MAX = 768;    // working resolution of the picture and its mask

  type Tool = "move" | "cut" | "erase" | "restore" | "draw" | "text";
  type TextStyle = "fill" | "outline" | "bg";
  interface TextItem { id: number; text: string; x: number; y: number; size: number; color: string; style: TextStyle }
  interface Snap { mask: ImageData | null; strokes: ImageData; texts: TextItem[]; tf: Tf }
  interface Tf { x: number; y: number; scale: number; rot: number; flip: boolean }

  const QUICK = ["😀","😂","🥹","😍","🥰","😎","🤔","🤯","😴","😭","🤣","🫢","👍","👎","🙌","👏","💪","🤝","🫶","❤️","💔","🔥","✨","🎉","💯","🤖","👻","🐱","🐶","🦊","🍕","☕","🌸","🌈","🗿","💎"];
  const COLORS = ["#ffffff", "#000000", "#ff3b30", "#ff9500", "#ffcc00", "#34c759", "#00c7be", "#007aff", "#5856d6", "#af52de", "#ff2d55"];

  // ---- state ---------------------------------------------------------------
  let tool = $state<Tool>("move");
  let tf = $state<Tf>({ x: 0, y: 0, scale: 1, rot: 0, flip: false });
  let brush = $state(28);
  let tolerance = $state(32);
  let color = $state("#ffffff");
  let outline = $state(true);
  let outlineColor = $state("#ffffff");
  let outlineWidth = $state(10);
  let texts = $state<TextItem[]>([]);
  let selText = $state<number | null>(null);
  let emojis = $state<string[]>(["😀"]);
  let emojiQuery = $state("");
  let packName = $state("My stickers");
  let packs = $state<string[]>([]);
  let newPack = $state(false);
  let stickerName = $state("sticker");
  let processing = $state(false);
  let hasImage = $state(false);
  let canUndo = $state(false), canRedo = $state(false);

  let stageEl = $state<HTMLCanvasElement | null>(null);
  let textInput = $state<HTMLInputElement | null>(null);

  // Off-screen layers.
  let srcC: HTMLCanvasElement | null = null;     // picture at working size
  let maskC: HTMLCanvasElement | null = null;    // alpha mask, same size
  let cutC: HTMLCanvasElement | null = null;     // picture × mask
  const strokesC = document.createElement("canvas"); strokesC.width = strokesC.height = S;
  const subjC = document.createElement("canvas"); subjC.width = subjC.height = S;
  const ringC = document.createElement("canvas"); ringC.width = ringC.height = S;

  let undoStack: Snap[] = [];
  let redoStack: Snap[] = [];
  let nextTextId = 1;

  // ---- open / reset ----------------------------------------------------------
  let opened: State | null = null;
  $effect(() => {
    const s = $stickerEditorState;
    if (s && s !== opened) { opened = s; void init(s); }
    else if (!s) opened = null;
  });

  async function init(s: State) {
    tool = "move"; tf = { x: 0, y: 0, scale: 1, rot: 0, flip: false };
    texts = []; selText = null; emojis = ["😀"]; emojiQuery = "";
    undoStack = []; redoStack = []; syncHistory();
    processing = false; outline = true;
    stickerName = (s.sourceName || "sticker").replace(/\.[^.]+$/, "").replace(/[^\p{L}\p{N}_-]/gu, "_").slice(0, 32) || "sticker";
    strokesC.getContext("2d")!.clearRect(0, 0, S, S);
    srcC = maskC = cutC = null;
    hasImage = false;
    try {
      const lib = await api.listLocalMedia();
      packs = lib.packs.filter((p) => !p.source && p.kind !== "emoji").map((p) => p.title);
    } catch { packs = []; }
    packName = s.pack ?? packs[0] ?? "My stickers";
    newPack = !packs.includes(packName);
    if (s.imageBytes.length) {
      const img = new Image();
      img.src = bytesToDataUrl(s.imageBytes, s.mime || "image/png");
      try { await img.decode(); } catch { showToast(tr("Could not read the picture")); }
      if (img.naturalWidth) {
        const k = Math.min(1, SRC_MAX / Math.max(img.naturalWidth, img.naturalHeight));
        srcC = document.createElement("canvas");
        srcC.width = Math.max(1, Math.round(img.naturalWidth * k));
        srcC.height = Math.max(1, Math.round(img.naturalHeight * k));
        srcC.getContext("2d")!.drawImage(img, 0, 0, srcC.width, srcC.height);
        maskC = document.createElement("canvas");
        maskC.width = srcC.width; maskC.height = srcC.height;
        const m = maskC.getContext("2d")!; m.fillStyle = "#fff"; m.fillRect(0, 0, maskC.width, maskC.height);
        cutC = document.createElement("canvas");
        cutC.width = srcC.width; cutC.height = srcC.height;
        rebuildCut();
        hasImage = true;
      }
    } else {
      tool = "text";
    }
    await tick();
    render();
  }

  function rebuildCut() {
    if (!srcC || !maskC || !cutC) return;
    const c = cutC.getContext("2d")!;
    c.globalCompositeOperation = "source-over";
    c.clearRect(0, 0, cutC.width, cutC.height);
    c.drawImage(srcC, 0, 0);
    c.globalCompositeOperation = "destination-in";
    c.drawImage(maskC, 0, 0);
    c.globalCompositeOperation = "source-over";
  }

  // ---- geometry --------------------------------------------------------------
  /** Scale from picture pixels to stage pixels. */
  const fitK = () => (srcC ? (S / Math.max(srcC.width, srcC.height)) * tf.scale : 1);
  function applyTf(c: CanvasRenderingContext2D) {
    c.translate(S / 2 + tf.x, S / 2 + tf.y);
    c.rotate((tf.rot * Math.PI) / 180);
    c.scale(fitK() * (tf.flip ? -1 : 1), fitK());
  }
  /** Stage point → picture pixel. */
  function toImage(px: number, py: number): [number, number] {
    if (!srcC) return [0, 0];
    let x = px - (S / 2 + tf.x), y = py - (S / 2 + tf.y);
    const a = (-tf.rot * Math.PI) / 180;
    [x, y] = [x * Math.cos(a) - y * Math.sin(a), x * Math.sin(a) + y * Math.cos(a)];
    const k = fitK();
    x /= k * (tf.flip ? -1 : 1); y /= k;
    return [x + srcC.width / 2, y + srcC.height / 2];
  }
  function stagePoint(e: PointerEvent | WheelEvent): [number, number] {
    const r = stageEl!.getBoundingClientRect();
    return [((e.clientX - r.left) / r.width) * S, ((e.clientY - r.top) / r.height) * S];
  }

  // ---- rendering -------------------------------------------------------------
  function fontFor(ti: TextItem) { return `800 ${ti.size}px Onest, system-ui, sans-serif`; }
  function textBox(c: CanvasRenderingContext2D, ti: TextItem) {
    c.font = fontFor(ti);
    const lines = ti.text.split("\n");
    const w = Math.max(...lines.map((l) => c.measureText(l).width), 10);
    const h = lines.length * ti.size * 1.15;
    return { w, h, lines };
  }
  function drawTexts(c: CanvasRenderingContext2D) {
    for (const ti of texts) {
      if (!ti.text.trim()) continue;
      const { w, h, lines } = textBox(c, ti);
      c.save();
      c.font = fontFor(ti);
      c.textAlign = "center"; c.textBaseline = "middle";
      if (ti.style === "bg") {
        c.fillStyle = ti.color;
        const pad = ti.size * 0.35;
        roundRect(c, ti.x - w / 2 - pad, ti.y - h / 2 - pad * 0.6, w + pad * 2, h + pad * 1.2, ti.size * 0.35);
        c.fill();
      }
      lines.forEach((l, i) => {
        const y = ti.y - h / 2 + ti.size * 1.15 * (i + 0.5);
        if (ti.style === "outline") {
          c.lineJoin = "round"; c.lineWidth = Math.max(4, ti.size * 0.18);
          c.strokeStyle = contrast(ti.color); c.strokeText(l, ti.x, y);
        }
        c.fillStyle = ti.style === "bg" ? contrast(ti.color) : ti.color;
        c.fillText(l, ti.x, y);
      });
      c.restore();
    }
  }
  function roundRect(c: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
    c.beginPath(); c.moveTo(x + r, y); c.arcTo(x + w, y, x + w, y + h, r); c.arcTo(x + w, y + h, x, y + h, r);
    c.arcTo(x, y + h, x, y, r); c.arcTo(x, y, x + w, y, r); c.closePath();
  }
  function contrast(hex: string) {
    const n = parseInt(hex.slice(1), 16);
    const l = 0.299 * ((n >> 16) & 255) + 0.587 * ((n >> 8) & 255) + 0.114 * (n & 255);
    return l > 150 ? "#000000" : "#ffffff";
  }

  /** Compose everything into `out` (the stage, or the export canvas). */
  function compose(out: CanvasRenderingContext2D, showSel: boolean) {
    const sc = subjC.getContext("2d")!;
    sc.clearRect(0, 0, S, S);
    if (cutC) { sc.save(); applyTf(sc); sc.drawImage(cutC, -cutC.width / 2, -cutC.height / 2); sc.restore(); }
    sc.drawImage(strokesC, 0, 0);
    drawTexts(sc);
    out.clearRect(0, 0, S, S);
    if (outline && outlineWidth > 0) {
      const rc = ringC.getContext("2d")!;
      rc.clearRect(0, 0, S, S);
      const steps = 24;
      for (let i = 0; i < steps; i++) {
        const a = (i / steps) * Math.PI * 2;
        rc.drawImage(subjC, Math.cos(a) * outlineWidth, Math.sin(a) * outlineWidth);
      }
      rc.globalCompositeOperation = "source-in";
      rc.fillStyle = outlineColor; rc.fillRect(0, 0, S, S);
      rc.globalCompositeOperation = "source-over";
      out.drawImage(ringC, 0, 0);
    }
    out.drawImage(subjC, 0, 0);
    if (showSel) drawSelFrame(out);
  }
  /** Dashed frame around the selected caption (stage only, never exported). */
  function drawSelFrame(c: CanvasRenderingContext2D) {
    const ti = texts.find((x) => x.id === selText);
    if (!ti) return;
    const { w, h } = textBox(c, ti);
    c.save();
    c.setLineDash([6, 5]); c.lineWidth = 2; c.strokeStyle = "rgba(255,255,255,.9)";
    c.strokeRect(ti.x - w / 2 - 10, ti.y - h / 2 - 8, Math.max(w, 30) + 20, h + 16);
    c.restore();
  }

  let raf = 0;
  function render() {
    if (raf) return;
    raf = requestAnimationFrame(() => {
      raf = 0;
      if (!stageEl) return;
      const c = stageEl.getContext("2d")!;
      if (stageEl.width !== S) { stageEl.width = S; stageEl.height = S; }
      compose(c, true);
    });
  }
  $effect(() => {
    void tf.x; void tf.y; void tf.scale; void tf.rot; void tf.flip;
    void outline; void outlineColor; void outlineWidth; void texts; void selText;
    render();
  });

  // ---- history ---------------------------------------------------------------
  function snap(): Snap {
    return {
      mask: maskC ? maskC.getContext("2d")!.getImageData(0, 0, maskC.width, maskC.height) : null,
      strokes: strokesC.getContext("2d")!.getImageData(0, 0, S, S),
      texts: texts.map((x) => ({ ...x })),
      tf: { ...tf },
    };
  }
  function restore(s: Snap) {
    if (s.mask && maskC) maskC.getContext("2d")!.putImageData(s.mask, 0, 0);
    strokesC.getContext("2d")!.putImageData(s.strokes, 0, 0);
    texts = s.texts.map((x) => ({ ...x }));
    tf = { ...s.tf };
    rebuildCut(); render();
  }
  function checkpoint() {
    undoStack.push(snap());
    if (undoStack.length > 20) undoStack.shift();
    redoStack = [];
    syncHistory();
  }
  function undo() { const s = undoStack.pop(); if (!s) return; redoStack.push(snap()); restore(s); syncHistory(); }
  function redo() { const s = redoStack.pop(); if (!s) return; undoStack.push(snap()); restore(s); syncHistory(); }
  function syncHistory() { canUndo = undoStack.length > 0; canRedo = redoStack.length > 0; }

  // ---- cut out (flood fill on the picture) -----------------------------------
  function floodRemove(seeds: [number, number][]) {
    if (!srcC || !maskC) return 0;
    const w = srcC.width, h = srcC.height;
    const src = srcC.getContext("2d")!.getImageData(0, 0, w, h).data;
    const mImg = maskC.getContext("2d")!.getImageData(0, 0, w, h);
    const m = mImg.data;
    const seen = new Uint8Array(w * h);
    const tol = tolerance * 3;
    const stack: number[] = [];
    let removed = 0;
    for (const [sx, sy] of seeds) {
      const x0 = Math.floor(sx), y0 = Math.floor(sy);
      if (x0 < 0 || y0 < 0 || x0 >= w || y0 >= h) continue;
      const p0 = (y0 * w + x0) * 4;
      if (m[p0 + 3] === 0 || seen[y0 * w + x0]) continue;
      const r0 = src[p0], g0 = src[p0 + 1], b0 = src[p0 + 2];
      stack.push(y0 * w + x0);
      while (stack.length) {
        const i = stack.pop()!;
        if (seen[i]) continue;
        seen[i] = 1;
        const p = i * 4;
        if (m[p + 3] === 0) continue;
        if (Math.abs(src[p] - r0) + Math.abs(src[p + 1] - g0) + Math.abs(src[p + 2] - b0) > tol && src[p + 3] > 8) continue;
        m[p + 3] = 0; removed++;
        const x = i % w;
        if (x > 0) stack.push(i - 1);
        if (x < w - 1) stack.push(i + 1);
        if (i >= w) stack.push(i - w);
        if (i < w * (h - 1)) stack.push(i + w);
      }
    }
    // Soften the new edge by one pixel.
    for (let y = 1; y < h - 1; y++) for (let x = 1; x < w - 1; x++) {
      const i = (y * w + x) * 4;
      if (m[i + 3] === 255 && (m[i - 1] === 0 || m[i + 7] === 0 || m[i - w * 4 + 3] === 0 || m[i + w * 4 + 3] === 0)) m[i + 3] = 160;
    }
    maskC.getContext("2d")!.putImageData(mImg, 0, 0);
    rebuildCut(); render();
    return removed;
  }
  function removeBackground() {
    if (!srcC) return;
    checkpoint();
    const w = srcC.width, h = srcC.height, seeds: [number, number][] = [];
    const step = Math.max(1, Math.floor(Math.min(w, h) / 64));
    for (let x = 0; x < w; x += step) seeds.push([x, 0], [x, h - 1]);
    for (let y = 0; y < h; y += step) seeds.push([0, y], [w - 1, y]);
    const n = floodRemove(seeds);
    if (!n) showToast(tr("Nothing to remove at the edges — tap the background with the cut-out tool"));
  }
  function resetMask() {
    if (!maskC) return;
    checkpoint();
    const m = maskC.getContext("2d")!; m.fillStyle = "#fff"; m.fillRect(0, 0, maskC.width, maskC.height);
    rebuildCut(); render();
  }

  // ---- pointer ---------------------------------------------------------------
  let drag: null | { kind: "pan" | "text" | "paint" | "draw"; sx: number; sy: number; ox: number; oy: number; id?: number; lx: number; ly: number } = null;

  function hitText(x: number, y: number): TextItem | undefined {
    const c = stageEl!.getContext("2d")!;
    return [...texts].reverse().find((ti) => {
      const { w, h } = textBox(c, ti);
      return Math.abs(x - ti.x) <= w / 2 + 12 && Math.abs(y - ti.y) <= h / 2 + 10;
    });
  }

  function paintMask(x0: number, y0: number, x1: number, y1: number) {
    if (!maskC) return;
    const [a, b] = toImage(x0, y0), [c2, d] = toImage(x1, y1);
    const r = brush / 2 / fitK();
    const m = maskC.getContext("2d")!;
    m.save();
    m.globalCompositeOperation = tool === "erase" ? "destination-out" : "source-over";
    m.strokeStyle = "#fff"; m.lineCap = "round"; m.lineJoin = "round"; m.lineWidth = r * 2;
    m.beginPath(); m.moveTo(a, b); m.lineTo(c2, d); m.stroke();
    m.restore();
    rebuildCut(); render();
  }
  function paintStroke(x0: number, y0: number, x1: number, y1: number) {
    const c = strokesC.getContext("2d")!;
    c.strokeStyle = color; c.lineCap = "round"; c.lineJoin = "round"; c.lineWidth = brush / 2;
    c.beginPath(); c.moveTo(x0, y0); c.lineTo(x1, y1); c.stroke();
    render();
  }

  function onDown(e: PointerEvent) {
    if (!stageEl) return;
    stageEl.setPointerCapture(e.pointerId);
    const [x, y] = stagePoint(e);
    const ht = tool === "move" || tool === "text" ? hitText(x, y) : undefined;
    if (ht) {
      checkpoint();
      selText = ht.id;
      drag = { kind: "text", sx: x, sy: y, ox: ht.x, oy: ht.y, id: ht.id, lx: x, ly: y };
      if (tool === "text") queueMicrotask(() => textInput?.focus());
      return;
    }
    selText = null;
    if (tool === "text") { addText(x, y); return; }
    if (tool === "cut") {
      if (!hasImage) return;
      checkpoint();
      const [ix, iy] = toImage(x, y);
      if (!floodRemove([[ix, iy]])) { undoStack.pop(); syncHistory(); }
      return;
    }
    if (tool === "erase" || tool === "restore") {
      if (!hasImage) return;
      checkpoint();
      drag = { kind: "paint", sx: x, sy: y, ox: 0, oy: 0, lx: x, ly: y };
      paintMask(x, y, x + 0.01, y);
      return;
    }
    if (tool === "draw") {
      checkpoint();
      drag = { kind: "draw", sx: x, sy: y, ox: 0, oy: 0, lx: x, ly: y };
      paintStroke(x, y, x + 0.01, y);
      return;
    }
    if (hasImage) { checkpoint(); drag = { kind: "pan", sx: x, sy: y, ox: tf.x, oy: tf.y, lx: x, ly: y }; }
  }
  function onMove(e: PointerEvent) {
    const [x, y] = stageEl ? stagePoint(e) : [0, 0];
    cursor = [x, y];
    if (!drag) { if (tool === "erase" || tool === "restore" || tool === "draw") render(); return; }
    if (drag.kind === "pan") tf = { ...tf, x: drag.ox + x - drag.sx, y: drag.oy + y - drag.sy };
    else if (drag.kind === "text") texts = texts.map((ti) => (ti.id === drag!.id ? { ...ti, x: drag!.ox + x - drag!.sx, y: drag!.oy + y - drag!.sy } : ti));
    else if (drag.kind === "paint") paintMask(drag.lx, drag.ly, x, y);
    else if (drag.kind === "draw") paintStroke(drag.lx, drag.ly, x, y);
    drag.lx = x; drag.ly = y;
  }
  function onUp() { drag = null; }
  function onWheel(e: WheelEvent) {
    if (!hasImage) return;
    e.preventDefault();
    if (e.shiftKey) tf = { ...tf, rot: Math.round((tf.rot + Math.sign(e.deltaY) * 5) % 360) };
    else tf = { ...tf, scale: Math.min(4, Math.max(0.2, tf.scale * (e.deltaY < 0 ? 1.08 : 1 / 1.08))) };
  }
  let cursor = $state<[number, number] | null>(null);

  // ---- text ------------------------------------------------------------------
  function addText(x = S / 2, y = S * 0.82) {
    checkpoint();
    const ti: TextItem = { id: nextTextId++, text: "", x, y, size: 56, color, style: "outline" };
    texts = [...texts, ti];
    selText = ti.id;
    queueMicrotask(() => textInput?.focus());
  }
  let selected = $derived(texts.find((x) => x.id === selText));
  function patchText(p: Partial<TextItem>) { if (selText !== null) texts = texts.map((x) => (x.id === selText ? { ...x, ...p } : x)); }
  function deleteText() { if (selText === null) return; checkpoint(); texts = texts.filter((x) => x.id !== selText); selText = null; }

  // ---- emoji -----------------------------------------------------------------
  let emojiResults = $derived.by(() => {
    const q = emojiQuery.trim().toLowerCase();
    if (!q) return QUICK;
    const out: string[] = [];
    for (const [, , items] of EMOJI_CATS) for (const [e, words] of items) { if (words.includes(q)) out.push(e); if (out.length >= 48) return out; }
    return out;
  });
  function toggleEmoji(e: string) {
    if (emojis.includes(e)) { if (emojis.length > 1) emojis = emojis.filter((x) => x !== e); }
    else emojis = [...emojis, e].slice(-3);
  }

  // ---- save ------------------------------------------------------------------
  async function save() {
    const pack = packName.trim();
    if (!pack || !stickerName.trim()) { showToast(tr("Pack and name required")); return; }
    if (!hasImage && !texts.some((x) => x.text.trim())) {
      // A blank sticker needs at least something drawn or written.
      const d = strokesC.getContext("2d")!.getImageData(0, 0, S, S).data;
      let any = false; for (let i = 3; i < d.length; i += 64) if (d[i]) { any = true; break; }
      if (!any) { showToast(tr("The sticker is empty")); return; }
    }
    processing = true;
    try {
      const out = document.createElement("canvas"); out.width = out.height = S;
      compose(out.getContext("2d")!, false);
      let blob: Blob = await new Promise((res, rej) => out.toBlob((b) => (b ? res(b) : rej(new Error("toBlob"))), "image/webp", 0.92));
      // Some WebViews can't encode WebP and hand back PNG.
      const ext = blob.type === "image/webp" ? "webp" : "png";
      if (blob.size > 512 * 1024 && ext === "webp") {
        blob = await new Promise((res, rej) => out.toBlob((b) => (b ? res(b) : rej(new Error("toBlob"))), "image/webp", 0.8));
      }
      const bytes = Array.from(new Uint8Array(await blob.arrayBuffer()));
      await api.saveSticker(pack, `${emojis.join("")}_${stickerName.trim()}.${ext}`, bytes);
      stickerLibraryTick.update((n) => n + 1);
      showToast(tr("Saved to “{pack}”", { pack }));
      closeStickerEditor();
    } catch (e) {
      showToast(`${tr("Save failed")}: ${e}`);
    } finally {
      processing = false;
    }
  }

  onMount(() => {
    const h = (e: KeyboardEvent) => {
      if (!get(stickerEditorState)) return;
      const typing = (e.target as HTMLElement)?.closest?.("input, textarea");
      if (e.key === "Escape") { e.preventDefault(); closeStickerEditor(); return; }
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "z" && !typing) { e.preventDefault(); if (e.shiftKey) redo(); else undo(); }
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "y" && !typing) { e.preventDefault(); redo(); }
      if (e.key === "Delete" && !typing && selText !== null) deleteText();
    };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  });

  const TOOLS: { id: Tool; label: string; icon: string; needsImage?: boolean }[] = [
    { id: "move", label: "Move", icon: `<path d="M5 9l-3 3 3 3M9 5l3-3 3 3M15 19l-3 3-3-3M19 9l3 3-3 3M2 12h20M12 2v20"/>` },
    { id: "cut", label: "Cut out", icon: `<path d="M15 4l5 5-9 9H6v-5z"/><path d="M4 20h16"/><circle cx="17" cy="5" r="1"/>`, needsImage: true },
    { id: "erase", label: "Erase", icon: `<path d="M20 20H8l-5-5 10-10 9 9-6 6"/><path d="M7 11l6 6"/>`, needsImage: true },
    { id: "restore", label: "Restore", icon: `<path d="M3 12a9 9 0 1 0 3-6.7L3 8"/><path d="M3 3v5h5"/>`, needsImage: true },
    { id: "draw", label: "Draw", icon: `<path d="M12 19l7-7 3 3-7 7-3-3z"/><path d="M18 13l-1.5-7.5L2 2l3.5 14.5L13 18l5-5z"/>` },
    { id: "text", label: "Text", icon: `<path d="M4 7V4h16v3M9 20h6M12 4v16"/>` },
  ];
</script>

{#if $stickerEditorState}
  <div class="backdrop" role="button" tabindex="-1" aria-label={$t("Close editor")}
       onclick={(e) => { if ((e.target as HTMLElement).classList.contains("backdrop")) closeStickerEditor(); }}
       onkeydown={() => {}}>
    <div class="sheet" role="dialog" aria-modal="true" tabindex="-1">
      <div class="head">
        <h2>{$t("Sticker editor")}</h2>
        <div class="hist">
          <button class="ib" onclick={undo} disabled={!canUndo} title={$t("Undo (Ctrl+Z)")} aria-label={$t("Undo")}>↶</button>
          <button class="ib" onclick={redo} disabled={!canRedo} title={$t("Redo (Ctrl+Shift+Z)")} aria-label={$t("Redo")}>↷</button>
        </div>
        <button class="ib x" onclick={closeStickerEditor} title={$t("Close")}>×</button>
      </div>

      <div class="body">
        <div class="left">
          <div class="tools">
            {#each TOOLS as tl (tl.id)}
              <button class="tool" class:active={tool === tl.id} disabled={tl.needsImage && !hasImage} onclick={() => { tool = tl.id; if (tl.id === "text" && !texts.length) addText(); }} title={$t(tl.label)}>
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">{@html tl.icon}</svg>
                <span>{$t(tl.label)}</span>
              </button>
            {/each}
          </div>
          <div class="stage" class:paint={tool === "erase" || tool === "restore" || tool === "draw"}>
            <canvas bind:this={stageEl}
              onpointerdown={onDown} onpointermove={onMove} onpointerup={onUp} onpointercancel={onUp}
              onpointerleave={() => { cursor = null; render(); }}
              onwheel={onWheel}></canvas>
            {#if cursor && (tool === "erase" || tool === "restore" || tool === "draw") && stageEl}
              <span class="brush" style:left="{(cursor[0] / S) * 100}%" style:top="{(cursor[1] / S) * 100}%"
                    style:width="{((tool === 'draw' ? brush / 2 : brush) / S) * 100}%"></span>
            {/if}
          </div>
          <div class="hint">{$t(
            tool === "move" ? "Drag to move · wheel to zoom · Shift+wheel to rotate" :
            tool === "cut" ? "Tap a colour to remove it — the background, usually" :
            tool === "erase" ? "Paint over what should disappear" :
            tool === "restore" ? "Paint to bring erased parts back" :
            tool === "draw" ? "Draw on the sticker" : "Tap the sticker to add a caption · drag to move it")}</div>
        </div>

        <div class="right">
          {#if tool === "move"}
            <div class="sec">
              <label class="sl">{$t("Zoom")} <span>{Math.round(tf.scale * 100)}%</span>
                <input type="range" min="0.2" max="4" step="0.01" value={tf.scale} oninput={(e) => (tf = { ...tf, scale: +(e.target as HTMLInputElement).value })} disabled={!hasImage} /></label>
              <label class="sl">{$t("Rotate")} <span>{tf.rot}°</span>
                <input type="range" min="-180" max="180" step="1" value={tf.rot} oninput={(e) => (tf = { ...tf, rot: +(e.target as HTMLInputElement).value })} disabled={!hasImage} /></label>
              <div class="btns">
                <button class="chip" onclick={() => { checkpoint(); tf = { ...tf, flip: !tf.flip }; }} disabled={!hasImage}>⇋ {$t("Flip")}</button>
                <button class="chip" onclick={() => { checkpoint(); tf = { x: 0, y: 0, scale: 1, rot: 0, flip: false }; }} disabled={!hasImage}>{$t("Reset")}</button>
              </div>
            </div>
          {:else if tool === "cut"}
            <div class="sec">
              <button class="big" onclick={removeBackground}>✂ {$t("Remove background")}</button>
              <label class="sl">{$t("Tolerance")} <span>{tolerance}</span>
                <input type="range" min="4" max="120" step="1" bind:value={tolerance} /></label>
              <button class="chip" onclick={resetMask}>{$t("Bring the whole picture back")}</button>
            </div>
          {:else if tool === "erase" || tool === "restore" || tool === "draw"}
            <div class="sec">
              <label class="sl">{$t("Brush")} <span>{brush}px</span>
                <input type="range" min="4" max="120" step="1" bind:value={brush} /></label>
              {#if tool === "draw"}
                <div class="colors">
                  {#each COLORS as c (c)}<button class="cc" class:on={color === c} style:background={c} onclick={() => (color = c)} aria-label={c}></button>{/each}
                  <input type="color" bind:value={color} aria-label={$t("Custom colour")} />
                </div>
                <button class="chip" onclick={() => { checkpoint(); strokesC.getContext("2d")!.clearRect(0, 0, S, S); render(); }}>{$t("Clear drawing")}</button>
              {/if}
            </div>
          {:else if tool === "text"}
            <div class="sec">
              {#if selected}
                <input class="inp" bind:this={textInput} value={selected.text} placeholder={$t("Caption")} maxlength="60"
                       oninput={(e) => patchText({ text: (e.target as HTMLInputElement).value })} />
                <div class="seg">
                  {#each [["fill", "Aa"], ["outline", "Outline"], ["bg", "Plate"]] as [s, l] (s)}
                    <button class:active={selected.style === s} onclick={() => patchText({ style: s as TextStyle })}>{$t(l)}</button>
                  {/each}
                </div>
                <label class="sl">{$t("Size")} <span>{selected.size}</span>
                  <input type="range" min="20" max="140" step="1" value={selected.size} oninput={(e) => patchText({ size: +(e.target as HTMLInputElement).value })} /></label>
                <div class="colors">
                  {#each COLORS as c (c)}<button class="cc" class:on={selected.color === c} style:background={c} onclick={() => patchText({ color: c })} aria-label={c}></button>{/each}
                </div>
                <div class="btns">
                  <button class="chip" onclick={() => addText()}>＋ {$t("Another caption")}</button>
                  <button class="chip danger" onclick={deleteText}>{$t("Delete")}</button>
                </div>
              {:else}
                <button class="big" onclick={() => addText()}>＋ {$t("Add a caption")}</button>
              {/if}
            </div>
          {/if}

          <div class="sec">
            <label class="tg"><input type="checkbox" bind:checked={outline} /> {$t("Outline")}</label>
            {#if outline}
              <div class="row">
                <input type="range" min="2" max="24" step="1" bind:value={outlineWidth} />
                {#each ["#ffffff", "#000000", "#ffcc00", "#00c7be"] as c (c)}<button class="cc sm" class:on={outlineColor === c} style:background={c} onclick={() => (outlineColor = c)} aria-label={c}></button>{/each}
              </div>
            {/if}
          </div>

          <div class="sec">
            <div class="lbl">{$t("Emoji")} <b>{emojis.join(" ")}</b></div>
            <input class="inp sm" bind:value={emojiQuery} placeholder={$t("Search emoji")} />
            <div class="egrid">
              {#each emojiResults as e (e)}
                <button class="em" class:on={emojis.includes(e)} onclick={() => toggleEmoji(e)}>{e}</button>
              {/each}
            </div>
          </div>

          <div class="sec">
            <div class="lbl">{$t("Pack")}</div>
            {#if packs.length && !newPack}
              <select class="inp" bind:value={packName}>
                {#each packs as p (p)}<option value={p}>{p}</option>{/each}
              </select>
              <button class="link" onclick={() => { newPack = true; packName = ""; }}>＋ {$t("New pack")}</button>
            {:else}
              <input class="inp" bind:value={packName} placeholder={$t("My stickers")} maxlength="48" />
              {#if packs.length}<button class="link" onclick={() => { newPack = false; packName = packs[0]; }}>{$t("Pick an existing pack")}</button>{/if}
            {/if}
            <div class="lbl">{$t("Name")}</div>
            <input class="inp" bind:value={stickerName} maxlength="32" />
          </div>
        </div>
      </div>

      <div class="foot">
        <button class="btn-ghost" onclick={closeStickerEditor} disabled={processing}>{$t("Cancel")}</button>
        <button class="btn-primary" onclick={save} disabled={processing}>{$t(processing ? "Saving…" : "Save sticker")}</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop { position: fixed; inset: 0; background: rgba(6, 7, 12, 0.72); backdrop-filter: blur(6px);
    display: grid; place-items: center; z-index: 275; padding: 16px; animation: fade .2s ease-out; }
  .sheet { width: min(940px, 96vw); max-height: 95vh; display: flex; flex-direction: column; background: var(--bg-shell);
    border: 1px solid var(--border-strong); border-radius: 18px; box-shadow: 0 40px 90px -20px rgba(0,0,0,.7); overflow: hidden;
    animation: pop .32s cubic-bezier(.2,.9,.25,1.1); }
  @keyframes fade { from { opacity: 0; } }
  @keyframes pop { from { opacity: 0; transform: translateY(14px) scale(.97); } }
  .head { display: flex; align-items: center; gap: 8px; padding: 10px 14px; border-bottom: 1px solid var(--border); }
  .head h2 { margin: 0; font-size: 15px; font-weight: 800; flex: 1; }
  .hist { display: flex; gap: 4px; }
  .ib { width: 32px; height: 32px; border-radius: 9px; border: 1px solid var(--border); background: transparent; color: var(--fg-muted);
    font-size: 17px; cursor: pointer; transition: background .15s, color .15s; }
  .ib:hover:not(:disabled) { background: var(--bg-hover); color: var(--fg); }
  .ib.x { font-size: 20px; }

  .body { display: grid; grid-template-columns: minmax(0, 1fr) 300px; gap: 16px; padding: 14px; overflow: auto; }
  .left { display: flex; flex-direction: column; gap: 10px; min-width: 0; }
  .tools { display: flex; gap: 6px; flex-wrap: wrap; }
  .tool { flex: 1; min-width: 64px; display: flex; flex-direction: column; align-items: center; gap: 3px; padding: 7px 4px;
    border: 1px solid var(--border); border-radius: 11px; background: var(--bg-input); color: var(--fg-muted); font-size: 11px; cursor: pointer;
    transition: transform .2s cubic-bezier(.2,.8,.2,1), background .2s, color .2s, border-color .2s; }
  .tool svg { width: 18px; height: 18px; }
  .tool:hover:not(:disabled) { transform: translateY(-1px); color: var(--fg); }
  .tool.active { background: var(--accent-soft); color: var(--accent); border-color: var(--accent); }
  .stage { position: relative; aspect-ratio: 1; width: min(100%, 60vh); margin: 0 auto; border-radius: 14px; overflow: hidden;
    background: repeating-conic-gradient(#1a1f2b 0 25%, #11151e 0 50%) 0 / 22px 22px; touch-action: none; }
  .stage canvas { width: 100%; height: 100%; display: block; cursor: grab; }
  .stage canvas:active { cursor: grabbing; }
  .stage.paint canvas { cursor: none; }
  .brush { position: absolute; aspect-ratio: 1; border-radius: 50%; border: 1.5px solid #fff; box-shadow: 0 0 0 1px #000;
    transform: translate(-50%, -50%); pointer-events: none; }
  .hint { text-align: center; font-size: 11.5px; color: var(--fg-dim); }

  .right { display: flex; flex-direction: column; gap: 10px; min-width: 0; }
  .sec { background: var(--bg-surface, var(--bg-input)); border: 1px solid var(--border); border-radius: 12px; padding: 10px;
    display: flex; flex-direction: column; gap: 8px; animation: slide .25s cubic-bezier(.2,.8,.2,1); }
  @keyframes slide { from { opacity: 0; transform: translateX(8px); } }
  .sl { display: flex; flex-direction: column; gap: 4px; font-size: 11px; font-weight: 700; color: var(--fg-dim); text-transform: uppercase; letter-spacing: .05em; }
  .sl span { position: absolute; right: 0; }
  .sl { position: relative; }
  .sl input { width: 100%; accent-color: var(--accent); }
  .lbl { font-size: 11px; font-weight: 700; color: var(--fg-dim); text-transform: uppercase; letter-spacing: .05em; }
  .lbl b { color: var(--fg); font-size: 15px; text-transform: none; }
  .btns, .row { display: flex; gap: 6px; flex-wrap: wrap; align-items: center; }
  .row input[type=range] { flex: 1; accent-color: var(--accent); }
  .chip { border: 1px solid var(--border); background: var(--bg-input); color: var(--fg); border-radius: 9px; padding: 6px 10px; font-size: 12px; cursor: pointer; }
  .chip:hover:not(:disabled) { border-color: var(--border-strong); }
  .chip.danger { color: #ff6b6b; }
  .big { border: none; border-radius: 11px; padding: 10px; font-weight: 800; font-size: 13px; cursor: pointer;
    background: linear-gradient(135deg, var(--accent), var(--accent-2)); color: #06070c; transition: transform .2s, box-shadow .2s; }
  .big:hover { transform: translateY(-1px); box-shadow: 0 8px 20px -8px var(--accent-glow); }
  .colors { display: flex; gap: 6px; flex-wrap: wrap; align-items: center; }
  .cc { width: 22px; height: 22px; border-radius: 50%; border: 2px solid var(--border-strong); cursor: pointer; padding: 0; transition: transform .15s; }
  .cc.sm { width: 18px; height: 18px; }
  .cc.on { border-color: var(--accent); transform: scale(1.15); }
  .colors input[type=color] { width: 26px; height: 26px; border: none; background: none; padding: 0; }
  .inp { background: var(--bg-input); border: 1px solid var(--border); border-radius: 9px; color: var(--fg); font: inherit; font-size: 13px; padding: 7px 10px; outline: none; width: 100%; box-sizing: border-box; }
  .inp:focus { border-color: var(--accent); }
  .inp.sm { padding: 5px 9px; font-size: 12px; }
  .seg { display: flex; gap: 3px; background: var(--bg-input); padding: 3px; border-radius: 9px; }
  .seg button { flex: 1; border: none; background: transparent; color: var(--fg-muted); border-radius: 7px; padding: 5px; font-size: 12px; font-weight: 700; cursor: pointer; }
  .seg button.active { background: var(--accent-soft); color: var(--accent); }
  .tg { display: flex; align-items: center; gap: 8px; font-size: 13px; font-weight: 600; cursor: pointer; }
  .egrid { display: grid; grid-template-columns: repeat(8, 1fr); gap: 2px; max-height: 112px; overflow-y: auto; }
  .em { aspect-ratio: 1; border: 1px solid transparent; border-radius: 7px; background: none; font-size: 18px; cursor: pointer; padding: 0; transition: transform .12s; }
  .em:hover { background: var(--bg-hover); transform: scale(1.12); }
  .em.on { background: var(--accent-soft); border-color: var(--accent); }
  .link { align-self: flex-start; border: none; background: none; color: var(--accent); font-size: 12px; cursor: pointer; padding: 0; }
  .foot { display: flex; justify-content: flex-end; gap: 10px; padding: 10px 14px; border-top: 1px solid var(--border); background: var(--bg-list); }
  .btn-ghost, .btn-primary { padding: 8px 18px; border-radius: 10px; font-size: 13px; font-weight: 700; cursor: pointer; border: none; }
  .btn-ghost { background: transparent; border: 1px solid var(--border); color: var(--fg-muted); }
  .btn-primary { background: linear-gradient(135deg, var(--accent), var(--accent-2)); color: #06070C; box-shadow: 0 6px 16px -4px var(--accent-glow); transition: transform .2s; }
  .btn-primary:hover:not(:disabled) { transform: translateY(-1px); }
  button:disabled { opacity: .45; cursor: not-allowed; }
  @media (max-width: 760px) { .body { grid-template-columns: 1fr; } .stage { width: 100%; } }
</style>
