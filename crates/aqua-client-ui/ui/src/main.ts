import App from "./App.svelte";
import { mount } from "svelte";
import "./app.css";

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
    setInterval(() => {
      const now = performance.now();
      const gap = now - last - 250;
      last = now;
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
