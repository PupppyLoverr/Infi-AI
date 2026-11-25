#!/usr/bin/env python3
"""Draw Solyra's own dock and app icons (first-party art, no icon library).

    python3 make_icons.py

The style follows macOS's: a superellipse "squircle" body with a top-to-bottom
gradient, one bold white or coloured glyph, a soft shadow under the body. The
drawings are Solyra's own. Each icon is 192 px with the body inset 12 px on
every side (the panel relies on that 168/192 ratio, see `icons::BODY`).
Everything is drawn at 4x and scaled down for clean edges.
"""
import math
import os

from PIL import Image, ImageDraw, ImageFilter

OUT = os.path.dirname(os.path.abspath(__file__))
K = 4                       # supersampling
S = 192 * K                 # canvas
B = 168 * K                 # body
O = (S - B) // 2            # body offset


def hexc(h, a=255):
    h = h.lstrip("#")
    return (int(h[0:2], 16), int(h[2:4], 16), int(h[4:6], 16), a)


def squircle_mask(size, n=5.0):
    """A superellipse |x|^n + |y|^n <= 1, the shape of a macOS icon."""
    m = Image.new("L", (size, size), 0)
    px = m.load()
    r = size / 2
    for y in range(size):
        dy = abs((y + 0.5 - r) / r)
        lim = (1 - dy ** n)
        if lim <= 0:
            continue
        dx = lim ** (1 / n) * r
        x0, x1 = int(math.ceil(r - dx)), int(math.floor(r + dx))
        for x in range(max(0, x0), min(size, x1 + 1)):
            px[x, y] = 255
    return m


MASK = squircle_mask(B)


def vgrad(size, top, bottom):
    g = Image.new("RGBA", (1, size[1]))
    for y in range(size[1]):
        t = y / max(1, size[1] - 1)
        g.putpixel((0, y), tuple(int(top[i] + (bottom[i] - top[i]) * t) for i in range(4)))
    return g.resize(size)


def canvas():
    return Image.new("RGBA", (S, S), (0, 0, 0, 0))


def body(top, bottom, rim=True):
    """A squircle tile with a gradient, a soft shadow and a faint lit rim."""
    img = canvas()
    shadow = Image.new("L", (S, S), 0)
    shadow.paste(MASK, (O, O + 6 * K))
    shadow = shadow.filter(ImageFilter.GaussianBlur(7 * K)).point(lambda v: int(v * 0.42))
    img.paste((0, 0, 0, 255), (0, 0), shadow)
    tile = vgrad((B, B), hexc(top), hexc(bottom))
    img.paste(tile, (O, O), MASK)
    if rim:
        # A thin lit edge along the top of the body, as on macOS icons: the
        # band between the shape and itself moved down a few pixels.
        from PIL import ImageChops
        moved = Image.new("L", (B, B), 0)
        moved.paste(MASK, (0, 3 * K))
        band = ImageChops.subtract(MASK, moved).point(lambda v: int(v * 0.42))
        img.paste((255, 255, 255, 255), (O, O), band)
    return img


def layer():
    return Image.new("RGBA", (S, S), (0, 0, 0, 0))


def over(img, lay):
    img.alpha_composite(lay)
    return img


def cx(v):
    return O + v * B


def finish(img, name):
    img = img.resize((192, 192), Image.LANCZOS)
    img.save(os.path.join(OUT, f"{name}.png"), optimize=True)


# ------------------------------------------------------------------ glyphs

def folder(d, x0, y0, x1, y1, back, front, tab=True):
    w, h = x1 - x0, y1 - y0
    r = h * 0.12
    if tab:
        d.rounded_rectangle((x0, y0, x0 + w * 0.42, y0 + h * 0.3), radius=r, fill=back)
    d.rounded_rectangle((x0, y0 + h * 0.12, x1, y1), radius=r, fill=back)
    d.rounded_rectangle((x0, y0 + h * 0.28, x1, y1), radius=r, fill=front)


def files():
    img = body("#6ccbff", "#1b74e8")
    lay = layer()
    d = ImageDraw.Draw(lay)
    folder(d, cx(0.2), cx(0.27), cx(0.8), cx(0.73), (255, 255, 255, 150), (255, 255, 255, 245))
    # A face of sorts: two eyes, a smile, Solyra's own friendly folder.
    for ex in (0.41, 0.59):
        d.ellipse((cx(ex) - K * 6, cx(0.47), cx(ex) + K * 6, cx(0.47) + K * 22), fill=hexc("#1b74e8"))
    d.arc((cx(0.38), cx(0.48), cx(0.62), cx(0.65)), 20, 160, fill=hexc("#1b74e8"), width=K * 6)
    finish(over(img, lay), "files")


def downloads(name="downloads", arrow=True):
    img = canvas()
    lay = layer()
    d = ImageDraw.Draw(lay)
    x0, y0, x1, y1 = cx(0.06), cx(0.14), cx(0.94), cx(0.86)
    sh = layer()
    ImageDraw.Draw(sh).rounded_rectangle((x0, y0 + K * 14, x1, y1 + K * 6), radius=K * 18, fill=(0, 0, 0, 90))
    img.alpha_composite(sh.filter(ImageFilter.GaussianBlur(K * 6)))
    folder(d, x0, y0, x1, y1, hexc("#3d8fe8"), hexc("#6cc0ff"))
    # front face gradient
    face = vgrad((int(x1 - x0), int(y1 - (y0 + (y1 - y0) * 0.28))), hexc("#7fcbff"), hexc("#3f9bf0"))
    m = Image.new("L", face.size, 0)
    ImageDraw.Draw(m).rounded_rectangle((0, 0, face.size[0] - 1, face.size[1] - 1), radius=(y1 - y0) * 0.12, fill=255)
    lay.paste(face, (int(x0), int(y0 + (y1 - y0) * 0.28)), m)
    d = ImageDraw.Draw(lay)
    if arrow:
        c = (cx(0.5), cx(0.6))
        d.ellipse((c[0] - K * 34, c[1] - K * 34, c[0] + K * 34, c[1] + K * 34), outline=(255, 255, 255, 200), width=K * 6)
        d.line((c[0], c[1] - K * 18, c[0], c[1] + K * 14), fill=(255, 255, 255, 230), width=K * 7)
        d.polygon([(c[0] - K * 14, c[1] + 2 * K), (c[0] + K * 14, c[1] + 2 * K), (c[0], c[1] + K * 18)], fill=(255, 255, 255, 230))
    finish(over(img, lay), name)


def folder_plain():
    downloads("folder", arrow=False)


def trash():
    img = canvas()
    lay = layer()
    d = ImageDraw.Draw(lay)
    top, bot = cx(0.2), cx(0.92)
    l0, r0, l1, r1 = cx(0.2), cx(0.8), cx(0.27), cx(0.73)
    sh = layer()
    ImageDraw.Draw(sh).polygon([(l0, top + K * 18), (r0, top + K * 18), (r1, bot + K * 8), (l1, bot + K * 8)], fill=(0, 0, 0, 90))
    img.alpha_composite(sh.filter(ImageFilter.GaussianBlur(K * 6)))
    d.polygon([(l0, top), (r0, top), (r1, bot), (l1, bot)], fill=(214, 220, 228, 215))
    for i in range(1, 6):
        t = i / 6
        xa = l0 + (r0 - l0) * t
        xb = l1 + (r1 - l1) * t
        d.line((xa, top + K * 10, xb, bot - K * 8), fill=(150, 158, 170, 200), width=K * 3)
    d.rounded_rectangle((cx(0.14), cx(0.12), cx(0.86), cx(0.2)), radius=K * 8, fill=(236, 240, 245, 255), outline=(160, 168, 180, 255), width=K * 2)
    finish(over(img, lay), "trash")


def launchpad():
    img = body("#ffffff", "#e4e5ec")
    lay = layer()
    d = ImageDraw.Draw(lay)
    cols = ["#ff5f57", "#ffbd2e", "#28c840", "#34aadc", "#5e5ce6", "#ff2d55", "#ff9f0a", "#30d158", "#bf5af2"]
    for i, c in enumerate(cols):
        r, q = divmod(i, 3)
        x = cx(0.22 + q * 0.2)
        y = cx(0.22 + r * 0.2)
        d.rounded_rectangle((x, y, x + cx(0.16) - O, y + cx(0.16) - O), radius=K * 10, fill=hexc(c))
    finish(over(img, lay), "launchpad")


def browser():
    img = body("#5ad1ff", "#1467e8")
    lay = layer()
    d = ImageDraw.Draw(lay)
    c, r = cx(0.5), B * 0.3
    d.ellipse((c - r, c - r, c + r, c + r), outline=(255, 255, 255, 240), width=K * 7)
    for a in range(0, 360, 30):
        ang = math.radians(a)
        inner = r * (0.78 if a % 90 else 0.66)
        d.line((c + math.cos(ang) * inner, c + math.sin(ang) * inner, c + math.cos(ang) * r * 0.92, c + math.sin(ang) * r * 0.92), fill=(255, 255, 255, 200), width=K * 3)
    ang = math.radians(-50)
    tip = (c + math.cos(ang) * r * 0.72, c + math.sin(ang) * r * 0.72)
    tail = (c - math.cos(ang) * r * 0.72, c - math.sin(ang) * r * 0.72)
    side = (math.cos(ang + math.pi / 2) * K * 14, math.sin(ang + math.pi / 2) * K * 14)
    d.polygon([tip, (c + side[0], c + side[1]), (c - side[0], c - side[1])], fill=hexc("#ff3b30"))
    d.polygon([tail, (c + side[0], c + side[1]), (c - side[0], c - side[1])], fill=(255, 255, 255, 250))
    finish(over(img, lay), "browser")


def terminal():
    img = body("#4a4a4f", "#141416")
    lay = layer()
    d = ImageDraw.Draw(lay)
    d.rounded_rectangle((cx(0.14), cx(0.16), cx(0.86), cx(0.84)), radius=K * 14, fill=(10, 10, 12, 255), outline=(255, 255, 255, 40), width=K * 2)
    w = K * 8
    d.line((cx(0.26), cx(0.34), cx(0.38), cx(0.43), cx(0.26), cx(0.52)), fill=(240, 240, 245, 255), width=w, joint="curve")
    d.line((cx(0.44), cx(0.54), cx(0.62), cx(0.54)), fill=hexc("#30d158"), width=w)
    finish(over(img, lay), "terminal")


def editor():
    img = body("#ffffff", "#eceef3")
    lay = layer()
    d = ImageDraw.Draw(lay)
    for i in range(6):
        y = cx(0.28 + i * 0.1)
        d.line((cx(0.22), y, cx(0.78 - (0.2 if i == 5 else 0)), y), fill=(185, 190, 200, 255), width=K * 3)
    d.line((cx(0.22), cx(0.2), cx(0.22), cx(0.8)), fill=hexc("#ff6961"), width=K * 2)
    # pencil
    a, b = (cx(0.36), cx(0.82)), (cx(0.84), cx(0.3))
    d.line((a, b), fill=hexc("#ff9f0a"), width=K * 22)
    d.line(((a[0] + (b[0] - a[0]) * 0.82, a[1] + (b[1] - a[1]) * 0.82), b), fill=hexc("#ff6b8b"), width=K * 22)
    d.polygon([(a[0] - K * 13, a[1] + K * 2), (a[0] + K * 2, a[1] + K * 13), (a[0] - K * 14, a[1] + K * 15)], fill=hexc("#4a3a2a"))
    finish(over(img, lay), "editor")


def viewer():
    img = body("#ffc46b", "#ff4f8b")
    lay = layer()
    d = ImageDraw.Draw(lay)
    d.rounded_rectangle((cx(0.16), cx(0.22), cx(0.84), cx(0.78)), radius=K * 14, fill=(255, 255, 255, 235))
    d.ellipse((cx(0.6), cx(0.3), cx(0.74), cx(0.44)), fill=hexc("#ffb02e"))
    d.polygon([(cx(0.2), cx(0.74)), (cx(0.42), cx(0.44)), (cx(0.58), cx(0.62)), (cx(0.66), cx(0.54)), (cx(0.8), cx(0.74))], fill=hexc("#3b82f6"))
    d.polygon([(cx(0.42), cx(0.44)), (cx(0.5), cx(0.55)), (cx(0.36), cx(0.55))], fill=(255, 255, 255, 220))
    finish(over(img, lay), "viewer")


def calculator():
    img = body("#3c3c40", "#18181b")
    lay = layer()
    d = ImageDraw.Draw(lay)
    d.rounded_rectangle((cx(0.18), cx(0.16), cx(0.82), cx(0.34)), radius=K * 8, fill=(120, 200, 140, 255))
    for r in range(2):
        for q in range(3):
            x, y = cx(0.18 + q * 0.23), cx(0.42 + r * 0.22)
            col = hexc("#ff9f0a") if q == 2 else (90, 90, 96, 255)
            d.rounded_rectangle((x, y, x + cx(0.18) - O, y + cx(0.17) - O), radius=K * 10, fill=col)
    w = K * 5
    x, y = cx(0.41 + 0.23) + K * 2, cx(0.42 + 0.22)
    m = cx(0.18 + 2 * 0.23) + (cx(0.18) - O) / 2
    d.line((m - K * 14, cx(0.505), m + K * 14, cx(0.505)), fill="white", width=w)
    d.line((m, cx(0.505) - K * 14, m, cx(0.505) + K * 14), fill="white", width=w)
    d.line((m - K * 14, cx(0.715) - K * 6, m + K * 14, cx(0.715) - K * 6), fill="white", width=w)
    d.line((m - K * 14, cx(0.715) + K * 6, m + K * 14, cx(0.715) + K * 6), fill="white", width=w)
    finish(over(img, lay), "calculator")


def gear(d, c, r_out, r_in, teeth, fill, hole):
    pts = []
    for i in range(teeth * 4):
        a = 2 * math.pi * i / (teeth * 4)
        rr = r_out if (i % 4) in (0, 1) else r_in
        pts.append((c[0] + math.cos(a) * rr, c[1] + math.sin(a) * rr))
    d.polygon(pts, fill=fill)
    d.ellipse((c[0] - r_in * 0.62, c[1] - r_in * 0.62, c[0] + r_in * 0.62, c[1] + r_in * 0.62), fill=hole)


def settings():
    img = body("#c9cbd3", "#6f727c")
    lay = layer()
    d = ImageDraw.Draw(lay)
    c = (cx(0.5), cx(0.5))
    gear(d, c, B * 0.34, B * 0.27, 12, (250, 250, 252, 255), (128, 131, 140, 255))
    gear(d, c, B * 0.17, B * 0.13, 8, (210, 212, 218, 255), (90, 92, 100, 255))
    finish(over(img, lay), "settings")


def music():
    img = body("#ff7a95", "#f5203f")
    lay = layer()
    d = ImageDraw.Draw(lay)
    w = K * 9
    d.line((cx(0.42), cx(0.68), cx(0.42), cx(0.26)), fill="white", width=w)
    d.line((cx(0.66), cx(0.62), cx(0.66), cx(0.2)), fill="white", width=w)
    d.polygon([(cx(0.42) - w / 2, cx(0.26)), (cx(0.66) + w / 2, cx(0.2)), (cx(0.66) + w / 2, cx(0.3)), (cx(0.42) - w / 2, cx(0.36))], fill="white")
    for x, y in ((0.36, 0.68), (0.6, 0.62)):
        d.ellipse((cx(x) - K * 22, cx(y) - K * 16, cx(x) + K * 22, cx(y) + K * 16), fill="white")
    finish(over(img, lay), "music")


def mail():
    img = body("#63d0ff", "#1a73f0")
    lay = layer()
    d = ImageDraw.Draw(lay)
    box = (cx(0.18), cx(0.3), cx(0.82), cx(0.72))
    d.rounded_rectangle(box, radius=K * 10, fill=(255, 255, 255, 250))
    d.line((box[0] + K * 4, box[1] + K * 4, cx(0.5), cx(0.53), box[2] - K * 4, box[1] + K * 4), fill=hexc("#1a73f0"), width=K * 5, joint="curve")
    finish(over(img, lay), "mail")


def notes():
    img = body("#fff8d6", "#ffd84a")
    lay = layer()
    d = ImageDraw.Draw(lay)
    d.rectangle((O, O + B * 0.0, O + B, cx(0.24)), fill=hexc("#ffcc00"))
    for i in range(5):
        y = cx(0.36 + i * 0.11)
        d.line((cx(0.2), y, cx(0.8 - (0.22 if i == 4 else 0)), y), fill=(200, 170, 80, 255), width=K * 3)
    img = over(img, lay)
    m = Image.new("L", (S, S), 0)
    m.paste(MASK, (O, O))
    clipped = canvas()
    clipped.paste(img, (0, 0), m)
    base = body("#fff8d6", "#ffd84a")
    base.paste(img, (0, 0), m)
    finish(base, "notes")


def photos():
    img = body("#ffffff", "#f0f0f4")
    lay = layer()
    d = ImageDraw.Draw(lay)
    cols = ["#ff9500", "#ffcc00", "#34c759", "#30b0c7", "#007aff", "#5856d6", "#af52de", "#ff2d55"]
    c = cx(0.5)
    for i, col in enumerate(cols):
        a = 2 * math.pi * i / 8 - math.pi / 2
        px, py = c + math.cos(a) * B * 0.15, c + math.sin(a) * B * 0.15
        petal = layer()
        ImageDraw.Draw(petal).ellipse((px - B * 0.13, py - B * 0.13, px + B * 0.13, py + B * 0.13), fill=hexc(col, 200))
        lay.alpha_composite(petal)
    finish(over(img, lay), "photos")


def monitor():
    img = body("#3a3a3e", "#111113")
    lay = layer()
    d = ImageDraw.Draw(lay)
    for i in range(4):
        y = cx(0.25 + i * 0.17)
        d.line((cx(0.16), y, cx(0.84), y), fill=(255, 255, 255, 30), width=K * 2)
    pts = [(0.16, 0.66), (0.3, 0.6), (0.4, 0.7), (0.5, 0.36), (0.6, 0.52), (0.7, 0.3), (0.84, 0.42)]
    d.line([(cx(x), cx(y)) for x, y in pts], fill=hexc("#30d158"), width=K * 7, joint="curve")
    finish(over(img, lay), "monitor")


def appstore():
    img = body("#3fc3ff", "#0a66f5")
    lay = layer()
    d = ImageDraw.Draw(lay)
    w = K * 11
    d.line((cx(0.5), cx(0.22), cx(0.28), cx(0.74)), fill="white", width=w)
    d.line((cx(0.5), cx(0.22), cx(0.72), cx(0.74)), fill="white", width=w)
    d.line((cx(0.22), cx(0.6), cx(0.78), cx(0.6)), fill="white", width=w)
    finish(over(img, lay), "appstore")


def overview():
    img = body("#8a8fff", "#3d43d6")
    lay = layer()
    d = ImageDraw.Draw(lay)
    for r in range(2):
        for q in range(2):
            x, y = cx(0.2 + q * 0.32), cx(0.24 + r * 0.28)
            d.rounded_rectangle((x, y, x + cx(0.28) - O, y + cx(0.22) - O), radius=K * 8, fill=(255, 255, 255, 230 if (r + q) % 2 == 0 else 160))
    finish(over(img, lay), "overview")


def wallpaper():
    img = body("#a78bfa", "#4338ca")
    lay = layer()
    d = ImageDraw.Draw(lay)
    d.rounded_rectangle((cx(0.18), cx(0.24), cx(0.82), cx(0.76)), radius=K * 10, outline=(255, 255, 255, 240), width=K * 6)
    d.arc((cx(0.1), cx(0.42), cx(0.9), cx(1.1)), 200, 340, fill=(255, 255, 255, 220), width=K * 6)
    finish(over(img, lay), "wallpaper")


def maps():
    img = body("#9be15d", "#2fb36b")
    lay = layer()
    d = ImageDraw.Draw(lay)
    d.line((cx(0.1), cx(0.72), cx(0.9), cx(0.36)), fill=(255, 255, 255, 220), width=K * 14)
    d.line((cx(0.1), cx(0.72), cx(0.9), cx(0.36)), fill=hexc("#ffcc00"), width=K * 4)
    pin = (cx(0.42), cx(0.42))
    d.ellipse((pin[0] - K * 22, pin[1] - K * 22, pin[0] + K * 22, pin[1] + K * 22), fill=hexc("#ff3b30"))
    d.polygon([(pin[0] - K * 16, pin[1] + K * 12), (pin[0] + K * 16, pin[1] + K * 12), (pin[0], pin[1] + K * 40)], fill=hexc("#ff3b30"))
    d.ellipse((pin[0] - K * 8, pin[1] - K * 8, pin[0] + K * 8, pin[1] + K * 8), fill="white")
    finish(over(img, lay), "maps")


if __name__ == "__main__":
    for f in (files, downloads, folder_plain, trash, launchpad, browser, terminal, editor, viewer, calculator,
              settings, music, mail, notes, photos, monitor, appstore, overview, wallpaper, maps):
        f()
