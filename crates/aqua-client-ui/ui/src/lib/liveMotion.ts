/**
 * Live photos (Telegram Tips, 1.2.0-4): a short muted clip shown like a
 * photo. The file's category says how it plays: "live" — once (stops on the
 * last frame; a tap replays), "live-loop" — around and around,
 * "live-bounce" — forward, then backward, and again.
 */
export type LiveStyle = "live" | "live-loop" | "live-bounce";

export const LIVE_STYLES: { id: LiveStyle; label: string }[] = [
  { id: "live", label: "Live — plays once" },
  { id: "live-loop", label: "Loop — plays continuously" },
  { id: "live-bounce", label: "Bounce — forward and reverse" },
];

export const isLive = (category: string | null | undefined) => !!category && category.toLowerCase().startsWith("live");

/** Svelte action on a <video>: play it the way `style` says. */
export function livePlay(v: HTMLVideoElement, style: string) {
  let raf = 0;
  let back = false;
  let last = 0;
  const step = (t: number) => {
    const dt = last ? (t - last) / 1000 : 0;
    last = t;
    const next = v.currentTime - dt;
    if (next <= 0.02) {
      back = false;
      v.currentTime = 0;
      void v.play().catch(() => {});
      return;
    }
    v.currentTime = next;
    raf = requestAnimationFrame(step);
  };
  const ended = () => {
    if (style === "live-bounce") {
      back = true;
      last = 0;
      raf = requestAnimationFrame(step);
    }
  };
  const tap = () => {
    if (style !== "live") return;
    v.currentTime = 0;
    void v.play().catch(() => {});
  };
  v.muted = true;
  v.loop = style === "live-loop";
  v.addEventListener("ended", ended);
  v.addEventListener("click", tap);
  return {
    destroy() {
      cancelAnimationFrame(raf);
      v.removeEventListener("ended", ended);
      v.removeEventListener("click", tap);
      void back;
    },
  };
}
