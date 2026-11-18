#!/usr/bin/env bash
# inject-wayland-debug.sh — drop WAYLAND_DEBUG=1 into the guest image's
# /etc/profile.d so every wayland client's protocol trace lands in the
# systemd journal -> forwarded to ttyS0 -> serial.log (server-side `->`
# view: attach/commit/create_buffer per wl_surface).
#
# Usage: tests/drive/inject-wayland-debug.sh [--remove]
#   Run BEFORE booting. Loop-mounts dist/cosmosos-x86_64.raw, writes
#   (or removes) /etc/profile.d/98-wayland-debug.sh, unmounts.
#
# WARNING: makes the guest VERY noisy (~MB/s of protocol lines) and can
# RCU-stall-warn under sustained dump (serial IRQ floods CPU). Test
# boots only — never ship this into a real image.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
RAW="$ROOT/dist/cosmosos-x86_64.raw"
F=etc/profile.d/98-wayland-debug.sh
REMOVE=0; [ "${1:-}" = "--remove" ] && REMOVE=1

LOOP=$(sudo losetup -fP --show "$RAW")
trap 'sudo umount /mnt 2>/dev/null; sudo losetup -d "$LOOP" 2>/dev/null' EXIT
sudo mount "${LOOP}p1" /mnt
if [ "$REMOVE" = 1 ]; then
    sudo rm -f "/mnt/$F"
    echo "removed $F"
else
    printf '# Test-run instrumentation: trace client wayland protocol into the journal.\nexport WAYLAND_DEBUG=1\n' | sudo tee "/mnt/$F" >/dev/null
    echo "wrote $F — boot now; protocol trace lands in work/image/serial.log"
fi
