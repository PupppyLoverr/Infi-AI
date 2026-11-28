#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-only
# In [lo, hi) of a fileset KC: "ldr xA, [xB, #off]" followed within 13 instructions by "mov x17, #imm" and
# "blraa xA, x17" (a call through a type-discriminated function pointer field), xA not loaded again in between.
# Prints off, imm, base reg, address.
import struct, sys, collections
path, lo, hi = sys.argv[1], int(sys.argv[2], 16), int(sys.argv[3], 16)
d = open(path, 'rb').read()
ncmds = struct.unpack_from('<I', d, 16)[0]; off = 32
hits = collections.defaultdict(set)
for _ in range(ncmds):
    cmd, sz = struct.unpack_from('<II', d, off)
    if cmd == 0x19:
        vmaddr, vmsize, fileoff, filesize = struct.unpack_from('<QQQQ', d, off + 24)
        a = max(lo, vmaddr); b = min(hi, vmaddr + filesize)
        if a < b:
            words = struct.unpack_from('<%dI' % ((b - a) // 4), d, fileoff + a - vmaddr)
            for i, w in enumerate(words):
                if (w & 0xffc00000) == 0xf9400000:          # ldr Xt, [Xn, #imm12*8]
                    rt, rn, o = w & 31, (w >> 5) & 31, ((w >> 10) & 0xfff) * 8
                    imm = None
                    for j in range(i + 1, min(i + 14, len(words))):
                        x = words[j]
                        if (x & 0xffc0001f) == 0xf9400000 | rt:     # xA loaded again: the call is through that load, not this one
                            break
                        if (x & 0xffe0001f) == 0xd2800011:  # movz x17, #imm16
                            imm = (x >> 5) & 0xffff
                        if (x & 0xfffffc1f) == (0xd73f0800 | 0) and ((x >> 5) & 31) == rt and imm is not None:
                            pass
                        if (x & 0xfffffc00) == 0xd73f0800 and ((x >> 5) & 31) == rt and (x & 31) == 17 and imm is not None:
                            hits[o].add(imm); print('%x off 0x%x disc 0x%x base x%d' % (a + 4 * i, o, imm, rn)); break
    off += sz
print('---')
for o in sorted(hits):
    print('0x%x %s' % (o, ' '.join('0x%x' % v for v in sorted(hits[o]))))
