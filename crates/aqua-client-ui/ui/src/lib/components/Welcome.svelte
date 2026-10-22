<script lang="ts">
  /**
   * What the chat pane shows before a chat is picked: the logo breathing
   * over drifting glows and a slow constellation (same feel as the site).
   * Stops when hidden, in the soft renderer and for reduced motion.
   */
  import { onMount } from "svelte";
  import { appIconUrl } from "../appIcon";
  import { t } from "../i18n";

  let canvas = $state<HTMLCanvasElement | null>(null);

  onMount(() => {
    const c = canvas;
    if (!c) return;
    const calm = matchMedia("(prefers-reduced-motion: reduce)").matches
      || document.documentElement.classList.contains("soft-render") || document.documentElement.classList.contains("wk-lowfx");
    if (calm) return;
    const ctx = c.getContext("2d");
    if (!ctx) return;
    const accent = getComputedStyle(document.documentElement).getPropertyValue("--accent").trim() || "#00E5C7";
    const rgb = /^#([0-9a-f]{6})$/i.test(accent)
      ? [0, 2, 4].map((i) => parseInt(accent.slice(1 + i, 3 + i), 16)).join(",")
      : "0,229,199";
    let W = 0, H = 0, raf = 0;
    let pts: { x: number; y: number; vx: number; vy: number; r: number; a: number }[] = [];
    const fit = () => {
      const r = c.getBoundingClientRect();
      const dpr = Math.min(2, devicePixelRatio || 1);
      W = r.width; H = r.height;
      c.width = W * dpr; c.height = H * dpr;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      const n = Math.round(Math.min(46, (W * H) / 26000));
      pts = Array.from({ length: n }, () => ({
        x: Math.random() * W, y: Math.random() * H,
        vx: (Math.random() - 0.5) * 0.25, vy: (Math.random() - 0.5) * 0.25,
        r: Math.random() * 1.5 + 0.5, a: Math.random() * 0.45 + 0.2,
      }));
    };
    const draw = () => {
      ctx.clearRect(0, 0, W, H);
      for (const p of pts) {
        p.x += p.vx; p.y += p.vy;
        if (p.x < 0 || p.x > W) p.vx *= -1;
        if (p.y < 0 || p.y > H) p.vy *= -1;
        ctx.beginPath(); ctx.arc(p.x, p.y, p.r, 0, Math.PI * 2);
        ctx.fillStyle = `rgba(${rgb},${p.a})`; ctx.fill();
      }
      for (let i = 0; i < pts.length; i++) for (let j = i + 1; j < pts.length; j++) {
        const a = pts[i], b = pts[j], d = Math.hypot(a.x - b.x, a.y - b.y);
        if (d < 120) {
          ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y);
          ctx.strokeStyle = `rgba(${rgb},${0.08 * (1 - d / 120)})`; ctx.stroke();
        }
      }
      raf = requestAnimationFrame(draw);
    };
    const ro = new ResizeObserver(fit);
    ro.observe(c);
    fit();
    const vis = () => { cancelAnimationFrame(raf); if (!document.hidden) raf = requestAnimationFrame(draw); };
    document.addEventListener("visibilitychange", vis);
    raf = requestAnimationFrame(draw);
    return () => { cancelAnimationFrame(raf); ro.disconnect(); document.removeEventListener("visibilitychange", vis); };
  });
</script>

<div class="welcome">
  <div class="glow g1"></div>
  <div class="glow g2"></div>
  <canvas bind:this={canvas}></canvas>
  <div class="center">
    <img class="logo" src={$appIconUrl} alt="" draggable="false" />
    <h2>Aquamarine</h2>
    <p>{$t("Pick a chat on the left.")}</p>
    <div class="lock"><span class="dot"></span>{$t("Messages are end-to-end encrypted")}</div>
  </div>
</div>

<style>
  .welcome { position: relative; place-self: stretch; overflow: hidden; display: grid; place-items: center; min-height: 0; height: 100%; }
  canvas { position: absolute; inset: 0; width: 100%; height: 100%; pointer-events: none; }
  .glow { position: absolute; border-radius: 50%; opacity: .22; pointer-events: none; }
  .g1 { width: 680px; height: 680px; background: radial-gradient(circle, var(--accent) 0%, transparent 62%); top: -270px; left: -230px; animation: drift 15s ease-in-out infinite; }
  .g2 { width: 620px; height: 620px; background: radial-gradient(circle, var(--accent-2, #7B5CFF) 0%, transparent 62%); bottom: -260px; right: -200px; animation: drift 15s ease-in-out -7.5s infinite; }
  @keyframes drift { 50% { transform: translate(40px, 30px) scale(1.15); } }
  .center { position: relative; text-align: center; animation: rise .6s cubic-bezier(.2,.8,.2,1) both; }
  @keyframes rise { from { opacity: 0; transform: translateY(18px); } }
  .logo { width: 92px; height: 92px; border-radius: 24px; animation: breathe 4.5s ease-in-out infinite, bob 6s ease-in-out infinite; }
  @keyframes breathe { 0%, 100% { box-shadow: 0 0 26px -6px var(--accent-glow, rgba(0,229,199,.45)); } 50% { box-shadow: 0 0 48px 2px var(--accent-glow, rgba(0,229,199,.45)); } }
  @keyframes bob { 50% { transform: translateY(-8px); } }
  h2 { margin: 18px 0 4px; font-size: 22px; font-weight: 800; letter-spacing: -.02em;
    background: linear-gradient(110deg, var(--fg), var(--accent)); -webkit-background-clip: text; background-clip: text; color: transparent; }
  p { margin: 0; color: var(--fg-dim); font-size: 14px; }
  .lock { margin-top: 16px; display: inline-flex; align-items: center; gap: 8px; padding: 6px 12px; border-radius: 999px;
    font-size: 12px; color: var(--accent); background: var(--accent-soft); border: 1px solid color-mix(in srgb, var(--accent) 30%, transparent); }
  .dot { width: 6px; height: 6px; border-radius: 50%; background: var(--accent); animation: blip 2.4s ease-in-out infinite; }
  @keyframes blip { 50% { opacity: .35; transform: scale(.7); } }
  @media (prefers-reduced-motion: reduce) { .glow, .logo, .dot, .center { animation: none; } }
  /* Linux (WebKitGTK): endless animations repaint the whole window on the CPU. */
  :global(html.wk-lowfx) .glow, :global(html.wk-lowfx) .logo, :global(html.wk-lowfx) .dot { animation: none; }
</style>
