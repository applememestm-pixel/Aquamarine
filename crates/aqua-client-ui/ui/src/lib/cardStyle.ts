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
