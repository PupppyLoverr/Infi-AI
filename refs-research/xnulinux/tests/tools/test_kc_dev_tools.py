#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-only
"""tools/dev/kc*.py (kcdis, kcstrs, kcxref, kcblraa) on a stand-in collection made here: a Mach-O header, two segments,
a few dozen instructions written as numbers and three strings, at a kernel's addresses. What each tool prints is
checked against what the instructions say, worked out here from their encodings.

kcdis needs llvm-objdump (on the path, as llvm-objdump or llvm-objdump-NN, or Xcode's): its tests are skipped where
there is none. The others need python3 only.

    python3 tests/tools/test_kc_dev_tools.py
"""
import glob
import os
import shutil
import struct
import subprocess
import sys
import tempfile
import unittest

DEV = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', 'tools', 'dev')
TEXT, EXEC = 0xfffffff007004000, 0xfffffff007f9c000     # the two segments' addresses (their file offsets: 0, 0x4000)
START = EXEC + 0xc24                                    # where the code is: not at a page's start
STRINGS = TEXT + 0x48c
NOP, RET = 0xd503201f, 0xd65f03c0


def at(i):
    return START + 4 * i


def rel(target, pc, bits, shift=2):
    """A pc-relative immediate of this many bits."""
    delta = target - pc
    assert delta % (1 << shift) == 0 and -(1 << (bits + shift - 1)) <= delta < 1 << (bits + shift - 1), hex(delta)
    return delta >> shift & (1 << bits) - 1


def bl(i, target):
    return 0x94000000 | rel(target, at(i), 26)


def b(i, target):
    return 0x14000000 | rel(target, at(i), 26)


def adrp(i, rd, target):
    pages = rel(target & ~0xfff, at(i) & ~0xfff, 21, 12)
    return 0x90000000 | (pages & 3) << 29 | (pages >> 2) << 5 | rd


def adr(i, rd, target):
    delta = rel(target, at(i), 21, 0)
    return 0x10000000 | (delta & 3) << 29 | (delta >> 2) << 5 | rd


def add(rd, rn, imm):
    return 0x91000000 | imm << 10 | rn << 5 | rd


def ldr(rt, rn, off):                                   # ldr xt, [xn, #off]
    return 0xf9400000 | off // 8 << 10 | rn << 5 | rt


def mov_x17(imm):
    return 0xd2800011 | imm << 5


def blraa(rn, rm=17):
    return 0xd73f0800 | rn << 5 | rm


def code():
    """The instructions, and for kcdis what each is: (mnemonic, the address its operand must name or None); (None,
    None) for those it is not asked about."""
    words, listing = [], []

    def put(mnemonic, target, word):
        words.append(word(len(words)))
        listing.append((mnemonic, target))

    def more(*insns):
        for insn in insns:
            put(None, None, insn if callable(insn) else lambda i, insn=insn: insn)

    far, data = EXEC + 0x2b00000, TEXT + 0x2bd9234       # 43 MB on (in no segment), and off the code's own page
    # 0: every kind of pc-relative operand
    put('bl', far, lambda i: bl(i, far))
    put('bl', START - 0x800, lambda i: bl(i, START - 0x800))
    put('b', at(40), lambda i: b(i, at(40)))
    put('b.ne', START, lambda i: 0x54000001 | rel(START, at(i), 19) << 5)
    put('cbz', at(40), lambda i: 0xb4000000 | rel(at(40), at(i), 19) << 5)
    put('tbnz', START, lambda i: 0x37180001 | rel(START, at(i), 14) << 5)
    put('adr', at(41), lambda i: adr(i, 9, at(41)))
    put('adrp', data & ~0xfff, lambda i: adrp(i, 8, data))
    put('add', None, lambda i: add(8, 8, data & 0xfff))
    put('ldr', at(42), lambda i: 0x5800000a | rel(at(42), at(i), 19) << 5)
    # 10: a run of zero words, then more (their addresses count on)
    more(0, 0, 0, 0)
    put('bl', START, lambda i: bl(i, START))
    more(NOP)
    # 16: strings (kcstrs): an address, two more made from it by adding, one from a page of its own, one more
    more(lambda i: adrp(i, 19, STRINGS), add(19, 19, STRINGS & 0xfff), add(1, 19, 0x10), add(2, 19, 0x20),
         lambda i: adrp(i, 0, STRINGS + 0x40), add(0, 0, (STRINGS + 0x40) & 0xfff), add(1, 19, 0x10), RET)
    # 24: calls through function pointers (kcblraa): one reached through another pointer, x8 loaded twice; one
    # straight from a field; one whose discriminator is set too far from the load
    more(ldr(8, 0, 8), ldr(8, 8, 0x10), 0xaa0103e0, mov_x17(0x50c7), blraa(8))
    more(ldr(9, 19, 0x28), mov_x17(0x1111), blraa(9))
    more(ldr(10, 0, 0x30), *[NOP] * 14, mov_x17(0x2222), blraa(10))
    # 49: callers (kcxref): two calls of the nop at 15 and, between them, a jump to it and a call of another
    more(lambda i: bl(i, at(15)), lambda i: b(i, at(15)), lambda i: bl(i, at(16)), lambda i: bl(i, at(15)), RET)
    return words, listing


def collection(path):
    """Writes the stand-in collection: __TEXT (the header, the strings) and __TEXT_EXEC (the code)."""
    words, listing = code()
    text = bytearray(0x4000)
    strings = b''.join(s.ljust(n, b'\0') for s, n in ((b'first name', 16), (b'second name', 16),
                                                      (b'the third of the names', 32), (b'\x01\x02\x03', 4)))
    text[0x48c:0x48c + len(strings)] = strings
    segs = [(b'__TEXT', TEXT, 0x4000, 0, 0x4000), (b'__TEXT_EXEC', EXEC, 0x4000, 0x4000, 0x4000)]
    cmds = b''.join(struct.pack('<II16s4Q4I', 0x19, 72, name, va, vmsize, off, size, 5, 5, 0, 0)
                    for name, va, vmsize, off, size in segs)
    text[0:32 + len(cmds)] = struct.pack('<8I', 0xfeedfacf, 0x100000c, 2, 12, len(segs), len(cmds), 0, 0) + cmds
    exec_ = bytearray(0x4000)
    exec_[0xc24:0xc24 + 4 * len(words)] = struct.pack(f'<{len(words)}I', *words)
    with open(path, 'wb') as f:
        f.write(text + exec_)
    return listing


class Tools(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.dir = tempfile.mkdtemp()
        cls.addClassCleanup(shutil.rmtree, cls.dir)
        cls.kc = os.path.join(cls.dir, 'kc')
        cls.listing = collection(cls.kc)
        cls.env = dict(os.environ)

    def run_tool(self, tool, *args):
        """(exit status, the lines it printed, what it wrote to stderr)."""
        args = [a if isinstance(a, str) else f'{a:x}' for a in args]         # (addresses are given in hex)
        r = subprocess.run([sys.executable, os.path.join(DEV, tool), self.kc] + args, capture_output=True, text=True,
                           env=self.env)
        return r.returncode, r.stdout.splitlines(), r.stderr


class Kcdis(Tools):
    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        if not shutil.which('llvm-objdump'):                    # llvm-objdump-18 and the like: under its plain name
            versioned = sorted((p for d in os.get_exec_path() for p in glob.glob(os.path.join(d, 'llvm-objdump-*'))),
                               key=lambda p: -int(p.rsplit('-', 1)[1]) if p.rsplit('-', 1)[1].isdigit() else 0)
            if versioned:
                os.symlink(versioned[0], os.path.join(cls.dir, 'llvm-objdump'))
                cls.env['PATH'] = cls.dir + os.pathsep + cls.env.get('PATH', '')
        r = subprocess.run([sys.executable, os.path.join(DEV, 'kcdis.py'), cls.kc, f'{at(23):x}', '4'],
                           capture_output=True, text=True, env=cls.env)
        if r.stdout.split() != [f'{at(23):x}', 'ret']:
            raise unittest.SkipTest('kcdis has no llvm-objdump that disassembles arm64 here: ' +
                                    (r.stderr.strip().splitlines() or ['no listing'])[-1])

    def listed(self, start, length):
        """kcdis's listing from instruction `start` on: [(address, mnemonic, the address its operand names or None)]."""
        status, lines, err = self.run_tool('kcdis.py', at(start), length)
        self.assertEqual((status, err), (0, ''))
        out = []
        for line in lines:
            address, mnemonic, *operands = line.split()
            named = [int(o, 16) for o in operands if o.startswith('0x')]      # (an immediate is written #0x...)
            out.append((int(address, 16), mnemonic, named[-1] if named else None))
        return out

    def test_pc_relative_operands_are_addresses_of_the_collection(self):
        lines = self.listed(0, '0x40')
        self.assertEqual([address for address, mnemonic, named in lines], [at(i) for i in range(16)])
        for (address, mnemonic, named), (expected, target) in zip(lines, self.listing):
            if expected:
                self.assertEqual((mnemonic, named), (expected, target), f'the instruction at {address:x}')
        self.assertEqual(sum(target is not None for expected, target in self.listing[:16]), 10)

    def test_the_addresses_after_a_run_of_zero_words(self):
        lines = self.listed(9, '24')
        self.assertEqual([address for address, mnemonic, named in lines], [at(i) for i in range(9, 15)])
        self.assertEqual(lines[5][1:], ('bl', START))

    def test_from_the_middle_of_a_page_the_page_is_the_instructions_own(self):
        self.assertEqual(self.listed(7, '4'), [(at(7), 'adrp', (TEXT + 0x2bd9234) & ~0xfff)])
        self.assertNotEqual(at(7) & 0xfff, 0)

    def test_a_length_that_is_not_whole_instructions(self):
        for length, count in (('8', 2), ('10', 2), ('0x2a', 10), ('7', 1), ('3', 0)):
            self.assertEqual([address for address, mnemonic, named in self.listed(0, length)],
                             [at(i) for i in range(count)], f'a length of {length}')

    def test_an_address_outside_the_collection(self):
        self.assertEqual(self.run_tool('kcdis.py', TEXT - 0x4000, '8'), (0, ['va not found'], ''))


class Kcstrs(Tools):
    def test_addresses_made_by_adrp_and_add(self):
        status, lines, err = self.run_tool('kcstrs.py', at(16), '0x20')
        self.assertEqual((status, err), (0, ''))
        self.assertEqual(lines, [
            f"{at(17):x}: x19 = {STRINGS:x} 'first name'",
            f"{at(18):x}: x1 = {STRINGS + 0x10:x} 'second name'",              # x19 + 0x10: the address, not its page
            f"{at(19):x}: x2 = {STRINGS + 0x20:x} 'the third of the names'",
            f"{at(21):x}: x0 = {STRINGS + 0x40:x} ",                           # no string there
            f"{at(22):x}: x1 = {STRINGS + 0x10:x} 'second name'"])

    def test_an_address_outside_the_collection(self):
        status, lines, err = self.run_tool('kcstrs.py', TEXT - 0x4000, '8')
        self.assertEqual((status, lines, err), (0, [f'{TEXT - 0x4000:x}: va not found'], ''))
        # and a range that runs out of it: what is inside, then that
        status, lines, err = self.run_tool('kcstrs.py', EXEC + 0x3ff8, '0x10')
        self.assertEqual((status, lines, err), (0, [f'{EXEC + 0x4000:x}: va not found'], ''))


class Kcxref(Tools):
    def test_the_callers_of_a_function(self):
        def callers(lo, hi, target):
            status, lines, err = self.run_tool('kcxref.py', lo, hi, target)
            self.assertEqual((status, err), (0, ''))
            return [int(line, 16) for line in lines]

        self.assertEqual(callers(EXEC, EXEC + 0x4000, at(15)), [at(49), at(52)])
        self.assertEqual(callers(EXEC, EXEC + 0x4000, START), [at(14)])          # a call backward
        self.assertEqual(callers(at(50), at(53), at(15)), [at(52)])              # only the range asked for
        self.assertEqual(callers(TEXT, EXEC + 0x4000, at(17)), [])


class Kcblraa(Tools):
    def test_calls_through_a_function_pointer_field(self):
        status, lines, err = self.run_tool('kcblraa.py', EXEC, EXEC + 0x4000)
        self.assertEqual((status, err), (0, ''))
        self.assertEqual(lines, [
            f'{at(25):x} off 0x10 disc 0x50c7 base x8',        # not the load before it, of the pointer it goes through
            f'{at(29):x} off 0x28 disc 0x1111 base x19',
            '---',
            '0x10 0x50c7',
            '0x28 0x1111'])


if __name__ == '__main__':
    unittest.main()
