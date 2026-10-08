#!/usr/bin/env bash
# notify.sh — boot a notify-check-injected image at 1920x1080 and log in
# through the greeter; the approval-card/banner clicks are then driven
# live from the host (tests/drive/qmp.sh) while the guest checker's
# socat calls block on the card decision.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
W="$ROOT/work/drive-notify"
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
trap 'kill $QPID 2>/dev/null' EXIT
for i in $(seq 1 15); do [ -S "$QMP_SOCK" ] && break; sleep 1; done
[ -S "$QMP_SOCK" ] || { echo "FATAL: qmp socket never appeared"; exit 1; }
echo "qemu pid $QPID — serial $SERIAL"
echo "QMP_SOCK=$QMP_SOCK MON_SOCK=$MON_SOCK"

hmp()  { printf '%s\n' "$1" | socat -t 8 - UNIX-CONNECT:"$MON_SOCK" >/dev/null 2>&1; }
dump() { hmp "screendump $W/$1.ppm"; sleep 0.4; [ -f "$W/$1.ppm" ] && convert "$W/$1.ppm" "$W/$1.png" 2>/dev/null; }
type() { MON_SOCK=$MON_SOCK "$ROOT/tests/drive/guest-type.sh" "$1"; }
qmp()  { QMP_SOCK=$QMP_SOCK "$ROOT/tests/drive/qmp.sh" "$@"; }

# greeter login is driven interactively from the host (dump greeter,
# click pw field, type password) — coordinates vary by resolution.

# stay alive for the interactive marker/click loop
for i in $(seq 1 1200); do
  sleep 5
  grep -q '==NOTIFY-END==' "$SERIAL" 2>/dev/null && break
done
qmp dump 99-end
kill $QPID 2>/dev/null || true
echo "== notify drive done =="
exit 0
