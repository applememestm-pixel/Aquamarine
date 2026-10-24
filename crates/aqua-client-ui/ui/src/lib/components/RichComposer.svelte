<script lang="ts">
  import { formattingAction } from "../keybindings";
  /**
   * The message box. A contenteditable that keeps the same plain-text value
   * the rest of the app works with ("hi [ce:pack/👍_001.tgs]!"), but shows
   * custom emoji as pictures right where the caret is — like Telegram.
   *
   * Parent API (bind:this): focus(), blur(), insert(text).
   */
  import { mediaKind, tgsSnapshot } from "./AnimatedMedia.svelte";
  import { askInput } from "./InputPrompt.svelte";
  import { tr } from "../i18n";

  interface Props {
    value: string;
    placeholder?: string;
    /** Resolve "pack/file" → media, for showing custom emoji. */
    resolve: (key: string) => { url: string; mime: string } | undefined;
    onkeydown?: (e: KeyboardEvent) => void;
    oninput?: () => void;
    class?: string;
  }
  let { value = $bindable(""), placeholder = "", resolve, onkeydown, oninput, class: klass = "" }: Props = $props();

  let el: HTMLDivElement | undefined = $state();
  let lastValue = "";
  let savedRange: Range | null = null;

  const TOKEN = /\[ce:([^\]\s]+)\]/g;

  function fallbackChar(key: string): string {
    const f = key.split("/").pop() ?? "";
    const i = f.indexOf("_");
    return i > 0 ? f.slice(0, i) : "▫";
  }

  function emojiNode(token: string, key: string): HTMLElement {
    const span = document.createElement("span");
    span.className = "ce";
    span.contentEditable = "false";
    span.dataset.token = token;
    const media = resolve(key);
    const kind = media ? mediaKind(media.url, media.mime) : null;
    if (media && kind === "image") {
      const img = document.createElement("img");
      img.src = media.url; img.alt = fallbackChar(key); img.draggable = false;
      span.appendChild(img);
    } else if (media && kind === "video") {
      const v = document.createElement("video");
      v.src = media.url; v.muted = true; v.loop = true; v.autoplay = true; v.playsInline = true;
      span.appendChild(v);
    } else if (media && kind === "tgs") {
      const img = document.createElement("img");
      img.alt = fallbackChar(key); img.draggable = false;
      span.appendChild(img);
      void tgsSnapshot(media.url).then((d) => { img.src = d; }).catch(() => { span.textContent = fallbackChar(key); });
    } else {
      span.textContent = fallbackChar(key);
    }
    return span;
  }

  function fragmentOf(str: string): DocumentFragment {
    const frag = document.createDocumentFragment();
    const pushText = (t: string) => {
      const lines = t.split("\n");
      lines.forEach((ln, i) => {
        if (i > 0) frag.appendChild(document.createElement("br"));
        if (ln) frag.appendChild(document.createTextNode(ln));
      });
    };
    let last = 0;
    for (const m of str.matchAll(TOKEN)) {
      pushText(str.slice(last, m.index));
      frag.appendChild(emojiNode(m[0], m[1]));
      last = (m.index ?? 0) + m[0].length;
    }
    pushText(str.slice(last));
    return frag;
  }

  function serialize(root: Node): string {
    let out = "";
    root.childNodes.forEach((n) => {
      if (n.nodeType === Node.TEXT_NODE) {
        out += (n.nodeValue ?? "").replace(/ /g, " ").replace(/​/g, "");
      } else if (n instanceof HTMLElement) {
        if (n.dataset.token) out += n.dataset.token;
        else if (n.tagName === "BR") { if (!n.hasAttribute("data-tail")) out += "\n"; }
        else if (n.tagName === "DIV" || n.tagName === "P") {
          if (out && !out.endsWith("\n")) out += "\n";
          out += serialize(n);
        } else out += serialize(n);
      }
    });
    return out;
  }

  /** A trailing line break needs one extra <br> to be visible. */
  function fixTail() {
    if (!el) return;
    el.querySelectorAll("br[data-tail]").forEach((b) => b.remove());
    // Splitting a text node at its end leaves an empty one behind; drop it
    // so a trailing <br> is really last.
    while (el.lastChild && el.lastChild.nodeType === Node.TEXT_NODE && !el.lastChild.nodeValue) el.lastChild.remove();
    const last = el.lastChild;
    if (last instanceof HTMLBRElement) {
      const tail = document.createElement("br");
      tail.setAttribute("data-tail", "");
      el.appendChild(tail);
    }
  }

  function sync() {
    if (!el) return;
    let v = serialize(el);
    if (v === "\n" && el.childNodes.length <= 2 && !el.querySelector("[data-token]")) { el.replaceChildren(); v = ""; }
    fixTail();
    lastValue = v;
    if (v !== value) value = v;
    rememberRange();
    oninput?.();
  }

  // External value changes (send clears it, drafts, edit-message) re-render.
  $effect(() => {
    const v = value;
    if (!el || v === lastValue) return;
    lastValue = v;
    el.replaceChildren(fragmentOf(v));
    fixTail();
    if (document.activeElement === el) caretToEnd();
  });

  function caretToEnd() {
    if (!el) return;
    const r = document.createRange();
    r.selectNodeContents(el);
    const tail = el.querySelector("br[data-tail]");
    if (tail) r.setStartBefore(tail);
    r.collapse(tail ? true : false);
    const sel = getSelection();
    sel?.removeAllRanges();
    sel?.addRange(r);
    savedRange = r.cloneRange();
  }

  function rememberRange() {
    const sel = getSelection();
    if (el && sel && sel.rangeCount && el.contains(sel.anchorNode)) savedRange = sel.getRangeAt(0).cloneRange();
  }

  export function focus() {
    if (!el) return;
    if (document.activeElement === el) return;
    // Focusing a contenteditable puts the caret at the start; put it back
    // where the user left it (or at the end).
    const keep = savedRange;
    el.focus();
    const sel = getSelection();
    if (keep && el.contains(keep.startContainer)) { sel?.removeAllRanges(); sel?.addRange(keep); }
    else caretToEnd();
  }
  export function blur() { el?.blur(); }

  /** Insert text (with optional [ce:] tokens) at the caret. */
  /** Touch screens: picking an emoji must not focus the field (the
   *  on-screen keyboard would jump up over the emoji panel). */
  const coarse = typeof matchMedia === "function" && matchMedia("(pointer: coarse)").matches;
  function endRange(): Range {
    const r = document.createRange();
    r.selectNodeContents(el!);
    const last = el!.lastChild;
    if (last && last.nodeName === "BR") { r.setStartBefore(last); r.setEndBefore(last); }
    r.collapse(false);
    return r;
  }
  export function insert(str: string) {
    if (!el) { value += str; return; }
    const quiet = coarse && document.activeElement !== el;
    let range: Range;
    const sel = getSelection();
    if (quiet) {
      range = savedRange && el.contains(savedRange.startContainer) ? savedRange.cloneRange() : endRange();
    } else {
      focus();
      if (!sel || !sel.rangeCount) return;
      range = sel.getRangeAt(0);
    }
    range.deleteContents();
    const frag = fragmentOf(str);
    const lastNode = frag.lastChild;
    range.insertNode(frag);
    if (lastNode) { range.setStartAfter(lastNode); range.collapse(true); }
    if (quiet) {
      sync();
      savedRange = lastNode && lastNode.parentNode ? (() => { const r = document.createRange(); r.setStartAfter(lastNode); r.collapse(true); return r; })() : range.cloneRange();
      return;
    }
    sel!.removeAllRanges();
    sel!.addRange(range);
    sync();
    // sync() may have added the tail <br>; keep the caret on the new line.
    if (lastNode && lastNode.parentNode === el) {
      const r = document.createRange();
      r.setStartAfter(lastNode);
      r.collapse(true);
      sel!.removeAllRanges();
      sel!.addRange(r);
    }
    savedRange = sel!.rangeCount ? sel!.getRangeAt(0).cloneRange() : null;
  }

  // ---- Telegram-style formatting of the selected text -------------------
  type Fmt = "b" | "i" | "u" | "s" | "spoiler" | "code" | "link" | "quote" | "clear";
  const MARK: Record<string, [string, string]> = {
    b: ["**", "**"], i: ["__", "__"], u: ["++", "++"], s: ["~~", "~~"], spoiler: ["||", "||"], code: ["`", "`"],
  };
  let fmtBar = $state<{ x: number; y: number; below: boolean } | null>(null);
  /** Mount on <body>: an ancestor with backdrop-filter would otherwise
   *  become the containing block of a position:fixed popup. */
  function portal(node: HTMLElement) {
    document.body.appendChild(node);
    return { destroy() { node.remove(); } };
  }

  function selectionInside(): Range | null {
    const sel = getSelection();
    if (!el || !sel || !sel.rangeCount || sel.isCollapsed) return null;
    const r = sel.getRangeAt(0);
    return el.contains(r.commonAncestorContainer) ? r : null;
  }
  function selectedText(r: Range): string {
    const holder = document.createElement("div");
    holder.appendChild(r.cloneContents());
    return serialize(holder);
  }
  function placeBar() {
    const r = selectionInside();
    if (!r || document.activeElement !== el) { fmtBar = null; return; }
    const rect = r.getBoundingClientRect();
    if (!rect.width && !rect.height) { fmtBar = null; return; }
    const below = rect.top < 60;
    fmtBar = { x: Math.max(150, Math.min(window.innerWidth - 150, rect.left + rect.width / 2)), y: below ? rect.bottom + 8 : rect.top - 8, below };
  }

  /** Wrap / unwrap the selection with Telegram markers (or prefix lines). */
  export function format(kind: Fmt) {
    const r = selectionInside() ?? (savedRange && el?.contains(savedRange.commonAncestorContainer) && !savedRange.collapsed ? savedRange : null);
    if (!r) return;
    const text = selectedText(r);
    const apply = (out: string) => {
      el?.focus();
      const sel = getSelection();
      sel?.removeAllRanges();
      sel?.addRange(r);
      insert(out);
      fmtBar = null;
    };
    if (kind === "link") {
      const saved = r.cloneRange();
      askInput({
        title: tr("Add a link"),
        fields: [{ label: "URL", placeholder: "https://", monospace: true }],
        submitLabel: tr("Insert"),
        onSubmit: async ([url]) => {
          let u = url.trim();
          if (!u) return tr("Enter a link");
          if (!/^(https?:|mailto:)/i.test(u)) u = "https://" + u;
          el?.focus();
          const sel = getSelection();
          sel?.removeAllRanges();
          sel?.addRange(saved);
          insert(`[${text.replace(/[\[\]]/g, "")}](${u})`);
          return null;
        },
      });
      return;
    }
    if (kind === "clear") {
      apply(text.replace(/\*\*|__|\+\+|~~|\|\||`/g, "").replace(/\[([^\]]+)\]\([^)]+\)/g, "$1").replace(/^>\s?/gm, ""));
      return;
    }
    if (kind === "quote") {
      const all = text.split("\n").every((l) => /^>\s?/.test(l));
      apply(all ? text.replace(/^>\s?/gm, "") : text.split("\n").map((l) => "> " + l).join("\n"));
      return;
    }
    const [a, b] = MARK[kind];
    // Toggle: already wrapped → unwrap.
    if (text.length >= a.length + b.length && text.startsWith(a) && text.endsWith(b)) apply(text.slice(a.length, text.length - b.length));
    else {
      // Keep spaces outside the markers ("word " → "**word** ").
      const lead = text.match(/^\s*/)?.[0] ?? "";
      const trail = text.match(/\s*$/)?.[0] ?? "";
      const core = text.slice(lead.length, text.length - trail.length);
      if (!core) return;
      apply(lead + a + core + b + trail);
    }
  }

  function onKey(e: KeyboardEvent) {
    {
      const FMT: Record<string, Fmt> = {
        fmtBold: "b", fmtItalic: "i", fmtUnderline: "u", fmtLink: "link",
        fmtStrike: "s", fmtSpoiler: "spoiler", fmtMono: "code", fmtClear: "clear", fmtQuote: "quote",
      };
      const act = formattingAction(e);
      const k = act ? FMT[act] : undefined;
      // Only intercept "no selection" for Ctrl combos (the browser's own
      // rich-text commands); a rebound plain key just types when nothing's selected.
      if (k && !selectionInside() && !(e.ctrlKey || e.metaKey)) { /* fall through */ } else if (k) {
        // No selection: Ctrl+B / I / U must not fall through to the
        // browser's own rich-text commands (they'd add <b> tags).
        e.preventDefault();
        if (selectionInside()) { e.stopPropagation(); format(k); }
        return;
      }
    }
    onkeydown?.(e);
    if (e.defaultPrevented) return;
    if (e.key === "Enter" && !e.isComposing) {
      e.preventDefault();
      insert("\n");
    }
  }

  function onPaste(e: ClipboardEvent) {
    const t = e.clipboardData?.getData("text/plain");
    if (t == null) return;
    e.preventDefault();
    insert(t.replace(/\r\n?/g, "\n"));
  }

  function onCopy(e: ClipboardEvent) {
    // Copy as our plain-text format so emoji survive copy → paste.
    const sel = getSelection();
    if (!sel || !sel.rangeCount || !el) return;
    const holder = document.createElement("div");
    holder.appendChild(sel.getRangeAt(0).cloneContents());
    e.clipboardData?.setData("text/plain", serialize(holder));
    e.preventDefault();
  }
  function onCut(e: ClipboardEvent) {
    onCopy(e);
    getSelection()?.getRangeAt(0).deleteContents();
    sync();
  }
</script>

<svelte:document onselectionchange={() => { rememberRange(); placeBar(); }} />
<svelte:window onscroll={() => (fmtBar = null)} onresize={() => (fmtBar = null)} />

{#if fmtBar}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div use:portal class="fmt-bar" class:below={fmtBar.below} style={`left:${fmtBar.x}px; top:${fmtBar.y}px;`} onmousedown={(e) => e.preventDefault()}>
    <button title={tr("Bold") + " (Ctrl+B)"} onclick={() => format("b")}><b>B</b></button>
    <button title={tr("Italic") + " (Ctrl+I)"} onclick={() => format("i")}><i>I</i></button>
    <button title={tr("Underline") + " (Ctrl+U)"} onclick={() => format("u")}><u>U</u></button>
    <button title={tr("Strikethrough") + " (Ctrl+Shift+X)"} onclick={() => format("s")}><s>S</s></button>
    <button title={tr("Spoiler") + " (Ctrl+Shift+P)"} onclick={() => format("spoiler")} class="sp">▒</button>
    <button title={tr("Monospace") + " (Ctrl+Shift+M)"} onclick={() => format("code")} class="mono">&lt;/&gt;</button>
    <button title={tr("Quote") + " (Ctrl+Shift+.)"} onclick={() => format("quote")}>❝</button>
    <button title={tr("Link") + " (Ctrl+K)"} onclick={() => format("link")}>🔗</button>
    <span class="fsep"></span>
    <button title={tr("Clear formatting") + " (Ctrl+Shift+N)"} onclick={() => format("clear")}>⌫</button>
  </div>
{/if}

<div
  bind:this={el}
  class="rich {klass}"
  class:empty={value === ""}
  contenteditable="true"
  role="textbox"
  aria-multiline="true"
  tabindex="0"
  spellcheck="true"
  data-placeholder={placeholder}
  oninput={sync}
  onkeydown={onKey}
  onkeyup={rememberRange}
  onmouseup={rememberRange}
  onblur={rememberRange}
  onpaste={onPaste}
  oncopy={onCopy}
  oncut={onCut}
  ondrop={(e) => e.preventDefault()}
></div>

<style>
  .rich {
    flex: 1;
    min-width: 0;
    min-height: 32px;
    max-height: 160px;
    overflow-y: auto;
    box-sizing: border-box;
    padding: 6px 4px;
    font-family: inherit;
    font-size: calc(14.5px * var(--font-scale, 1));
    line-height: 1.35;
    color: var(--fg);
    outline: none;
    white-space: pre-wrap;
    word-break: break-word;
    cursor: text;
  }
  .rich.empty::before {
    content: attr(data-placeholder);
    color: var(--fg-dim);
    pointer-events: none;
    position: absolute;
  }
  .rich :global(.ce) {
    display: inline-block;
    width: 1.35em; height: 1.35em;
    vertical-align: -0.3em;
    margin: 0 1px;
    user-select: all;
  }
  .rich :global(.ce img), .rich :global(.ce video) { width: 100%; height: 100%; object-fit: contain; display: block; }
  .fmt-bar {
    position: fixed; z-index: 260;
    transform: translate(-50%, -100%);
    display: flex; align-items: center; gap: 2px;
    padding: 4px;
    background: var(--bg-surface-2, var(--bg-list));
    border: 1px solid var(--border-strong);
    border-radius: 10px;
    box-shadow: 0 10px 28px -8px rgba(0, 0, 0, 0.55);
    animation: fmt-in 0.12s ease-out;
  }
  .fmt-bar.below { transform: translate(-50%, 0); }
  @keyframes fmt-in { from { opacity: 0; margin-top: 4px; } to { opacity: 1; margin-top: 0; } }
  .fmt-bar button {
    min-width: 28px; height: 28px; padding: 0 6px;
    border: none; border-radius: 7px; background: transparent;
    color: var(--fg); font-size: 13.5px; cursor: pointer;
  }
  .fmt-bar button:hover { background: var(--bg-hover); color: var(--accent); }
  .fmt-bar .mono { font-family: var(--font-mono); font-size: 11.5px; }
  .fmt-bar .sp { color: var(--fg-dim); }
  .fsep { width: 1px; height: 18px; background: var(--border-strong); margin: 0 2px; }
</style>
