#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-only
"""Compares two kernel collections: their fixups (decoded), their fileset entries' segments and sections, the bytes of
each entry's segments with pointers taken out, and their prelink info. For checking kclink against another linker.

    kccompare.py A B

Its exit status is 1 when they differ."""
import plistlib
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from macho import Image, kc_read_fixups  # noqa: E402


def load(path):
    buf = open(path, 'rb').read()
    img = Image(buf)
    hdr = img.segments[0].vmaddr
    fx = kc_read_fixups(buf, hdr, None)
    ents = {n: Image(buf, fo) for n, va, fo in img.entries}
    pi = img.segment('__PRELINK_INFO')
    info = plistlib.loads(buf[pi.fileoff:pi.fileoff + pi.filesize].rstrip(b'\0'))
    return buf, img, hdr, fx, ents, info


def main():
    a, b = sys.argv[1], sys.argv[2]
    A, B = load(a), load(b)
    bufa, imga, hdra, fxa, ea, ia = A
    bufb, imgb, hdrb, fxb, eb, ib = B
    print(f'header {hdra:#x} / {hdrb:#x}; {len(fxa)} / {len(fxb)} pointers; {len(ea)} / {len(eb)} entries')
    problems = 0                # everything said below is a difference and counts as one
    for sa, sb in zip(imga.segments, imgb.segments):
        if (sa.name, sa.vmaddr, sa.vmsize) != (sb.name, sb.vmaddr, sb.vmsize):
            print(f'  segment {sa.name} {sa.vmaddr:#x}+{sa.vmsize:#x} / {sb.name} {sb.vmaddr:#x}+{sb.vmsize:#x}')
            problems += 1
    only = set(ea) ^ set(eb)
    if only:
        print('  entries in one only:', sorted(only))
        problems += len(only)
    for name in sorted(set(ea) & set(eb)):
        xa, xb = ea[name], eb[name]
        segs_a = [(s.name, s.vmaddr, s.vmsize) for s in xa.segments if s.name != '__LINKEDIT']
        segs_b = [(s.name, s.vmaddr, s.vmsize) for s in xb.segments if s.name != '__LINKEDIT']
        if segs_a != segs_b:
            print(f'{name}: segments differ: {segs_a} / {segs_b}')
            problems += 1
            continue
        secs_a = [(s.segname, s.sectname, s.addr, s.size, s.flags) for s in xa.sections()]
        secs_b = [(s.segname, s.sectname, s.addr, s.size, s.flags) for s in xb.sections()]
        if secs_a != secs_b:
            print(f'{name}: sections differ: {set(secs_a) ^ set(secs_b)}')
            problems += 1
        for s in xa.segments:
            if s.name == '__LINKEDIT' or not s.filesize:
                continue
            o = s.vmaddr - hdra
            bad_ptr, bad_bytes = [], []
            ptr = set()
            for loc in range(o, o + s.filesize, 4):
                fa, fb = fxa.get(loc), fxb.get(loc)
                if fa or fb:
                    ptr.add(loc)
                    ptr.add(loc + 4)
                    if not fa or not fb or (fa.target, fa.key, fa.div, fa.addr_div) != \
                            (fb.target, fb.key, fb.div, fb.addr_div):
                        bad_ptr.append((loc, fa, fb))
            skip_hdr = 0
            if s.name == '__TEXT':          # load commands: compared above
                skip_hdr = 32 + struct.unpack_from('<I', bufa, o + 20)[0]
            for loc in range(o + skip_hdr, o + s.filesize, 4):
                if loc not in ptr and bufa[loc:loc + 4] != bufb[loc:loc + 4]:
                    bad_bytes.append(loc)
            if bad_ptr or bad_bytes:
                problems += 1
                print(f'{name} {s.name}: {len(bad_ptr)} pointers and {len(bad_bytes)} words differ;'
                      f' first pointers {[(hex(l), x, y) for l, x, y in bad_ptr[:3]]}'
                      f' first words {[hex(l) for l in bad_bytes[:6]]}')
    da = {d['CFBundleIdentifier']: d for d in ia['_PrelinkInfoDictionary']}
    db = {d['CFBundleIdentifier']: d for d in ib['_PrelinkInfoDictionary']}
    if set(da) != set(db):
        print('prelink info: entries in one only:', sorted(set(da) ^ set(db)))
        problems += len(set(da) ^ set(db))
    for k in sorted(set(da) & set(db)):
        if da[k] != db[k]:
            keys = sorted(x for x in set(da[k]) | set(db[k]) if da[k].get(x) != db[k].get(x))
            print(f'prelink info {k}: {[(x, da[k].get(x), db[k].get(x)) for x in keys]}'[:400])
            problems += 1
    if {x: y for x, y in ia.items() if x != '_PrelinkInfoDictionary'} != \
            {x: y for x, y in ib.items() if x != '_PrelinkInfoDictionary'}:
        print('prelink info: top-level keys differ')
        problems += 1
    print(f'{problems} differences')
    return problems


if __name__ == '__main__':
    sys.exit(1 if main() else 0)
