#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-only
# Find BL callers of a target in [lo, hi) of a fileset KC and print the instruction words after each (debugging aid).
import struct, sys
path, lo, hi, target = sys.argv[1], int(sys.argv[2], 16), int(sys.argv[3], 16), int(sys.argv[4], 16)
d = open(path, 'rb').read()
ncmds = struct.unpack_from('<I', d, 16)[0]; off = 32
for _ in range(ncmds):
    cmd, sz = struct.unpack_from('<II', d, off)
    if cmd == 0x19:
        vmaddr, vmsize, fileoff, filesize = struct.unpack_from('<QQQQ', d, off + 24)
        a = max(lo, vmaddr); b = min(hi, vmaddr + filesize)
        for va in range(a & ~3, b, 4):
            w = struct.unpack_from('<I', d, fileoff + va - vmaddr)[0]
            if (w & 0xfc000000) == 0x94000000:
                imm = w & 0x3ffffff
                if imm & 0x2000000: imm -= 1 << 26
                if va + imm * 4 == target:
                    print('%x' % va)
    off += sz
