#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-only
"""Which definition a kext gets of a symbol that several of its objects define, and which objects it takes at all
(scripts/kclink/kextld.py: Kext.layout(), Obj). The objects here are symbol tables only, so nothing is compiled:

    python3 tests/kclink/test_kextld_definitions.py
"""
import itertools
import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', 'scripts', 'kclink'))
import kextld                                                                   # noqa: E402
from macho import HEADER, MH_MAGIC_64, MH_OBJECT, CPU_TYPE_ARM64, N_EXT, N_SECT, N_UNDF   # noqa: E402

WEAK = 0x80                     # N_WEAK_DEF, in n_desc


class SymbolsOnly:
    """What layout() reads of an object when it has no sections: its path and its symbol table."""
    sects = ()

    def __init__(self, index, syms):
        self.path, self.syms, self.where = f'obj{index}.o', syms, {}


def layout(*tables):
    """Lays out a kext of objects with these symbol tables. A defined symbol's place is its value in one section."""
    kx = kextld.Kext({}, [])
    kx.objs = [SymbolsOnly(i, t) for i, t in enumerate(tables)]
    const = kx.section('__TEXT', '__const')
    kx.place_of = lambda o, sect, value: (const, value)
    kx.layout()
    return kx


def defined(name, value, weak=False):
    return (name, N_SECT | N_EXT, 1, WEAK if weak else 0, value)


def common(name, size, log2align=2):
    return (name, N_UNDF | N_EXT, 0, log2align << 8, size)


def where(kx, name):
    out, off = kx.globals[name]
    return out.name, off


class Definitions(unittest.TestCase):
    def place(self, *entries):
        """entries: ('W' or 'S', place), an object each. The place '_s' ends up at."""
        return where(layout(*[[defined('_s', p, weak=k == 'W')] for k, p in entries]), '_s')[1]

    def test_strong_over_weak_in_either_order(self):
        self.assertEqual(self.place(('W', 0), ('S', 8)), 8)
        self.assertEqual(self.place(('S', 8), ('W', 0)), 8)

    def test_first_of_several_weak_ones(self):
        self.assertEqual(self.place(('W', 8), ('W', 0), ('W', 16)), 8)

    def test_two_strong_ones_are_refused(self):
        with self.assertRaises(kextld.LinkError):
            self.place(('S', 0), ('S', 8))

    def test_weak_and_strong_at_one_place_then_strong_elsewhere(self):
        # one object's symbol at the end of its part of a section and the next object's at the start of its part
        # have the same place: the strong one of the two makes the symbol strong, in either order
        for first, second in (('W', 'S'), ('S', 'W')):
            with self.assertRaises(kextld.LinkError, msg=f'{first}@0 {second}@0 S@8'):
                self.place((first, 0), (second, 0), ('S', 8))

    def test_weak_and_strong_at_one_place_then_weak_elsewhere(self):
        for first, second in (('W', 'S'), ('S', 'W')):
            self.assertEqual(self.place((first, 0), (second, 0), ('W', 8)), 0)

    def test_every_short_sequence_against_the_rule(self):
        """Entries at one place are one definition, strong if any of them is; strong ones at two places are an error;
        otherwise the strong one's place; otherwise the first weak one's."""
        def rule(seq):
            strong = {p for k, p in seq if k == 'S'}
            return 'error' if len(strong) > 1 else strong.pop() if strong else seq[0][1]

        def linked(seq):
            try:
                return self.place(*seq)
            except kextld.LinkError:
                return 'error'

        entries = [(k, p) for k in 'WS' for p in (0, 8, 16)]
        wrong = [seq for n in range(1, 6) for seq in itertools.product(entries, repeat=n) if linked(seq) != rule(seq)]
        self.assertEqual(wrong[:3], [], f'{len(wrong)} sequences differ from the rule')


class Commons(unittest.TestCase):
    def test_a_common_symbol_alone_gets_common_storage(self):
        kx = layout([common('_c', 4)])
        self.assertEqual(where(kx, '_c')[0], '__common')
        self.assertEqual(kx.out[('__DATA', '__common')].size, 4)

    def test_the_largest_of_several_commons(self):
        kx = layout([common('_c', 8, 0)], [common('_c', 32, 3)])
        self.assertEqual(kx.out[('__DATA', '__common')].size, 32)

    def test_a_definition_is_the_symbol_in_either_order(self):
        for weak in (False, True):
            for tables in ([[common('_c', 4)], [defined('_c', 8, weak)]], [[defined('_c', 8, weak)], [common('_c', 4)]]):
                kx = layout(*tables)
                self.assertEqual(where(kx, '_c'), ('__const', 8), f'weak={weak}')
                self.assertNotIn(('__DATA', '__common'), kx.out)      # and no storage is set aside for it

    def test_other_commons_keep_their_storage_beside_a_defined_one(self):
        kx = layout([common('_c', 4), common('_d', 16, 3)], [defined('_c', 8)])
        self.assertEqual(where(kx, '_d'), ('__common', 0))
        self.assertEqual(kx.out[('__DATA', '__common')].size, 16)


class Objects(unittest.TestCase):
    def obj(self, cputype, cpusubtype, filetype=MH_OBJECT):
        f = tempfile.NamedTemporaryFile(suffix='.o', delete=False)
        self.addCleanup(os.unlink, f.name)
        f.write(HEADER.pack(MH_MAGIC_64, cputype, cpusubtype, filetype, 0, 0, 0, 0))
        f.close()
        return f.name

    def test_arm64e_objects_are_taken(self):
        for subtype in (2, 0x80000002, 0xc0000002):     # plain, a user ABI version, the kernel's (clang -mkernel)
            kextld.Obj(self.obj(CPU_TYPE_ARM64, subtype))

    def test_arm64_objects_are_refused(self):
        # the kext is written as arm64e whatever went in: an arm64 object's pointers would be in it unsigned
        for subtype in (0, 1):                          # CPU_SUBTYPE_ARM64_ALL, _V8
            with self.assertRaises(kextld.LinkError, msg=hex(subtype)):
                kextld.Obj(self.obj(CPU_TYPE_ARM64, subtype))

    def test_other_files_are_refused(self):
        with self.assertRaises(kextld.LinkError):
            kextld.Obj(self.obj(0x01000007, 3))                         # x86_64
        with self.assertRaises(kextld.LinkError):
            kextld.Obj(self.obj(CPU_TYPE_ARM64, 0xc0000002, filetype=2))    # an executable, not an object


if __name__ == '__main__':
    unittest.main()
