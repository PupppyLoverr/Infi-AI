#!/usr/bin/env bash
# portal.sh — drive the xdg-desktop-portal verification end to end.
#
# Sparse-copies dist/cosmosos-x86_64.raw, injects portal-check, boots
# under OVMF (headless, GUEST_RES default 1024x768), waits for the serial
# markers, then harvets evidence (evidence.txt + portal-shot.png) via a
# loop mount and reports PASS/FAIL counts.
#
# Env: QMP_SOCK MON_SOCK SERIAL override the sockets/log paths so a live
# user QEMU on the default paths is never touched.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
W="$ROOT/work/drive-portal"
RAW="$W/test.raw"
mkdir -p "$W"

cp --sparse=always dist/cosmosos-x86_64.raw "$RAW"
rm -f "$W"/evidence.txt "$W"/portal-shot.png "$W"/serial.log "$W"/*.ppm "$W"/*.png

./tests/drive/inject-portal-check.sh "$RAW"

# OVMF + virtio-vga launch line, same recipe as the other drive boots.
cp /usr/share/OVMF/OVMF_VARS.fd "$W/vars.fd"
cat > "$W/launch.sh" <<EOF
#!/bin/sh
exec qemu-system-x86_64 \\
  -enable-kvm -cpu host -m 2G -smp 2 \\
  -drive if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE.fd \\
  -drive if=pflash,format=raw,file=$W/vars.fd \\
  -drive file=$RAW,format=raw,if=virtio \\
  -device virtio-vga,xres=\${GUEST_RES_X:-1024},yres=\${GUEST_RES_Y:-768} \\
  -device virtio-tablet-pci \\
  -device virtio-keyboard-pci \\
  -audiodev none,id=snd0 -device intel-hda -device hda-duplex,audiodev=snd0 \\
  -netdev user,id=n0 -device virtio-net-pci,netdev=n0 \\
  -smbios type=1,product=CosmosOS \\
  -serial file:$W/serial.log \\
  -monitor unix:$W/monitor.sock,server,nowait \\
  -qmp unix:$W/qmp.sock,server,nowait \\
  -display none
EOF
chmod +x "$W/launch.sh"

echo "== launching portal drive boot =="
setsid "$W/launch.sh" &
sleep 1
QMP_SOCK="${QMP_SOCK:-$W/qmp.sock}"
MON_SOCK="${MON_SOCK:-$W/monitor.sock}"
SERIAL="${SERIAL:-$W/serial.log}"

wait_grep() { # wait_grep PATTERN SECONDS
  local i=0
  while [ $i -lt "$2" ]; do
    grep -q "$1" "$SERIAL" 2>/dev/null && return 0
    sleep 1; i=$((i+1))
  done
  return 1
}

cleanup() {
  printf 'quit\n' | socat -t 5 - UNIX-CONNECT:"$MON_SOCK" >/dev/null 2>&1 || true
  sleep 1
  # loop-mount harvest: evidence file + PNG live on @ inside the image
  local loop
  loop=$(sudo losetup -fP --show "$RAW" 2>/dev/null || true)
  if [ -n "$loop" ]; then
    sudo mount -o subvol=@ "${loop}p2" /mnt 2>/dev/null || true
    sudo cp -a /mnt/var/lib/cosmos/portal/evidence.txt "$W/" 2>/dev/null || true
    sudo cp -a /mnt/var/lib/cosmos/portal/portal-shot.png "$W/" 2>/dev/null || true
    sudo umount /mnt 2>/dev/null || true
    sudo losetup -d "$loop" 2>/dev/null || true
    sudo chown -R "$USER" "$W" 2>/dev/null || true
  fi
}
trap cleanup EXIT

echo "== waiting for PT markers =="
wait_grep '==PT-END==' 360 || echo "!! PT-END never arrived"

grep -E 'PASS:|FAIL:|==PT-' "$SERIAL" | tail -30 || true
p=$(grep -c 'PASS:' "$SERIAL" 2>/dev/null || echo 0)
f=$(grep -c 'FAIL:' "$SERIAL" 2>/dev/null || echo 0)
echo "== PORTAL VERDICT: ${p} PASS / ${f} FAIL =="
