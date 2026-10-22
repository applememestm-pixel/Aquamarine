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
    // Phones fire resize while scrolling (the address bar): keep the dots.
    if (pts.length && innerWidth === W && Math.abs(innerHeight - H) < 160) return;
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
  let last = 0;
  function frame(t = 0) {
    raf = requestAnimationFrame(frame);
    if (t - last < 33) return; // ~30 fps is plenty for drifting dots
    last = t;
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
  // Quick and early: things start rising a bit before they reach the screen,
  // and only what enters together is staggered (docs text isn't animated).
  const SEL = ".sec-head, .card, .dl, .step, .code, .note, .api-hero, .ep, .mock, .hero > div:nth-child(2)";
  const io = "IntersectionObserver" in window && !reduce ? new IntersectionObserver((es) => {
    let k = 0;
    for (const e of es) if (e.isIntersecting) {
      e.target.style.setProperty("--rv-delay", Math.min(k++, 5) * 45 + "ms");
      e.target.classList.add("in"); io.unobserve(e.target);
    }
  }, { threshold: 0, rootMargin: "0px 0px 12% 0px" }) : null;
  function scan(scope = document) {
    scope.querySelectorAll(SEL).forEach((el) => {
      if (el.classList.contains("rv") || el.closest(".md")) return;
      el.classList.add("rv");
      if (io) io.observe(el); else el.classList.add("in");
    });
  }
  scan();
  new MutationObserver((ms) => { for (const m of ms) for (const n of m.addedNodes) if (n.nodeType === 1) scan(n.parentElement || document); })
    .observe(document.body, { childList: true, subtree: true });

  // ---- section rail: the page's sections, always at hand ----
  const secs = [...document.querySelectorAll("section.band[id]")].filter((s) => s.querySelector(".eyebrow"));
  if (secs.length >= 3) {
    const rail = document.createElement("nav");
    rail.className = "sec-rail";
    rail.setAttribute("aria-label", "Sections");
    const label = (s) => (s.querySelector(".eyebrow")?.textContent || s.id).trim();
    const links = secs.map((s) => {
      const a = document.createElement("a");
      a.href = "#" + s.id; a.textContent = label(s);
      rail.appendChild(a);
      return a;
    });
    document.body.appendChild(rail);
    // Labels follow the language switch.
    const relabel = new MutationObserver(() => links.forEach((a, i) => { const t = label(secs[i]); if (a.textContent !== t) a.textContent = t; }));
    secs.forEach((s) => { const e = s.querySelector(".eyebrow"); if (e) relabel.observe(e, { subtree: true, characterData: true, childList: true }); });
    const spy = new IntersectionObserver((es) => {
      for (const e of es) if (e.isIntersecting) {
        const i = secs.indexOf(e.target);
        links.forEach((a, j) => a.classList.toggle("on", j === i));
      }
    }, { rootMargin: "-45% 0px -50% 0px" });
    secs.forEach((s) => spy.observe(s));
  }


  // ---- tour: the phone shows whichever card is in the middle of the screen ----
  const phone = document.querySelector(".tour-phone");
  if (phone) {
    const cards = [...document.querySelectorAll(".tour-card")];
    const typed = phone.querySelector(".tp-typed");
    let typer = 0;
    const typeIt = () => {
      if (!typed) return;
      clearInterval(typer);
      const word = "party";
      let i = 0;
      typed.dataset.text = "";
      typer = setInterval(() => { typed.dataset.text = word.slice(0, ++i); if (i >= word.length) clearInterval(typer); }, 120);
    };
    const show = (card) => {
      cards.forEach((c) => c.classList.toggle("on", c === card));
      const step = card.dataset.step;
      if (phone.dataset.on === step) return;
      phone.dataset.on = step;
      // restart the screen's little animations
      phone.querySelectorAll(`[data-screen="${step}"] *`).forEach((el) => { el.style.animation = "none"; void el.offsetWidth; el.style.animation = ""; });
      if (step === "gif") typeIt();
    };
    // The card nearest to the reading line wins (the middle of the screen;
    // on phones, the part below the sticky phone).
    let pend = 0;
    const pick = () => {
      pend = 0;
      const narrow = innerWidth <= 900;
      const line = narrow ? innerHeight * 0.78 : innerHeight * 0.5;
      let best = cards[0], bd = Infinity;
      for (const c of cards) {
        const r = c.getBoundingClientRect();
        const d = Math.abs((r.top + r.bottom) / 2 - line);
        if (d < bd) { bd = d; best = c; }
      }
      show(best);
    };
    addEventListener("scroll", () => { if (!pend) pend = requestAnimationFrame(pick); }, { passive: true });
    addEventListener("resize", pick);
    pick();
  }

  // ---- a comet behind the pointer: little stars that fall and fade ----
  if (!reduce && matchMedia("(pointer: fine)").matches) {
    const tc = document.createElement("canvas");
    tc.className = "fx-trail";
    tc.setAttribute("aria-hidden", "true");
    document.body.appendChild(tc);
    const tx = tc.getContext("2d");
    let tw = 0, th = 0, tdpr = 1, sparks = [], tRaf = 0, px = -1, py = -1;
    const fit = () => {
      tdpr = Math.min(2, devicePixelRatio || 1); tw = innerWidth; th = innerHeight;
      tc.width = tw * tdpr; tc.height = th * tdpr; tc.style.width = tw + "px"; tc.style.height = th + "px";
      tx.setTransform(tdpr, 0, 0, tdpr, 0, 0);
    };
    fit(); addEventListener("resize", fit);
    const COLORS = ["0,229,199", "123,92,255", "255,255,255", "130,240,255"];
    const star = (x, y, r, rot) => {
      tx.beginPath();
      for (let i = 0; i < 8; i++) {
        const a = rot + (i * Math.PI) / 4, rr = i % 2 ? r * 0.38 : r;
        tx.lineTo(x + Math.cos(a) * rr, y + Math.sin(a) * rr);
      }
      tx.closePath(); tx.fill();
    };
    const tick = () => {
      tx.clearRect(0, 0, tw, th);
      sparks = sparks.filter((p) => (p.life -= 0.022) > 0);
      for (const p of sparks) {
        p.vy += 0.045; p.x += p.vx; p.y += p.vy; p.rot += p.spin;
        tx.fillStyle = `rgba(${p.c},${p.life * 0.9})`;
        tx.shadowColor = `rgba(${p.c},${p.life})`; tx.shadowBlur = 8;
        star(p.x, p.y, p.r * (0.5 + p.life * 0.5), p.rot);
      }
      tx.shadowBlur = 0;
      tRaf = sparks.length ? requestAnimationFrame(tick) : 0;
    };
    addEventListener("pointermove", (e) => {
      if (px < 0) { px = e.clientX; py = e.clientY; return; }
      const dx = e.clientX - px, dy = e.clientY - py, d = Math.hypot(dx, dy);
      px = e.clientX; py = e.clientY;
      if (d < 3) return;
      const n = Math.min(3, Math.ceil(d / 22));
      for (let i = 0; i < n && sparks.length < 90; i++) {
        sparks.push({
          x: px - dx * (i / n) + (Math.random() - 0.5) * 6, y: py - dy * (i / n) + (Math.random() - 0.5) * 6,
          vx: -dx * 0.03 + (Math.random() - 0.5) * 0.8, vy: -dy * 0.03 + (Math.random() - 0.5) * 0.8 - 0.4,
          r: 2 + Math.random() * 3.2, rot: Math.random() * Math.PI, spin: (Math.random() - 0.5) * 0.2,
          life: 0.8 + Math.random() * 0.2, c: COLORS[(Math.random() * COLORS.length) | 0],
        });
      }
      if (!tRaf) tRaf = requestAnimationFrame(tick);
    }, { passive: true });
  }

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
