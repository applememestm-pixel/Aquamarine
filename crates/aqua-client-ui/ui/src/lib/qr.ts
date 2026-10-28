/**
 * QR codes: what goes inside them and how to read them back.
 *
 *  - a person: `aqua:@name` (+ `?s=<server>` when known), like Telegram's
 *    profile QR — scanning opens a chat with them;
 *  - a key check: `aqua:key:<name>:<code>`, like Signal's safety number —
 *    scanning theirs compares it with the code on this device;
 *  - a server: `aqua:srv?u=<address>` (a plain http(s) address works too) —
 *    scanning adds it to Settings → Network & proxy;
 *  - an invitation to an invite-only server: `aqua:join?s=<address>&c=<code>`
 *    (the server's admin panel makes them) — scanning picks that server for
 *    signing up and passes the code along.
 */
import qrcode from "qrcode-generator";

export type QrPayload =
  | { kind: "user"; username: string; server: string }
  | { kind: "key"; username: string; code: string }
  | { kind: "server"; url: string }
  | { kind: "join"; url: string; code: string };

export function userQr(username: string, server = ""): string {
  return `aqua:@${username}` + (server ? `?s=${encodeURIComponent(server)}` : "");
}
export function serverQr(url: string): string {
  return `aqua:srv?u=${encodeURIComponent(url)}`;
}
export function keyQr(username: string, code: string): string {
  return `aqua:key:${username}:${code.replace(/\s+/g, "")}`;
}

export function parseQr(text: string): QrPayload | null {
  const t = (text ?? "").trim();
  let m = /^aqua:@([a-z0-9_.]{3,32})(?:\?s=([^&\s]+))?$/i.exec(t);
  if (m) return { kind: "user", username: m[1].toLowerCase(), server: m[2] ? decodeURIComponent(m[2]) : "" };
  m = /^aqua:key:([a-z0-9_.]{3,32}):([0-9a-z]+)$/i.exec(t);
  if (m) return { kind: "key", username: m[1].toLowerCase(), code: m[2] };
  m = /^aqua:srv\?u=([^&\s]+)$/i.exec(t);
  if (m) { const url = decodeURIComponent(m[1]); if (/^https?:\/\/[^\s]+$/i.test(url)) return { kind: "server", url }; }
  m = /^aqua:join\?s=([^&\s]+)&c=([0-9a-f]{8,64})$/i.exec(t);
  if (m) { const url = decodeURIComponent(m[1]); if (/^https?:\/\/[^\s]+$/i.test(url)) return { kind: "join", url, code: m[2].toLowerCase() }; }
  if (/^https?:\/\/[^\s]+$/i.test(t)) return { kind: "server", url: t };
  return null;
}

/** The modules of a QR code (true = dark), error correction "Q" (the avatar in the middle hides some). */
export function qrMatrix(text: string): boolean[][] {
  const q = qrcode(0, "Q");
  q.addData(text, "Byte");
  q.make();
  const n = q.getModuleCount();
  return Array.from({ length: n }, (_, r) => Array.from({ length: n }, (_, c) => q.isDark(r, c)));
}

/** Read a QR code from a video frame: the browser's detector when there is
 *  one (Android), else a small decoder in JavaScript (desktop). */
let detector: { detect: (s: CanvasImageSource) => Promise<{ rawValue: string }[]> } | null | undefined;
export async function readQr(video: HTMLVideoElement, canvas: HTMLCanvasElement): Promise<string | null> {
  if (!video.videoWidth) return null;
  if (detector === undefined) {
    const BD = (globalThis as unknown as { BarcodeDetector?: new (o: { formats: string[] }) => typeof detector }).BarcodeDetector;
    try { detector = BD ? new BD({ formats: ["qr_code"] }) : null; } catch { detector = null; }
  }
  if (detector) {
    try { const r = await detector.detect(video); if (r[0]?.rawValue) return r[0].rawValue; return null; } catch { detector = null; }
  }
  const w = Math.min(640, video.videoWidth), h = Math.round(video.videoHeight * (w / video.videoWidth));
  canvas.width = w; canvas.height = h;
  const g = canvas.getContext("2d", { willReadFrequently: true });
  if (!g) return null;
  g.drawImage(video, 0, 0, w, h);
  const { default: jsQR } = await import("jsqr");
  const img = g.getImageData(0, 0, w, h);
  return jsQR(img.data, w, h, { inversionAttempts: "attemptBoth" })?.data ?? null;
}

// ---- An invitation scanned before signing up: used by the sign-up form ----
const INV_KEY = "aqua.joinInvite.v1";
export function saveInvite(url: string, code: string) {
  try { localStorage.setItem(INV_KEY, JSON.stringify({ url: url.replace(/\/+$/, ""), code })); } catch { /* ignore */ }
}
/** The invitation code for this server, if one was scanned. */
export function inviteFor(url: string): string {
  try {
    const v = JSON.parse(localStorage.getItem(INV_KEY) || "null") as { url: string; code: string } | null;
    return v && v.url === url.replace(/\/+$/, "") ? v.code : "";
  } catch { return ""; }
}
