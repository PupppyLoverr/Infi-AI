#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-only
"""Two pieces of scripts/kclink/kclink.py on their own: the prelink info as it is written (prelink_xml: a property list
with nothing between its elements, which must read back as what plistlib itself writes), and the room handed out to
native kexts (Holes: first fit, aligned).

    python3 tests/kclink/test_kclink_prelink_info.py
"""
import os
import plistlib
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', 'scripts', 'kclink'))
import kclink                                                                   # noqa: E402

INFO = {
    '_PrelinkInfoDictionary': [
        {'CFBundleIdentifier': 'org.example.kext', '_PrelinkExecutableLoadAddr': -0xffffff8f8fc000,
         '_PrelinkExecutableSize': 0x1020, 'OSBundleUUID': bytes(range(16)), 'OSBundleLibraries': {},
         'IOKitPersonalities': {'P': {'IOClass': 'A<B> & C', 'IOProbeScore': 0, 'Flag': True, 'Off': False,
                                      'List': [1, 'two', b'\x03', [], {}]}}},
        {'CFBundleIdentifier': 'com.apple.kpi.bsd', '_PrelinkExecutableLoadAddr': 0x7fffffffffffffff}],
    '_PrelinkKCID': b'\xff' * 16, 'ManagerVersion': 1, 'Text': 'two\r\nlines\rand one\nmore', 'Unicode': 'kéxt',
    'Largest': (1 << 64) - 1, 'Smallest': -1 << 63,
}


class PrelinkInfo(unittest.TestCase):
    def test_it_reads_back_as_what_plistlib_writes(self):
        out = kclink.prelink_xml(INFO)
        self.assertEqual(plistlib.loads(out), plistlib.loads(plistlib.dumps(INFO)))
        self.assertEqual(plistlib.loads(out)['Text'], 'two\nlines\nand one\nmore')     # (as plistlib has them)

    def test_nothing_between_the_elements(self):
        out = kclink.prelink_xml(INFO)
        head, _, body = out.partition(b'<plist version="1.0">')
        self.assertEqual(head, plistlib.dumps(INFO).partition(b'<plist version="1.0">')[0])
        self.assertTrue(head.startswith(b'<?xml version="1.0" encoding="UTF-8"?>\n<!DOCTYPE plist '))
        self.assertTrue(body.endswith(b'</dict></plist>\n'))
        text = body.decode().replace('two\nlines\nand one\nmore', '')
        self.assertNotIn('\t', text)
        self.assertEqual(text.count('\n'), 1)
        self.assertNotIn('> <', text)
        self.assertLess(len(out), len(plistlib.dumps(INFO)))

    def test_one_of_each(self):
        out = kclink.prelink_xml({'b': [True, False, -5, 'a&b<c>', b'\x00\x01\x02', {}, []], 'a': {'k': 1}})
        self.assertEqual(out.partition(b'<plist version="1.0">')[2].decode(),
                         '<dict><key>a</key><dict><key>k</key><integer>1</integer></dict><key>b</key><array><true/>'
                         '<false/><integer>-5</integer><string>a&amp;b&lt;c&gt;</string><data>AAEC</data><dict/>'
                         '<array/></array></dict></plist>\n')

    def test_the_keys_in_one_order_whatever_order_they_came_in(self):
        there = {'b': 1, 'a': {'y': 1, 'x': 2}, 'c': 3}
        back = {'c': 3, 'a': {'x': 2, 'y': 1}, 'b': 1}
        self.assertEqual(kclink.prelink_xml(there), kclink.prelink_xml(back))
        out = kclink.prelink_xml(there)
        self.assertLess(out.index(b'<key>a</key>'), out.index(b'<key>b</key>'))

    def test_what_a_property_list_cannot_hold(self):
        for bad in ({'a': None}, {'a': {1, 2}}, {'a': object()}, {1: 'a'}, {'a': 1 << 64}):
            with self.assertRaises((SystemExit, TypeError, OverflowError), msg=repr(bad)):
                kclink.prelink_xml(bad)


class Holes(unittest.TestCase):
    def test_first_fit(self):
        h = kclink.Holes(0x1000, 0x9000)
        self.assertEqual(h.fit(0x800, 16), 0x1000)
        self.assertEqual(h.fit(0x800, 16), 0x1800)
        self.assertEqual(h.free, [(0x2000, 0x9000)])

    def test_aligned(self):
        h = kclink.Holes(0x1010, 0x9000)
        self.assertEqual(h.fit(0x10, 0x1000), 0x2000)
        self.assertEqual(h.free, [(0x1010, 0x2000), (0x2010, 0x9000)])
        self.assertEqual(h.fit(0x20, 16), 0x1010)               # the room before it is still there
        self.assertEqual(h.fit(0x1000, 0x4000), 0x4000)

    def test_taken_ranges(self):
        h = kclink.Holes(0, 0x10000)
        h.take(0x4000, 0x8000)                                  # the middle: two holes
        self.assertEqual(h.free, [(0, 0x4000), (0x8000, 0x10000)])
        h.take(0x3000, 0x9000)                                  # over the end of one and the start of the other
        self.assertEqual(h.free, [(0, 0x3000), (0x9000, 0x10000)])
        h.take(0x20000, 0x30000)                                # outside: nothing changes
        self.assertEqual(h.free, [(0, 0x3000), (0x9000, 0x10000)])
        self.assertEqual(h.fit(0x4000, 16), 0x9000)             # too large for the first
        self.assertEqual(h.fit(0x3000, 16), 0)                  # exactly the first
        self.assertEqual(h.free, [(0xd000, 0x10000)])

    def test_no_room(self):
        h = kclink.Holes(0x10, 0x1000)
        self.assertIsNone(h.fit(0xff1, 1))                      # a byte too many
        self.assertIsNone(h.fit(0x800, 0x1000))                 # small enough, but not from the next page boundary
        self.assertEqual(h.free, [(0x10, 0x1000)])
        self.assertEqual(h.fit(0xff0, 1), 0x10)
        self.assertEqual(h.free, [])


if __name__ == '__main__':
    unittest.main()
