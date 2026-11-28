# SPDX-License-Identifier: GPL-2.0-only
"""What the tests of scripts/kclink link and how they read the result back, written from the formats alone
(<mach-o/loader.h>, <mach-o/nlist.h>, <mach-o/reloc.h>, <mach-o/arm64/reloc.h>, <mach-o/fixup-chains.h>, the A64
instruction encodings) and sharing no code with scripts/kclink, so that a mistake there is not made here too:

  Object            an arm64e object file in the making: sections, symbols, relocations (no compiler)
  kernel()          a stand-in kernel: an executable with threaded rebases (__TEXT,__thread_starts) and symbols
  commands() ...    a reader of load commands, segments and symbol tables
  chained_fixups()  a walk of a collection's LC_DYLD_CHAINED_FIXUPS
  bl, adrp ...      the instructions relocations are applied to, and their operands read back
  compile()         clang, for the tests that link what a compiler really emits (skipped where there is none)
"""
import functools
import glob
import os
import shlex
import shutil
import struct
import subprocess
import sys
import tempfile
import unittest
import warnings

KCLINK = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', 'scripts', 'kclink')
sys.path.insert(0, KCLINK)                                  # for the tests' `import kextld`, `import macho`

MH_MAGIC_64, CPU_TYPE_ARM64, CPU_SUBTYPE_ARM64E = 0xfeedfacf, 0x0100000c, 2
PTRAUTH_ABI = 0x80000000        # in the subtype: the pointer authentication ABI is versioned (what a compiler writes)
MH_OBJECT, MH_EXECUTE, MH_KEXT_BUNDLE, MH_FILESET = 1, 2, 0xb, 0xc
LC_SEGMENT_64, LC_SYMTAB, LC_UNIXTHREAD, LC_DYSYMTAB, LC_UUID, LC_FUNCTION_STARTS = 0x19, 0x2, 0x5, 0xb, 0x1b, 0x26
LC_DYLD_CHAINED_FIXUPS, LC_FILESET_ENTRY = 0x80000034, 0x80000035
N_EXT, N_PEXT, N_UNDF, N_ABS, N_SECT, N_WEAK_DEF = 0x01, 0x10, 0x0, 0x2, 0xe, 0x80
S_ZEROFILL, S_CSTRING_LITERALS, S_ATTR_DEBUG, TEXT_FLAGS = 0x1, 0x2, 0x02000000, 0x80000400
# <mach-o/arm64/reloc.h>
(UNSIGNED, SUBTRACTOR, BRANCH26, PAGE21, PAGEOFF12, GOT_LOAD_PAGE21, GOT_LOAD_PAGEOFF12, POINTER_TO_GOT, TLVP_PAGE21,
 TLVP_PAGEOFF12, ADDEND, AUTHENTICATED_POINTER) = range(12)
KEY_IA, KEY_IB, KEY_DA, KEY_DB = range(4)                   # <ptrauth.h>
DYLD_CHAINED_PTR_64_KERNEL_CACHE = 8
MASK64 = (1 << 64) - 1


def align(v, a):
    return -(-v // a) * a


def name16(s):
    return s.encode().ljust(16, b'\0')


def sext(v, bits):
    return v - (1 << bits) if v >> (bits - 1) & 1 else v


def words(*insns):
    return struct.pack(f'<{len(insns)}I', *insns)


# ---- instructions (their immediates zero, as an assembler leaves what a relocation fills in) ----
BL, B, NOP, RET = 0x94000000, 0x14000000, 0xd503201f, 0xd65f03c0


def adrp(rd):
    return 0x90000000 | rd


def add(rd, rn):                                            # add xd, xn, #0
    return 0x91000000 | rn << 5 | rd


def movz(rd, imm16):                                        # mov wd, #imm16
    return 0x52800000 | imm16 << 5 | rd


# name: (the load or store of register 2 at [x1, #0], log2 of the bytes it moves: what its 12-bit offset counts in)
LOADS = {
    'strb w': (0x39000000, 0), 'ldrb w': (0x39400000, 0), 'ldrsb x': (0x39800000, 0), 'ldrsb w': (0x39c00000, 0),
    'strh w': (0x79000000, 1), 'ldrh w': (0x79400000, 1), 'ldrsh x': (0x79800000, 1), 'ldrsh w': (0x79c00000, 1),
    'str w': (0xb9000000, 2), 'ldr w': (0xb9400000, 2), 'ldrsw x': (0xb9800000, 2),
    'str x': (0xf9000000, 3), 'ldr x': (0xf9400000, 3), 'prfm': (0xf9800000, 3),
    'str b': (0x3d000000, 0), 'ldr b': (0x3d400000, 0), 'str h': (0x7d000000, 1), 'ldr h': (0x7d400000, 1),
    'str s': (0xbd000000, 2), 'ldr s': (0xbd400000, 2), 'str d': (0xfd000000, 3), 'ldr d': (0xfd400000, 3),
    'str q': (0x3d800000, 4), 'ldr q': (0x3dc00000, 4),
}
LOADS = {k: (v | 1 << 5 | 2, s) for k, (v, s) in LOADS.items()}


def branch_target(insn, pc):
    """Where the b or bl at pc goes."""
    return pc + (sext(insn & 0x3ffffff, 26) << 2)


def adrp_page(insn, pc):
    """The page the adrp at pc computes."""
    return (((pc >> 12) + sext((insn >> 5 & 0x7ffff) << 2 | insn >> 29 & 3, 21)) << 12) & MASK64


def imm12(insn):
    return insn >> 10 & 0xfff


# ---- an object file ----
class Object:
    """An arm64e MH_OBJECT: one unnamed segment, its sections one after the other from address 0, a symbol table.
    Sections and symbols are numbered as n_sect and r_symbolnum number them (from 1, from 0)."""

    def __init__(self, subtype=CPU_SUBTYPE_ARM64E | PTRAUTH_ABI):
        self.subtype = subtype
        self.sects = []         # [segment, name, data or None, size, log2 alignment, flags, address, relocations]
        self.syms = []          # (name, n_type, n_sect, n_desc, n_value)

    def section(self, seg, name, data=b'', align_=0, flags=0, zerofill=None):
        """A section with this content (zerofill: of that many bytes, with no content); returns its number."""
        addr = align(self.sects[-1][6] + self.sects[-1][3] if self.sects else 0, 1 << align_)
        if zerofill is not None:
            data, flags = None, flags | S_ZEROFILL
        self.sects.append([seg, name, None if data is None else bytearray(data),
                           zerofill if data is None else len(data), align_, flags, addr, []])
        return len(self.sects)

    def text(self, *insns):
        return self.section('__TEXT', '__text', words(*insns), 2, TEXT_FLAGS)

    def addr(self, sect, off=0):
        """The address in the object of an offset into a section."""
        return self.sects[sect - 1][6] + off

    def data(self, sect):
        return self.sects[sect - 1][2]

    def define(self, name, sect, off=0, ext=True, weak=False, private=False):
        """A symbol at an offset into a section; returns its index."""
        self.syms.append((name, N_SECT | (N_EXT if ext else 0) | (N_PEXT if private else 0), sect,
                          N_WEAK_DEF if weak else 0, self.addr(sect, off)))
        return len(self.syms) - 1

    def undefined(self, name):
        self.syms.append((name, N_UNDF | N_EXT, 0, 0, 0))
        return len(self.syms) - 1

    def absolute(self, name, value, ext=True):
        self.syms.append((name, N_ABS | (N_EXT if ext else 0), 0, 0, value))
        return len(self.syms) - 1

    def reloc(self, sect, off, typ, num, pcrel=0, length=2, extern=1, addend=None, minus=None):
        """A relocation_info for the bytes at an offset into a section. num: a symbol's index, or a section's number
        when extern is 0. addend: an ARM64_RELOC_ADDEND in front of it; minus: an ARM64_RELOC_SUBTRACTOR of that
        symbol in front of it (the two are a pair)."""
        def info(typ, num, pcrel, length, extern):
            return struct.pack('<iI', off, num & 0xffffff | pcrel << 24 | length << 25 | extern << 27 | typ << 28)
        entries = info(typ, num, pcrel, length, extern)
        if addend is not None:
            entries = info(ADDEND, addend, 0, 2, 0) + entries
        if minus is not None:
            entries = info(SUBTRACTOR, minus, 0, length, 1) + entries
        self.sects[sect - 1][7].append(entries)

    def pointer(self, sect, off, sym, addend=0):
        """.quad sym + addend"""
        struct.pack_into('<q', self.data(sect), off, addend)
        self.reloc(sect, off, UNSIGNED, sym, length=3)

    def auth_pointer(self, sect, off, sym, key, diversity=0, addr_div=0, addend=0):
        """.quad sym + addend@AUTH(key, diversity[, addr]): what the 8 bytes hold until the pointer is signed is
        addend:32 diversity:16 addrDiv:1 key:2 and, top bit, that it is to be signed."""
        struct.pack_into('<Q', self.data(sect), off, addend & 0xffffffff | diversity << 32 | addr_div << 48 |
                         key << 49 | 1 << 63)
        self.reloc(sect, off, AUTHENTICATED_POINTER, sym, length=3)

    def build(self):
        nsects = len(self.sects)
        ncmds_size = 72 + 80 * nsects + 24
        pos = 32 + ncmds_size
        body, offsets = bytearray(), []
        for seg, name, data, size, al, flags, addr, relocs in self.sects:
            if data is None:
                offsets.append(0)
                continue
            pad = align(pos + len(body), 1 << al) - pos - len(body)
            body += bytes(pad)
            offsets.append(pos + len(body))
            body += data
        body += bytes(align(len(body), 8) - len(body))
        reloffs = []
        for s in self.sects:
            reloffs.append(pos + len(body) if s[7] else 0)
            body += b''.join(reversed(s[7]))            # as an assembler writes them: the last one first
        strtab, nlists = bytearray(b'\0'), bytearray()
        for name, typ, sect, desc, value in self.syms:
            nlists += struct.pack('<IBBHQ', len(strtab), typ, sect, desc, value)
            strtab += name.encode() + b'\0'
        symoff = pos + len(body)
        end = self.sects[-1][6] + self.sects[-1][3] if self.sects else 0
        filesize = sum(s[3] for s in self.sects if s[2] is not None)
        cmds = struct.pack('<II16sQQQQiiII', LC_SEGMENT_64, 72 + 80 * nsects, b'', 0, end, pos, filesize, 7, 7, nsects,
                           0)
        for (seg, name, data, size, al, flags, addr, relocs), off, reloff in zip(self.sects, offsets, reloffs):
            cmds += struct.pack('<16s16sQQIIIIIIII', name16(name), name16(seg), addr, size, off, al, reloff,
                                sum(map(len, relocs)) // 8, flags, 0, 0, 0)
        cmds += struct.pack('<IIIIII', LC_SYMTAB, 24, symoff, len(self.syms), symoff + len(nlists), len(strtab))
        return struct.pack('<IIIIIIII', MH_MAGIC_64, CPU_TYPE_ARM64, self.subtype, MH_OBJECT, 2, len(cmds), 0, 0) + \
            cmds + bytes(body) + bytes(nlists) + bytes(strtab)

    def write(self, directory, name):
        path = os.path.join(directory, name)
        with open(path, 'wb') as f:
            f.write(self.build())
        return path


# ---- reading a Mach-O ----
def commands(buf, off=0):
    """[(cmd, its bytes)] of the image whose header is at off."""
    magic, _, _, _, ncmds, _, _, _ = struct.unpack_from('<8I', buf, off)
    assert magic == MH_MAGIC_64, hex(magic)
    out, p = [], off + 32
    for _ in range(ncmds):
        cmd, size = struct.unpack_from('<II', buf, p)
        out.append((cmd, bytes(buf[p:p + size])))
        p += size
    return out


def segments(buf, off=0):
    """[{name, vmaddr, vmsize, fileoff, filesize, maxprot, initprot, flags, sections: [{sectname, segname, addr, size,
    offset, align, reloff, nreloc, flags}]}] of the image at off, in load command order."""
    out = []
    for cmd, raw in commands(buf, off):
        if cmd != LC_SEGMENT_64:
            continue
        n, vmaddr, vmsize, fileoff, filesize, maxprot, initprot, nsects, flags = struct.unpack_from('<16s4Q4I', raw, 8)
        seg = dict(name=n.rstrip(b'\0').decode(), vmaddr=vmaddr, vmsize=vmsize, fileoff=fileoff, filesize=filesize,
                   maxprot=maxprot, initprot=initprot, flags=flags, sections=[])
        for i in range(nsects):
            sn, sg, addr, size, offset, al, reloff, nreloc, sflags = struct.unpack_from('<16s16sQQIIIII', raw,
                                                                                        72 + 80 * i)
            seg['sections'].append(dict(sectname=sn.rstrip(b'\0').decode(), segname=sg.rstrip(b'\0').decode(),
                                        addr=addr, size=size, offset=offset, align=al, reloff=reloff, nreloc=nreloc,
                                        flags=sflags))
        out.append(seg)
    return out


def symtab(buf, off=0, base=0):
    """[(name, n_type, n_sect, n_desc, n_value)] of the image at off (its file offsets counted from base)."""
    for cmd, raw in commands(buf, off):
        if cmd == LC_SYMTAB:
            symoff, nsyms, stroff, strsize = struct.unpack_from('<4I', raw, 8)
            out = []
            for i in range(nsyms):
                strx, typ, sect, desc, value = struct.unpack_from('<IBBHQ', buf, base + symoff + 16 * i)
                end = buf.index(b'\0', base + stroff + strx)
                out.append((bytes(buf[base + stroff + strx:end]).decode(), typ, sect, desc, value))
            return out
    return []


def fileset_entries(buf):
    """{name: (vmaddr, fileoff)} of a collection's LC_FILESET_ENTRY commands."""
    out = {}
    for cmd, raw in commands(buf):
        if cmd == LC_FILESET_ENTRY:
            vmaddr, fileoff, name_off = struct.unpack_from('<QQI', raw, 8)
            out[raw[name_off:].split(b'\0')[0].decode()] = (vmaddr, fileoff)
    return out


class Pointer:
    """One dyld_chained_ptr_64_kernel_cache_rebase: target:30 cacheLevel:2 diversity:16 addrDiv:1 key:2 next:12
    isAuth:1, from the lowest bit up."""

    def __init__(self, raw):
        self.raw = raw
        self.target, self.cache_level = raw & 0x3fffffff, raw >> 30 & 3
        self.diversity, self.addr_div, self.key = raw >> 32 & 0xffff, raw >> 48 & 1, raw >> 49 & 3
        self.next, self.auth = raw >> 51 & 0xfff, raw >> 63

    def signing(self):
        """(key, diversity, address diversity) of a signed pointer, None of a plain one."""
        return (self.key, self.diversity, self.addr_div) if self.auth else None


def chained_fixups(buf, payload=None):
    """Walks the chains of a collection's LC_DYLD_CHAINED_FIXUPS (or of the payload at (offset, size) of buf).
    Returns (header, starts, pointers):
    header: the dyld_chained_fixups_header's seven fields; starts: per segment of the collection, None or
    {offset, size, page_size, pointer_format, segment_offset, max_valid_pointer, page_start: [...]}, offset from the
    payload's start; pointers: {offset from the collection's header: Pointer}, in the order the chains are walked."""
    if payload is None:
        raw = next(raw for cmd, raw in commands(buf) if cmd == LC_DYLD_CHAINED_FIXUPS)
        payload = struct.unpack_from('<II', raw, 8)
    dataoff, datasize = payload
    header = struct.unpack_from('<7I', buf, dataoff)
    image = dataoff + header[1]                             # dyld_chained_starts_in_image
    seg_count = struct.unpack_from('<I', buf, image)[0]
    starts, pointers = [], {}
    for i in range(seg_count):
        info = struct.unpack_from('<I', buf, image + 4 + 4 * i)[0]
        if not info:
            starts.append(None)
            continue
        assert image + info + 22 <= dataoff + datasize
        size, page_size, fmt, seg_off, max_valid, page_count = struct.unpack_from('<IHHQIH', buf, image + info)
        page_start = list(struct.unpack_from(f'<{page_count}H', buf, image + info + 22))
        starts.append(dict(offset=header[1] + info, size=size, page_size=page_size, pointer_format=fmt,
                           segment_offset=seg_off, max_valid_pointer=max_valid, page_start=page_start))
        for page, start in enumerate(page_start):
            if start == 0xffff:                             # DYLD_CHAINED_PTR_START_NONE
                continue
            loc = seg_off + page * page_size + start
            while True:
                p = pointers[loc] = Pointer(struct.unpack_from('<Q', buf, loc)[0])
                if not p.next:
                    break
                loc += 4 * p.next                           # DYLD_CHAINED_PTR_64_KERNEL_CACHE: a stride of 4
    return header, starts, pointers


# ---- a stand-in kernel ----
def thread_pointer(target, nxt=0, sign=None, text=0):
    """A threaded rebase as the static linker leaves it in a kernel (what XNU's rebase_threaded_starts reads):
    plain: the address' low 43 bits, its top byte in bits 43-50; signed (key, diversity, address diversity): the
    32-bit offset from __TEXT, then diversity:16 addrDiv:1 key:2; next:11 at bit 51, isBind at 62, isAuth at 63."""
    if sign is None:
        return target & 0x7ffffffffff | (target >> 56) << 43 | nxt << 51
    key, diversity, addr_div = sign
    return (target - text) & 0xffffffff | diversity << 32 | addr_div << 48 | key << 49 | nxt << 51 | 1 << 63


def kernel(segs, symbols=(), chains=(), stride=4, extra=()):
    """An arm64e executable like a kernel linked for a collection.
    segs: [(name, vmaddr, content, [(section name, offset in the segment, size, flags)])], __TEXT first: its first
    0x1000 bytes are left for the header, and it gets the __thread_starts section, at 0x1000.
    symbols: [(name, address, external)]; chains: [[(address, target, signing or None)]], each one chain of
    threaded rebases in address order; extra: more load commands, as bytes."""
    segs = [(n, va, bytearray(c), list(sects)) for n, va, c, sects in segs]
    text = segs[0]
    assert text[0] == '__TEXT'

    def at(va):
        for n, base, content, _ in segs:
            if base <= va < base + len(content):
                return content, va - base
        raise ValueError(hex(va))

    starts = [0 if stride == 4 else 1]
    for chain in chains:
        starts.append(chain[0][0] - text[1])
        for (va, target, sign), following in zip(chain, list(chain[1:]) + [None]):
            nxt = (following[0] - va) // stride if following else 0
            assert nxt < 0x800 and (not following or (following[0] - va) % stride == 0), 'too far for one chain'
            content, off = at(va)
            struct.pack_into('<Q', content, off, thread_pointer(target, nxt, sign, text[1]))
    starts.append(0xffffffff)
    text[2][0x1000:0x1000 + 4 * len(starts)] = words(*starts)
    text[3].insert(0, ('__thread_starts', 0x1000, 4 * len(starts), 0))

    sections = [(n, va + off) for n, va, c, sects in segs for sn, off, size, fl in sects]
    strtab, nlists = bytearray(b'\0'), bytearray()
    for name, value, external in symbols:
        n_sect = max(i + 1 for i, (seg, a) in enumerate(sections) if a <= value)    # the section it lies in
        nlists += struct.pack('<IBBHQ', len(strtab), N_SECT | (N_EXT if external else 0), n_sect, 0, value)
        strtab += name.encode() + b'\0'
    cmds, fileoff = [], 0
    for n, va, content, sects in segs:
        cmd = struct.pack('<II16sQQQQiiII', LC_SEGMENT_64, 72 + 80 * len(sects), name16(n), va, len(content), fileoff,
                          len(content), 3, 3, len(sects), 0)
        for sn, off, size, fl in sects:
            cmd += struct.pack('<16s16sQQIIIIIIII', name16(sn), name16(n), va + off, size, fileoff + off, 0, 0, 0, fl,
                               0, 0, 0)
        cmds.append(cmd)
        fileoff += len(content)
    linkedit = bytes(nlists) + bytes(strtab)
    last = segs[-1]
    cmds.append(struct.pack('<II16sQQQQiiII', LC_SEGMENT_64, 72, name16('__LINKEDIT'), last[1] + len(last[2]),
                            align(len(linkedit), 0x4000), fileoff, len(linkedit), 1, 1, 0, 0))
    cmds.append(struct.pack('<IIIIII', LC_SYMTAB, 24, fileoff, len(symbols), fileoff + len(nlists), len(strtab)))
    cmds.append(struct.pack('<II', LC_UNIXTHREAD, 288) + struct.pack('<II', 6, 68) + bytes(256) +
                struct.pack('<QQ', symbols[0][1] if symbols else 0, 0))         # ARM_THREAD_STATE64: pc
    cmds += extra
    body = b''.join(cmds)
    assert 32 + len(body) <= 0x1000
    text[2][0:32 + len(body)] = struct.pack('<IIIIIIII', MH_MAGIC_64, CPU_TYPE_ARM64, 0xc0000002, MH_EXECUTE,
                                            len(cmds), len(body), 0x200001, 0) + body
    return b''.join(bytes(c) for n, va, c, sects in segs) + linkedit


# ---- linking a kext with kextld, the way kclink.py does ----
SPEC = {'name': 'Test', 'bundle': 'org.example.kext.test', 'version': '1.2.3', 'start': '_test_start',
        'stop': '_test_stop'}
HEADER_VA = 0xfffffff007004000                               # the collection's header
KEXT_VA = {'__TEXT': HEADER_VA + 0x100000, '__TEXT_EXEC': HEADER_VA + 0x2000000, '__DATA_CONST': HEADER_VA + 0x1000000,
           '__DATA': HEADER_VA + 0x3000000}
OUTSIDE = {'_test_start': HEADER_VA + 0x2f00000, '_test_stop': HEADER_VA + 0x2f00040}   # what the kext links against


class Linked:
    """A kext linked by kextld: its segments' bytes at their addresses, its pointers, its symbols."""

    def __init__(self, kx, result, addrs):
        self.kx, self.addrs = kx, addrs
        self.segments, self.fixups, self.symbols = result['segments'], result['fixups'], result['symbols']

    def bytes(self, va, n):
        for seg, b in self.segments.items():
            if self.addrs[seg] <= va and va + n <= self.addrs[seg] + len(b):
                return bytes(b[va - self.addrs[seg]:va - self.addrs[seg] + n])
        raise ValueError(f'{va:#x} is in no segment')

    def u32(self, va):
        return struct.unpack('<I', self.bytes(va, 4))[0]

    def u64(self, va):
        return struct.unpack('<Q', self.bytes(va, 8))[0]

    def section(self, seg, name):
        """(address, size) of a section of the kext."""
        s = self.kx.out[(seg, name)]
        return s.addr, s.size


def kext(objects, spec=None):
    """A kextld.Kext of object files (paths), with the spec's keys over SPEC's."""
    import kextld
    with warnings.catch_warnings():
        warnings.simplefilter('ignore', ResourceWarning)    # (kextld leaves closing its objects to the collector)
        return kextld.Kext(dict(SPEC, **(spec or {})), objects)


def link(objects, outside=None, spec=None, addrs=None, inherited=None):
    """Links object files (paths) into a kext placed at KEXT_VA (or addrs); outside: {symbol: address} of what is not
    in the kext. Raises kextld.LinkError as kextld does."""
    kx = kext(objects, spec)
    sizes = kx.layout()
    addrs = dict(KEXT_VA, **(addrs or {}))
    for seg, (size, al) in sizes.items():
        assert addrs[seg] % al == 0, (seg, al)
    kx.place(addrs, HEADER_VA)
    return Linked(kx, kx.link(dict(OUTSIDE, **(outside or {})).get, inherited), addrs)


# ---- a compiler, where there is one ----
PROBES = {      # what a test needs of the compiler: (file suffix, a source that shows it, a relocation type it gives)
    'asm': ('.s', '.text\n_f:\n\tbl _g\n', BRANCH26),
    'auth': ('.s', '.data\n_p:\n\t.quad _g@AUTH(ia,7)\n', AUTHENTICATED_POINTER),
    'c': ('.c', 'extern int g(int);\nint f(int x) { return g(x) + 1; }\n', BRANCH26),
}
CFLAGS = ['-O1', '-ffreestanding', '-nostdinc', '-fno-stack-protector']


def relocation_types(buf):
    return {struct.unpack_from('<I', buf, s['reloff'] + 8 * i + 4)[0] >> 28
            for seg in segments(buf) for s in seg['sections'] for i in range(s['nreloc'])}


def run_compiler(cc, source, suffix, directory, name='probe'):
    """(the object's path, None), or (None, what the compiler said)."""
    src, obj = os.path.join(directory, name + suffix), os.path.join(directory, name + '.o')
    with open(src, 'w') as f:
        f.write(source)
    try:
        r = subprocess.run(cc + ['--target=arm64e-apple-macos'] + (CFLAGS if suffix == '.c' else []) +
                           ['-c', src, '-o', obj], capture_output=True, text=True)
    except OSError as e:
        return None, str(e)
    return (obj, None) if r.returncode == 0 else (None, r.stderr.strip() or f'exit status {r.returncode}')


@functools.lru_cache(maxsize=None)
def compiler(kind):
    """(command, None) of a clang that makes arm64e Mach-O objects of this kind of source, or (None, why not).
    $KCLINK_TEST_CC names the compiler to use instead of the clang on the path."""
    named = os.environ.get('KCLINK_TEST_CC')
    if named:
        candidates = [shlex.split(named)]
    else:                                                   # clang, then clang-NN, the newest first
        versioned = {os.path.basename(p) for d in os.get_exec_path() for p in glob.glob(os.path.join(d, 'clang-*'))}
        versioned = sorted((c for c in versioned if c[6:].isdigit()), key=lambda c: -int(c[6:]))
        candidates = [[c] for c in ['clang'] + versioned if shutil.which(c)]
    suffix, source, reloc = PROBES[kind]
    why = 'no clang on the path'
    for cc in candidates:
        with tempfile.TemporaryDirectory() as d:
            obj, err = run_compiler(cc, source, suffix, d)
            if obj is None:
                why = f'{" ".join(cc)} makes no arm64e object of it: {err.splitlines()[-1]}'
                continue
            with open(obj, 'rb') as f:
                buf = f.read()
            magic, cpu, sub, filetype = struct.unpack_from('<4I', buf)
            if (magic, cpu, sub & 0xffffff, filetype) != (MH_MAGIC_64, CPU_TYPE_ARM64, CPU_SUBTYPE_ARM64E, MH_OBJECT):
                why = f'{" ".join(cc)} --target=arm64e-apple-macos made something else than an arm64e object'
            elif reloc not in relocation_types(buf):
                why = f'{" ".join(cc)} wrote no relocation of type {reloc} for it'
            else:
                return cc, None
    return None, why


def compile(test, source, kind='asm', name='t'):
    """Compiles a source of this kind ('asm', 'auth': assembly with signed pointers, 'c') for the test, which is
    skipped where no compiler does that; returns the object's path."""
    cc, why = compiler(kind)
    if cc is None:
        raise unittest.SkipTest(f'needs a compiler ({kind}): {why}')
    d = tempfile.mkdtemp()
    test.addCleanup(shutil.rmtree, d)
    obj, err = run_compiler(cc, source, PROBES[kind][0], d, name)
    if obj is None:
        test.fail(f'{" ".join(cc)} compiles the probe but not this source:\n{err}')
    return obj


def relocations(buf):
    """[(section number, r_address, type, symbolnum, extern, length, pcrel)] of an object, in the file's order."""
    out = []
    sects = [s for seg in segments(buf) for s in seg['sections']]
    for n, s in enumerate(sects, 1):
        for i in range(s['nreloc']):
            addr, info = struct.unpack_from('<iI', buf, s['reloff'] + 8 * i)
            out.append((n, addr, info >> 28, info & 0xffffff, info >> 27 & 1, info >> 25 & 3, info >> 24 & 1))
    return out


def describe(buf):
    """What an object says, with nothing of how its symbol table happens to be numbered: each section's bytes, its
    relocations in the file's order with their symbols by name, and where its external symbols are."""
    sects = [s for seg in segments(buf) for s in seg['sections']]
    syms = symtab(buf)
    out = {}
    for n, s in enumerate(sects, 1):
        zerofill = s['flags'] & 0xff == S_ZEROFILL
        relocs = [(addr, typ, syms[num][0] if ext else num, ext, length, pcrel)
                  for sn, addr, typ, num, ext, length, pcrel in relocations(buf) if sn == n]
        content = s['size'] if zerofill else bytes(buf[s['offset']:s['offset'] + s['size']])
        out[(s['segname'], s['sectname'])] = (content, s['align'], relocs)
    for name, typ, sect, desc, value in syms:
        if typ & N_EXT:
            where = (sects[sect - 1]['sectname'], value - sects[sect - 1]['addr']) if typ & 0xe == N_SECT else None
            out[name] = (typ, desc, where)
    return out
