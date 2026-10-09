#!/usr/bin/env python3
"""WCAG 2 contrast measured on real screenshots (v5 §1.2).

Usage: contrast.py regions.json > v5-contrast.txt

regions.json: [{"shot": "v5-04.png", "name": "Files sidebar header",
                "box": [x, y, w, h], "role": "primary|secondary|tertiary"}]
Inside each box the background is the median colour and the text colour
is the pixel farthest in luminance from it, so a box should hug one
text run plus its surrounding fill.
"""
import json
import os
import sys

from PIL import Image

MIN = {"primary": 7.0, "secondary": 4.5, "tertiary": 3.0}


def lin(c):
    c /= 255
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def lum(rgb):
    r, g, b = (lin(v) for v in rgb[:3])
    return 0.2126 * r + 0.7152 * g + 0.0722 * b


def ratio(a, b):
    la, lb = lum(a), lum(b)
    return (max(la, lb) + 0.05) / (min(la, lb) + 0.05)


def measure(img, box):
    x, y, w, h = box
    px = list(img.crop((x, y, x + w, y + h)).convert("RGB").getdata())
    by_l = sorted(px, key=lum)
    bg = by_l[len(by_l) // 2]
    fg = max(px, key=lambda p: abs(lum(p) - lum(bg)))
    return fg, bg, ratio(fg, bg)


def main():
    regions = json.load(open(sys.argv[1]))
    base = os.path.dirname(os.path.abspath(sys.argv[1]))
    shots, fails = {}, 0
    print(f"{'shot':28} {'region':34} {'role':9} {'fg':>13} {'bg':>13} ratio  min  result")
    for r in regions:
        img = shots.setdefault(r["shot"], Image.open(os.path.join(base, r["shot"])))
        fg, bg, cr = measure(img, r["box"])
        need = MIN[r["role"]]
        ok = cr >= need
        fails += not ok
        print(f"{r['shot']:28} {r['name']:34} {r['role']:9} {str(fg):>13} {str(bg):>13} "
              f"{cr:5.2f} {need:4.1f}  {'PASS' if ok else 'FAIL'}")
    print(f"\n{len(regions) - fails}/{len(regions)} pass")
    sys.exit(1 if fails else 0)


if __name__ == "__main__":
    main()
