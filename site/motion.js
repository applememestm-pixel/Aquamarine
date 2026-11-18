/* Aquamarine site motion: a quiet background (a few slow points, faintly
   linked, in the theme's accent), sections that rise in as they scroll into
   view, a chat mock that types.
   Everything stops for prefers-reduced-motion and when the tab is hidden. */
(() => {
  const reduce = matchMedia("(prefers-reduced-motion: reduce)").matches;
  const root = document.documentElement;
  root.classList.add("js-motion");

  // ---- background layers ----
  const bg = document.createElement("div");
  bg.className = "fx-bg";
  bg.setAttribute("aria-hidden", "true");
  bg.innerHTML = '<canvas class="fx-particles"></canvas>';
  document.body.prepend(bg);

  // ---- particles: points drifting, linked when close ----
  const canvas = bg.querySelector("canvas");
  const ctx = canvas.getContext("2d");
  let W = 0, H = 0, dpr = 1, pts = [], raf = 0, mouse = { x: -1e4, y: -1e4 };
  // The accent as "r,g,b" (style.css --fx-rgb); follows the theme.
  const fxRgb = () => getComputedStyle(root).getPropertyValue("--fx-rgb").trim() || "90,190,120";
  let rgb = fxRgb();
  function resize() {
    // Phones fire resize while scrolling (the address bar): keep the dots.
    if (pts.length && innerWidth === W && Math.abs(innerHeight - H) < 160) return;
    dpr = Math.min(2, devicePixelRatio || 1);
    W = innerWidth; H = innerHeight;
    canvas.width = W * dpr; canvas.height = H * dpr;
    canvas.style.width = W + "px"; canvas.style.height = H + "px";
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    const n = Math.round(Math.min(56, (W * H) / 28000));
    pts = Array.from({ length: n }, () => ({
      x: Math.random() * W, y: Math.random() * H,
      vx: (Math.random() - 0.5) * 0.22, vy: (Math.random() - 0.5) * 0.22,
      r: Math.random() * 1.3 + 0.9, a: Math.random() * 0.3 + 0.35,
      tw: Math.random() * Math.PI * 2, ts: 0.01 + Math.random() * 0.02, // twinkle phase and speed
    }));
  }
  let last = 0;
  function frame(t = 0) {
    raf = requestAnimationFrame(frame);
    if (t - last < 33) return; // ~30 fps is plenty for drifting dots
    last = t;
    ctx.clearRect(0, 0, W, H);
    const link = 120;
    for (const p of pts) {
      p.x += p.vx; p.y += p.vy;
      if (p.x < 0 || p.x > W) p.vx *= -1;
      if (p.y < 0 || p.y > H) p.vy *= -1;
      // gently pushed away from the cursor
      const dx = p.x - mouse.x, dy = p.y - mouse.y, d2 = dx * dx + dy * dy;
      if (d2 < 10000) { const k = 0.25 / Math.sqrt(d2 + 1); p.x += dx * k; p.y += dy * k; }
      p.tw += p.ts;
      const a = p.a * (0.6 + 0.4 * Math.sin(p.tw));
      ctx.beginPath(); ctx.arc(p.x, p.y, p.r, 0, Math.PI * 2);
      ctx.fillStyle = `rgba(${rgb},${a})`; ctx.fill();
    }
    for (let i = 0; i < pts.length; i++) for (let j = i + 1; j < pts.length; j++) {
      const a = pts[i], b = pts[j], d = Math.hypot(a.x - b.x, a.y - b.y);
      if (d < link) {
        ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y);
        ctx.strokeStyle = `rgba(${rgb},${0.13 * (1 - d / link)})`; ctx.lineWidth = 1; ctx.stroke();
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

  // ---- theme: dark by default, light by the system; the switch in the nav
  // remembers a choice (html[data-theme], set early by each page's head) ----
  const THEME_KEY = "aqua.site.theme";
  const sysLight = matchMedia("(prefers-color-scheme: light)");
  const isLight = () => root.dataset.theme ? root.dataset.theme === "light" : sysLight.matches;
  const navIn = document.querySelector(".nav-in");
  if (navIn) {
    const tb = document.createElement("button");
    tb.type = "button"; tb.className = "theme-btn";
    const SUN = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M2 12h2M20 12h2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/></svg>';
    const MOON = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M20 14.5A8 8 0 1 1 9.5 4a6.5 6.5 0 0 0 10.5 10.5Z"/></svg>';
    const paintBtn = () => {
      const ru = (document.documentElement.lang || "ru").startsWith("ru");
      tb.innerHTML = isLight() ? MOON : SUN;
      tb.title = isLight() ? (ru ? "Тёмная тема" : "Dark theme") : (ru ? "Светлая тема" : "Light theme");
      tb.setAttribute("aria-label", tb.title);
      rgb = fxRgb();
    };
    tb.addEventListener("click", () => {
      const next = isLight() ? "dark" : "light";
      root.dataset.theme = next;
      try { localStorage.setItem(THEME_KEY, next); } catch { /* private mode */ }
      paintBtn();
    });
    sysLight.addEventListener?.("change", paintBtn);
    navIn.insertBefore(tb, navIn.querySelector(".lang-btn, .nav-cta"));
    paintBtn();
  }

  // ---- the CeZeOs sun in the corner: turns a little each time the reader
  // moves to another section (or heading, on the docs) ----
  const sun = document.createElement("span");
  sun.className = "sun corner-sun"; sun.setAttribute("aria-hidden", "true");
  document.body.prepend(sun);
  let turn = 0, current = null;
  const SUN_SEL = ".hero, section.band, .api-hero, .ad-hero, .md h1, .md h2";
  const sunSpy = "IntersectionObserver" in window ? new IntersectionObserver((es) => {
    for (const e of es) if (e.isIntersecting && e.target !== current) {
      if (current) { turn += 45; root.style.setProperty("--sun-turn", turn + "deg"); }
      current = e.target;
    }
  }, { rootMargin: "-40% 0px -55% 0px" }) : null;
  const watchSun = (scope) => { if (sunSpy) scope.querySelectorAll(SUN_SEL).forEach((el) => sunSpy.observe(el)); };
  watchSun(document);
  if (sunSpy) new MutationObserver((ms) => { for (const m of ms) for (const n of m.addedNodes) if (n.nodeType === 1) { if (n.matches?.(SUN_SEL)) sunSpy.observe(n); watchSun(n); } })
    .observe(document.body, { childList: true, subtree: true });

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


  // ---- tour: a carousel. Arrows on both sides, dots, ←/→, a swipe on the
  // phone; it moves on by itself until someone touches it. ----
  const tour = document.querySelector(".tour");
  const phone = tour && tour.querySelector(".tour-phone");
  if (tour && phone) {
    const cards = [...tour.querySelectorAll(".tour-card")];
    const dots = tour.querySelector(".tour-dots");
    const prog = document.createElement("i");
    prog.className = "tp-progress"; phone.appendChild(prog);
    const typed = phone.querySelector(".tp-typed");
    let typer = 0, cur = -1, timer = 0, manual = false;
    const AUTO = 7000;
    tour.style.setProperty("--auto", AUTO + "ms");
    const typeIt = () => {
      if (!typed) return;
      clearInterval(typer);
      const word = typed.dataset.word || (typed.dataset.word = typed.dataset.text || "race");
      let i = 0; typed.dataset.text = "";
      typer = setInterval(() => { typed.dataset.text = word.slice(0, ++i); if (i >= word.length) clearInterval(typer); }, 120);
    };
    const dotBtns = cards.map((c, i) => {
      const d = document.createElement("button");
      d.type = "button"; d.setAttribute("role", "tab");
      d.setAttribute("aria-label", (i + 1) + ": " + (c.querySelector("h3")?.textContent || ""));
      d.addEventListener("click", () => { manual = true; go(i); });
      dots.appendChild(d);
      return d;
    });
    const restartAuto = () => {
      clearTimeout(timer);
      tour.classList.remove("playing");
      if (reduce || manual) return;
      void prog.offsetWidth;
      tour.classList.add("playing");
      timer = setTimeout(() => go(cur + 1), AUTO);
    };
    function go(i) {
      const n = cards.length;
      i = ((i % n) + n) % n;
      const dir = i > cur || (cur === n - 1 && i === 0) ? 1 : -1;
      cards.forEach((c, j) => {
        c.style.setProperty("--from", (j === i ? -dir * 30 : dir * -30) + "px");
        c.classList.toggle("on", j === i);
        c.setAttribute("aria-hidden", String(j !== i));
      });
      dotBtns.forEach((d, j) => { d.classList.toggle("on", j === i); d.setAttribute("aria-selected", String(j === i)); });
      cur = i;
      const step = cards[i].dataset.step;
      phone.dataset.on = step;
      // restart the screen's little animations
      phone.querySelectorAll(`[data-screen="${step}"] *`).forEach((el) => { el.style.animation = "none"; void el.offsetWidth; el.style.animation = ""; });
      if (step === "gif") typeIt();
      restartAuto();
    }
    const step = (d) => { manual = true; go(cur + d); };
    tour.querySelector(".tour-arrow.prev").addEventListener("click", () => step(-1));
    tour.querySelector(".tour-arrow.next").addEventListener("click", () => step(1));
    tour.addEventListener("keydown", (e) => {
      if (e.key === "ArrowLeft") { e.preventDefault(); step(-1); }
      if (e.key === "ArrowRight") { e.preventDefault(); step(1); }
    });
    // Swipe (or drag with the mouse) across the phone.
    let sx = null;
    phone.addEventListener("pointerdown", (e) => { sx = e.clientX; });
    addEventListener("pointerup", (e) => {
      if (sx === null) return;
      const dx = e.clientX - sx; sx = null;
      if (Math.abs(dx) > 40) step(dx < 0 ? 1 : -1);
    });
    // Waits while the pointer is over it or it's off screen.
    tour.addEventListener("pointerenter", () => { clearTimeout(timer); tour.classList.remove("playing"); });
    tour.addEventListener("pointerleave", restartAuto);
    if ("IntersectionObserver" in window) new IntersectionObserver((es) => {
      for (const e of es) if (e.isIntersecting) restartAuto(); else { clearTimeout(timer); tour.classList.remove("playing"); }
    }).observe(tour);

    // The owner's sticker / emoji packs (tools/site-packs.sh) replace the
    // stand-ins when the site has them.
    fetch("assets/packs/manifest.json").then((r) => (r.ok ? r.json() : null)).then((m) => {
      if (!m) return;
      const el = (src, cls) => {
        const v = /\.webm$/i.test(src);
        const x = document.createElement(v ? "video" : "img");
        x.src = src; if (cls) x.className = cls;
        if (v) { x.muted = true; x.loop = true; x.autoplay = true; x.playsInline = true; } else x.loading = "lazy";
        return x;
      };
      if (m.sticker && m.sticker.length) {
        const grid = phone.querySelector('[data-pack="sticker"]');
        grid.replaceChildren(...m.sticker.slice(0, 8).map((src) => { const d = document.createElement("div"); d.className = "st"; d.appendChild(el(src)); return d; }));
        phone.querySelector("[data-pack-sent]").replaceChildren(el(m.sticker[0]));
      }
      if (m.emoji && m.emoji.length) {
        phone.querySelector('[data-pack="emoji"]').replaceChildren(...m.emoji.slice(0, 7).map((src) => el(src)));
      }
    }).catch(() => { /* no packs on this copy of the site */ });
    go(0);
  }

  // ---- the storage donut: point at a part (or its legend line) to see it ----
  document.querySelectorAll(".donut").forEach((d) => {
    const store = d.closest(".tp-store");
    const segs = [...d.querySelectorAll(".seg")];
    const legs = store ? [...store.querySelectorAll(".legend [data-seg]")] : [];
    const b = d.querySelector(".hole b"), sm = d.querySelector(".hole small");
    const total = b.textContent, totalSub = sm.textContent;
    const show = (i) => {
      d.classList.toggle("hover", i != null);
      segs.forEach((s, j) => s.classList.toggle("on", j === i));
      legs.forEach((l, j) => l.classList.toggle("on", j === i));
      if (i == null) { b.textContent = total; sm.textContent = totalSub; return; }
      b.textContent = legs[i]?.dataset.gb || ""; sm.textContent = legs[i]?.textContent.trim() || "";
    };
    segs.forEach((s, i) => { s.addEventListener("pointerenter", () => show(i)); s.addEventListener("pointerleave", () => show(null)); s.addEventListener("click", () => show(i)); });
    legs.forEach((l, i) => { l.addEventListener("pointerenter", () => show(i)); l.addEventListener("pointerleave", () => show(null)); l.addEventListener("click", () => show(i)); });
  });

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
