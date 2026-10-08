#!/usr/bin/env bash
# assist-tray.sh — host driver for the assist-tray regression test.
#
# Bug (fixed in #90): while the Snap Assist picker was up it covered the
# screen with a fullscreen surface, so a menubar-tray click was swallowed —
# quick-settings never opened and the picker stayed up. This test asserts
# the fixed contract on EVERY run:
#   1. super+Right snap  -> assist picker opens   (ipc snap_assist open:true)
#   2. tray click        -> picker closes          (ipc snap_assist open:false)
#   3. pill click in the -> a config(appearance)   (proves the flyout really
#      quick-settings        event broadcasts        opened and takes input)
#      flyout's Theme row
#
# It copies the image, injects the guest checker (assist-tray-check.sh),
# boots headless, sends inputs on a schedule via HMP sendkey + QMP clicks,
# then prints PASS/FAIL from the guest's evidence file.
#
# Usage: tests/drive/assist-tray.sh [IMAGE]
#   IMAGE defaults to dist/cosmosos-x86_64.raw (copied — dist/ untouched).
#   Work dir: work/drive-assist/ (serial.log, evidence.txt, dumps, PNGs).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
SRC="${1:-$ROOT/dist/cosmosos-x86_64.raw}"
W="$ROOT/work/drive-assist"
mkdir -p "$W"
export MON_SOCK="$W/monitor.sock" QMP_SOCK="$W/qmp.sock"

log() { echo "[assist-tray] $*"; }
hmp() { printf '%s\n' "$1" | socat -t 2 - UNIX-CONNECT:"$MON_SOCK" >/dev/null 2>&1; }
wait_marker() { # wait_marker PATTERN SECONDS
  local i=0
  while [ $i -lt "$2" ]; do
    grep -q "$1" "$W/serial.log" 2>/dev/null && return 0
    sleep 2; i=$((i+2))
  done
  return 1
}

# --- fresh image copy + inject checker -------------------------------------
log "copying image (sparse)"
cp --sparse=always "$SRC" "$W/test.raw"
rm -f "$W/evidence.txt" "$W/events.log" "$W/flyout.png" "$W"/assist-*.png
"$ROOT/tests/drive/inject-assist-tray-check.sh" "$W/test.raw"
rm -f "$W/serial.log"          # -serial file: appends at fd offset; fresh file
cp /usr/share/OVMF/OVMF_VARS.fd "$W/vars.fd"

# --- boot -------------------------------------------------------------------
cat > "$W/launch.sh" <<EOF
#!/bin/bash
cd "$ROOT"
exec qemu-system-x86_64 -enable-kvm -cpu host -m 2G -smp 2 \\
  -drive if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE.fd \\
  -drive if=pflash,format=raw,file=$W/vars.fd \\
  -drive file=$W/test.raw,format=raw,if=virtio \\
  -device virtio-vga,xres=1024,yres=768 -device virtio-tablet-pci \\
  -audiodev none,id=snd0 -device intel-hda -device hda-duplex,audiodev=snd0 \\
  -netdev user,id=n0 -device virtio-net-pci,netdev=n0 \\
  -smbios type=1,product=CosmosOS \\
  -serial file:$W/serial.log \\
  -monitor unix:$W/monitor.sock,server,nowait \\
  -qmp unix:$W/qmp.sock,server,nowait \\
  -display none
EOF
chmod +x "$W/launch.sh"
log "booting (headless, OVMF, 1024x768)"
setsid "$W/launch.sh" &>/dev/null &
QPID=$!
cleanup() {
  kill "$QPID" 2>/dev/null || true
  sleep 1
  # harvest whatever the guest left inside the image
  LOOP=$(sudo losetup -fP --show "$W/test.raw" 2>/dev/null || true)
  if [ -n "$LOOP" ]; then
    sudo mount -o subvol=@,ro "${LOOP}p2" /mnt 2>/dev/null && {
      sudo cp -r /mnt/var/lib/cosmos/assist-tray/. "$W/" 2>/dev/null || true
      sudo umount /mnt
    }
    sudo losetup -d "$LOOP" 2>/dev/null || true
  fi
}
trap cleanup EXIT

# --- wait for the guest checker ---------------------------------------------
log "waiting for ==AT-READY== (guest subscriber up)"
if ! wait_marker '==AT-READY==' 300; then
  log "FAIL: guest checker never ready"; tail -20 "$W/serial.log" || true; exit 1
fi
sleep 4

# --- drive the inputs --------------------------------------------------------
# Host and guest ping-pong on serial markers: the guest checker watches the
# IPC event stream and tells us when to send each input, so nothing here
# depends on window spawn positions or timing luck.
# two windows on ws0 so the assist has a candidate
log "opening terminal + files"
hmp 'sendkey meta_l-ret';  sleep 3
hmp 'sendkey meta_l-e';    sleep 2

# guest focuses the newest window itself via ipc (spawn positions vary)
wait_marker '==AT-FOCUSED==' 90 || log "WARN: no AT-FOCUSED — snapping anyway"
sleep 1

# snap the focused window right -> picker should open on the left half
log "super+Right snap (assist should open)"
hmp 'sendkey meta_l-right'
sleep 2
"$ROOT/tests/drive/shot.sh" assist-1-snapped 2>/dev/null || true

# tray click once the picker is confirmed open (or after a grace period)
wait_marker 'PASS: assist picker opened' 60 || log "WARN: picker-open marker missing — clicking tray anyway"
sleep 1
# tray zone at 1024px wide: x>=~915. 950 verified inside.
log "tray click (picker must close + quick-settings must open)"
"$ROOT/tests/drive/qmp.sh" click 950 16
sleep 2
"$ROOT/tests/drive/shot.sh" assist-2-trayclick 2>/dev/null || true

# wait for the guest's prompt, then click inside the flyout's dark/light
# pill (measured ~(965,233) at 1024x768) — toggle + restore.
wait_marker '==AT-CLICK-PILL==' 90 || log "WARN: no CLICK-PILL marker — clicking anyway"
log "pill click x2 (proves flyout alive)"
"$ROOT/tests/drive/qmp.sh" click 965 233
sleep 2
"$ROOT/tests/drive/qmp.sh" click 965 233
sleep 2
"$ROOT/tests/drive/shot.sh" assist-3-flyout 2>/dev/null || true

# --- verdict ------------------------------------------------------------------
log "waiting for ==AT-END=="
wait_marker '==AT-END==' 120 || log "WARN: AT-END timeout — harvesting anyway"
sleep 1
kill "$QPID" 2>/dev/null || true
sleep 1

EV="$W/evidence.txt"
[ -f "$EV" ] || EV=$(sudo sh -c 'LOOP=$(losetup -fP --show '"$W"'/test.raw) && mount -o subvol=@,ro ${LOOP}p2 /mnt && cat /mnt/var/lib/cosmos/assist-tray/evidence.txt && umount /mnt && losetup -d $LOOP' 2>/dev/null) || true
echo "================ guest evidence ================"
if [ -f "$EV" ]; then cat "$EV"; else echo "(no evidence file — see serial.log)"; fi
echo "================================================="
P=$(grep -c '^PASS:' "$EV" 2>/dev/null || echo 0)
F=$(grep -c '^FAIL:' "$EV" 2>/dev/null || echo 0)
log "verdict: $P PASS / $F FAIL  (evidence: $W/evidence.txt, serial: $W/serial.log)"
[ "$F" = 0 ] && [ "$P" -ge 3 ]
