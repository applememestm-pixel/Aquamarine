// Voice-to-text on the device (stt.rs / crates/aqua-stt).
//
// The WebView decodes the recording (whatever it can play: opus in webm/ogg,
// aac in mp4, the sound of a circle), resamples it to 16 kHz mono and hands
// the samples to Rust. Before the first transcript the model has to be
// downloaded once — always after the person agreed (askModel).
import { writable, get } from "svelte/store";
import { api } from "./tauri";
import { tr } from "./i18n";
import { askConfirm } from "./components/Confirm.svelte";

/** Download progress (0…1) while the model is being fetched, else null. */
export const sttProgress = writable<number | null>(null);

let unlisten: Promise<() => void> | null = null;
function follow() {
  unlisten ??= api.onSttProgress((p) => { if (p.total > 0) sttProgress.set(p.done / p.total); });
}

const mb = (n: number) => `${Math.round(n / 1048576)} MB`;

let asking: Promise<boolean> | null = null;
/** True when the model is here (downloaded now if the person agreed). */
export async function ensureModel(): Promise<boolean> {
  const st = await api.sttStatus();
  if (!st.supported) throw new Error(tr("Voice-to-text isn't available on this device yet."));
  if (st.ready) return true;
  if (st.downloading || get(sttProgress) != null) return waitDownload();
  asking ??= askConfirm({
    title: tr("Download the speech recognition model?"),
    text: tr("To turn voice into text, Aquamarine needs a recognition model on this device ({size}, once). It runs right here: transcripts then work without the internet — over Bluetooth and Wi-Fi too — and no server, ours included, ever hears your voice messages. You can delete it in Settings → Data and storage.", { size: mb(st.size) }),
    ok: tr("Download"),
  }).finally(() => { asking = null; });
  if (!(await asking)) return false;
  follow();
  sttProgress.set(0);
  try { await api.sttDownload(); }
  finally { sttProgress.set(null); }
  return true;
}

async function waitDownload(): Promise<boolean> {
  follow();
  for (;;) {
    await new Promise((r) => setTimeout(r, 700));
    const st = await api.sttStatus();
    if (!st.downloading) { sttProgress.set(null); return st.ready; }
  }
}

/** 16 kHz mono s16le of any recording the WebView can decode. */
export async function decode16k(data: ArrayBuffer): Promise<Uint8Array> {
  const Ctx = window.AudioContext ?? (window as unknown as { webkitAudioContext: typeof AudioContext }).webkitAudioContext;
  const ctx = new Ctx();
  let buf: AudioBuffer;
  try { buf = await ctx.decodeAudioData(data); }
  catch { throw new Error(tr("This recording's sound can't be read here.")); }
  finally { void ctx.close(); }
  const len = Math.max(1, Math.ceil(buf.duration * 16000));
  const off = new OfflineAudioContext(1, len, 16000);
  const src = off.createBufferSource();
  src.buffer = buf; // several channels are mixed down to one
  src.connect(off.destination);
  src.start();
  const pcm = (await off.startRendering()).getChannelData(0);
  const out = new Int16Array(pcm.length);
  for (let i = 0; i < pcm.length; i++) out[i] = Math.max(-32768, Math.min(32767, Math.round(pcm[i] * 32767)));
  return new Uint8Array(out.buffer);
}

// Transcripts are kept on this device only (never synced): by message.
const KEY = "aqua.transcripts.v1";
const MAX = 1500;
function load(): Record<string, string> { try { return JSON.parse(localStorage.getItem(KEY) || "{}"); } catch { return {}; } }
export function cachedTranscript(id: string): string | null { return load()[id] ?? null; }
function remember(id: string, text: string) {
  try {
    const m = load();
    delete m[id];
    m[id] = text;
    const keys = Object.keys(m);
    for (const k of keys.slice(0, Math.max(0, keys.length - MAX))) delete m[k];
    localStorage.setItem(KEY, JSON.stringify(m));
  } catch { /* full or blocked: just not kept */ }
}

/** Text of a recording (a URL the WebView can fetch, or the bytes). `id`
 *  keys the local cache (a message id); null for a draft. Null when the
 *  person declined the model download. */
export async function transcribe(src: string | Blob, id: string | null): Promise<string | null> {
  if (id) { const c = cachedTranscript(id); if (c != null) return c; }
  if (!(await ensureModel())) return null;
  const data = typeof src === "string" ? await (await fetch(src)).arrayBuffer() : await src.arrayBuffer();
  const text = (await api.sttTranscribe(await decode16k(data))).trim();
  if (id) remember(id, text);
  return text;
}
