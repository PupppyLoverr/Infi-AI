#!/usr/bin/env bash
# inject-papercut-check.sh — install tests/drive/papercut-check.sh into a
# CosmosOS raw image plus a systemd oneshot that runs it at boot, dumping
# hostname/locale/zram/DRM/motd evidence to /dev/ttyS0 (lands in the QEMU
# serial log verbatim — no console typing needed).
#
# Usage: tests/drive/inject-papercut-check.sh [IMAGE] [--remove]
#   IMAGE defaults to dist/cosmosos-x86_64.raw; pass a copied/test image to
#   avoid touching dist/. Run BEFORE booting. Test boots only.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
RAW="${1:-$ROOT/dist/cosmosos-x86_64.raw}"
case "$RAW" in --*) RAW="$ROOT/dist/cosmosos-x86_64.raw"; shift;; esac
REMOVE=0; [ "${1:-}" = "--remove" ] || [ "${2:-}" = "--remove" ] && REMOVE=1

LOOP=$(sudo losetup -fP --show "$RAW")
trap 'sudo umount /mnt 2>/dev/null; sudo losetup -d "$LOOP" 2>/dev/null' EXIT
sudo mount "${LOOP}p1" /mnt

if [ "$REMOVE" = 1 ]; then
    sudo rm -f /mnt/usr/local/sbin/papercut-check.sh \
              /mnt/etc/systemd/system/papercut-check.service \
              /mnt/etc/systemd/system/multi-user.target.wants/papercut-check.service
    echo "removed papercut-check"
    exit 0
fi

sudo install -m 0755 "$ROOT/tests/drive/papercut-check.sh" /mnt/usr/local/sbin/papercut-check.sh
sudo mkdir -p /mnt/etc/systemd/system/multi-user.target.wants
sudo tee /mnt/etc/systemd/system/papercut-check.service >/dev/null <<'EOF'
[Unit]
Description=Dump papercut verification evidence to ttyS0
After=multi-user.target
# run late enough that DRM/zram/hostname are all settled
After=systemd-zram-setup@zram0.service systemd-udev-settle.service
Wants=systemd-udev-settle.service

[Service]
Type=oneshot
ExecStart=/usr/local/sbin/papercut-check.sh
StandardOutput=null

[Install]
WantedBy=multi-user.target
EOF
sudo ln -sf /etc/systemd/system/papercut-check.service \
    /mnt/etc/systemd/system/multi-user.target.wants/papercut-check.service
echo "installed papercut-check into $RAW — boot now; evidence lands in the serial log"
