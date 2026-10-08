#!/usr/bin/env bash
# inject-notify-check.sh — install tests/drive/notify-check.sh into a
# CosmosOS raw image plus a systemd oneshot that runs it at boot.
# Mirrors inject-island-check.sh (GPT/btrfs @ layout, serial-getty mask).
#
# Usage: tests/drive/inject-notify-check.sh [IMAGE] [--remove]
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
RAW="${1:-$ROOT/dist/cosmosos-x86_64.raw}"
case "$RAW" in --*) RAW="$ROOT/dist/cosmosos-x86_64.raw"; shift;; esac
REMOVE=0; { [ "${1:-}" = "--remove" ] || [ "${2:-}" = "--remove" ]; } && REMOVE=1

LOOP=$(sudo losetup -fP --show "$RAW")
trap 'sudo umount /mnt 2>/dev/null; sudo losetup -d "$LOOP" 2>/dev/null' EXIT
sudo mount -o subvol=@ "${LOOP}p2" /mnt

if [ "$REMOVE" = 1 ]; then
    sudo rm -f /mnt/usr/local/sbin/notify-check.sh \
              /mnt/etc/systemd/system/notify-check.service \
              /mnt/etc/systemd/system/multi-user.target.wants/notify-check.service
    echo "removed notify-check"
    exit 0
fi

sudo install -m 0755 "$ROOT/tests/drive/notify-check.sh" /mnt/usr/local/sbin/notify-check.sh
sudo mkdir -p /mnt/etc/systemd/system/multi-user.target.wants
sudo ln -sf /dev/null /mnt/etc/systemd/system/serial-getty@ttyS0.service
sudo tee /mnt/etc/systemd/system/notify-check.service >/dev/null <<'EOF'
[Unit]
Description=Verify notification actions + approval cards via QMP inputs
After=multi-user.target
After=systemd-udev-settle.service
Wants=systemd-udev-settle.service

[Service]
Type=oneshot
ExecStart=/usr/local/sbin/notify-check.sh
StandardOutput=null
TimeoutStartSec=600
EOF
sudo ln -sf ../notify-check.service /mnt/etc/systemd/system/multi-user.target.wants/notify-check.service
echo "injected notify-check into $RAW"
