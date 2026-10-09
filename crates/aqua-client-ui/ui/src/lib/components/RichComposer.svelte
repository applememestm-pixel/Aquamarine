<script lang="ts">
  /**
   * The message box. A contenteditable that keeps the same plain-text value
   * the rest of the app works with ("hi [ce:pack/👍_001.tgs]!"), but shows
   * custom emoji as pictures right where the caret is — like Telegram.
   *
   * Parent API (bind:this): focus(), blur(), insert(text).
   */
  import { mediaKind, tgsSnapshot } from "./AnimatedMedia.svelte";

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
  export function insert(str: string) {
    if (!el) { value += str; return; }
    focus();
    const sel = getSelection();
    if (!sel || !sel.rangeCount) return;
    const range = sel.getRangeAt(0);
    range.deleteContents();
    const frag = fragmentOf(str);
    const lastNode = frag.lastChild;
    range.insertNode(frag);
    if (lastNode) { range.setStartAfter(lastNode); range.collapse(true); }
    sel.removeAllRanges();
    sel.addRange(range);
    sync();
    // sync() may have added the tail <br>; keep the caret on the new line.
    if (lastNode && lastNode.parentNode === el) {
      const r = document.createRange();
      r.setStartAfter(lastNode);
      r.collapse(true);
      sel.removeAllRanges();
      sel.addRange(r);
    }
    savedRange = sel.rangeCount ? sel.getRangeAt(0).cloneRange() : null;
  }

  function onKey(e: KeyboardEvent) {
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

<svelte:document onselectionchange={rememberRange} />

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
    font-size: 14.5px;
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
</style>
