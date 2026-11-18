#!/usr/bin/env bash
# island.sh — drive the dynamic-island verification end to end.
# Sparse-copy -> inject -> OVMF headless boot -> QMP inputs on guest
# serial markers -> PIL pixel asserts + screendump manifest -> verdict.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
W="$ROOT/work/drive-island"
RAW="$W/test.raw"
mkdir -p "$W"
QMP_SOCK="$W/qmp-$$.sock"; MON_SOCK="$W/monitor-$$.sock"; SERIAL="$W/serial.log"
export QMP_SOCK MON_SOCK

SRC="${COSMOS_IMG:-dist/cosmosos-x86_64.raw}"
cp --sparse=always "$SRC" "$RAW"
rm -f "$W"/evidence.txt "$W"/*.png "$W"/*.ppm "$SERIAL"
./tests/drive/inject-island-check.sh "$RAW"

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

# Fail fast if QEMU never starts (stale QEMU holding the image write-lock
# would otherwise ghost the whole run).
for i in $(seq 1 15); do [ -S "$QMP_SOCK" ] && break; sleep 1; done
[ -S "$QMP_SOCK" ] || { echo "FATAL: qemu qmp socket never appeared"; exit 1; }

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

# island card region check: centred card is x 322..702, y 36..~200.
# Open card paints a dark rounded rect there; closed = wallpaper.
island_open_px() {
  /usr/bin/python3 - "$1" <<'PY'
from PIL import Image
import sys
im = Image.open(sys.argv[1]).convert("RGB")
# sample a strip across the card body (below the header row)
px = [im.getpixel((x, 120)) for x in range(400, 620, 20)]
dark = sum(1 for p in px if p[0] < 60 and p[1] < 60 and p[2] < 70)
sys.exit(0 if dark >= 8 else 1)
PY
}

cleanup() {
  printf 'quit\n' | socat -t 5 - UNIX-CONNECT:"$MON_SOCK" >/dev/null 2>&1 || true
  sleep 1
  local loop
  loop=$(sudo losetup -fP --show "$RAW" 2>/dev/null || true)
  if [ -n "$loop" ]; then
    sudo mount -o subvol=@ "${loop}p2" /mnt 2>/dev/null || true
    sudo cp -a /mnt/var/lib/cosmos/island/ "$W/" 2>/dev/null || true
    sudo umount /mnt 2>/dev/null || true
    sudo losetup -d "$loop" 2>/dev/null || true
    sudo chown -R "$USER" "$W" 2>/dev/null || true
  fi
}
trap cleanup EXIT

echo "== island drive =="

# greetd login first — the checker only wl-copies once the session is up.
i=0; until grep -q 'greetd' "$SERIAL" 2>/dev/null || [ $i -ge 90 ]; do sleep 1; i=$((i+1)); done
sleep 12
qmp click 512 465        # greeter password field
sleep 0.5
qkey type cosmos
qkey ret               # greeter Enter submits (b5a8cb1)
sleep 2

echo "== island drive: waiting for session + first clip =="
wait_marker '==IS-CLIP1==' 300 || { echo "FATAL: never ready"; exit 1; }
dump 01-pill-snippet     # pill should show 'alpha-clip' snippet

wait_marker '==IS-CLICK-PILL==' 30 || echo "!! marker late"
qmp click 512 16         # pill centre at 1024x768
sleep 2
dump 02-island-open
OPEN_OK=0; island_open_px "$W/02-island-open.png" && OPEN_OK=1
[ $OPEN_OK = 1 ] && echo "PASS: island card opened on pill click" || echo "FAIL: no card in dump"

wait_marker '==IS-CLIP2-SENT==' 60 || echo "!! clip2 marker late"
sleep 1
dump 03-island-history   # two rows, beta-clip newest-first

wait_marker '==IS-CLICK-ROW1==' 60 || echo "!! row0 marker late" || true
qmp click 512 147        # clip row 1 (oldest=alpha): row0 y113 + ROW_H 34
sleep 2
dump 04-row0-clicked

wait_marker '==IS-DEDUP==' 90 || echo "!! dedup marker late"
sleep 1
dump 05-island-dedup

wait_marker '==IS-PCMANFM==' 60 || echo "!! pcmanfm marker late"
sleep 6                  # pcmanfm window mapping via satellite
dump 06-pcmanfm

# Stage 7 — drag the seeded file row out of pcmanfm's right pane onto the
# pill, then open the card and click the staged-file row. The checker
# polls wl-paste for a path.
wait_marker '==IS-DRAG==' 90 || echo "!! drag marker late"
# pcmanfm lists /home/cosmos; island-drag-test.txt is the first file row
# of the right pane (~x500,y188 at 1024x768). Drop target: pill centre.
qmp drag 500 188 512 16
sleep 2
dump 07-post-drag
# open the card if it isn't already (pill click toggles)
island_open_px "$W/07-post-drag.png" || { qmp click 512 16; sleep 1.5; }
# staged-file row0: card-top ~36 + local file_row_y(0) 150 => ~y203
qmp click 512 203
sleep 1
dump 08-file-row-click

wait_marker '==IS-ESC==' 90 || echo "!! esc marker late"
island_open_px "$W/08-file-row-click.png" || { qmp click 512 16; sleep 1.5; }
qkey esc
sleep 2
dump 09-after-esc
island_open_px "$W/09-after-esc.png" && echo "FAIL: card still open after Esc" || echo "PASS: Esc dismissed card"

wait_marker '==IS-AWAY==' 60 || echo "!! away marker late"
island_open_px "$W/09-after-esc.png" || qmp click 512 16   # open only if Esc closed it
sleep 1.5
dump 10-reopen
island_open_px "$W/10-reopen.png" || { qmp click 512 16; sleep 1.5; }
# click-away = click on a real client window: it takes keyboard focus,
# the island's Exclusive-keyboard surface gets a leave -> dismiss.
# (600,300) is pcmanfm's body — below the card, inside its list pane.
qmp click 600 300
sleep 2
dump 11-clickaway
island_open_px "$W/11-clickaway.png" && echo "FAIL: card open after click-away" || echo "PASS: click-away dismissed card"

wait_marker '==IS-GUARD==' 60 || echo "!! guard marker late"
# within ~300ms of the leave-dismiss, a pill re-click must not reopen
qmp click 512 16
sleep 1.5
qmp click 600 300        # leave-dismiss via real client focus
qmp click 512 16         # this click lands <300ms after dismissal
sleep 1
dump 12-guard
island_open_px "$W/12-guard.png" && echo "FAIL: guard-window re-click reopened card" || echo "PASS: 300ms re-click guard held"
sleep 1.5
qmp click 512 16         # guard is not sticky — pill reopens normally
sleep 1.5
dump 13-guard-lifted
island_open_px "$W/13-guard-lifted.png" && echo "PASS: pill reopens after guard window" || echo "FAIL: pill reopen broken"

# Stage 11 — widget check: the checker seeded ~/.local/share/cosmos/widgets/
# uptime and asserts /usr/share/cosmos/skills; open the Start launcher and
# dump the WIDGETS strip (refresh_secs=5 — card should appear within ~10s).
wait_marker '==IS-WIDGET==' 60 || echo "!! widget marker late"
qkey esc               # close whatever is open (island was reopened above)
sleep 1
qkey meta_l            # Start launcher
sleep 10
dump 14-widgets
echo "14-widgets dumped — WIDGETS strip verified by eye in the PNG"

wait_marker '==IS-END==' 90 || echo "!! IS-END never arrived"
grep -E 'PASS:|FAIL:|==IS-' "$SERIAL" | tail -30
p=$(grep -c 'PASS:' "$SERIAL" || true); f=$(grep -c 'FAIL:' "$SERIAL" || true)
echo "== ISLAND VERDICT: $p PASS / $f FAIL =="
