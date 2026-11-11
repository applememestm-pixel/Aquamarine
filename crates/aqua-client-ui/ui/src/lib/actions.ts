/**
 * `use:keepOnScreen` — for fixed-position popups (context menus, popovers):
 * after layout, and whenever the popup changes size (e.g. the reaction row
 * expands), nudge it back inside the window instead of letting it run off
 * the bottom / right edge.
 */
export function keepOnScreen(node: HTMLElement, margin = 8) {
  const fit = () => {
    const r = node.getBoundingClientRect();
    // The real size, not the rect: a menu still in its "pop" animation is
    // scaled down, and fitting that small rect left the grown menu hanging
    // off the bottom of the screen.
    const w = node.offsetWidth || r.width, h = node.offsetHeight || r.height;
    const x = r.left, y = r.top;
    // The visible part: above the on-screen keyboard, clear of the gesture bar.
    const vv = window.visualViewport;
    const vw = window.innerWidth;
    const vh = Math.min(window.innerHeight, vv ? vv.height + vv.offsetTop : Infinity);
    const bottomGap = margin + (matchMedia("(max-width: 720px)").matches ? 16 : 0);
    let left = x, top = y;
    if (x + w > vw - margin) left = Math.max(margin, vw - margin - w);
    if (y + h > vh - bottomGap) top = Math.max(margin, vh - bottomGap - h);
    if (left < margin) left = margin;
    if (top < margin) top = margin;
    if (left !== x) node.style.left = `${left}px`;
    if (top !== y) node.style.top = `${top}px`;
    // Taller than the window: let it scroll.
    const maxH = `${vh - margin - bottomGap}px`;
    if (h > vh - margin - bottomGap && node.style.maxHeight !== maxH) { node.style.maxHeight = maxH; node.style.overflowY = "auto"; }
  };
  // Fit on the next frame, not inside the observer callback: changing the
  // size from the callback re-triggers it ("ResizeObserver loop…").
  let raf = 0;
  const ro = new ResizeObserver(() => { cancelAnimationFrame(raf); raf = requestAnimationFrame(fit); });
  ro.observe(node);
  requestAnimationFrame(fit);
  window.addEventListener("resize", fit);
  return { destroy() { cancelAnimationFrame(raf); ro.disconnect(); window.removeEventListener("resize", fit); } };
}

/** A horizontal strip a mouse can scroll: the wheel moves it sideways, and
 *  it can be dragged (a click after a drag is swallowed). Touch scrolls it
 *  natively already. */
export function scrollX(node: HTMLElement) {
  const wheel = (e: WheelEvent) => {
    if (node.scrollWidth <= node.clientWidth || Math.abs(e.deltaX) > Math.abs(e.deltaY)) return;
    e.preventDefault();
    node.scrollBy({ left: e.deltaY, behavior: e.deltaMode === 0 && Math.abs(e.deltaY) < 60 ? "auto" : "smooth" });
  };
  let drag: { x: number; left: number; moved: boolean } | null = null;
  const down = (e: PointerEvent) => {
    if (e.pointerType !== "mouse" || e.button !== 0) return;
    drag = { x: e.clientX, left: node.scrollLeft, moved: false };
  };
  const move = (e: PointerEvent) => {
    if (!drag) return;
    const dx = e.clientX - drag.x;
    if (!drag.moved && Math.abs(dx) > 5) { drag.moved = true; try { node.setPointerCapture(e.pointerId); } catch { /* fine */ } }
    if (drag.moved) node.scrollLeft = drag.left - dx;
  };
  const up = () => {
    if (drag?.moved) {
      // The click that ends a drag doesn't open anything.
      const stop = (ev: Event) => { ev.stopPropagation(); ev.preventDefault(); };
      node.addEventListener("click", stop, { capture: true, once: true });
      setTimeout(() => node.removeEventListener("click", stop, { capture: true }), 0);
    }
    drag = null;
  };
  node.addEventListener("wheel", wheel, { passive: false });
  node.addEventListener("pointerdown", down);
  node.addEventListener("pointermove", move);
  node.addEventListener("pointerup", up);
  node.addEventListener("pointercancel", up);
  return {
    destroy() {
      node.removeEventListener("wheel", wheel);
      node.removeEventListener("pointerdown", down);
      node.removeEventListener("pointermove", move);
      node.removeEventListener("pointerup", up);
      node.removeEventListener("pointercancel", up);
    },
  };
}
