/* Aquamarine docs viewer: renders the repository's Markdown documents
   (copied into ./docs/ by .github/workflows/pages.yml) with GitHub-style
   heading anchors, rewrites links between documents, and builds the
   sidebar and the "on this page" list. Used by docs.html and api.html. */
(function () {
  const REPO = "https://github.com/applememestm-pixel/Aquamarine";
  const GROUPS = [
    ["Обзор", "Overview", [
      ["passport", "Техпаспорт", "Technical passport"],
      ["README", "Оглавление документации", "Documentation index"],
      ["architecture", "Архитектура", "Architecture"],
    ]],
    ["Безопасность", "Security", [
      ["protocol", "Протокол шифрования", "Encryption protocol"],
      ["security-model", "Модель угроз и данные", "Threat model & data"],
    ]],
    ["Функции", "Features", [
      ["delivery", "Транспорт и доставка", "Transport & delivery"],
      ["accounts", "Учётные записи и устройства", "Accounts & devices"],
      ["backups", "Резервные копии", "Backups"],
      ["storage", "Хранение на устройстве", "On-device storage"],
      ["files", "Файлы и медиа", "Files & media"],
      ["groups", "Группы и каналы", "Groups & channels"],
      ["calls", "Звонки и голосовые комнаты", "Calls & voice rooms"],
      ["workspace", "Календарь и рабочее пространство", "Calendar & workspace"],
      ["sealed-messages", "Запечатанные послания", "Sealed messages"],
      ["bluetooth", "Bluetooth", "Bluetooth"],
      ["stickers", "Стикеры и реакции", "Stickers & reactions"],
      ["profiles", "Профили и игры", "Profiles & games"],
      ["bots", "Боты и мини-приложения", "Bots & mini apps"],
    ]],
    ["Эксплуатация", "Operations", [
      ["api", "HTTP API", "HTTP API"],
      ["federation", "Федерация", "Federation"],
      ["deployment", "Развёртывание", "Deployment"],
      ["building", "Сборка и выпуск", "Building & releases"],
      ["licenses", "Лицензии", "Licenses"],
    ]],
  ];
  const IDS = new Set(GROUPS.flatMap((g) => g[2].map((d) => d[0])));
  const lang = (() => { try { return localStorage.getItem("aqua.site.lang") || (navigator.language.toLowerCase().startsWith("ru") ? "ru" : "en"); } catch { return "ru"; } })();
  const L = (ru, en) => (lang === "en" ? en : ru);

  // GitHub's heading slugs (also for Cyrillic): lower case, drop
  // punctuation, spaces → "-", duplicates get "-1", "-2"…
  function slugger() {
    const seen = new Map();
    return (text) => {
      let s = text.trim().toLowerCase().replace(/[^\p{L}\p{N}\- _]/gu, "").replace(/ /g, "-");
      const n = seen.get(s) ?? 0;
      seen.set(s, n + 1);
      return n ? `${s}-${n}` : s;
    };
  }

  /** Where a link written in `srcPath` (a repo path) should point here. */
  function rewrite(href, srcPath) {
    if (/^(https?:|mailto:)/.test(href)) return { url: href, ext: true };
    if (href.startsWith("#")) return { url: href, ext: false };
    const [path, frag] = href.split("#");
    const base = srcPath.split("/").slice(0, -1);
    for (const part of path.split("/")) {
      if (part === "..") base.pop();
      else if (part && part !== ".") base.push(part);
    }
    const full = base.join("/");
    let id = null;
    if (full === "README.md") id = "passport";
    else if (/^docs\/[\w-]+\.md$/.test(full)) id = full.slice(5, -3);
    if (id && IDS.has(id)) return { url: (id === "api" ? "api.html" : `docs.html?d=${id}`) + (frag ? "#" + frag : ""), ext: false };
    const name = full.split("/").pop() ?? "";
    // "crates/aqua-core" is a folder; "LICENSE-MIT" and "x.md" are files.
    const isDir = path.endsWith("/") || (!/\.[\w]+$/.test(name) && !/^[A-Z][A-Z0-9_-]+$/.test(name));
    return { url: `${REPO}/${isDir ? "tree" : "blob"}/main/${full}${frag ? "#" + frag : ""}`, ext: true };
  }

  async function load(id) {
    const file = id === "passport" ? "docs/passport.md" : `docs/${id}.md`;
    const r = await fetch(file, { cache: "no-cache" });
    if (!r.ok) throw new Error(`${file}: ${r.status}`);
    return r.text();
  }

  function render(md, id, into) {
    const srcPath = id === "passport" ? "README.md" : `docs/${id}.md`;
    into.innerHTML = window.marked.parse(md, { gfm: true });
    // The document's own "Contents" list: the left column already is one.
    into.querySelectorAll("h2").forEach((h) => {
      if (!/^(содержание|contents)$/i.test(h.textContent.trim())) return;
      let n = h.nextElementSibling;
      h.remove();
      while (n && n.tagName !== "H2") { const next = n.nextElementSibling; n.remove(); n = next; }
    });
    const slug = slugger();
    const toc = [];
    into.querySelectorAll("h1, h2, h3, h4").forEach((h) => {
      h.id = slug(h.textContent);
      if (h.tagName === "H2" || h.tagName === "H3") {
        toc.push({ id: h.id, text: h.textContent, level: h.tagName === "H2" ? 2 : 3 });
        const a = document.createElement("a");
        a.className = "anchor"; a.href = "#" + h.id; a.textContent = "#"; a.setAttribute("aria-hidden", "true");
        h.appendChild(a);
      }
    });
    into.querySelectorAll("a[href]").forEach((a) => {
      if (a.classList.contains("anchor")) return;
      const r = rewrite(a.getAttribute("href"), srcPath);
      a.setAttribute("href", r.url);
      if (r.ext) { a.target = "_blank"; a.rel = "noopener"; }
    });
    const h1 = into.querySelector("h1");
    if (h1) document.title = `${h1.textContent} — Aquamarine`;
    return toc;
  }

  function sidebar(el, current, filter = "") {
    const q = filter.trim().toLowerCase();
    el.innerHTML = `<input type="search" placeholder="${L("Найти документ…", "Find a document…")}" value="${filter.replace(/"/g, "&quot;")}">` +
      GROUPS.map(([gr, ge, docs]) => {
        const items = docs.filter((d) => !q || (d[1] + " " + d[2] + " " + d[0]).toLowerCase().includes(q));
        if (!items.length) return "";
        return `<h4>${L(gr, ge)}</h4>` + items.map(([id, ru, en]) =>
          `<a href="${id === "api" ? "api.html" : "docs.html?d=" + id}" class="${id === current ? "on" : ""}">${L(ru, en)}</a>`).join("");
      }).join("");
    // The open document's sections, right under its name (not while searching).
    const cur = !q && el.querySelector("a.on");
    if (cur && outlineToc.length) {
      const box = document.createElement("div");
      cur.after(box);
      outline(box, outlineToc);
    }
    const inp = el.querySelector("input");
    inp.addEventListener("input", () => { const pos = inp.selectionStart; sidebar(el, current, inp.value); const n = el.querySelector("input"); n.focus(); n.setSelectionRange(pos, pos); });
  }

  // ---- The page's outline, in the left column (the only one) ----
  // Sections (H2) in a column; the one being read opens to show its
  // subsections (H3), the others stay folded. The lit line is kept in view
  // inside the scrolling column.
  let outlineToc = [];
  function outline(el, toc) {
    outlineToc = toc;
    if (!el) return;
    const clean = (t) => t.replace(/#$/, "");
    const groups = [];
    for (const t of toc) {
      if (t.level === 2 || !groups.length) groups.push({ h: t, subs: [] });
      else groups[groups.length - 1].subs.push(t);
    }
    el.classList.add("doc-outline");
    el.innerHTML = groups.map((g) => `<div class="og" data-h="${g.h.id}"><a href="#${g.h.id}" class="o2">${clean(g.h.text)}</a>` +
      (g.subs.length ? `<div class="o3s"><div>${g.subs.map((s) => `<a href="#${s.id}" class="o3">${clean(s.text)}</a>`).join("")}</div></div>` : "") + "</div>").join("");
    lastSpy = "";
    startSpy();
    spy();
  }

  function follow(panel, link) {
    if (!panel || !link || panel.scrollHeight <= panel.clientHeight) return;
    const top = link.getBoundingClientRect().top - panel.getBoundingClientRect().top + panel.scrollTop, h = link.offsetHeight;
    const view = panel.scrollTop, vh = panel.clientHeight;
    if (top < view + vh * 0.15 || top + h > view + vh * 0.75) {
      panel.scrollTo({ top: Math.max(0, top - vh * 0.3), behavior: "smooth" });
    }
  }

  let lastSpy = "", spyOn = false, pend = 0;
  function spy() {
    pend = 0;
    const box = document.querySelector(".doc-outline");
    if (!box || !outlineToc.length) return;
    let cur = "", h2 = "";
    for (const t of outlineToc) {
      const h = document.getElementById(t.id);
      if (!h || h.getBoundingClientRect().top >= 120) break;
      cur = t.id; if (t.level === 2) h2 = t.id;
    }
    if (!cur) { cur = outlineToc[0].id; h2 = cur; }
    if (cur === lastSpy) return;
    lastSpy = cur;
    box.querySelectorAll(".og").forEach((g) => g.classList.toggle("open", g.dataset.h === h2));
    let on = null;
    box.querySelectorAll("a").forEach((a) => { const hit = a.getAttribute("href") === "#" + cur; a.classList.toggle("on", hit); if (hit) on = a; });
    // Wait for the open/close animation before scrolling the column.
    setTimeout(() => follow(box.closest(".doc-side") || box, on), 320);
  }
  function startSpy() {
    if (spyOn) return;
    spyOn = true;
    window.addEventListener("scroll", () => { if (!pend) pend = requestAnimationFrame(spy); }, { passive: true });
  }

  window.AquaDocs = { load, render, sidebar, outline, lang, L, REPO };

  // docs.html boots itself; api.html calls the pieces it needs.
  document.addEventListener("DOMContentLoaded", async () => {
    const main = document.getElementById("doc");
    if (!main || main.dataset.page === "api") return;
    let id = new URLSearchParams(location.search).get("d") || "README";
    if (!IDS.has(id)) id = "README";
    sidebar(document.getElementById("doc-side"), id);
    if (lang === "en") document.getElementById("doc-note").hidden = false;
    try {
      outlineToc = render(await load(id), id, main);
      sidebar(document.getElementById("doc-side"), id);
      if (location.hash) document.getElementById(decodeURIComponent(location.hash.slice(1)))?.scrollIntoView();
    } catch (e) {
      main.innerHTML = `<p class="loading">${L("Документ не загрузился", "The document didn't load")} (${String(e)}). <a href="${REPO}/tree/main/docs" target="_blank" rel="noopener">GitHub</a></p>`;
    }
  });
})();
