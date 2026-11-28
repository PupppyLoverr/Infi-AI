#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-only
"""Convert the hosted macOS's framebuffer dump (xnu.ko fb_dump: raw 32-bit BGRA rows) to a PNG.

    fb2png.py DUMP WIDTH HEIGHT OUT.png
"""
import struct, sys, zlib

src, w, h, out = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), sys.argv[4]
d = open(src, 'rb').read()
if len(d) < w * h * 4:
    sys.exit(f'{src}: {len(d)} bytes, expected {w * h * 4}')
rows = bytearray()
for y in range(h):
    row = d[y * w * 4:(y + 1) * w * 4]
    rgb = bytearray(w * 3)
    rgb[0::3] = row[2::4]   # R
    rgb[1::3] = row[1::4]   # G
    rgb[2::3] = row[0::4]   # B
    rows += b'\0' + rgb
def chunk(t, b):
    return struct.pack('>I', len(b)) + t + b + struct.pack('>I', zlib.crc32(t + b) & 0xffffffff)
png = b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 2, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(bytes(rows), 6)) + chunk(b'IEND', b'')
open(out, 'wb').write(png)
