/**
 * Middle-button autoscroll (Windows-style), for every scrollable pane:
 * press the wheel over a chat or the chat list, move the mouse away from the
 * anchor and the pane scrolls — faster the further you go. Hold-and-drag
 * ends on release; a click toggles it on until the next click or Esc.
 * WebKitGTK has none of its own, and WebView2's was blocked by us.
 */
function scrollableAt(el: Element | null): HTMLElement | null {
  for (let n = el as HTMLElement | null; n && n !== document.body; n = n.parentElement) {
    const cs = getComputedStyle(n);
    if (/(auto|scroll)/.test(cs.overflowY) && n.scrollHeight > n.clientHeight + 2) return n;
  }
  return null;
}

export function startAutoscroll(): () => void {
  let target: HTMLElement | null = null;
  let ax = 0, ay = 0, mx = 0, my = 0, t0 = 0, raf = 0, moved = false;
  let marker: HTMLDivElement | null = null;
  let lastDir = "";

  const stop = () => {
    target = null;
    cancelAnimationFrame(raf);
    marker?.remove(); marker = null;
    document.documentElement.classList.remove("autoscrolling", "as-up", "as-down");
    lastDir = "";
  };
  const frame = () => {
    if (!target) return;
    const dy = my - ay, dx = mx - ax;
    const f = (d: number) => (Math.abs(d) < 12 ? 0 : Math.sign(d) * Math.pow(Math.abs(d) - 12, 1.25) / 6);
    target.scrollTop += f(dy);
    if (target.scrollWidth > target.clientWidth) target.scrollLeft += f(dx);
    const dir = Math.abs(dy) < 12 ? "" : dy > 0 ? "down" : "up";
    if (dir !== lastDir) {
      lastDir = dir;
      const r = document.documentElement.classList;
      r.toggle("as-up", dir === "up");
      r.toggle("as-down", dir === "down");
    }
    raf = requestAnimationFrame(frame);
  };
  const down = (e: MouseEvent) => {
    if (target) { e.preventDefault(); stop(); return; }
    if (e.button !== 1) return;
    const t = e.target as HTMLElement;
    if (t.closest("input, textarea, [contenteditable], a, video, audio")) return;
    const sc = scrollableAt(t);
    if (!sc) return;
    e.preventDefault();
    target = sc; ax = mx = e.clientX; ay = my = e.clientY; t0 = performance.now(); moved = false;
    marker = document.createElement("div");
    marker.className = "aq-autoscroll";
    marker.style.left = `${ax}px`; marker.style.top = `${ay}px`;
    document.body.appendChild(marker);
    document.documentElement.classList.add("autoscrolling");
    raf = requestAnimationFrame(frame);
  };
  const move = (e: MouseEvent) => {
    mx = e.clientX; my = e.clientY;
    if (target && Math.hypot(mx - ax, my - ay) > 14) moved = true;
  };
  const up = (e: MouseEvent) => {
    // Held and dragged → release ends it; a quick click keeps it on.
    if (target && e.button === 1 && (moved || performance.now() - t0 > 450)) stop();
  };
  const key = (e: KeyboardEvent) => { if (target && e.key === "Escape") { e.preventDefault(); stop(); } };
  const wheel = () => { if (target) stop(); };
  const aux = (e: MouseEvent) => { if (e.button === 1) e.preventDefault(); };

  document.addEventListener("mousedown", down, true);
  document.addEventListener("mousemove", move, true);
  document.addEventListener("mouseup", up, true);
  document.addEventListener("keydown", key, true);
  document.addEventListener("wheel", wheel, { passive: true, capture: true });
  document.addEventListener("auxclick", aux);
  return () => {
    stop();
    document.removeEventListener("mousedown", down, true);
    document.removeEventListener("mousemove", move, true);
    document.removeEventListener("mouseup", up, true);
    document.removeEventListener("keydown", key, true);
    document.removeEventListener("wheel", wheel, true);
    document.removeEventListener("auxclick", aux);
  };
}
