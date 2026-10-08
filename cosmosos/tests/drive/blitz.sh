#!/usr/bin/env bash
# blitz.sh — visual-blitz verify boot: sparse copy @1920x1080, greeter
# login, then host-driven screendumps. No injected checker — the host
# drives everything via HMP/QMP so each blitz item can re-verify on a
# fresh pull without rebuilding the injection.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
W="$ROOT/work/drive-blitz"
RAW="$W/test.raw"
mkdir -p "$W"
QMP_SOCK="$W/qmp-$$.sock"; MON_SOCK="$W/monitor-$$.sock"; SERIAL="$W/serial.log"
export QMP_SOCK MON_SOCK

SRC="${COSMOS_IMG:-dist/cosmosos-x86_64.raw}"
[ -f "$RAW" ] || cp --sparse=always "$SRC" "$RAW"
rm -f "$SERIAL"

cp -f /usr/share/OVMF/OVMF_VARS_4M.fd "$W/vars.fd" 2>/dev/null || true
qemu-system-x86_64 -enable-kvm -m 2G -smp 2 \
  -drive if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE_4M.fd \
  -drive if=pflash,format=raw,file="$W/vars.fd" \
  -drive file="$RAW",format=raw,if=virtio \
  -device virtio-tablet-pci \
  -device virtio-vga,xres=1920,yres=1080 -display none \
  -serial file:$SERIAL \
  -monitor unix:$MON_SOCK,server,nowait -qmp unix:$QMP_SOCK,server,nowait \
  -smbios type=1,product=CosmosOS -no-reboot &
QPID=$!
for i in $(seq 1 15); do [ -S "$QMP_SOCK" ] && break; sleep 1; done
[ -S "$QMP_SOCK" ] || { echo "FATAL: qmp socket never appeared"; exit 1; }
echo "qemu pid $QPID — serial $SERIAL"
echo "QMP_SOCK=$QMP_SOCK MON_SOCK=$MON_SOCK"
echo "$QPID" > "$W/qemu.pid"
# stay alive — host drives dumps/logins interactively
for i in $(seq 1 1200); do
  sleep 5
  grep -q '==BLITZ-END==' "$SERIAL" 2>/dev/null && break
done
kill $QPID 2>/dev/null || true
echo "== blitz drive done =="
