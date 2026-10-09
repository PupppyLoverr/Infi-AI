#!/usr/bin/env python3
"""Paint four original landscape photos as PNGs with only the standard library.

Usage: mkphotos.py OUT_DIR

Each scene is drawn from gradients, layered sine ridges and seeded noise, so
the image build needs no assets and no imaging libraries.
"""
import math
import os
import random
import struct
import sys
import zlib

W, H = 1280, 800


def png(path, rows):
    raw = b"".join(b"\x00" + bytes(r) for r in rows)

    def chunk(kind, data):
        return (struct.pack(">I", len(data)) + kind + data
                + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF))

    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n")
        f.write(chunk(b"IHDR", struct.pack(">IIBBBBB", W, H, 8, 2, 0, 0, 0)))
        f.write(chunk(b"IDAT", zlib.compress(raw, 6)))
        f.write(chunk(b"IEND", b""))


def mix(a, b, t):
    t = min(max(t, 0.0), 1.0)
    return [a[i] + (b[i] - a[i]) * t for i in range(3)]


def ramp(stops, t):
    for (t0, c0), (t1, c1) in zip(stops, stops[1:]):
        if t <= t1:
            return mix(c0, c1, (t - t0) / (t1 - t0 or 1))
    return stops[-1][1]


def ridge(seed, base, amp, waves=5):
    rnd = random.Random(seed)
    parts = [(rnd.uniform(0.6, 1.4) * (k + 1) * 0.004, rnd.uniform(0, 6.3), amp / (k + 1) ** 0.9)
             for k in range(waves)]
    return [base + sum(a * math.sin(x * f + p) for f, p, a in parts) for x in range(W)]


def canvas(sky):
    return [[c for _ in range(W) for c in (int(v) for v in sky(y))] for y in range(H)]


def put(rows, x, y, col, alpha=1.0):
    if 0 <= x < W and 0 <= y < H:
        r, i = rows[y], x * 3
        for k in range(3):
            r[i + k] = int(r[i + k] + (col[k] - r[i + k]) * alpha)


def fill_below(rows, heights, col_at):
    for x, h in enumerate(heights):
        for y in range(max(int(h), 0), H):
            c = col_at(x, y, h)
            r, i = rows[y], x * 3
            r[i], r[i + 1], r[i + 2] = int(c[0]), int(c[1]), int(c[2])


def glow(rows, cx, cy, radius, col, strength):
    for y in range(max(cy - radius, 0), min(cy + radius, H)):
        for x in range(max(cx - radius, 0), min(cx + radius, W)):
            d = math.hypot(x - cx, y - cy) / radius
            if d < 1:
                put(rows, x, y, col, strength * (1 - d) ** 2)


def dunes():
    sky = [(0, (54, 40, 92)), (0.45, (196, 102, 98)), (0.62, (247, 176, 104)), (1, (250, 214, 150))]
    rows = canvas(lambda y: ramp(sky, y / (H * 0.62)))
    glow(rows, 860, 470, 260, (255, 226, 170), 0.55)
    glow(rows, 860, 470, 46, (255, 246, 222), 1.0)
    layers = [(410, (196, 116, 84)), (480, (164, 86, 66)), (560, (122, 60, 54)), (650, (78, 38, 44))]
    for n, (base, col) in enumerate(layers):
        hs = ridge(11 + n, base, 26 + n * 8)
        fill_below(rows, hs, lambda x, y, h, c=col: mix(c, [v * 0.72 for v in c], (y - h) / 220))
    return rows


def lake():
    sky = [(0, (118, 150, 196)), (0.6, (226, 196, 190)), (1, (246, 214, 186))]
    rows = canvas(lambda y: ramp(sky, y / (H * 0.55)))
    horizon = int(H * 0.55)
    far = ridge(21, horizon - 70, 34, 6)
    near = ridge(22, horizon - 24, 22, 6)
    fill_below(rows, far, lambda x, y, h: (120, 128, 156))
    fill_below(rows, near, lambda x, y, h: (72, 82, 108))
    rnd = random.Random(23)
    for y in range(horizon, H):
        src = horizon - (y - horizon) - 1
        shade = 0.82 - 0.25 * (y - horizon) / (H - horizon)
        for x in range(W):
            sx = min(max(x + int(3 * math.sin(y * 0.35 + x * 0.01)), 0), W - 1)
            s = rows[max(src, 0)][sx * 3: sx * 3 + 3]
            put(rows, x, y, [v * shade + 18 * (1 - shade) for v in s])
        if rnd.random() < 0.35:
            x0 = rnd.randrange(W)
            for x in range(x0, min(x0 + rnd.randrange(40, 160), W)):
                put(rows, x, y, (250, 236, 220), 0.18)
    return rows


def forest():
    sky = [(0, (182, 208, 206)), (1, (226, 232, 220))]
    rows = canvas(lambda y: ramp(sky, y / H))
    tones = [(250, (150, 178, 172)), (340, (112, 146, 138)), (430, (76, 112, 102)),
             (520, (46, 80, 70)), (620, (26, 52, 46))]
    for n, (base, col) in enumerate(tones):
        hs = ridge(31 + n, base, 40 + n * 6, 7)
        rnd, ground = random.Random(40 + n), list(hs)
        for x in range(0, W, 4):
            if rnd.random() < 0.55:
                tree, half = rnd.uniform(10, 22 + n * 7), 4 + n
                for dx in range(-half, half + 1):
                    if 0 <= x + dx < W:
                        top = ground[x] - tree * (1 - abs(dx) / (half + 1))
                        hs[x + dx] = min(hs[x + dx], top)
        fill_below(rows, hs, lambda x, y, h, c=col, g=ground: mix(c, (226, 232, 220), max(0, 0.18 - (y - g[x]) / 900)))
    return rows


def night():
    sky = [(0, (6, 10, 28)), (0.7, (16, 28, 62)), (1, (30, 46, 86))]
    rows = canvas(lambda y: ramp(sky, y / H))
    rnd = random.Random(51)
    for _ in range(900):
        x, y, b = rnd.randrange(W), rnd.randrange(int(H * 0.75)), rnd.uniform(0.3, 1)
        put(rows, x, y, (236, 240, 255), b)
    for x in range(W):
        top = 150 + 70 * math.sin(x * 0.0042 + 0.8) + 25 * math.sin(x * 0.013)
        flicker = 0.55 + 0.45 * math.sin(x * 0.09) * math.sin(x * 0.023 + 1.3)
        for y in range(int(top), int(top) + 260):
            t = (y - top) / 260
            col = mix((120, 255, 190), (60, 120, 220), t)
            put(rows, x, y, col, 0.42 * flicker * min(1.0, t * 6) * (1 - t) ** 1.5)
    hs = ridge(52, H * 0.78, 30, 5)
    fill_below(rows, hs, lambda x, y, h: (8, 12, 20))
    return rows


SCENES = [("Dunes at dusk", dunes), ("Lake at dawn", lake),
          ("Forest ridges", forest), ("Aurora night", night)]

if __name__ == "__main__":
    out = sys.argv[1]
    os.makedirs(out, exist_ok=True)
    for name, paint in SCENES:
        png(os.path.join(out, f"{name}.png"), paint())
