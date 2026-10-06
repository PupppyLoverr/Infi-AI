#!/usr/bin/env bash
# shot.sh NAME — monitor screendump into work/drive-<rev>/NAME.png.
# Retries until a window-body crop is non-flat (freshly mapped windows can
# render blank for a frame; any keypress wakes the renderer).
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MON="$ROOT/work/image/monitor.sock"
DIR="${DRIVE_DIR:-$ROOT/work/drive-latest}"
NAME="$1"
mkdir -p "$DIR"
for i in 1 2 3 4 5 6; do
  printf "screendump %s\n" "$DIR/$NAME.ppm" | socat -t 2 - UNIX-CONNECT:"$MON" >/dev/null 2>&1
  sleep 1
  [ -f "$DIR/$NAME.ppm" ] || continue
  dev=$(convert "$DIR/$NAME.ppm" -crop 360x140+400+430 +repage -format "%[fx:standard_deviation]" info: 2>/dev/null || echo 0)
  awk "BEGIN{exit !($dev>0.02)}" && break
done
convert "$DIR/$NAME.ppm" "$DIR/$NAME.png"
echo "$DIR/$NAME.png (dev=$dev)"
