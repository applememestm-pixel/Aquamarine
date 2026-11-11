/**
 * Profile card looks: the card colour (taken from the banner unless set by
 * hand) and the black-and-white emoji pattern over the banner.
 */

const avgCache = new Map<string, string>();

/** Average colour of an image, or "" when it can't be read (cross-origin,
 *  video, not loaded). */
export function averageColor(url: string): Promise<string> {
  if (!url) return Promise.resolve("");
  const hit = avgCache.get(url);
  if (hit !== undefined) return Promise.resolve(hit);
  return new Promise((resolve) => {
    const img = new Image();
    img.crossOrigin = "anonymous";
    img.onload = () => {
      try {
        const c = document.createElement("canvas");
        c.width = 24; c.height = 8;
        const g = c.getContext("2d", { willReadFrequently: true });
        if (!g) return resolve("");
        g.drawImage(img, 0, 0, 24, 8);
        const d = g.getImageData(0, 0, 24, 8).data;
        let r = 0, gg = 0, b = 0, n = 0;
        for (let i = 0; i < d.length; i += 4) { r += d[i]; gg += d[i + 1]; b += d[i + 2]; n++; }
        const hex = "#" + [r, gg, b].map((v) => Math.round(v / n).toString(16).padStart(2, "0")).join("");
        avgCache.set(url, hex);
        resolve(hex);
      } catch { avgCache.set(url, ""); resolve(""); }
    };
    img.onerror = () => resolve("");
    img.src = url;
  });
}

function rgb(hex: string): [number, number, number] {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim());
  const n = m ? parseInt(m[1], 16) : 0x4f7afe;
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

/** Halfway between the two banner colours. */
export function mixHex(a: string, b: string): string {
  const x = rgb(a), y = rgb(b);
  return "#" + x.map((v, i) => Math.round((v + y[i]) / 2).toString(16).padStart(2, "0")).join("");
}

/** Light enough that dark text reads better on it. */
export function isLight(hex: string): boolean {
  const [r, g, b] = rgb(hex);
  return 0.299 * r + 0.587 * g + 0.114 * b > 170;
}

export const PATTERN_EMOJI = ["⭐", "❤️", "🌸", "🐾", "🎮", "🎵", "🔥", "❄️", "🌙", "💎", "🍀", "👾", "🦋", "☕"];

const patCache = new Map<string, string>();

/** A tile of the emoji drawn in white (black-and-white), as a data URL for
 *  `background-image`. Drawn on a canvas so it looks the same everywhere. */
/**
 * Telegram's profile pattern: the emoji in rings around the avatar,
 * biggest and brightest next to it, fading outwards. Drawn once per emoji
 * for a banner 900×200 (shown at its height, anchored top-left, where the
 * avatar sits: centre x 74, just under the banner's bottom edge).
 */
const ringCache = new Map<string, string>();
export function patternRingUrl(emoji: string): string {
  emoji = [...(emoji ?? "").trim()].slice(0, 4).join("");
  if (!emoji) return "";
  const hit = ringCache.get(emoji);
  if (hit) return hit;
  try {
    const K = 2, W = 900, H = 200, CX = 74, CY = 206;
    const c = document.createElement("canvas");
    c.width = W * K; c.height = H * K;
    const g = c.getContext("2d", { willReadFrequently: true });
    if (!g) return "";
    g.scale(K, K);
    g.textAlign = "center";
    g.textBaseline = "middle";
    const rings = [84, 128, 176, 230, 290, 356, 428, 506, 590, 680, 776, 878];
    rings.forEach((r, i) => {
      const t = i / (rings.length - 1);
      const size = 30 - 13 * t;
      const n = Math.max(6, Math.floor((2 * Math.PI * r) / (size * 2.5)));
      g.globalAlpha = 0.95 - 0.62 * t;
      g.font = `${size}px "Noto Color Emoji","Apple Color Emoji","Segoe UI Emoji",sans-serif`;
      for (let k = 0; k < n; k++) {
        const a = ((k + (i % 2 ? 0.5 : 0)) / n) * Math.PI * 2;
        const x = CX + Math.cos(a) * r, y = CY + Math.sin(a) * r * 0.9;
        if (x < -20 || x > W + 20 || y < -20 || y > H + 10) continue;
        g.save();
        g.translate(x, y);
        g.rotate(a + Math.PI / 2);
        g.fillText(emoji, 0, 0);
        g.restore();
      }
    });
    g.globalAlpha = 1;
    // White by brightness, as in the tiled pattern.
    const img = g.getImageData(0, 0, W * K, H * K);
    const d = img.data;
    for (let i = 0; i < d.length; i += 4) {
      if (!d[i + 3]) continue;
      const l = 0.299 * d[i] + 0.587 * d[i + 1] + 0.114 * d[i + 2];
      d[i + 3] = Math.round(d[i + 3] * (0.6 + 0.4 * (l / 255)));
      d[i] = d[i + 1] = d[i + 2] = 255;
    }
    g.putImageData(img, 0, 0);
    const url = c.toDataURL("image/png");
    ringCache.set(emoji, url);
    return url;
  } catch {
    return "";
  }
}

export function patternUrl(emoji: string): string {
  emoji = [...(emoji ?? "").trim()].slice(0, 4).join("");
  if (!emoji) return "";
  const hit = patCache.get(emoji);
  if (hit) return hit;
  try {
    const W = 168, H = 112;
    const c = document.createElement("canvas");
    c.width = W; c.height = H;
    const g = c.getContext("2d", { willReadFrequently: true });
    if (!g) return "";
    g.textAlign = "center";
    g.textBaseline = "middle";
    // Staggered rows of two sizes, slightly turned, like Telegram's.
    const spots: [number, number, number, number][] = [
      [24, 22, 22, -0.25], [84, 30, 16, 0.2], [144, 20, 20, 0.1],
      [52, 78, 16, 0.3], [114, 84, 24, -0.15],
    ];
    for (const [x, y, size, rot] of spots) {
      g.save();
      g.translate(x, y);
      g.rotate(rot);
      g.font = `${size}px "Noto Color Emoji","Apple Color Emoji","Segoe UI Emoji",sans-serif`;
      g.fillText(emoji, 0, 0);
      g.restore();
    }
    // Black and white: keep the shape, paint it white by its brightness.
    const img = g.getImageData(0, 0, W, H);
    const d = img.data;
    for (let i = 0; i < d.length; i += 4) {
      const l = 0.299 * d[i] + 0.587 * d[i + 1] + 0.114 * d[i + 2];
      const a = d[i + 3];
      d[i] = d[i + 1] = d[i + 2] = 255;
      d[i + 3] = Math.round(a * (0.55 + 0.45 * (l / 255)));
    }
    g.putImageData(img, 0, 0);
    const url = c.toDataURL("image/png");
    patCache.set(emoji, url);
    return url;
  } catch {
    return "";
  }
}

/** Mix `a` into `b`: pct = how much of `a` (0–100). */
export function mixPct(a: string, b: string, pct: number): string {
  const x = rgb(a), y = rgb(b), k = Math.max(0, Math.min(100, pct)) / 100;
  return "#" + x.map((v, i) => Math.round(v * k + y[i] * (1 - k)).toString(16).padStart(2, "0")).join("");
}

/** Relative luminance (WCAG), 0 = black, 1 = white. */
export function luminance(hex: string): number {
  const lin = (v: number) => { const c = v / 255; return c <= 0.03928 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4); };
  const [r, g, b] = rgb(hex);
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}

/** Colours offered for the card, besides "from the banner" and any colour. */
export const CARD_SWATCHES = [
  "#2EE6C5", "#4F7AFE", "#7B5CFF", "#C04CFD", "#FF5CA8", "#FF5A5F",
  "#FF8A3D", "#FFC83D", "#7BD957", "#1FB57A", "#3A4A63", "#F2EEE6", "#38BDF8", "#16181F",
];

export const CARD_INTENSITY = 62;
export const CARD_LENGTH = 60;

export interface CardLook {
  /** CSS background for the whole card. */
  background: string;
  /** The colour most of the text sits on — for picking dark or light text. */
  base: string;
  /** The colour at the very top (under the avatar). */
  top: string;
  /** Dark text reads better than light on it. */
  light: boolean;
  border: string;
}

/**
 * The card's background: `intensity` (0–100) is how strong the colour is at
 * the top, `length` (0–100) how far down it fades into the app background
 * (0 = one solid colour, 100 = fades almost out). Text colour follows the
 * result, so a pale card gets dark text.
 */
export function cardLook(card: string, intensity = CARD_INTENSITY, length = CARD_LENGTH, shell = "#0E1117"): CardLook {
  const top = Math.max(0, Math.min(100, intensity));
  const bottom = top * (1 - Math.max(0, Math.min(100, length)) / 100 * 0.8);
  const ratio = (x: number, y: number) => (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05);
  // The colour fades either into the dark app background or into a pale
  // neutral. Fading a pale colour into the dark one gave a grey middle where
  // neither white nor dark text reads, so take whichever fade keeps the
  // worst spot of the gradient most readable (WCAG contrast).
  const variant = (into: string) => {
    const a = mixPct(card, into, top), b = mixPct(card, into, (top + bottom) / 2), c = mixPct(card, into, bottom);
    const ls = [a, b, c].map(luminance);
    const dark = Math.min(...ls.map((l) => ratio(l, 0.012)));   // ~#16181e text
    const white = Math.min(...ls.map((l) => ratio(l, 0.93)));   // ~#f4f6fa text
    return { a, b, c, light: dark > white, score: Math.max(dark, white) };
  };
  const onDark = variant(shell), onPale = variant("#F4F5F8");
  // The dark fade is the look of the app: keep it unless it reads clearly worse.
  const v = onDark.score >= 4.5 || onDark.score >= onPale.score ? onDark : onPale;
  return {
    background: `linear-gradient(180deg, ${v.a} 0%, ${v.b} 50%, ${v.c} 100%)`,
    base: v.b,
    top: v.a,
    light: v.light,
    border: mixPct(card, v.light ? "#000000" : "#ffffff", 70),
  };
}

/** The app's background colour, for mixing the card into it. */
export function shellColor(): string {
  try {
    const v = getComputedStyle(document.documentElement).getPropertyValue("--bg-shell").trim();
    if (/^#[0-9a-f]{6}$/i.test(v)) return v;
    const m = /rgba?\((\d+)\D+(\d+)\D+(\d+)/.exec(v);
    if (m) return "#" + [m[1], m[2], m[3]].map((x) => (+x).toString(16).padStart(2, "0")).join("");
  } catch { /* not in a browser */ }
  return "#0E1117";
}
