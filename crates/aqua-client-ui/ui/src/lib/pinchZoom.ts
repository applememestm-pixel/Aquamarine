/**
 * Two fingers on any picture in the app — a photo, a video, a sticker, a
 * GIF, an emoji — and it grows under them, like in Telegram: a copy floats
 * above everything, follows the fingers (zoom and move), and flies back into
 * place when they let go. The photo viewer has its own zoom (Lightbox).
 */
const MEDIA = "img, video, canvas, svg.qr";
const SKIP = ".lb, [data-no-pinch], input, textarea, [contenteditable], .avatar, .ava, [class*='avatar']";

interface Pinch {
  src: HTMLElement; clone: HTMLElement; veil: HTMLDivElement;
  ox: number; oy: number; d0: number; raf: number;
}

const dist = (t: TouchList) => Math.hypot(t[0].clientX - t[1].clientX, t[0].clientY - t[1].clientY);
const mid = (t: TouchList) => ({ x: (t[0].clientX + t[1].clientX) / 2, y: (t[0].clientY + t[1].clientY) / 2 });

function mediaAt(x: number, y: number): HTMLElement | null {
  for (const el of document.elementsFromPoint(x, y)) {
    if (!(el instanceof HTMLElement || el instanceof SVGElement)) continue;
    if (el.closest(SKIP)) return null;
    const m = (el as Element).closest(MEDIA) as HTMLElement | null;
    if (m) {
      const r = m.getBoundingClientRect();
      return r.width >= 28 && r.height >= 28 ? m : null;
    }
  }
  return null;
}

/** A copy of what's on screen now; videos and animations keep playing in it. */
function copyOf(el: HTMLElement): { node: HTMLElement; tick?: () => void } {
  if (el instanceof HTMLImageElement) {
    const i = document.createElement("img");
    i.src = el.currentSrc || el.src;
    i.style.objectFit = getComputedStyle(el).objectFit;
    return { node: i };
  }
  if (el instanceof HTMLVideoElement || el instanceof HTMLCanvasElement) {
    const c = document.createElement("canvas");
    const w = el instanceof HTMLVideoElement ? el.videoWidth || el.clientWidth : el.width;
    const h = el instanceof HTMLVideoElement ? el.videoHeight || el.clientHeight : el.height;
    c.width = Math.max(1, w); c.height = Math.max(1, h);
    const ctx = c.getContext("2d");
    const draw = () => { try { ctx?.clearRect(0, 0, c.width, c.height); ctx?.drawImage(el, 0, 0, c.width, c.height); } catch { /* tainted */ } };
    draw();
    return { node: c, tick: draw };
  }
  return { node: el.cloneNode(true) as HTMLElement };
}

export function startPinchZoom(): () => void {
  let p: Pinch | null = null;

  const begin = (e: TouchEvent) => {
    if (p || e.touches.length !== 2) return;
    const m = mid(e.touches);
    const src = mediaAt(e.touches[0].clientX, e.touches[0].clientY) ?? mediaAt(m.x, m.y);
    if (!src) return;
    const r = src.getBoundingClientRect();
    const { node, tick } = copyOf(src);
    const veil = document.createElement("div");
    veil.style.cssText = "position:fixed;inset:0;z-index:9998;pointer-events:none;background:rgba(0,0,0,0);transition:background .2s";
    node.style.cssText += `;position:fixed;left:${r.left}px;top:${r.top}px;width:${r.width}px;height:${r.height}px;margin:0;z-index:9999;`
      + `pointer-events:none;transform-origin:${m.x - r.left}px ${m.y - r.top}px;will-change:transform;border-radius:${getComputedStyle(src).borderRadius}`;
    document.body.append(veil, node);
    const prev = src.style.visibility;
    src.style.visibility = "hidden";
    src.dataset.pinchVis = prev;
    p = { src, clone: node, veil, ox: m.x, oy: m.y, d0: Math.max(1, dist(e.touches)), raf: 0 };
    document.documentElement.classList.add("pinching");
    if (tick) { const loop = () => { tick(); if (p) p.raf = requestAnimationFrame(loop); }; p.raf = requestAnimationFrame(loop); }
    e.preventDefault();
  };

  const move = (e: TouchEvent) => {
    if (!p) return;
    e.preventDefault();
    if (e.touches.length < 2) return;
    const m = mid(e.touches);
    const s = Math.max(0.8, Math.min(6, dist(e.touches) / p.d0));
    p.clone.style.transform = `translate(${m.x - p.ox}px, ${m.y - p.oy}px) scale(${s})`;
    p.veil.style.background = `rgba(0,0,0,${Math.min(0.6, Math.max(0, (s - 1) * 0.5))})`;
  };

  const end = (e: TouchEvent) => {
    if (!p || e.touches.length >= 2) return;
    const cur = p;
    p = null;
    cancelAnimationFrame(cur.raf);
    cur.clone.style.transition = "transform .24s cubic-bezier(.2,.9,.3,1)";
    cur.clone.style.transform = "none";
    cur.veil.style.background = "rgba(0,0,0,0)";
    setTimeout(() => {
      document.documentElement.classList.remove("pinching");
      cur.src.style.visibility = cur.src.dataset.pinchVis ?? "";
      delete cur.src.dataset.pinchVis;
      cur.clone.remove(); cur.veil.remove();
    }, 250);
  };

  document.addEventListener("touchstart", begin, { passive: false, capture: true });
  document.addEventListener("touchmove", move, { passive: false, capture: true });
  document.addEventListener("touchend", end, { capture: true });
  document.addEventListener("touchcancel", end, { capture: true });
  return () => {
    document.removeEventListener("touchstart", begin, { capture: true });
    document.removeEventListener("touchmove", move, { capture: true });
    document.removeEventListener("touchend", end, { capture: true });
    document.removeEventListener("touchcancel", end, { capture: true });
  };
}
