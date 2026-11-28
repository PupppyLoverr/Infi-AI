#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-only
"""Every relocation scripts/kclink/kextld.py resolves, checked in what it linked: the instruction's operand decoded,
or the pointer it hands to the collection's chained fixups (its target, key, diversity, address diversity).

The objects are built here, relocation by relocation (mkmacho.py), so the tests need python3 only. Where there is a
clang that makes arm64e objects, the same code is assembled too: the assembler must write what was built by hand, byte
for byte, and its object must link to the same result.

    python3 tests/kclink/test_kextld_relocations.py
"""
import os
import shutil
import struct
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import mkmacho                                                                  # noqa: E402
from mkmacho import (AUTHENTICATED_POINTER, B, BL, BRANCH26, GOT_LOAD_PAGE21, GOT_LOAD_PAGEOFF12,  # noqa: E402
                     HEADER_VA, KEXT_VA, KEY_DA, KEY_DB, KEY_IA, KEY_IB, LOADS, NOP, PAGE21, PAGEOFF12, POINTER_TO_GOT,
                     RET, TLVP_PAGE21, UNSIGNED, Object, add, adrp, adrp_page, branch_target, imm12, movz)
import kextld                                                                   # noqa: E402

LDR_X1_X1 = 0xf9400021          # ldr x1, [x1]
OPERAND = {name: name.split()[0] + ' ' + (name.split()[1] + '2' if ' ' in name else 'pldl2keep') for name in LOADS}
# the same object as assembly: code() below builds it by hand. (With .subsections_via_symbols after it, as a compiler
# ends its output: without, the assembler resolves a branch within the section itself and writes no relocation.)
SOURCE = '''        .section __TEXT,__text,regular,pure_instructions
        .globl _code
        .p2align 2
_code:
        bl _near
        b _far
        bl _outside
        bl _near+4
        adrp x1, _datum@PAGE
        add x1, x1, _datum@PAGEOFF
        adrp x1, _datum+0x12340@PAGE
        add x1, x1, _datum+0x12340@PAGEOFF
        adrp x1, _outside_data@GOTPAGE
        ldr x1, [x1, _outside_data@GOTPAGEOFF]
        adrp x1, _datum@GOTPAGE
        ldr x1, [x1, _datum@GOTPAGEOFF]
''' + ''.join(f'        {OPERAND[name]}, [x1, _datum@PAGEOFF]\n' for name in LOADS) + '''        .globl _near
_near:
        mov w0, #1
        ret
        .section __DATA,__data
        .globl _datum
        .p2align 4
_datum:
        .quad 0x1111111111111111
        .quad 0x2222222222222222
        .globl _table
_table:
        .quad _code
        .quad _code+20
        .quad _table-8
        .quad _outside
'''
SIGNED = '''        .quad _code@AUTH(ia,0x1234)
        .quad _code+8@AUTH(ib,0xffff,addr)
        .quad _table@AUTH(da,0)
        .quad _outside-4@AUTH(db,0xbeef,addr)
'''
LOADS_AT, NEAR_AT = 48, 48 + 4 * len(LOADS)                  # offsets in _code


def code(signed=True):
    """The object SOURCE (+ SIGNED) describes, built by hand."""
    o = Object()
    insns = [BL, B, BL, BL, adrp(1), add(1, 1), adrp(1), add(1, 1), adrp(1), LDR_X1_X1, adrp(1), LDR_X1_X1]
    insns += [insn for insn, scale in LOADS.values()] + [movz(0, 1), RET]
    t = o.text(*insns)
    d = o.section('__DATA', '__data', struct.pack('<2Q', 0x1111111111111111, 0x2222222222222222) +
                  bytes(64 if signed else 32), 4)
    s = {'_code': o.define('_code', t), '_near': o.define('_near', t, NEAR_AT), '_datum': o.define('_datum', d),
         '_table': o.define('_table', d, 16)}
    for name in ('_far', '_outside', '_outside_data'):
        s[name] = o.undefined(name)
    o.reloc(t, 0, BRANCH26, s['_near'], pcrel=1)
    o.reloc(t, 4, BRANCH26, s['_far'], pcrel=1)
    o.reloc(t, 8, BRANCH26, s['_outside'], pcrel=1)
    o.reloc(t, 12, BRANCH26, s['_near'], pcrel=1, addend=4)
    o.reloc(t, 16, PAGE21, s['_datum'], pcrel=1)
    o.reloc(t, 20, PAGEOFF12, s['_datum'])
    o.reloc(t, 24, PAGE21, s['_datum'], pcrel=1, addend=0x12340)
    o.reloc(t, 28, PAGEOFF12, s['_datum'], addend=0x12340)
    o.reloc(t, 32, GOT_LOAD_PAGE21, s['_outside_data'], pcrel=1)
    o.reloc(t, 36, GOT_LOAD_PAGEOFF12, s['_outside_data'])
    o.reloc(t, 40, GOT_LOAD_PAGE21, s['_datum'], pcrel=1)
    o.reloc(t, 44, GOT_LOAD_PAGEOFF12, s['_datum'])
    for i in range(len(LOADS)):
        o.reloc(t, LOADS_AT + 4 * i, PAGEOFF12, s['_datum'])
    o.pointer(d, 16, s['_code'])
    o.pointer(d, 24, s['_code'], 20)
    o.pointer(d, 32, s['_table'], -8)
    o.pointer(d, 40, s['_outside'])
    if signed:
        o.auth_pointer(d, 48, s['_code'], KEY_IA, 0x1234)
        o.auth_pointer(d, 56, s['_code'], KEY_IB, 0xffff, 1, addend=8)
        o.auth_pointer(d, 64, s['_table'], KEY_DA)
        o.auth_pointer(d, 72, s['_outside'], KEY_DB, 0xbeef, 1, addend=-4)
    return o


def far():
    """A second object: _far, the function _code branches to."""
    o = Object()
    o.define('_far', o.text(movz(0, 2), RET))
    return o


class Case(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, self.dir)

    def link(self, *objects, **how):
        """Links objects (an Object, or the path of a compiled one) into a kext."""
        paths = [o if isinstance(o, str) else o.write(self.dir, f'o{i}.o') for i, o in enumerate(objects)]
        return mkmacho.link(paths, **how)

    def refused(self, *objects, **how):
        """The message kextld refuses the link with."""
        with self.assertRaises(kextld.LinkError) as e:
            self.link(*objects, **how)
        return str(e.exception)

    def pointer(self, k, va):
        """(target, signing) of the pointer kextld hands back for the 8 bytes at va, which it leaves zero."""
        self.assertEqual(k.u64(va), 0, f'the bytes of the pointer at {va:#x}')
        f = k.fixups[va]
        return f.target, None if f.key is None else (f.key, f.div, f.addr_div)


OUTSIDE = {'_outside': KEXT_VA['__TEXT_EXEC'] - 0x400000, '_outside_data': HEADER_VA + 0x3800000}


class Built(Case):
    """The object of SOURCE as code() builds it, linked with far(): every relocation of it."""
    signed = True

    def object(self):
        return code(self.signed)

    def setUp(self):
        super().setUp()
        self.k = self.link(self.object(), far(), outside=OUTSIDE)
        self.code, self.datum = self.k.symbols['_code'], self.k.symbols['_datum']
        self.table = self.k.symbols['_table']

    def insn(self, off):
        return self.k.u32(self.code + off)

    def test_the_symbols_are_where_their_content_is(self):
        k = self.k
        self.assertEqual(k.u32(k.symbols['_near']), movz(0, 1))
        self.assertEqual(k.u32(k.symbols['_far']), movz(0, 2))
        self.assertEqual(k.u64(self.datum), 0x1111111111111111)
        self.assertEqual(self.table, self.datum + 16)
        self.assertEqual(self.code, k.section('__TEXT_EXEC', '__text')[0])
        self.assertEqual(self.datum % 16, 0)

    def test_branch26(self):
        k = self.k
        for off, op, target in ((0, BL, k.symbols['_near']), (4, B, k.symbols['_far']), (8, BL, OUTSIDE['_outside']),
                                (12, BL, k.symbols['_near'] + 4)):
            self.assertEqual(self.insn(off) & 0xfc000000, op, f'the instruction at {off}')
            self.assertEqual(branch_target(self.insn(off), self.code + off), target, f'the branch at {off}')
        self.assertLess(OUTSIDE['_outside'], self.code)       # (one of them goes backward)

    def test_page21_and_pageoff12_of_an_add(self):
        for off, target in ((16, self.datum), (24, self.datum + 0x12340)):
            self.assertEqual(self.insn(off) & 0x9f00001f, adrp(1))
            self.assertEqual(adrp_page(self.insn(off), self.code + off), target & ~0xfff)
            self.assertEqual(self.insn(off + 4) & ~(0xfff << 10), add(1, 1))
            self.assertEqual(imm12(self.insn(off + 4)), target & 0xfff)
        self.assertNotEqual(self.datum >> 12, (self.datum + 0x12340) >> 12)       # (the addend changes the page)
        self.assertNotEqual(self.datum & 0xfff, 0)

    def test_pageoff12_of_every_load_and_store(self):
        """The 12 bits count in units of what the instruction moves: 1, 2, 4, 8 or 16 bytes."""
        for i, (name, (insn, scale)) in enumerate(LOADS.items()):
            linked = self.insn(LOADS_AT + 4 * i)
            self.assertEqual(linked & ~(0xfff << 10), insn, name)
            self.assertEqual(imm12(linked) << scale, self.datum & 0xfff, name)

    def test_got_load(self):
        k = self.k
        got, size = k.section('__DATA_CONST', '__got')
        self.assertEqual((size, got % 8), (16, 0))
        slots = {}
        for off, name in ((32, '_outside_data'), (40, '_datum')):
            self.assertEqual(self.insn(off + 4) & ~(0xfff << 10), LDR_X1_X1)
            slot = adrp_page(self.insn(off), self.code + off) + (imm12(self.insn(off + 4)) << 3)
            self.assertTrue(got <= slot < got + size and slot % 8 == 0, name)
            slots[name] = slot
        self.assertNotEqual(slots['_outside_data'], slots['_datum'])
        self.assertEqual(self.pointer(k, slots['_outside_data']), (OUTSIDE['_outside_data'], None))
        self.assertEqual(self.pointer(k, slots['_datum']), (self.datum, None))

    def test_unsigned(self):
        k = self.k
        self.assertEqual(self.pointer(k, self.table), (self.code, None))
        self.assertEqual(self.pointer(k, self.table + 8), (self.code + 20, None))
        self.assertEqual(self.pointer(k, self.table + 16), (self.table - 8, None))
        self.assertEqual(self.pointer(k, self.table + 24), (OUTSIDE['_outside'], None))

    def test_authenticated_pointer(self):
        if not self.signed:
            self.skipTest('this compiler does not assemble @AUTH')
        k = self.k
        self.assertEqual(self.pointer(k, self.table + 32), (self.code, (KEY_IA, 0x1234, 0)))
        self.assertEqual(self.pointer(k, self.table + 40), (self.code + 8, (KEY_IB, 0xffff, 1)))
        self.assertEqual(self.pointer(k, self.table + 48), (self.table, (KEY_DA, 0, 0)))
        self.assertEqual(self.pointer(k, self.table + 56), (OUTSIDE['_outside'] - 4, (KEY_DB, 0xbeef, 1)))

    def test_no_other_pointers(self):
        k = self.k
        data = k.section('__DATA', '__data')[0]
        mine = {self.table + 8 * i for i in range(8 if self.signed else 4)}
        mine |= {k.section('__DATA_CONST', '__got')[0] + 8 * i for i in range(2)}
        mine |= {data + kextld.KMOD_START, data + kextld.KMOD_STOP}
        self.assertEqual(set(k.fixups), mine)


class Assembled(Built):
    """The same, of what clang assembles from SOURCE: its object is what code() builds, and links the same."""

    def object(self):
        cc, why = mkmacho.compiler('auth')
        type(self).signed = cc is not None
        return mkmacho.compile(self, SOURCE + (SIGNED if self.signed else '') + '.subsections_via_symbols\n',
                               'auth' if self.signed else 'asm')

    def test_the_assembler_writes_what_was_built_by_hand(self):
        with open(self.object(), 'rb') as f:
            assembled = mkmacho.describe(f.read())
        built = mkmacho.describe(code(self.signed).build())
        self.assertEqual(sorted(assembled, key=str), sorted(built, key=str))
        for what in built:
            self.assertEqual(assembled[what], built[what], what)


class Branches(Case):
    def caller(self, addend=None):
        o = Object()
        t = o.text(BL, RET)
        o.define('_caller', t)
        o.reloc(t, 0, BRANCH26, o.undefined('_callee'), pcrel=1, addend=addend)
        return o

    def test_the_addend_is_a_signed_24_bit_number(self):
        callee = KEXT_VA['__TEXT_EXEC'] + 0x10000
        for addend in (-4, -0x800000, 0x7ffffc):
            k = self.link(self.caller(addend), outside={'_callee': callee})
            at = k.symbols['_caller']
            self.assertEqual(branch_target(k.u32(at), at), callee + addend, hex(addend))

    def test_an_addend_applies_to_the_relocation_after_it_only(self):
        o = Object()
        t = o.text(BL, BL, RET, RET)
        o.define('_caller', t)
        callee = o.define('_callee', t, 8)
        o.reloc(t, 0, BRANCH26, callee, pcrel=1, addend=4)
        o.reloc(t, 4, BRANCH26, callee, pcrel=1)
        k = self.link(o)
        at = k.symbols['_caller']
        self.assertEqual(branch_target(k.u32(at), at), k.symbols['_callee'] + 4)
        self.assertEqual(branch_target(k.u32(at + 4), at + 4), k.symbols['_callee'])

    def test_as_far_as_a_branch_reaches_and_no_farther(self):
        at = KEXT_VA['__TEXT_EXEC']                          # where _caller's bl is
        for delta in (0x7fffffc, -0x8000000):
            k = self.link(self.caller(), outside={'_callee': at + delta})
            self.assertEqual(branch_target(k.u32(at), at), at + delta)
        for delta in (0x8000000, -0x8000004):
            self.assertIn('_callee', self.refused(self.caller(), outside={'_callee': at + delta}))

    def test_a_symbol_nothing_defines(self):
        self.assertIn('unresolved symbols: _callee', self.refused(self.caller()))

    def test_a_static_function_is_its_own_objects(self):
        """Two objects with a local symbol of one name each call their own; a third's external one is another."""
        objects = []
        for i in range(3):
            o = Object()
            t = o.text(BL, RET, movz(0, 10 + i), RET)
            o.define(f'_caller{i}', t)
            o.reloc(t, 0, BRANCH26, o.define('_helper', t, 8, ext=i == 2), pcrel=1)
            objects.append(o)
        k = self.link(*objects)
        for i in range(3):
            at = k.symbols[f'_caller{i}']
            self.assertEqual(k.u32(branch_target(k.u32(at), at)), movz(0, 10 + i), f'object {i}')

    def test_the_strong_definition_also_for_the_object_with_the_weak_one(self):
        weak, strong = Object(), Object()
        t = weak.text(BL, RET, movz(0, 1), RET)
        weak.define('_caller', t)
        weak.reloc(t, 0, BRANCH26, weak.define('_f', t, 8, weak=True), pcrel=1)
        strong.define('_f', strong.text(movz(0, 2), RET))
        for objects in ((weak, strong), (strong, weak)):
            k = self.link(*objects)
            at = k.symbols['_caller']
            self.assertEqual(k.u32(branch_target(k.u32(at), at)), movz(0, 2))


class Pages(Case):
    def reader(self, insn, sym_off=0, addend=None, align_=4):
        """adrp x1, _d@PAGE; <insn> [x1, _d@PAGEOFF], with _d at sym_off of a section of this alignment."""
        o = Object()
        t = o.text(adrp(1), insn, RET)
        d = o.section('__DATA', '__data', bytes(sym_off + 16), align_)
        o.define('_reader', t)
        sym = o.define('_d', d, sym_off)
        o.reloc(t, 0, PAGE21, sym, pcrel=1, addend=addend)
        o.reloc(t, 4, PAGEOFF12, sym, addend=addend)
        return o

    def operand(self, k, scale=0):
        at = k.symbols['_reader']
        return adrp_page(k.u32(at), at) + (imm12(k.u32(at + 4)) << scale)

    def test_a_negative_addend_into_the_page_before(self):
        k = self.link(self.reader(add(1, 1), addend=-8, align_=12))
        self.assertEqual(k.symbols['_d'] & 0xfff, 0)
        self.assertEqual(self.operand(k), k.symbols['_d'] - 8)

    def test_pages_below_and_above_as_far_as_adrp_reaches(self):
        text = KEXT_VA['__TEXT_EXEC']
        for data in (text - 0x40000000, text + 0xfffe0000, text - 0x100000000):
            k = self.link(self.reader(add(1, 1)), addrs={'__DATA': data})
            self.assertEqual(k.symbols['_d'] & ~0x3fff, data)
            self.assertEqual(self.operand(k), k.symbols['_d'], hex(data))
        for data in (text + 0x100000000, text - 0x100004000):
            self.assertIn('out of range', self.refused(self.reader(add(1, 1)), addrs={'__DATA': data}))

    def test_an_offset_the_instruction_cannot_count(self):
        """A load of 2, 4, 8 or 16 bytes at an address that is not a multiple of that is refused, not rounded."""
        for name, (insn, scale) in LOADS.items():
            for off in (1 << scale >> 1, 1 << scale):
                if off & ((1 << scale) - 1):
                    self.assertIn('not aligned', self.refused(self.reader(insn, off)), f'{name} at +{off}')
                else:
                    k = self.link(self.reader(insn, off))
                    self.assertEqual(self.operand(k, scale), k.symbols['_d'], f'{name} at +{off}')

    def test_a_page_offset_into_another_instruction(self):
        for insn in (NOP, 0xb1000021, 0xd1000021):          # adds and sub take no @PAGEOFF from an assembler either
            self.assertIn('page offset into instruction', self.refused(self.reader(insn)))


class Got(Case):
    def setUp(self):
        super().setUp()
        self.objects = []

    def loader(self, name, define=False):
        """adrp x1, name@GOTPAGE; ldr x1, [x1, name@GOTPAGEOFF]"""
        o = Object()
        t = o.text(adrp(1), LDR_X1_X1, RET)
        o.define(f'_loader_{len(self.objects)}', t)
        sym = o.define(name, o.section('__DATA', '__data', bytes(8), 3)) if define else o.undefined(name)
        o.reloc(t, 0, GOT_LOAD_PAGE21, sym, pcrel=1)
        o.reloc(t, 4, GOT_LOAD_PAGEOFF12, sym)
        self.objects.append(o)

    def slot(self, k, i):
        at = k.symbols[f'_loader_{i}']
        return adrp_page(k.u32(at), at) + (imm12(k.u32(at + 4)) << 3)

    def test_one_slot_for_a_symbol_whoever_loads_it(self):
        self.loader('_a')
        self.loader('_b', define=True)
        self.loader('_a')
        self.loader('_b')
        k = self.link(*self.objects, outside={'_a': HEADER_VA + 0x3900000})
        got, size = k.section('__DATA_CONST', '__got')
        self.assertEqual(size, 16)
        self.assertEqual(self.slot(k, 0), self.slot(k, 2))
        self.assertEqual(self.slot(k, 1), self.slot(k, 3))
        self.assertEqual({self.slot(k, 0), self.slot(k, 1)}, {got, got + 8})
        self.assertEqual(self.pointer(k, self.slot(k, 0)), (HEADER_VA + 0x3900000, None))
        self.assertEqual(self.pointer(k, self.slot(k, 1)), (k.symbols['_b'], None))

    def test_a_symbol_nothing_defines(self):
        self.loader('_nowhere')
        self.assertIn('unresolved symbols: _nowhere', self.refused(*self.objects))

    def test_no_got_without_a_load_through_it(self):
        k = self.link(far())
        self.assertNotIn(('__DATA_CONST', '__got'), k.kx.out)
        self.assertNotIn('__DATA_CONST', k.segments)


class Pointers(Case):
    def holder(self):
        o = Object()
        t = o.text(movz(0, 3), RET)
        d = o.section('__DATA', '__data', bytes(64), 3)
        return o, t, d, o.define('_f', t), o.define('_p', d)

    def test_every_key_with_and_without_address_diversity(self):
        o, t, d, f, p = self.holder()
        cases = [(key, div, addr) for key in (KEY_IA, KEY_IB, KEY_DA, KEY_DB) for div, addr in ((0, 0), (0xa5a5, 1))]
        for i, (key, div, addr) in enumerate(cases):
            o.auth_pointer(d, 8 * i, f, key, div, addr)
        k = self.link(o)
        for i, case in enumerate(cases):
            self.assertEqual(self.pointer(k, k.symbols['_p'] + 8 * i), (k.symbols['_f'], case))

    def test_a_signed_pointers_addend_is_a_signed_32_bit_number(self):
        o, t, d, f, p = self.holder()
        addends = (0x7fffffff, -0x80000000, -1, 0x10)
        for i, addend in enumerate(addends):
            o.auth_pointer(d, 8 * i, f, KEY_DA, 0xffff, 1, addend=addend)
        k = self.link(o)
        for i, addend in enumerate(addends):
            self.assertEqual(self.pointer(k, k.symbols['_p'] + 8 * i),
                             ((k.symbols['_f'] + addend) & mkmacho.MASK64, (KEY_DA, 0xffff, 1)), hex(addend))

    def test_a_pointer_into_a_section_by_its_number(self):
        """r_extern 0: the 8 bytes hold the target's address in the object, r_symbolnum its section."""
        o, t, d, f, p = self.holder()
        c = o.section('__DATA', '__const', struct.pack('<2Q', 0x33, 0x44), 3)
        struct.pack_into('<Q', o.data(d), 0, o.addr(c, 8))
        o.reloc(d, 0, UNSIGNED, c, length=3, extern=0)
        struct.pack_into('<Q', o.data(d), 8, o.addr(t, 4) | 2 << 49 | 0x77 << 32 | 1 << 63)
        o.reloc(d, 8, AUTHENTICATED_POINTER, t, length=3, extern=0)
        k = self.link(o)
        const = k.section('__DATA_CONST', '__const')[0]
        self.assertEqual(k.u64(const + 8), 0x44)
        self.assertEqual(self.pointer(k, k.symbols['_p']), (const + 8, None))
        self.assertEqual(self.pointer(k, k.symbols['_p'] + 8), (k.symbols['_f'] + 4, (KEY_DA, 0x77, 0)))

    def test_a_32_bit_pointer_is_refused(self):
        o, t, d, f, p = self.holder()
        o.reloc(d, 0, UNSIGNED, f, length=2)
        self.assertIn('32-bit pointer at __data+0x0', self.refused(o))

    def test_a_pointer_not_at_a_multiple_of_4(self):
        """A collection's chained fixups are at multiples of 4 (a pointer in a packed structure may not be): refused
        with the object and the place, not left for the collection's writer to find."""
        for off in (1, 2, 7):
            o, t, d, f, p = self.holder()
            c = o.section('__DATA', '__const', bytes(16), 0)
            o.pointer(c, off, f)
            self.assertIn(f'o0.o: pointer at __const+{off:#x} not at a multiple of 4', self.refused(o))
        o, t, d, f, p = self.holder()
        c = o.section('__DATA', '__const', bytes(16), 0)
        o.pointer(c, 4, f)                                      # 4 is fine, 8 is not needed
        k = self.link(o)
        self.assertEqual(self.pointer(k, k.section('__DATA_CONST', '__const')[0] + 4), (k.symbols['_f'], None))

    def test_a_symbol_nothing_defines(self):
        o, t, d, f, p = self.holder()
        o.pointer(d, 0, o.undefined('_nowhere'))
        o.auth_pointer(d, 8, o.undefined('_nowhere_else'), KEY_IA)
        self.assertIn('unresolved symbols: _nowhere _nowhere_else', self.refused(o))

    def test_a_vtable_slot_for_a_method_no_kext_exports(self):
        """The spec's `vtable` class: a slot naming what nothing exports takes the superclass's, by its index."""
        o, t, d, f, p = self.holder()
        vt = o.section('__DATA', '__const', bytes(48), 3)
        o.define('__ZTV3Sub', vt)
        o.auth_pointer(vt, 16, f, KEY_IA, 0x1111, 1)
        o.auth_pointer(vt, 24, o.undefined('__ZN4Base7privateEv'), KEY_IA, 0x2222, 1)
        o.auth_pointer(vt, 32, o.undefined('__ZN4Base5otherEv'), KEY_IA, 0x3333, 1, addend=4)
        spec = {'vtable': {'class': 'Sub', 'base': 'Base'}}
        base = {1: HEADER_VA + 0x2a00000, 2: HEADER_VA + 0x2a00100}
        k = self.link(o, spec=spec, inherited=base.get)
        at = k.symbols['__ZTV3Sub']
        self.assertEqual(self.pointer(k, at + 16), (k.symbols['_f'], (KEY_IA, 0x1111, 1)))
        self.assertEqual(self.pointer(k, at + 24), (base[1], (KEY_IA, 0x2222, 1)))
        self.assertEqual(self.pointer(k, at + 32), (base[2] + 4, (KEY_IA, 0x3333, 1)))
        self.assertIn('__ZN4Base5otherEv', self.refused(o, spec=spec, inherited={1: base[1]}.get))
        self.assertIn('__ZN4Base7privateEv', self.refused(o))      # and not without the spec saying so


class NotLinked(Case):
    """What kextld does not resolve it must refuse: left as the assembler wrote it, a kext would load and run wrong."""

    def test_a_difference_of_two_symbols(self):
        o = Object()
        t = o.text(RET, RET)
        d = o.section('__DATA', '__data', bytes(8), 3)
        o.reloc(d, 0, UNSIGNED, o.define('_b', t, 4), length=3, minus=o.define('_a', t))
        self.assertIn('relocation type 1 at __data+0x0', self.refused(o))

    def test_a_pointer_to_a_got_slot(self):
        for pcrel, length in ((1, 2), (0, 3)):
            o = Object()
            d = o.section('__DATA', '__data', bytes(8), 3)
            o.reloc(d, 0, POINTER_TO_GOT, o.undefined('_outside'), pcrel=pcrel, length=length)
            self.assertIn('relocation type 7 at __data+0x0', self.refused(o, outside=OUTSIDE))

    def test_a_thread_local(self):
        o = Object()
        t = o.text(adrp(0), RET)
        o.reloc(t, 0, TLVP_PAGE21, o.undefined('_outside'), pcrel=1)
        self.assertIn('relocation type 8 at __text+0x0', self.refused(o, outside=OUTSIDE))

    def test_a_got_load_of_a_section(self):
        o = Object()
        t = o.text(adrp(1), RET)
        o.reloc(t, 0, GOT_LOAD_PAGE21, t, pcrel=1, extern=0)
        self.assertIn('GOT load of a section', self.refused(o))

    def test_a_symbol_in_a_section_the_kext_does_not_get(self):
        o = Object()
        d = o.section('__DATA', '__data', bytes(8), 3)
        o.pointer(d, 0, o.define('_unwind', o.section('__LD', '__compact_unwind', bytes(32), 3), ext=False))
        self.assertIn('dropped section __LD,__compact_unwind', self.refused(o))


if __name__ == '__main__':
    unittest.main()
