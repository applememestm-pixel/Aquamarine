/**
 * Phones: keep the composer above the on-screen keyboard.
 *
 * The Android WebView (edge-to-edge) doesn't shrink the page when the
 * keyboard comes up — only the visual viewport shrinks, and the browser
 * scrolls just the caret into view, so the message field was half under
 * the keyboard. Here the keyboard's height goes into `--kb` and
 * `html.kb-open` is set; the open chat leaves that much room at the bottom
 * (app.css), and the page is scrolled back so nothing drifts. Where the
 * system does shrink the page instead, only `kb-open` is set (the bottom
 * safe-area padding then isn't needed above the keyboard).
 */
export function startKeyboardInset(): () => void {
  const vv = window.visualViewport;
  const root = document.documentElement;
  let tall = window.innerHeight, width = window.innerWidth;
  let raf = 0;
  const apply = () => {
    raf = 0;
    if (window.innerWidth !== width) { width = window.innerWidth; tall = window.innerHeight; }
    tall = Math.max(tall, window.innerHeight);
    const pinch = vv && Math.abs(vv.scale - 1) > 0.01;
    const kb = vv && !pinch ? Math.max(0, Math.round(window.innerHeight - vv.height)) : 0;
    const shrunk = tall - window.innerHeight;
    const overlay = kb > 120;
    const open = overlay || shrunk > 120;
    root.style.setProperty("--kb", overlay ? `${kb}px` : "0px");
    root.classList.toggle("kb-open", open);
    // Undo the browser's own "scroll the caret into view": the room is ours now.
    if (overlay && (window.scrollY || document.scrollingElement?.scrollTop)) window.scrollTo(0, 0);
    // A list that was at the bottom stays at the bottom.
    for (const el of document.querySelectorAll<HTMLElement>(".messages")) {
      if (el.dataset.atEnd === "1") el.scrollTop = el.scrollHeight;
    }
  };
  const schedule = () => {
    // Remember who was at the bottom before the size changes.
    for (const el of document.querySelectorAll<HTMLElement>(".messages")) {
      el.dataset.atEnd = el.scrollHeight - el.scrollTop - el.clientHeight < 48 ? "1" : "0";
    }
    if (!raf) raf = requestAnimationFrame(apply);
  };
  vv?.addEventListener("resize", schedule);
  vv?.addEventListener("scroll", schedule);
  window.addEventListener("resize", schedule);
  apply();
  return () => {
    vv?.removeEventListener("resize", schedule);
    vv?.removeEventListener("scroll", schedule);
    window.removeEventListener("resize", schedule);
    cancelAnimationFrame(raf);
  };
}
