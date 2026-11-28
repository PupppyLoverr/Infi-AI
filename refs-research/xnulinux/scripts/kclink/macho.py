# SPDX-License-Identifier: GPL-2.0-only
"""Mach-O reading and writing for kclink: images inside a buffer (a kernel, a kernel collection's fileset entries, kext
objects), their load commands, segments, sections and symbols, and the chained fixups of a kernel collection
(DYLD_CHAINED_PTR_64_KERNEL_CACHE) and of a kernel linked with threaded rebases (__TEXT,__thread_starts).

Formats as <mach-o/loader.h>, <mach-o/nlist.h>, <mach-o/fixup-chains.h> and <mach-o/arm64/reloc.h> define them."""
import struct

MH_MAGIC_64 = 0xfeedfacf
MH_OBJECT, MH_EXECUTE, MH_KEXT_BUNDLE, MH_FILESET = 0x1, 0x2, 0xb, 0xc
CPU_TYPE_ARM64 = 0x0100000c
LC_SYMTAB, LC_UNIXTHREAD, LC_DYSYMTAB, LC_SEGMENT_64, LC_UUID = 0x2, 0x5, 0xb, 0x19, 0x1b
LC_SEGMENT_SPLIT_INFO, LC_FUNCTION_STARTS, LC_DATA_IN_CODE = 0x1e, 0x26, 0x29
LC_DYLD_CHAINED_FIXUPS, LC_FILESET_ENTRY = 0x80000034, 0x80000035
S_ZEROFILL, S_GB_ZEROFILL, S_THREAD_LOCAL_ZEROFILL = 0x1, 0xc, 0x12
N_STAB, N_PEXT, N_TYPE, N_EXT = 0xe0, 0x10, 0x0e, 0x01
N_UNDF, N_ABS, N_SECT, N_INDR = 0x0, 0x2, 0xe, 0xa

HEADER = struct.Struct('<IIIIIIII')
SEGMENT = struct.Struct('<II16sQQQQiiII')
SECTION = struct.Struct('<16s16sQQIIIIIIII')
NLIST = struct.Struct('<IBBHQ')


def cstr(b):
    return b.split(b'\0', 1)[0].decode()


def name16(s):
    return s.encode().ljust(16, b'\0')


def align(v, a):
    return (v + a - 1) & ~(a - 1)


class Section:
    def __init__(self, raw):
        (sn, sg, self.addr, self.size, self.offset, self.align, self.reloff, self.nreloc, self.flags,
         self.reserved1, self.reserved2, self.reserved3) = SECTION.unpack(raw)
        self.sectname, self.segname = cstr(sn), cstr(sg)

    @property
    def zerofill(self):
        return (self.flags & 0xff) in (S_ZEROFILL, S_GB_ZEROFILL, S_THREAD_LOCAL_ZEROFILL)

    def pack(self):
        return SECTION.pack(name16(self.sectname), name16(self.segname), self.addr, self.size, self.offset, self.align,
                            self.reloff, self.nreloc, self.flags, self.reserved1, self.reserved2, self.reserved3)


class Segment:
    def __init__(self, raw=None, name='', vmaddr=0, vmsize=0, fileoff=0, filesize=0, prot=1, flags=0):
        self.sections = []
        if raw is None:
            self.name, self.vmaddr, self.vmsize, self.fileoff, self.filesize = name, vmaddr, vmsize, fileoff, filesize
            self.maxprot = self.initprot = prot
            self.flags = flags
            return
        (_, _, n, self.vmaddr, self.vmsize, self.fileoff, self.filesize, self.maxprot, self.initprot, nsects,
         self.flags) = SEGMENT.unpack_from(raw)
        self.name = cstr(n)
        self.sections = [Section(raw[SEGMENT.size + SECTION.size * i:SEGMENT.size + SECTION.size * (i + 1)])
                         for i in range(nsects)]

    def pack(self):
        return SEGMENT.pack(LC_SEGMENT_64, SEGMENT.size + SECTION.size * len(self.sections), name16(self.name),
                            self.vmaddr, self.vmsize, self.fileoff, self.filesize, self.maxprot, self.initprot,
                            len(self.sections), self.flags) + b''.join(s.pack() for s in self.sections)

    def contains(self, va):
        return self.vmaddr <= va < self.vmaddr + self.vmsize


class Image:
    """A Mach-O image whose header is at offset `off` of `buf`; file offsets in it are relative to `base`
    (0 for a file of its own, also 0 for a kernel collection's fileset entries, whose offsets are the collection's)."""

    def __init__(self, buf, off=0, base=0):
        self.buf, self.off, self.base = buf, off, base
        magic, self.cputype, self.cpusubtype, self.filetype, ncmds, self.sizeofcmds, self.flags, _ = \
            HEADER.unpack_from(buf, off)
        if magic != MH_MAGIC_64:
            raise ValueError(f'not a 64-bit Mach-O at {off:#x}')
        self.cmds = []          # (cmd, raw bytes), in order
        self.segments = []
        self.symtab = self.dysymtab = None
        self.entries = []       # MH_FILESET: (name, vmaddr, fileoff)
        p = off + HEADER.size
        for _ in range(ncmds):
            cmd, size = struct.unpack_from('<II', buf, p)
            raw = bytes(buf[p:p + size])
            self.cmds.append((cmd, raw))
            if cmd == LC_SEGMENT_64:
                self.segments.append(Segment(raw))
            elif cmd == LC_SYMTAB:
                self.symtab = struct.unpack_from('<IIII', raw, 8)
            elif cmd == LC_DYSYMTAB:
                self.dysymtab = struct.unpack_from('<18I', raw, 8)
            elif cmd == LC_FILESET_ENTRY:
                va, fo, noff = struct.unpack_from('<QQI', raw, 8)
                self.entries.append((cstr(raw[noff:]), va, fo))
            p += size

    def cmd(self, which):
        for c, raw in self.cmds:
            if c == which:
                return raw
        return None

    def segment(self, name):
        for s in self.segments:
            if s.name == name:
                return s
        return None

    def sections(self):
        return [s for seg in self.segments for s in seg.sections]

    def section(self, seg, sect):
        for s in self.sections():
            if s.segname == seg and s.sectname == sect:
                return s
        return None

    def va_to_off(self, va):
        """The file offset (in buf) of a virtual address, or None."""
        for s in self.segments:
            if s.vmaddr <= va < s.vmaddr + s.filesize:
                return self.base + s.fileoff + (va - s.vmaddr)
        return None

    def symbols(self):
        """[(name, type, sect, desc, value)] of the image's LC_SYMTAB."""
        if not self.symtab:
            return []
        symoff, nsyms, stroff, strsize = self.symtab
        b, out = self.buf, []
        for i in range(nsyms):
            strx, typ, sect, desc, value = NLIST.unpack_from(b, self.base + symoff + NLIST.size * i)
            end = b.index(b'\0', self.base + stroff + strx)
            out.append((bytes(b[self.base + stroff + strx:end]).decode(errors='replace'), typ, sect, desc, value))
        return out

    def exports(self):
        """{name: address} of the image's external defined symbols."""
        return {n: v for n, t, s, d, v in self.symbols()
                if not t & N_STAB and t & N_EXT and (t & N_TYPE) in (N_SECT, N_ABS)}


# Chained fixups of a kernel collection: DYLD_CHAINED_PTR_64_KERNEL_CACHE, 4-byte stride
#   target:30 (offset from the collection's header)  cacheLevel:2  diversity:16  addrDiv:1  key:2  next:12  isAuth:1
PTR_64_KERNEL_CACHE = 8
KC_PAGE = 0x4000


class Fixup:
    """A pointer the collection's loader writes: the address it points to, and how it is signed (key None: plain)."""
    __slots__ = ('target', 'key', 'div', 'addr_div')

    def __init__(self, target, key=None, div=0, addr_div=0):
        self.target, self.key, self.div, self.addr_div = target, key, div, addr_div

    def __repr__(self):
        return f'Fixup({self.target:#x}' + ('' if self.key is None else f', key {self.key} div {self.div:#x}'
                                             f'{" addr" if self.addr_div else ""}') + ')'


def kc_decode(raw):
    """A DYLD_CHAINED_PTR_64_KERNEL_CACHE value: (target offset, cacheLevel, key or None, diversity, addrDiv, next)."""
    target, level = raw & 0x3fffffff, (raw >> 30) & 3
    nxt, auth = (raw >> 51) & 0xfff, raw >> 63
    if auth:
        return target, level, (raw >> 49) & 3, (raw >> 32) & 0xffff, (raw >> 48) & 1, nxt
    return target, level, None, 0, 0, nxt


def kc_encode(target, key, div, addr_div, nxt):
    if not 0 <= target < 1 << 30:
        raise ValueError(f'fixup target offset {target:#x} out of range')
    v = target | nxt << 51
    if key is not None:
        v |= div << 32 | addr_div << 48 | key << 49 | 1 << 63
    return v


def kc_read_fixups(buf, header_va, top_segments):
    """{offset from the collection's header: Fixup(target address)} of a collection's LC_DYLD_CHAINED_FIXUPS (only its
    own pointers, cacheLevel 0)."""
    img = Image(buf)
    raw = img.cmd(LC_DYLD_CHAINED_FIXUPS)
    out = {}
    if raw is None:
        return out
    dataoff, _ = struct.unpack_from('<II', raw, 8)
    _, starts_off, _, _, imports_count, _, _ = struct.unpack_from('<7I', buf, dataoff)
    if imports_count:
        raise ValueError('a kernel collection with chained imports')
    s = dataoff + starts_off
    seg_count = struct.unpack_from('<I', buf, s)[0]
    for i in range(seg_count):
        so = struct.unpack_from('<I', buf, s + 4 + 4 * i)[0]
        if not so:
            continue
        _, page_size, fmt, seg_off, _, page_count = struct.unpack_from('<IHHQIH', buf, s + so)
        if fmt != PTR_64_KERNEL_CACHE:
            raise ValueError(f'chained pointer format {fmt} (expected {PTR_64_KERNEL_CACHE})')
        for p in range(page_count):
            start = struct.unpack_from('<H', buf, s + so + 22 + 2 * p)[0]
            if start == 0xffff:
                continue
            if start & 0x8000:
                raise ValueError('multiple chain starts in a page')
            loc = seg_off + p * page_size + start
            while True:
                target, level, key, div, addr_div, nxt = kc_decode(struct.unpack_from('<Q', buf, loc)[0])
                if level:
                    raise ValueError(f'pointer at {loc:#x} into another collection (level {level})')
                out[loc] = Fixup(header_va + target, key, div, addr_div)
                if not nxt:
                    break
                loc += nxt * 4
    return out


def kc_write_fixups(out, header_va, top_segments, fixups):
    """Writes every pointer of `fixups` ({offset: Fixup}) into `out` (the collection's bytes, file offset == offset from
    the header) as DYLD_CHAINED_PTR_64_KERNEL_CACHE chains, and returns the LC_DYLD_CHAINED_FIXUPS payload.
    `top_segments`: the collection's segments in load-command order, [(name, offset, size)]."""
    per_seg = [[] for _ in top_segments]
    locs = sorted(fixups)
    si = 0
    for loc in locs:
        if loc & 3:
            raise ValueError(f'misaligned pointer at {loc:#x}')
        while si < len(top_segments) and loc >= top_segments[si][1] + top_segments[si][2]:
            si += 1
        if si == len(top_segments) or loc < top_segments[si][1]:
            raise ValueError(f'pointer at {loc:#x} outside the collection\'s segments')
        per_seg[si].append(loc)
    seg_infos = []
    for (name, seg_off, seg_size), seglocs in zip(top_segments, per_seg):
        if not seglocs:
            seg_infos.append(None)
            continue
        page_count = (seg_size + KC_PAGE - 1) // KC_PAGE
        starts = [0xffff] * page_count
        for j, loc in enumerate(seglocs):
            page = (loc - seg_off) // KC_PAGE
            if starts[page] == 0xffff:
                starts[page] = (loc - seg_off) % KC_PAGE
            nxt = 0
            if j + 1 < len(seglocs) and (seglocs[j + 1] - seg_off) // KC_PAGE == page:
                nxt = (seglocs[j + 1] - loc) // 4
            f = fixups[loc]
            struct.pack_into('<Q', out, loc, kc_encode(f.target - header_va, f.key, f.div, f.addr_div, nxt))
        seg_infos.append(struct.pack('<IHHQIH', 22 + 2 * page_count, KC_PAGE, PTR_64_KERNEL_CACHE, seg_off, 0,
                                     page_count) + struct.pack(f'<{page_count}H', *starts))
    # dyld_chained_fixups_header, dyld_chained_starts_in_image, dyld_chained_starts_in_segment (8-byte aligned)
    image = bytearray(struct.pack('<I', len(top_segments)) + b'\0' * 4 * len(top_segments))
    pos = align(len(image), 8)
    image += b'\0' * (pos - len(image))
    for i, info in enumerate(seg_infos):
        if info is None:
            continue
        struct.pack_into('<I', image, 4 + 4 * i, len(image))
        image += info
        image += b'\0' * (align(len(image), 8) - len(image))
    starts_off = 32
    blob = struct.pack('<7I', 0, starts_off, starts_off + len(image), starts_off + len(image), 0, 1, 0)
    blob += b'\0' * (starts_off - len(blob)) + image
    return bytes(blob)


def threaded_rebases(img):
    """{address: Fixup} of a kernel linked with threaded rebases: the chains listed in __TEXT,__thread_starts (as XNU's
    rebase_threaded_starts walks them). Signed pointers hold a 32-bit offset from the header; plain ones the address,
    sign-extended from 51 bits with the top byte kept apart."""
    sect = img.section('__TEXT', '__thread_starts')
    out = {}
    if sect is None or not sect.size:
        return out
    text = img.segment('__TEXT')
    words = struct.unpack_from(f'<{sect.size // 4}I', img.buf, img.base + sect.offset)
    stride = 8 if words[0] & 1 else 4
    for start in words[1:]:
        if start == 0xffffffff:
            break
        va = text.vmaddr + start
        while True:
            off = img.va_to_off(va)
            v = struct.unpack_from('<Q', img.buf, off)[0]
            if (v >> 62) & 1:
                raise ValueError(f'a threaded bind at {va:#x} in the kernel')
            if v >> 63:
                out[va] = Fixup(text.vmaddr + (v & 0xffffffff), (v >> 49) & 3, (v >> 32) & 0xffff, (v >> 48) & 1)
            else:
                top8 = (v & 0x0007f80000000000) << 13
                low = v & 0x000007ffffffffff
                if low & (1 << 42):
                    low |= ~0x000007ffffffffff & 0xffffffffffffffff
                out[va] = Fixup(top8 | (low & 0x00ffffffffffffff))
            delta = (v >> 51) & 0x7ff
            if not delta:
                break
            va += delta * stride
    return out
