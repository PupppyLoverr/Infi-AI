#!/bin/sh
# assist-tray-check.sh — guest-side checker for the assist-tray regression
# (bug fixed in #90: while the Snap Assist picker was up, a menubar-tray
# click was swallowed by the picker's fullscreen bbox — quick-settings
# never opened and the picker stayed up).
#
# Installed into a test image by inject-assist-tray-check.sh, run once at
# boot by assist-tray-check.service. The host driver (tests/drive/
# assist-tray.sh) sends the inputs via QMP/HMP on a schedule; this script
# watches the compositor IPC event stream and asserts the real state
# transitions:
#
#   1. snap_assist {"open":true}  — picker opened after super+Right snap
#   2. snap_assist {"open":false} — picker closed after the tray click
#   3. config {"appearance":...}  — a click inside the quick-settings card
#      (the dark/light pill) toggled a setting, which can only happen if
#      the flyout actually opened and took pointer focus
#
# Plus an IPC Screenshot PNG pulled right after the picker close for
# visual evidence of the open flyout.
#
# Stage markers go to /dev/ttyS0 live (serial-getty@ttyS0 is masked by the
# injector, so the line stays ours) so the host can drive inputs off them.
# The full evidence file also lands on ttyS0 at the end and stays on disk
# under /var/lib/cosmos/assist-tray/ for host loop-mount harvest.

DIR=/var/lib/cosmos/assist-tray
OUT=$DIR/evidence.txt
EVENTS=$DIR/events.log
SOCK=/run/user/1000/cosmos-ipc.sock
mkdir -p "$DIR"
exec >"$OUT" 2>&1

say() { # live marker: evidence file + serial at once
  echo "$@"
  echo "$@" > /dev/ttyS0 2>/dev/null || true
}

wait_file() { # wait_file PATH SECONDS
  i=0
  while [ $i -lt "$2" ]; do
    [ -e "$1" ] && return 0
    sleep 1; i=$((i+1))
  done
  return 1
}

wait_grep() { # wait_grep PATTERN FILE SECONDS
  i=0
  while [ $i -lt "$3" ]; do
    grep -q "$1" "$2" 2>/dev/null && return 0
    sleep 1; i=$((i+1))
  done
  return 1
}

say "==ASSIST-TRAY-CHECK=="
say "utc=$(date -u '+%Y-%m-%dT%H:%M:%SZ')"

# Session up = ipc socket + shell alive; give windows/compositor settle time.
if ! wait_file "$SOCK" 150; then
  say "FAIL: $SOCK never appeared"
  say "==AT-END=="
  exec 1>&2 2>/dev/null; cat "$OUT" > /dev/ttyS0 2>/dev/null || true
  exit 1
fi
for _ in $(seq 1 60); do
  pgrep -x cosmos-shell >/dev/null 2>&1 && break
  sleep 1
done
sleep 8

# Subscribe to the compositor event stream; keep stdin open so the socket
# stays alive for broadcasts. socat -t closes on 240s idle — the run is
# shorter than that once inputs start.
: > "$EVENTS"
{ printf '%s\n' '{"op":"subscribe"}'; sleep 240; } | \
  socat -t 240 - UNIX-CONNECT:"$SOCK" >> "$EVENTS" 2>/dev/null &
SUB_PID=$!

say "==AT-READY=="

# Window spawn positions vary per boot, so the host can't click-focus a
# window reliably — focus the newest toplevel ourselves via IPC once two
# windows exist, then tell the host to send the snap key.
ipc_send() { printf '%s\n' "$1" | socat -t 5 - UNIX-CONNECT:"$SOCK" >/dev/null 2>&1 || true; }
i=0; WINID=""
while [ $i -lt 60 ]; do
  LASTW=$(grep '"type":"windows"' "$EVENTS" | tail -1)
  NW=$(printf '%s' "$LASTW" | grep -o '"id":[0-9]*' | wc -l)
  if [ "$NW" -ge 2 ]; then
    WINID=$(printf '%s' "$LASTW" | grep -o '"id":[0-9]*' | tail -1 | cut -d: -f2)
    break
  fi
  sleep 1; i=$((i+1))
done
if [ -n "$WINID" ]; then
  ipc_send "{\"op\":\"focus_window\",\"id\":$WINID}"
  echo "-- focused newest window id=$WINID via ipc"
  say "==AT-FOCUSED=="
else
  say "FAIL: never saw 2 windows to focus"
fi

# Stage 1 — picker opens after the host's super+Right snap.
echo "-- waiting for snap_assist open:true"
if wait_grep '"type":"snap_assist"[^}]*"open":true' "$EVENTS" 60; then
  say "PASS: assist picker opened (snap_assist open:true)"
  grep '"type":"snap_assist"' "$EVENTS" | head -2
else
  say "FAIL: assist picker never opened in 60s"
fi

# Stage 2 — picker closes after the host's tray click.
echo "-- waiting for snap_assist open:false"
if wait_grep '"type":"snap_assist"[^}]*"open":false' "$EVENTS" 60; then
  say "PASS: picker closed on tray click (snap_assist open:false)"
else
  say "FAIL: picker still open 60s after tray click"
fi

# Visual evidence: compositor-side PNG right after the close — should show
# the quick-settings card in the top-right if the flyout opened. The PNG is
# written by the compositor process (user cosmos), so it must go to /tmp —
# $DIR is root-owned.
sleep 1
SHOT_TMP=/tmp/assist-tray-flyout.png
SHOT=$DIR/flyout.png
rm -f "$SHOT_TMP"
REPLY=$(printf '%s\n' "{\"op\":\"screenshot\",\"path\":\"$SHOT_TMP\"}" | \
  socat -t 8 - UNIX-CONNECT:"$SOCK" 2>/dev/null || true)
echo "-- screenshot reply: $REPLY"
if wait_file "$SHOT_TMP" 15 && [ -s "$SHOT_TMP" ]; then
  cp "$SHOT_TMP" "$SHOT"
  echo "PASS: ipc screenshot written $SHOT ($(stat -c%s "$SHOT") bytes)"
else
  echo "WARN: ipc screenshot missing/empty"
fi

# Tell the host to click inside the flyout now.
say "==AT-CLICK-PILL=="

# Stage 3 — a click inside the flyout (host clicks the dark/light pill)
# must produce a config event containing "appearance". Only counts config
# events AFTER the picker closed (subscribe also emits a baseline config).
echo "-- waiting for config event carrying appearance (flyout pill click)"
i=0; saw=""
while [ $i -lt 60 ]; do
  if awk '/"open":false/{f=1} f&&/"type":"config"/&&/"appearance"/{s=1;exit} END{exit (s?0:1)}' "$EVENTS" 2>/dev/null; then
    saw=1; break
  fi
  sleep 1; i=$((i+1))
done
if [ -n "$saw" ]; then
  say "PASS: quick-settings flyout functional — pill click produced config(appearance)"
  awk '/"open":false/{f=1} f&&/"type":"config"/' "$EVENTS" | head -2
else
  say "FAIL: no config(appearance) event — flyout likely never opened"
fi

echo "-- event stream tail:"
tail -8 "$EVENTS"
say "==AT-END=="
kill "$SUB_PID" 2>/dev/null || true
exec 1>&2 2>/dev/null
cat "$OUT" > /dev/ttyS0 2>/dev/null || true
