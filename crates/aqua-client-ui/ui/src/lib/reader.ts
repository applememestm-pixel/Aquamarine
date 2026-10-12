/**
 * The full-screen reader ("Instant View") for articles and Telegraph pages,
 * plus the one place that decides what clicking a link does.
 */
import { writable } from "svelte/store";
import { api } from "./tauri";
import { telegraphPathOf } from "./markup";

export interface ReaderArticle {
  title: string;
  subtitle?: string | null;
  body: string;
  cover_data_url: string | null;
  images?: string[];
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

/** Telegraph links open in the reader; everything else in the system browser. */
export async function openLink(href: string): Promise<void> {
  const tp = telegraphPathOf(href);
  if (tp) { openTelegraph(tp, href); return; }
  try {
    await api.openExternal(href);
  } catch {
    // Browser preview / mobile: let the webview handle it.
    try { window.open(href, "_blank", "noopener"); } catch { /* ignore */ }
  }
}
