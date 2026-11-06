import App from "./App.svelte";
import { mount } from "svelte";
import "./app.css";

// Phones and tablets: no desktop title bar even when the screen is turned
// sideways and wider than the phone layout's breakpoint.
if (/android|iphone|ipad/i.test(navigator.userAgent)) document.documentElement.classList.add("phone");
// Android: windows close with Back, no × on them (app.css, lib/back.ts).
if (/android/i.test(navigator.userAgent)) document.documentElement.classList.add("android");

// Middle button: fast auto-scroll of our own. The browser's version left its
// round marker painted on the screen; this one has no marker, only the
// cursor shows it. Hold and move = scroll while held; a short click = keeps
// scrolling with the mouse until the next click (like Chrome). The speed
// grows with the distance from where it started.
{
  let box: HTMLElement | null = null, ox = 0, oy = 0, mx = 0, my = 0, at = 0, moved = false, raf = 0, last = 0;
  const scrollable = (el: Element | null): HTMLElement | null => {
    for (let n = el as HTMLElement | null; n && n !== document.body; n = n.parentElement) {
      const cs = getComputedStyle(n);
      if (/(auto|scroll)/.test(cs.overflowY + cs.overflowX) && (n.scrollHeight > n.clientHeight + 1 || n.scrollWidth > n.clientWidth + 1)) return n;
    }
    return null;
  };
  const speed = (d: number) => { const a = Math.max(0, Math.abs(d) - 12); return Math.sign(d) * Math.pow(a, 1.35) / 10; };
  const stop = () => {
    box = null; cancelAnimationFrame(raf);
    document.documentElement.classList.remove("aq-autoscroll");
    removeEventListener("mousemove", move, true);
  };
  const tickScroll = (t: number) => {
    if (!box) return;
    const dt = last ? Math.min(48, t - last) / 16.7 : 1; last = t;
    box.scrollBy(speed(mx - ox) * dt, speed(my - oy) * dt);
    raf = requestAnimationFrame(tickScroll);
  };
  const move = (e: MouseEvent) => { mx = e.clientX; my = e.clientY; if (Math.hypot(mx - ox, my - oy) > 12) moved = true; };
  addEventListener("mousedown", (e) => {
    if (box) { e.preventDefault(); e.stopPropagation(); stop(); return; }
    if (e.button !== 1) return;
    e.preventDefault(); // no browser auto-scroll, no marker
    if ((e.target as Element)?.closest?.("a[href]")) return; // middle-click on a link stays a link
    const b = scrollable(e.target as Element);
    if (!b) return;
    box = b; ox = mx = e.clientX; oy = my = e.clientY; at = performance.now(); moved = false; last = 0;
    document.documentElement.classList.add("aq-autoscroll");
    addEventListener("mousemove", move, true);
    raf = requestAnimationFrame(tickScroll);
  }, true);
  addEventListener("mouseup", (e) => {
    // Held and dragged: let go = stop. A short click keeps it on.
    if (box && e.button === 1 && (moved || performance.now() - at > 400)) stop();
  }, true);
  addEventListener("keydown", (e) => { if (box && e.key === "Escape") stop(); }, true);
  addEventListener("wheel", () => { if (box) stop(); }, { capture: true, passive: true });
  addEventListener("blur", () => { if (box) stop(); });
}

// Linux desktop = WebKitGTK, which paints CSS blur on the CPU every frame
// (no Chrome in the UA there; Android's WebView and Windows' WebView2 have it,
// macOS WKWebView isn't Linux). Lighter effects keep it smooth — see app.css.
{
  const ua = navigator.userAgent;
  if (/Linux/.test(ua) && !/Android|Chrome/.test(ua)) {
    document.documentElement.classList.add("wk-lowfx");
    // Diagnostics for the launcher log: report main-thread stalls together
    // with what was clicked last ("[aqua-stall] 2300 ms after click …").
    let last = performance.now();
    let lastClick = "";
    let lastClickAt = 0;
    addEventListener("pointerdown", (e) => {
      const t = e.target as Element | null;
      lastClick = t ? `${t.tagName.toLowerCase()}.${String((t as HTMLElement).className || "").split(" ")[0]}` : "?";
      lastClickAt = performance.now();
    }, true);
    // How long a click takes to show on screen ("[aqua-click] 180 ms on
    // button.chat-item"): the next painted frame after the click handlers.
    addEventListener("click", () => {
      const t0 = lastClickAt || performance.now();
      const what = lastClick;
      requestAnimationFrame(() => requestAnimationFrame(() => {
        const ms = performance.now() - t0;
        if (ms > 120) console.warn(`[aqua-click] ${Math.round(ms)} ms on ${what}`);
      }));
    }, true);
    setInterval(() => {
      const now = performance.now();
      const gap = now - last - 250;
      last = now;
      // Hidden / minimised windows get throttled timers: not a stall.
      if (document.hidden) return;
      if (gap > 400) console.warn(`[aqua-stall] ${Math.round(gap)} ms; last click ${lastClick} ${Math.round((now - lastClickAt) / 1000)} s ago`);
    }, 250);
    for (const ev of ["error", "stalled", "waiting"] as const) {
      addEventListener(ev, (e) => {
        const m = e.target as HTMLMediaElement;
        if (m && (m.tagName === "AUDIO" || m.tagName === "VIDEO")) console.warn(`[aqua-media] ${ev} ${m.tagName} err=${m.error?.code ?? "-"} ${m.error?.message ?? ""} src=${(m.currentSrc || "").slice(0, 60)}`);
      }, true);
    }
    // Smoothness report for the launcher log: frames per second during each
    // scroll gesture ("[aqua-fps] scroll 41 fps, worst frame 52 ms"), so a
    // "it lags" report comes with numbers. At most one line per 10 s.
    let active = false, frames = 0, t0 = 0, prev = 0, worst = 0, lastLog = 0;
    let idle: ReturnType<typeof setTimeout> | null = null;
    let what = "";
    const tick = () => {
      if (!active) return;
      const now = performance.now();
      worst = Math.max(worst, now - prev);
      prev = now;
      frames++;
      requestAnimationFrame(tick);
    };
    const end = () => {
      active = false;
      const dur = prev - t0;
      const now = performance.now();
      if (dur < 400 || frames < 5 || now - lastLog < 10_000) return;
      lastLog = now;
      console.warn(`[aqua-fps] scroll ${Math.round((frames * 1000) / dur)} fps, worst frame ${Math.round(worst)} ms over ${Math.round(dur)} ms in ${what}`);
    };
    addEventListener("scroll", (e) => {
      if (!active) {
        active = true; frames = 0; worst = 0; t0 = prev = performance.now();
        const el = e.target as HTMLElement | null;
        what = el && el.className ? `.${String(el.className).split(" ")[0]}` : "page";
        requestAnimationFrame(tick);
      }
      if (idle) clearTimeout(idle);
      idle = setTimeout(end, 250);
    }, { capture: true, passive: true });
    // The "ResizeObserver loop completed with undelivered notifications"
    // message is harmless noise from WebKit; keep it out of the log.
    addEventListener("error", (e) => {
      if (e.message?.includes("ResizeObserver loop")) { e.preventDefault(); e.stopImmediatePropagation(); }
    });
  }
}

const target = document.getElementById("app");
if (!target) throw new Error("#app root not found");

// A chat opened in its own window (Ctrl+click in the list): `#popout=<id>`.
const pop = /^#popout=(.+)$/.exec(location.hash);
if (pop) document.documentElement.classList.add("popout");
const app = pop
  ? mount((await import("./Popout.svelte")).default, { target, props: { chat: decodeURIComponent(pop[1]) } })
  : mount(App, { target });
export default app;
