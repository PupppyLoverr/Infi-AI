#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-only
"""scripts/kclink/kccompare.py on collections that are the same and on ones made to differ in one thing: a word of a
kext's code, a byte of a string, where a pointer goes, how it is signed, a kext's zero-filled data, a byte of the
kernel, a kext's entry in the prelink info, a kext that only one has, the size of a segment of the collection, a key
of the prelink info itself. The collections are those of mkcollection.py.

    python3 tests/kclink/test_kccompare.py
"""
import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import mkmacho                                                                  # noqa: E402
from mkcollection import (BASE, KCCOMPARE, SPEC_ONE, SPEC_THREE, Workdir, kext_one, kext_three)    # noqa: E402

ONE, THREE = 'org.example.kext.one', 'org.example.kext.three'


class Compared(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.w = w = Workdir(cls.addClassCleanup)
        w.template()
        w.kernel()
        cls.link('a.kc')

    @classmethod
    def link(cls, output, one=None, three=None, spec_one=SPEC_ONE, kernel='kernel'):
        """A collection with the kexts One and Three, made of these objects."""
        w = cls.w
        status, out, err = w.link('--native', w.native(spec_one, one or kext_one()),
                                  w.native(SPEC_THREE, three or kext_three('_kernel_data')), output=output,
                                  kernel=kernel)
        assert status == 0, err
        return output

    def compare(self, a, b):
        """kccompare's lines about two collections, after the first (the two headers and counts)."""
        status, out, err = self.w.run(KCCOMPARE, a, b)
        self.assertEqual(err, '')
        lines = out.splitlines()
        self.assertTrue(lines[0].startswith(f'header {BASE:#x} / {BASE:#x}; '), lines[0])
        self.assertEqual(status, 0 if lines[-1] == '0 differences' else 1, 'its exit status says whether they differ')
        return lines[1:]

    def one_difference(self, other, about):
        """The one line kccompare has about `a.kc` and this other collection, whichever is named first."""
        for a, b in (('a.kc', other), (other, 'a.kc')):
            lines = self.compare(a, b)
            self.assertEqual(lines[1:], ['1 differences'], lines)
            self.assertTrue(lines[0].startswith(about), lines[0])
        return self.compare('a.kc', other)[0]

    def test_the_same(self):
        self.assertEqual(self.compare('a.kc', 'a.kc'), ['0 differences'])
        self.assertEqual(self.compare('a.kc', self.link('again.kc')), ['0 differences'])
        status, out, err = self.w.run(KCCOMPARE, 'a.kc', 'a.kc')
        self.assertEqual(status, 0)
        self.assertEqual(out.splitlines()[0], f'header {BASE:#x} / {BASE:#x}; 15 / 15 pointers; 3 / 3 entries')

    def test_a_word_of_code(self):
        line = self.one_difference(self.link('b.kc', one=kext_one(answer=10)),
                                   f'{ONE} __TEXT_EXEC: 0 pointers and 1 words')
        helper = self.w.collection('a.kc').natives['_helper']
        self.assertIn(f"first words ['{helper - BASE:#x}']", line)

    def test_a_byte_of_a_string(self):
        self.one_difference(self.link('b.kc', one=kext_one(message=b'hello from a kexT\0')),
                            f'{ONE} __TEXT: 0 pointers and 1 words')

    def test_where_a_pointer_goes(self):
        line = self.one_difference(self.link('b.kc', three=kext_three('_kernel_func')),
                                   f'{THREE} __DATA: 1 pointers and 0 words')
        a = self.w.collection('a.kc')
        self.assertIn(f'{a.natives["_three_pointer"] - BASE:#x}', line)

    def test_how_a_pointer_is_signed(self):
        line = self.one_difference(self.link('b.kc', one=kext_one(diversity=0x2222)),
                                   f'{ONE} __DATA: 1 pointers and 0 words')
        self.assertIn('div 0x1111', line)
        self.assertIn('div 0x2222', line)

    def test_a_pointer_where_the_other_has_none(self):
        plain = kext_three('_kernel_data')
        del plain[0].sects[1][7][:]                             # its pointer's relocation: 8 bytes of zero stay
        self.one_difference(self.link('b.kc', three=plain), f'{THREE} __DATA: 1 pointers and 0 words')

    def test_zero_filled_data(self):
        more = kext_three('_kernel_data')
        more[0].section('__DATA', '__bss', zerofill=64)
        self.one_difference(self.link('b.kc', three=more), f'{THREE}: segments differ')

    def test_a_byte_of_the_kernel(self):
        buf = bytearray(self.w.read('kernel'))
        data = next(s for s in mkmacho.segments(buf) if s['name'] == '__DATA')
        buf[data['fileoff'] + 0x20] ^= 1
        self.w.write('kernel.b', bytes(buf))
        self.one_difference(self.link('b.kc', kernel='kernel.b'), 'com.apple.kernel __DATA: 0 pointers and 1 words')

    def test_a_kexts_entry_in_the_prelink_info(self):
        spec = dict(SPEC_ONE, libraries={'com.apple.kpi.libkern': '9.0'})
        line = self.one_difference(self.link('b.kc', spec_one=spec), f'prelink info {ONE}: ')
        self.assertIn("('OSBundleLibraries', {'com.apple.kpi.libkern': '8.0'}, {'com.apple.kpi.libkern': '9.0'})", line)

    def test_a_kext_that_only_one_has(self):
        """Its entry in the collection and its entry in the prelink info: two differences, not none."""
        w = self.w
        status, out, err = w.link('--native', w.native(SPEC_ONE, kext_one()), output='b.kc')
        self.assertEqual((status, err), (0, ''))
        for a, b in (('a.kc', 'b.kc'), ('b.kc', 'a.kc')):
            self.assertEqual(self.compare(a, b), [f"  entries in one only: ['{THREE}']",
                                                  f"prelink info: entries in one only: ['{THREE}']", '2 differences'])

    def test_a_segment_of_the_collection(self):
        """A prelink info of two pages where the other's has one: __PRELINK_INFO is another size."""
        spec = dict(SPEC_ONE, plist={'Padding': 'x' * 0x4000})
        lines = self.compare('a.kc', self.link('b.kc', spec_one=spec))
        self.assertEqual(len(lines), 3, lines)
        self.assertEqual(lines[0], f'  segment __PRELINK_INFO {BASE + 0x40000:#x}+0x4000 / __PRELINK_INFO '
                                   f'{BASE + 0x40000:#x}+0x8000')
        self.assertTrue(lines[1].startswith(f"prelink info {ONE}: [('Padding', None, 'xxxx"), lines[1])
        self.assertEqual(lines[2], '2 differences')

    def test_a_key_of_the_prelink_info_itself(self):
        template = self.w.read('template.kc')
        self.assertEqual(template.count(b'xnulinux-free-kc'), 1)
        self.w.write('other.kc', template.replace(b'xnulinux-free-kc', b'xnulinux-free-kd'))
        w = self.w
        status, out, err = w.link('--native', w.native(SPEC_ONE, kext_one()),
                                  w.native(SPEC_THREE, kext_three('_kernel_data')), base='other.kc', output='b.kc')
        self.assertEqual((status, err), (0, ''))
        self.assertEqual(self.compare('a.kc', 'b.kc'), ['prelink info: top-level keys differ', '1 differences'])


if __name__ == '__main__':
    unittest.main()
