/* Aquamarine site motion: a living background (grid, drifting glows, a
   constellation of particles), sections that rise in as they scroll into
   view, a chat mock that types, cards that light up under the cursor.
   Everything stops for prefers-reduced-motion and when the tab is hidden. */
(() => {
  const reduce = matchMedia("(prefers-reduced-motion: reduce)").matches;
  const root = document.documentElement;
  root.classList.add("js-motion");

  // ---- background layers ----
  const bg = document.createElement("div");
  bg.className = "fx-bg";
  bg.setAttribute("aria-hidden", "true");
  bg.innerHTML = '<div class="fx-grid"></div><div class="fx-glow g1"></div><div class="fx-glow g2"></div><div class="fx-glow g3"></div><canvas class="fx-particles"></canvas>';
  document.body.prepend(bg);

  // ---- particles: points drifting, linked when close ----
  const canvas = bg.querySelector("canvas");
  const ctx = canvas.getContext("2d");
  let W = 0, H = 0, dpr = 1, pts = [], raf = 0, mouse = { x: -1e4, y: -1e4 };
  function resize() {
    dpr = Math.min(2, devicePixelRatio || 1);
    W = innerWidth; H = innerHeight;
    canvas.width = W * dpr; canvas.height = H * dpr;
    canvas.style.width = W + "px"; canvas.style.height = H + "px";
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    const n = Math.round(Math.min(70, (W * H) / 22000));
    pts = Array.from({ length: n }, () => ({
      x: Math.random() * W, y: Math.random() * H,
      vx: (Math.random() - 0.5) * 0.32, vy: (Math.random() - 0.5) * 0.32,
      r: Math.random() * 1.6 + 0.5, a: Math.random() * 0.5 + 0.25,
    }));
  }
  function frame() {
    ctx.clearRect(0, 0, W, H);
    const link = 130;
    for (const p of pts) {
      p.x += p.vx; p.y += p.vy;
      if (p.x < 0 || p.x > W) p.vx *= -1;
      if (p.y < 0 || p.y > H) p.vy *= -1;
      // gently pushed away from the cursor
      const dx = p.x - mouse.x, dy = p.y - mouse.y, d2 = dx * dx + dy * dy;
      if (d2 < 14000) { const k = 0.6 / Math.sqrt(d2 + 1); p.x += dx * k; p.y += dy * k; }
      ctx.beginPath(); ctx.arc(p.x, p.y, p.r, 0, Math.PI * 2);
      ctx.fillStyle = `rgba(0,229,199,${p.a})`; ctx.fill();
    }
    for (let i = 0; i < pts.length; i++) for (let j = i + 1; j < pts.length; j++) {
      const a = pts[i], b = pts[j], d = Math.hypot(a.x - b.x, a.y - b.y);
      if (d < link) {
        ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y);
        ctx.strokeStyle = `rgba(0,229,199,${0.09 * (1 - d / link)})`; ctx.lineWidth = 1; ctx.stroke();
      }
    }
    raf = requestAnimationFrame(frame);
  }
  if (!reduce) {
    resize();
    addEventListener("resize", resize);
    addEventListener("pointermove", (e) => { mouse.x = e.clientX; mouse.y = e.clientY; }, { passive: true });
    document.addEventListener("visibilitychange", () => {
      cancelAnimationFrame(raf);
      if (!document.hidden) raf = requestAnimationFrame(frame);
    });
    raf = requestAnimationFrame(frame);
  }

  // ---- nav gets a backdrop once the page scrolls ----
  const nav = document.querySelector("nav");
  const onScroll = () => nav && nav.classList.toggle("scrolled", scrollY > 24);
  addEventListener("scroll", onScroll, { passive: true }); onScroll();

  // ---- reveal on scroll (also for cards rendered later by the page) ----
  const SEL = ".sec-head, .card, .dl, .step, .code, .note, .md > *, .api-hero, .ep, .mock, .hero > div:nth-child(2)";
  const io = "IntersectionObserver" in window && !reduce ? new IntersectionObserver((es) => {
    for (const e of es) if (e.isIntersecting) { e.target.classList.add("in"); io.unobserve(e.target); }
  }, { threshold: 0.12, rootMargin: "0px 0px -6% 0px" }) : null;
  function scan(scope = document) {
    const groups = new Map();
    scope.querySelectorAll(SEL).forEach((el) => {
      if (el.classList.contains("rv")) return;
      el.classList.add("rv");
      // stagger siblings: 0, 70, 140 ms …
      const g = groups.get(el.parentElement) ?? 0;
      groups.set(el.parentElement, g + 1);
      el.style.setProperty("--rv-delay", Math.min(g, 8) * 70 + "ms");
      if (io) io.observe(el); else el.classList.add("in");
    });
  }
  scan();
  new MutationObserver((ms) => { for (const m of ms) for (const n of m.addedNodes) if (n.nodeType === 1) scan(n.parentElement || document); })
    .observe(document.body, { childList: true, subtree: true });

  // ---- cards light up where the cursor is ----
  document.addEventListener("pointermove", (e) => {
    const c = e.target.closest && e.target.closest(".card, .dl, .code, .note");
    if (!c) return;
    const r = c.getBoundingClientRect();
    c.style.setProperty("--mx", e.clientX - r.left + "px");
    c.style.setProperty("--my", e.clientY - r.top + "px");
  }, { passive: true });

  // ---- the chat mock types its last message, again and again ----
  const body = document.querySelector(".mock-body");
  if (body && !reduce) {
    const typing = document.createElement("div");
    typing.className = "typing"; typing.innerHTML = "<span></span><span></span><span></span>";
    body.appendChild(typing);
    const kids = [...body.children];
    const play = () => {
      kids.forEach((k, i) => { k.classList.remove("show"); setTimeout(() => k.classList.add("show"), 500 + i * 650); });
    };
    play();
    setInterval(play, 9000);
  } else if (body) {
    [...body.children].forEach((k) => k.classList.add("show"));
  }
})();
