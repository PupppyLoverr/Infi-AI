#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-only
"""A whole kernel collection, the way scripts/free-kc/build.sh makes one: mk-template.py's empty collection, a kernel
and a kext from objects linked into it by kclink.py. Nothing of a real kernel is used: the kernel is a stand-in of a
few pages with threaded rebases and four symbols, the kext is two objects built here, XNU's pseudo-extension property
lists are made up (mkcollection.py). Checked: the empty collection; the linked one's load commands, the kernel and the
kext in it, every chained fixup (walked by a reader of its own: page starts, the last pointer of a page, one over a
page's end) and the prelink info.

    python3 tests/kclink/test_kclink_collection.py
"""
import json
import os
import plistlib
import struct
import sys
import unittest
import uuid

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import mkmacho                                                                  # noqa: E402
from mkmacho import (KEY_IA, LC_DYLD_CHAINED_FIXUPS, LC_FILESET_ENTRY, LC_FUNCTION_STARTS, LC_SEGMENT_64,  # noqa: E402
                     LC_SYMTAB, LC_UNIXTHREAD, LC_UUID, MH_EXECUTE, MH_FILESET, MH_KEXT_BUNDLE, S_ZEROFILL,
                     adrp_page, branch_target, imm12, movz)
from mkcollection import (BASE, KPI, LAYOUT, MK_TEMPLATE, NO_ADDRESS, PAGE, REPO, SPEC_ONE, VERSION,  # noqa: E402
                          Workdir, kext_one, region)
import macho                                                                    # noqa: E402


class Template(unittest.TestCase):
    def setUp(self):
        self.w = Workdir(self.addCleanup)
        self.buf = self.w.template()

    def test_the_load_commands(self):
        magic, cpu, sub, filetype, ncmds, size, flags, _ = struct.unpack_from('<8I', self.buf)
        self.assertEqual((magic, cpu, sub & 0xffffff, filetype), (0xfeedfacf, 0x0100000c, 2, MH_FILESET))
        cmds = mkmacho.commands(self.buf)
        self.assertEqual(sum(len(raw) for cmd, raw in cmds), size)
        self.assertEqual([cmd for cmd, raw in cmds], [LC_UUID, 0x32, LC_UNIXTHREAD, LC_DYLD_CHAINED_FIXUPS] +
                         [LC_SEGMENT_64] * 7 + [LC_FILESET_ENTRY])
        self.assertTrue(all(len(raw) % 8 == 0 for cmd, raw in cmds))
        self.assertEqual(len(cmds[2][1]), 288)                  # room for the kernel's thread state
        self.assertEqual(mkmacho.fileset_entries(self.buf), {'com.apple.kernel': (BASE + 0x4000, 0x4000)})
        self.assertLessEqual(32 + size, 0x4000)

    def test_the_segments_are_the_layouts(self):
        segs = mkmacho.segments(self.buf)
        self.assertEqual([[s['name'], hex(s['fileoff']), hex(s['filesize']), s['initprot'],
                           s['sections'][0]['sectname'] if s['sections'] else None] for s in segs], LAYOUT['segments'])
        for s in segs:
            self.assertEqual((s['vmaddr'], s['vmsize'], s['maxprot']),
                             (BASE + s['fileoff'], s['filesize'], s['initprot']))
            for sect in s['sections']:                           # __PRELINK_TEXT,__text and __PRELINK_INFO,__info
                self.assertEqual((sect['addr'], sect['size'], sect['offset'], sect['segname']),
                                 (s['vmaddr'], s['vmsize'], s['fileoff'], s['name']))
        self.assertEqual(len(self.buf), 0x60000)                # it ends where the kernel's link-edit data will start

    def test_nothing_in_it_but_the_prelink_info(self):
        info, size = region(LAYOUT, '__PRELINK_INFO')
        blank = bytearray(self.buf)
        for a, b in ((0, 32 + struct.unpack_from('<I', self.buf, 20)[0]), (0x4000, 0x4000 + 32 + 144),
                     (info, info + size), (0x5c000, 0x5c020)):
            blank[a:b] = bytes(b - a)
        self.assertEqual(blank, bytes(len(self.buf)))

    def test_the_kernels_slot(self):
        """A header where the kernel's will be, with the kernel's __LINKINFO slot and the shared __LINKEDIT."""
        img = macho.Image(self.buf, 0x4000)
        self.assertEqual((img.filetype, [s.name for s in img.segments]), (MH_EXECUTE, ['__LINKINFO', '__LINKEDIT']))
        self.assertEqual([(s.vmaddr - BASE, s.vmsize, s.fileoff) for s in img.segments],
                         [(0x58000, 0x4000, 0x58000), (0x60000, 0, 0x60000)])

    def test_chained_fixups_with_no_chains(self):
        header, starts, pointers = mkmacho.chained_fixups(self.buf)
        self.assertEqual((header[0], header[4], starts, pointers), (0, 0, [], {}))
        raw = next(raw for cmd, raw in mkmacho.commands(self.buf) if cmd == LC_DYLD_CHAINED_FIXUPS)
        off, size = struct.unpack_from('<II', raw, 8)
        self.assertEqual(off, 0x5c000)                          # after the kernel's __LINKINFO slot
        self.assertLessEqual(header[1] + 4, size)
        self.assertEqual(macho.kc_read_fixups(self.buf, BASE, None), {})

    def test_the_prelink_info(self):
        info, size = region(LAYOUT, '__PRELINK_INFO')
        raw = self.buf[info:info + size].rstrip(b'\0')
        self.assertEqual(self.buf[info + len(raw)], 0)          # a C string
        d = plistlib.loads(raw)
        self.assertEqual(sorted(d), ['KMBuildVersion', 'ManagerVersion', 'OSPrelinkInfoContainsUUIDs',
                                     '_PrelinkInfoDictionary', '_PrelinkKCID'])
        self.assertEqual(len(d['_PrelinkKCID']), 16)
        self.assertEqual([e['CFBundleIdentifier'] for e in d['_PrelinkInfoDictionary']], list(KPI.values()))
        for (bundle, ident), e in zip(KPI.items(), d['_PrelinkInfoDictionary']):
            self.assertEqual((e['CFBundleVersion'], e['CFBundleShortVersionString']), ('27.2.0', '27.2'))
            self.assertEqual(e['_PrelinkBundlePath'], f'/System/Library/Extensions/System.kext/PlugIns/{bundle}.kext')
            self.assertEqual((e['_PrelinkExecutableLoadAddr'], e['_PrelinkExecutableSourceAddr']),
                             (NO_ADDRESS, NO_ADDRESS))          # no code: the kernel's own symbols
            self.assertIs(e['OSKernelResource'], True)

    def test_the_same_from_the_same_and_another_for_another_kernel(self):
        self.assertEqual(self.w.template('again.kc'), self.buf)
        other = self.w.template('other.kc', version='27.3.0')
        self.assertNotEqual(other, self.buf)
        ids = [plistlib.loads(b[0x40000:0x48000].rstrip(b'\0'))['_PrelinkKCID'] for b in (self.buf, other)]
        self.assertNotEqual(ids[0], ids[1])
        uuids = [next(raw for cmd, raw in mkmacho.commands(b) if cmd == LC_UUID) for b in (self.buf, other)]
        self.assertNotEqual(uuids[0], uuids[1])

    def test_a_pseudo_extension_that_is_missing(self):
        os.remove(self.w.path('xnu', 'config', 'System.kext', 'PlugIns', 'Mach.kext', 'Info.plist'))
        status, out, err = self.w.run(MK_TEMPLATE, 'layout.json', 'xnu', VERSION, 'none.kc')
        self.assertNotEqual(status, 0)
        self.assertFalse(os.path.exists(self.w.path('none.kc')))

    def test_the_layout_of_the_repository(self):
        """scripts/free-kc/layout.json: the segments follow each other on page boundaries, the kernel's header is at
        the start of __PRELINK_TEXT, its __LINKINFO slot and the chained fixups are in __LINKEDIT, and mk-template.py
        makes a collection of it."""
        with open(os.path.join(REPO, 'scripts', 'free-kc', 'layout.json')) as f:
            layout = json.load(f)
        end = 0
        for name, off, size, prot, sect in layout['segments']:
            self.assertEqual((int(off, 16), int(size, 16) % PAGE), (end, 0), name)
            end += int(size, 16)
        self.assertEqual([n for n, *rest in layout['segments']], [n for n, *rest in LAYOUT['segments']])
        self.assertEqual(int(layout['kernel_header'], 16), region(layout, '__PRELINK_TEXT')[0])
        self.assertEqual(int(layout['kernel_linkinfo'][0], 16), region(layout, '__LINKEDIT')[0])
        self.assertEqual(int(layout['kernel_linkedit'], 16), end)
        w = Workdir(self.addCleanup, layout)
        buf = w.template()
        self.assertEqual(len(buf), end)
        self.assertEqual([(s['name'], s['fileoff'], s['filesize']) for s in mkmacho.segments(buf)],
                         [(n, int(off, 16), int(size, 16)) for n, off, size, prot, sect in layout['segments']])


class Linked(unittest.TestCase):
    """The template, the stand-in kernel and one kext from two objects, linked by kclink.py."""

    @classmethod
    def setUpClass(cls):
        cls.w = w = Workdir(cls.addClassCleanup)
        w.template()
        cls.ksyms, cls.kpointers = w.kernel()
        status, cls.log, err = w.link('--native', w.native(SPEC_ONE, kext_one()))
        assert status == 0, err
        cls.kc = w.collection()

    def test_the_collections_load_commands(self):
        kc, template = self.kc, self.w.read('template.kc')
        cmds, before = mkmacho.commands(kc.buf), mkmacho.commands(template)
        self.assertEqual(struct.unpack_from('<8I', kc.buf)[:4], struct.unpack_from('<8I', template)[:4])
        self.assertEqual(sum(len(raw) for cmd, raw in cmds), struct.unpack_from('<I', kc.buf, 20)[0])
        self.assertEqual([cmd for cmd, raw in cmds], [cmd for cmd, raw in before] + [LC_FILESET_ENTRY])
        self.assertEqual(cmds[:2], before[:2])                  # LC_UUID, LC_BUILD_VERSION
        kernel_thread = next(raw for cmd, raw in mkmacho.commands(self.w.read('kernel')) if cmd == LC_UNIXTHREAD)
        self.assertEqual(cmds[2][1], kernel_thread)
        self.assertEqual(struct.unpack_from('<Q', kernel_thread, 16 + 32 * 8)[0], self.ksyms['_kernel_start'])
        self.assertEqual(list(kc.entries), ['com.apple.kernel', 'org.example.kext.one'])
        for name, (va, off) in kc.entries.items():
            self.assertEqual(va - BASE, off, name)
            self.assertEqual(struct.unpack_from('<I', kc.buf, off)[0], 0xfeedfacf, name)

    def test_the_collections_segments(self):
        kc = self.kc
        self.assertEqual(list(kc.segments), [n for n, *rest in LAYOUT['segments']])
        end = 0
        for s in kc.segments.values():
            self.assertEqual((s['vmaddr'] - BASE, s['vmsize']), (s['fileoff'], s['filesize']), s['name'])
            self.assertEqual((s['fileoff'] % PAGE, s['filesize'] % PAGE), (0, 0), s['name'])
            self.assertGreaterEqual(s['fileoff'], end, s['name'])
            end = s['fileoff'] + s['filesize']
        self.assertEqual(end, len(kc.buf))                      # __LINKEDIT runs to the end of the file
        for name in ('__TEXT', '__PRELINK_TEXT', '__DATA_CONST', '__TEXT_EXEC', '__DATA'):
            self.assertEqual((kc.segments[name]['fileoff'], kc.segments[name]['filesize']), region(LAYOUT, name))
        text = kc.segments['__PRELINK_TEXT']
        self.assertEqual((text['sections'][0]['addr'], text['sections'][0]['size']), (text['vmaddr'], text['vmsize']))

    def test_the_kernel_in_it(self):
        kc, kernel = self.kc, self.w.read('kernel')
        off, segs = kc.entry('com.apple.kernel')
        self.assertEqual(off, 0x4000)
        magic, cpu, sub, filetype, ncmds, size, flags, _ = struct.unpack_from('<8I', kc.buf, off)
        self.assertEqual(filetype, MH_EXECUTE)
        self.assertEqual(flags, struct.unpack_from('<I', kernel, 24)[0] | 0x80000000)    # MH_DYLIB_IN_CACHE
        cmds, before = mkmacho.commands(kc.buf, off), mkmacho.commands(kernel)
        self.assertEqual([cmd for cmd, raw in cmds], [cmd for cmd, raw in before if cmd != LC_FUNCTION_STARTS])
        was = {s['name']: s for s in mkmacho.segments(kernel)}
        self.assertEqual(list(segs), list(was))
        for name, s in segs.items():
            if name == '__LINKEDIT':
                continue
            self.assertEqual((s['vmaddr'], s['vmsize'], s['filesize']),
                             (was[name]['vmaddr'], was[name]['vmsize'], was[name]['filesize']), name)
            self.assertEqual(s['fileoff'], s['vmaddr'] - BASE, name)
            for sect, old in zip(s['sections'], was[name]['sections']):
                self.assertEqual((sect['sectname'], sect['addr'], sect['size'], sect['flags']),
                                 (old['sectname'], old['addr'], old['size'], old['flags']))
                self.assertEqual(sect['offset'], 0 if sect['flags'] == S_ZEROFILL else sect['addr'] - BASE)
            # its bytes, but for the pointers and the header (both rewritten)
            mine = bytearray(kc.buf[s['fileoff']:s['fileoff'] + s['filesize']])
            theirs = bytearray(kernel[was[name]['fileoff']:was[name]['fileoff'] + s['filesize']])
            for buf in (mine, theirs):
                if name == '__TEXT':
                    buf[0:0x1000] = bytes(0x1000)
                for va, target, sign in self.kpointers:
                    if s['vmaddr'] <= va < s['vmaddr'] + s['vmsize']:
                        buf[va - s['vmaddr']:va - s['vmaddr'] + 8] = bytes(8)
            self.assertEqual(mine, theirs, name)
        # its symbols, in the link-edit data every entry shares to the end of the file
        self.assertEqual(mkmacho.symtab(kc.buf, off), mkmacho.symtab(kernel))
        le = segs['__LINKEDIT']
        self.assertEqual(le['fileoff'] + le['filesize'], len(kc.buf))
        self.assertEqual(le['vmaddr'] - BASE, le['fileoff'])
        symoff = struct.unpack_from('<I', next(raw for cmd, raw in cmds if cmd == LC_SYMTAB), 8)[0]
        self.assertGreaterEqual(symoff, le['fileoff'])

    def test_the_kext_in_it(self):
        kc, n = self.kc, self.kc.natives
        off, segs = kc.entry('org.example.kext.one')
        magic, cpu, sub, filetype, ncmds, size, flags, _ = struct.unpack_from('<8I', kc.buf, off)
        self.assertEqual((filetype, sub & 0xffffff), (MH_KEXT_BUNDLE, 2))
        self.assertEqual(list(segs), ['__TEXT', '__TEXT_EXEC', '__DATA', '__DATA_CONST', '__LINKEDIT'])
        inside = {'__TEXT': '__PRELINK_TEXT', '__TEXT_EXEC': '__TEXT_EXEC', '__DATA': '__DATA',
                  '__DATA_CONST': '__DATA_CONST'}
        _, kernel = kc.entry('com.apple.kernel')
        for name, where in inside.items():
            s, r = segs[name], kc.segments[where]
            self.assertEqual(s['fileoff'], s['vmaddr'] - BASE, name)
            self.assertTrue(r['vmaddr'] <= s['vmaddr'] and s['vmaddr'] + s['vmsize'] <= r['vmaddr'] + r['vmsize'], name)
            for k in kernel.values():                           # and in none of the kernel's
                self.assertTrue(s['vmaddr'] + s['vmsize'] <= k['vmaddr'] or k['vmaddr'] + k['vmsize'] <= s['vmaddr'],
                                f'{name} and the kernel\'s {k["name"]}')
        self.assertEqual(segs['__TEXT']['vmaddr'], kc.entries['org.example.kext.one'][0])
        self.assertEqual(segs['__LINKEDIT']['fileoff'] + segs['__LINKEDIT']['filesize'], len(kc.buf))
        # its code, resolved against itself and the kernel
        start = n['_one_start']
        self.assertEqual(start, segs['__TEXT_EXEC']['vmaddr'])
        self.assertEqual(branch_target(kc.u32(start), start), n['_helper'])
        self.assertEqual(kc.u32(n['_helper']), movz(0, 9))
        self.assertEqual(branch_target(kc.u32(start + 4), start + 4), self.ksyms['_kernel_func'])
        slot = adrp_page(kc.u32(start + 8), start + 8) + (imm12(kc.u32(start + 12)) << 3)
        self.assertEqual(kc.pointer(slot), (self.ksyms['_kernel_data'], None))
        message = adrp_page(kc.u32(start + 16), start + 16) + imm12(kc.u32(start + 20))
        self.assertEqual(message, n['_message'])
        self.assertEqual(kc.buf[message - BASE:message - BASE + 18], b'hello from a kext\0')
        self.assertEqual(sorted(n), ['_helper', '_message', '_one_start', '_one_stop', '_pointers', '_table'])

    def test_every_pointer_is_a_chained_fixup(self):
        kc, n, k = self.kc, self.kc.natives, self.ksyms
        _, segs = kc.entry('org.example.kext.one')
        data = segs['__DATA']['vmaddr']
        got = next(s for s in segs['__DATA_CONST']['sections'] if s['sectname'] == '__got')['addr']
        expected = {va: (target, sign) for va, target, sign in self.kpointers}
        expected.update({
            n['_pointers']: (k['_kernel_data'], None), n['_pointers'] + 8: (n['_one_start'], (KEY_IA, 0x1111, 1)),
            n['_pointers'] + 16: (n['_message'], None),
            n['_table']: (n['_helper'], (KEY_IA, 0, 0)), n['_table'] + 8: (k['_kernel_private'], None),
            got: (k['_kernel_data'], None),
            data + 180: (n['_one_start'], (KEY_IA, 0xe4fe, 0)), data + 188: (n['_one_stop'], (KEY_IA, 0xe4fe, 0))})
        self.assertEqual({BASE + off: (BASE + p.target, p.signing()) for off, p in kc.pointers.items()}, expected)
        self.assertTrue(all(p.cache_level == 0 for p in kc.pointers.values()))
        self.assertEqual(len(expected), 12)

    def test_the_chains(self):
        kc = self.kc
        self.assertEqual((kc.header[0], kc.header[4]), (0, 0))  # version 0, no imports
        self.assertEqual(len(kc.starts), len(kc.segments))      # one entry per segment of the collection, in order
        for s, starts in zip(kc.segments.values(), kc.starts):
            mine = sorted(off for off in kc.pointers if s['fileoff'] <= off < s['fileoff'] + s['filesize'])
            if not mine:
                self.assertIsNone(starts, s['name'])
                continue
            self.assertEqual((starts['pointer_format'], starts['page_size'], starts['segment_offset']),
                             (8, PAGE, s['fileoff']), s['name'])        # DYLD_CHAINED_PTR_64_KERNEL_CACHE
            self.assertEqual(len(starts['page_start']), s['filesize'] // PAGE)
            for page, first in enumerate(starts['page_start']):
                on_page = [off for off in mine if (off - s['fileoff']) // PAGE == page]
                self.assertEqual(first, (on_page[0] - s['fileoff']) % PAGE if on_page else 0xffff,
                                 f'{s["name"]}, page {page}')
                for off, following in zip(on_page, on_page[1:] + [None]):
                    self.assertEqual(kc.pointers[off].next, (following - off) // 4 if following else 0, hex(off))
        # the kernel's pointer over the end of a page is the last of its page; the next page starts after it
        dc = kc.segments['__DATA_CONST']
        self.assertIn(dc['fileoff'] + 0x3ffc, kc.pointers)
        self.assertEqual(kc.starts[2]['page_start'][:2], [0, 8])
        self.assertEqual(kc.pointers[dc['fileoff'] + 0x3ffc].next, 0)
        # what macho.py reads back is what was walked here
        back = macho.kc_read_fixups(kc.buf, BASE, None)
        self.assertEqual({off: (f.target, None if f.key is None else (f.key, f.div, f.addr_div))
                          for off, f in back.items()},
                         {off: (BASE + p.target, p.signing()) for off, p in kc.pointers.items()})

    def test_the_kexts_kmod_info(self):
        kc = self.kc
        _, segs = kc.entry('org.example.kext.one')
        at = kc.kexts['org.example.kext.one']['_PrelinkKmodInfo'] + (1 << 64)
        self.assertEqual(at, segs['__DATA']['vmaddr'])
        nxt, version, ident, name, vers, refs, reflist, address, size = \
            struct.unpack_from('<QIi64s64siQQQ', kc.buf, at - BASE)
        self.assertEqual((nxt, version, ident, refs, reflist), (0, 1, -1, -1, 0))
        self.assertEqual((name.rstrip(b'\0'), vers.rstrip(b'\0')), (b'org.example.kext.one', b'1.0.0'))
        self.assertEqual((address, size), (segs['__TEXT']['vmaddr'], segs['__TEXT']['vmsize']))

    def test_the_prelink_info(self):
        kc = self.kc
        seg = kc.segments['__PRELINK_INFO']
        self.assertTrue(kc.plist.startswith(b'<?xml'))
        self.assertLessEqual(len(kc.plist), seg['filesize'])
        self.assertEqual(seg['filesize'], -(-len(kc.plist) // PAGE) * PAGE)       # as many pages as it takes
        self.assertEqual(kc.buf[seg['fileoff'] + len(kc.plist):seg['fileoff'] + seg['filesize']],
                         bytes(seg['filesize'] - len(kc.plist)))
        # nothing between the elements: a kernel's prelink info has no room for indentation
        body = kc.plist[kc.plist.index(b'<plist'):]
        self.assertNotIn(b'\t', body)
        self.assertEqual(body.count(b'\n'), 1)
        # the template's entries and keys, and the kext's
        before = plistlib.loads(self.w.read('template.kc')[0x40000:0x48000].rstrip(b'\0'))
        self.assertEqual({k: v for k, v in kc.info.items() if k != '_PrelinkInfoDictionary'},
                         {k: v for k, v in before.items() if k != '_PrelinkInfoDictionary'})
        self.assertEqual(kc.info['_PrelinkInfoDictionary'][:7], before['_PrelinkInfoDictionary'])
        self.assertEqual(list(kc.kexts), list(KPI.values()) + ['org.example.kext.one'])
        d = kc.kexts['org.example.kext.one']
        _, segs = kc.entry('org.example.kext.one')
        text = segs['__TEXT']
        self.assertEqual(d['_PrelinkExecutableLoadAddr'], text['vmaddr'] - (1 << 64))
        self.assertEqual(d['_PrelinkExecutableSourceAddr'], text['vmaddr'] - (1 << 64))
        self.assertEqual(d['_PrelinkExecutableSize'], text['vmsize'])
        self.assertEqual((d['CFBundleIdentifier'], d['CFBundleExecutable'], d['CFBundleVersion']),
                         ('org.example.kext.one', 'One', '1.0.0'))
        self.assertEqual(d['CFBundlePackageType'], 'KEXT')
        self.assertEqual((d['OSBundleLibraries'], d['IOKitPersonalities']),
                         (SPEC_ONE['libraries'], SPEC_ONE['personalities']))
        self.assertEqual(d['OSBundleUUID'], uuid.uuid5(uuid.NAMESPACE_URL, 'org.example.kext.one').bytes)
        cmds = mkmacho.commands(kc.buf, kc.entries['org.example.kext.one'][1])
        self.assertEqual(next(raw for cmd, raw in cmds if cmd == LC_UUID)[8:], d['OSBundleUUID'])

    def test_what_it_says(self):
        self.assertIn('kernel: 4 threaded pointers (2 signed)', self.log)
        header = self.kc.entries['org.example.kext.one'][0]
        self.assertIn(f'org.example.kext.one: linked, header at {header:#x}, 8 pointers', self.log)
        self.assertIn(f'wrote out.kc: {len(self.kc.buf):#x} bytes, base {BASE:#x}, 12 chained pointers', self.log)


if __name__ == '__main__':
    unittest.main()
