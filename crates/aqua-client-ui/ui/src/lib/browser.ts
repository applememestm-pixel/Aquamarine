/**
 * The built-in browser (computers): web links open in a window of the app
 * (windows.rs::open_browser_window — a plain page with no access to the
 * app). Off: the system browser. Phones always use the system browser.
 */
import { writable } from "svelte/store";
import { IS_DESKTOP } from "./platform";

const KEY = "aqua.inAppBrowser";
let first = IS_DESKTOP;
try { const v = localStorage.getItem(KEY); if (v != null) first = v === "1"; } catch { /* storage blocked */ }
export const inAppBrowser = writable<boolean>(first);
inAppBrowser.subscribe((v) => { try { localStorage.setItem(KEY, v ? "1" : "0"); } catch { /* ignore */ } });
