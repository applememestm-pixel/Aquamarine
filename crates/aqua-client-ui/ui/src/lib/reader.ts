/**
 * The full-screen reader ("Instant View") for articles and Telegraph pages,
 * plus the one place that decides what clicking a link does.
 */
import { writable } from "svelte/store";
import { api } from "./tauri";
import { telegraphPathOf } from "./markup";
import { inAppBrowser } from "./browser";
import { IS_DESKTOP } from "./platform";
import { get } from "svelte/store";

export interface ReaderArticle {
  title: string;
  subtitle?: string | null;
  body: string;
  cover_data_url: string | null;
  images?: string[];
  media?: import("./types").ArticleMedia[];
}

export type ReaderState =
  | { kind: "article"; article: ReaderArticle; author?: string; time?: string }
  | { kind: "telegraph"; path: string; url: string };

export const reader = writable<ReaderState | null>(null);

export function openArticleReader(article: ReaderArticle, meta: { author?: string; time?: string } = {}) {
  reader.set({ kind: "article", article, ...meta });
}
export function openTelegraph(path: string, url?: string) {
  reader.set({ kind: "telegraph", path, url: url ?? `https://telegra.ph/${path}` });
}
export function closeReader() {
  reader.set(null);
}

/** Telegraph links open in the reader; web links in the built-in browser
 *  (computers, when on), everything else in the system browser. */
export async function openLink(href: string): Promise<void> {
  const tp = telegraphPathOf(href);
  if (tp) { openTelegraph(tp, href); return; }
  if (IS_DESKTOP && get(inAppBrowser) && /^https?:\/\//i.test(href)) {
    try { await api.openBrowserWindow(href); return; } catch { /* fall back to the system browser */ }
  }
  try {
    await api.openExternal(href);
  } catch {
    // Browser preview / mobile: let the webview handle it.
    try { window.open(href, "_blank", "noopener"); } catch { /* ignore */ }
  }
}
