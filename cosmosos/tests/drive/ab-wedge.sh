#!/bin/bash
# ab-wedge.sh — un-instrumented sweep A/B: rapid focus alternation between two
# terminals, screendump after each click, then measure body transparency.
# Usage: DRIVE_DIR=work/drive-AB ./tests/drive/ab-wedge.sh [label]
# Emits $DRIVE_DIR/ab-<label>-NN.png + ab-<label>.txt tally.
set -u
cd "$(dirname "$0")/../.."
D="${DRIVE_DIR:-work/drive-ab}"
LABEL="${1:-run}"
mkdir -p "$D"
MON="${MON_SOCK:-work/image/monitor.sock}"

click() { tests/drive/qmp.sh click "$1" "$2"; }

# spawn two terminals at known cascade spots, let them settle
printf 'sendkey meta_l-ret\n' | socat -t 2 - UNIX-CONNECT:$MON >/dev/null; sleep 2
printf 'sendkey meta_l-ret\n' | socat -t 2 - UNIX-CONNECT:$MON >/dev/null; sleep 3
DRIVE_DIR=$D tests/drive/shot.sh "ab-${LABEL}-00-spawn" >/dev/null

# 15 alternations: A titlebar -> B titlebar (top-left vs right window)
# window A ~ (300,57) tbar / body (300,400); window B ~ (790,57) / (790,300)
for i in $(seq 1 15); do
  if [ $((i % 2)) -eq 1 ]; then click 790 57; else click 300 57; fi
  sleep 1.2
  DRIVE_DIR=$D tests/drive/shot.sh "$(printf 'ab-%s-%02d' "$LABEL" "$i")" >/dev/null
done
echo "== $LABEL done =="
