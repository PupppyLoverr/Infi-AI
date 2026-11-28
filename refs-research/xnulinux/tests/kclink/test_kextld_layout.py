#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-only
"""Where scripts/kclink/kextld.py puts what the objects of a kext bring: each section into the kext's segments, aligned,
in the objects' order, nothing over anything else, zero-fill last; the kmod_info in front of the data; the room for the
Mach-O header; and what the kext's header and its entry in the prelink info then say. Objects built here (mkmacho.py):

    python3 tests/kclink/test_kextld_layout.py
"""
import os
import random
import shutil
import struct
import sys
import tempfile
import unittest
import uuid

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import mkmacho                                                                  # noqa: E402
from mkmacho import (HEADER_VA, KEXT_VA, KEY_IA, LC_SEGMENT_64, LC_SYMTAB, LC_UUID, MH_KEXT_BUNDLE, OUTSIDE,  # noqa: E402
                     RET, S_ATTR_DEBUG, S_CSTRING_LITERALS, S_ZEROFILL, TEXT_FLAGS, Object, movz)
import kextld                                                                   # noqa: E402

# where a section of an object goes in the kext (kextld's PLACEMENT, said again: a change there is meant to show here)
GOES_TO = {
    ('__TEXT', '__text'): ('__TEXT_EXEC', '__text'), ('__TEXT', '__cstring'): ('__TEXT', '__cstring'),
    ('__TEXT', '__const'): ('__TEXT', '__const'), ('__TEXT', '__literal8'): ('__TEXT', '__literal8'),
    ('__TEXT', '__literal16'): ('__TEXT', '__literal16'), ('__DATA', '__const'): ('__DATA_CONST', '__const'),
    ('__DATA', '__mod_init_func'): ('__DATA_CONST', '__mod_init_func'), ('__DATA', '__data'): ('__DATA', '__data'),
    ('__DATA', '__bss'): ('__DATA', '__bss'), ('__DATA', '__common'): ('__DATA', '__common'),
}
ZEROFILL = {'__bss', '__common'}
KMOD_INFO = 196                  # sizeof(kmod_info_t), <mach/kmod.h> (packed to 4 bytes)


class Case(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, self.dir)

    def link(self, *objects, **how):
        return mkmacho.link([o.write(self.dir, f'o{i}.o') for i, o in enumerate(objects)], **how)

    def refused(self, *objects):
        with self.assertRaises(kextld.LinkError) as e:
            self.link(*objects)
        return str(e.exception)


def mixed(seed, objects=3):
    """Objects with sections of every size and alignment (1 byte to a 16K page), each piece its own byte pattern and a
    symbol. Returns (objects, {symbol: (section it goes to, log2 alignment, its bytes or its zero-filled size)})."""
    rnd, out, pieces = random.Random(seed), [], {}
    for i in range(objects):
        o = Object()
        for key in rnd.sample(sorted(GOES_TO), rnd.randint(4, len(GOES_TO))):
            al, size = rnd.choice((0, 0, 1, 2, 3, 4, 4, 6, 12, 14)), rnd.choice((1, 3, 8, 16, 100, 333, 4096))
            name = f'_piece{len(pieces)}'
            if key[1] in ZEROFILL:
                sect, content = o.section(*key, align_=al, zerofill=size), size
            else:
                content = bytes((len(pieces) * 7 + j) % 251 + 1 for j in range(size))
                flags = {'__text': TEXT_FLAGS, '__cstring': S_CSTRING_LITERALS}.get(key[1], 0)
                sect = o.section(*key, content, al, flags)
            o.define(name, sect)
            pieces[name] = (GOES_TO[key], al, content)
        out.append(o)
    return out, pieces


class Sections(Case):
    def test_every_piece_aligned_whole_and_apart(self):
        for seed in range(12):
            objects, pieces = mixed(seed)
            k = self.link(*objects)
            spans = []
            for name, (where, al, content) in pieces.items():
                at, size = k.symbols[name], content if isinstance(content, int) else len(content)
                self.assertEqual(at % (1 << al), 0, f'seed {seed}: {name} at {at:#x}, alignment 2^{al}')
                self.assertEqual(k.bytes(at, size), bytes(size) if isinstance(content, int) else content, name)
                start, length = k.section(*where)
                self.assertTrue(start <= at and at + size <= start + length, f'seed {seed}: {name} in {where}')
                spans.append((at, at + size, name))
            spans.sort()
            for (a1, b1, n1), (a2, b2, n2) in zip(spans, spans[1:]):
                self.assertLessEqual(b1, a2, f'seed {seed}: {n1} and {n2} overlap')
            # in one section: in the order of the objects (the pieces are numbered in it)
            for where in set(w for w, al, content in pieces.values()):
                inside = [k.symbols[n] for n, (w, al, content) in pieces.items() if w == where]
                self.assertEqual(inside, sorted(inside), f'seed {seed}: {where}')

    def test_segments_are_as_aligned_as_what_is_in_them(self):
        for seed in range(12):
            objects, pieces = mixed(seed)
            sizes = mkmacho.kext([o.write(self.dir, f'o{i}.o') for i, o in enumerate(objects)]).layout()
            for seg, (size, al) in sizes.items():
                most = max([1 << a for (s, n), a, c in pieces.values() if s == seg] + [16])
                self.assertEqual(al, most, f'seed {seed}: {seg}')
                self.assertEqual(size % 16, 0, seg)

    def test_sections_of_a_segment_follow_each_other_zero_fill_last(self):
        for seed in range(12):
            objects, pieces = mixed(seed)
            k = self.link(*objects)
            for seg in k.segments:
                sects = sorted((s.addr, s.addr + s.size, s.name) for s in k.kx.out.values() if s.seg == seg)
                for (a1, b1, n1), (a2, b2, n2) in zip(sects, sects[1:]):
                    self.assertLessEqual(b1, a2, f'seed {seed}: {seg},{n1} and {n2} overlap')
                self.assertLessEqual(sects[-1][1], k.addrs[seg] + len(k.segments[seg]))
                names = [n for a, b, n in sects]
                first = min([i for i, n in enumerate(names) if n in ZEROFILL] + [len(names)])
                self.assertTrue(all(n in ZEROFILL for n in names[first:]), f'seed {seed}: {seg}: {names}')

    def test_the_kmod_info_is_first_in_the_data(self):
        o = Object()
        o.define('_mine', o.section('__DATA', '__data', b'\xaa' * 24, 5))
        k = self.link(o)
        data, size = k.section('__DATA', '__data')
        self.assertEqual(data, KEXT_VA['__DATA'])
        self.assertEqual(k.symbols['_mine'], data + 224)         # after 196 bytes, at the next multiple of 32
        self.assertEqual(size, 224 + 24)
        info = k.bytes(data, KMOD_INFO)
        nxt, version, ident, name, vers, refs, reflist, address, length, hdr_size, start, stop = \
            struct.unpack('<QIi64s64siQQQQQQ', info)
        self.assertEqual((nxt, version, ident, refs, reflist, hdr_size), (0, 1, -1, -1, 0, 0))
        self.assertEqual((name.rstrip(b'\0'), vers.rstrip(b'\0')), (b'org.example.kext.test', b'1.2.3'))
        self.assertEqual((address, length), (KEXT_VA['__TEXT'], len(k.segments['__TEXT'])))
        # start and stop: signed as XNU calls them, the IA key and kmod_start_func_t's type discriminator
        self.assertEqual((start, stop), (0, 0))
        for off, name in ((180, '_test_start'), (188, '_test_stop')):
            f = k.fixups[data + off]
            self.assertEqual((f.target, f.key, f.div, f.addr_div), (OUTSIDE[name], KEY_IA, 0xe4fe, 0))

    def test_start_and_stop_of_the_kext_itself(self):
        o = Object()
        t = o.text(movz(0, 0), RET, movz(0, 0), RET)
        o.define('_my_start', t)
        o.define('_my_stop', t, 8, private=True)
        k = self.link(o, spec={'start': '_my_start', 'stop': '_my_stop'})
        data = k.section('__DATA', '__data')[0]
        self.assertEqual(k.fixups[data + 180].target, KEXT_VA['__TEXT_EXEC'])
        self.assertEqual(k.fixups[data + 188].target, KEXT_VA['__TEXT_EXEC'] + 8)
        with self.assertRaises(kextld.LinkError) as e:
            self.link(o, spec={'start': '_my_start', 'stop': '_no_such_stop'})
        self.assertIn('unresolved symbols: _no_such_stop', str(e.exception))

    def test_room_for_the_header_in_front_of_text(self):
        o = Object()
        o.define('_s', o.section('__TEXT', '__cstring', b'first\0', 0, S_CSTRING_LITERALS))
        k = self.link(o)
        self.assertEqual(k.symbols['_s'], KEXT_VA['__TEXT'] + 0x1000)
        self.assertEqual(k.bytes(KEXT_VA['__TEXT'], 0x1000), bytes(0x1000))     # (the header is written by kclink)
        self.assertEqual(len(k.segments['__TEXT']), 0x1010)

    def test_a_kext_of_nothing(self):
        k = self.link()
        self.assertEqual({seg: len(b) for seg, b in k.segments.items()}, {'__TEXT': 0x1000, '__DATA': 208})
        self.assertEqual(k.section('__DATA', '__data'), (KEXT_VA['__DATA'], KMOD_INFO))
        self.assertEqual(sorted(k.fixups), [KEXT_VA['__DATA'] + 180, KEXT_VA['__DATA'] + 188])

    def test_a_section_with_no_place_is_refused(self):
        for key in (('__DATA', '__objc_const'), ('__TEXT', '__gcc_except_tab'), ('__DATA', '__thread_vars')):
            o = Object()
            o.section(*key, bytes(8), 3)
            self.assertIn(f'section {key[0]},{key[1]} has no place in a kext', self.refused(o))

    def test_what_a_kext_does_not_get(self):
        """Unwind tables, debug information and bitcode take no room, and their relocations are not looked at."""
        plain, with_more = Object(), Object()
        for o in (plain, with_more):
            o.define('_f', o.text(movz(0, 5), RET))
        for seg, name, flags in (('__LD', '__compact_unwind', S_ATTR_DEBUG), ('__TEXT', '__eh_frame', 0),
                                 ('__DWARF', '__debug_info', S_ATTR_DEBUG), ('__DWARF', '__apple_names', S_ATTR_DEBUG),
                                 ('__LLVM', '__bitcode', 0), ('__LLVM', '__cmdline', 0)):
            s = with_more.section(seg, name, bytes(64), 3, flags)
            with_more.reloc(s, 0, mkmacho.UNSIGNED, 1, length=3, extern=0)
            with_more.reloc(s, 8, mkmacho.SUBTRACTOR, 0, length=3)
        a, b = self.link(plain), self.link(with_more)
        self.assertEqual(a.segments, b.segments)
        self.assertEqual(sorted(a.fixups), sorted(b.fixups))
        self.assertEqual(sorted(s.name for s in b.kx.out.values()), ['__data', '__text'])


class Header(Case):
    LINKEDIT = (HEADER_VA + 0x3e00000, 0x3e00000, 0x8000)

    def setUp(self):
        super().setUp()
        o = Object()
        o.define('_f', o.text(movz(0, 5), RET))
        o.define('_s', o.section('__TEXT', '__cstring', b'text\0', 0, S_CSTRING_LITERALS))
        o.define('_c', o.section('__DATA', '__const', bytes(24), 3))
        o.define('_d', o.section('__DATA', '__data', bytes(8), 3))
        o.define('_z', o.section('__DATA', '__bss', align_=6, zerofill=100))
        o.define('_k', o.section('__DATA', '__kalloc_type', bytes(16), 3))
        self.k = self.link(o)
        self.header = self.k.kx.header(self.LINKEDIT)

    def test_it_is_a_kext_of_a_collection(self):
        magic, cpu, sub, filetype, ncmds, size, flags, _ = struct.unpack_from('<8I', self.header)
        self.assertEqual((magic, cpu, sub & 0xffffff, filetype), (0xfeedfacf, 0x0100000c, 2, MH_KEXT_BUNDLE))
        self.assertTrue(flags & 0x80000000)                      # MH_DYLIB_IN_CACHE
        cmds = mkmacho.commands(self.header)
        self.assertEqual(len(self.header), 32 + size)
        self.assertEqual(sum(len(raw) for cmd, raw in cmds), size)
        self.assertLessEqual(len(self.header), 0x1000)
        self.assertEqual([cmd for cmd, raw in cmds], [LC_SEGMENT_64] * 5 + [LC_SYMTAB, LC_UUID])
        self.assertEqual(cmds[-1][1][8:], uuid.uuid5(uuid.NAMESPACE_URL, 'org.example.kext.test').bytes)
        self.assertEqual(struct.unpack_from('<4I', cmds[-2][1], 8), (self.LINKEDIT[1], 0, self.LINKEDIT[1], 0))

    def test_its_segments_are_where_they_were_placed(self):
        segs = mkmacho.segments(self.header)
        self.assertEqual([s['name'] for s in segs], ['__TEXT', '__TEXT_EXEC', '__DATA', '__DATA_CONST', '__LINKEDIT'])
        for s in segs[:4]:
            self.assertEqual(s['vmaddr'], KEXT_VA[s['name']])
            self.assertEqual(s['vmsize'], len(self.k.segments[s['name']]))
            self.assertEqual((s['fileoff'], s['filesize']), (s['vmaddr'] - HEADER_VA, s['vmsize']))
            self.assertEqual(s['initprot'], {'__TEXT': 1, '__TEXT_EXEC': 5, '__DATA': 3, '__DATA_CONST': 3}[s['name']])
        le = segs[4]
        self.assertEqual((le['vmaddr'], le['fileoff'], le['vmsize'], le['filesize'], le['sections']),
                         (self.LINKEDIT[0], self.LINKEDIT[1], self.LINKEDIT[2], self.LINKEDIT[2], []))

    def test_its_sections(self):
        sects = {(s['segname'], s['sectname']): s for seg in mkmacho.segments(self.header) for s in seg['sections']}
        self.assertEqual(sorted(sects), [('__DATA', '__bss'), ('__DATA', '__data'), ('__DATA_CONST', '__const'),
                                         ('__DATA_CONST', '__kalloc_type'), ('__TEXT', '__cstring'),
                                         ('__TEXT_EXEC', '__text')])
        for (seg, name), s in sects.items():
            self.assertEqual((s['addr'], s['size']), self.k.section(seg, name), name)
            self.assertEqual(s['offset'], 0 if name == '__bss' else s['addr'] - HEADER_VA, name)
        for name, sym in (('__text', '_f'), ('__cstring', '_s'), ('__const', '_c'), ('__bss', '_z'),
                          ('__kalloc_type', '_k')):
            s = next(s for (seg, n), s in sects.items() if n == name)
            self.assertEqual(s['addr'], self.k.symbols[sym], name)
        self.assertEqual(sects[('__TEXT_EXEC', '__text')]['flags'], TEXT_FLAGS)
        self.assertEqual(sects[('__TEXT', '__cstring')]['flags'], S_CSTRING_LITERALS)
        self.assertEqual(sects[('__DATA', '__bss')]['flags'], S_ZEROFILL)
        self.assertEqual((sects[('__DATA', '__bss')]['align'], sects[('__DATA', '__bss')]['size']), (6, 100))
        self.assertEqual(sects[('__DATA', '__data')]['size'], 208)

    def test_its_entry_in_the_prelink_info(self):
        kx = self.k.kx
        d = kx.plist(self.k.section('__DATA', '__data')[0])
        signed = KEXT_VA['__TEXT'] - (1 << 64)                   # as a kernel's prelink info has addresses
        self.assertEqual((d['_PrelinkExecutableLoadAddr'], d['_PrelinkExecutableSourceAddr']), (signed, signed))
        self.assertEqual(d['_PrelinkKmodInfo'], KEXT_VA['__DATA'] - (1 << 64))
        self.assertEqual(d['_PrelinkExecutableSize'], len(self.k.segments['__TEXT']))
        self.assertEqual((d['CFBundleIdentifier'], d['CFBundleExecutable'], d['CFBundleVersion']),
                         ('org.example.kext.test', 'Test', '1.2.3'))
        self.assertEqual(d['OSBundleUUID'], uuid.uuid5(uuid.NAMESPACE_URL, 'org.example.kext.test').bytes)
        self.assertEqual((d['OSBundleLibraries'], d['IOKitPersonalities'], d['OSBundleCompatibleVersion']),
                         ({}, {}, '1.0.0'))
        self.assertEqual(d['_PrelinkBundlePath'], '/System/Library/Extensions/Test.kext')

    def test_what_the_spec_says_goes_into_the_entry(self):
        kx = self.k.kx
        kx.spec.update(libraries={'com.apple.kpi.iokit': '8.0'}, personalities={'P': {'IOClass': 'Test'}},
                       compatible_version='1.1', plist={'OSBundleRequired': 'Safe Boot', 'Extra': 1})
        d = kx.plist(0)
        self.assertEqual((d['OSBundleLibraries'], d['IOKitPersonalities'], d['OSBundleCompatibleVersion']),
                         ({'com.apple.kpi.iokit': '8.0'}, {'P': {'IOClass': 'Test'}}, '1.1'))
        self.assertEqual((d['OSBundleRequired'], d['Extra']), ('Safe Boot', 1))


if __name__ == '__main__':
    unittest.main()
