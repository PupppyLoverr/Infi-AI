#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-only
"""Set the lowest code-signature platform version AMFI accepts in a built kernel collection (in place).

The hosted kernel links Apple's kexts from one macOS build (the collection) while the system disk can be another: AMFI
from macOS 27.2 expects platform binaries to carry platform version 27 in their code directory, and the binaries of a
macOS 27.0.1 disk carry 26. AMFI then judges e.g. system_installd invalid ("platformVersion 26 is lower than expected
version 27"): the process runs without CS_VALID and services that check its signature refuse it.

AMFI keeps the expected version in a 32-bit global, read by a small accessor that the check calls right before it
logs that message. This finds the message, the accessor called just before its use, the global the accessor loads, and
writes the given version there. It changes nothing when the value is already the requested one.

It also turns off AMFI's version restriction, which the same mismatch trips: 27.2's AMFI refuses to run some Apple
programs unless they carry a com.apple.private.amfi.version-restriction entitlement of at least a given version (a table
of signing identifiers and versions, e.g. com.apple.SecurityAgent). Programs of an older macOS do not carry it, and AMFI
kills them at exec ("com.apple.SecurityAgent disallowed without correct ... entitlement version"); SecurityAgent runs
loginwindow's login mechanisms, so no graphical session starts. The check function starts with a fast path that allows
every process without one flag (`bti c; tbnz w1, #N, slow; mov w0, #1; ret`); the tbnz becomes a nop, so the check
always allows.

    kc-amfi-platform.py KC VERSION
"""
import struct
import sys

MESSAGE = b'%s: platformVersion %u is lower than expected version %u.'
VR_MESSAGE = b'%s: %s disallowed without correct com.apple.private.amfi.version-restriction entitlement version'
NOP = 0xd503201f


def segments(d):
    out = []
    ncmds = struct.unpack_from('<I', d, 16)[0]
    off = 32
    for _ in range(ncmds):
        cmd, size = struct.unpack_from('<II', d, off)
        if cmd == 0x19:  # LC_SEGMENT_64
            name = d[off + 8:off + 24].rstrip(b'\0').decode()
            vmaddr, _vmsize, fileoff, filesize = struct.unpack_from('<QQQQ', d, off + 24)
            out.append((name, vmaddr, fileoff, filesize))
        off += size
    return out


def va_to_file(segs, va):
    for _n, v, fo, fs in segs:
        if v <= va < v + fs:
            return fo + va - v
    return None


def file_to_va(segs, fo):
    for _n, v, f0, fs in segs:
        if f0 <= fo < f0 + fs:
            return v + fo - f0
    return None


def word(d, fo):
    return struct.unpack_from('<I', d, fo)[0]


def adrp_page(w, pc):
    imm = ((w >> 29) & 3) | (((w >> 5) & 0x7ffff) << 2)
    if imm & (1 << 20):
        imm -= 1 << 21
    return (pc & ~0xfff) + (imm << 12)


def string_xrefs(d, segs, target):
    """adrp xN + add xN, xN, #lo pairs in executable segments that form target."""
    for name, v, f0, fs in segs:
        if 'TEXT_EXEC' not in name:
            continue
        for fo in range(f0, f0 + fs - 8, 4):
            w = word(d, fo)
            if (w & 0x9f000000) != 0x90000000:
                continue
            pc = v + fo - f0
            page = adrp_page(w, pc)
            if page != target & ~0xfff:
                continue
            rd = w & 31
            w2 = word(d, fo + 4)
            if (w2 & 0xffc00000) == 0x91000000 and (w2 >> 5) & 31 == rd and page + ((w2 >> 10) & 0xfff) == target:
                yield pc, fo


def accessor_global(d, segs, fva):
    """The global a `bti c; adrp x8, P; ldr w0, [x8, #off]; ret` accessor at fva loads, or None."""
    fo = va_to_file(segs, fva)
    if fo is None:
        return None
    w0, w1, w2, w3 = (word(d, fo + 4 * i) for i in range(4))
    if w0 != 0xd503245f or w3 != 0xd65f03c0:  # bti c ... ret
        return None
    if (w1 & 0x9f00001f) != 0x90000008:  # adrp x8
        return None
    if (w2 & 0xffc003ff) != 0xb9400100:  # ldr w0, [x8, #imm]
        return None
    return adrp_page(w1, fva + 4) + ((w2 >> 10) & 0xfff) * 4


def version_restriction_check(d, segs):
    """File offset of the version-restriction check's fast-path tbnz (or of the nop already there), or None."""
    at = d.find(VR_MESSAGE + b'\0')
    if at < 0:
        return None
    found = set()
    for pc, fo in string_xrefs(d, segs, file_to_va(segs, at)):
        for back in range(1, 8):  # bl check; tbnz w0, #0, allowed; ... adrp x3, message
            w = word(d, fo - 4 * back)
            if (w & 0xfc000000) != 0x94000000:
                continue
            imm = w & 0x3ffffff
            if imm & 0x2000000:
                imm -= 1 << 26
            ffo = va_to_file(segs, pc - 4 * back + imm * 4)
            if ffo is None or word(d, ffo) != 0xd503245f:  # bti c
                continue
            t, m, r = word(d, ffo + 4), word(d, ffo + 8), word(d, ffo + 12)
            if m == 0x52800020 and r == 0xd65f03c0 and ((t & 0x7f00001f) == 0x37000001 or t == NOP):  # tbnz w1 / nop; mov w0, #1; ret
                found.add(ffo + 4)
    return found.pop() if len(found) == 1 else None


def main():
    path, version = sys.argv[1], int(sys.argv[2])
    d = bytearray(open(path, 'rb').read())
    segs = segments(d)
    at = d.find(MESSAGE + b'\0')
    if at < 0:
        sys.exit('kc-amfi-platform: AMFI platform version message not found')
    globals_found = set()
    for pc, fo in string_xrefs(d, segs, file_to_va(segs, at)):
        for back in range(1, 12):  # the accessor is called a few instructions before the message is loaded
            w = word(d, fo - 4 * back)
            if (w & 0xfc000000) != 0x94000000:  # bl
                continue
            imm = w & 0x3ffffff
            if imm & 0x2000000:
                imm -= 1 << 26
            g = accessor_global(d, segs, pc - 4 * back + imm * 4)
            if g is not None:
                globals_found.add(g)
    if len(globals_found) != 1:
        sys.exit('kc-amfi-platform: expected one AMFI platform version global, found %d' % len(globals_found))
    g = globals_found.pop()
    gfo = va_to_file(segs, g)
    old = struct.unpack_from('<I', d, gfo)[0]
    if not 10 <= old <= 60:
        sys.exit('kc-amfi-platform: implausible expected platform version %d at %#x' % (old, g))
    changed = old != version
    if changed:
        struct.pack_into('<I', d, gfo, version)
    print('kc-amfi-platform: AMFI expected platform version at %#x: %d -> %d' % (g, old, version))
    vr = version_restriction_check(d, segs)
    if vr is None:
        print('kc-amfi-platform: AMFI version-restriction check not found: left as it is')
    else:
        if word(d, vr) != NOP:
            struct.pack_into('<I', d, vr, NOP)
            changed = True
        print('kc-amfi-platform: AMFI version-restriction check at %#x: always allows' % file_to_va(segs, vr))
    if changed:
        open(path, 'wb').write(d)


if __name__ == '__main__':
    main()
