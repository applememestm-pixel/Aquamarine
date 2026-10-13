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
import { tr } from "./i18n";
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

const streams = new Map<string, Promise<string>>();

let support: Promise<{ blocking: string[]; recommended: string[] }> | null = null;
/** Missing GStreamer packages (Linux), checked once. */
export function mediaSupport(): Promise<{ blocking: string[]; recommended: string[] }> {
  support ??= api.mediaSupport().then((r) => ({ blocking: r?.blocking ?? [], recommended: r?.recommended ?? [] })).catch(() => ({ blocking: [], recommended: [] }));
  return support;
}

/** Install hint for missing packages. */
export function installHint(pkgs: string[]): string {
  return `sudo pacman -S --needed ${pkgs.join(" ")}`;
}

/**
 * Throws when playing would hang the app: WebKitGTK without
 * gst-plugins-good (no audio sink / MP4 demuxer) locks up the whole window
 * on the first <audio>/<video>.
 */
async function assertPlayable(): Promise<void> {
  const s = await mediaSupport();
  if (s.blocking.length) {
    throw new Error(tr("Sound and video need GStreamer plugins. Install: {cmd}", { cmd: installHint([...s.blocking, ...s.recommended]) }));
  }
}

/**
 * Loopback-HTTP URL for playing a local video / audio file. Tauri's asset
 * protocol loads a whole file into memory per request (and on Linux pushes
 * it through the UI thread), which froze the app on big videos; the Rust
 * media server streams with real range requests instead. Falls back to the
 * asset URL if the server can't be used.
 */
export function streamUrl(path: string): Promise<string> {
  let p = streams.get(path);
  if (!p) {
    p = assertPlayable().then(() => api.mediaStreamUrl(path).catch(() => assetUrl(path)));
    streams.set(path, p);
    p.catch(() => streams.delete(path));
  }
  return p;
}

const ASSET_URL = /^(?:asset:\/\/localhost\/|https?:\/\/asset\.localhost\/)(.+)$/;

/** Turn an asset: URL of a video / audio file into a streaming URL. */
export async function streamable(url: string): Promise<string> {
  const m = ASSET_URL.exec(url);
  if (!m) return url;
  let path = "";
  try { path = decodeURIComponent(m[1]); } catch { return url; }
  return /\.(mp4|m4v|webm|mov|mkv|avi|ogv|mp3|m4a|aac|wav|ogg|oga|opus|flac|weba)$/i.test(path) ? streamUrl(path) : url;
}

/** Playable URL of an article video / track. */
export async function mediaUrl(m: ArticleMedia): Promise<string> {
  return streamUrl(await mediaPath(m));
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

/**
 * `<video use:streamSrc={url}>` — like `src={url}`, but an asset: URL of a
 * local video / audio file is swapped for its loopback streaming URL first.
 * A `#t=…` fragment is kept.
 */
export function streamSrc(node: HTMLMediaElement, url: string) {
  let cur = "";
  const apply = (u: string) => {
    cur = u;
    const hash = u.indexOf("#");
    const base = hash >= 0 ? u.slice(0, hash) : u;
    const frag = hash >= 0 ? u.slice(hash) : "";
    void streamable(base).then((s) => { if (cur === u) node.src = s + frag; }).catch((e) => console.warn("[aqua-media]", String(e)));
  };
  apply(url);
  return { update: apply, destroy() { cur = ""; } };
}

/** Human text for a failed <audio>/<video>, with the Linux codec hint. */
export function mediaErrorText(el: HTMLMediaElement): string {
  const code = el.error?.code;
  const linux = document.documentElement.classList.contains("wk-lowfx");
  if (code === 4 || code === 3) {
    return linux
      ? "No decoder for this format — install GStreamer codecs: sudo pacman -S --needed gst-libav gst-plugins-good"
      : "This format can't be played here";
  }
  return el.error?.message || "Playback failed";
}

const stills = new Map<string, Promise<string>>();
/** First frame of a GIF as a still image (for "Autoplay GIFs" off). */
export function gifStill(url: string): Promise<string> {
  let p = stills.get(url);
  if (!p) {
    p = (async () => {
      try {
        const bmp = await createImageBitmap(await (await fetch(url)).blob());
        const c = document.createElement("canvas");
        c.width = bmp.width; c.height = bmp.height;
        c.getContext("2d")?.drawImage(bmp, 0, 0);
        bmp.close?.();
        return await new Promise<string>((res) => c.toBlob((b) => res(b ? URL.createObjectURL(b) : url), "image/png"));
      } catch {
        return url;
      }
    })();
    stills.set(url, p);
  }
  return p;
}
