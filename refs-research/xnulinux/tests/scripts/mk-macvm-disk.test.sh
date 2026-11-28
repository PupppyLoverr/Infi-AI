#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# scripts/mk-macvm-disk.sh, its first step only: which attached disk images it force-detaches before it makes its clone.
# hdiutil is a stand-in: `info -plist` lists four attached images, `detach` is only written down, and `attach` fails,
# which ends the script there (on Linux its `cp -c` does, one line earlier). No image is touched; diskutil and sudo
# are stand-ins that fail, should the script ever get to them.
. "$(dirname "$0")/lib.sh"
sandbox_repo
V=$HOME/.tart/vms; mkdir -p "$V/xnumac" "$V/xnupoc"; : > "$V/xnumac/disk.img"; : > "$V/xnupoc/disk.img"; : > "$HOME/installed.img"
# attached: disk21 an earlier run's clone, disk22 the installed disk (the default SRC), disk23 another VM's disk, which
# is called disk.img as every tart VM's is, disk24 an image under another name
python3 - "$T/macvm/mac.img" "$V/xnumac/disk.img" "$V/xnupoc/disk.img" "$HOME/installed.img" > "$T/info.plist" <<'EOF'
import plistlib, sys
image = lambda path, n: {'image-path': path, 'system-entities': [
    {'dev-entry': f'/dev/disk{n}', 'content-hint': 'GUID_partition_scheme'}, {'dev-entry': f'/dev/disk{n}s2', 'content-hint': 'Apple_APFS'}]}
sys.stdout.buffer.write(plistlib.dumps({'images': [image(p, 21 + i) for i, p in enumerate(sys.argv[1:])]}))
EOF
stub hdiutil <<'EOF'
case $1 in info) cat "$T/info.plist";; attach) exit 1;; esac
EOF
for c in diskutil sudo; do stub $c <<'EOF'
exit 1
EOF
done
detached() { grep '^hdiutil detach ' "$T/calls" | sed 's|.*/dev/||; s| .*||' | sort | tr '\n' ' '; }

/bin/bash "$T/repo/scripts/mk-macvm-disk.sh" > "$T/out" 2>&1
check "detached first: the clone an earlier run left (mac.img) and the installed disk, not another VM's disk.img" [ "$(detached)" = 'disk21 disk22 ' ]
: > "$T/calls"; SRC=$HOME/installed.img /bin/bash "$T/repo/scripts/mk-macvm-disk.sh" >> "$T/out" 2>&1
check 'with SRC=<an image of another name>: the clone and that image, and no disk.img' [ "$(detached)" = 'disk21 disk24 ' ]
[ $FAILED = 0 ] || sed 's/^/    | /' "$T/out" "$T/calls"
finish
