#!/usr/bin/env bash
# inject-assist-tray-check.sh — install tests/drive/assist-tray-check.sh
# into a CosmosOS raw image plus a systemd oneshot that runs it at boot.
# Mirrors inject-papercut-check.sh (same GPT/btrfs @ subvol layout, same
# serial-getty@ttyS0 mask so the checker keeps the serial line).
#
# Usage: tests/drive/inject-assist-tray-check.sh [IMAGE] [--remove]
#   IMAGE defaults to dist/cosmosos-x86_64.raw; pass a copied/test image to
#   avoid touching dist/. Run BEFORE booting. Test boots only.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
RAW="${1:-$ROOT/dist/cosmosos-x86_64.raw}"
case "$RAW" in --*) RAW="$ROOT/dist/cosmosos-x86_64.raw"; shift;; esac
REMOVE=0; { [ "${1:-}" = "--remove" ] || [ "${2:-}" = "--remove" ]; } && REMOVE=1

LOOP=$(sudo losetup -fP --show "$RAW")
trap 'sudo umount /mnt 2>/dev/null; sudo losetup -d "$LOOP" 2>/dev/null' EXIT
# GPT layout: p1 = FAT32 ESP, p2 = btrfs (guest root lives in subvol @).
sudo mount -o subvol=@ "${LOOP}p2" /mnt

if [ "$REMOVE" = 1 ]; then
    sudo rm -f /mnt/usr/local/sbin/assist-tray-check.sh \
              /mnt/etc/systemd/system/assist-tray-check.service \
              /mnt/etc/systemd/system/multi-user.target.wants/assist-tray-check.service
    echo "removed assist-tray-check"
    exit 0
fi

sudo install -m 0755 "$ROOT/tests/drive/assist-tray-check.sh" /mnt/usr/local/sbin/assist-tray-check.sh
sudo mkdir -p /mnt/etc/systemd/system/multi-user.target.wants
# serial-getty on ttyS0 vhangup's the line ~12s in and kills live markers —
# in test boots the serial line is OUR channel, mask the getty.
sudo ln -sf /dev/null /mnt/etc/systemd/system/serial-getty@ttyS0.service
sudo tee /mnt/etc/systemd/system/assist-tray-check.service >/dev/null <<'EOF'
[Unit]
Description=Assert assist-tray regression via compositor IPC events
After=multi-user.target
# needs the graphical session up (cosmos-ipc.sock lives in the user runtime)
After=systemd-udev-settle.service
Wants=systemd-udev-settle.service

[Service]
Type=oneshot
ExecStart=/usr/local/sbin/assist-tray-check.sh
StandardOutput=null
# the checker watches events with its own timeouts; let it run long
TimeoutStartSec=420

[Install]
WantedBy=multi-user.target
EOF
sudo ln -sf /etc/systemd/system/assist-tray-check.service \
    /mnt/etc/systemd/system/multi-user.target.wants/assist-tray-check.service
echo "installed assist-tray-check into $RAW — boot via tests/drive/assist-tray.sh"
