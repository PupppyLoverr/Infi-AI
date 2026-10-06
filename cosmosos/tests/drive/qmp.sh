#!/usr/bin/env bash
# qmp.sh — QEMU QMP input-send-event helper (works headless; HMP mouse_* is
# dead with -display none). Sockets live in cosmosos/work/image/.
#
#   qmp.sh move X Y                    abs pointer move (0..32767 over framebuffer)
#   qmp.sh px PX PY                    same but takes framebuffer pixels (1024x768)
#   qmp.sh btn left|right|middle down|up
#   qmp.sh click PX PY                 pixel-space move + down(0.4s)+up — real click
#   qmp.sh drag PX1 PY1 PX2 PY2        pixel-space press, move, release
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
QMP="$ROOT/work/image/qmp.sock"
CAPS='{"execute":"qmp_capabilities"}'

to_abs() { awk "BEGIN{printf \"%d\", ($1/$2)*32767}"; }
ax() { to_abs "$1" 1024; }
ay() { to_abs "$1" 768; }

send() {
  printf '%s\n{"execute":"input-send-event","arguments":{"events":%s}}\n' \
    "$CAPS" "$1" | socat -t 3 - UNIX-CONNECT:"$QMP" >/dev/null
}
move() { send "[{\"type\":\"abs\",\"data\":{\"axis\":\"x\",\"value\":$1}},{\"type\":\"abs\",\"data\":{\"axis\":\"y\",\"value\":$2}}]"; }
btn()  { local d=false; [ "$2" = down ] && d=true; send "[{\"type\":\"btn\",\"data\":{\"button\":\"$1\",\"down\":$d}}]"; }

case "${1:-}" in
  move)  move "$2" "$3" ;;
  px)    move "$(ax "$2")" "$(ay "$3")" ;;
  btn)   btn "$2" "$3" ;;
  click) move "$(ax "$2")" "$(ay "$3")"; sleep 0.15; btn left down; sleep 0.4; btn left up ;;
  drag)  move "$(ax "$2")" "$(ay "$3")"; sleep 0.15; btn left down; sleep 0.3; move "$(ax "$4")" "$(ay "$5")"; sleep 0.3; btn left up ;;
  *) echo "usage: qmp.sh move X Y | px PX PY | btn B down|up | click PX PY | drag P1 P2" >&2; exit 1 ;;
esac
