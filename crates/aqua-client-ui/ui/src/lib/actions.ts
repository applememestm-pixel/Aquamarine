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
    if (r.height > vh - 2 * margin) { node.style.maxHeight = `${vh - 2 * margin}px`; node.style.overflowY = "auto"; }
  };
  const ro = new ResizeObserver(() => fit());
  ro.observe(node);
  requestAnimationFrame(fit);
  window.addEventListener("resize", fit);
  return { destroy() { ro.disconnect(); window.removeEventListener("resize", fit); } };
}
