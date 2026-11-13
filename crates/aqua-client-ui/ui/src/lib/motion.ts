/**
 * Svelte transitions run in JS, past the CSS switches in app.css (reduced
 * motion, power saving, Linux's software rendering): the same rules here.
 * Checked at the moment a transition starts, so a switch applies at once.
 */
export function calm(): boolean {
  if (typeof document === "undefined") return true;
  const c = document.documentElement.classList;
  return matchMedia("(prefers-reduced-motion: reduce)").matches
    || c.contains("soft-render") || c.contains("wk-lowfx") || c.contains("ps-ui") || c.contains("ps-chat");
}

/** `n` ms, or 0 when things shouldn't move. */
export const motionMs = (n: number) => (calm() ? 0 : n);
