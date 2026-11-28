#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# scripts/mk-puredarwin-disk.sh on this host, Linux or a Mac: a disk from a small root file system made here, then the
# disk looked at from outside (check.py: the partition and its GUID against what the script printed, the HFS+ volume
# and its journal, every file with its owner, mode and content). Like the script it needs sudo; on Linux hfsprogs,
# gdisk and the kernel's hfsplus module, which it must be able to load: a host without them fails, it does not skip.
#   tests/puredarwin-disk/run.sh [SCRIPT]	(SCRIPT: another copy of mk-puredarwin-disk.sh to try)
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
SCRIPT=${1:-$HERE/../../scripts/mk-puredarwin-disk.sh}
W=$(mktemp -d); M="$W/mnt"; mkdir "$M"
trap 'rm -rf "$W"' EXIT
python3 "$HERE/check.py" --make-root "$W/root.tar.gz"
bash "$SCRIPT" "$W/root.tar.gz" "$W/pd.img" 64m > "$W/pd.conf"
cat "$W/pd.conf"
[ ! -e "$W/pd.img.dmg" ] || { echo "the script left its work file $W/pd.img.dmg"; exit 1; }
if [ "$(uname -s)" = Linux ]; then
  L=$(sudo losetup --show -fP -r "$W/pd.img")
  trap 'sudo umount "$M" 2>/dev/null || true; sudo losetup -d "$L" 2>/dev/null || true; rm -rf "$W"' EXIT
  sudo fsck.hfsplus -fn "${L}p1"
  sudo mount -t hfsplus -o ro "${L}p1" "$M"
else
  trap 'sudo hdiutil detach -quiet "$M" 2>/dev/null || true; rm -rf "$W"' EXIT
  sudo hdiutil attach -quiet -readonly -owners on -nobrowse -imagekey diskimage-class=CRawDiskImage -mountpoint "$M" "$W/pd.img"
fi
sudo python3 "$HERE/check.py" "$W/pd.img" "$W/pd.conf" "$W/root.tar.gz" "$M"
echo "mk-puredarwin-disk.sh on $(uname -s): ok"
