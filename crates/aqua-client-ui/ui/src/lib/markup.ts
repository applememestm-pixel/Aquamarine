/**
 * Telegram-style text formatting, shared by chat messages, articles and the
 * Telegraph reader. Messages keep travelling as plain text with markers
 * (what Telegram Desktop also accepts when you type them):
 *
 *   **bold**   __italic__   ++underline++   ~~strike~~   ||spoiler||
 *   `code`     ```block```  > quote         [text](https://link)
 *
 * Articles additionally get headings, lists, dividers, `*italic*` and
 * photo carousels: `[[gallery:0,1,2|caption]]` (indexes into the article's
 * `images`). Everything parses into a small tree rendered by Markup.svelte —
 * never through {@html}, so nobody can inject markup into someone's client.
 */

export type Inline =
  | { t: "text"; v: string }
  | { t: "br" }
  | { t: "emoji"; key: string }
  | { t: "code"; v: string }
  | { t: "b" | "i" | "u" | "s" | "spoiler"; c: Inline[] }
  | { t: "link"; href: string; c: Inline[] }
  | { t: "mention"; v: string }
  | { t: "hashtag"; v: string };

export type Block =
  | { t: "p"; c: Inline[] }
  | { t: "h1" | "h2"; c: Inline[] }
  | { t: "quote"; c: Block[]; pull?: boolean }
  | { t: "pre"; v: string; lang?: string }
  | { t: "ul" | "ol"; items: Inline[][]; start?: number }
  | { t: "hr" }
  /** Photo carousel; `idx` points into the images list given to the renderer. */
  | { t: "gallery"; idx: number[]; caption?: Inline[] }
  /** A video file (Telegraph) — played after it is cached locally. */
  | { t: "video"; url: string; caption?: Inline[] }
  /** Any link shown as a rich preview card (YouTube embeds in Telegraph). */
  | { t: "embed"; url: string; caption?: Inline[] };

export type Mode = "message" | "article";

const PAIRS: [string, "b" | "i" | "u" | "s" | "spoiler"][] = [
  ["**", "b"], ["__", "i"], ["++", "u"], ["~~", "s"], ["||", "spoiler"],
];

function pushText(out: Inline[], s: string) {
  const parts = s.split("\n");
  parts.forEach((p, i) => {
    if (i > 0) out.push({ t: "br" });
    if (!p) return;
    const last = out[out.length - 1];
    if (last && last.t === "text") last.v += p;
    else out.push({ t: "text", v: p });
  });
}

/** Strip trailing punctuation that is almost never part of a pasted URL. */
function trimUrl(u: string): string {
  let s = u.replace(/[.,!?:;»"'…]+$/, "");
  // Keep balanced parentheses (Wikipedia links), drop a dangling ")".
  while (s.endsWith(")") && (s.match(/\(/g)?.length ?? 0) < (s.match(/\)/g)?.length ?? 0)) s = s.slice(0, -1);
  return s;
}

const isSpace = (c: string | undefined) => c === undefined || /\s/.test(c);
const WORD_BEFORE = /[\p{L}\p{N}_]/u;

export function parseInline(s: string, mode: Mode = "message"): Inline[] {
  const out: Inline[] = [];
  let buf = "";
  const flush = () => { if (buf) { pushText(out, buf); buf = ""; } };
  let i = 0;
  while (i < s.length) {
    const ch = s[i];
    const prev = i > 0 ? s[i - 1] : undefined;

    // \** → a literal ** (only where a marker could start, so a shrug
    // ¯\_(ツ)_/¯ or a Windows path keeps its backslash)
    if (ch === "\\" && i + 1 < s.length) {
      const n1 = s[i + 1], n2 = s[i + 2];
      const marker = n1 === "`" || n1 === "[" || n1 === "\\" || n1 === "@" || n1 === "#"
        || ("*_~|+".includes(n1) && n2 === n1) || (mode === "article" && n1 === "*");
      if (marker) { buf += n1; i += 2; continue; }
    }
    // `inline code` (no formatting inside)
    if (ch === "`" && !s.startsWith("```", i)) {
      const j = s.indexOf("`", i + 1);
      if (j > i + 1 && !s.slice(i + 1, j).includes("\n")) {
        flush(); out.push({ t: "code", v: s.slice(i + 1, j) }); i = j + 1; continue;
      }
    }
    if (ch === "[") {
      // [ce:pack/file] custom emoji
      if (s.startsWith("[ce:", i)) {
        const j = s.indexOf("]", i);
        const key = j > 0 ? s.slice(i + 4, j) : "";
        if (key && !/\s/.test(key)) { flush(); out.push({ t: "emoji", key }); i = j + 1; continue; }
      }
      // [text](https://url)
      const m = /^\[([^\]\n]+)\]\(((?:https?:\/\/|mailto:)[^\s)]+)\)/i.exec(s.slice(i));
      if (m) {
        flush(); out.push({ t: "link", href: m[2], c: parseInline(m[1], mode) }); i += m[0].length; continue;
      }
    }
    // Bare links
    if ((ch === "h" || ch === "H" || ch === "w" || ch === "W") && (prev === undefined || !WORD_BEFORE.test(prev))) {
      const m = /^(?:https?:\/\/|www\.)[^\s<>]+/i.exec(s.slice(i));
      if (m) {
        const url = trimUrl(m[0]);
        if (url.length > 4) {
          flush();
          out.push({ t: "link", href: /^www\./i.test(url) ? "https://" + url : url, c: [{ t: "text", v: url }] });
          i += url.length; continue;
        }
      }
    }
    // @mentions and #hashtags
    if ((ch === "@" || ch === "#") && (prev === undefined || !WORD_BEFORE.test(prev))) {
      const m = ch === "@" ? /^@([A-Za-z0-9_]{3,32})/.exec(s.slice(i)) : /^#([\p{L}\p{N}_]{2,64})/u.exec(s.slice(i));
      if (m && !(ch === "#" && /^\d+$/.test(m[1]))) {
        flush(); out.push(ch === "@" ? { t: "mention", v: m[1] } : { t: "hashtag", v: m[1] }); i += m[0].length; continue;
      }
    }
    // Paired markers
    let done = false;
    for (const [d, t] of PAIRS) {
      if (!s.startsWith(d, i)) continue;
      const from = i + d.length;
      if (isSpace(s[from])) break;
      let j = s.indexOf(d, from + 1);
      // `****` or a closing marker preceded by a space doesn't close.
      while (j > 0 && isSpace(s[j - 1])) j = s.indexOf(d, j + 1);
      if (j > from) {
        flush(); out.push({ t, c: parseInline(s.slice(from, j), mode) }); i = j + d.length; done = true;
      }
      break;
    }
    if (done) continue;
    // Articles: *italic* (Markdown habit)
    if (mode === "article" && ch === "*" && s[i + 1] !== "*" && !isSpace(s[i + 1])) {
      const j = s.indexOf("*", i + 1);
      if (j > i + 1 && s[j + 1] !== "*" && !isSpace(s[j - 1]) && !s.slice(i + 1, j).includes("\n")) {
        flush(); out.push({ t: "i", c: parseInline(s.slice(i + 1, j), mode) }); i = j + 1; continue;
      }
    }
    buf += ch;
    i++;
  }
  flush();
  return out;
}

const GALLERY = /^\[\[gallery:([\d,\s]+)(?:\|(.*))?\]\]$/;

export function parseBlocks(src: string, mode: Mode = "message"): Block[] {
  const lines = src.replace(/\r\n?/g, "\n").split("\n");
  const out: Block[] = [];
  let para: string[] = [];
  const flushPara = () => {
    if (para.length) {
      // Messages keep blank lines as-is; articles split paragraphs on them.
      const text = para.join("\n");
      if (text.trim() || mode === "message") out.push({ t: "p", c: parseInline(mode === "message" ? text : text.trim(), mode) });
      para = [];
    }
  };
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    const trimmed = line.trim();

    // ```lang … ``` (also ```one line```)
    if (trimmed.startsWith("```")) {
      const one = /^```([^`]*)```$/.exec(trimmed);
      if (one && trimmed.length > 6) { flushPara(); out.push({ t: "pre", v: one[1] }); i++; continue; }
      const lang = trimmed.slice(3).trim();
      const body: string[] = [];
      let j = i + 1;
      while (j < lines.length && !lines[j].trim().startsWith("```")) body.push(lines[j++]);
      if (j < lines.length || body.length) {
        // Text after the closing ``` on the same line stays text.
        flushPara();
        out.push({ t: "pre", v: body.join("\n"), lang: lang && /^[\w+#.-]{1,20}$/.test(lang) ? lang : undefined });
        if (j < lines.length) {
          const tail = lines[j].trim().slice(3);
          if (tail) para.push(tail);
        }
        i = j + 1; continue;
      }
    }
    // > quote (consecutive lines)
    if (/^>(\s|$)/.test(line) || (line.startsWith(">") && !line.startsWith(">>"))) {
      flushPara();
      const body: string[] = [];
      while (i < lines.length && lines[i].startsWith(">") && !lines[i].startsWith(">>")) body.push(lines[i++].replace(/^>\s?/, ""));
      out.push({ t: "quote", c: parseBlocks(body.join("\n"), mode) });
      continue;
    }
    if (mode === "article") {
      // >> pull quote (Telegraph "aside")
      if (line.startsWith(">>")) {
        flushPara();
        const body: string[] = [];
        while (i < lines.length && lines[i].startsWith(">>")) body.push(lines[i++].replace(/^>>\s?/, ""));
        out.push({ t: "quote", pull: true, c: parseBlocks(body.join("\n"), mode) });
        continue;
      }
      const g = GALLERY.exec(trimmed);
      if (g) {
        flushPara();
        const idx = g[1].split(",").map((x) => parseInt(x.trim(), 10)).filter((n) => Number.isFinite(n) && n >= 0);
        if (idx.length) out.push({ t: "gallery", idx, caption: g[2]?.trim() ? parseInline(g[2].trim(), mode) : undefined });
        i++; continue;
      }
      const h = /^(#{1,3})\s+(.+)$/.exec(trimmed);
      if (h) { flushPara(); out.push({ t: h[1].length === 1 ? "h1" : "h2", c: parseInline(h[2], mode) }); i++; continue; }
      if (/^(-{3,}|\*{3,}|_{3,})$/.test(trimmed)) { flushPara(); out.push({ t: "hr" }); i++; continue; }
      if (/^[-*•]\s+/.test(trimmed)) {
        flushPara();
        const items: Inline[][] = [];
        while (i < lines.length && /^[-*•]\s+/.test(lines[i].trim())) items.push(parseInline(lines[i++].trim().replace(/^[-*•]\s+/, ""), mode));
        out.push({ t: "ul", items });
        continue;
      }
      const ol = /^(\d{1,4})[.)]\s+/.exec(trimmed);
      if (ol) {
        flushPara();
        const items: Inline[][] = [];
        const start = parseInt(ol[1], 10);
        while (i < lines.length && /^\d{1,4}[.)]\s+/.test(lines[i].trim())) items.push(parseInline(lines[i++].trim().replace(/^\d{1,4}[.)]\s+/, ""), mode));
        out.push({ t: "ol", items, start: start !== 1 ? start : undefined });
        continue;
      }
      if (trimmed === "") { flushPara(); i++; continue; }
    }
    para.push(line);
    i++;
  }
  flushPara();
  // A message that is only whitespace around a block shouldn't leave empty paragraphs.
  return mode === "message" ? out.filter((b, k) => !(b.t === "p" && b.c.length === 0 && (k === 0 || k === out.length - 1))) : out;
}

// ------------------------------------------------------------------ helpers

function inlineText(c: Inline[]): string {
  return c.map((x) => {
    switch (x.t) {
      case "text": return x.v;
      case "br": return "\n";
      case "emoji": { const f = x.key.split("/").pop() ?? ""; const k = f.indexOf("_"); return k > 0 ? f.slice(0, k) : ""; }
      case "code": return x.v;
      case "mention": return "@" + x.v;
      case "hashtag": return "#" + x.v;
      case "spoiler": return "▒▒▒";
      default: return inlineText(x.c);
    }
  }).join("");
}

/** Plain text without markers — for chat-list previews, excerpts, search. */
export function plainText(src: string, mode: Mode = "message"): string {
  return parseBlocks(src, mode).map((b) => {
    switch (b.t) {
      case "p": case "h1": case "h2": return inlineText(b.c);
      case "quote": return b.c.map((x) => (x.t === "p" ? inlineText(x.c) : "")).join("\n");
      case "pre": return b.v;
      case "ul": case "ol": return b.items.map(inlineText).join("\n");
      case "gallery": return b.caption ? inlineText(b.caption) : "";
      default: return "";
    }
  }).filter(Boolean).join("\n");
}

/** True when the text uses any formatting marker (cheap pre-check). */
export function hasMarkup(s: string): boolean {
  return /\*\*|__|\+\+|~~|\|\||`|^>|\[[^\]]+\]\(|https?:\/\/|www\.|@\w{3}|#\p{L}/mu.test(s);
}

/** All links in a text, in order (for link previews). */
export function linksIn(src: string): string[] {
  const urls: string[] = [];
  const walk = (c: Inline[]) => {
    for (const x of c) {
      if (x.t === "link") { if (/^https?:/i.test(x.href)) urls.push(x.href); }
      else if ("c" in x && Array.isArray(x.c)) walk(x.c as Inline[]);
    }
  };
  const blocks = (bs: Block[]) => {
    for (const b of bs) {
      if (b.t === "p" || b.t === "h1" || b.t === "h2") walk(b.c);
      else if (b.t === "quote") blocks(b.c);
      else if (b.t === "ul" || b.t === "ol") b.items.forEach(walk);
    }
  };
  blocks(parseBlocks(src, "message"));
  return urls;
}

export function telegraphPathOf(href: string): string | null {
  try {
    const u = new URL(href);
    const host = u.hostname.replace(/^www\./, "");
    if (!["telegra.ph", "graph.org", "te.legra.ph"].includes(host)) return null;
    const p = u.pathname.split("/").filter(Boolean)[0] ?? "";
    return p && p !== "file" ? decodeURIComponent(p) : null;
  } catch { return null; }
}

// ------------------------------------------------------------------ Telegraph

/** Telegraph API content node. */
export type TgNode = string | { tag: string; attrs?: Record<string, string>; children?: TgNode[] };

function tgUrl(src: string): string {
  if (!src) return src;
  if (src.startsWith("//")) return "https:" + src;
  if (src.startsWith("/")) return "https://telegra.ph" + src;
  return src;
}

function tgInline(nodes: TgNode[] | undefined): Inline[] {
  const out: Inline[] = [];
  for (const n of nodes ?? []) {
    if (typeof n === "string") { pushText(out, n); continue; }
    const kids = () => tgInline(n.children);
    switch (n.tag) {
      case "br": out.push({ t: "br" }); break;
      case "strong": case "b": case "mark": out.push({ t: "b", c: kids() }); break;
      case "em": case "i": out.push({ t: "i", c: kids() }); break;
      case "u": out.push({ t: "u", c: kids() }); break;
      case "s": out.push({ t: "s", c: kids() }); break;
      case "code": out.push({ t: "code", v: inlineText(kids()) }); break;
      case "a": {
        const href = tgUrl(n.attrs?.href ?? "");
        if (/^(https?:|mailto:)/i.test(href)) out.push({ t: "link", href, c: kids() });
        else out.push(...kids());
        break;
      }
      default: out.push(...kids());
    }
  }
  return out;
}

/**
 * Telegraph page content → our blocks. Image URLs are appended to `images`
 * (the gallery blocks index into it); runs of image-only figures merge into
 * one carousel.
 */
export function telegraphToBlocks(nodes: TgNode[], images: string[]): Block[] {
  const out: Block[] = [];
  const caption = (n: TgNode & object): Inline[] | undefined => {
    const fc = (n.children ?? []).find((c) => typeof c !== "string" && c.tag === "figcaption");
    const c = fc && typeof fc !== "string" ? tgInline(fc.children) : [];
    return inlineText(c).trim() ? c : undefined;
  };
  const pushImage = (src: string, cap?: Inline[]) => {
    const idx = images.push(tgUrl(src)) - 1;
    const last = out[out.length - 1];
    if (last && last.t === "gallery" && !last.caption && !cap) last.idx.push(idx);
    else out.push({ t: "gallery", idx: [idx], caption: cap });
  };
  for (const n of nodes) {
    if (typeof n === "string") { if (n.trim()) out.push({ t: "p", c: tgInline([n]) }); continue; }
    switch (n.tag) {
      case "p": {
        // A paragraph holding only an image is how older pages embed photos.
        const only = (n.children ?? []).filter((c) => typeof c !== "string" || c.trim());
        if (only.length === 1 && typeof only[0] !== "string" && only[0].tag === "img") { pushImage(only[0].attrs?.src ?? ""); break; }
        out.push({ t: "p", c: tgInline(n.children) });
        break;
      }
      case "h3": case "h1": case "h2": out.push({ t: "h1", c: tgInline(n.children) }); break;
      case "h4": case "h5": case "h6": out.push({ t: "h2", c: tgInline(n.children) }); break;
      case "blockquote": out.push({ t: "quote", c: [{ t: "p", c: tgInline(n.children) }] }); break;
      case "aside": out.push({ t: "quote", pull: true, c: [{ t: "p", c: tgInline(n.children) }] }); break;
      case "ul": case "ol": {
        const items = (n.children ?? []).filter((c) => typeof c !== "string" && c.tag === "li").map((li) => tgInline((li as { children?: TgNode[] }).children));
        out.push({ t: n.tag, items });
        break;
      }
      case "pre": out.push({ t: "pre", v: inlineText(tgInline(n.children)) }); break;
      case "hr": out.push({ t: "hr" }); break;
      case "img": pushImage(n.attrs?.src ?? ""); break;
      case "figure": {
        const media = (n.children ?? []).flatMap((c) => (typeof c === "string" ? [] : c.tag === "div" ? (c.children ?? []) : [c]))
          .find((c) => typeof c !== "string" && ["img", "video", "iframe"].includes(c.tag));
        if (!media || typeof media === "string") break;
        const cap = caption(n);
        const src = media.attrs?.src ?? "";
        if (media.tag === "img") pushImage(src, cap);
        else if (media.tag === "video") out.push({ t: "video", url: tgUrl(src), caption: cap });
        else {
          // /embed/youtube?url=… → the original link
          let url = tgUrl(src);
          try { const q = new URL(url).searchParams.get("url"); if (q) url = q; } catch { /* keep */ }
          out.push({ t: "embed", url, caption: cap });
        }
        break;
      }
      default:
        if (n.children?.length) out.push({ t: "p", c: tgInline(n.children) });
    }
  }
  return out.filter((b) => !(b.t === "p" && !inlineText(b.c).trim()));
}

export function wordCount(blocks: Block[]): number {
  const text = blocks.map((b) => {
    if (b.t === "p" || b.t === "h1" || b.t === "h2") return inlineText(b.c);
    if (b.t === "quote") return b.c.map((x) => (x.t === "p" ? inlineText(x.c) : "")).join(" ");
    if (b.t === "ul" || b.t === "ol") return b.items.map(inlineText).join(" ");
    if (b.t === "pre") return b.v;
    return "";
  }).join(" ");
  return text.split(/\s+/).filter(Boolean).length;
}

/** Article teaser: the paragraphs' text (no headings, captions or code). */
export function excerptOf(src: string, max = 260): string {
  const parts: string[] = [];
  let len = 0;
  for (const b of parseBlocks(src, "article")) {
    if (b.t !== "p" && b.t !== "quote") continue;
    const s = (b.t === "p" ? inlineText(b.c) : b.c.map((x) => (x.t === "p" ? inlineText(x.c) : "")).join(" ")).replace(/\s+/g, " ").trim();
    if (!s) continue;
    parts.push(s);
    len += s.length;
    if (len > max) break;
  }
  const out = parts.join(" ");
  return out.length > max ? out.slice(0, max).trimEnd() + "…" : out;
}
