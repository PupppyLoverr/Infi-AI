#!/usr/bin/env python3
"""Side-by-side reference comparisons (v5 §5).

Usage: compare.py [EVIDENCE_DIR] [REFERENCE_DIR]

Writes v5-compare-ref-N.png: the reference on the left and our matching
capture on the right, both scaled to the same height on a neutral strip.
Pairs whose reference or capture is missing are reported and skipped.
"""
import os
import sys

from PIL import Image

PAIRS = {1: "v5-01", 2: "v5-15", 3: "v5-04", 4: "v5-13", 5: "v5-11", 6: "v5-01", 7: "v5-06"}
HEIGHT, GUTTER, BG = 720, 24, (128, 128, 132)


def fit(img):
    w = round(img.width * HEIGHT / img.height)
    return img.convert("RGB").resize((w, HEIGHT), Image.LANCZOS)


def main():
    here = os.path.dirname(os.path.abspath(__file__))
    ev = sys.argv[1] if len(sys.argv) > 1 else here
    refs = sys.argv[2] if len(sys.argv) > 2 else os.path.join(here, "..", "reference")
    for n, shot in PAIRS.items():
        ref_path = os.path.join(refs, f"ref-{n}.png")
        shot_path = os.path.join(ev, f"{shot}.png")
        missing = [p for p in (ref_path, shot_path) if not os.path.exists(p)]
        if missing:
            print(f"ref-{n}: skipped, missing {', '.join(os.path.basename(p) for p in missing)}")
            continue
        left, right = fit(Image.open(ref_path)), fit(Image.open(shot_path))
        out = Image.new("RGB", (left.width + right.width + 3 * GUTTER, HEIGHT + 2 * GUTTER), BG)
        out.paste(left, (GUTTER, GUTTER))
        out.paste(right, (left.width + 2 * GUTTER, GUTTER))
        dst = os.path.join(ev, f"v5-compare-ref-{n}.png")
        out.save(dst, optimize=True)
        print(f"ref-{n}: {os.path.basename(dst)} ({out.width}x{out.height})")


if __name__ == "__main__":
    main()
