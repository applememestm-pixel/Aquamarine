/**
 * Call-quality engine: everything that decides HOW good a call looks and
 * sounds, kept as pure functions so it can be unit-tested outside WebRTC.
 *
 *  - capture constraints for mic / camera / screen (Discord-style presets)
 *  - target bitrates per resolution × fps
 *  - SDP tuning of what we ADVERTISE to the peer (Opus FEC/stereo/bitrate,
 *    Chromium x-google bitrate hints so video ramps up instantly instead of
 *    crawling up from ~300 kbps)
 *  - codec ordering (hardware H.264 first by default)
 *  - RTCRtpSender encoding parameters
 */

export type ResPreset = "720" | "1080" | "1440" | "source";
export type FpsPreset = 15 | 30 | 60;
export type ContentHint = "motion" | "detail";
export type CodecPref = "auto" | "h264" | "vp9" | "av1" | "vp8";

export const RES_DIMS: Record<Exclude<ResPreset, "source">, [number, number]> = {
  "720": [1280, 720],
  "1080": [1920, 1080],
  "1440": [2560, 1440],
};

export const RES_LABEL: Record<ResPreset, string> = {
  "720": "720p",
  "1080": "1080p",
  "1440": "1440p",
  source: "Source",
};

/** Bitrate at 30 fps (kbps). Discord Nitro-ish numbers, a bit more generous
 *  because we are P2P and don't pay for the relay bandwidth. */
const BASE30: Record<ResPreset, number> = { "720": 2500, "1080": 4500, "1440": 8000, source: 12000 };

/** Target video bitrate (kbps) for a screen share / camera preset. */
export function targetVideoKbps(res: ResPreset, fps: number, capKbps = 0): number {
  const mul = fps >= 50 ? 1.6 : fps <= 15 ? 0.6 : 1;
  const kbps = Math.round(BASE30[res] * mul);
  return capKbps > 0 ? Math.min(kbps, capKbps) : kbps;
}

/** Bitrate from the ACTUAL captured frame size (a 1080p cap on an
 *  ultrawide, or a small window, gives fewer pixels than the preset).
 *  Sub-linear in pixels: 720p30≈2.4, 1080p30=4.5, 1440p30≈6.9, 4K30≈12.7 Mbps. */
export function kbpsForFrame(w: number, h: number, fps: number, capKbps = 0): number {
  const ratio = (w * h) / (1920 * 1080);
  const mul = fps >= 50 ? 1.6 : fps <= 15 ? 0.6 : 1;
  let kbps = Math.round(4500 * Math.pow(Math.max(ratio, 0.05), 0.75) * mul);
  kbps = Math.max(800, Math.min(kbps, 25000));
  return capKbps > 0 ? Math.min(kbps, capKbps) : kbps;
}

/** Ceiling advertised in SDP — the per-sender maxBitrate does the real work. */
export const SDP_VIDEO_MAX_KBPS = 40000;
/** Where Chromium's bandwidth estimator starts. Default is ~300 kbps which
 *  makes a fresh 1080p60 share look like mush for 10–20 s. */
export const SDP_VIDEO_START_KBPS = 4000;
export const SDP_VIDEO_MIN_KBPS = 300;

// ---------------------------------------------------------------------------
// Capture constraints
// ---------------------------------------------------------------------------

export interface MicPrefs {
  micId?: string;
  echoCancellation: boolean;
  noiseSuppression: boolean;
  autoGainControl: boolean;
}

export function micConstraints(p: MicPrefs): MediaTrackConstraints {
  const c: MediaTrackConstraints = {
    echoCancellation: p.echoCancellation,
    noiseSuppression: p.noiseSuppression,
    autoGainControl: p.autoGainControl,
    sampleRate: { ideal: 48000 },
    channelCount: { ideal: 1 },
  };
  // `ideal`, not `exact`: an unplugged headset must not make the call fail.
  if (p.micId && p.micId !== "default") c.deviceId = { ideal: p.micId };
  return c;
}

export function cameraConstraints(camId: string | undefined, res: "720" | "1080"): MediaTrackConstraints {
  const [w, h] = RES_DIMS[res];
  const c: MediaTrackConstraints = {
    width: { ideal: w },
    height: { ideal: h },
    frameRate: { ideal: 30, max: 30 },
  };
  if (camId && camId !== "default") c.deviceId = { ideal: camId };
  return c;
}

/** Video constraints for a screen track — used both for getDisplayMedia and
 *  for applyConstraints when the quality is changed mid-stream. Only `max`
 *  is used so a small window is never upscaled. */
export function screenVideoConstraints(res: ResPreset, fps: number): MediaTrackConstraints {
  const v: MediaTrackConstraints = { frameRate: { ideal: fps, max: fps } };
  if (res !== "source") {
    const [w, h] = RES_DIMS[res];
    v.width = { max: w };
    v.height = { max: h };
  }
  return v;
}

export function displayMediaOptions(res: ResPreset, fps: number, withAudio: boolean): DisplayMediaStreamOptions {
  const video = { ...screenVideoConstraints(res, fps), cursor: "always" } as MediaTrackConstraints;
  return {
    video,
    audio: withAudio
      ? ({ echoCancellation: false, noiseSuppression: false, autoGainControl: false, suppressLocalAudioPlayback: false } as MediaTrackConstraints)
      : false,
    // Chromium extras (ignored elsewhere)
    systemAudio: withAudio ? "include" : "exclude",
    selfBrowserSurface: "exclude",
    surfaceSwitching: "include",
    monitorTypeSurfaces: "include",
  } as DisplayMediaStreamOptions;
}

// ---------------------------------------------------------------------------
// SDP tuning (applied to the SDP we SEND — the peer's encoder reads it)
// ---------------------------------------------------------------------------

export interface SdpTuning {
  /** Opus bitrate for the voice m-line (kbps). */
  voiceKbps: number;
  /** Opus bitrate for the screen-audio m-line (kbps). */
  musicKbps: number;
  videoMaxKbps: number;
  videoStartKbps: number;
  videoMinKbps: number;
}

export const DEFAULT_SDP_TUNING: SdpTuning = {
  voiceKbps: 96,
  musicKbps: 192,
  videoMaxKbps: SDP_VIDEO_MAX_KBPS,
  videoStartKbps: SDP_VIDEO_START_KBPS,
  videoMinKbps: SDP_VIDEO_MIN_KBPS,
};

const NON_MEDIA_CODECS = new Set(["rtx", "red", "ulpfec", "flexfec-03"]);

function parseFmtp(params: string): [string, string][] {
  return params
    .split(";")
    .map((kv) => kv.trim())
    .filter(Boolean)
    .map((kv) => {
      const i = kv.indexOf("=");
      return i < 0 ? [kv, ""] : [kv.slice(0, i).trim(), kv.slice(i + 1).trim()];
    }) as [string, string][];
}

function setFmtp(params: [string, string][], updates: Record<string, string | number>): string {
  const map = new Map(params);
  for (const [k, v] of Object.entries(updates)) map.set(k, String(v));
  return [...map.entries()].map(([k, v]) => (v === "" ? k : `${k}=${v}`)).join(";");
}

/** Upsert fmtp params for payload `pt` inside one m-section (array of lines). */
function upsertFmtp(section: string[], pt: string, updates: Record<string, string | number>) {
  const prefix = `a=fmtp:${pt} `;
  const idx = section.findIndex((l) => l.startsWith(prefix));
  if (idx >= 0) {
    section[idx] = prefix + setFmtp(parseFmtp(section[idx].slice(prefix.length)), updates);
    return;
  }
  const rtpmap = section.findIndex((l) => l.startsWith(`a=rtpmap:${pt} `));
  const line = prefix + setFmtp([], updates);
  if (rtpmap >= 0) section.splice(rtpmap + 1, 0, line);
  else section.push(line);
}

/**
 * Tune an SDP before it is signalled to the peer.
 *  - 1st audio m-line (voice): Opus FEC on, DTX off, mono, `voiceKbps`.
 *  - 2nd audio m-line (screen audio): Opus stereo, `musicKbps`.
 *  - every video codec: x-google min/start/max bitrate.
 */
export function tuneSdp(sdp: string, t: SdpTuning = DEFAULT_SDP_TUNING): string {
  const eol = sdp.includes("\r\n") ? "\r\n" : "\n";
  const lines = sdp.split(/\r?\n/);
  const trailing = lines.length && lines[lines.length - 1] === "" ? lines.pop() : undefined;

  // Split into [session, m1, m2, …]
  const sections: string[][] = [[]];
  for (const l of lines) {
    if (l.startsWith("m=")) sections.push([]);
    sections[sections.length - 1].push(l);
  }

  let audioIdx = 0;
  for (let s = 1; s < sections.length; s++) {
    const sec = sections[s];
    const kind = sec[0].slice(2).split(" ")[0];
    if (sec[0].split(" ")[1] === "0") continue; // rejected m-line

    if (kind === "audio") {
      const music = audioIdx === 1;
      audioIdx++;
      for (const l of sec.slice()) {
        const m = /^a=rtpmap:(\d+) opus\/48000/i.exec(l);
        if (!m) continue;
        upsertFmtp(sec, m[1], {
          minptime: 10,
          useinbandfec: 1,
          usedtx: 0,
          stereo: music ? 1 : 0,
          "sprop-stereo": music ? 1 : 0,
          maxaveragebitrate: (music ? t.musicKbps : t.voiceKbps) * 1000,
          maxplaybackrate: 48000,
        });
      }
    } else if (kind === "video") {
      for (const l of sec.slice()) {
        const m = /^a=rtpmap:(\d+) ([^/]+)\/90000/i.exec(l);
        if (!m || NON_MEDIA_CODECS.has(m[2].toLowerCase())) continue;
        upsertFmtp(sec, m[1], {
          "x-google-min-bitrate": t.videoMinKbps,
          "x-google-start-bitrate": t.videoStartKbps,
          "x-google-max-bitrate": t.videoMaxKbps,
        });
      }
    }
  }

  const out = sections.flat();
  if (trailing !== undefined) out.push(trailing);
  return out.join(eol);
}

// ---------------------------------------------------------------------------
// Codec preference
// ---------------------------------------------------------------------------

export interface CodecLike {
  mimeType: string;
  sdpFmtpLine?: string;
  clockRate?: number;
}

const AUTO_ORDER = ["video/h264", "video/vp9", "video/vp8", "video/av1"];

function h264Rank(c: CodecLike): number {
  const f = (c.sdpFmtpLine ?? "").toLowerCase();
  const pm = /packetization-mode=1/.test(f) ? 0 : 10;
  const plid = /profile-level-id=([0-9a-f]{6})/.exec(f)?.[1] ?? "";
  const prof = plid.startsWith("64") ? 0 : plid.startsWith("4d") ? 1 : plid.startsWith("42e0") ? 2 : 3;
  return pm + prof;
}

/**
 * Order codecs for setCodecPreferences: the preferred family first (best
 * profile first inside H.264), then the other families, then rtx/red/fec.
 * `encodable` (sender capabilities) pushes codecs we can't encode down.
 */
export function orderCodecs<T extends CodecLike>(codecs: T[], pref: CodecPref, encodable?: CodecLike[]): T[] {
  const order = pref === "auto" ? AUTO_ORDER : [`video/${pref}`, ...AUTO_ORDER.filter((m) => m !== `video/${pref}`)];
  const canEncode = (c: T) =>
    !encodable || encodable.some((e) => e.mimeType.toLowerCase() === c.mimeType.toLowerCase());
  const media = codecs.filter((c) => !NON_MEDIA_CODECS.has(c.mimeType.split("/")[1]?.toLowerCase() ?? ""));
  const extra = codecs.filter((c) => !media.includes(c));
  const rank = (c: T) => {
    const mime = c.mimeType.toLowerCase();
    let fam = order.indexOf(mime);
    if (fam < 0) fam = order.length;
    const enc = canEncode(c) ? 0 : 1000;
    const sub = mime === "video/h264" ? h264Rank(c) : mime === "video/vp9" && /profile-id=0/.test(c.sdpFmtpLine ?? "") ? 0 : 1;
    return enc + fam * 100 + sub;
  };
  const sorted = media
    .map((c, i) => ({ c, i, r: rank(c) }))
    .sort((a, b) => a.r - b.r || a.i - b.i)
    .map((x) => x.c);
  return [...sorted, ...extra];
}

// ---------------------------------------------------------------------------
// Sender parameters
// ---------------------------------------------------------------------------

export interface SenderTuning {
  maxKbps: number;
  maxFps?: number;
  hint?: ContentHint | "camera" | "voice" | "music";
}

/** Mutates + returns RTCRtpSendParameters for a sender. */
export function tuneSendParameters(p: RTCRtpSendParameters, t: SenderTuning): RTCRtpSendParameters {
  if (!p.encodings || p.encodings.length === 0) p.encodings = [{} as RTCRtpEncodingParameters];
  const e = p.encodings[0] as RTCRtpEncodingParameters & Record<string, unknown>;
  e.active = true;
  e.maxBitrate = Math.round(t.maxKbps * 1000);
  if (t.maxFps) e.maxFramerate = t.maxFps;
  if (t.hint !== "voice" && t.hint !== "music") e.scaleResolutionDownBy = 1;
  e.priority = "high";
  e.networkPriority = "high";
  const dp =
    t.hint === "detail" ? "maintain-resolution"
    : t.hint === "motion" ? "maintain-framerate"
    : t.hint === "camera" ? "balanced"
    : undefined;
  if (dp) (p as unknown as Record<string, unknown>).degradationPreference = dp;
  return p;
}

/** Short human label, e.g. "1080p60". */
export function qualityLabel(res: ResPreset, fps: number): string {
  return res === "source" ? `Source ${fps}fps` : `${res}p${fps}`;
}
