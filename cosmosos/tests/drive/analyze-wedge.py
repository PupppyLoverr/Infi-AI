#!/usr/bin/env python3
"""Wedge detector: for each ab-*.png, measure whether window bodies show
wallpaper pixels (transparent wedge) vs painted content. Reports per-frame
verdict + a tally. Bodies probed at fixed regions matching the two cascade
spots the drive clicks between."""
import sys, glob, os
import numpy as np
from PIL import Image

def probe(path):
    img = np.asarray(Image.open(path).convert("L"), dtype=int)
    # wallpaper reference: top-center bare desktop band
    wall = img[60:100, 430:600].mean()
    # body probes: left-window body center, right-window body center
    left  = img[200:500, 100:520].mean()
    right = img[180:480, 620:1000].mean()
    return wall, left, right

rows = []
for f in sorted(glob.glob(os.path.join(sys.argv[1], "ab-*.png"))):
    wall, left, right = probe(f)
    # painted terminal body ~ lum 5-25 w/ content variance; wallpaper ~24-35 smooth
    # heuristic: a body region whose mean is within ~4 lum of wallpaper and
    # has LOW stddev-ish range is transparent. Use brightness gap instead:
    wedged = []
    for name, v in (("L", left), ("R", right)):
        if abs(v - wall) < 3.5 and v > 15:
            wedged.append(name)
    rows.append((os.path.basename(f), wall, left, right, "".join(wedged) or "-"))

wedge_ct = sum(1 for r in rows if r[4] != "-")
for r in rows:
    print(f"{r[0]:28s} wall={r[1]:5.1f} L={r[2]:5.1f} R={r[3]:5.1f} wedge={r[4]}")
print(f"\nwedged frames: {wedge_ct}/{len(rows)}")
