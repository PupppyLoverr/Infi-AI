# SPDX-License-Identifier: GPL-2.0-only
"""Links a kext from arm64e object files for a kernel collection: its sections laid out into the segments a kext in a
collection has (__TEXT with the Mach-O header, __TEXT_EXEC, __DATA_CONST, __DATA), its kmod_info, a GOT for what the
code loads through one, every relocation resolved, and every pointer handed back as a chained fixup for the collection.

Placement is the caller's: `layout()` gives each segment's size and alignment, `place()` takes the address each one
goes to, `link()` resolves and returns the bytes of each segment, its load commands and its pointers."""
import struct
import uuid

from macho import (HEADER, MH_MAGIC_64, MH_OBJECT, MH_KEXT_BUNDLE, CPU_TYPE_ARM64, LC_SEGMENT_64, LC_SYMTAB, LC_UUID,
                   N_STAB, N_PEXT, N_TYPE, N_EXT, N_UNDF, N_ABS, N_SECT, NLIST, Fixup, Image, Section, Segment,
                   align, name16)

# where each section of an object goes in the kext: (segment, section)
PLACEMENT = {
    ('__TEXT', '__text'): ('__TEXT_EXEC', '__text'),
    ('__TEXT', '__cstring'): ('__TEXT', '__cstring'),
    ('__TEXT', '__const'): ('__TEXT', '__const'),
    ('__TEXT', '__literal4'): ('__TEXT', '__literal4'),
    ('__TEXT', '__literal8'): ('__TEXT', '__literal8'),
    ('__TEXT', '__literal16'): ('__TEXT', '__literal16'),
    ('__TEXT', '__os_log'): ('__TEXT', '__os_log'),
    ('__TEXT', '__ustring'): ('__TEXT', '__ustring'),
    ('__DATA', '__const'): ('__DATA_CONST', '__const'),
    ('__DATA_CONST', '__const'): ('__DATA_CONST', '__const'),
    ('__DATA', '__kalloc_type'): ('__DATA_CONST', '__kalloc_type'),
    ('__DATA_CONST', '__kalloc_type'): ('__DATA_CONST', '__kalloc_type'),
    ('__DATA', '__kalloc_var'): ('__DATA_CONST', '__kalloc_var'),
    ('__DATA_CONST', '__kalloc_var'): ('__DATA_CONST', '__kalloc_var'),
    ('__DATA_CONST', '__assert'): ('__DATA_CONST', '__assert'),
    ('__DATA', '__mod_init_func'): ('__DATA_CONST', '__mod_init_func'),
    ('__DATA', '__mod_term_func'): ('__DATA_CONST', '__mod_term_func'),
    ('__DATA', '__data'): ('__DATA', '__data'),
    ('__DATA', '__bss'): ('__DATA', '__bss'),
    ('__DATA', '__common'): ('__DATA', '__common'),
}
# sections that carry nothing into the kext
DROPPED = {('__LD', '__compact_unwind'), ('__TEXT', '__eh_frame'), ('__LLVM', '__bitcode'), ('__LLVM', '__cmdline')}
SEGMENTS = ['__TEXT', '__TEXT_EXEC', '__DATA_CONST', '__DATA']      # their order in the kext
PROT = {'__TEXT': 1, '__TEXT_EXEC': 5, '__DATA_CONST': 3, '__DATA': 3}
SECT_FLAGS = {'__text': 0x80000400, '__cstring': 0x2, '__literal4': 0x3, '__literal8': 0x4, '__literal16': 0xe,
              '__mod_init_func': 0x9, '__mod_term_func': 0xa, '__got': 0x6, '__bss': 0x1, '__common': 0x1}
ZEROFILL = {'__bss', '__common'}

# arm64 relocation types
UNSIGNED, SUBTRACTOR, BRANCH26, PAGE21, PAGEOFF12, GOT_LOAD_PAGE21, GOT_LOAD_PAGEOFF12, POINTER_TO_GOT = range(8)
ADDEND, AUTHENTICATED_POINTER = 10, 11
CPU_SUBTYPE_ARM64E = 2

# kmod_info_t (<mach/kmod.h>, 4-byte packed): next, info_version, id, name[64], version[64], reference_count,
# reference_list, address, size, hdr_size, start, stop
KMOD_INFO_SIZE = 196
KMOD_ADDRESS, KMOD_SIZE, KMOD_START, KMOD_STOP = 156, 164, 180, 188
# kmod_info's start and stop are signed with the IA key and the type discriminator of kmod_start_func_t, as XNU calls
# them (the value Apple's own kexts carry in their kmod_info)
KMOD_FUNC_DIV = 0xe4fe
MH_FLAGS = 0x80000085           # MH_NOUNDEFS | MH_DYLDLINK | MH_TWOLEVEL | MH_DYLIB_IN_CACHE
HEADER_RESERVE = 0x1000         # room for the header in __TEXT


class LinkError(Exception):
    pass


class OutSection:
    def __init__(self, seg, name):
        self.seg, self.name = seg, name
        self.size, self.align = 0, 0        # align: log2
        self.pieces = []                    # (object index, input section index, offset in this section)
        self.data = None
        self.addr = 0


class Obj:
    def __init__(self, path):
        self.path = path
        self.buf = open(path, 'rb').read()
        self.img = Image(self.buf)
        # arm64e only (the subtype's low 24 bits; the rest is the pointer authentication ABI's version): an arm64 object
        # signs none of its pointers, and the kext is an arm64e one whatever went into it
        if (self.img.filetype != MH_OBJECT or self.img.cputype != CPU_TYPE_ARM64 or
                self.img.cpusubtype & 0xffffff != CPU_SUBTYPE_ARM64E):
            raise LinkError(f'{path}: not an arm64e object')
        self.sects = self.img.sections()       # input sections, 1-based in nlist n_sect
        self.syms = self.img.symbols()
        self.where = {}                         # input section index -> (OutSection, offset)


class Kext:
    """One kext from its spec (scripts/kexts' NAME.json: name, bundle, version, start, stop, objects...)."""

    def __init__(self, spec, objects):
        self.spec = spec
        self.objs = [Obj(p) for p in objects]
        self.out = {}                           # (seg, sect) -> OutSection, in creation order
        self.globals = {}                       # name -> (OutSection, offset) of defined external symbols
        self.got = {}                           # symbol name -> index in __got
        self.segs = {}                          # seg name -> {'size', 'align', 'addr'}
        self.symbols = {}                       # every named symbol's address once linked, for symbolizing

    def section(self, seg, name):
        key = (seg, name)
        if key not in self.out:
            self.out[key] = OutSection(seg, name)
        return self.out[key]

    def layout(self):
        """Lays out the sections into segments; returns {segment: (size, alignment in bytes)}."""
        # kmod_info first in __DATA,__data (what _PrelinkKmodInfo points at)
        kmod = self.section('__DATA', '__data')
        kmod.size, kmod.align = KMOD_INFO_SIZE, 3
        for oi, o in enumerate(self.objs):
            for si, s in enumerate(o.sects):
                key = (s.segname, s.sectname)
                if key in DROPPED or s.sectname.startswith('__debug') or s.segname == '__DWARF':
                    continue
                if key not in PLACEMENT:
                    raise LinkError(f'{o.path}: section {s.segname},{s.sectname} has no place in a kext')
                out = self.section(*PLACEMENT[key])
                off = align(out.size, 1 << s.align)
                out.pieces.append((oi, si, off))
                out.size, out.align = off + s.size, max(out.align, s.align)
                o.where[si] = (out, off)
        # common symbols: zero-filled storage in __DATA,__common (but for one an object defines: that is the symbol)
        common = {}
        defined = {n for o in self.objs for n, t, sc, d, v in o.syms
                   if not t & N_STAB and t & (N_EXT | N_PEXT) and (t & N_TYPE) == N_SECT}
        for o in self.objs:
            for n, t, sc, d, v in o.syms:
                if not t & N_STAB and (t & N_TYPE) == N_UNDF and t & N_EXT and v and n not in defined:
                    size, al = common.get(n, (0, 0))
                    common[n] = (max(size, v), max(al, (d >> 8) & 0xf))
        if common:
            out = self.section('__DATA', '__common')
            for n, (size, al) in common.items():
                off = align(out.size, 1 << al)
                out.size, out.align = off + size, max(out.align, al)
                self.globals[n] = (out, off)
        # defined external symbols (private externs too: one kext is one linkage unit). Of several definitions the one
        # that is not weak (N_WEAK_DEF) is the kext's, wherever its object comes; of weak ones only, the first
        weak = set()                            # the symbols whose definition so far is a weak one
        for oi, o in enumerate(self.objs):
            for n, t, sc, d, v in o.syms:
                if t & N_STAB or not t & (N_EXT | N_PEXT) or (t & N_TYPE) != N_SECT:
                    continue
                where = self.place_of(o, sc, v)
                if n in self.globals and self.globals[n] != where:
                    if d & 0x80:                        # N_WEAK_DEF: the definition there stays
                        continue
                    if n not in weak:
                        prev = self.globals[n]
                        raise LinkError(f'{n} defined twice (in {o.path} and at {prev[0].seg},{prev[0].name})')
                    weak.discard(n)                     # over a weak one
                elif not d & 0x80:
                    weak.discard(n)                     # (at the place of a weak one too: the two are one, not weak)
                elif n not in self.globals:
                    weak.add(n)
                self.globals[n] = where
        # a GOT entry for every symbol the code loads through one
        for o in self.objs:
            for s in o.sects:
                for r in self.relocs(o, s):
                    if r[1] in (GOT_LOAD_PAGE21, GOT_LOAD_PAGEOFF12, POINTER_TO_GOT):
                        if not r[3]:
                            raise LinkError(f'{o.path}: GOT load of a section, not a symbol')
                        name = o.syms[r[2]][0]
                        self.got.setdefault(name, len(self.got))
        if self.got:
            g = self.section('__DATA_CONST', '__got')
            g.size, g.align = 8 * len(self.got), 3
        # segments: their sections in order, zero-fill last in __DATA
        for seg in SEGMENTS:
            size = HEADER_RESERVE if seg == '__TEXT' else 0
            al = 4
            sects = [s for s in self.out.values() if s.seg == seg]
            sects.sort(key=lambda s: s.name in ZEROFILL)
            for s in sects:
                s.segoff = align(size, 1 << s.align)
                size = s.segoff + s.size
                al = max(al, s.align)
            size = align(size, 16)
            self.segs[seg] = {'size': size, 'align': 1 << al, 'addr': None, 'sects': sects}
        return {seg: (v['size'], v['align']) for seg, v in self.segs.items() if v['size']}

    def place_of(self, o, sect, value):
        s = o.sects[sect - 1]
        if sect - 1 not in o.where:
            raise LinkError(f'{o.path}: a symbol in dropped section {s.segname},{s.sectname}')
        out, off = o.where[sect - 1]
        return out, off + value - s.addr

    def place(self, addrs, base):
        """Takes each segment's address ({segment: address}); `base`: the collection's header address (a collection's
        file offsets are its addresses less that)."""
        for seg, v in self.segs.items():
            v['addr'] = addrs.get(seg, 0)
            v['fileoff'] = v['addr'] - base
            for s in v['sects']:
                s.addr = v['addr'] + s.segoff

    @staticmethod
    def relocs(o, s):
        out = []
        for i in range(s.nreloc):
            addr, info = struct.unpack_from('<iI', o.buf, s.reloff + 8 * i)
            out.append((addr, info >> 28, info & 0xffffff, (info >> 27) & 1, (info >> 25) & 3, (info >> 24) & 1))
        return out

    def addr_of(self, where):
        out, off = where
        return out.addr + off

    def vtable(self):
        """The address of the vtable of the spec's `vtable` class (the C++ class of this kext whose superclass is in
        another kext), or None."""
        vt = self.spec.get('vtable')
        if not vt:
            return None
        name = f'__ZTV{len(vt["class"])}{vt["class"]}'
        if name not in self.globals:
            raise LinkError(f'{self.spec["bundle"]}: no vtable {name}')
        return self.addr_of(self.globals[name])

    def link(self, resolve, inherited=None):
        """Resolves every relocation (`resolve(name)`: the address of a symbol outside this kext, or None) and returns
        {'segments': {seg: bytes}, 'fixups': {address: Fixup}, 'symbols': {name: address}}.
        A slot of the spec's `vtable` class that names a method no kext exports (a superclass's private override) gets
        what the superclass's vtable has in that slot: `inherited(slot)`, slots counted from the vtable's address
        point."""
        sp = self.spec
        unresolved = set()
        pending = []                            # (address, symbol, Fixup, addend) of pointers to unresolved symbols
        fixups = {}

        def sym_addr(o, idx):
            n, t, sc, d, v = o.syms[idx]
            typ = t & N_TYPE
            if typ == N_SECT and not (t & (N_EXT | N_PEXT) and n in self.globals):
                return self.addr_of(self.place_of(o, sc, v))    # (an external one: the kext's definition, below)
            if typ == N_ABS:
                return v
            if n in self.globals:
                return self.addr_of(self.globals[n])
            return resolve(n)

        def ext_addr(name):
            if name in self.globals:
                return self.addr_of(self.globals[name])
            a = resolve(name)
            if a is None:
                unresolved.add(name)
                return 0
            return a

        for s in self.out.values():
            s.data = bytearray(0 if s.name in ZEROFILL else s.size)
        for oi, o in enumerate(self.objs):
            for si, s in enumerate(o.sects):
                if si not in o.where:
                    continue
                out, off = o.where[si]
                if not s.zerofill:
                    out.data[off:off + s.size] = o.buf[s.offset:s.offset + s.size]
                addend = 0
                for raddr, typ, num, ext, length, pcrel in self.relocs(o, s):
                    if typ == ADDEND:
                        addend = num - (1 << 24) if num & (1 << 23) else num
                        continue
                    pos = off + raddr
                    place = out.addr + pos
                    if ext:
                        name = o.syms[num][0]
                        target = sym_addr(o, num)
                        if target is None and typ not in (UNSIGNED, AUTHENTICATED_POINTER, GOT_LOAD_PAGE21,
                                                          GOT_LOAD_PAGEOFF12):
                            unresolved.add(name)
                            addend = 0
                            continue
                    else:                       # a section-relative reference: its address is in the content
                        target, name = None, None
                    if typ in (UNSIGNED, AUTHENTICATED_POINTER):
                        if length != 3:
                            raise LinkError(f'{o.path}: {8 << length}-bit pointer at {s.sectname}+{raddr:#x}')
                        if place & 3:           # (a collection's chained fixups are at multiples of 4)
                            raise LinkError(f'{o.path}: pointer at {s.sectname}+{raddr:#x} not at a multiple of 4 '
                                            f'bytes ({place:#x})')
                        raw = struct.unpack_from('<Q', out.data, pos)[0]
                        if typ == AUTHENTICATED_POINTER:
                            add = raw & 0xffffffff
                            add = add - (1 << 32) if add & (1 << 31) else add
                            f = Fixup(0, (raw >> 49) & 3, (raw >> 32) & 0xffff, (raw >> 48) & 1)
                        else:
                            add, f = raw, Fixup(0)
                        if ext and target is None:
                            pending.append((place, name, f, add))
                            struct.pack_into('<Q', out.data, pos, 0)
                            addend = 0
                            continue
                        if target is None:      # section index num, the content an address in the object
                            ts = o.sects[num - 1]
                            tout, toff = o.where[num - 1]
                            target, add = tout.addr + toff + (add - ts.addr), 0
                        f.target = (target + add) & 0xffffffffffffffff
                        struct.pack_into('<Q', out.data, pos, 0)
                        fixups[place] = f
                    elif typ == BRANCH26:
                        delta = target + addend - place
                        if not -(1 << 27) <= delta < (1 << 27) or delta & 3:
                            raise LinkError(f'{o.path}: branch to {name} out of range ({delta:#x})')
                        insn = struct.unpack_from('<I', out.data, pos)[0]
                        struct.pack_into('<I', out.data, pos, (insn & 0xfc000000) | ((delta >> 2) & 0x3ffffff))
                    elif typ in (PAGE21, GOT_LOAD_PAGE21):
                        t = self.got_addr(name) if typ == GOT_LOAD_PAGE21 else target + addend
                        pages = (t >> 12) - (place >> 12)
                        if not -(1 << 20) <= pages < (1 << 20):
                            raise LinkError(f'{o.path}: adrp to {name} out of range')
                        insn = struct.unpack_from('<I', out.data, pos)[0]
                        insn = (insn & 0x9f00001f) | ((pages & 3) << 29) | (((pages >> 2) & 0x7ffff) << 5)
                        struct.pack_into('<I', out.data, pos, insn)
                    elif typ in (PAGEOFF12, GOT_LOAD_PAGEOFF12):
                        t = self.got_addr(name) if typ == GOT_LOAD_PAGEOFF12 else target + addend
                        insn = struct.unpack_from('<I', out.data, pos)[0]
                        struct.pack_into('<I', out.data, pos, pageoff12(insn, t & 0xfff, o.path, name))
                    else:
                        raise LinkError(f'{o.path}: relocation type {typ} at {s.sectname}+{raddr:#x}')
                    addend = 0
        if pending:
            vt = self.vtable()
            for place, name, f, add in pending:
                j = (place - vt - 16) // 8 if vt is not None else -1
                t = inherited(j) if j >= 0 and (place - vt) % 8 == 0 and inherited else None
                if t is None:
                    unresolved.add(name)
                    continue
                f.target = (t + add) & 0xffffffffffffffff
                fixups[place] = f
        # the GOT
        if self.got:
            g = self.out[('__DATA_CONST', '__got')]
            for name, i in self.got.items():
                fixups[g.addr + 8 * i] = Fixup(ext_addr(name))
        # kmod_info
        text, data = self.segs['__TEXT'], self.out[('__DATA', '__data')]
        k = data.data
        struct.pack_into('<QIi64s64si', k, 0, 0, 1, -1, sp['bundle'].encode(), sp['version'].encode(), -1)
        struct.pack_into('<QQQQ', k, KMOD_ADDRESS, text['addr'], text['size'], 0, 0)
        fixups[data.addr + KMOD_START] = Fixup(ext_addr(sp['start']), 0, KMOD_FUNC_DIV, 0)
        fixups[data.addr + KMOD_STOP] = Fixup(ext_addr(sp['stop']), 0, KMOD_FUNC_DIV, 0)
        if unresolved:
            raise LinkError(f'{sp["bundle"]}: unresolved symbols: {" ".join(sorted(unresolved))}')
        # every named symbol, for symbolizing
        for o in self.objs:
            for n, t, sc, d, v in o.syms:
                if n and not t & N_STAB and (t & N_TYPE) == N_SECT and sc - 1 in o.where:
                    self.symbols.setdefault(n, self.addr_of(self.place_of(o, sc, v)))
        for n, where in self.globals.items():
            self.symbols[n] = self.addr_of(where)
        segments = {}
        for seg, v in self.segs.items():
            if not v['size']:
                continue
            b = bytearray(v['size'])
            for s in v['sects']:
                if s.name not in ZEROFILL:
                    b[s.segoff:s.segoff + s.size] = s.data
            segments[seg] = b
        return {'segments': segments, 'fixups': fixups, 'symbols': dict(self.symbols)}

    def got_addr(self, name):
        return self.out[('__DATA_CONST', '__got')].addr + 8 * self.got[name]

    def uuid(self):
        return uuid.uuid5(uuid.NAMESPACE_URL, self.spec['bundle']).bytes

    def header(self, linkedit):
        """The kext's Mach-O header and load commands (written at the start of its __TEXT); `linkedit`: the
        collection's (vmaddr, fileoff, size) of the __LINKEDIT the kext's empty symbol table points into."""
        cmds = []
        order = ['__TEXT', '__TEXT_EXEC', '__DATA', '__DATA_CONST']
        for seg in order:
            v = self.segs[seg]
            if not v['size']:
                continue
            sg = Segment(name=seg, vmaddr=v['addr'], vmsize=v['size'], fileoff=v['fileoff'], filesize=v['size'],
                         prot=PROT[seg])
            for s in v['sects']:
                sec = Section(b'\0' * 80)
                sec.sectname, sec.segname = s.name, seg
                sec.addr, sec.size, sec.align = s.addr, s.size, s.align
                sec.offset = 0 if s.name in ZEROFILL else v['fileoff'] + s.segoff
                sec.flags = SECT_FLAGS.get(s.name, 0)
                if s.name == '__kalloc_type' or s.name == '__kalloc_var':
                    sec.flags = 0x8000000
                sg.sections.append(sec)
            cmds.append(sg.pack())
        le_va, le_off, le_size = linkedit
        cmds.append(Segment(name='__LINKEDIT', vmaddr=le_va, vmsize=le_size, fileoff=le_off, filesize=le_size).pack())
        cmds.append(struct.pack('<IIIIII', LC_SYMTAB, 24, le_off, 0, le_off, 0))
        cmds.append(struct.pack('<II', LC_UUID, 24) + self.uuid())
        body = b''.join(cmds)
        hdr = HEADER.pack(MH_MAGIC_64, CPU_TYPE_ARM64, 0xc0000002, MH_KEXT_BUNDLE, len(cmds), len(body), MH_FLAGS, 0)
        if len(hdr) + len(body) > HEADER_RESERVE:
            raise LinkError(f'{self.spec["bundle"]}: load commands do not fit in {HEADER_RESERVE:#x} bytes')
        return hdr + body

    def plist(self, data_va):
        sp = self.spec
        text = self.segs['__TEXT']
        d = {
            'CFBundleExecutable': sp['name'], 'CFBundleIdentifier': sp['bundle'],
            'CFBundleInfoDictionaryVersion': '6.0', 'CFBundleName': sp['name'], 'CFBundlePackageType': 'KEXT',
            'CFBundleShortVersionString': sp['version'], 'CFBundleVersion': sp['version'],
            'OSBundleCompatibleVersion': sp.get('compatible_version', '1.0.0'),
            'OSBundleLibraries': sp.get('libraries', {}), 'IOKitPersonalities': sp.get('personalities', {}),
            'OSBundleRequired': 'Root', 'OSBundleUUID': self.uuid(),
            '_PrelinkBundlePath': f'/System/Library/Extensions/{sp["name"]}.kext',
            '_PrelinkExecutableRelativePath': f'Contents/MacOS/{sp["name"]}',
            '_PrelinkExecutableLoadAddr': signed(text['addr']), '_PrelinkExecutableSourceAddr': signed(text['addr']),
            '_PrelinkExecutableSize': text['size'], '_PrelinkKmodInfo': signed(data_va),
        }
        d.update(sp.get('plist', {}))
        return d


def signed(v):
    return v - (1 << 64) if v >= 1 << 63 else v


def pageoff12(insn, lo, path, name):
    """The low 12 bits of an address into an add (immediate) or a load/store (unsigned offset, scaled)."""
    if (insn & 0x7f000000) == 0x11000000:                   # add/adds (immediate), 32 or 64-bit
        return (insn & ~(0xfff << 10)) | (lo << 10)
    if (insn & 0x3b000000) == 0x39000000:                   # ldr/str (unsigned immediate), integer or SIMD&FP
        scale = insn >> 30
        if insn & (1 << 26) and scale == 0 and insn & (1 << 23):
            scale = 4                                       # 128-bit q register
        if lo & ((1 << scale) - 1):
            raise LinkError(f'{path}: {name}: offset {lo:#x} not aligned for a {1 << scale}-byte access')
        return (insn & ~(0xfff << 10)) | ((lo >> scale) << 10)
    raise LinkError(f'{path}: {name}: page offset into instruction {insn:#010x}')
