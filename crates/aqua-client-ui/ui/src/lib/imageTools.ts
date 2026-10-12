/**
 * Shrink a photo for sending inside a message: longest side ≤ maxSide,
 * WebP when the webview can encode it (Chromium / WebView2), JPEG otherwise
 * (WebKit can't encode WebP and would silently fall back to a huge PNG).
 */
export async function shrinkImage(blob: Blob, maxSide = 1600, quality = 0.82): Promise<string> {
  const bmp = await createImageBitmap(blob);
  const k = Math.min(1, maxSide / Math.max(bmp.width, bmp.height));
  const w = Math.max(1, Math.round(bmp.width * k));
  const h = Math.max(1, Math.round(bmp.height * k));
  const canvas = document.createElement("canvas");
  canvas.width = w; canvas.height = h;
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("canvas unavailable");
  ctx.drawImage(bmp, 0, 0, w, h);
  bmp.close?.();
  const encode = (type: string) => new Promise<Blob | null>((res) => canvas.toBlob(res, type, quality));
  let out = await encode("image/webp");
  if (!out || out.type !== "image/webp") out = await encode("image/jpeg");
  if (!out) throw new Error("could not encode the image");
  return await blobToDataUrl(out);
}

export function blobToDataUrl(b: Blob): Promise<string> {
  return new Promise((res, rej) => {
    const r = new FileReader();
    r.onload = () => res(String(r.result));
    r.onerror = () => rej(r.error);
    r.readAsDataURL(b);
  });
}

/** Approximate decoded size of a data: URL, in bytes. */
export function dataUrlBytes(u: string): number {
  const i = u.indexOf(",");
  return Math.floor(((u.length - i - 1) * 3) / 4);
}
