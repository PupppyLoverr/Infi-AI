#!/usr/bin/env bash
# lock.sh — drive the greetd + session-lock verification end to end.
# Sparse-copy -> inject -> OVMF headless boot -> greeter login via
# QMP-key typing -> session lock flows -> panic sweep -> verdict.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
W="$ROOT/work/drive-lock"
RAW="$W/test.raw"
mkdir -p "$W"
QMP_SOCK="$W/qmp-$$.sock"; MON_SOCK="$W/monitor-$$.sock"; SERIAL="$W/serial.log"
export QMP_SOCK MON_SOCK

SRC="${COSMOS_IMG:-dist/cosmosos-x86_64.raw}"
cp --sparse=always "$SRC" "$RAW"
rm -f "$W"/*.png "$W"/*.ppm "$SERIAL"
rm -rf "$W/lock"
./tests/drive/inject-lock-check.sh "$RAW"

cp /usr/share/OVMF/OVMF_VARS.fd "$W/vars.fd"
cat > "$W/launch.sh" <<EOF
#!/bin/sh
exec qemu-system-x86_64 -enable-kvm -cpu host -m 2G -smp 2 \\
  -drive if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE.fd \\
  -drive if=pflash,format=raw,file=$W/vars.fd \\
  -drive file=$RAW,format=raw,if=virtio \\
  -device virtio-vga,xres=1024,yres=768 -device virtio-tablet-pci \\
  -device virtio-keyboard-pci \\
  -audiodev none,id=snd0 -device intel-hda -device hda-duplex,audiodev=snd0 \\
  -netdev user,id=n0 -device virtio-net-pci,netdev=n0 \\
  -smbios type=1,product=CosmosOS \\
  -serial file:$SERIAL \\
  -monitor unix:$MON_SOCK,server,nowait -qmp unix:$QMP_SOCK,server,nowait \\
  -display none
EOF
chmod +x "$W/launch.sh"
setsid "$W/launch.sh" &

# Fail fast if QEMU never starts (e.g. a stale QEMU still holds the
# image write-lock — sockets never appear and every stage would ghost).
for i in $(seq 1 15); do [ -S "$QMP_SOCK" ] && break; sleep 1; done
[ -S "$QMP_SOCK" ] || { echo "FATAL: qemu qmp socket never appeared — image lock or launch failure"; exit 1; }

wait_marker() { local i=0; while [ $i -lt "$2" ]; do grep -q "$1" "$SERIAL" 2>/dev/null && return 0; sleep 1; i=$((i+1)); done; return 1; }
dump() {
  printf 'screendump %s\n' "$W/$1.ppm" | socat -t 8 - UNIX-CONNECT:"$MON_SOCK" >/dev/null 2>&1 || true
  sleep 1
  for _ in 1 2 3; do
    /usr/bin/python3 -c "from PIL import Image; Image.open('$W/$1.ppm').save('$W/$1.png')" 2>/dev/null && break
    sleep 1
  done || true
}
qmp()  { QMP_SOCK=$QMP_SOCK "$ROOT/tests/drive/qmp.sh" "$@"; }
hmp()  { printf '%s\n' "$1" | socat -t 8 - UNIX-CONNECT:"$MON_SOCK" >/dev/null 2>&1; }
qkey() { "$ROOT/tests/drive/qkey.sh" "$@"; }
type()     { MON_SOCK=$MON_SOCK RUN=1 "$ROOT/tests/drive/guest-type.sh" "$1"; }
typeno()   { MON_SOCK=$MON_SOCK "$ROOT/tests/drive/guest-type.sh" "$1"; }
gsubmit()  { qmp click 512 518; sleep 0.5; }   # greeter Sign in button
lsubmit()  { qkey ret; sleep 0.5; }   # lock handles Enter via typed_return

# Centered card presence: greeter/lock cards are dark-theme but dense
# with bright text + the password field's accent border. Count
# bright/lum>100 px in the centre band vs the corners.
card_px() {
  /usr/bin/python3 - "$1" <<'PY'
from PIL import Image
import sys
im = Image.open(sys.argv[1]).convert("RGB")
w, h = im.size
cx, cy = w // 2, h // 2
blue = txt = 0
for x in range(cx - 180, cx + 180, 3):
    for y in range(cy - 140, cy + 140, 3):
        r, g, b = im.getpixel((x, y))[:3]
        if r + g + b > 400:
            txt += 1
        if b > 150 and b > r + 60 and b > g + 30:  # accent-blue field border
            blue += 1
print(f"blue-border={blue} white-text={txt}", file=sys.stderr)
sys.exit(0 if blue > 15 or txt > 40 else 1)
PY
}

cleanup() {
  printf 'quit\n' | socat -t 5 - UNIX-CONNECT:"$MON_SOCK" >/dev/null 2>&1 || true
  sleep 1
  local loop
  loop=$(sudo losetup -fP --show "$RAW" 2>/dev/null || true)
  if [ -n "$loop" ]; then
    sudo mount -o subvol=@ "${loop}p2" /mnt 2>/dev/null || true
    sudo cp -a /mnt/var/lib/cosmos/lock/ "$W/" 2>/dev/null || true
    sudo umount /mnt 2>/dev/null || true
    sudo losetup -d "$loop" 2>/dev/null || true
    sudo chown -R "$USER" "$W" 2>/dev/null || true
  fi
}
trap cleanup EXIT

echo "== lock drive =="

# 1. greetd -> cage -> cosmos-greeter card.
i=0; until grep -q 'greetd' "$SERIAL" 2>/dev/null || [ $i -ge 90 ]; do sleep 1; i=$((i+1)); done
sleep 12   # cage + greeter map
dump 01-greeter
card_px "$W/01-greeter.png" && echo "PASS: greeter card rendered" || echo "FAIL: no greeter card"

# 2. wrong password -> error line; right password -> session.
# The password field sits ~(512,465); click it before typing — after a
# failed submit the field can lose keyboard focus.
qmp click 512 465
sleep 0.5
typeno 'wrongpw'; gsubmit
sleep 2
dump 02-greeter-wrong
# greeter Enter now submits (b5a8cb1) — exercise it first, click as fallback.
qmp click 512 465
sleep 0.5
type 'cosmos'          # sends pw chars + Enter — tests the Enter-submit path
if ! wait_marker '==LOCK-READY==' 60; then
  echo "!! Enter-submit did not reach session — falling back to Sign-in click"
  qmp click 512 465; sleep 0.5
  for i in $(seq 1 16); do qkey backspace; sleep 0.05; done  # clear any leftover pw text
  typeno 'cosmos'; gsubmit
  wait_marker '==LOCK-READY==' 120 || echo "FAIL: session never came up"
fi
sleep 8   # panel + shell settle
dump 03-desktop
card_px "$W/03-desktop.png" && echo "FAIL: still on greeter" || echo "PASS: greeter -> desktop"

# 3. super+L -> lock card; wrong pw -> error; right pw -> desktop.
qkey meta_l-l
sleep 3
dump 04-locked
card_px "$W/04-locked.png" && echo "PASS: lock card rendered" || echo "FAIL: no lock card"
typeno 'nope'; lsubmit
sleep 2
dump 05-lock-wrong
typeno 'cosmos'; lsubmit
sleep 3
dump 06-unlocked
card_px "$W/06-unlocked.png" && echo "FAIL: still locked after right password" || echo "PASS: unlock -> desktop"

# 4. Control Centre -> Lock button (footer middle third).
qmp click 866 15         # tray -> quick settings
sleep 2
dump 07-quick
qmp click 866 290        # Lock button
sleep 3
dump 08-cclock
card_px "$W/08-cclock.png" && echo "PASS: Control Centre Lock locked" || echo "FAIL: CC Lock did nothing"
typeno 'cosmos'; lsubmit
sleep 3

# 5. sweep x3 — lock/unlock cycles, journal panic check is guest-side.
for n in 1 2 3; do
  qkey meta_l-l; sleep 2.5
  typeno 'cosmos'; lsubmit; sleep 2.5
  echo "sweep $n done"
done
dump 09-sweep-end

wait_marker '==AGENTD-DONE==' 240 || echo "!! agentd stage late"
wait_marker '==LOCK-END==' 360 || echo "!! LOCK-END never arrived"
grep -E 'PASS:|FAIL:|==LOCK|==AGENTD|panic count' "$SERIAL" | tail -40
p=$(grep -c 'PASS:' "$SERIAL" || true); f=$(grep -c 'FAIL:' "$SERIAL" || true)
echo "== LOCK VERDICT: $p PASS / $f FAIL =="

# Session-end forensics: was a greetd respawn preceded by a real session end
# (compositor/session exit lines) or did it steal vt1 over a live session?
echo "== RESPAWN-FORENSICS =="
grep -an 'check_children\|greetd.*restart\|restart counter\|session.*end\|compositor.*exit\|cosmos-session.*exit\|Stopped session\|Deactivated.*session\|scope.*Deactivat' "$SERIAL" | tail -30
