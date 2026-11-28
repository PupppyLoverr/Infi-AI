#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-only
"""What tools/hosted/xnu-input.py writes for a command line: the evdev records XnuInput reads (kexts/XnuInput), decoded.

    python3 tests/tools/test_xnu_input.py
"""
import contextlib
import importlib.util
import io
import os
import struct
import sys
import tempfile
import time
import unittest

TOOL = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', 'tools', 'hosted', 'xnu-input.py')
spec = importlib.util.spec_from_file_location('xnu_input', TOOL)
xnu_input = importlib.util.module_from_spec(spec)
spec.loader.exec_module(xnu_input)

EV_SYN, EV_KEY, EV_REL, EV_ABS = 0, 1, 2, 3
REL_WHEEL, SHIFT, CMD, COMMA = 8, 42, 125, 51


class Records(unittest.TestCase):
    def setUp(self):
        self.sleep, time.sleep = time.sleep, lambda seconds: None       # the tool paces its frames; the test need not wait
        self.addCleanup(setattr, time, 'sleep', self.sleep)

    def run_tool(self, *argv):
        """(records, exit status or message): records are (type, code, value) of what the tool appended to its file."""
        f = tempfile.NamedTemporaryFile(delete=False)
        f.close()
        self.addCleanup(os.unlink, f.name)
        status = 0
        with contextlib.redirect_stderr(io.StringIO()):
            try:
                xnu_input.main(['--file', f.name, *argv])
            except SystemExit as e:
                status = e.code
        b = open(f.name, 'rb').read()
        self.assertEqual(len(b) % 8, 0)
        return [struct.unpack_from('<HHi', b, i) for i in range(0, len(b), 8)], status

    def frames(self, records):
        """The records of each frame (what a SYN_REPORT ends), without the SYN."""
        out, cur = [], []
        for r in records:
            if r[0] == EV_SYN:
                out.append(cur)
                cur = []
            else:
                cur.append(r)
        self.assertEqual(cur, [], 'records after the last SYN_REPORT')
        return out

    def test_scroll_is_whole_and_fits_a_report(self):
        # XnuInput's tablet report carries the wheel as one signed byte and clamps a frame's total at -127..127
        for notches in (1, -1, 5, 127, 128, 300, -300, -1000):
            records, status = self.run_tool('scroll', str(notches))
            self.assertEqual(status, 0)
            wheel = [sum(v for t, c, v in f if (t, c) == (EV_REL, REL_WHEEL)) for f in self.frames(records)]
            self.assertTrue(all(-127 <= w <= 127 for w in wheel), f'scroll {notches}: a frame of {wheel}')
            self.assertEqual(sum(wheel), notches, f'scroll {notches}: frames {wheel}')

    def test_key_takes_punctuation_by_its_character(self):
        records, status = self.run_tool('key', 'cmd', ',')              # Settings...
        self.assertEqual(status, 0)
        self.assertEqual(self.frames(records), [[(EV_KEY, CMD, 1), (EV_KEY, COMMA, 1)], [(EV_KEY, COMMA, 0), (EV_KEY, CMD, 0)]])

    def test_key_with_no_key_writes_nothing(self):
        for argv in (('key',), ('key', 'nosuchkey')):
            records, status = self.run_tool(*argv)
            self.assertEqual(records, [])
            self.assertTrue(status, argv)

    def test_key_then_another_command(self):
        records, status = self.run_tool('key', 'cmd', 'space', 'type', 'a')
        self.assertEqual(status, 0)
        self.assertEqual([c for f in self.frames(records) for t, c, v in f if v == 1], [CMD, 57, 30])

    def test_type_presses_shift_for_capitals_and_symbols(self):
        records, status = self.run_tool('type', 'aA1!,<')
        self.assertEqual(status, 0)
        downs = [[c for t, c, v in f if t == EV_KEY and v == 1] for f in self.frames(records)][::2]
        self.assertEqual(downs, [[30], [SHIFT, 30], [2], [SHIFT, 2], [COMMA], [SHIFT, COMMA]])

    def test_type_types_all_of_it_or_nothing(self):
        # a character with no key on the US layout: the text would arrive with a hole in it
        for text in ('café', 'Été', 'ok € ok'):
            records, status = self.run_tool('type', text)
            self.assertEqual(records, [], text)
            self.assertIn('type:', str(status))


if __name__ == '__main__':
    unittest.main()
