#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# On the Mac or on Linux: PureDarwin's root file system (scripts/puredarwin-root.sh) as the hosted system's disk, a GUID partition table
# with one journaled HFS+ volume (hfs.kext, Apple's or the free collection's, mounts it), and the ROOT_UUID xnu.ko boots it by.
# On Linux (the machine that runs xnu.ko: no Mac is needed) the volume is mkfs.hfsplus's (hfsprogs; sgdisk from gdisk; the
# kernel's hfsplus module), with a journal as hdiutil's has: without one XNU refuses the root after the first run that
# ended without an unmount ("cannot mount dirty non-journaled volumes"), and a hosted system is stopped, never unmounted.
# Linux writes to a journaled volume only with -o force, which is safe on a new one: its journal is empty and stays so.
# The kernel mounts the root read-only; PureDarwin's own images use ext4, whose kext clears that, and its mount(8) has no
# -uw for launchctl's remount, so /sbin/fsck (which launchctl runs first, on a read-only root) remounts it read-write as
# PureDarwin's HFS images do. Nothing checks the disk, here or before a boot (xnulinux-boot.sh runs no fsck.hfsplus).
#   mk-puredarwin-disk.sh ROOT.tar.gz OUT.img [SIZE (1g)]   prints the disk's configuration for xnulinux-boot.sh (MAC_CONF):
#   ROOT_UUID and BOOT_UUID, the partition's GUID
set -euo pipefail
ROOT=$1 OUT=$2 SIZE=${3:-1g}
M=$(mktemp -d); D="$OUT.dmg"
rm -f "$D"
if [ "$(uname -s)" = Linux ]; then
  uuid=$(tr a-z A-Z < /proc/sys/kernel/random/uuid)
  truncate -s "$(echo "$SIZE" | tr a-z A-Z)" "$D"
  sgdisk -o -n 1:2048:0 -t 1:af00 -c 1:PureDarwin -u 1:"$uuid" "$D" >/dev/null
  sudo modprobe hfsplus
  L=$(sudo losetup --show -fP "$D")
  trap 'sudo umount "$M" 2>/dev/null || true; sudo losetup -d "$L" 2>/dev/null || true' EXIT
  sudo mkfs.hfsplus -J -v PureDarwin "${L}p1" >/dev/null
  sudo mount -t hfsplus -o rw,force "${L}p1" "$M"
else
  hdiutil create -quiet -size "$SIZE" -fs "Journaled HFS+" -volname PureDarwin -layout GPTSPUD "$D"
  hdiutil attach -quiet -owners on -nobrowse -mountpoint "$M" "$D"
  trap 'hdiutil detach -quiet "$M" 2>/dev/null || true' EXIT
fi
sudo tar -xzf "$ROOT" -C "$M"
if [ "$(uname -s)" = Linux ]; then
  # Linux's hfsplus writes a hard link the old way, without the link chain macOS has kept since 10.5. XNU's hfs marks
  # such a file as having a chain when it first touches it, and from then on fsck finds the volume corrupt ("Incorrect
  # number of file hard links"). The root's few hard links (zsh and zsh-5.9.1) become files of their own here.
  sudo find "$M" -xdev -type f -links +1 -print0 | while IFS= read -r -d '' f; do
    sudo cp -p "$f" "$f.mkpd" && sudo mv -f "$f.mkpd" "$f"
  done
fi
sudo rm -rf "$M/.fseventsd" "$M/.Spotlight-V100"
F=$(mktemp)
cat > "$F" <<'FSCK'
#!/bin/zsh
# xnulinux: no check (nothing checks this disk before a boot); remount the HFS+ root read-write
dev=$(/bin/df / | /bin/awk 'NR==2 {print $1}')
/sbin/mount -t hfs -o update,rw "$dev" / && echo "xnulinux: root $dev remounted read-write"
exit 0
FSCK
sudo install -m 755 -o 0 -g 0 "$F" "$M/sbin/fsck"; rm -f "$F"
if [ "$(uname -s)" = Linux ]; then
  sync; sudo umount "$M"; sudo losetup -d "$L"; trap - EXIT; rmdir "$M"
else
  P=$(mktemp); diskutil info -plist "$M" > "$P"; uuid=$(plutil -extract DiskUUID raw "$P"); rm -f "$P"
  sync; hdiutil detach -quiet "$M"; trap - EXIT; rmdir "$M"
fi
mv "$D" "$OUT"
printf 'ROOT_UUID=%s\nBOOT_UUID=%s\n' "$uuid" "$uuid"
