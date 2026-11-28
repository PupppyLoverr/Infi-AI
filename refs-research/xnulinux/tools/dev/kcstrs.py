#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-only
# For adrp+add pairs in [va, va+len) of a fileset KC: the target address and the C string there (debugging aid).
import struct, sys
path, va0, n = sys.argv[1], int(sys.argv[2], 16), int(sys.argv[3], 0)
d = open(path, 'rb').read()
segs = []
ncmds = struct.unpack_from('<I', d, 16)[0]; off = 32
for _ in range(ncmds):
    cmd, sz = struct.unpack_from('<II', d, off)
    if cmd == 0x19:
        segs.append(struct.unpack_from('<QQQQ', d, off + 24))
    off += sz
def rd(va, k):
    for vmaddr, vmsize, fileoff, filesize in segs:
        if vmaddr <= va < vmaddr + filesize:
            return d[fileoff + va - vmaddr: fileoff + va - vmaddr + k]
    return None
regs = {}
for i in range(0, n, 4):
    va = va0 + i
    w = rd(va, 4)
    if w is None or len(w) < 4:
        print('%x: va not found' % va); break
    w = struct.unpack('<I', w)[0]
    if (w & 0x9f000000) == 0x90000000:   # adrp
        rd_ = w & 31; immlo = (w >> 29) & 3; immhi = (w >> 5) & 0x7ffff
        imm = (immhi << 2) | immlo
        if imm & (1 << 20): imm -= 1 << 21
        regs[rd_] = (va & ~0xfff) + (imm << 12)
    elif (w & 0xffc00000) == 0x91000000:  # add xd, xn, #imm12
        rd_, rn = w & 31, (w >> 5) & 31; imm = (w >> 10) & 0xfff
        if rn in regs:
            t = regs[rd_] = regs[rn] + imm      # (xd holds that address now: an add from it is from there, not from the page)
            s = rd(t, 96) or b''
            s = s.split(b'\0')[0]
            pr = s.decode('latin1') if s and all(32 <= c < 127 for c in s) else ''
            print('%x: x%d = %x %s' % (va, rd_, t, repr(pr) if pr else ''))
