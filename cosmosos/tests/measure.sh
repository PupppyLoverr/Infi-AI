#!/usr/bin/env bash
# measure.sh — headless QEMU boot of the CosmosOS image that collects the
# budget numbers and writes docs/budgets.md.
#
#   image real + sparse size      (du on the raw file)
#   boot -> cosmos-panel seconds  (serial: "layer surface created" marker)
#   idle guest RAM after settle   (/proc/meminfo + free -m via the injected
#                                  papercut-check service -> /dev/ttyS0)
#
# Usage: tests/measure.sh [image.raw]     (default dist/cosmosos-x86_64.raw)
# Boots a SPARSE COPY with its own sockets so it can run beside a live
# session. Writes/updates docs/budgets.md.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SRC="${1:-$ROOT/dist/cosmosos-x86_64.raw}"
WORK="$ROOT/work/measure"
SERIAL="$WORK/serial.log"
BUDGETS="$ROOT/docs/budgets.md"
mkdir -p "$WORK"

IMG="$WORK/measure.raw"
echo "measure: copying image..."
cp --sparse=always "$SRC" "$IMG"

echo "measure: injecting check service"
"$ROOT/tests/drive/inject-papercut-check.sh" "$IMG" >/dev/null

rm -f "$SERIAL"
cp /usr/share/OVMF/OVMF_VARS.fd "$WORK/OVMF_VARS.fd"
echo "measure: booting headless (KVM, 2G, 1024x768)..."
sg kvm -c "qemu-system-x86_64 -enable-kvm -cpu host -m 2G -smp 2 \
  -drive if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE.fd \
  -drive if=pflash,format=raw,file=$WORK/OVMF_VARS.fd \
  -drive file=$IMG,format=raw,if=virtio \
  -device virtio-vga,xres=1024,yres=768 -device virtio-tablet-pci \
  -device virtio-keyboard-pci \
  -audiodev none,id=snd0 -device intel-hda -device hda-duplex,audiodev=snd0 \
  -netdev user,id=n0 -device virtio-net-pci,netdev=n0 \
  -smbios type=1,product=CosmosOS \
  -serial file:$SERIAL \
  -monitor unix:$WORK/monitor.sock,server,nowait \
  -qmp unix:$WORK/qmp.sock,server,nowait \
  -display none" > "$WORK/qemu.log" 2>&1 &
QPID=""
QPID=""
# greetd needs a login before the panel can appear: wait for the greeter
# card (~greetd.service start + greeter map), type the cosmos password,
# click Sign in. The panel marker then arrives as usual.
sleep 18
MON_SOCK="$WORK/monitor.sock" "$ROOT/tests/drive/guest-type.sh" 'cosmos' 2>/dev/null || true
QMP_SOCK="$WORK/qmp.sock" "$ROOT/tests/drive/qmp.sh" click 512 518 || true
for _ in $(seq 1 90); do
  grep -q 'PAPERCUT-CHECK-END' "$SERIAL" 2>/dev/null && \
  grep -q 'layer surface created' "$SERIAL" 2>/dev/null && break
  sleep 2
done
pkill -f "qemu-system.*measure.raw" 2>/dev/null || true

grep -q 'PAPERCUT-CHECK-END' "$SERIAL" || { echo "measure: check never finished" >&2; exit 1; }

# --- parse -----------------------------------------------------------------
REAL="$(du -h "$SRC" | cut -f1)"
SPARSE="$(du -h --apparent-size "$SRC" | cut -f1)"
PANEL_S="$(grep -oE '\[\s*[0-9]+\.[0-9]+\]' "$SERIAL" | head -1 || true)"
# panel marker: first 'layer surface created' line's kernel-relative seconds
PANEL_S="$(awk '/layer surface created/ {match($0,/\[\s*([0-9.]+)\]/,a); print a[1]; exit}' "$SERIAL")"
MEM_TOTAL="$(awk '/MemTotal:/ {print $2; exit}' "$SERIAL")"
MEM_AVAIL="$(awk '/MemAvailable:/ {print $2; exit}' "$SERIAL")"
SWAP_FREE="$(awk '/SwapFree:/ {print $2; exit}' "$SERIAL")"
SWAP_TOTAL="$(awk '/SwapTotal:/ {print $2; exit}' "$SERIAL")"
MEM_USED=$(( (MEM_TOTAL - MEM_AVAIL) / 1024 ))
DATE="$(date -u +%Y-%m-%d)"
REV="$(cd "$ROOT" && git rev-parse --short HEAD 2>/dev/null || echo '?')"

cat > "$BUDGETS" <<EOF
# CosmosOS budgets

Measured by \`tests/measure.sh\` on $DATE at $REV (headless KVM boot,
QEMU -m 2G -smp 2, GUEST_RES=1024x768, OVMF).

| metric | value |
|---|---|
| image file | $REAL real / $SPARSE sparse |
| boot -> cosmos-panel surface | ${PANEL_S:-?} s (serial marker) |
| idle RAM at desktop (used) | ~${MEM_USED} MiB of $(( MEM_TOTAL / 1024 )) MiB |
| swap | zram0 ${SWAP_FREE:-?} kB free of ${SWAP_TOTAL:-?} kB |

Raw evidence: work/measure/serial.log (==PAPERCUT-CHECK== block).
EOF

echo "measure: wrote $BUDGETS"
cat "$BUDGETS"
