/**
 * Side panels (shared media, thread, calendar…) get a grip on their left
 * edge: drag it to change the width. Remembered per panel.
 *   <aside use:sideResize={"media"}>
 */
export function sideResize(node: HTMLElement, key: string) {
  // Phones show these panels full-width: nothing to drag there.
  if (document.documentElement.classList.contains("phone")) return {};
  const store = `aqua.side.${key}`;
  const clamp = (w: number) => Math.round(Math.max(240, Math.min(w, window.innerWidth * 0.6)));
  try { const w = Number(localStorage.getItem(store)); if (w > 0) node.style.width = clamp(w) + "px"; } catch { /* ignore */ }
  if (getComputedStyle(node).position === "static") node.style.position = "relative";
  const grip = document.createElement("div");
  grip.className = "side-grip";
  grip.title = "↔";
  Object.assign(grip.style, { position: "absolute", left: "-3px", top: "0", width: "6px", height: "100%", cursor: "col-resize", zIndex: "5" });
  node.appendChild(grip);
  grip.addEventListener("pointerdown", (e) => {
    e.preventDefault();
    const x0 = e.clientX, w0 = node.getBoundingClientRect().width;
    grip.setPointerCapture(e.pointerId);
    document.documentElement.style.cursor = "col-resize";
    const move = (ev: PointerEvent) => { node.style.width = clamp(w0 + (x0 - ev.clientX)) + "px"; };
    const up = () => {
      grip.removeEventListener("pointermove", move);
      document.documentElement.style.cursor = "";
      try { localStorage.setItem(store, String(Math.round(node.getBoundingClientRect().width))); } catch { /* ignore */ }
    };
    grip.addEventListener("pointermove", move);
    grip.addEventListener("pointerup", up, { once: true });
    grip.addEventListener("pointercancel", up, { once: true });
  });
  grip.addEventListener("dblclick", () => { node.style.width = ""; try { localStorage.removeItem(store); } catch { /* ignore */ } });
  return { destroy: () => grip.remove() };
}
