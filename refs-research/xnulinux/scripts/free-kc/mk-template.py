#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-only
"""An empty MH_FILESET kernel collection to link into, made from numbers alone (layout.json) and the kernel's own
pseudo-extensions, so that nothing of a vendor's kernelcache goes into the collection scripts/free-kc/build.sh links.

    mk-template.py LAYOUT.json XNU_SOURCE KERNEL_VERSION OUTPUT

scripts/kclink/kclink.py links into a collection it is given: its top-level segments, the kernel's fileset entry (where
the kernel's header goes), the prelink info and the chained fixups. This writes a file with exactly that structure and
nothing in it: the segments are zero, the kernel entry has only its __LINKINFO slot and the shared __LINKEDIT, the fixups
have no chains, and the prelink info lists only the kernel's com.apple.kpi.* pseudo-extensions, from XNU's own
config/System.kext/PlugIns.
"""
import hashlib, json, plistlib, struct, sys
from pathlib import Path

LC_SEGMENT_64, LC_UUID, LC_BUILD_VERSION, LC_UNIXTHREAD = 0x19, 0x1b, 0x32, 0x5
LC_CHAINED_FIXUPS, LC_FILESET_ENTRY = 0x80000034, 0x80000035
BASE = 0xfffffff007004000   # VM_KERNEL_LINK_ADDRESS the kernel is linked for: the collection needs no move
KPI = {'BSDKernel': 'com.apple.kpi.bsd', 'MACFramework': 'com.apple.kpi.dsep', 'IOKit': 'com.apple.kpi.iokit',
       'Libkern': 'com.apple.kpi.libkern', 'Mach': 'com.apple.kpi.mach', 'Private': 'com.apple.kpi.private',
       'Unsupported': 'com.apple.kpi.unsupported'}

layout = json.loads(Path(sys.argv[1]).read_text())
xnu, version, output = Path(sys.argv[2]), sys.argv[3], Path(sys.argv[4])
num = lambda s: int(s, 16)   # noqa: E731
segs = [(name, num(off), num(size), prot, sect) for name, off, size, prot, sect in layout['segments']]
linkinfo_off, linkinfo_size = map(num, layout['kernel_linkinfo'])
linkedit_off = num(layout['kernel_linkedit'])

# the prelink info: the kernel's pseudo-extensions, as a kernelcache builder lists them
entries = []
for bundle, ident in KPI.items():
    text = (xnu / 'config/System.kext/PlugIns' / f'{bundle}.kext/Info.plist').read_text()
    text = text.replace('###KERNEL_VERSION_LONG###', version).replace('###KERNEL_VERSION_SHORT###', '.'.join(version.split('.')[:2]))
    entry = plistlib.loads(text.encode())
    assert entry['CFBundleIdentifier'] == ident and entry.get('OSKernelResource'), bundle
    entry.update({'_PrelinkBundlePath': f'/System/Library/Extensions/System.kext/PlugIns/{bundle}.kext',
                  '_PrelinkExecutableLoadAddr': 0x7fffffffffffffff, '_PrelinkExecutableSourceAddr': 0x7fffffffffffffff})
    entries.append(entry)
kcid = hashlib.sha256(b'xnulinux free kernel collection ' + version.encode()).digest()[:16]
info = plistlib.dumps({'_PrelinkInfoDictionary': entries, '_PrelinkKCID': kcid, 'OSPrelinkInfoContainsUUIDs': True,
                       'ManagerVersion': 1, 'KMBuildVersion': 'xnulinux-free-kc'}, fmt=plistlib.FMT_XML) + b'\0'
info_off, info_size = next((o, s) for n, o, s, _, _ in segs if n == '__PRELINK_INFO')
assert len(info) <= info_size

size = linkedit_off   # the file ends where the kernel's own link-edit data would start: kclink appends after it
image = bytearray(size)
image[info_off:info_off + len(info)] = info

# chained fixups with no chains: a header and a starts table of zero segments
fixups = struct.pack('<7I', 0, 28, 32, 32, 0, 1, 0) + struct.pack('<I', 0)
fixups += bytes(-len(fixups) % 8)
fixups_off = linkinfo_off + linkinfo_size   # after the kernel's __LINKINFO slot, before its __LINKEDIT
assert fixups_off + len(fixups) <= linkedit_off
image[fixups_off:fixups_off + len(fixups)] = fixups

# the kernel's fileset entry: a Mach-O header with its __LINKINFO slot and the shared __LINKEDIT; kclink writes the real
# kernel's header over it
kh = num(layout['kernel_header'])
kcmds = struct.pack('<II16s4Q4I', LC_SEGMENT_64, 72, b'__LINKINFO', BASE + linkinfo_off, linkinfo_size, linkinfo_off, linkinfo_size, 1, 1, 0, 0)
kcmds += struct.pack('<II16s4Q4I', LC_SEGMENT_64, 72, b'__LINKEDIT', BASE + linkedit_off, 0, linkedit_off, 0, 1, 1, 0, 0)
image[kh:kh + 32 + len(kcmds)] = struct.pack('<8I', 0xfeedfacf, 0x100000c, 0xc0000002, 2, 2, len(kcmds), 0x200001, 0) + kcmds

# the collection's header
cmds = [struct.pack('<II16s', LC_UUID, 24, hashlib.sha256(kcid).digest()[:16]),
        struct.pack('<6I', LC_BUILD_VERSION, 24, 1, 0x1b0000, 0x1b0000, 0),   # macOS 27, no tools
        struct.pack('<II', LC_UNIXTHREAD, 288) + bytes(280),                   # kclink puts the kernel's  
        struct.pack('<4I', LC_CHAINED_FIXUPS, 16, fixups_off, len(fixups))]
for name, off, size_, prot, sect in segs:
    nsect = 1 if sect else 0
    body = struct.pack('<II16s4Q4I', LC_SEGMENT_64, 72 + 80 * nsect, name.encode(), BASE + off, size_, off, size_, prot, prot, nsect, 0)
    if sect:
        body += struct.pack('<16s16s2Q8I', sect.encode(), name.encode(), BASE + off, size_, off, 0, 0, 0, 0, 0, 0, 0)
    cmds.append(body)
name = b'com.apple.kernel\0'
cmds.append(struct.pack('<IIQQII', LC_FILESET_ENTRY, 32 + len(name) + (-(32 + len(name)) % 8), BASE + kh, kh, 32, 0)
            + name + bytes(-(32 + len(name)) % 8))
blob = b''.join(cmds)
image[0:32 + len(blob)] = struct.pack('<8I', 0xfeedfacf, 0x100000c, 0xc0000002, 12, len(cmds), len(blob), 0, 0) + blob
output.write_bytes(image)
print(f'{output}: {len(image):#x} bytes, {len(entries)} pseudo-extensions')
