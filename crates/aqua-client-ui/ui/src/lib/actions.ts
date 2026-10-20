/**
 * `use:keepOnScreen` — for fixed-position popups (context menus, popovers):
 * after layout, and whenever the popup changes size (e.g. the reaction row
 * expands), nudge it back inside the window instead of letting it run off
 * the bottom / right edge.
 */
export function keepOnScreen(node: HTMLElement, margin = 8) {
  const fit = () => {
    const r = node.getBoundingClientRect();
    const vw = window.innerWidth, vh = window.innerHeight;
    let left = r.left, top = r.top;
    if (r.right > vw - margin) left = Math.max(margin, vw - margin - r.width);
    if (r.bottom > vh - margin) top = Math.max(margin, vh - margin - r.height);
    if (left !== r.left) node.style.left = `${left}px`;
    if (top !== r.top) node.style.top = `${top}px`;
    // Taller than the window: let it scroll.
    const maxH = `${vh - 2 * margin}px`;
    if (r.height > vh - 2 * margin && node.style.maxHeight !== maxH) { node.style.maxHeight = maxH; node.style.overflowY = "auto"; }
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
