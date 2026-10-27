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
    const n = Math.round(Math.min(110, (W * H) / 14000));
    pts = Array.from({ length: n }, () => ({
      x: Math.random() * W, y: Math.random() * H,
      vx: (Math.random() - 0.5) * 0.6, vy: (Math.random() - 0.5) * 0.6,
      r: Math.random() * 1.8 + 0.8, a: Math.random() * 0.45 + 0.45,
      tw: Math.random() * Math.PI * 2, ts: 0.03 + Math.random() * 0.05, // twinkle phase and speed
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
      p.tw += p.ts;
      const a = p.a * (0.6 + 0.4 * Math.sin(p.tw));
      // a soft halo, then the star (cheaper than shadowBlur on phones)
      ctx.beginPath(); ctx.arc(p.x, p.y, p.r * 3, 0, Math.PI * 2);
      ctx.fillStyle = `rgba(0,229,199,${a * 0.12})`; ctx.fill();
      ctx.beginPath(); ctx.arc(p.x, p.y, p.r, 0, Math.PI * 2);
      ctx.fillStyle = `rgba(0,229,199,${a})`; ctx.fill();
    }
    for (let i = 0; i < pts.length; i++) for (let j = i + 1; j < pts.length; j++) {
      const a = pts[i], b = pts[j], d = Math.hypot(a.x - b.x, a.y - b.y);
      if (d < link) {
        ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y);
        ctx.strokeStyle = `rgba(0,229,199,${0.16 * (1 - d / link)})`; ctx.lineWidth = 1; ctx.stroke();
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

  // ---- the cursor: a small teal dot instead of the arrow, a thin comet
  // tail behind it (the colour of the site's text selection). Selecting
  // text or typing brings the normal cursor back. A button in the nav turns
  // both off (remembered): the normal cursor, no tail. ----
  if (!reduce && matchMedia("(pointer: fine)").matches) {
    const FX_KEY = "aqua.site.fx";
    let fxOn = (() => { try { return localStorage.getItem(FX_KEY) !== "off"; } catch { return true; } })();

    const tc = document.createElement("canvas");
    tc.className = "fx-trail";
    tc.setAttribute("aria-hidden", "true");
    document.body.appendChild(tc);
    const dot = document.createElement("div");
    dot.className = "fx-dot";
    dot.setAttribute("aria-hidden", "true");
    document.body.appendChild(dot);
    const tx = tc.getContext("2d");
    let tw = 0, th = 0, tdpr = 1, tRaf = 0;
    const fit = () => {
      tdpr = Math.min(2, devicePixelRatio || 1); tw = innerWidth; th = innerHeight;
      tc.width = tw * tdpr; tc.height = th * tdpr; tc.style.width = tw + "px"; tc.style.height = th + "px";
      tx.setTransform(tdpr, 0, 0, tdpr, 0, 0);
    };
    fit(); addEventListener("resize", fit);
    // The last positions of the pointer; each lives TAIL ms. The head chases
    // the pointer a little (smoothing), so the tail curves instead of zigzagging.
    const TAIL = 480, WIDTH = 2.6;
    let pts = [], head = null, target = null, still = 0;
    const wipe = () => { pts = []; head = null; tx.clearRect(0, 0, tw, th); };
    const tick = (now) => {
      tRaf = 0;
      // Turned off (or the pointer left): nothing more is drawn, and the
      // loop doesn't keep itself alive by chasing the pointer.
      if (!fxOn || !target) { wipe(); return; }
      if (target) {
        if (!head) head = { ...target };
        head.x += (target.x - head.x) * 0.32; head.y += (target.y - head.y) * 0.32;
        const lastPt = pts[pts.length - 1];
        if (!lastPt || Math.hypot(head.x - lastPt.x, head.y - lastPt.y) > 1.5) pts.push({ x: head.x, y: head.y, t: now });
      }
      pts = pts.filter((p) => now - p.t < TAIL);
      tx.clearRect(0, 0, tw, th);
      if (pts.length > 1) {
        tx.lineCap = "round"; tx.lineJoin = "round";
        tx.globalCompositeOperation = "lighter";
        for (let i = 1; i < pts.length; i++) {
          const a = pts[i - 1], b = pts[i];
          const k = 1 - (now - b.t) / TAIL;          // 1 at the head, 0 at the end
          tx.strokeStyle = `rgba(0,229,199,${0.5 * k})`;
          tx.shadowColor = `rgba(0,229,199,${0.35 * k})`;
          tx.shadowBlur = 6 * k;
          tx.lineWidth = Math.max(0.4, WIDTH * k);
          tx.beginPath(); tx.moveTo(a.x, a.y); tx.lineTo(b.x, b.y); tx.stroke();
        }
        tx.shadowBlur = 0;
        tx.globalCompositeOperation = "source-over";
      }
      // Keep going while the tail is still fading or the head catches up.
      if (pts.length || (head && target && Math.hypot(target.x - head.x, target.y - head.y) > 0.5)) tRaf = requestAnimationFrame(tick);
      else if (now - still > 100) head = null;
    };

    // Where the dot gives way to the normal cursor: text being selected,
    // fields to type in.
    const TYPING = "input, textarea, select, [contenteditable=''], [contenteditable='true']";
    let selecting = false, overField = false;
    const native = () => !fxOn || selecting || overField;
    const paint = () => {
      const show = !native() && !!target;
      root.classList.toggle("fx-cursor", !native());
      dot.classList.toggle("on", show);
    };
    document.addEventListener("selectionchange", () => {
      const s = getSelection();
      selecting = !!s && !s.isCollapsed && String(s).trim() !== "";
      paint();
    });
    addEventListener("pointermove", (e) => {
      target = { x: e.clientX, y: e.clientY };
      overField = !!(e.target.closest && e.target.closest(TYPING));
      dot.style.transform = `translate(${e.clientX}px, ${e.clientY}px)`;
      const link = e.target.closest && e.target.closest("a, button, summary, label, [role=button]");
      dot.classList.toggle("big", !!link);
      paint();
      if (!fxOn || native()) return;
      still = performance.now();
      if (!tRaf) tRaf = requestAnimationFrame(tick);
    }, { passive: true });
    addEventListener("pointerdown", () => dot.classList.add("press"), { passive: true });
    addEventListener("pointerup", () => dot.classList.remove("press"), { passive: true });
    const away = () => { target = null; wipe(); paint(); };
    document.addEventListener("pointerleave", away);
    document.documentElement.addEventListener("mouseleave", away);
    addEventListener("blur", away);
    document.addEventListener("visibilitychange", () => { if (document.hidden) away(); });

    // The switch in the nav.
    const btn = document.createElement("button");
    btn.type = "button"; btn.className = "fx-toggle";
    const label = () => {
      // The same language the page picks (index.html, docs.js).
      let ru = true;
      try { ru = (localStorage.getItem("aqua.site.lang") || (navigator.language.toLowerCase().startsWith("ru") ? "ru" : "en")) === "ru"; } catch { /* storage blocked */ }
      btn.title = fxOn ? (ru ? "Выключить эффекты курсора (след и точку)" : "Turn the cursor effects off (trail and dot)") : (ru ? "Включить эффекты курсора" : "Turn the cursor effects on");
      btn.setAttribute("aria-label", btn.title);
      btn.setAttribute("aria-pressed", String(fxOn));
    };
    btn.innerHTML = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><circle cx="16" cy="8" r="3"/><path d="M13.5 10.5 4 20M11 9 6 14M15 13l-5 5"/></svg>';
    btn.addEventListener("click", () => {
      fxOn = !fxOn;
      try { localStorage.setItem(FX_KEY, fxOn ? "on" : "off"); } catch { /* private mode */ }
      if (!fxOn) wipe();
      btn.classList.toggle("off", !fxOn);
      label(); paint();
    });
    btn.classList.toggle("off", !fxOn);
    label();
    const navIn = document.querySelector(".nav-in");
    if (navIn) navIn.insertBefore(btn, navIn.querySelector(".lang-btn, .nav-cta"));
    paint();
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
