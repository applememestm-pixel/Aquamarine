/**
 * MediaRecorder that works on every WebView we ship in. Android's WebView
 * may claim a format in isTypeSupported() and still refuse it in the
 * constructor or in start() (or support only MP4); WebKitGTK and WebView2
 * differ again. So: try each candidate until one actually starts, then the
 * browser's own default.
 */
export interface Started { recorder: MediaRecorder; mime: string }

export const VIDEO_TYPES = ["video/webm;codecs=vp9,opus", "video/webm;codecs=vp8,opus", "video/webm", "video/mp4;codecs=avc1,mp4a", "video/mp4"];
export const AUDIO_TYPES = ["audio/webm;codecs=opus", "audio/webm", "audio/ogg;codecs=opus", "audio/ogg", "audio/mp4;codecs=mp4a", "audio/mp4", "audio/aac"];

export function startRecorder(stream: MediaStream, types: string[], onData: (b: Blob) => void, timeslice?: number): Started {
  const tried: string[] = [];
  const list = [...types.filter((t) => { try { return MediaRecorder.isTypeSupported(t); } catch { return false; } }), ""];
  for (const t of list) {
    try {
      const r = new MediaRecorder(stream, t ? { mimeType: t } : undefined);
      r.ondataavailable = (e) => { if (e.data && e.data.size > 0) onData(e.data); };
      if (timeslice) r.start(timeslice); else r.start();
      return { recorder: r, mime: r.mimeType || t || "" };
    } catch (e) {
      tried.push(`${t || "default"}: ${(e as Error)?.name ?? e}`);
    }
  }
  throw new Error(`this device can't record (${tried.join("; ")})`);
}

/** File extension for a recorded MIME type. */
export function extFor(mime: string, fallback: string): string {
  const m = mime.toLowerCase();
  if (m.includes("mp4")) return m.startsWith("audio") ? "m4a" : "mp4";
  if (m.includes("ogg")) return "ogg";
  if (m.includes("aac")) return "aac";
  if (m.includes("webm")) return m.startsWith("audio") ? "weba" : "webm";
  return fallback;
}
