/**
 * Android: room for the status bar, the navigation bar and the cutout.
 *
 * The app draws edge-to-edge. `env(safe-area-inset-*)` alone went wrong
 * there: at an interface scale other than 100 % (theme.ts zooms through
 * the viewport tag) and with the 3-button navigation bar the bottom bar and
 * the composer rode high, with a gap under them, and sheets opened off
 * centre. MainActivity hands over the bars and the WebView's size in device
 * pixels; from the CSS pixels the page really has, this works out how much
 * of each bar lies over the page and puts it in `--sat` / `--sab` / `--sal`
 * / `--sar` (CSS uses `var(--sab, env(safe-area-inset-bottom))`). A bar the
 * WebView already keeps the page out of counts as 0 — no double padding.
 */
interface Raw { l: number; t: number; r: number; b: number; w: number; h: number }

declare global {
  interface Window { __aquaInsets?: (o: Raw) => void; AquaInsets?: { insets(): string } }
}

export function startInsets() {
  if (typeof window === "undefined" || !/android/i.test(navigator.userAgent)) return;
  const root = document.documentElement;
  let last: Raw | null = null;
  const read = (): Raw | null => {
    try { const s = window.AquaInsets?.insets(); return s ? (JSON.parse(s) as Raw) : null; } catch { return null; }
  };
  const px = (n: number) => `${Math.max(0, Math.round(n * 2) / 2)}px`;
  const apply = () => {
    const n = last ?? read();
    if (!n || !(n.w > 0) || !(n.h > 0) || !(innerWidth > 0)) return;
    last = n;
    // Device pixels per CSS pixel of the page (the interface scale included).
    const k = n.w / innerWidth;
    const bottomEdge = innerHeight * k; // where the page ends, in device px
    root.style.setProperty("--sat", px(n.t / k));
    root.style.setProperty("--sab", px((bottomEdge - (n.h - n.b)) / k));
    root.style.setProperty("--sal", px(n.l / k));
    root.style.setProperty("--sar", px(n.r / k));
  };
  window.__aquaInsets = (o) => { last = o; apply(); };
  window.addEventListener("resize", apply);
  window.visualViewport?.addEventListener("resize", apply);
  window.addEventListener("orientationchange", () => setTimeout(apply, 300));
  apply();
  // The bridge may answer a moment after the page starts.
  setTimeout(apply, 400);
  setTimeout(apply, 1500);
}
