#!/usr/bin/env bash
# inject-v4-blockers-check.sh — install tests/drive/v4-blockers-check.sh into a
# CosmosOS raw image plus a systemd oneshot that runs it at boot.
# Mirrors inject-island-check.sh (GPT/btrfs @ layout, serial-getty mask).
#
# Usage: tests/drive/inject-v4-blockers-check.sh [IMAGE] [--remove]
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
RAW="${1:-$ROOT/dist/cosmosos-x86_64.raw}"
case "$RAW" in --*) RAW="$ROOT/dist/cosmosos-x86_64.raw"; shift;; esac
REMOVE=0; { [ "${1:-}" = "--remove" ] || [ "${2:-}" = "--remove" ]; } && REMOVE=1

LOOP=$(sudo losetup -fP --show "$RAW")
trap 'sudo umount /mnt 2>/dev/null; sudo losetup -d "$LOOP" 2>/dev/null' EXIT
sudo mount -o subvol=@ "${LOOP}p2" /mnt

if [ "$REMOVE" = 1 ]; then
    sudo rm -f /mnt/usr/local/sbin/v4-blockers-check.sh \
              /mnt/etc/systemd/system/v4-blockers-check.service \
              /mnt/etc/systemd/system/multi-user.target.wants/v4-blockers-check.service
    echo "removed v4-blockers-check"
    exit 0
fi

sudo install -m 0755 "$ROOT/tests/drive/v4-blockers-check.sh" /mnt/usr/local/sbin/v4-blockers-check.sh
sudo mkdir -p /mnt/etc/systemd/system/multi-user.target.wants
sudo ln -sf /dev/null /mnt/etc/systemd/system/serial-getty@ttyS0.service
sudo tee /mnt/etc/systemd/system/v4-blockers-check.service >/dev/null <<'EOF'
[Unit]
Description=Verify v4 blockers: logind session, lock takeover, Files counts
After=multi-user.target
After=systemd-udev-settle.service
Wants=systemd-udev-settle.service

[Service]
Type=oneshot
ExecStart=/usr/local/sbin/v4-blockers-check.sh
StandardOutput=null
TimeoutStartSec=600
EOF
sudo ln -sf ../v4-blockers-check.service /mnt/etc/systemd/system/multi-user.target.wants/v4-blockers-check.service
echo "injected v4-blockers-check into $RAW"
