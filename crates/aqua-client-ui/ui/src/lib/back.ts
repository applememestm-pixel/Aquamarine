/**
 * The phone's Back button / back gesture, one place for all of it.
 *
 * Whatever is open on top — a dialog, a sheet, the emoji panel, the message
 * menu or a selection, the open chat, a tab other than Chats — registers
 * how to close itself with `onBack`. While anything is registered, one
 * history entry ("guard") sits on top; Back pops it, the newest thing
 * closes, and the guard goes back while something is still open. With
 * nothing open there is no guard, and Back leaves the app as usual.
 *
 * Before, only the open chat had a history entry: Back with a sheet or a
 * menu open, or on the Files / Contacts tab, closed the whole app.
 *
 * Settings keeps its own entries (section → list → closed); while it's
 * open its pops are left to it.
 */
type Closer = { id: number; close: () => void };
const stack: Closer[] = [];
let guard = false;
let ignore = 0;
let seq = 0;

function ensureGuard() {
  if (guard) return;
  try { history.pushState({ aquaBack: true }, ""); guard = true; } catch { /* ignore */ }
}

/** Register a way to close something; returns `release` for when it closes
 *  by other means (a tap on ×, a swipe…). Svelte: `$effect(() => { if (open) return onBack(close); })`. */
export function onBack(close: () => void): () => void {
  const c = { id: ++seq, close };
  stack.push(c);
  ensureGuard();
  return () => {
    const i = stack.indexOf(c);
    if (i < 0) return; // already closed by Back
    stack.splice(i, 1);
    if (!stack.length && guard && !settingsOpen()) {
      guard = false;
      ignore++;
      try { history.back(); } catch { ignore--; }
    }
  };
}

const settingsOpen = () => document.body.classList.contains("settings-open") || !!history.state?.aquaSettings;

/** Close the newest open thing, as Back does (Esc on a phone keyboard, a swipe…). */
export function backOnce(): boolean {
  const top = stack.pop();
  if (!top) return false;
  try { top.close(); } catch (e) { console.warn("[back]", e); }
  return true;
}

export function startBack(): () => void {
  const onPop = () => {
    if (ignore) { ignore--; return; }
    // Settings handles its own entries.
    if (document.body.classList.contains("settings-open")) return;
    // Landed on our own guard: an entry above it (Settings) was popped, not ours.
    if (history.state?.aquaBack) { guard = true; return; }
    guard = false;
    backOnce();
    if (stack.length) ensureGuard();
  };
  window.addEventListener("popstate", onPop);

  // Any modal dialog counts as "open on top": Back sends it the Esc it
  // already understands.
  const dialogs = new Map<Element, () => void>();
  const scan = () => {
    for (const el of document.querySelectorAll('[role="dialog"][aria-modal="true"]')) {
      if (dialogs.has(el) || el.closest("[data-own-back]")) continue;
      dialogs.set(el, onBack(() => {
        el.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
      }));
    }
    for (const [el, release] of dialogs) {
      if (!el.isConnected) { dialogs.delete(el); release(); }
    }
  };
  let queued = false;
  const mo = new MutationObserver(() => {
    if (queued) return;
    queued = true;
    queueMicrotask(() => { queued = false; scan(); });
  });
  mo.observe(document.body, { childList: true, subtree: true });
  scan();

  return () => { window.removeEventListener("popstate", onPop); mo.disconnect(); };
}
