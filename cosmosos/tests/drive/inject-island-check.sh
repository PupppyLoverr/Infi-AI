#!/usr/bin/env bash
# inject-island-check.sh — install tests/drive/island-check.sh into a
# CosmosOS raw image plus a systemd oneshot that runs it at boot.
# Mirrors inject-portal-check.sh (same GPT/btrfs @ layout, same
# serial-getty@ttyS0 mask so the checker keeps the serial line).
#
# Usage: tests/drive/inject-island-check.sh [IMAGE] [--remove]
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
RAW="${1:-$ROOT/dist/cosmosos-x86_64.raw}"
case "$RAW" in --*) RAW="$ROOT/dist/cosmosos-x86_64.raw"; shift;; esac
REMOVE=0; { [ "${1:-}" = "--remove" ] || [ "${2:-}" = "--remove" ]; } && REMOVE=1

LOOP=$(sudo losetup -fP --show "$RAW")
trap 'sudo umount /mnt 2>/dev/null; sudo losetup -d "$LOOP" 2>/dev/null' EXIT
sudo mount -o subvol=@ "${LOOP}p2" /mnt

if [ "$REMOVE" = 1 ]; then
    sudo rm -f /mnt/usr/local/sbin/island-check.sh \
              /mnt/etc/systemd/system/island-check.service \
              /mnt/etc/systemd/system/multi-user.target.wants/island-check.service
    echo "removed island-check"
    exit 0
fi

sudo install -m 0755 "$ROOT/tests/drive/island-check.sh" /mnt/usr/local/sbin/island-check.sh
sudo mkdir -p /mnt/etc/systemd/system/multi-user.target.wants
sudo ln -sf /dev/null /mnt/etc/systemd/system/serial-getty@ttyS0.service
sudo tee /mnt/etc/systemd/system/island-check.service >/dev/null <<'EOF'
[Unit]
Description=Verify dynamic island via wl-clipboard + QMP inputs
After=multi-user.target
After=systemd-udev-settle.service
Wants=systemd-udev-settle.service

[Service]
Type=oneshot
ExecStart=/usr/local/sbin/island-check.sh
StandardOutput=null
TimeoutStartSec=420

[Install]
WantedBy=multi-user.target
EOF
sudo ln -sf /etc/systemd/system/island-check.service \
  /mnt/etc/systemd/system/multi-user.target.wants/island-check.service
echo "installed island-check into $RAW — boot now; evidence lands in the serial log"
