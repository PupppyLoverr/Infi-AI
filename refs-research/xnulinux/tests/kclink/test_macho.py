#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-only
"""scripts/kclink/macho.py against the formats: what it reads of an image (load commands, segments, sections, symbols),
that what it writes of a segment is what it read, a collection's chained fixups written and walked back bit by bit
(DYLD_CHAINED_PTR_64_KERNEL_CACHE: page starts, the stride of 4, the last pointer of a page), and a kernel's threaded
rebases. The images are made here (mkmacho.py), the chains walked by a reader of its own:

    python3 tests/kclink/test_macho.py
"""
import os
import struct
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import mkmacho                                                                  # noqa: E402
from mkmacho import (KEY_DA, KEY_DB, KEY_IA, KEY_IB, LC_DYLD_CHAINED_FIXUPS, LC_FILESET_ENTRY,  # noqa: E402
                     LC_SEGMENT_64, LC_SYMTAB, LC_UNIXTHREAD, N_EXT, N_PEXT, N_SECT, N_WEAK_DEF, RET, S_ZEROFILL,
                     TEXT_FLAGS, Object)
import macho                                                                    # noqa: E402

PAGE = 0x4000
VA = 0xfffffff007004000


def an_object():
    o = Object()
    t = o.text(RET, RET)
    d = o.section('__DATA', '__data', b'\x11' * 24, 3)
    z = o.section('__DATA', '__bss', align_=4, zerofill=48)
    u = o.section('__LD', '__compact_unwind', bytes(32), 3, mkmacho.S_ATTR_DEBUG)      # a name of all 16 bytes
    o.define('_f', t)
    o.define('_local', t, 4, ext=False)
    o.define('_weak', d, 8, weak=True)
    o.define('_hidden', d, 16, private=True)
    o.define('_zero', z, 8)
    o.undefined('_elsewhere')
    o.absolute('_abs', 0x1234)
    o.syms.append(('stab', 0x24, 1, 0, 0))                    # N_FUN: a debugger's entry, not a symbol
    o.pointer(d, 0, 0)
    return o, (t, d, z, u)


class Reading(unittest.TestCase):
    def test_an_object(self):
        o, (t, d, z, u) = an_object()
        buf = o.build()
        img = macho.Image(buf)
        self.assertEqual((img.filetype, img.cputype, img.cpusubtype & 0xffffff), (macho.MH_OBJECT, 0x0100000c, 2))
        self.assertEqual([c for c, raw in img.cmds], [LC_SEGMENT_64, LC_SYMTAB])
        self.assertEqual(sum(len(raw) for c, raw in img.cmds), img.sizeofcmds)
        self.assertEqual(len(img.segments), 1)
        sects = img.sections()
        self.assertEqual([(s.segname, s.sectname) for s in sects], [('__TEXT', '__text'), ('__DATA', '__data'),
                                                                    ('__DATA', '__bss'), ('__LD', '__compact_unwind')])
        self.assertEqual([(s.addr, s.size, s.align) for s in sects], [(0, 8, 2), (8, 24, 3), (32, 48, 4), (80, 32, 3)])
        self.assertEqual([s.zerofill for s in sects], [False, False, True, False])
        self.assertEqual((sects[0].flags, sects[2].flags), (TEXT_FLAGS, S_ZEROFILL))
        self.assertEqual(buf[sects[1].offset:sects[1].offset + 24], bytes(8) + b'\x11' * 16)
        self.assertEqual((sects[1].nreloc, sects[0].nreloc), (1, 0))
        self.assertEqual(struct.unpack_from('<iI', buf, sects[1].reloff), (0, 0 | 3 << 25 | 1 << 27))
        self.assertIs(img.section('__DATA', '__bss').zerofill, True)
        self.assertIsNone(img.section('__DATA', '__const'))
        self.assertIsNone(img.segment('__TEXT'))                # (an object's one segment has no name)

    def test_its_symbols(self):
        o, sects = an_object()
        img = macho.Image(o.build())
        self.assertEqual(img.symbols(), [
            ('_f', N_SECT | N_EXT, 1, 0, 0), ('_local', N_SECT, 1, 0, 4), ('_weak', N_SECT | N_EXT, 2, N_WEAK_DEF, 16),
            ('_hidden', N_SECT | N_EXT | N_PEXT, 2, 0, 24), ('_zero', N_SECT | N_EXT, 3, 0, 40),
            ('_elsewhere', N_EXT, 0, 0, 0), ('_abs', 2 | N_EXT, 0, 0, 0x1234), ('stab', 0x24, 1, 0, 0)])
        self.assertEqual(img.exports(), {'_f': 0, '_weak': 16, '_hidden': 24, '_zero': 40, '_abs': 0x1234})

    def test_zero_fill_is_a_type_not_a_flag(self):
        for flags, zerofill in ((0x1, True), (0xc, True), (0x12, True), (0x80000401, True), (0x0, False),
                                (0x2, False), (0x11, False), (0x100, False)):
            o = Object()
            o.section('__DATA', '__x', b'', 0, flags)
            self.assertIs(macho.Image(o.build()).sections()[0].zerofill, zerofill, hex(flags))

    def test_no_mach_o(self):
        for buf in (bytes(64), struct.pack('<I', 0xfeedface) + bytes(60), struct.pack('>I', 0xfeedfacf) + bytes(60)):
            with self.assertRaises(ValueError):
                macho.Image(buf)

    def kernel(self):
        segs = [('__TEXT', VA + PAGE, bytes(2 * PAGE), [('__const', 0x2000, 0x100, 0)]),
                ('__DATA_CONST', VA + 0x100000, bytes(PAGE), [('__const', 0, PAGE, 0)]),
                ('__TEXT_EXEC', VA + 0x200000, mkmacho.words(RET, RET) + bytes(PAGE - 8),
                 [('__text', 0, 8, TEXT_FLAGS)]),
                ('__DATA', VA + 0x300000, bytes(PAGE), [('__data', 0, 0x100, 0), ('__bss', 0x100, 0x200, S_ZEROFILL)])]
        symbols = [('_start', VA + 0x200000, True), ('_second', VA + 0x200004, False), ('_datum', VA + 0x300010, True)]
        return mkmacho.kernel(segs, symbols, extra=[struct.pack('<4I', 0x26, 16, 0, 0)])

    def test_an_executable(self):
        buf = self.kernel()
        img = macho.Image(buf)
        self.assertEqual(img.filetype, macho.MH_EXECUTE)
        self.assertEqual([s.name for s in img.segments], ['__TEXT', '__DATA_CONST', '__TEXT_EXEC', '__DATA',
                                                          '__LINKEDIT'])
        self.assertEqual([(s.vmaddr - VA, s.vmsize, s.fileoff) for s in img.segments[:4]],
                         [(PAGE, 2 * PAGE, 0), (0x100000, PAGE, 2 * PAGE), (0x200000, PAGE, 3 * PAGE),
                          (0x300000, PAGE, 4 * PAGE)])
        self.assertEqual(img.segment('__DATA').sections[1].sectname, '__bss')
        self.assertEqual(img.section('__TEXT', '__thread_starts').addr, VA + PAGE + 0x1000)
        self.assertEqual([c for c, raw in img.cmds][-3:], [LC_SYMTAB, LC_UNIXTHREAD, 0x26])
        self.assertEqual(img.cmd(0x26), struct.pack('<4I', 0x26, 16, 0, 0))
        self.assertIsNone(img.cmd(LC_FILESET_ENTRY))
        self.assertEqual([(s.segname, s.sectname) for s in img.sections()][:4], [
            ('__TEXT', '__thread_starts'), ('__TEXT', '__const'), ('__DATA_CONST', '__const'),
            ('__TEXT_EXEC', '__text')])
        self.assertEqual(img.symbols(), [('_start', 0xf, 4, 0, VA + 0x200000), ('_second', 0xe, 4, 0, VA + 0x200004),
                                         ('_datum', 0xf, 5, 0, VA + 0x300010)])
        self.assertEqual(img.exports(), {'_start': VA + 0x200000, '_datum': VA + 0x300010})

    def test_addresses_to_file_offsets(self):
        img = macho.Image(self.kernel())
        self.assertEqual(img.va_to_off(VA + PAGE), 0)
        self.assertEqual(img.va_to_off(VA + 0x200004), 3 * PAGE + 4)
        self.assertEqual(img.va_to_off(VA + 0x300000 + PAGE - 1), 5 * PAGE - 1)
        for va in (VA, VA + 3 * PAGE, VA + 0x100000 + PAGE, VA + 0x400000, 0):
            self.assertIsNone(img.va_to_off(va), hex(va))
        seg = img.segment('__TEXT_EXEC')
        self.assertTrue(seg.contains(seg.vmaddr) and seg.contains(seg.vmaddr + PAGE - 1))
        self.assertFalse(seg.contains(seg.vmaddr - 1) or seg.contains(seg.vmaddr + PAGE))

    def test_an_image_inside_a_collection(self):
        """A fileset entry: its header somewhere in the file, its file offsets the collection's."""
        inner = bytearray(self.kernel())
        at = 3 * PAGE
        buf = bytearray(at) + inner
        p = at + 32
        for cmd, raw in mkmacho.commands(buf, at):               # move its offsets, as a collection has them
            if cmd == LC_SEGMENT_64:
                struct.pack_into('<Q', buf, p + 40, struct.unpack_from('<Q', buf, p + 40)[0] + at)
            elif cmd == LC_SYMTAB:
                symoff, nsyms, stroff, strsize = struct.unpack_from('<4I', buf, p + 8)
                struct.pack_into('<4I', buf, p + 8, symoff + at, nsyms, stroff + at, strsize)
            p += len(raw)
        entry = struct.pack('<IIQQII', LC_FILESET_ENTRY, 48, VA + PAGE, at, 32, 0) + b'org.example.one\0'
        header = struct.pack('<8I', 0xfeedfacf, 0x0100000c, 2, macho.MH_FILESET, 1, len(entry), 0, 0)
        buf[0:32 + len(entry)] = header + entry
        top = macho.Image(buf)
        self.assertEqual((top.filetype, top.entries), (macho.MH_FILESET, [('org.example.one', VA + PAGE, at)]))
        img = macho.Image(buf, at)
        self.assertEqual(img.va_to_off(VA + 0x200004), at + 3 * PAGE + 4)
        self.assertEqual(img.exports(), {'_start': VA + 0x200000, '_datum': VA + 0x300010})
        # and one whose offsets are its own: counted from `base`
        own = macho.Image(bytearray(at) + inner, at, at)
        self.assertEqual(own.va_to_off(VA + 0x200004), at + 3 * PAGE + 4)
        self.assertEqual(own.exports(), img.exports())


class Writing(unittest.TestCase):
    def test_a_segment_packs_to_what_was_read(self):
        o, sects = an_object()
        for buf in (o.build(), Reading().kernel()):
            for cmd, raw in mkmacho.commands(buf):
                if cmd == LC_SEGMENT_64:
                    seg = macho.Segment(raw)
                    self.assertEqual(seg.pack(), raw)
                    for i, s in enumerate(seg.sections):
                        self.assertEqual(s.pack(), raw[72 + 80 * i:152 + 80 * i])

    def test_a_segment_made_from_numbers(self):
        seg = macho.Segment(name='__DATA_CONST', vmaddr=VA + 0x8000, vmsize=0x4000, fileoff=0x8000, filesize=0x3000,
                            prot=3, flags=0x10)
        s = macho.Section(bytes(80))
        s.sectname, s.segname, s.addr, s.size, s.offset, s.align, s.flags = '__const', '__DATA_CONST', VA + 0x8010, \
            0x20, 0x8010, 4, 0x8000000
        seg.sections.append(s)
        raw = seg.pack()
        self.assertEqual(struct.unpack_from('<II', raw), (LC_SEGMENT_64, 152))
        back, = mkmacho.segments(struct.pack('<8I', 0xfeedfacf, 0x0100000c, 2, 2, 1, len(raw), 0, 0) + raw)
        self.assertEqual(back, dict(name='__DATA_CONST', vmaddr=VA + 0x8000, vmsize=0x4000, fileoff=0x8000,
                                    filesize=0x3000, maxprot=3, initprot=3, flags=0x10, sections=[dict(
                                        sectname='__const', segname='__DATA_CONST', addr=VA + 0x8010, size=0x20,
                                        offset=0x8010, align=4, reloff=0, nreloc=0, flags=0x8000000)]))

    def test_names_and_rounding(self):
        self.assertEqual(macho.name16('__TEXT'), b'__TEXT' + bytes(10))
        self.assertEqual(macho.name16('__compact_unwind'), b'__compact_unwind')
        self.assertEqual((macho.cstr(b'__compact_unwind'), macho.cstr(b'ab\0cd\0')), ('__compact_unwind', 'ab'))
        self.assertEqual([macho.align(v, 16) for v in (0, 1, 16, 17)], [0, 16, 16, 32])


def collection(size, segments, fixups, header_va=VA):
    """A file of `size` bytes with macho.kc_write_fixups()'s chains for `fixups` and, at its end, the payload, the
    LC_DYLD_CHAINED_FIXUPS of its header pointing there. Returns (bytes, (payload offset, size))."""
    out = bytearray(size)
    blob = macho.kc_write_fixups(out, header_va, segments, fixups)
    out[0:48] = struct.pack('<8I', 0xfeedfacf, 0x0100000c, 2, macho.MH_FILESET, 1, 16, 0, 0) + \
        struct.pack('<4I', LC_DYLD_CHAINED_FIXUPS, 16, size, len(blob))
    return bytes(out) + blob, (size, len(blob))


SEGMENTS = [('__TEXT', 0, PAGE), ('__DATA_CONST', PAGE, 2 * PAGE), ('__TEXT_EXEC', 3 * PAGE, PAGE),
            ('__DATA', 4 * PAGE, 3 * PAGE)]


class ChainedFixups(unittest.TestCase):
    def test_one_pointer_bit_by_bit(self):
        # dyld_chained_ptr_64_kernel_cache_rebase: target:30 cacheLevel:2 diversity:16 addrDiv:1 key:2 next:12 isAuth:1
        self.assertEqual(macho.kc_encode(0x1234567, None, 0, 0, 0), 0x1234567)
        self.assertEqual(macho.kc_encode(0x3fffffff, None, 0, 0, 0xfff), 0x3fffffff | 0xfff << 51)
        self.assertEqual(macho.kc_encode(0x10, KEY_DB, 0xbeef, 1, 5),
                         0x10 | 0xbeef << 32 | 1 << 48 | 3 << 49 | 5 << 51 | 1 << 63)
        self.assertEqual(macho.kc_encode(0, KEY_IA, 0, 0, 0), 1 << 63)
        for target in (-1, 1 << 30):
            with self.assertRaises(ValueError):
                macho.kc_encode(target, None, 0, 0, 0)

    def test_decoding_is_the_inverse(self):
        for key in (None, KEY_IA, KEY_IB, KEY_DA, KEY_DB):
            for div, addr, nxt in ((0, 0, 0), (0xffff, 1, 0xfff), (0x8001, 0, 1)):
                raw = macho.kc_encode(0x2aaaaaaa, key, div, addr, nxt)
                p = mkmacho.Pointer(raw)
                self.assertEqual((p.target, p.cache_level, p.next), (0x2aaaaaaa, 0, nxt))
                self.assertEqual(p.signing(), None if key is None else (key, div, addr))
                self.assertEqual(macho.kc_decode(raw), (0x2aaaaaaa, 0, key, div if key is not None else 0,
                                                        addr if key is not None else 0, nxt))
        self.assertEqual(macho.kc_decode(0x10 | 2 << 30)[:2], (0x10, 2))

    def fixups(self):
        """Pointers in two of four segments: the first and the last word of a page, one over a page's end, one
        alone on its page, pages with none."""
        f = {}
        for off in (PAGE, PAGE + 8, PAGE + 0x3ffc, 2 * PAGE + 4, 2 * PAGE + 0x3ff8):       # __DATA_CONST
            f[off] = macho.Fixup(VA + 0x100 + off)
        f[PAGE + 8] = macho.Fixup(VA + 0x3fffffff, KEY_IA, 0xe4fe, 0)
        f[5 * PAGE + 0x10] = macho.Fixup(VA, KEY_DA, 0xffff, 1)                            # __DATA, its second page
        f[6 * PAGE + 0x3ff8] = macho.Fixup(VA + 4 * PAGE)                                  # its third
        return f

    def test_the_chains_written(self):
        fixups = self.fixups()
        buf, payload = collection(8 * PAGE, SEGMENTS, dict(reversed(list(fixups.items()))))
        header, starts, pointers = mkmacho.chained_fixups(buf)
        # dyld_chained_fixups_header: version 0, no imports (their offsets past the starts), symbols uncompressed
        self.assertEqual((header[0], header[4], header[6]), (0, 0, 0))
        self.assertEqual(header[2], header[3])
        self.assertEqual(header[2], payload[1])
        self.assertEqual([s is not None for s in starts], [False, True, False, True])
        dc, data = starts[1], starts[3]
        for s, off, pages in ((dc, PAGE, [0, 4]), (data, 4 * PAGE, [0xffff, 0x10, 0x3ff8])):
            self.assertEqual((s['page_size'], s['pointer_format'], s['segment_offset'], s['max_valid_pointer']),
                             (PAGE, 8, off, 0))
            self.assertEqual(s['page_start'], pages)
            self.assertEqual(s['size'], 22 + 2 * len(pages))    # as far as the last page_start
            self.assertEqual(s['offset'] % 8, 0)
        self.assertLessEqual(dc['offset'] + dc['size'], data['offset'])
        self.assertLessEqual(data['offset'] + data['size'], payload[1])
        # every pointer is on a chain, and says what it was given
        self.assertEqual(sorted(pointers), sorted(fixups))
        for off, p in pointers.items():
            f = fixups[off]
            self.assertEqual((p.target, p.cache_level), (f.target - VA, 0), hex(off))
            self.assertEqual(p.signing(), None if f.key is None else (f.key, f.div, f.addr_div), hex(off))
        # next: in units of 4 bytes to the next pointer of the page, 0 at the last one
        self.assertEqual([pointers[off].next for off in sorted(pointers)],
                         [2, (0x3ffc - 8) // 4, 0, (0x3ff8 - 4) // 4, 0, 0, 0])
        # nothing else of the file is touched
        blank = bytearray(buf[:8 * PAGE])
        for off in fixups:
            self.assertNotEqual(blank[off:off + 8], bytes(8))
            blank[off:off + 8] = bytes(8)
        self.assertEqual(blank[48:], bytes(8 * PAGE - 48))

    def test_read_back(self):
        fixups = self.fixups()
        buf, payload = collection(8 * PAGE, SEGMENTS, fixups)
        back = macho.kc_read_fixups(buf, VA, None)
        self.assertEqual({off: (f.target, f.key, f.div, f.addr_div) for off, f in back.items()},
                         {off: (f.target, f.key, f.div, f.addr_div) for off, f in fixups.items()})
        no_command = struct.pack('<8I', 0xfeedfacf, 0x0100000c, 2, 12, 0, 0, 0, 0)
        self.assertEqual(macho.kc_read_fixups(no_command, VA, None), {})

    def test_the_farthest_two_pointers_of_a_page(self):
        """next has 12 bits and counts 4 bytes: from a page's first word its last (4095 words on) is in reach."""
        fixups = {PAGE: macho.Fixup(VA), PAGE + 0x3ffc: macho.Fixup(VA + 8)}
        buf, payload = collection(8 * PAGE, SEGMENTS, fixups)
        header, starts, pointers = mkmacho.chained_fixups(buf)
        self.assertEqual([p.next for p in pointers.values()], [0xfff, 0])
        self.assertEqual(sorted(macho.kc_read_fixups(buf, VA, None)), sorted(fixups))

    def test_no_pointers(self):
        buf, payload = collection(8 * PAGE, SEGMENTS, {})
        header, starts, pointers = mkmacho.chained_fixups(buf)
        self.assertEqual((starts, pointers), ([None] * 4, {}))
        self.assertEqual(macho.kc_read_fixups(buf, VA, None), {})

    def test_pointers_that_cannot_be_written(self):
        for off, why in ((PAGE + 2, 'misaligned'), (8 * PAGE, 'outside'), (-8, 'outside')):
            with self.assertRaises(ValueError, msg=hex(off)) as e:
                collection(8 * PAGE, SEGMENTS, {off: macho.Fixup(VA)})
            self.assertIn(why, str(e.exception))
        gap = [('__TEXT', 0, PAGE), ('__DATA', 2 * PAGE, PAGE)]
        with self.assertRaises(ValueError):
            collection(8 * PAGE, gap, {PAGE + 8: macho.Fixup(VA)})
        for target in (VA - 8, VA + (1 << 30)):                 # 30 bits of offset from the collection's header
            with self.assertRaises(ValueError):
                collection(8 * PAGE, SEGMENTS, {PAGE: macho.Fixup(target)})

    def test_chains_that_are_not_a_collections(self):
        fixups = self.fixups()
        buf, (at, size) = collection(8 * PAGE, SEGMENTS, fixups)
        header, starts, pointers = mkmacho.chained_fixups(buf)
        info = at + starts[1]['offset']

        def changed(off, fmt, value):
            b = bytearray(buf)
            struct.pack_into(fmt, b, off, value)
            return bytes(b)

        first = struct.unpack_from('<Q', buf, PAGE)[0]
        for what, bad in (('imports', changed(at + 16, '<I', 1)),
                          ('the pointer format', changed(info + 6, '<H', 2)),               # DYLD_CHAINED_PTR_64
                          ('several chains in a page', changed(info + 22, '<H', 0x8000)),   # ..._START_MULTI
                          ('another cache level', changed(PAGE, '<Q', first | 1 << 30))):
            with self.assertRaises(ValueError, msg=what):
                macho.kc_read_fixups(bad, VA, None)


class ThreadedRebases(unittest.TestCase):
    TEXT, DATA = VA + PAGE, VA + 0x100000

    def kernel(self, chains, stride=4):
        return macho.Image(mkmacho.kernel([('__TEXT', self.TEXT, bytes(2 * PAGE), []),
                                           ('__DATA', self.DATA, bytes(2 * PAGE), [('__data', 0, 2 * PAGE, 0)])],
                                          chains=chains, stride=stride))

    def rebases(self, chains, stride=4):
        return {va: (f.target, None if f.key is None else (f.key, f.div, f.addr_div))
                for va, f in macho.threaded_rebases(self.kernel(chains, stride)).items()}

    def test_plain_and_signed_pointers(self):
        d = self.DATA
        chains = [[(d, self.TEXT + 0x10, None), (d + 8, 0x0000000100004000, None), (d + 0x14, 0x8000000100004008, None),
                   (d + 0x20, self.TEXT + 0x1ff0, (KEY_IA, 0x1234, 0)), (d + 0x28, d + 0x7ff8, (KEY_DB, 0xffff, 1))],
                  [(d + 0x3ffc, d + 0x4000, (KEY_IB, 0, 0)), (d + 0x4004, 0xffffffffffffffff, None)]]
        self.assertEqual((d + 0x3ffc) % PAGE, PAGE - 4)         # (that one lies over the end of a page)
        self.assertEqual(self.rebases(chains), {va: (target, sign) for chain in chains for va, target, sign in chain})

    def test_every_chain_of_the_starts(self):
        d = self.DATA
        chains = [[(d + 0x100, d, None)], [(d + 0x200, d + 8, None), (d + 0x200 + 4 * 0x7ff, d + 16, None)],
                  [(d + 0x4000, d + 24, (KEY_DA, 7, 1))]]
        self.assertEqual(self.rebases(chains), {va: (target, sign) for chain in chains for va, target, sign in chain})

    def test_a_stride_of_eight(self):
        d = self.DATA
        chain = [(d, d + 0x40, None), (d + 8, d + 0x48, (KEY_IA, 1, 0)), (d + 8 + 8 * 0x7ff, d + 0x50, None)]
        self.assertEqual(self.rebases([chain], stride=8), {va: (target, sign) for va, target, sign in chain})

    def test_none(self):
        self.assertEqual(self.rebases([]), {})
        o, sects = an_object()
        self.assertEqual(macho.threaded_rebases(macho.Image(o.build())), {})

    def test_a_bind_is_refused(self):
        img = self.kernel([[(self.DATA, self.DATA + 8, None)]])
        buf = bytearray(img.buf)
        off = img.va_to_off(self.DATA)
        struct.pack_into('<Q', buf, off, struct.unpack_from('<Q', buf, off)[0] | 1 << 62)
        with self.assertRaises(ValueError):
            macho.threaded_rebases(macho.Image(buf))


if __name__ == '__main__':
    unittest.main()
