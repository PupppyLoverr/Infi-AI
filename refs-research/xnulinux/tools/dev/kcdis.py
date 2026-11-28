#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-only
# Disassemble [va, va+len) of a fileset kernel collection (top-level LC_SEGMENT_64s) with llvm-objdump. Debugging aid.
import re, shutil, struct, subprocess, sys, tempfile
path, va, n = sys.argv[1], int(sys.argv[2], 16), int(sys.argv[3], 0)
d = open(path, 'rb').read()
ncmds = struct.unpack_from('<I', d, 16)[0]; off = 32
for _ in range(ncmds):
    cmd, sz = struct.unpack_from('<II', d, off)
    if cmd == 0x19:
        vmaddr, vmsize, fileoff, filesize = struct.unpack_from('<QQQQ', d, off + 24)
        if vmaddr <= va < vmaddr + filesize:
            fo = fileoff + (va - vmaddr); code = d[fo:fo + n]
            code = code[:len(code) & ~3]    # whole instructions (a length that is not a multiple of 4)
            # an arm64e object whose __text is this code AT va: the disassembler then gives every pc-relative operand (bl, b.cond,
            # cbz, tbz, adr, adrp, ldr of a literal) as the address in the collection it is, not as an offset into a scratch file
            obj = struct.pack('<8I', 0xfeedfacf, 0x100000c, 2, 1, 1, 152, 0, 0) + \
                struct.pack('<II16s4Q4I', 0x19, 152, b'', va, len(code), 184, len(code), 7, 7, 1, 0) + \
                struct.pack('<16s16s2Q8I', b'__text', b'__TEXT', va, len(code), 184, 2, 0, 0, 0x80000400, 0, 0, 0) + code
            objdump = shutil.which('llvm-objdump') or shutil.which('xcrun') and subprocess.run(
                ['xcrun', '-f', 'llvm-objdump'], capture_output=True, text=True).stdout.strip()
            if not objdump:
                sys.exit('kcdis: no llvm-objdump (on the path, or Xcode\'s)')
            with tempfile.TemporaryDirectory() as t:
                open(t + '/c.o', 'wb').write(obj)
                # -z: a run of zero words is one '...' line otherwise
                r = subprocess.run([objdump, '-d', '-z', '--no-show-raw-insn', '--mattr=+v8.5a,+pauth', t + '/c.o'],
                                   capture_output=True, text=True)
            if r.returncode:
                sys.exit('kcdis: ' + r.stderr.strip())
            for l in r.stdout.splitlines():
                a, _, insn = l.strip().partition(':')
                if '\t' in insn and re.fullmatch('[0-9a-f]+', a):
                    print('%x %s' % (int(a, 16), re.sub(r' <[^>]*>$', '', insn.strip())))    # (<__text+0x..>: its place in c.o)
            sys.exit(0)
    off += sz
print('va not found')
