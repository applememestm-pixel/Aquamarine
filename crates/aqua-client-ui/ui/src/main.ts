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
    // Is WebKit drawing with the GPU? A software renderer (llvmpipe) means
    // the graphics driver failed to load — after an NVIDIA update without a
    // reboot, typically — and every repaint costs CPU.
    try {
      const gl = document.createElement("canvas").getContext("webgl") as WebGLRenderingContext | null;
      const ext = gl?.getExtension("WEBGL_debug_renderer_info");
      const r = gl ? String(gl.getParameter(ext ? ext.UNMASKED_RENDERER_WEBGL : gl.RENDERER)) : "none";
      const soft = !gl || /llvmpipe|softpipe|swrast|software/i.test(r);
      console.warn(`[aqua-gpu] renderer: ${r}${soft ? " (SOFTWARE — no GPU acceleration)" : ""}`);
      if (soft) {
        document.documentElement.classList.add("soft-render");
        (window as unknown as { __aquaSoftRender?: string }).__aquaSoftRender = r;
      }
    } catch { /* ignore */ }
    for (const ev of ["error", "stalled", "waiting"] as const) {
      addEventListener(ev, (e) => {
        const m = e.target as HTMLMediaElement;
        if (m && (m.tagName === "AUDIO" || m.tagName === "VIDEO")) console.warn(`[aqua-media] ${ev} ${m.tagName} err=${m.error?.code ?? "-"} ${m.error?.message ?? ""} src=${(m.currentSrc || "").slice(0, 60)}`);
      }, true);
    }
  }
}

const target = document.getElementById("app");
if (!target) throw new Error("#app root not found");

const app = mount(App, { target });
export default app;
