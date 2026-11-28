#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-only
"""Links a kernel collection (an MH_FILESET kernelcache) for xnu.ko: the hosted kernel, kexts linked from object files
(scripts/kclink/kextld.py), and kexts kept from another collection.

    kclink.py BASE KERNEL OUTPUT [--keep BUNDLE_ID...] [--native SPEC.json...] [--rebind ADDRESS=SYMBOL...]

BASE is the collection the output is laid out like: its segments (__PRELINK_TEXT, __DATA_CONST, __TEXT_EXEC,
__PRELINK_INFO, __DATA, __LINKEDIT), its kexts (those --keep names, with every kext they depend on, stay where they
are, byte for byte but for their pointers) and its prelink info. It is a macOS kernelcache (scripts/apple-kc/link.sh) or
an empty collection made from numbers (scripts/free-kc/mk-template.py).

KERNEL is the kernel, linked at the addresses of its slots in the collection (threaded rebases, __TEXT,__thread_starts).
The output's addresses are the kernel's: the collection's header sits as far below the kernel's __TEXT as in BASE.

Every pointer goes into the collection's chained fixups (DYLD_CHAINED_PTR_64_KERNEL_CACHE). A kept kext's pointers into
BASE's kernel go to the same symbol in KERNEL (BASE's kernel symbol table names them); --rebind sends the pointer at
ADDRESS (in BASE) to another of KERNEL's symbols.

A native kext's spec (JSON): name, bundle, version, objects (a JSON list of object files; relative paths are from the
directory kclink runs in), start, stop, libraries, personalities, plist (more Info.plist keys), patch_personalities
({bundle: {personality: {key: value}}}, merged into a kept kext's IOKitPersonalities). Its segments go into space no
kernel or kept kext uses, first fit. Its symbols, as linked, are written to OUTPUT with suffix .natives.json."""
import argparse
import base64
import json
import os
import plistlib
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from macho import (HEADER, LC_SEGMENT_64, LC_SYMTAB, LC_DYSYMTAB, LC_UNIXTHREAD, LC_UUID, LC_SEGMENT_SPLIT_INFO,  # noqa
                   LC_FUNCTION_STARTS, LC_DATA_IN_CODE, LC_DYLD_CHAINED_FIXUPS, LC_FILESET_ENTRY, MH_FILESET,
                   N_STAB, N_TYPE, N_SECT, N_EXT, NLIST, KC_PAGE, Fixup, Image, Segment, align, kc_read_fixups,
                   kc_write_fixups, threaded_rebases)
import kextld  # noqa: E402

KERNEL_ID = 'com.apple.kernel'
MH_DYLIB_IN_CACHE = 0x80000000
# the collection's segment each of the kernel's goes into
KERNEL_REGION = {'__TEXT': '__PRELINK_TEXT', '__DATA_CONST': '__DATA_CONST', '__LASTDATA_CONST': '__DATA_CONST',
                 '__TEXT_EXEC': '__TEXT_EXEC', '__KLD': '__TEXT_EXEC', '__LAST': '__TEXT_EXEC',
                 '__KLDDATA': '__DATA', '__DATA': '__DATA', '__BOOTDATA': '__DATA', '__LINKINFO': '__LINKEDIT'}
KEXT_REGION = {'__TEXT': '__PRELINK_TEXT', '__TEXT_EXEC': '__TEXT_EXEC', '__DATA_CONST': '__DATA_CONST',
               '__DATA': '__DATA', '__LINKINFO': '__LINKEDIT'}
ADDRESS_KEYS = ('_PrelinkExecutableLoadAddr', '_PrelinkExecutableSourceAddr', '_PrelinkKmodInfo')
NO_ADDRESS = 0x7fffffffffffffff         # a codeless kext's _PrelinkExecutableLoadAddr
KERNEL_DROPPED_CMDS = {LC_SEGMENT_SPLIT_INFO, LC_FUNCTION_STARTS, LC_DATA_IN_CODE, 0x1d}   # 0x1d: LC_CODE_SIGNATURE


def fail(msg):
    sys.exit(f'kclink: {msg}')


def u64(v):
    return v & 0xffffffffffffffff


def prelink_xml(info):
    """The prelink info as XML with nothing between its elements, the way the kernel writes a dictionary itself
    (OSSerialize). plistlib indents every element with a tab per level and puts it on a line of its own, which a
    kernel's prelink info does not have the room for: it ends where the kernel's __KLDDATA starts. The values are
    those plistlib writes (the same elements, the same order of keys); what comes out is read back and compared."""
    def esc(text):
        text = text.replace('\r\n', '\n').replace('\r', '\n')       # as plistlib writes a string
        return text.replace('&', '&amp;').replace('<', '&lt;').replace('>', '&gt;')

    def xml(v):
        if isinstance(v, bool):
            return '<true/>' if v else '<false/>'
        if isinstance(v, int):
            if not -1 << 63 <= v < 1 << 64:
                fail(f'prelink info: integer {v} out of range')
            return f'<integer>{v}</integer>'
        if isinstance(v, str):
            return f'<string>{esc(v)}</string>'
        if isinstance(v, (bytes, bytearray)):
            return f'<data>{base64.b64encode(v).decode()}</data>'
        if isinstance(v, dict):
            if not all(isinstance(k, str) for k in v):
                fail('prelink info: a dictionary key that is not a string')
            items = ''.join(f'<key>{esc(k)}</key>{xml(v[k])}' for k in sorted(v))
            return f'<dict>{items}</dict>' if v else '<dict/>'
        if isinstance(v, (list, tuple)):
            return f'<array>{"".join(xml(x) for x in v)}</array>' if v else '<array/>'
        fail(f'prelink info: a value of type {type(v).__name__}')

    indented = plistlib.dumps(info)
    head = indented[:indented.index(b'<plist')]                     # the XML declaration and the document type
    out = head + f'<plist version="1.0">{xml(info)}</plist>\n'.encode()
    if plistlib.loads(out) != plistlib.loads(indented):
        fail('prelink info: the compact form does not read back as the same dictionary')
    return out


def has_code(d):
    a = d.get('_PrelinkExecutableLoadAddr')
    return a is not None and a != NO_ADDRESS


class Holes:
    """Free ranges of one segment of the collection (offsets), handed out first fit."""

    def __init__(self, start, end):
        self.free = [(start, end)]

    def take(self, start, end):
        out = []
        for a, b in self.free:
            if end <= a or b <= start:
                out.append((a, b))
                continue
            if a < start:
                out.append((a, start))
            if end < b:
                out.append((end, b))
        self.free = out

    def fit(self, size, al):
        for a, b in self.free:
            s = align(a, al)
            if s + size <= b:
                self.take(s, s + size)
                return s
        return None


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    ap.add_argument('base')
    ap.add_argument('kernel')
    ap.add_argument('output')
    ap.add_argument('--keep', nargs='*', default=[])
    ap.add_argument('--native', nargs='*', default=[])
    ap.add_argument('--rebind', nargs='*', default=[], help='ADDRESS=SYMBOL: the pointer at ADDRESS (in BASE)')
    ap.add_argument('--reserve', nargs='*', default=[], help='SEGMENT:START-END: offsets no native kext goes to')
    args = ap.parse_args()

    bbuf = open(args.base, 'rb').read()
    base = Image(bbuf)
    if base.filetype != MH_FILESET:
        fail(f'{args.base}: not a kernel collection')
    old_hdr = base.segments[0].vmaddr
    regions = {}                        # name -> [start, end] (offsets from the header)
    for s in base.segments:
        if s.fileoff != s.vmaddr - old_hdr:
            fail(f'{args.base}: segment {s.name} not mapped at its file offset')
        regions[s.name] = [s.fileoff, s.fileoff + s.vmsize]
    old_entries = {n: Image(bbuf, fo) for n, va, fo in base.entries}
    if KERNEL_ID not in old_entries:
        fail(f'{args.base}: no {KERNEL_ID}')
    old_kernel = old_entries[KERNEL_ID]
    info_seg = base.segment('__PRELINK_INFO')
    info = plistlib.loads(bbuf[info_seg.fileoff:info_seg.fileoff + info_seg.filesize].rstrip(b'\0'))

    kbuf = open(args.kernel, 'rb').read()
    kernel = Image(kbuf)
    ktext = kernel.segment('__TEXT')
    hdr = ktext.vmaddr - next(fo for n, va, fo in base.entries if n == KERNEL_ID)
    slide = u64(hdr - old_hdr)

    # what is kept: the kexts named, every kext they depend on, and the codeless ones all of whose libraries are there
    infos = {d['CFBundleIdentifier']: d for d in info['_PrelinkInfoDictionary']}
    keep = set()
    todo = list(args.keep)
    while todo:
        k = todo.pop()
        if k in keep:
            continue
        if k not in infos:
            fail(f'{k}: not in {args.base}')
        keep.add(k)
        todo += list(infos[k].get('OSBundleLibraries', {}))
    changed = True
    while changed:
        changed = False
        for k, d in infos.items():
            if k not in keep and not has_code(d) and all(lib in keep or lib not in infos and lib == KERNEL_ID
                                                         for lib in d.get('OSBundleLibraries', {})):
                keep.add(k)
                changed = True
    kept = [n for n, va, fo in base.entries if n in keep and n != KERNEL_ID]
    print(f'keeping {len(kept)} kexts: {kept}')

    # the output: the kernel's segments where it was linked, the kept kexts where they were, the native kexts in what
    # is left
    out = bytearray(align(len(bbuf), KC_PAGE))
    holes = {name: Holes(a, b) for name, (a, b) in regions.items()}
    members = {name: [] for name in regions}

    def claim(region, start, size, who):
        if region not in regions:
            fail(f'{who}: no segment {region} in {args.base}')
        members[region].append((start, start + size, who))
        holes[region].take(start, start + size)

    # the kernel: from its first segment to the end of its last in each of the collection's segments (what lies
    # between its own, e.g. __DATA_CONST and __LASTDATA_CONST, is its too)
    kspan = {}
    for s in kernel.segments:
        if s.name == '__LINKEDIT' or not s.vmsize:
            continue
        if s.name not in KERNEL_REGION:
            fail(f'kernel segment {s.name} has no place in the collection')
        a, b = kspan.get(KERNEL_REGION[s.name], (1 << 64, 0))
        kspan[KERNEL_REGION[s.name]] = (min(a, s.vmaddr - hdr), max(b, s.vmaddr - hdr + s.vmsize))
    for region, (a, b) in kspan.items():
        claim(region, a, b - a, f'kernel in {region}')
    # kexts' data starts at the kernel's (empty) __PRELINK_DATA segment, where the kernel takes it to start
    pd = kernel.segment('__PRELINK_DATA')
    if pd is not None and regions['__DATA'][0] <= pd.vmaddr - hdr <= regions['__DATA'][1]:
        holes['__DATA'].take(regions['__DATA'][0], pd.vmaddr - hdr)
    for r in args.reserve:
        region, rng = r.split(':')
        a, b = (int(x, 0) for x in rng.split('-'))
        holes[region].take(a, b)
    for n in kept:
        for s in old_entries[n].segments:
            if s.name != '__LINKEDIT' and s.vmsize:
                claim(KEXT_REGION.get(s.name, s.name), s.vmaddr - old_hdr, s.vmsize, f'{n} {s.name}')
    for region, ms in members.items():
        ms.sort()
        for (a1, b1, w1), (a2, b2, w2) in zip(ms, ms[1:]):
            if a2 < b1:
                fail(f'{w1} ({a1:#x}-{b1:#x}) and {w2} ({a2:#x}-{b2:#x}) overlap')

    # fixups: the kernel's, the kept kexts', the native kexts'
    fixups = {}
    for va, f in threaded_rebases(kernel).items():
        fixups[va - hdr] = f
    print(f'kernel: {len(fixups)} threaded pointers ({sum(f.key is not None for f in fixups.values())} signed)')

    old_ksyms = {}
    for n, t, sc, d, v in old_kernel.symbols():
        if not t & N_STAB and (t & N_TYPE) == N_SECT:
            old_ksyms.setdefault(v, []).append(n)
    ksyms, kexports = {}, {}
    for n, t, sc, d, v in kernel.symbols():
        if not t & N_STAB and (t & N_TYPE) == N_SECT:
            if t & N_EXT:
                kexports[n] = ksyms[n] = v
            else:
                ksyms.setdefault(n, v)

    def in_old_kernel(va):
        return any(s.contains(va) for s in old_kernel.segments if s.name != '__LINKEDIT')

    kept_ranges = sorted((s.vmaddr, s.vmaddr + s.vmsize, n) for n in kept for s in old_entries[n].segments
                         if s.name != '__LINKEDIT' and s.vmsize)

    def kept_owner(va):
        lo, hi = 0, len(kept_ranges)
        while lo < hi:
            mid = (lo + hi) // 2
            if kept_ranges[mid][1] <= va:
                lo = mid + 1
            else:
                hi = mid
        if lo < len(kept_ranges) and kept_ranges[lo][0] <= va < kept_ranges[lo][1]:
            return kept_ranges[lo][2]
        return None

    rebinds = {}
    for r in args.rebind:
        a, sym = r.split('=', 1)
        if sym not in ksyms:
            fail(f'--rebind {r}: no {sym} in the kernel')
        rebinds[int(a, 0) - old_hdr] = ksyms[sym]
    old_fixups = kc_read_fixups(bbuf, old_hdr, None)
    unbound, n_kept, to_kernel = {}, 0, 0
    for loc, f in old_fixups.items():
        owner = kept_owner(old_hdr + loc)
        if owner is None:
            continue
        n_kept += 1
        if loc in rebinds:
            fixups[loc] = Fixup(rebinds.pop(loc), f.key, f.div, f.addr_div)
            continue
        if kept_owner(f.target):
            fixups[loc] = Fixup(u64(f.target + slide), f.key, f.div, f.addr_div)
            continue
        if in_old_kernel(f.target):
            names = old_ksyms.get(f.target, [])
            new = next((kexports[n] for n in names if n in kexports), None)
            if new is None:
                new = next((ksyms[n] for n in names if n in ksyms), None)
            if new is None:
                unbound.setdefault(owner, set()).add(f'{names[0] if names else "?"} ({f.target:#x})')
                continue
            to_kernel += 1
            fixups[loc] = Fixup(new, f.key, f.div, f.addr_div)
            continue
        unbound.setdefault(owner, set()).add(f'{f.target:#x}, in no kept kext')
    if rebinds:
        fail(f'--rebind: no pointer of a kept kext at {", ".join(hex(a + old_hdr) for a in rebinds)}')
    if unbound:
        for owner, what in sorted(unbound.items()):
            print(f'{owner}: pointers to nothing in the output: {", ".join(sorted(what)[:12])}'
                  f'{" ..." if len(what) > 12 else ""}', file=sys.stderr)
        fail('kept kexts point at what is not kept')
    print(f'kept kexts: {n_kept} chained pointers ({to_kernel} into the kernel, by symbol)')

    # native kexts: laid out, placed, then linked against the kernel, the kept kexts and each other
    specs = []
    for path in args.native:
        spec = json.loads(Path(path).read_text())
        if spec['bundle'] in keep or spec['bundle'] in (s['bundle'] for s, _ in specs):
            fail(f'{path}: {spec["bundle"]} is in the collection already')
        objects = json.loads(Path(spec['objects']).read_text())
        try:
            specs.append((spec, kextld.Kext(spec, objects)))
        except kextld.LinkError as e:
            fail(f'{spec["bundle"]}: {e}')
    for spec, kx in specs:
        addrs = {}
        try:
            sizes = kx.layout()
        except kextld.LinkError as e:
            fail(f'{spec["bundle"]}: {e}')
        for seg, (size, al) in sizes.items():
            region = KEXT_REGION[seg]
            off = holes[region].fit(size, al)
            if off is None:
                fail(f'{spec["bundle"]}: no room for its {seg} ({size:#x} bytes) in {region}')
            members[region].append((off, off + size, f'{spec["bundle"]} {seg}'))
            addrs[seg] = hdr + off
        kx.place(addrs, hdr)
    kept_exports = {}
    for n in kept:
        for name, v in old_entries[n].exports().items():
            kept_exports.setdefault(name, u64(v + slide))
    native_exports = {}
    for spec, kx in specs:
        for name, where in kx.globals.items():
            native_exports.setdefault(name, kx.addr_of(where))

    def resolve(name):
        for table in (native_exports, kexports, kept_exports, ksyms):     # ksyms: the kernel's private symbols too
            if name in table:
                return table[name]
        return None

    kept_syms = {}

    def base_vtable(spec):
        """The address of the spec's `vtable` superclass's vtable, in the kept kext that has it."""
        vt = spec['vtable']
        if vt['base_kext'] not in keep:
            fail(f'{spec["bundle"]}: vtable base kext {vt["base_kext"]} not kept')
        if vt['base_kext'] not in kept_syms:
            syms = {}
            for n, t, sc, d, v in old_entries[vt['base_kext']].symbols():
                if not t & N_STAB and (t & N_TYPE) == N_SECT:
                    syms.setdefault(n, set()).add(u64(v + slide))
            kept_syms[vt['base_kext']] = syms
        name = f'__ZTV{len(vt["base"])}{vt["base"]}'
        found = kept_syms[vt['base_kext']].get(name, set())
        if len(found) != 1:
            fail(f'{spec["bundle"]}: {len(found)} vtables {name} in {vt["base_kext"]}')
        a = next(iter(found))
        # its end: the next symbol of the kext
        end = min((v for vs in kept_syms[vt['base_kext']].values() for v in vs if v > a), default=a)
        return a, (end - a - 16) // 8

    def slot(vtable, j):
        a, n = vtable
        if not 0 <= j < n:
            return None
        f = fixups.get(a + 16 + 8 * j - hdr)
        return f.target if f else None

    natives = {}
    linked = []
    for spec, kx in specs:
        inherited = None
        if spec.get('vtable'):
            bvt = base_vtable(spec)
            inherited = lambda j, bvt=bvt: slot(bvt, j)     # noqa: E731
        try:
            r = kx.link(resolve, inherited)
        except kextld.LinkError as e:
            fail(str(e))
        if spec.get('vtable'):
            # the slots this kext does not override must be its superclass's: a header that does not match the
            # superclass the collection has shows here
            vt, nvt = spec['vtable'], kx.vtable()
            own = [(kx.segs[s]['addr'], kx.segs[s]['addr'] + kx.segs[s]['size']) for s in kx.segs if kx.segs[s]['size']]
            bad = []
            for j in range(vt['slots']):
                if j in vt.get('ignore_slots', []):
                    continue
                f = r['fixups'].get(nvt + 16 + 8 * j)
                t = f.target if f else None
                if t is not None and any(a <= t < b for a, b in own):
                    continue
                if t != slot(bvt, j):
                    bad.append(j)
            print(f'vtable check {vt["class"]} vs {vt["base"]}: {len(bad)} problem slots')
            if bad:
                fail(f'{spec["bundle"]}: slots {bad[:20]} of {vt["class"]}\'s vtable are not {vt["base"]}\'s')
        for va, f in r['fixups'].items():
            fixups[va - hdr] = f
        natives.update(r['symbols'])
        linked.append((spec, kx, r))
        print(f'{spec["bundle"]}: linked, header at {kx.segs["__TEXT"]["addr"]:#x}, {len(r["fixups"])} pointers')

    # bytes: the kept kexts', the kernel's, the native kexts'
    for n in kept:
        for s in old_entries[n].segments:
            if s.name != '__LINKEDIT' and s.filesize:
                out[s.fileoff:s.fileoff + s.filesize] = bbuf[s.fileoff:s.fileoff + s.filesize]
    le = base.segment('__LINKEDIT')
    out[le.fileoff:len(bbuf)] = bbuf[le.fileoff:]
    for s in kernel.segments:
        if s.name != '__LINKEDIT' and s.filesize:
            off = s.vmaddr - hdr
            out[off:off + s.filesize] = kbuf[s.fileoff:s.fileoff + s.filesize]

    # the kept kexts' headers, symbols and kmod_info at the output's addresses
    for n in kept:
        img = Image(out, old_entries[n].off)
        p = img.off + HEADER.size
        for cmd, raw in img.cmds:
            if cmd == LC_SEGMENT_64:
                seg = Segment(raw)
                seg.vmaddr = u64(seg.vmaddr + slide)
                for sec in seg.sections:
                    sec.addr = u64(sec.addr + slide)
                out[p:p + len(raw)] = seg.pack()
            p += len(raw)
        if img.symtab:
            symoff, nsyms = img.symtab[:2]
            for i in range(nsyms):
                o = symoff + NLIST.size * i
                typ, value = out[o + 4], struct.unpack_from('<Q', out, o + 8)[0]
                if not typ & N_STAB and (typ & N_TYPE) == N_SECT:
                    struct.pack_into('<Q', out, o + 8, u64(value + slide))
        d = infos[n]
        kmod = d.get('_PrelinkKmodInfo')
        if kmod:
            o = u64(kmod) - old_hdr + kextld.KMOD_ADDRESS
            a = struct.unpack_from('<Q', out, o)[0]
            if a:
                struct.pack_into('<Q', out, o, u64(a + slide))

    # the kernel's symbols, after BASE's __LINKEDIT
    symoff, nsyms, stroff, strsize = kernel.symtab
    k_symoff = len(out)
    out += kbuf[symoff:symoff + NLIST.size * nsyms]
    k_stroff = len(out)
    out += kbuf[stroff:stroff + strsize]
    out += b'\0' * (align(len(out), 8) - len(out))
    k_le = (k_symoff, len(out) - k_symoff)

    # prelink info: the kept kexts' entries at the output's addresses, the native kexts'
    entries = []
    patches = {}
    for spec, kx, r in linked:
        for bundle, pers in spec.get('patch_personalities', {}).items():
            patches.setdefault(bundle, {}).update(pers)
    for d in info['_PrelinkInfoDictionary']:
        k = d['CFBundleIdentifier']
        if k not in keep:
            continue
        d = dict(d)
        if has_code(d):
            for key in ADDRESS_KEYS:
                if key in d:
                    d[key] = kextld.signed(u64(d[key] + slide))
        for pers, keys in patches.pop(k, {}).items():
            d['IOKitPersonalities'] = dict(d.get('IOKitPersonalities', {}))
            if pers not in d['IOKitPersonalities']:
                fail(f'patch_personalities: {k} has no personality {pers}')
            d['IOKitPersonalities'][pers] = {**d['IOKitPersonalities'][pers], **keys}
        entries.append(d)
    if patches:
        fail(f'patch_personalities: {", ".join(patches)} not kept')
    for spec, kx, r in linked:
        entries.append(kx.plist(kx.out[('__DATA', '__data')].addr))
    info = dict(info)
    info['_PrelinkInfoDictionary'] = entries
    plist = prelink_xml(info)

    # the collection's segments: BASE's, grown to what is in them; __PRELINK_INFO as large as the prelink info
    pi = regions['__PRELINK_INFO']
    pi[1] = pi[0] + align(len(plist), KC_PAGE)
    for region, ms in members.items():
        if ms and region not in ('__LINKEDIT',):
            regions[region][0] = min(regions[region][0], min(a for a, b, w in ms))
            regions[region][1] = max(regions[region][1], align(max(b for a, b, w in ms), KC_PAGE))
    order = sorted(regions.items(), key=lambda kv: kv[1][0])
    for (n1, (a1, b1)), (n2, (a2, b2)) in zip(order, order[1:]):
        if a2 < b1:
            fail(f'segments {n1} ({a1:#x}-{b1:#x}) and {n2} ({a2:#x}-{b2:#x}) overlap')
    out[pi[0]:pi[0] + len(plist)] = plist
    top = [(s.name, regions[s.name][0], regions[s.name][1] - regions[s.name][0]) for s in base.segments]

    # the native kexts' bytes and headers
    for spec, kx, r in linked:
        for seg, b in r['segments'].items():
            off = kx.segs[seg]['addr'] - hdr
            out[off:off + len(b)] = b
        h = kx.header((hdr + k_le[0], k_le[0], k_le[1]))
        off = kx.segs['__TEXT']['addr'] - hdr
        out[off:off + len(h)] = h

    # the kernel's header: its segments at their file offsets in the collection, its symbols where they went
    cmds = []
    for cmd, raw in kernel.cmds:
        if cmd in KERNEL_DROPPED_CMDS:
            continue
        if cmd == LC_SEGMENT_64:
            seg = Segment(raw)
            if seg.name == '__LINKEDIT':
                seg.vmaddr, seg.fileoff = hdr + k_le[0], k_le[0]
                seg.vmsize = seg.filesize = k_le[1]
            else:
                seg.fileoff = seg.vmaddr - hdr
                for sec in seg.sections:
                    sec.offset = 0 if sec.zerofill else sec.addr - hdr
            raw = seg.pack()
        elif cmd == LC_SYMTAB:
            raw = struct.pack('<IIIIII', LC_SYMTAB, 24, k_symoff, nsyms, k_stroff, strsize)
        elif cmd == LC_DYSYMTAB:
            f = list(struct.unpack_from('<18I', raw, 8))
            if any(f[i] for i in (7, 9, 11, 13, 15, 17)):      # ntoc, nmodtab, nextrefsyms, nindirect, nextrel, nlocrel
                fail('the kernel has tables of its LC_DYSYMTAB besides symbols, not carried into the collection')
            raw = struct.pack('<II18I', LC_DYSYMTAB, 80, *f)
        cmds.append(raw)
    body = b''.join(cmds)
    koff = ktext.vmaddr - hdr
    magic, cpu, sub, ft, _, old_size, flags, res = HEADER.unpack_from(kbuf, 0)
    flags |= MH_DYLIB_IN_CACHE      # XNU takes the kernel to be in a collection (its chained fixups, not __thread_starts)
    out[koff:koff + HEADER.size + old_size] = HEADER.pack(magic, cpu, sub, ft, len(cmds), len(body), flags, res) + \
        body + b'\0' * (old_size - len(body))

    # chained fixups, then the collection's header
    blob = kc_write_fixups(out, hdr, top, fixups)
    fix_off = len(out)
    out += blob
    out += b'\0' * (align(len(out), KC_PAGE) - len(out))
    regions['__LINKEDIT'][1] = len(out)
    # every entry's __LINKEDIT: the collection's link-edit data its symbol table is in, to the end of the file
    le_start = min([k_symoff] + [old_entries[n].segment('__LINKEDIT').fileoff for n in kept
                                 if old_entries[n].segment('__LINKEDIT')])
    for off in [koff] + [old_entries[n].off for n in kept] + [kx.segs['__TEXT']['addr'] - hdr for spec, kx, r in linked]:
        img = Image(out, off)
        p = off + HEADER.size
        for cmd, raw in img.cmds:
            if cmd == LC_SEGMENT_64 and Segment(raw).name == '__LINKEDIT':
                seg = Segment(raw)
                seg.vmaddr, seg.fileoff = hdr + le_start, le_start
                seg.vmsize = seg.filesize = len(out) - le_start
                out[p:p + len(raw)] = seg.pack()
            p += len(raw)
    top = [(s.name, regions[s.name][0], regions[s.name][1] - regions[s.name][0]) for s in base.segments]

    tcmds = []
    for cmd, raw in base.cmds:
        if cmd == LC_UNIXTHREAD:
            raw = kernel.cmd(LC_UNIXTHREAD)
        elif cmd == LC_DYLD_CHAINED_FIXUPS:
            raw = struct.pack('<IIII', LC_DYLD_CHAINED_FIXUPS, 16, fix_off, len(blob))
        elif cmd == LC_SEGMENT_64:
            s = Segment(raw)
            a, size = regions[s.name][0], regions[s.name][1] - regions[s.name][0]
            s.vmaddr, s.vmsize, s.fileoff, s.filesize = hdr + a, size, a, size
            for sec in s.sections:          # __PRELINK_TEXT,__text and __PRELINK_INFO,__info span their segment
                sec.addr, sec.size, sec.offset = s.vmaddr, (len(plist) if s.name == '__PRELINK_INFO' else size), a
            raw = s.pack()
        elif cmd == LC_FILESET_ENTRY:
            continue
        tcmds.append(raw)
    fileset = [(KERNEL_ID, ktext.vmaddr)] + [(n, u64(old_entries[n].segment('__TEXT').vmaddr + slide)) for n in kept] \
        + [(spec['bundle'], kx.segs['__TEXT']['addr']) for spec, kx, r in linked]
    for name, va in fileset:
        nm = name.encode() + b'\0'
        size = align(32 + len(nm), 8)
        tcmds.append(struct.pack('<IIQQII', LC_FILESET_ENTRY, size, va, va - hdr, 32, 0) + nm.ljust(size - 32, b'\0'))
    body = b''.join(tcmds)
    if HEADER.size + len(body) > regions['__TEXT'][1]:
        fail(f'the collection\'s load commands ({len(body):#x} bytes) do not fit in its __TEXT')
    magic, cpu, sub, ft, _, _, flags, res = HEADER.unpack_from(bbuf, 0)
    out[0:regions['__TEXT'][1]] = (HEADER.pack(magic, cpu, sub, ft, len(tcmds), len(body), flags, res) + body).ljust(
        regions['__TEXT'][1], b'\0')

    Path(args.output).write_bytes(out)
    Path(args.output).with_suffix('.natives.json').write_text(json.dumps(natives))
    print(f'wrote {args.output}: {len(out):#x} bytes, base {hdr:#x}, {len(fixups)} chained pointers')


if __name__ == '__main__':
    main()
