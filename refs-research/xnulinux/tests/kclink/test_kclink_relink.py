#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-only
"""More of scripts/kclink/kclink.py, on the stand-in collection of mkcollection.py: several kexts that use each other,
where kexts may not go (--reserve, the kernel's __PRELINK_DATA), a collection linked again with its kexts kept (--keep:
for a kernel somewhere else whose symbols moved, --rebind, patch_personalities), that the same input gives the same
bytes, and what it refuses.

    python3 tests/kclink/test_kclink_relink.py
"""
import json
import os
import struct
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import mkmacho                                                                  # noqa: E402
from mkmacho import KEY_IA, LC_SEGMENT_64, Object, RET, branch_target          # noqa: E402
from mkcollection import (BASE, KPI, PAGE, SPEC_ONE, SPEC_THREE, SPEC_TWO, Workdir, kext_one, kext_three,  # noqa: E402
                          kext_two)

SLIDE = 0x8000000
ONE, TWO, THREE = 'org.example.kext.one', 'org.example.kext.two', 'org.example.kext.three'


def spans(kc, name):
    """[(start, end)] of an entry's segments in the file, its share of __LINKEDIT left out."""
    return [(s['fileoff'], s['fileoff'] + s['filesize']) for s in kc.entry(name)[1].values()
            if s['name'] != '__LINKEDIT']


class Natives(unittest.TestCase):
    def setUp(self):
        self.w = Workdir(self.addCleanup)
        self.w.template()
        self.ksyms, self.kpointers = self.w.kernel()
        self.one = self.w.native(SPEC_ONE, kext_one())

    def linked(self, *args, **how):
        status, out, err = self.w.link(*args, **how)
        self.assertEqual((status, err), (0, ''))
        return self.w.collection(how.get('output', 'out.kc'))

    def refused(self, *args, **how):
        """What kclink says when it gives up: one line, and no collection."""
        status, out, err = self.w.link(*args, output='refused.kc', **how)
        self.assertNotEqual(status, 0)
        self.assertFalse(os.path.exists(self.w.path('refused.kc')))
        self.assertEqual(err.count('\n'), 1, err)
        self.assertTrue(err.startswith('kclink: '), err)
        return err

    def test_a_kext_that_calls_another(self):
        kc = self.linked('--native', self.one, self.w.native(SPEC_TWO, kext_two()))
        self.assertEqual(list(kc.entries), ['com.apple.kernel', ONE, TWO])
        at = kc.natives['_two_start']
        self.assertEqual(branch_target(kc.u32(at), at), kc.natives['_helper'])
        taken = sorted(span + (name,) for name in kc.entries for span in spans(kc, name))
        for (a1, b1, n1), (a2, b2, n2) in zip(taken, taken[1:]):
            self.assertLessEqual(b1, a2, f'{n1} and {n2} overlap')
        self.assertEqual(len(kc.pointers), 14)
        self.assertEqual(list(kc.kexts)[7:], [ONE, TWO])
        self.assertEqual(kc.kexts[TWO]['OSBundleLibraries'], {ONE: '1.0'})
        # the other order: the first one's call binds to a kext linked after it
        kc = self.linked('--native', self.w.native(SPEC_TWO, kext_two()), self.one)
        self.assertEqual(list(kc.entries), ['com.apple.kernel', TWO, ONE])
        at = kc.natives['_two_start']
        self.assertEqual(branch_target(kc.u32(at), at), kc.natives['_helper'])

    def test_reserved_room_stays_free(self):
        plain = self.linked('--native', self.one)
        data = plain.entry(ONE)[1]['__DATA']
        self.assertEqual(data['fileoff'], 0x4c000)              # behind the kernel's page of data
        kc = self.linked('--native', self.one, '--reserve', '__DATA:0x4c000-0x50000', '__TEXT_EXEC:0x34000-0x34100')
        self.assertEqual(kc.entry(ONE)[1]['__DATA']['fileoff'], 0x50000)
        self.assertEqual(kc.entry(ONE)[1]['__TEXT_EXEC']['fileoff'], 0x34100)
        self.assertEqual(kc.entry(ONE)[1]['__TEXT']['fileoff'], plain.entry(ONE)[1]['__TEXT']['fileoff'])

    def test_kexts_data_from_the_kernels_prelink_data_on(self):
        self.w.kernel('kernel.pd', prelink_data=0x54000)
        kc = self.linked('--native', self.one, kernel='kernel.pd')
        self.assertEqual(kc.entry(ONE)[1]['__DATA']['fileoff'], 0x54000)

    def test_a_kernel_laid_out_for_somewhere_else(self):
        """The collection is where the kernel is: its header as far below the kernel's __TEXT as in the template."""
        ksyms, kpointers = self.w.kernel('kernel.high', base=BASE + SLIDE)
        low, high = self.linked('--native', self.one), self.linked('--native', self.one, kernel='kernel.high',
                                                                   output='high.kc')
        self.assertEqual(high.base, BASE + SLIDE)
        self.assertEqual({n: (va - SLIDE, off) for n, (va, off) in high.entries.items()}, low.entries)
        self.assertEqual({off: (p.target, p.signing()) for off, p in high.pointers.items()},
                         {off: (p.target, p.signing()) for off, p in low.pointers.items()})
        self.assertEqual({n: a - SLIDE for n, a in high.natives.items()}, low.natives)
        self.assertEqual(high.kexts[ONE]['_PrelinkKmodInfo'], low.kexts[ONE]['_PrelinkKmodInfo'] + SLIDE)

    def test_what_is_refused(self):
        w = self.w
        self.assertIn('kernel: not a kernel collection', self.refused(base='kernel'))
        self.assertIn('org.example.nowhere: not in template.kc', self.refused('--keep', 'org.example.nowhere'))
        self.assertIn(f'{ONE} is in the collection already', self.refused('--native', self.one, self.one))
        o = Object()
        o.define('_three_start', o.text(RET))
        self.assertIn(f'{THREE}: unresolved symbols: _three_stop',
                      self.refused('--native', w.native(SPEC_THREE, [o])))
        self.assertIn(f'{THREE}: unresolved symbols: _nowhere',
                      self.refused('--native', w.native(SPEC_THREE, kext_three('_nowhere'))))
        big = kext_three('_kernel_data')
        big[0].section('__DATA', '__bss', zerofill=0x10000)
        self.assertIn(f'{THREE}: no room for its __DATA (0x100d0 bytes) in __DATA',
                      self.refused('--native', w.native(SPEC_THREE, big)))
        # what kextld finds before it links, too: in one line, with the kext
        odd = kext_three('_kernel_data')
        odd[0].section('__DATA', '__objc_const', bytes(8), 3)
        self.assertIn(f'{THREE}: three0.o: section __DATA,__objc_const has no place in a kext',
                      self.refused('--native', w.native(SPEC_THREE, odd)))
        self.assertIn(f'{THREE}: _three_start defined twice (in three1.o and at __TEXT_EXEC,__text)',
                      self.refused('--native', w.native(SPEC_THREE, kext_three('_kernel_data') * 2)))
        spec = w.native(SPEC_THREE, kext_three('_kernel_data'))
        other = bytearray(w.read('three0.o'))
        other[4:8] = struct.pack('<I', 0x01000007)              # CPU_TYPE_X86_64
        w.write('three0.o', bytes(other))
        self.assertIn(f'{THREE}: three0.o: not an arm64', self.refused('--native', spec))
        packed = kext_three('_kernel_data')
        packed[0].pointer(packed[0].section('__DATA', '__const', bytes(16), 0), 1, 0)
        self.assertIn('three0.o: pointer at __const+0x1 not at a multiple of 4',
                      self.refused('--native', w.native(SPEC_THREE, packed)))
        w.kernel('kernel.odd', more=[('__ODD', 0x38000)])
        self.assertIn('kernel segment __ODD has no place in the collection', self.refused(kernel='kernel.odd'))
        w.kernel('kernel.over', more=[('__KLD', 0x40000)])       # code where the prelink info is
        self.assertIn('overlap', self.refused(kernel='kernel.over'))


class Kept(unittest.TestCase):
    """A collection with three kexts, linked again for a kernel that is SLIDE higher and has _kernel_data elsewhere in
    its data: two kexts kept (one named, one because the named one's OSBundleLibraries has it), the third not."""

    @classmethod
    def setUpClass(cls):
        cls.w = w = Workdir(cls.addClassCleanup)
        w.template()
        cls.ksyms, _ = w.kernel()
        status, out, err = w.link('--native', w.native(SPEC_ONE, kext_one()), w.native(SPEC_TWO, kext_two()),
                                  w.native(SPEC_THREE, kext_three('_kernel_data')), output='first.kc')
        assert status == 0, err
        cls.ksyms2, cls.kpointers2 = w.kernel('kernel2', base=BASE + SLIDE, data_at=0x40)
        status, cls.log, err = w.link('--keep', TWO, base='first.kc', kernel='kernel2', output='second.kc')
        assert status == 0, err
        cls.first, cls.second = w.collection('first.kc'), w.collection('second.kc')

    def test_what_is_kept(self):
        self.assertIn(f"keeping 2 kexts: ['{ONE}', '{TWO}']", self.log)
        self.assertIn('kept kexts: 10 chained pointers (3 into the kernel, by symbol)', self.log)
        self.assertEqual(list(self.second.entries), ['com.apple.kernel', ONE, TWO])
        self.assertEqual(list(self.second.kexts), list(KPI.values()) + [ONE, TWO])      # the pseudo-extensions stay
        self.assertEqual(self.second.natives, {})

    def test_the_kexts_moved_with_the_kernel(self):
        first, second = self.first, self.second
        self.assertEqual(second.base, BASE + SLIDE)
        for name in (ONE, TWO):
            self.assertEqual(second.entries[name], (first.entries[name][0] + SLIDE, first.entries[name][1]))
            (off, was), (_, now) = first.entry(name), second.entry(name)
            self.assertEqual(list(now), list(was))
            for seg, s in was.items():
                if seg == '__LINKEDIT':
                    continue
                moved = dict(s, vmaddr=s['vmaddr'] + SLIDE, sections=[dict(x, addr=x['addr'] + SLIDE)
                                                                      for x in s['sections']])
                self.assertEqual(now[seg], moved, f'{name} {seg}')
            le = now['__LINKEDIT']
            self.assertEqual((le['vmaddr'] - second.base, le['fileoff'] + le['filesize']),
                             (le['fileoff'], len(second.buf)), name)
            # the rest of its header is as it was
            cmds = zip(mkmacho.commands(first.buf, off), mkmacho.commands(second.buf, off))
            self.assertTrue(all(a == b for a, b in cmds if a[0] != LC_SEGMENT_64), name)

    def test_their_bytes_are_as_they_were(self):
        """But for the pointers, the load commands and the address in the kmod_info."""
        first, second = self.first, self.second
        for name in (ONE, TWO):
            kmod = first.kexts[name]['_PrelinkKmodInfo'] + (1 << 64) - BASE
            for a, b in spans(first, name):
                was, now = bytearray(first.buf[a:b]), bytearray(second.buf[a:b])
                for buf, kc in ((was, first), (now, second)):
                    for off in kc.pointers:
                        if a <= off < b:
                            buf[off - a:off - a + 8] = bytes(8)
                    if a == kc.entries[name][1]:
                        buf[0:0x1000] = bytes(0x1000)
                    if a <= kmod < b:
                        buf[kmod - a + 156:kmod - a + 164] = bytes(8)
                self.assertEqual(now, was, f'{name} at {a:#x}')
            self.assertEqual(struct.unpack_from('<Q', second.buf, kmod + 156)[0],
                             struct.unpack_from('<Q', first.buf, kmod + 156)[0] + SLIDE)

    def test_their_pointers(self):
        """Into a kept kext: moved with it. Into the kernel: to the symbol of that name in the new one."""
        first, second = self.first, self.second
        old_kernel = {a: n for n, a in self.ksyms.items()}
        kept = [span for name in (ONE, TWO) for span in spans(first, name)]
        expected = {va - second.base: (target, sign) for va, target, sign in self.kpointers2}
        for off, p in first.pointers.items():
            if not any(a <= off < b for a, b in kept):
                continue                                        # the old kernel's, the kext's that is not kept
            target = BASE + p.target
            expected[off] = (self.ksyms2[old_kernel[target]] if target in old_kernel else target + SLIDE, p.signing())
        self.assertEqual({off: (second.base + p.target, p.signing()) for off, p in second.pointers.items()}, expected)
        self.assertEqual(len(expected), 14)
        # by symbol, not by the distance the kernel moved: _kernel_data is somewhere else in the new kernel's data
        self.assertEqual(second.pointer(first.natives['_pointers'] + SLIDE), (self.ksyms2['_kernel_data'], None))
        self.assertNotEqual(self.ksyms2['_kernel_data'], self.ksyms['_kernel_data'] + SLIDE)
        # one that is not exported, too
        self.assertEqual(second.pointer(first.natives['_table'] + SLIDE + 8), (self.ksyms2['_kernel_private'], None))

    def test_their_entries_in_the_prelink_info(self):
        for name in (ONE, TWO):
            was, now = self.first.kexts[name], self.second.kexts[name]
            addresses = ('_PrelinkExecutableLoadAddr', '_PrelinkExecutableSourceAddr', '_PrelinkKmodInfo')
            moved = dict(was, **{k: was[k] + SLIDE for k in addresses})
            self.assertEqual(now, moved, name)
        for ident in KPI.values():
            self.assertEqual(self.second.kexts[ident], self.first.kexts[ident])

    def relink(self, *args):
        return self.w.link('--keep', TWO, *args, base='first.kc', kernel='kernel2', output='again.kc')

    def test_rebind(self):
        """--rebind ADDRESS=SYMBOL: the pointer at that address of the old collection, to another symbol, signed as
        it was."""
        plain, signed = self.first.natives['_table'] + 8, self.first.natives['_pointers'] + 8
        self.assertEqual(self.first.pointer(signed)[1], (KEY_IA, 0x1111, 1))
        status, out, err = self.relink('--rebind', f'{plain:#x}=_kernel_func', f'{signed:#x}=_kernel_start')
        self.assertEqual((status, err), (0, ''))
        again = self.w.collection('again.kc')
        self.assertEqual(again.pointer(plain + SLIDE), (self.ksyms2['_kernel_func'], None))
        self.assertEqual(again.pointer(signed + SLIDE), (self.ksyms2['_kernel_start'], (KEY_IA, 0x1111, 1)))
        rest = {off for off in again.pointers if off not in (plain - BASE, signed - BASE)}
        self.assertEqual({off: (again.pointers[off].target, again.pointers[off].signing()) for off in rest},
                         {off: (p.target, p.signing()) for off, p in self.second.pointers.items() if off in rest})
        self.assertEqual(len(rest), 12)
        status, out, err = self.relink('--rebind', f'{plain:#x}=_nowhere')
        self.assertEqual((status, err), (1, f'kclink: --rebind {plain:#x}=_nowhere: no _nowhere in the kernel\n'))
        status, out, err = self.relink('--rebind', f'{plain + 4:#x}=_kernel_func')
        self.assertEqual((status, err), (1, f'kclink: --rebind: no pointer of a kept kext at {plain + 4:#x}\n'))

    def test_a_native_kext_beside_the_kept_ones(self):
        """In room no kept kext has, and with a key for a kept kext's personality (patch_personalities)."""
        spec = dict(SPEC_THREE, patch_personalities={ONE: {'One': {'IOProbeScore': 5000}}})
        status, out, err = self.relink('--native', self.w.native(spec, kext_three('_kernel_data')))
        self.assertEqual((status, err), (0, ''))
        again = self.w.collection('again.kc')
        self.assertEqual(list(again.entries), ['com.apple.kernel', ONE, TWO, THREE])
        taken = sorted(span + (name,) for name in again.entries for span in spans(again, name))
        for (a1, b1, n1), (a2, b2, n2) in zip(taken, taken[1:]):
            self.assertLessEqual(b1, a2, f'{n1} and {n2} overlap')
        self.assertEqual(again.pointer(again.natives['_three_pointer']), (self.ksyms2['_kernel_data'], None))
        self.assertEqual(again.kexts[ONE]['IOKitPersonalities'],
                         {'One': dict(SPEC_ONE['personalities']['One'], IOProbeScore=5000)})
        self.assertEqual(again.kexts[TWO], self.second.kexts[TWO])
        for patch, why in (({ONE: {'Other': {'x': 1}}}, f'patch_personalities: {ONE} has no personality Other'),
                           ({THREE + '.not': {'One': {}}}, f'patch_personalities: {THREE}.not not kept')):
            spec = dict(SPEC_THREE, patch_personalities=patch)
            status, out, err = self.relink('--native', self.w.native(spec, kext_three('_kernel_data')))
            self.assertEqual((status, err), (1, f'kclink: {why}\n'))

    def test_a_kept_kext_that_points_into_one_that_is_not(self):
        w = self.w
        status, out, err = w.link('--native', w.native(SPEC_ONE, kext_one()), w.native(SPEC_THREE, kext_three()),
                                  output='pointing.kc')
        self.assertEqual((status, err), (0, ''))
        status, out, err = w.link('--keep', THREE, base='pointing.kc', kernel='kernel2', output='again.kc')
        self.assertEqual(status, 1)
        self.assertIn(f'{THREE}: pointers to nothing in the output', err)
        self.assertTrue(err.endswith('kclink: kept kexts point at what is not kept\n'), err)


class TheSameTwice(unittest.TestCase):
    def build(self, seed):
        """A template, a collection with two kexts, and that linked again for another kernel; with python's hashes
        (the order of its sets) seeded as said."""
        w = Workdir(self.addCleanup, env={'PYTHONHASHSEED': seed})
        template = w.template()
        w.kernel()
        status, out, err = w.link('--native', w.native(SPEC_ONE, kext_one()), w.native(SPEC_TWO, kext_two()))
        self.assertEqual((status, err), (0, ''))
        w.kernel('kernel2', base=BASE + SLIDE, data_at=0x40)
        status, out, err = w.link('--keep', TWO, '--native', w.native(SPEC_THREE, kext_three('_kernel_data')),
                                  base='out.kc', kernel='kernel2', output='again.kc')
        self.assertEqual((status, err), (0, ''))
        return template, w.read('out.kc'), w.read('out.natives.json'), w.read('again.kc'), w.read('again.natives.json')

    def test_byte_for_byte(self):
        first = self.build('1')
        for seed in ('1', '2', '4711'):
            again = self.build(seed)
            for what, a, b in zip(('the template', 'the collection', 'its symbols', 'the collection linked again',
                                   'its symbols'), first, again):
                self.assertEqual(a, b, f'{what}, hash seed {seed}')
        self.assertEqual(len(first[1]) % PAGE, 0)
        self.assertEqual(sorted(json.loads(first[4])), ['_three_pointer', '_three_start', '_three_stop'])


if __name__ == '__main__':
    unittest.main()
