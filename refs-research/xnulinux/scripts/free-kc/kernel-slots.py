#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-only
"""The kernel's segment slots inside the kernel collection, from numbers alone (layout.json): macOS-DroidVM's
k27/segaddrs.sh, the -segaddr list the kernel is linked with, for a host that has no Apple kernelcache.

    kernel-slots.py LAYOUT.json KERNEL.macho [--write k27/segaddrs.sh]

The slots follow the kernel's size. With macOS 27.2's kernelcache scripts/vm-xnu-rebuild.sh has k27/kc27-layout.py work
them out again after every kernel build. Without it nothing did: the kernel was linked with the segaddrs.sh
macOS-DroidVM commits, which is for its own kernel, and with xnu-patches __TEXT_EXEC is larger than its slot there. The
linker lays it over __KLD and __LAST without a word, and scripts/free-kc/build.sh ends in kclink's "segments __TEXT_EXEC
and __PRELINK_INFO overlap".

The placement is kc27-layout.py's: __TEXT_EXEC ends where __KLD starts, __KLDDATA, __DATA and __BOOTDATA end where the
kernelcache's __BOOTDATA ends, and a segment that grows has MARGIN of room. What it reads from the kernelcache is four
offsets (layout.json's kernel_slots) and the top-level segments mk-template.py builds its collection from. The file written
is the one kc27-layout.py writes, byte for byte, so the kernel is the same with the kernelcache and without. Nothing is
written, and the exit status is 1, for a kernel that does not fit.
"""
import json, struct, sys
from pathlib import Path

KB = 0xfffffff007004000   # the collection's first byte, where the kernel is linked (mk-template.py's BASE)
PAGE, MARGIN = 0x4000, 0x10000
GROWS = ('__TEXT', '__DATA_CONST', '__TEXT_EXEC', '__DATA', '__BOOTDATA')   # segments given MARGIN of room to grow
MOVE = '-Wl,-rename_section,__TEXT,__cstring,__TEXT_EXEC,__cstring'   # the kernel's strings are linked into __TEXT_EXEC
EMPTY = ['__PRELINK_DATA', '__PRELINK_INFO', '__PLK_TEXT_EXEC', '__PLK_DATA_CONST', '__PLK_LLVM_COV', '__PLK_LINKEDIT']
ORDER = ('__TEXT', '__DATA_CONST', '__LASTDATA_CONST', '__TEXT_EXEC', '__KLD', '__LAST', '__KLDDATA', '__DATA', '__BOOTDATA')

args = [a for a in sys.argv[1:] if a != '--write']
write = Path(args.pop(sys.argv.index('--write') - 1)) if '--write' in sys.argv else None
layout, kernel = json.loads(Path(args[0]).read_text()), Path(args[1]).read_bytes()
num = lambda s: int(s, 16)   # noqa: E731
up = lambda v: -(-v // PAGE) * PAGE   # noqa: E731
top = {name: (num(off), num(off) + num(size)) for name, off, size, _, _ in layout['segments']}
slot = {name: num(off) for name, off in layout['kernel_slots'].items()}

# the kernel's segment sizes, with __TEXT,__cstring counted where MOVE puts it (a kernel linked without it has it in __TEXT)
ours, off, cstring = {}, 32, 0
for _ in range(struct.unpack_from('<I', kernel, 16)[0]):
    cmd, size = struct.unpack_from('<II', kernel, off)
    if cmd == 0x19:   # LC_SEGMENT_64
        name = kernel[off + 8:off + 24].rstrip(b'\0').decode()
        ours[name] = struct.unpack_from('<Q', kernel, off + 32)[0]
        for s in range(struct.unpack_from('<I', kernel, off + 64)[0]):
            if name == '__TEXT' and kernel[off + 72 + 80 * s:off + 88 + 80 * s].rstrip(b'\0') == b'__cstring':
                cstring = struct.unpack_from('<Q', kernel, off + 72 + 80 * s + 40)[0]
    off += size
if cstring:
    ours['__TEXT'], ours['__TEXT_EXEC'] = up(ours['__TEXT'] - cstring), up(ours['__TEXT_EXEC'] + cstring)
need = {name: up(size + (MARGIN if name in GROWS else 0)) for name, size in ours.items()}

place = {'__TEXT': num(layout['kernel_header']), '__DATA_CONST': top['__DATA_CONST'][0], '__LASTDATA_CONST': slot['__LASTDATA_CONST'],
         '__TEXT_EXEC': slot['__KLD'] - need['__TEXT_EXEC'], '__KLD': slot['__KLD'], '__LAST': slot['__LAST']}
place['__KLDDATA'] = slot['__BOOTDATA_END'] - need['__KLDDATA'] - need['__DATA'] - need['__BOOTDATA']
place['__DATA'] = place['__KLDDATA'] + need['__KLDDATA']
place['__BOOTDATA'] = place['__DATA'] + need['__DATA']
place['__PRELINK_TEXT'] = top['__PRELINK_INFO'][0]
linkinfo, linkinfo_size = map(num, layout['kernel_linkinfo'])

# each segment inside the collection's segment it belongs to (the data ones may start in the tail of __PRELINK_INFO, which
# the free collection leaves empty), and none over the next
data = (top['__PRELINK_INFO'][0], top['__DATA'][1])
inside = {'__TEXT': top['__PRELINK_TEXT'], '__DATA_CONST': top['__DATA_CONST'], '__LASTDATA_CONST': top['__DATA_CONST'],
          '__TEXT_EXEC': top['__TEXT_EXEC'], '__KLD': top['__TEXT_EXEC'], '__LAST': top['__TEXT_EXEC'], '__KLDDATA': data,
          '__DATA': data, '__BOOTDATA': top['__DATA']}
bad, end = [], 0
for name in ORDER:
    s, e = place[name], place[name] + need[name]
    ok = inside[name][0] <= s and e <= inside[name][1] and end <= s
    print(f'{name:18} {s:#10x}-{e:#10x} ours {ours[name]:#9x} {"ok" if ok else "DOES NOT FIT"}')
    if not ok: bad.append(name)
    end = e
if ours.get('__LINKINFO', 0) > linkinfo_size: bad.append('__LINKINFO')
if place['__KLDDATA'] < top['__PRELINK_INFO'][0] + 0x100000: bad.append('the prelink info (less than 1 MiB left)')
if bad:
    sys.exit(f'{args[1]}: no room in the collection for {", ".join(bad)}')
if write:
    o = up(place['__BOOTDATA'] + need['__BOOTDATA'])
    write.write_text('\n'.join([
        '# -segaddr list for linking the kernel into the kc27 fileset KC (generated by k27/kc27-layout.py; offsets from the KC header page)',
        f'KB={KB:#x}', f'echo -n "{MOVE} "', 'a() { printf -- "-Wl,-segaddr,%s,0x%x " "$1" $((KB + $2)); }', '{',
        ' '.join(f'a {name} {place[name]:#x};' for name in ORDER + ('__PRELINK_TEXT',)),
        f'o={o:#x}; for n in {" ".join(EMPTY)}; do a $n $o; o=$((o+{PAGE:#x})); done', f'a __LINKINFO {linkinfo:#x}',
        "} | sed 's/ $//'"]) + '\n')
    write.with_suffix('.json').write_text(json.dumps({'DATA_CONST_START': top['__DATA_CONST'][0], 'DATA_START': place['__KLDDATA']}) + '\n')
    print('wrote', write)
