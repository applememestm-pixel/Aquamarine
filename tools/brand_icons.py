#!/usr/bin/env python3
"""Every icon of the messenger, the server app and the site from one picture.

    python3 tools/brand_icons.py [brand/orbit-1024.png]

The source is the logo on its dark background, square (1024×1024). Desktop
icons get a rounded square, Android its adaptive layers (the logo keyed out of
the background on top of the background colour), the tray a keyed logo
(colour and white), the app/site logos a rounded square.
"""
import sys
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFilter

ROOT = Path(__file__).resolve().parent.parent
SRC = Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / "brand" / "orbit-1024.png"
APPS = [ROOT / "crates/aqua-client-ui/icons", ROOT / "crates/aqua-server-app/icons"]

src = Image.open(SRC).convert("RGB")
assert src.size[0] == src.size[1], "the source must be square"
BG = src.getpixel((4, 4))


def rounded(img: Image.Image, radius=0.225) -> Image.Image:
    """The picture in a rounded square (transparent corners), smooth edges."""
    s = img.size[0]
    big = s * 4
    m = Image.new("L", (big, big), 0)
    ImageDraw.Draw(m).rounded_rectangle((0, 0, big - 1, big - 1), radius=int(big * radius), fill=255)
    m = m.resize((s, s), Image.LANCZOS)
    out = img.convert("RGBA")
    out.putalpha(m)
    return out


def circle(img: Image.Image) -> Image.Image:
    s = img.size[0]
    big = s * 4
    m = Image.new("L", (big, big), 0)
    ImageDraw.Draw(m).ellipse((0, 0, big - 1, big - 1), fill=255)
    out = img.convert("RGBA")
    out.putalpha(m.resize((s, s), Image.LANCZOS))
    return out


def keyed() -> Image.Image:
    """The logo without its background: alpha from how much brighter than
    the background each pixel is; colour un-premultiplied from it."""
    bg = Image.new("RGB", src.size, BG)
    diff = ImageChops.subtract(src, bg).convert("L")
    alpha = diff.point(lambda v: max(0, min(255, int((v - 4) * 3.2))))
    rgba = src.convert("RGBA")
    rgba.putalpha(alpha)
    # Trim to the logo, then a square with a little air.
    box = alpha.point(lambda v: 255 if v > 24 else 0).getbbox()
    rgba = rgba.crop(box)
    w, h = rgba.size
    side = int(max(w, h) * 1.08)
    sq = Image.new("RGBA", (side, side), (0, 0, 0, 0))
    sq.paste(rgba, ((side - w) // 2, (side - h) // 2))
    return sq


def fit(img: Image.Image, size: int) -> Image.Image:
    return img.resize((size, size), Image.LANCZOS)


def on_canvas(logo: Image.Image, size: int, part: float) -> Image.Image:
    """`logo` at `part` of a transparent square `size`."""
    c = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    inner = fit(logo, int(size * part))
    c.paste(inner, ((size - inner.size[0]) // 2, (size - inner.size[1]) // 2), inner)
    return c


app = rounded(src)
logo = keyed()

for d in APPS:
    for name, size in [("32x32.png", 32), ("128x128.png", 128), ("128x128@2x.png", 256), ("icon.png", 512)]:
        fit(app, size).save(d / name)
    fit(app, 256).save(d / "icon.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
    fit(app, 1024).save(d / "icon.icns")
    # Tray: the logo itself (it's small there), in colour and in white.
    tray = on_canvas(logo, 64, 0.96)
    tray.save(d / "tray.png")
    white = Image.new("RGBA", tray.size, (255, 255, 255, 0))
    white.putalpha(tray.getchannel("A"))
    white.save(d / "tray-mono.png")
    # Android: legacy square / round, adaptive foreground (108dp, logo in the 66dp safe zone).
    for dens, s in [("mdpi", 48), ("hdpi", 72), ("xhdpi", 96), ("xxhdpi", 144), ("xxxhdpi", 192)]:
        md = d / "android" / f"mipmap-{dens}"
        md.mkdir(parents=True, exist_ok=True)
        fit(app, s).save(md / "ic_launcher.png")
        circle(fit(src, s)).save(md / "ic_launcher_round.png")
        on_canvas(logo, int(s * 108 / 48), 0.6).save(md / "ic_launcher_foreground.png")
    vals = d / "android" / "values"
    vals.mkdir(parents=True, exist_ok=True)
    (vals / "ic_launcher_background.xml").write_text(
        '<?xml version="1.0" encoding="utf-8"?>\n<resources>\n  <color name="ic_launcher_background">#%02X%02X%02X</color>\n</resources>\n' % BG)

# The app's own logo (title bar, rail, favicon) and the site's.
fit(app, 256).save(ROOT / "crates/aqua-client-ui/ui/public/logo.webp", quality=92)
fit(app, 256).save(ROOT / "site/logo.png")
fit(app, 64).save(ROOT / "site/favicon.png")
# The site's header / footer mark: the orbit alone, on transparent.
fit(logo, 128).save(ROOT / "site/logo-mark.png")
# The relay's admin page (lib.rs: __LOGO__).
import base64, io
buf = io.BytesIO()
fit(app, 256).save(buf, format="WEBP", quality=90)
(ROOT / "crates/aqua-server/src/admin-logo.b64").write_text(base64.b64encode(buf.getvalue()).decode())
print("icons written from", SRC, "background", BG)
