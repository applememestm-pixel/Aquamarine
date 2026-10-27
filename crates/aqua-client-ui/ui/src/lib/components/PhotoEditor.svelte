<script module lang="ts">
  import { editorState, editAndSend, closeEditor } from "../panels/photoEditor";
  import type { EditRequest } from "../panels/photoEditor";
  export { editorState, editAndSend, closeEditor };
</script>

<script lang="ts">
  import { untrack } from "svelte";
  import { api, bytesToDataUrl } from "../tauri";
  import { showToast } from "./Toast.svelte";
  import { loadActiveMessages } from "../stores";
  import { t, tr } from "../i18n";

  // ───────────────────────── types ─────────────────────────
  //
  // Everything is non-destructive: the source image is never modified.
  // "Doc space" = the source after rotation + horizontal flip, before
  // crop. Crop rect, strokes and text boxes all live in doc space; a
  // rotate/flip remaps them so annotations stay attached to the photo.

  type Rot = 0 | 90 | 180 | 270;
  type Tool = "crop" | "draw" | "text" | "adjust" | "filters";
  type TextStyle = "plain" | "pill" | "outline";
  type AspectId = "free" | "original" | "1:1" | "4:3" | "16:9";
  type Handle = "n" | "s" | "e" | "w" | "nw" | "ne" | "sw" | "se";
  type MapKind = "cw" | "ccw" | "flip";

  interface Pt { x: number; y: number }
  interface Box { x: number; y: number; w: number; h: number }
  interface Stroke { color: string; size: number; erase: boolean; pts: Pt[] }
  interface TextItem { id: number; x: number; y: number; text: string; color: string; size: number; style: TextStyle }
  interface Adjust { brightness: number; contrast: number; saturation: number; warmth: number; hue: number; blur: number }
  interface Snap {
    rot: Rot; flip: boolean; crop: Box | null; adj: Adjust; preset: string;
    strokes: Stroke[]; texts: TextItem[];
  }

  type OpFn = "brightness" | "contrast" | "saturate" | "grayscale" | "sepia" | "hue-rotate" | "blur";
  interface Op { fn: OpFn; v: number }
  interface Preset { id: string; label: string; ops: Op[]; tint: number }

  // ───────────────────────── constants ─────────────────────────

  const op = (fn: OpFn, v: number): Op => ({ fn, v });

  const PRESETS: Preset[] = [
    { id: "original", label: "Original", ops: [], tint: 0 },
    { id: "vivid", label: "Vivid", ops: [op("saturate", 1.45), op("contrast", 1.12), op("brightness", 1.04)], tint: 0 },
    { id: "mono", label: "Mono", ops: [op("grayscale", 1)], tint: 0 },
    { id: "noir", label: "Noir", ops: [op("grayscale", 1), op("contrast", 1.5), op("brightness", 0.88)], tint: 0 },
    { id: "warm", label: "Warm", ops: [op("sepia", 0.15), op("saturate", 1.15)], tint: 45 },
    { id: "cool", label: "Cool", ops: [op("saturate", 0.9), op("brightness", 1.03)], tint: -45 },
    { id: "fade", label: "Fade", ops: [op("contrast", 0.82), op("brightness", 1.1), op("saturate", 0.75)], tint: 0 },
    { id: "retro", label: "Retro", ops: [op("sepia", 0.5), op("contrast", 1.1), op("saturate", 1.25), op("hue-rotate", -12)], tint: 10 },
  ];

  const PALETTE = [
    "#00F0FF", "#39FF88", "#C6FF00", "#FFE600", "#FF9F1C",
    "#FF2E63", "#FF3CF5", "#8B5CFF", "#FFFFFF", "#0A0B10",
  ];

  const ADJ0: Adjust = { brightness: 100, contrast: 100, saturation: 100, warmth: 0, hue: 0, blur: 0 };

  const SLIDERS: { key: keyof Adjust; label: string; min: number; max: number }[] = [
    { key: "brightness", label: "Brightness", min: 40, max: 160 },
    { key: "contrast", label: "Contrast", min: 40, max: 160 },
    { key: "saturation", label: "Saturation", min: 0, max: 200 },
    { key: "warmth", label: "Warmth", min: -100, max: 100 },
    { key: "hue", label: "Hue", min: -180, max: 180 },
    { key: "blur", label: "Blur", min: 0, max: 20 },
  ];

  const ASPECTS: { id: AspectId; label: string }[] = [
    { id: "free", label: "Free" },
    { id: "original", label: "Original" },
    { id: "1:1", label: "1:1" },
    { id: "4:3", label: "4:3" },
    { id: "16:9", label: "16:9" },
  ];

  const HANDLES: Handle[] = ["nw", "n", "ne", "e", "se", "s", "sw", "w"];

  const ICONS: Record<string, string[]> = {
    crop: ["M6 2v14a2 2 0 0 0 2 2h14", "M18 22V8a2 2 0 0 0-2-2H2"],
    draw: ["M12 20h9", "M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4Z"],
    text: ["M4 7V4h16v3", "M9 20h6", "M12 4v16"],
    adjust: ["M4 21v-7", "M4 10V3", "M12 21v-9", "M12 8V3", "M20 21v-5", "M20 12V3", "M1 14h6", "M9 8h6", "M17 16h6"],
    filters: [
      "M6.5 8.5a5.5 5.5 0 1 0 11 0a5.5 5.5 0 1 0-11 0",
      "M3 14.5a5.5 5.5 0 1 0 11 0a5.5 5.5 0 1 0-11 0",
      "M10 14.5a5.5 5.5 0 1 0 11 0a5.5 5.5 0 1 0-11 0",
    ],
    rotl: ["M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8", "M3 3v5h5"],
    rotr: ["M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8", "M21 3v5h-5"],
    flip: [
      "M8 3H5a2 2 0 0 0-2 2v14c0 1.1.9 2 2 2h3", "M16 3h3a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2h-3",
      "M12 20v2", "M12 14v2", "M12 8v2", "M12 2v2",
    ],
    undo: ["M9 14 4 9l5-5", "M4 9h10.5a5.5 5.5 0 0 1 0 11H11"],
    redo: ["m15 14 5-5-5-5", "M20 9H9.5a5.5 5.5 0 0 0 0 11H13"],
    reset: [
      "M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8", "M21 3v5h-5",
      "M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16", "M8 16H3v5",
    ],
    close: ["M18 6 6 18", "m6 6 12 12"],
    eraser: ["m7 21-4.3-4.3c-1-1-1-2.5 0-3.4l9.6-9.6c1-1 2.5-1 3.4 0l5.6 5.6c1 1 1 2.5 0 3.4L13 21", "M22 21H7", "m5 11 9 9"],
    trash: ["M3 6h18", "M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6", "M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"],
    check: ["M20 6 9 17l-5-5"],
    send: ["m22 2-7 20-4-9-9-4Z", "M22 2 11 13"],
  };

  // ───────────────────────── state ─────────────────────────

  let src = $state<HTMLImageElement | null>(null);
  let srcW = $state(0);
  let srcH = $state(0);

  let rot = $state<Rot>(0);
  let flip = $state(false);
  let crop = $state<Box | null>(null);
  let adj = $state<Adjust>({ ...ADJ0 });
  let preset = $state("original");
  let strokes = $state.raw<Stroke[]>([]);
  let texts = $state<TextItem[]>([]);

  let tool = $state<Tool | null>(null);
  let draft = $state<Box | null>(null);
  let aspect = $state<AspectId>("free");
  let portrait = $state(false);

  let brushColor = $state(PALETTE[0]);
  let brushSize = $state(8);
  let erasing = $state(false);

  let textColor = $state("#FFFFFF");
  let textStyle = $state<TextStyle>("plain");
  let textSizePx = $state(34);
  let selectedId = $state<number | null>(null);
  let editingId = $state<number | null>(null);
  let textSeq = 0;
  let editIsNew = false;
  let editDirty = false;
  let editStartText = "";
  let clickWasEditing = false;

  let past = $state.raw<Snap[]>([]);
  let future = $state.raw<Snap[]>([]);
  let sliderOpen: string | null = null;

  let processing = $state(false);
  let stageW = $state(0);
  let stageH = $state(0);
  let frameEl = $state<HTMLDivElement | null>(null);
  let baseCanvas = $state<HTMLCanvasElement | null>(null);
  let strokeCanvas = $state<HTMLCanvasElement | null>(null);
  let textLayer = $state<HTMLDivElement | null>(null);
  let hover = $state<Pt | null>(null);
  let thumbUrl = $state<string | null>(null);

  // ───────────────────────── derived ─────────────────────────

  const docW = $derived(rot % 180 === 0 ? srcW : srcH);
  const docH = $derived(rot % 180 === 0 ? srcH : srcW);
  const view = $derived<Box>(tool === "crop" || !crop ? { x: 0, y: 0, w: docW, h: docH } : crop);
  const scale = $derived.by(() => {
    if (!view.w || !view.h || !stageW || !stageH) return 1;
    const pad = 28;
    return Math.max(0.02, Math.min(2, (stageW - pad * 2) / view.w, (stageH - pad * 2) / view.h));
  });
  const activePreset = $derived(PRESETS.find((p) => p.id === preset) ?? PRESETS[0]);
  const tint = $derived(clamp(adj.warmth + activePreset.tint, -100, 100));
  const previewFilter = $derived(cssFilter(filterOps(scale, true)));
  const selected = $derived(texts.find((x) => x.id === selectedId) ?? null);
  const ratio = $derived(aspectValue(aspect, portrait));
  const dirty = $derived(
    rot !== 0 || flip || crop !== null || preset !== "original" || strokes.length > 0 || texts.length > 0 ||
      (Object.keys(ADJ0) as (keyof Adjust)[]).some((k) => adj[k] !== ADJ0[k]),
  );

  // ───────────────────────── helpers ─────────────────────────

  function clamp(v: number, lo: number, hi: number) { return Math.min(hi, Math.max(lo, v)); }
  function mid(a: Pt, b: Pt): Pt { return { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 }; }

  function mkCanvas(w: number, h: number) {
    const c = document.createElement("canvas");
    c.width = Math.max(1, Math.round(w));
    c.height = Math.max(1, Math.round(h));
    return c;
  }
  function ctx2d(c: HTMLCanvasElement) {
    const x = c.getContext("2d");
    if (!x) throw new Error("no 2d context");
    return x;
  }

  function contrastColor(hex: string) {
    const m = /^#?([0-9a-f]{6})$/i.exec(hex);
    if (!m) return "#FFFFFF";
    const n = parseInt(m[1], 16);
    const lum = 0.299 * (n >> 16) + 0.587 * ((n >> 8) & 255) + 0.114 * (n & 255);
    return lum > 150 ? "#0A0B10" : "#FFFFFF";
  }

  function blurPx() { return (adj.blur * Math.max(srcW, srcH)) / 1000; }

  function filterOps(blurScale: number, withBlur: boolean): Op[] {
    const list = [...activePreset.ops];
    if (adj.brightness !== 100) list.push(op("brightness", adj.brightness / 100));
    if (adj.contrast !== 100) list.push(op("contrast", adj.contrast / 100));
    if (adj.saturation !== 100) list.push(op("saturate", adj.saturation / 100));
    if (adj.hue !== 0) list.push(op("hue-rotate", adj.hue));
    if (withBlur && adj.blur > 0) list.push(op("blur", +(blurPx() * blurScale).toFixed(2)));
    return list;
  }

  function cssFilter(ops: Op[]) {
    if (!ops.length) return "none";
    return ops
      .map((o) => (o.fn === "hue-rotate" ? `hue-rotate(${o.v}deg)` : o.fn === "blur" ? `blur(${o.v}px)` : `${o.fn}(${o.v})`))
      .join(" ");
  }

  function tintColor(v: number) { return v > 0 ? "rgb(255,138,40)" : "rgb(40,130,255)"; }
  function tintAlpha(v: number) { return (Math.abs(v) / 100) * 0.6; }

  function aspectValue(id: AspectId, port: boolean): number | null {
    switch (id) {
      case "original": return docH ? docW / docH : null;
      case "1:1": return 1;
      case "4:3": return port ? 3 / 4 : 4 / 3;
      case "16:9": return port ? 9 / 16 : 16 / 9;
      default: return null;
    }
  }
  function aspectLabel(id: AspectId, label: string) {
    if (!portrait || aspect !== id) return label;
    return id === "4:3" ? "3:4" : id === "16:9" ? "9:16" : label;
  }

  function mapPt(p: Pt, kind: MapKind, W: number, H: number): Pt {
    if (kind === "cw") return { x: H - p.y, y: p.x };
    if (kind === "ccw") return { x: p.y, y: W - p.x };
    return { x: W - p.x, y: p.y };
  }
  function mapBox(r: Box, kind: MapKind, W: number, H: number): Box {
    const a = mapPt({ x: r.x, y: r.y }, kind, W, H);
    const b = mapPt({ x: r.x + r.w, y: r.y + r.h }, kind, W, H);
    return { x: Math.min(a.x, b.x), y: Math.min(a.y, b.y), w: Math.abs(a.x - b.x), h: Math.abs(a.y - b.y) };
  }

  /** Draw the source oriented into doc space, offset so `v` lands at 0,0. */
  function drawOriented(ctx: CanvasRenderingContext2D, im: HTMLImageElement, v: Box, r: Rot, f: boolean, W: number, H: number) {
    ctx.save();
    ctx.translate(-v.x, -v.y);
    ctx.translate(W / 2, H / 2);
    if (f) ctx.scale(-1, 1);
    ctx.rotate((r * Math.PI) / 180);
    ctx.drawImage(im, -im.naturalWidth / 2, -im.naturalHeight / 2);
    ctx.restore();
  }

  // ── strokes ──
  function strokeSetup(ctx: CanvasRenderingContext2D, s: Stroke) {
    ctx.globalCompositeOperation = s.erase ? "destination-out" : "source-over";
    ctx.strokeStyle = ctx.fillStyle = s.erase ? "#000" : s.color;
    ctx.lineWidth = s.size;
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
  }
  function drawStroke(ctx: CanvasRenderingContext2D, s: Stroke) {
    const p = s.pts;
    if (!p.length) return;
    strokeSetup(ctx, s);
    if (p.length === 1) {
      ctx.beginPath();
      ctx.arc(p[0].x, p[0].y, s.size / 2, 0, Math.PI * 2);
      ctx.fill();
      return;
    }
    ctx.beginPath();
    ctx.moveTo(p[0].x, p[0].y);
    for (let i = 1; i < p.length - 1; i++) {
      const m = mid(p[i], p[i + 1]);
      ctx.quadraticCurveTo(p[i].x, p[i].y, m.x, m.y);
    }
    const last = p[p.length - 1];
    ctx.lineTo(last.x, last.y);
    ctx.stroke();
  }
  /** Incremental: draws only the newest smoothed segment of a live stroke. */
  function drawStrokeTail(ctx: CanvasRenderingContext2D, s: Stroke) {
    const p = s.pts, n = p.length;
    if (n < 2) return;
    strokeSetup(ctx, s);
    const a = n >= 3 ? mid(p[n - 3], p[n - 2]) : p[0];
    const b = mid(p[n - 2], p[n - 1]);
    ctx.beginPath();
    ctx.moveTo(a.x, a.y);
    ctx.quadraticCurveTo(p[n - 2].x, p[n - 2].y, b.x, b.y);
    ctx.stroke();
  }
  function renderStrokes(ctx: CanvasRenderingContext2D, list: Stroke[], v: Box) {
    ctx.save();
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, ctx.canvas.width, ctx.canvas.height);
    ctx.translate(-v.x, -v.y);
    for (const s of list) drawStroke(ctx, s);
    ctx.restore();
  }

  // ── text (canvas) — mirrors the .txt CSS below ──
  function roundedRect(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
    r = Math.min(r, w / 2, h / 2);
    ctx.beginPath();
    ctx.moveTo(x + r, y);
    ctx.arcTo(x + w, y, x + w, y + h, r);
    ctx.arcTo(x + w, y + h, x, y + h, r);
    ctx.arcTo(x, y + h, x, y, r);
    ctx.arcTo(x, y, x + w, y, r);
    ctx.closePath();
  }
  function drawTexts(ctx: CanvasRenderingContext2D, list: TextItem[], v: Box) {
    const family = getComputedStyle(textLayer ?? document.body).fontFamily || "sans-serif";
    for (const it of list) {
      if (!it.text.trim()) continue;
      const fs = it.size;
      const lh = fs * 1.2;
      const lines = it.text.split("\n");
      ctx.save();
      ctx.font = `700 ${fs}px ${family}`;
      ctx.textAlign = "center";
      ctx.textBaseline = "alphabetic";
      const widths = lines.map((l) => ctx.measureText(l).width);
      const bw = Math.max(...widths);
      const m = ctx.measureText("Hg");
      const asc = m.fontBoundingBoxAscent ?? fs * 0.8;
      const desc = m.fontBoundingBoxDescent ?? fs * 0.2;
      const cx = it.x - v.x, cy = it.y - v.y;
      const bh = lines.length * lh;
      const top = cy - bh / 2;
      const k = contrastColor(it.color);
      if (it.style === "pill") {
        const px = fs * 0.55, py = fs * 0.3;
        roundedRect(ctx, cx - bw / 2 - px, top - py, bw + px * 2, bh + py * 2, fs * 0.45);
        ctx.fillStyle = it.color;
        ctx.fill();
      }
      lines.forEach((line, i) => {
        const y = top + i * lh + (lh - (asc + desc)) / 2 + asc;
        if (it.style === "outline") {
          ctx.strokeStyle = k;
          ctx.lineWidth = fs * 0.2;
          ctx.lineJoin = "round";
          ctx.strokeText(line, cx, y);
          ctx.fillStyle = it.color;
        } else if (it.style === "pill") {
          ctx.fillStyle = k;
        } else {
          ctx.fillStyle = it.color;
          ctx.shadowColor = "rgba(0,0,0,0.5)";
          ctx.shadowOffsetY = fs * 0.04;
          ctx.shadowBlur = fs * 0.18;
        }
        ctx.fillText(line, cx, y);
      });
      ctx.restore();
    }
  }

  // ── canvas filter fallback (engines without ctx.filter) ──
  let filterSupport: boolean | null = null;
  function canvasFilterSupported() {
    if (filterSupport !== null) return filterSupport;
    try {
      const c = document.createElement("canvas").getContext("2d");
      if (!c || typeof c.filter !== "string") return (filterSupport = false);
      c.filter = "blur(2px)";
      filterSupport = c.filter === "blur(2px)";
    } catch {
      filterSupport = false;
    }
    return filterSupport;
  }
  type Mat = number[]; // 3×4 row-major, offsets in 0..1
  const ID: Mat = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0];
  function opMatrix(o: Op): Mat | null {
    const v = o.v;
    switch (o.fn) {
      case "brightness": return [v, 0, 0, 0, 0, v, 0, 0, 0, 0, v, 0];
      case "contrast": { const b = 0.5 - 0.5 * v; return [v, 0, 0, b, 0, v, 0, b, 0, 0, v, b]; }
      case "saturate": return [
        0.213 + 0.787 * v, 0.715 - 0.715 * v, 0.072 - 0.072 * v, 0,
        0.213 - 0.213 * v, 0.715 + 0.285 * v, 0.072 - 0.072 * v, 0,
        0.213 - 0.213 * v, 0.715 - 0.715 * v, 0.072 + 0.928 * v, 0,
      ];
      case "grayscale": { const g = 1 - clamp(v, 0, 1); return [
        0.2126 + 0.7874 * g, 0.7152 - 0.7152 * g, 0.0722 - 0.0722 * g, 0,
        0.2126 - 0.2126 * g, 0.7152 + 0.2848 * g, 0.0722 - 0.0722 * g, 0,
        0.2126 - 0.2126 * g, 0.7152 - 0.7152 * g, 0.0722 + 0.9278 * g, 0,
      ]; }
      case "sepia": { const s = 1 - clamp(v, 0, 1); return [
        0.393 + 0.607 * s, 0.769 - 0.769 * s, 0.189 - 0.189 * s, 0,
        0.349 - 0.349 * s, 0.686 + 0.314 * s, 0.168 - 0.168 * s, 0,
        0.272 - 0.272 * s, 0.534 - 0.534 * s, 0.131 + 0.869 * s, 0,
      ]; }
      case "hue-rotate": { const a = (v * Math.PI) / 180, c = Math.cos(a), s = Math.sin(a); return [
        0.213 + c * 0.787 - s * 0.213, 0.715 - c * 0.715 - s * 0.715, 0.072 - c * 0.072 + s * 0.928, 0,
        0.213 - c * 0.213 + s * 0.143, 0.715 + c * 0.285 + s * 0.14, 0.072 - c * 0.072 - s * 0.283, 0,
        0.213 - c * 0.213 - s * 0.787, 0.715 - c * 0.715 + s * 0.715, 0.072 + c * 0.928 + s * 0.072, 0,
      ]; }
      default: return null;
    }
  }
  function matMul(a: Mat, b: Mat): Mat {
    const o = new Array(12).fill(0);
    for (let i = 0; i < 3; i++) {
      for (let j = 0; j < 4; j++) {
        let s = 0;
        for (let k = 0; k < 3; k++) s += a[i * 4 + k] * b[k * 4 + j];
        o[i * 4 + j] = s + (j === 3 ? a[i * 4 + 3] : 0);
      }
    }
    return o;
  }
  function applyMatrix(c: HTMLCanvasElement, ops: Op[]) {
    let m = ID;
    for (const o of ops) { const k = opMatrix(o); if (k) m = matMul(k, m); }
    if (m.every((v, i) => v === ID[i])) return;
    const ctx = ctx2d(c);
    const img = ctx.getImageData(0, 0, c.width, c.height);
    const d = img.data;
    const o0 = m[3] * 255, o1 = m[7] * 255, o2 = m[11] * 255;
    for (let i = 0; i < d.length; i += 4) {
      const r = d[i], g = d[i + 1], b = d[i + 2];
      d[i] = m[0] * r + m[1] * g + m[2] * b + o0;
      d[i + 1] = m[4] * r + m[5] * g + m[6] * b + o1;
      d[i + 2] = m[8] * r + m[9] * g + m[10] * b + o2;
    }
    ctx.putImageData(img, 0, 0);
  }
  function softBlur(c: HTMLCanvasElement, px: number) {
    const f = Math.max(1, px / 1.5);
    const tmp = mkCanvas(c.width / f, c.height / f);
    const tctx = ctx2d(tmp);
    tctx.imageSmoothingQuality = "high";
    tctx.drawImage(c, 0, 0, tmp.width, tmp.height);
    const ctx = ctx2d(c);
    ctx.clearRect(0, 0, c.width, c.height);
    ctx.imageSmoothingQuality = "high";
    ctx.drawImage(tmp, 0, 0, c.width, c.height);
  }

  // ───────────────────────── history ─────────────────────────

  function snap(): Snap {
    return {
      rot, flip, crop: crop ? { ...crop } : null, adj: { ...adj }, preset, strokes,
      texts: $state.snapshot(texts) as TextItem[],
    };
  }
  function restore(s: Snap) {
    rot = s.rot;
    flip = s.flip;
    crop = s.crop ? { ...s.crop } : null;
    adj = { ...s.adj };
    preset = s.preset;
    strokes = s.strokes;
    texts = s.texts.map((x) => ({ ...x }));
    editingId = null;
    if (selectedId !== null && !texts.some((x) => x.id === selectedId)) selectedId = null;
  }
  function checkpoint() {
    past = [...past.slice(-99), snap()];
    future = [];
  }
  function leaveTransient() {
    finishEditing();
    if (tool === "crop") { draft = null; tool = null; }
    sliderOpen = null;
  }
  function undo() {
    leaveTransient();
    if (!past.length) return;
    future = [...future, snap()];
    const s = past[past.length - 1];
    past = past.slice(0, -1);
    restore(s);
  }
  function redo() {
    leaveTransient();
    if (!future.length) return;
    past = [...past, snap()];
    const s = future[future.length - 1];
    future = future.slice(0, -1);
    restore(s);
  }
  function applyDefaults() {
    rot = 0; flip = false; crop = null; adj = { ...ADJ0 }; preset = "original";
    strokes = []; texts = []; selectedId = null; editingId = null;
  }
  function resetAll() {
    leaveTransient();
    if (!dirty) return;
    checkpoint();
    applyDefaults();
    tool = null;
  }

  // ───────────────────────── loading ─────────────────────────

  let prevReq: EditRequest | null = null;
  let loadToken = 0;
  $effect(() => {
    const s = $editorState;
    untrack(() => {
      if (s === prevReq) return;
      prevReq = s;
      loadToken++;
      src = null;
      thumbUrl = null;
      if (!s) return;
      applyDefaults();
      past = []; future = [];
      tool = null; draft = null; processing = false; hover = null;
      const token = loadToken;
      const im = new Image();
      im.onload = () => {
        if (token !== loadToken) return;
        srcW = im.naturalWidth;
        srcH = im.naturalHeight;
        src = im;
      };
      im.onerror = () => {
        if (token !== loadToken) return;
        showToast(tr("Couldn't open this image"));
        closeEditor(false);
      };
      im.src = bytesToDataUrl(s.bytes, s.mime);
    });
  });

  // ───────────────────────── rendering ─────────────────────────

  // Base layer: geometry only, at natural resolution. Colour work is
  // previewed with a CSS filter (same syntax as ctx.filter on export).
  $effect(() => {
    const c = baseCanvas, im = src, v = view, r = rot, f = flip, W = docW, H = docH;
    if (!c || !im || !v.w || !v.h) return;
    c.width = Math.round(v.w);
    c.height = Math.round(v.h);
    const ctx = ctx2d(c);
    ctx.clearRect(0, 0, c.width, c.height);
    drawOriented(ctx, im, v, r, f, W, H);
  });

  $effect(() => {
    const c = strokeCanvas, v = view, list = strokes;
    if (!c || !src || !v.w || !v.h) return;
    c.width = Math.round(v.w);
    c.height = Math.round(v.h);
    renderStrokes(ctx2d(c), list, v);
  });

  // Filter thumbnails (only while the Filters tool is open).
  $effect(() => {
    if (tool !== "filters" || !src) return;
    const r = crop ?? { x: 0, y: 0, w: docW, h: docH };
    const k = 128 / Math.max(r.w, r.h);
    const c = mkCanvas(r.w * k, r.h * k);
    const ctx = ctx2d(c);
    ctx.scale(k, k);
    drawOriented(ctx, src, r, rot, flip, docW, docH);
    thumbUrl = c.toDataURL("image/png");
  });

  // ───────────────────────── tools ─────────────────────────

  function setTool(next: Tool | null) {
    finishEditing();
    if (tool === "crop" && next !== "crop") draft = null;
    if (next !== "text") selectedId = null;
    if (next === "crop" && tool !== "crop") {
      draft = crop ? { ...crop } : { x: 0, y: 0, w: docW, h: docH };
      aspect = "free";
      portrait = false;
    }
    tool = next;
  }
  function toggleTool(next: Tool) { setTool(tool === next ? null : next); }

  function rotateBy(dir: 1 | -1) {
    if (!src) return;
    finishEditing();
    checkpoint();
    transformAll(dir === 1 ? "cw" : "ccw", docW, docH);
    rot = ((((rot + (flip ? -dir : dir) * 90) % 360) + 360) % 360) as Rot;
    if (draft && (aspect === "4:3" || aspect === "16:9")) portrait = !portrait;
  }
  function flipH() {
    if (!src) return;
    finishEditing();
    checkpoint();
    transformAll("flip", docW, docH);
    flip = !flip;
  }
  function transformAll(kind: MapKind, W: number, H: number) {
    strokes = strokes.map((s) => ({ ...s, pts: s.pts.map((p) => mapPt(p, kind, W, H)) }));
    for (const it of texts) {
      const p = mapPt(it, kind, W, H);
      it.x = p.x;
      it.y = p.y;
    }
    if (crop) crop = mapBox(crop, kind, W, H);
    if (draft) draft = mapBox(draft, kind, W, H);
  }

  function selectPreset(id: string) {
    if (id === preset) return;
    checkpoint();
    preset = id;
  }

  /** Range inputs: one history entry per drag/keyboard burst. */
  function beginSlider(key: string) {
    if (sliderOpen === key) return;
    sliderOpen = key;
    checkpoint();
  }
  function endSlider() { sliderOpen = null; }

  function setAdj(key: keyof Adjust, v: number) {
    if (adj[key] === v) return;
    beginSlider("adj:" + key);
    adj[key] = v;
  }
  function resetAdj(key?: keyof Adjust) {
    const keys = key ? [key] : (Object.keys(ADJ0) as (keyof Adjust)[]);
    if (keys.every((k) => adj[k] === ADJ0[k])) return;
    checkpoint();
    for (const k of keys) adj[k] = ADJ0[k];
  }
  function fmtAdj(key: keyof Adjust, v: number) {
    if (key === "hue") return `${v > 0 ? "+" : ""}${v}°`;
    if (key === "blur") return String(v);
    const d = key === "warmth" ? v : v - 100;
    return `${d > 0 ? "+" : ""}${d}`;
  }

  // ── crop ──
  function minCrop() { return Math.max(8, 28 / scale); }
  function fitAspect(a: number) {
    if (!draft) return;
    let w = docW, h = docW / a;
    if (h > docH) { h = docH; w = docH * a; }
    const cx = draft.x + draft.w / 2, cy = draft.y + draft.h / 2;
    draft = { x: clamp(cx - w / 2, 0, docW - w), y: clamp(cy - h / 2, 0, docH - h), w, h };
  }
  function chooseAspect(id: AspectId) {
    if (id === aspect && (id === "4:3" || id === "16:9")) portrait = !portrait;
    else {
      aspect = id;
      portrait = (id === "4:3" || id === "16:9") && docH > docW;
    }
    const a = aspectValue(aspect, portrait);
    if (a) fitAspect(a);
  }
  function resizeBox(r0: Box, h: Handle, dx: number, dy: number, a: number | null): Box {
    const W = docW, H = docH, MIN = minCrop();
    let L = r0.x, T = r0.y, R = r0.x + r0.w, B = r0.y + r0.h;
    if (h.includes("w")) L = clamp(L + dx, 0, R - MIN);
    if (h.includes("e")) R = clamp(R + dx, L + MIN, W);
    if (h.includes("n")) T = clamp(T + dy, 0, B - MIN);
    if (h.includes("s")) B = clamp(B + dy, T + MIN, H);
    if (!a) return { x: L, y: T, w: R - L, h: B - T };
    if (h.length === 2) {
      const ax = h.includes("w") ? r0.x + r0.w : r0.x;
      const ay = h.includes("n") ? r0.y + r0.h : r0.y;
      let w = R - L, hh = B - T;
      if (w / a >= hh) hh = w / a; else w = hh * a;
      const maxW = h.includes("w") ? ax : W - ax;
      const maxH = h.includes("n") ? ay : H - ay;
      if (w > maxW) { w = maxW; hh = w / a; }
      if (hh > maxH) { hh = maxH; w = hh * a; }
      return { x: h.includes("w") ? ax - w : ax, y: h.includes("n") ? ay - hh : ay, w, h: hh };
    }
    if (h === "e" || h === "w") {
      let w = R - L, hh = w / a;
      const cy = r0.y + r0.h / 2;
      const maxH = 2 * Math.min(cy, H - cy);
      if (hh > maxH) { hh = maxH; w = hh * a; }
      return { x: h === "w" ? r0.x + r0.w - w : r0.x, y: cy - hh / 2, w, h: hh };
    }
    let hh = B - T, w = hh * a;
    const cx = r0.x + r0.w / 2;
    const maxW = 2 * Math.min(cx, W - cx);
    if (w > maxW) { w = maxW; hh = w / a; }
    return { x: cx - w / 2, y: h === "n" ? r0.y + r0.h - hh : r0.y, w, h: hh };
  }
  function applyCrop() {
    if (!draft) return;
    const x = clamp(Math.round(draft.x), 0, docW - 1);
    const y = clamp(Math.round(draft.y), 0, docH - 1);
    const r: Box = { x, y, w: clamp(Math.round(draft.w), 1, docW - x), h: clamp(Math.round(draft.h), 1, docH - y) };
    const full = r.x === 0 && r.y === 0 && r.w === docW && r.h === docH;
    const next = full ? null : r;
    const same = next === crop || (!!next && !!crop && next.x === crop.x && next.y === crop.y && next.w === crop.w && next.h === crop.h);
    if (!same) { checkpoint(); crop = next; }
    draft = null;
    tool = null;
  }

  // ── pointer plumbing ──
  function drag(e: PointerEvent, move: (dx: number, dy: number) => void, end?: () => void) {
    const el = e.currentTarget as HTMLElement;
    const id = e.pointerId, x0 = e.clientX, y0 = e.clientY;
    try { el.setPointerCapture(id); } catch { /* ignore */ }
    const mv = (ev: PointerEvent) => { if (ev.pointerId === id) move(ev.clientX - x0, ev.clientY - y0); };
    const up = (ev: PointerEvent) => {
      if (ev.pointerId !== id) return;
      el.removeEventListener("pointermove", mv);
      el.removeEventListener("pointerup", up);
      el.removeEventListener("pointercancel", up);
      end?.();
    };
    el.addEventListener("pointermove", mv);
    el.addEventListener("pointerup", up);
    el.addEventListener("pointercancel", up);
  }
  function toDoc(e: { clientX: number; clientY: number }): Pt {
    const r = frameEl?.getBoundingClientRect();
    if (!r) return { x: 0, y: 0 };
    return { x: (e.clientX - r.left) / scale + view.x, y: (e.clientY - r.top) / scale + view.y };
  }

  function cropMoveDown(e: PointerEvent) {
    if (!draft || e.button !== 0) return;
    e.stopPropagation();
    e.preventDefault();
    const r0 = { ...draft };
    drag(e, (dx, dy) => {
      draft = { ...r0, x: clamp(r0.x + dx / scale, 0, docW - r0.w), y: clamp(r0.y + dy / scale, 0, docH - r0.h) };
    });
  }
  function cropHandleDown(e: PointerEvent, h: Handle) {
    if (!draft || e.button !== 0) return;
    e.stopPropagation();
    e.preventDefault();
    const r0 = { ...draft };
    drag(e, (dx, dy) => { draft = resizeBox(r0, h, dx / scale, dy / scale, ratio); });
  }

  // ── drawing ──
  let live: Stroke | null = null;
  function liveCtx() {
    if (!strokeCanvas) return null;
    const ctx = ctx2d(strokeCanvas);
    ctx.setTransform(1, 0, 0, 1, -view.x, -view.y);
    return ctx;
  }
  function framePointerDown(e: PointerEvent) {
    clickWasEditing = editingId !== null;
    if (e.button !== 0 || !src) return;
    if (tool === "draw") {
      e.preventDefault();
      const p = toDoc(e);
      live = { color: brushColor, size: brushSize / scale, erase: erasing, pts: [p] };
      const ctx = liveCtx();
      if (ctx) { drawStroke(ctx, live); ctx.setTransform(1, 0, 0, 1, 0, 0); }
      drag(e, (dx, dy) => {
        if (!live) return;
        const raw = toDoc({ clientX: e.clientX + dx, clientY: e.clientY + dy });
        const last = live.pts[live.pts.length - 1];
        if (Math.hypot(raw.x - last.x, raw.y - last.y) * scale < 1.5) return;
        // light low-pass to take the jitter out of mouse/pen input
        live.pts.push({ x: last.x + (raw.x - last.x) * 0.6, y: last.y + (raw.y - last.y) * 0.6 });
        const c = liveCtx();
        if (c) { drawStrokeTail(c, live); c.setTransform(1, 0, 0, 1, 0, 0); }
      }, () => {
        if (!live) return;
        const s = live;
        live = null;
        checkpoint();
        strokes = [...strokes, s];
      });
    } else if (tool !== "text" && tool !== "crop") {
      selectedId = null;
    }
  }
  function frameHover(e: PointerEvent) {
    if (tool !== "draw" || !frameEl) { if (hover) hover = null; return; }
    const r = frameEl.getBoundingClientRect();
    hover = { x: e.clientX - r.left, y: e.clientY - r.top };
  }

  // ── text ──
  function frameClick(e: MouseEvent) {
    if (tool !== "text" || !src) return;
    if (clickWasEditing) { clickWasEditing = false; selectedId = null; return; }
    if (selectedId !== null) { selectedId = null; return; }
    const p = toDoc(e);
    checkpoint();
    const id = ++textSeq;
    texts.push({ id, x: p.x, y: p.y, text: "", color: textColor, size: textSizePx / scale, style: textStyle });
    selectedId = id;
    editingId = id;
    editIsNew = true;
    editDirty = false;
    editStartText = "";
  }
  function startEditing(id: number) {
    const it = texts.find((x) => x.id === id);
    if (!it) return;
    finishEditing();
    if (tool !== "text") setTool("text");
    checkpoint();
    selectedId = id;
    editingId = id;
    editIsNew = false;
    editDirty = false;
    editStartText = it.text;
  }
  function finishEditing() {
    const id = editingId;
    if (id === null) return;
    editingId = null;
    const i = texts.findIndex((x) => x.id === id);
    if (i < 0) return;
    const txt = texts[i].text.replace(/\s+$/, "");
    if (!txt.trim()) {
      texts.splice(i, 1);
      if (selectedId === id) selectedId = null;
      if (editIsNew && !editDirty) past = past.slice(0, -1);
    } else {
      texts[i].text = txt;
      if (!editIsNew && !editDirty && txt === editStartText) past = past.slice(0, -1);
    }
    const a = document.activeElement;
    if (a instanceof HTMLElement && a.isContentEditable) a.blur();
  }
  function editable(node: HTMLElement, text: string) {
    node.innerText = text;
    node.focus();
    const sel = window.getSelection();
    if (sel) {
      const range = document.createRange();
      range.selectNodeContents(node);
      sel.removeAllRanges();
      sel.addRange(range);
    }
  }
  function textDown(e: PointerEvent, item: TextItem) {
    if (e.button !== 0) return;
    e.stopPropagation();
    e.preventDefault();
    finishEditing();
    const wasSelected = selectedId === item.id;
    if (tool !== "text") setTool("text");
    selectedId = item.id;
    textColor = item.color;
    textStyle = item.style;
    const sx = item.x, sy = item.y;
    let moved = false;
    drag(e, (dx, dy) => {
      if (!moved && Math.hypot(dx, dy) < 3) return;
      if (!moved) { moved = true; checkpoint(); }
      item.x = sx + dx / scale;
      item.y = sy + dy / scale;
    }, () => {
      if (!moved && wasSelected) startEditing(item.id);
    });
  }
  /** Style change on the selected box. While typing, the edit's own
   *  checkpoint already covers it. */
  function touchSelected(fn: (it: TextItem) => void) {
    const it = selected;
    if (!it) return;
    if (editingId === it.id) editDirty = true;
    else checkpoint();
    fn(it);
  }
  function setTextColor(c: string) {
    textColor = c;
    if (selected && selected.color !== c) touchSelected((it) => (it.color = c));
  }
  function setTextStyle(s: TextStyle) {
    textStyle = s;
    if (selected && selected.style !== s) touchSelected((it) => (it.style = s));
  }
  function setTextSize(px: number) {
    textSizePx = px;
    const it = selected;
    if (!it) return;
    if (editingId === it.id) editDirty = true;
    else beginSlider("text-size");
    it.size = px / scale;
  }
  function deleteSelected() {
    const id = selectedId;
    if (id === null) return;
    const wasEditing = editingId === id;
    if (!wasEditing) checkpoint();
    else if (editIsNew && !editDirty) past = past.slice(0, -1); // never existed
    editingId = null;
    texts = texts.filter((x) => x.id !== id);
    selectedId = null;
  }
  function textCss(it: TextItem) {
    return `left:${(it.x - view.x) * scale}px;top:${(it.y - view.y) * scale}px;font-size:${it.size * scale}px;--c:${it.color};--k:${contrastColor(it.color)}`;
  }

  // ───────────────────────── keyboard ─────────────────────────

  function isTyping(el: EventTarget | null) {
    if (!(el instanceof HTMLElement)) return false;
    if (el.isContentEditable || el instanceof HTMLTextAreaElement) return true;
    if (el instanceof HTMLInputElement) return !["range", "checkbox", "radio", "button", "color", "submit"].includes(el.type);
    return false;
  }
  function onKey(e: KeyboardEvent) {
    if (!$editorState) return;
    e.stopPropagation(); // the editor owns the keyboard while open
    if (isTyping(e.target)) {
      if (e.key === "Escape") { e.preventDefault(); finishEditing(); }
      return;
    }
    if (processing) { e.preventDefault(); return; }
    const mod = e.ctrlKey || e.metaKey;
    const onButton = e.target instanceof HTMLButtonElement;
    if (e.key === "Escape") {
      e.preventDefault();
      if (editingId !== null) finishEditing();
      else if (tool) setTool(null);
      else cancel();
    } else if (mod && e.key === "Enter") {
      e.preventDefault();
      void send();
    } else if (mod && e.code === "KeyZ") {
      e.preventDefault();
      if (e.shiftKey) redo(); else undo();
    } else if (mod && e.code === "KeyY") {
      e.preventDefault();
      redo();
    } else if (e.key === "Enter" && tool === "crop" && !onButton) {
      e.preventDefault();
      applyCrop();
    } else if ((e.key === "Delete" || e.key === "Backspace") && selectedId !== null) {
      e.preventDefault();
      deleteSelected();
    }
  }
  $effect(() => {
    if (!$editorState) return;
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  });

  // ───────────────────────── export / send ─────────────────────────

  function renderExport(isPng: boolean): HTMLCanvasElement {
    const im = src!;
    const W = docW, H = docH;
    const r = crop ?? { x: 0, y: 0, w: W, h: H };
    const base = mkCanvas(r.w, r.h);
    const w = base.width, h = base.height;
    const bctx = ctx2d(base);
    const colorOps = filterOps(1, false);
    if (canvasFilterSupported()) {
      bctx.filter = cssFilter(filterOps(1, true));
      drawOriented(bctx, im, r, rot, flip, W, H);
      if (adj.blur > 0 && !isPng) {
        // Blur fades the edges to transparent; back-fill them so JPEG
        // doesn't get a dark rim.
        bctx.globalCompositeOperation = "destination-over";
        bctx.filter = cssFilter(colorOps);
        drawOriented(bctx, im, r, rot, flip, W, H);
      }
      bctx.filter = "none";
      bctx.globalCompositeOperation = "source-over";
    } else {
      drawOriented(bctx, im, r, rot, flip, W, H);
      if (adj.blur > 0) softBlur(base, blurPx());
      applyMatrix(base, colorOps);
    }
    if (tint) {
      let mask: HTMLCanvasElement | null = null;
      if (isPng) { mask = mkCanvas(w, h); ctx2d(mask).drawImage(base, 0, 0); }
      bctx.globalCompositeOperation = "soft-light";
      bctx.globalAlpha = tintAlpha(tint);
      bctx.fillStyle = tintColor(tint);
      bctx.fillRect(0, 0, w, h);
      bctx.globalAlpha = 1;
      if (mask) { bctx.globalCompositeOperation = "destination-in"; bctx.drawImage(mask, 0, 0); }
      bctx.globalCompositeOperation = "source-over";
    }

    const out = mkCanvas(w, h);
    const o = ctx2d(out);
    if (!isPng) { o.fillStyle = "#FFFFFF"; o.fillRect(0, 0, w, h); }
    o.drawImage(base, 0, 0);
    if (strokes.length) {
      const sl = mkCanvas(w, h);
      renderStrokes(ctx2d(sl), strokes, r);
      o.drawImage(sl, 0, 0);
    }
    drawTexts(o, texts, r);
    return out;
  }

  async function send() {
    const s = $editorState;
    if (!s || !src || processing) return;
    finishEditing();
    if (tool === "crop" && draft) applyCrop();
    processing = true;
    try {
      await new Promise((r) => requestAnimationFrame(() => r(null))); // let "Processing…" paint
      const isPng = s.mime === "image/png" || (!s.mime && /\.png$/i.test(s.name));
      const mime = isPng ? "image/png" : "image/jpeg";
      const ext = isPng ? "png" : "jpg";
      const canvas = renderExport(isPng);
      const blob: Blob = await new Promise((res, rej) =>
        canvas.toBlob((b) => (b ? res(b) : rej(new Error("toBlob failed"))), mime, isPng ? undefined : 0.92),
      );
      const bytes = Array.from(new Uint8Array(await blob.arrayBuffer()));
      const base = s.name.replace(/\.[^.]+$/, "") || "photo";
      await api.sendEditedImage(s.peerId, `${base}-edited.${ext}`, mime, bytes);
      await loadActiveMessages();
      closeEditor(true);
    } catch (e) {
      showToast(tr("Failed to send edited image: {error}", { error: String(e) }));
      processing = false;
    }
  }

  function cancel() {
    if (processing) return;
    closeEditor(false);
  }
</script>

{#snippet icon(name: string, size = 18)}
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    {#each ICONS[name] ?? [] as d}<path {d} />{/each}
  </svg>
{/snippet}

{#snippet swatches(current: string, pick: (c: string) => void)}
  <div class="swatches" role="radiogroup" aria-label={$t("Colour")}>
    {#each PALETTE as c}
      <button
        class="sw"
        class:on={c.toLowerCase() === current.toLowerCase()}
        style:--sw={c}
        role="radio"
        aria-checked={c.toLowerCase() === current.toLowerCase()}
        aria-label={c}
        onmousedown={(e) => e.preventDefault()}
        onclick={() => pick(c)}
      ></button>
    {/each}
  </div>
{/snippet}

{#if $editorState}
  <div class="pe-backdrop">
    <div class="pe" role="dialog" aria-modal="true" aria-label={$t("Edit photo")}>
      <header class="pe-head">
        <div class="title">{$t("Edit photo")}</div>
        <div class="hist">
          <button class="ib" onclick={undo} disabled={!past.length || processing} title={`${$t("Undo")} (Ctrl+Z)`} aria-label={$t("Undo")}>
            {@render icon("undo")}
          </button>
          <button class="ib" onclick={redo} disabled={!future.length || processing} title={`${$t("Redo")} (Ctrl+Shift+Z)`} aria-label={$t("Redo")}>
            {@render icon("redo")}
          </button>
          <span class="vsep"></span>
          <button class="ib" onclick={resetAll} disabled={!dirty || processing} title={$t("Reset all")}>
            {@render icon("reset", 16)}<span>{$t("Reset")}</span>
          </button>
        </div>
        <button class="ib close" onclick={cancel} disabled={processing} title={`${$t("Close")} (Esc)`} aria-label={$t("Close")}>
          {@render icon("close")}
        </button>
      </header>

      <div class="stage" bind:clientWidth={stageW} bind:clientHeight={stageH}>
        {#if src}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div
            class="frame"
            class:t-draw={tool === "draw"}
            class:t-text={tool === "text"}
            bind:this={frameEl}
            style:width="{view.w * scale}px"
            style:height="{view.h * scale}px"
            onpointerdown={framePointerDown}
            onpointermove={frameHover}
            onpointerleave={() => (hover = null)}
            onclick={frameClick}
          >
            <canvas class="layer" bind:this={baseCanvas} style:filter={previewFilter}></canvas>
            {#if tint}
              <div class="layer tint" style:background={tintColor(tint)} style:opacity={tintAlpha(tint)}></div>
            {/if}
            <canvas class="layer passive" bind:this={strokeCanvas}></canvas>

            <div class="layer texts" class:inert={tool === "draw" || tool === "crop"} bind:this={textLayer}>
              {#each texts as item (item.id)}
                {#if editingId === item.id}
                  <div
                    class="txt s-{item.style} sel editing"
                    style={textCss(item)}
                    contenteditable="plaintext-only"
                    role="textbox"
                    tabindex="0"
                    aria-multiline="true"
                    spellcheck="false"
                    use:editable={item.text}
                    oninput={(e) => (item.text = e.currentTarget.innerText)}
                    onblur={finishEditing}
                    onpointerdown={(e) => e.stopPropagation()}
                    onclick={(e) => e.stopPropagation()}
                  ></div>
                {:else}
                  <div
                    class="txt s-{item.style}"
                    class:sel={selectedId === item.id}
                    style={textCss(item)}
                    role="button"
                    tabindex="-1"
                    onpointerdown={(e) => textDown(e, item)}
                    onclick={(e) => e.stopPropagation()}
                    ondblclick={() => startEditing(item.id)}
                  >{item.text}</div>
                {/if}
              {/each}
            </div>

            {#if tool === "crop" && draft}
              <div
                class="crop-box"
                role="presentation"
                style:left="{(draft.x - view.x) * scale}px"
                style:top="{(draft.y - view.y) * scale}px"
                style:width="{draft.w * scale}px"
                style:height="{draft.h * scale}px"
                onpointerdown={cropMoveDown}
              >
                <i class="g gv" style:left="33.333%"></i>
                <i class="g gv" style:left="66.666%"></i>
                <i class="g gh" style:top="33.333%"></i>
                <i class="g gh" style:top="66.666%"></i>
                {#each HANDLES as h}
                  <span class="hd hd-{h}" class:corner={h.length === 2} role="presentation" onpointerdown={(e) => cropHandleDown(e, h)}></span>
                {/each}
              </div>
            {/if}

            {#if tool === "draw" && hover}
              <div
                class="brush-ring"
                class:erase={erasing}
                style:left="{hover.x}px"
                style:top="{hover.y}px"
                style:width="{brushSize}px"
                style:height="{brushSize}px"
                style:--bc={erasing ? "#fff" : brushColor}
              ></div>
            {/if}
          </div>
        {:else}
          <div class="loading">{$t("Loading…")}</div>
        {/if}
      </div>

      {#if tool}
        <div class="panel">
          {#if tool === "crop"}
            <div class="chips" role="radiogroup" aria-label={$t("Aspect ratio")}>
              {#each ASPECTS as a}
                <button class="chip" class:on={aspect === a.id} role="radio" aria-checked={aspect === a.id} onclick={() => chooseAspect(a.id)}>
                  {a.id === "free" || a.id === "original" ? $t(a.label) : aspectLabel(a.id, a.label)}
                </button>
              {/each}
            </div>
            <div class="grow"></div>
            <button class="btn-ghost sm" onclick={() => setTool(null)}>{$t("Cancel")}</button>
            <button class="btn-primary sm" onclick={applyCrop}>{@render icon("check", 15)}{$t("Apply")}</button>
          {:else if tool === "draw"}
            <div class="seg">
              <button class:on={!erasing} onclick={() => (erasing = false)}>{@render icon("draw", 15)}{$t("Brush")}</button>
              <button class:on={erasing} onclick={() => (erasing = true)}>{@render icon("eraser", 15)}{$t("Eraser")}</button>
            </div>
            {@render swatches(brushColor, (c) => { brushColor = c; erasing = false; })}
            <label class="size">
              <span class="lbl">{$t("Size")}</span>
              <input type="range" min="2" max="60" step="1" value={brushSize} oninput={(e) => (brushSize = +e.currentTarget.value)} />
              <span class="dot" style:--d="{Math.min(brushSize, 26)}px" style:--bc={erasing ? "var(--fg-muted)" : brushColor}></span>
            </label>
          {:else if tool === "text"}
            {@render swatches(textColor, setTextColor)}
            <div class="seg">
              {#each [["plain", "Plain"], ["pill", "Background"], ["outline", "Outline"]] as [id, label]}
                <button
                  class:on={textStyle === id}
                  onmousedown={(e) => e.preventDefault()}
                  onclick={() => setTextStyle(id as TextStyle)}
                >{$t(label)}</button>
              {/each}
            </div>
            <label class="size">
              <span class="lbl">{$t("Size")}</span>
              <input
                type="range" min="12" max="120" step="1"
                value={selected ? Math.round(selected.size * scale) : textSizePx}
                oninput={(e) => setTextSize(+e.currentTarget.value)}
                onchange={endSlider}
              />
            </label>
            <div class="grow"></div>
            {#if selected}
              <button class="btn-ghost sm danger" onmousedown={(e) => e.preventDefault()} onclick={deleteSelected}>
                {@render icon("trash", 15)}{$t("Delete")}
              </button>
            {:else}
              <span class="hint">{$t("Click on the photo to add text")}</span>
            {/if}
          {:else if tool === "adjust"}
            <div class="adj">
              {#each SLIDERS as sl}
                <label class="slider">
                  <span class="row">
                    <span class="lbl">{$t(sl.label)}</span>
                    <span class="val" class:changed={adj[sl.key] !== ADJ0[sl.key]}>{fmtAdj(sl.key, adj[sl.key])}</span>
                  </span>
                  <input
                    type="range" min={sl.min} max={sl.max} step="1"
                    value={adj[sl.key]}
                    oninput={(e) => setAdj(sl.key, +e.currentTarget.value)}
                    onchange={endSlider}
                    ondblclick={() => resetAdj(sl.key)}
                  />
                </label>
              {/each}
            </div>
            <button class="btn-ghost sm" onclick={() => resetAdj()} title={$t("Reset adjustments")}>{@render icon("reset", 14)}</button>
          {:else if tool === "filters"}
            <div class="filters">
              {#each PRESETS as p}
                <button class="fl" class:on={preset === p.id} onclick={() => selectPreset(p.id)}>
                  <span class="th">
                    {#if thumbUrl}<img src={thumbUrl} alt="" style:filter={cssFilter(p.ops)} />{/if}
                    {#if p.tint}<span class="th-tint" style:background={tintColor(p.tint)} style:opacity={tintAlpha(p.tint)}></span>{/if}
                  </span>
                  <span class="fl-l">{$t(p.label)}</span>
                </button>
              {/each}
            </div>
          {/if}
        </div>
      {/if}

      <footer class="pe-bar">
        <div class="tools">
          {#each [["crop", "Crop"], ["draw", "Draw"], ["text", "Text"], ["adjust", "Adjust"], ["filters", "Filters"]] as [id, label]}
            <button class="tb" class:on={tool === id} onclick={() => toggleTool(id as Tool)} disabled={!src || processing} aria-pressed={tool === id}>
              {@render icon(id)}<span>{$t(label)}</span>
            </button>
          {/each}
          <span class="vsep"></span>
          <button class="tb icon" onclick={() => rotateBy(-1)} disabled={!src || processing} title={$t("Rotate left")} aria-label={$t("Rotate left")}>
            {@render icon("rotl")}
          </button>
          <button class="tb icon" onclick={() => rotateBy(1)} disabled={!src || processing} title={$t("Rotate right")} aria-label={$t("Rotate right")}>
            {@render icon("rotr")}
          </button>
          <button class="tb icon" class:on={flip} onclick={flipH} disabled={!src || processing} title={$t("Flip horizontal")} aria-label={$t("Flip horizontal")}>
            {@render icon("flip")}
          </button>
        </div>
        <div class="actions">
          <button class="btn-ghost" onclick={cancel} disabled={processing}>{$t("Cancel")}</button>
          <button class="btn-primary" onclick={send} disabled={processing || !src} title={$t("Ctrl+Enter")}>
            {#if processing}
              <span class="spin"></span>{$t("Processing…")}
            {:else}
              {@render icon("send", 15)}{$t("Send")}
            {/if}
          </button>
        </div>
      </footer>
    </div>
  </div>
{/if}

<style>
  .pe-backdrop {
    position: fixed; inset: var(--tb-h, 0px) 0 0 0;
    z-index: 210;
    background: rgba(6, 7, 12, 0.85);
    backdrop-filter: blur(10px);
    -webkit-backdrop-filter: blur(10px);
    display: flex;
    padding: 18px;
    animation: pe-fade 0.16s ease-out;
  }
  @keyframes pe-fade { from { opacity: 0; } to { opacity: 1; } }
  @keyframes pe-rise { from { opacity: 0; transform: translateY(6px); } to { opacity: 1; transform: none; } }

  .pe {
    flex: 1; min-width: 0; min-height: 0;
    display: flex; flex-direction: column;
    background: var(--bg-shell);
    border: 1px solid var(--border);
    border-radius: calc(18px * var(--rk, 1));
    overflow: hidden;
    box-shadow: 0 40px 120px -24px rgba(0, 0, 0, 0.75);
    animation: pe-rise 0.2s ease-out;
  }

  /* ── header ── */
  .pe-head {
    display: flex; align-items: center; gap: 12px;
    padding: 10px 12px 10px 18px;
    border-bottom: 1px solid var(--border);
  }
  .title { flex: 1; font-size: 14.5px; font-weight: 700; letter-spacing: 0.01em; color: var(--fg); }
  .hist { display: flex; align-items: center; gap: 2px; }
  .ib {
    height: 32px; min-width: 32px; padding: 0 8px;
    display: inline-flex; align-items: center; justify-content: center; gap: 6px;
    background: transparent; border: 1px solid transparent; border-radius: calc(9px * var(--rk, 1));
    color: var(--fg-muted); font: inherit; font-size: 12.5px; font-weight: 600;
    cursor: pointer;
    transition: background 0.12s, color 0.12s;
  }
  .ib:hover:not(:disabled) { background: var(--bg-hover); color: var(--fg); }
  .ib.close:hover:not(:disabled) { color: var(--danger); }
  .vsep { width: 1px; height: 22px; background: var(--border); margin: 0 6px; flex: none; }

  /* ── stage ── */
  .stage {
    flex: 1; min-height: 0; position: relative;
    display: grid; place-items: center;
    overflow: hidden;
    background:
      radial-gradient(ellipse at 50% 40%, rgba(255, 255, 255, 0.03), transparent 65%),
      #05060b;
  }
  .loading { color: var(--fg-dim); font-size: 13px; }
  .frame {
    position: relative;
    overflow: hidden;
    touch-action: none;
    user-select: none;
    -webkit-user-select: none;
    background-color: #1b1d26;
    background-image: conic-gradient(#272a36 25%, #1b1d26 0 50%, #272a36 0 75%, #1b1d26 0);
    background-size: 16px 16px;
    box-shadow: 0 0 0 1px rgba(255, 255, 255, 0.06), 0 28px 70px -16px rgba(0, 0, 0, 0.85);
  }
  .frame.t-draw { cursor: none; }
  .frame.t-text { cursor: text; }
  .layer { position: absolute; inset: 0; width: 100%; height: 100%; }
  canvas.layer { display: block; }
  .passive, .tint { pointer-events: none; }
  .tint { mix-blend-mode: soft-light; }

  /* ── text boxes (keep in sync with drawTexts) ── */
  .texts { pointer-events: none; font-family: var(--font-ui, system-ui, sans-serif); }
  .txt {
    position: absolute;
    transform: translate(-50%, -50%);
    white-space: pre;
    line-height: 1.2;
    font-weight: 700;
    text-align: center;
    pointer-events: auto;
    cursor: move;
    outline: none;
    min-width: 0.5em;
    min-height: 1.2em;
    caret-color: currentColor;
  }
  .texts.inert .txt { pointer-events: none; }
  .txt.s-plain { color: var(--c); text-shadow: 0 0.04em 0.18em rgba(0, 0, 0, 0.5); }
  .txt.s-pill { color: var(--k); background: var(--c); padding: 0.3em 0.55em; border-radius: 0.45em; }
  .txt.s-outline {
    color: var(--c);
    -webkit-text-stroke: 0.2em var(--k);
    paint-order: stroke fill;
  }
  .txt.sel { box-shadow: 0 0 0 1px var(--accent), 0 0 14px -2px var(--accent-glow); border-radius: 0.3em; }
  .txt.s-pill.sel { border-radius: 0.45em; }
  .txt.editing { cursor: text; box-shadow: 0 0 0 1.5px var(--accent), 0 0 18px -2px var(--accent-glow); }

  /* ── crop ── */
  .crop-box {
    position: absolute;
    cursor: move;
    box-shadow: 0 0 0 9999px rgba(4, 5, 10, 0.62);
    outline: 1px solid rgba(255, 255, 255, 0.8);
  }
  .g { position: absolute; background: rgba(255, 255, 255, 0.28); pointer-events: none; }
  .gv { top: 0; bottom: 0; width: 1px; }
  .gh { left: 0; right: 0; height: 1px; }
  .hd { position: absolute; width: 22px; height: 22px; }
  .hd::after { content: ""; position: absolute; background: #fff; border-radius: 2px; box-shadow: 0 0 8px var(--accent-glow); }
  .hd-n, .hd-s { left: calc(50% - 11px); cursor: ns-resize; }
  .hd-e, .hd-w { top: calc(50% - 11px); cursor: ew-resize; }
  .hd-n { top: -11px; } .hd-s { bottom: -11px; }
  .hd-w { left: -11px; } .hd-e { right: -11px; }
  .hd-n::after, .hd-s::after { left: 3px; right: 3px; top: 9px; height: 4px; }
  .hd-e::after, .hd-w::after { top: 3px; bottom: 3px; left: 9px; width: 4px; }
  .hd.corner::after { display: none; }
  .hd.corner { border: 0 solid var(--accent); filter: drop-shadow(0 0 4px var(--accent-glow)); }
  .hd-nw { left: -4px; top: -4px; border-left-width: 3px !important; border-top-width: 3px !important; cursor: nwse-resize; }
  .hd-ne { right: -4px; top: -4px; border-right-width: 3px !important; border-top-width: 3px !important; cursor: nesw-resize; }
  .hd-sw { left: -4px; bottom: -4px; border-left-width: 3px !important; border-bottom-width: 3px !important; cursor: nesw-resize; }
  .hd-se { right: -4px; bottom: -4px; border-right-width: 3px !important; border-bottom-width: 3px !important; cursor: nwse-resize; }

  .brush-ring {
    position: absolute;
    min-width: 4px; min-height: 4px;
    transform: translate(-50%, -50%);
    border-radius: 50%;
    border: 1.5px solid var(--bc);
    box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.55), 0 0 10px -1px var(--bc);
    pointer-events: none;
  }
  .brush-ring.erase { border-style: dashed; box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.55); }

  /* ── tool panel ── */
  .panel {
    display: flex; align-items: center; flex-wrap: wrap; gap: 12px 16px;
    padding: 12px 16px;
    min-height: 62px;
    border-top: 1px solid var(--border);
    background: var(--bg-surface);
    animation: pe-rise 0.16s ease-out;
  }
  .grow { flex: 1; }
  .hint { font-size: 12px; color: var(--fg-dim); }
  .lbl {
    font-size: 11px; font-weight: 700; letter-spacing: 0.06em; text-transform: uppercase;
    color: var(--fg-dim);
  }
  input[type="range"] { accent-color: var(--accent); cursor: pointer; }

  .chips { display: flex; gap: 6px; flex-wrap: wrap; }
  .chip, .seg button {
    height: 30px; padding: 0 12px;
    display: inline-flex; align-items: center; gap: 6px;
    border: 1px solid var(--border);
    background: var(--bg-input);
    color: var(--fg-muted);
    border-radius: 999px;
    font: inherit; font-size: 12px; font-weight: 600;
    cursor: pointer;
    transition: color 0.12s, background 0.12s, border-color 0.12s;
  }
  .chip:hover, .seg button:hover { color: var(--fg); border-color: var(--border-strong); }
  .chip.on, .seg button.on {
    color: var(--accent);
    background: var(--accent-soft);
    border-color: var(--accent-soft);
    border-color: color-mix(in srgb, var(--accent) 40%, transparent);
    box-shadow: 0 0 14px -6px var(--accent-glow);
  }
  .seg { display: inline-flex; padding: 2px; gap: 2px; border: 1px solid var(--border); border-radius: 999px; background: var(--bg-input); }
  .seg button { border-color: transparent; background: transparent; height: 26px; }

  .swatches { display: flex; gap: 7px; flex-wrap: wrap; }
  .sw {
    width: 22px; height: 22px; padding: 0;
    border-radius: 50%;
    background: var(--sw);
    border: 2px solid var(--bg-surface);
    box-shadow: 0 0 0 1px var(--border-strong);
    cursor: pointer;
    transition: transform 0.12s, box-shadow 0.12s;
  }
  .sw:hover { transform: scale(1.1); }
  .sw.on { box-shadow: 0 0 0 2px var(--accent), 0 0 12px -1px var(--sw); transform: scale(1.08); }

  .size { display: inline-flex; align-items: center; gap: 10px; }
  .size input { width: 130px; }
  .dot {
    width: 26px; height: 26px; display: grid; place-items: center; flex: none;
  }
  .dot::after {
    content: ""; width: var(--d); height: var(--d); border-radius: 50%;
    background: var(--bc); box-shadow: 0 0 10px -2px var(--bc);
  }

  .adj {
    flex: 1;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(170px, 1fr));
    gap: 10px 22px;
  }
  .slider { display: flex; flex-direction: column; gap: 4px; }
  .slider .row { display: flex; justify-content: space-between; align-items: baseline; }
  .slider input { width: 100%; }
  .val { font-size: 11.5px; color: var(--fg-dim); font-variant-numeric: tabular-nums; }
  .val.changed { color: var(--accent); }

  .filters { display: flex; gap: 10px; overflow-x: auto; padding: 2px; max-width: 100%; }
  .fl {
    display: flex; flex-direction: column; align-items: center; gap: 6px;
    padding: 0; background: none; border: none;
    color: var(--fg-muted); font: inherit; font-size: 11.5px; font-weight: 600;
    cursor: pointer;
  }
  .th {
    position: relative; display: block;
    width: 62px; height: 62px;
    border-radius: calc(12px * var(--rk, 1)); overflow: hidden;
    border: 2px solid var(--border);
    background: var(--bg-input);
    transition: border-color 0.12s, box-shadow 0.12s;
  }
  .th img { width: 100%; height: 100%; object-fit: cover; display: block; }
  .th-tint { position: absolute; inset: 0; mix-blend-mode: soft-light; }
  .fl:hover .th { border-color: var(--border-strong); }
  .fl.on { color: var(--accent); }
  .fl.on .th { border-color: var(--accent); box-shadow: 0 0 16px -4px var(--accent-glow); }

  /* ── bottom bar ── */
  .pe-bar {
    display: flex; align-items: center; gap: 12px;
    padding: 10px 14px;
    border-top: 1px solid var(--border);
    background: var(--bg-shell);
  }
  .tools { flex: 1; min-width: 0; display: flex; align-items: center; justify-content: center; flex-wrap: wrap; gap: 4px; }
  .tb {
    height: 38px; padding: 0 12px;
    display: inline-flex; align-items: center; gap: 7px;
    background: transparent; border: 1px solid transparent; border-radius: calc(11px * var(--rk, 1));
    color: var(--fg-muted); font: inherit; font-size: 12.5px; font-weight: 600;
    cursor: pointer;
    transition: color 0.12s, background 0.12s, box-shadow 0.12s;
  }
  .tb.icon { padding: 0; width: 38px; justify-content: center; }
  .tb:hover:not(:disabled) { color: var(--fg); background: var(--bg-hover); }
  .tb.on, .tb.on:hover:not(:disabled) {
    color: var(--accent);
    background: var(--accent-soft);
    border-color: var(--accent-soft);
    border-color: color-mix(in srgb, var(--accent) 35%, transparent);
    box-shadow: 0 0 18px -6px var(--accent-glow);
  }
  .actions { display: flex; gap: 8px; flex: none; }

  .btn-ghost, .btn-primary {
    height: 38px; padding: 0 18px;
    display: inline-flex; align-items: center; justify-content: center; gap: 7px;
    border-radius: calc(11px * var(--rk, 1));
    font: inherit; font-size: 13px; font-weight: 700;
    cursor: pointer;
    transition: transform 0.12s, color 0.12s, border-color 0.12s, box-shadow 0.12s;
  }
  .btn-ghost.sm, .btn-primary.sm { height: 32px; padding: 0 14px; font-size: 12.5px; border-radius: calc(9px * var(--rk, 1)); }
  .btn-ghost { background: transparent; border: 1px solid var(--border); color: var(--fg-muted); }
  .btn-ghost:hover:not(:disabled) { color: var(--fg); border-color: var(--border-strong); }
  .btn-ghost.danger:hover:not(:disabled) { color: var(--danger); border-color: var(--danger); }
  .btn-primary {
    border: none;
    background: linear-gradient(135deg, var(--accent), var(--accent-2));
    color: #06070c;
    box-shadow: 0 6px 18px -4px var(--accent-glow);
  }
  .btn-primary:hover:not(:disabled) { transform: translateY(-1px); box-shadow: 0 8px 24px -4px var(--accent-glow); }
  button:disabled { opacity: 0.45; cursor: not-allowed; }
  .spin {
    width: 13px; height: 13px; border-radius: 50%;
    border: 2px solid rgba(6, 7, 12, 0.3); border-top-color: #06070c;
    animation: pe-spin 0.7s linear infinite;
  }
  @keyframes pe-spin { to { transform: rotate(360deg); } }

  @media (max-width: 760px) {
    .pe-backdrop { padding: 0; }
    .pe { border-radius: 0; border: none; }
    .tb span { display: none; }
    .tb { padding: 0; width: 38px; justify-content: center; }
    .pe-bar { flex-wrap: wrap; }
    .actions { width: 100%; justify-content: flex-end; }
    .hist .ib span { display: none; }
  }
</style>
