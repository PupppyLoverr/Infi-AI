#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# On a Linux host: PureDarwin's arm64 root file system as a tarball, from one of its disk images (Nix: .#image-arm64-virt-minimal
# of the fork AFK-surf/PureDarwin, docs/free-kc.md; hydra.lab004.dev's release images are upstream's, without its fixes). The image's root is the GPT partition
# "Darwin ext4 Root"; owners and modes are kept.
#   puredarwin-root.sh IMAGE OUT.tar.gz
set -euo pipefail
IMG=$1 OUT=$2
start=$(python3 - "$IMG" <<'PY'
import struct, sys
f = open(sys.argv[1], 'rb'); f.seek(512)
h = f.read(92); lba, n, size = struct.unpack('<QIi', h[72:88])
f.seek(lba * 512)
for _ in range(n):
    e = f.read(size)
    if e[56:128].decode('utf-16le').rstrip('\0') == 'Darwin ext4 Root':
        print(struct.unpack('<Q', e[32:40])[0]); break
else:
    sys.exit('no "Darwin ext4 Root" partition')
PY
)
M=$(mktemp -d); trap 'sudo umount "$M" 2>/dev/null; rmdir "$M"' EXIT
sudo mount -o ro,loop,offset=$((start * 512)) "$IMG" "$M"
sudo tar -C "$M" --numeric-owner -czf "$OUT" .
echo "$OUT: $(du -h "$OUT" | cut -f1)"
