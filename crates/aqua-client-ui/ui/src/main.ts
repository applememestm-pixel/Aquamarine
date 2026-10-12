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
  }
}

const target = document.getElementById("app");
if (!target) throw new Error("#app root not found");

const app = mount(App, { target });
export default app;
