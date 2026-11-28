#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-only
"""Write the raw static trust cache module of this Mac's OS (the payload of StaticTrustCache.img4) to OUT.

    extract-trustcache.py OUT [PREBOOT]

Run on the Mac whose OS the hosted system disk was made from, or give the mounted Preboot volume of that disk (an
installed macOS VM disk, scripts/mk-macvm-disk.sh). The output is Apple data: it stays local (the VM's
/lib/firmware/xnu/trustcache-mac.bin, xnulinux-boot.sh's TRUSTCACHE, passed to xnu.ko as trustcache=), never in the repository.
"""
import glob, struct, sys

preboot = sys.argv[2] if len(sys.argv) > 2 else '/System/Volumes/Preboot'
paths = glob.glob(preboot + '/*/boot/*/usr/standalone/firmware/FUD/StaticTrustCache.img4')
if len(paths) != 1:
    sys.exit(f'expected one StaticTrustCache.img4, found {paths}')
d = open(paths[0], 'rb').read()
# IMG4 > IM4P: the payload is the OCTET STRING after the 'trst' tag and the version string
i = d.find(b'trst')
if i < 0:
    sys.exit('no trst payload')
j = d.find(b'\x04', i + 4 + 3)                     # OCTET STRING tag after the IA5String version
n = d[j + 1]
if n & 0x80:
    k = n & 0x7f
    length = int.from_bytes(d[j + 2:j + 2 + k], 'big'); start = j + 2 + k
else:
    length = n; start = j + 2
payload = d[start:start + length]
version, count = struct.unpack_from('<I', payload, 0)[0], struct.unpack_from('<I', payload, 20)[0]
if version not in (1, 2) or len(payload) < 24 + count * (24 if version == 2 else 22):
    sys.exit(f'unexpected trust cache: version {version}, {count} entries, {len(payload)} bytes')
open(sys.argv[1], 'wb').write(payload)
print(f'{paths[0]}: trust cache v{version}, {count} entries, {len(payload)} bytes -> {sys.argv[1]}')
