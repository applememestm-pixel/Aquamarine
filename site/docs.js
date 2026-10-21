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
    const inp = el.querySelector("input");
    inp.addEventListener("input", () => { const pos = inp.selectionStart; sidebar(el, current, inp.value); const n = el.querySelector("input"); n.focus(); n.setSelectionRange(pos, pos); });
  }

  function tocList(el, toc) {
    if (!el) return;
    el.innerHTML = toc.length ? `<h4>${L("На этой странице", "On this page")}</h4>` + toc.map((t) => `<a href="#${t.id}" class="l${t.level}">${t.text.replace(/#$/, "")}</a>`).join("") : "";
    const links = [...el.querySelectorAll("a")];
    const heads = toc.map((t) => document.getElementById(t.id)).filter(Boolean);
    const spy = () => {
      let cur = heads[0]?.id;
      for (const h of heads) if (h.getBoundingClientRect().top < 120) cur = h.id;
      links.forEach((a) => a.classList.toggle("on", a.getAttribute("href") === "#" + cur));
    };
    window.addEventListener("scroll", spy, { passive: true });
    spy();
  }

  window.AquaDocs = { load, render, sidebar, tocList, lang, L, REPO };

  // docs.html boots itself; api.html calls the pieces it needs.
  document.addEventListener("DOMContentLoaded", async () => {
    const main = document.getElementById("doc");
    if (!main || main.dataset.page === "api") return;
    let id = new URLSearchParams(location.search).get("d") || "README";
    if (!IDS.has(id)) id = "README";
    sidebar(document.getElementById("doc-side"), id);
    if (lang === "en") document.getElementById("doc-note").hidden = false;
    try {
      const toc = render(await load(id), id, main);
      tocList(document.getElementById("doc-toc"), toc);
      if (location.hash) document.getElementById(decodeURIComponent(location.hash.slice(1)))?.scrollIntoView();
    } catch (e) {
      main.innerHTML = `<p class="loading">${L("Документ не загрузился", "The document didn't load")} (${String(e)}). <a href="${REPO}/tree/main/docs" target="_blank" rel="noopener">GitHub</a></p>`;
    }
  });
})();
