import { IS_ANDROID } from "./platform";

/**
 * Screen sharing on Android. The WebView has no getDisplayMedia, so the
 * app asks the system itself (MediaProjection — the same "Start recording
 * or casting?" prompt Gemini Live shows; android/MainActivity.kt,
 * `ScreenBridge` + `AquaScreenService`). Frames of the screen come back as
 * JPEG, are drawn into a canvas, and the canvas's captureStream() is the
 * video track the call sends — like a computer's shared screen.
 */
interface ScreenBridge {
  start(fps: number, maxSide: number): boolean;
  stop(): void;
  state(): string;
  frame(after: number): string;
}

function bridge(): ScreenBridge | null {
  if (!IS_ANDROID || typeof window === "undefined") return null;
  const b = (window as unknown as { AquaScreen?: ScreenBridge }).AquaScreen;
  return b && typeof b.start === "function" ? b : null;
}

/** This build of the Android app can share the screen. */
export function canShareAndroidScreen(): boolean {
  return bridge() !== null;
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** Ask for the screen and return it as a MediaStream (video only). Throws
 *  `NotAllowedError` if the user said no. */
export async function androidScreen(fps = 10, maxSide = 1280): Promise<MediaStream> {
  const b = bridge();
  if (!b) throw new Error("screen capture isn't available");
  fps = Math.max(1, Math.min(15, Math.round(fps)));
  if (b.state() === "on") b.stop();
  if (!b.start(fps, maxSide)) throw new Error("screen capture isn't available");
  const t0 = Date.now();
  for (;;) {
    await sleep(150);
    const s = b.state();
    if (s === "on") break;
    if (s === "denied") throw new DOMException("Screen sharing was not allowed", "NotAllowedError");
    if (s === "error" || s === "off") throw new Error("screen capture failed");
    if (Date.now() - t0 > 90_000) { b.stop(); throw new Error("screen capture timed out"); }
  }

  const canvas = document.createElement("canvas");
  canvas.width = 16;
  canvas.height = 16;
  const ctx = canvas.getContext("2d");
  if (!ctx || typeof canvas.captureStream !== "function") { b.stop(); throw new Error("screen capture isn't available"); }
  const stream = canvas.captureStream(fps);
  const track = stream.getVideoTracks()[0];
  const img = new Image();
  let seq = 0;
  let alive = true;
  let busy = false;
  const stopTrack = track.stop.bind(track);
  const end = () => {
    if (!alive) return;
    alive = false;
    try { b.stop(); } catch { /* gone */ }
    stopTrack();
    // The call's own "ended" handler (the system's "Stop sharing", the
    // status-bar chip) — a stopped track doesn't fire it by itself.
    track.dispatchEvent(new Event("ended"));
  };
  // The call stops the track: stop the capture too.
  track.stop = () => { if (alive) { alive = false; try { b.stop(); } catch { /* gone */ } } stopTrack(); };
  img.onload = () => {
    if (canvas.width !== img.naturalWidth || canvas.height !== img.naturalHeight) {
      canvas.width = img.naturalWidth;
      canvas.height = img.naturalHeight;
    }
    ctx.drawImage(img, 0, 0);
    busy = false;
  };
  img.onerror = () => { busy = false; };
  const tick = () => {
    if (!alive) return;
    if (track.readyState === "ended") { alive = false; try { b.stop(); } catch { /* gone */ } return; }
    if (b.state() !== "on") { end(); return; }
    if (!busy) {
      const f = b.frame(seq);
      if (f) {
        // "seq,width,height,base64"
        const a = f.indexOf(","), c = f.indexOf(",", a + 1), d = f.indexOf(",", c + 1);
        if (a > 0 && d > c) {
          seq = Number(f.slice(0, a)) || seq + 1;
          busy = true;
          img.src = "data:image/jpeg;base64," + f.slice(d + 1);
        }
      }
    }
    setTimeout(tick, Math.max(30, Math.floor(1000 / (fps * 2))));
  };
  tick();
  return stream;
}
