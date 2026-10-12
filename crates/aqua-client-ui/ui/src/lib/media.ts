/**
 * Cheap handles for heavy media.
 *
 * - Article photos travel as data: URLs. Putting a 300 KB string into every
 *   <img src> (and CSS url()) makes the webview re-parse and re-decode it on
 *   each render — that is what made the full-screen reader stutter. They are
 *   turned into blob: URLs once and reused.
 * - Article videos / music are encrypted blobs on the relay; the Rust side
 *   downloads + decrypts them once into a local cache.
 */
import { api, assetUrl } from "./tauri";
import type { ArticleMedia } from "./types";

const blobUrls = new Map<string, string>();
const MAX_BLOBS = 400;

/** data: URL → blob: URL (memoised). Anything else is returned unchanged. */
export function objectUrl(src: string | null | undefined): string {
  if (!src) return "";
  if (!src.startsWith("data:")) return src;
  const hit = blobUrls.get(src);
  if (hit) return hit;
  try {
    const comma = src.indexOf(",");
    const meta = src.slice(5, comma);
    const mime = meta.split(";")[0] || "application/octet-stream";
    let bytes: Uint8Array;
    if (meta.endsWith(";base64")) {
      const bin = atob(src.slice(comma + 1));
      bytes = new Uint8Array(bin.length);
      for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
    } else {
      bytes = new TextEncoder().encode(decodeURIComponent(src.slice(comma + 1)));
    }
    const url = URL.createObjectURL(new Blob([bytes as BlobPart], { type: mime }));
    if (blobUrls.size >= MAX_BLOBS) {
      const [k, v] = blobUrls.entries().next().value as [string, string];
      blobUrls.delete(k);
      URL.revokeObjectURL(v);
    }
    blobUrls.set(src, url);
    return url;
  } catch {
    return src;
  }
}

const fetched = new Map<string, Promise<string>>();

/** Local path of an article video / track (downloaded + decrypted once). */
export function mediaPath(m: ArticleMedia): Promise<string> {
  let p = fetched.get(m.cid);
  if (!p) {
    p = api.fetchAttachment(m);
    fetched.set(m.cid, p);
    p.catch(() => fetched.delete(m.cid));
  }
  return p;
}

/** Playable URL of an article video / track. */
export async function mediaUrl(m: ArticleMedia): Promise<string> {
  return assetUrl(await mediaPath(m));
}

export function fmtDuration(sec: number | null | undefined): string {
  if (!sec || !isFinite(sec)) return "";
  const s = Math.round(sec);
  const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), r = s % 60;
  return h ? `${h}:${String(m).padStart(2, "0")}:${String(r).padStart(2, "0")}` : `${m}:${String(r).padStart(2, "0")}`;
}

export function fmtSize(n: number | null | undefined): string {
  if (!n) return "";
  if (n < 1024 * 1024) return `${Math.max(1, Math.round(n / 1024))} KB`;
  return `${(n / 1024 / 1024).toFixed(n < 100 * 1024 * 1024 ? 1 : 0)} MB`;
}
