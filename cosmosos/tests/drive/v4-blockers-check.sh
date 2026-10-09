#!/bin/sh
# v4-blockers-check.sh — guest-side verifier for v4 section A.
# Installed by inject-v4-blockers-check.sh, run once at boot. The host
# drives the password entry + screendumps over QMP on the ==MARKERS==.
#   A1 logind registers the greetd session as a wayland/user session and
#      `loginctl lock-session` reaches the compositor.
#   A2 wrong/right password (host), then the locker is killed while
#      locked: the compositor must keep the lock and a respawned locker
#      takes over; the host unlocks through it.
#   A5 Files' drawn row count equals its footer count and `ls`.
#   A3 zero panics in this boot's journal.
DIR=/var/lib/cosmos/v4
OUT=$DIR/evidence.txt
mkdir -p "$DIR"
exec >"$OUT" 2>&1

say() { echo "$@"; echo "$@" > /dev/ttyS0 2>/dev/null || true; }
XRD=/run/user/1000
IPC=$XRD/cosmos-ipc.sock
# Journal since cursor $CUR with ANSI colour escapes stripped: tracing's
# fmt layer colours field names, so `rows=0` is stored as
# `rows\e[0m\e[2m=\e[0m0` and a plain grep for `rows=` never matches.
jlog() {
  journalctl -b --after-cursor="$CUR" --no-pager -o cat 2>/dev/null | sed 's/\x1b\[[0-9;]*m//g'
}
# Wait until `pattern` appears in the journal after cursor $CUR (max $2 s).
await() {
  i=0
  while [ $i -lt "$2" ]; do
    jlog | grep -q "$1" && return 0
    sleep 1; i=$((i+1))
  done
  return 1
}
mark() { CUR=$(journalctl -b -n0 --show-cursor --no-pager | sed -n 's/^-- cursor: //p'); }

say "==V4-CHECK== utc=$(date -u +%FT%TZ)"
i=0
while [ ! -S "$IPC" ] && [ $i -lt 240 ]; do sleep 1; i=$((i+1)); done
[ -S "$IPC" ] || { say "FAIL: no cosmos session after 240s"; exit 0; }
sleep 5

# --- A1 ---------------------------------------------------------------
loginctl list-sessions --no-legend | sed 's/^/  sessions: /'
SID=""
for s in $(loginctl list-sessions --no-legend | awk '{print $1}'); do
  props=$(loginctl show-session "$s" -p Name -p Type -p Class -p Service -p State)
  echo "$props" | sed "s/^/  session $s: /"
  if echo "$props" | grep -qx 'Name=cosmos' && echo "$props" | grep -qx 'Type=wayland' \
     && echo "$props" | grep -qx 'Class=user'; then SID=$s; fi
done
if [ -n "$SID" ]; then say "PASS A1: logind session $SID Type=wayland Class=user (greetd)"
else say "FAIL A1: no wayland/user logind session for cosmos"; fi

# --- A2 (part 1: lock via logind, host types wrong then right) --------
mark
[ -n "$SID" ] && loginctl lock-session "$SID"
if await "cosmos: session locked" 15; then say "PASS A1: loginctl lock-session locked the session"
else say "FAIL A1: loginctl lock-session did not lock"; fi
say "==LOCKED=="          # host: shot v4-14, type wrong pw, shot v4-15, type right pw
if await "cosmos: session unlocked" 240; then say "PASS A2: correct password unlocked"
else say "FAIL A2: no unlock within 240s"; fi
journalctl -b --after-cursor="$CUR" --no-pager -o cat | grep -i "incorrect\|auth" | sed 's/^/  lock: /'

# --- A2 (part 2: kill the locker while locked) ------------------------
sleep 3; mark
loginctl lock-session "$SID"
await "cosmos: session locked" 15
sleep 3
OLD=$(pgrep -x cosmos-lock | head -1)
say "killing locker pid=$OLD while locked"
kill -9 "$OLD" 2>/dev/null
if await "locker died while locked" 10 && await "takeover=true" 15; then
  NEW=$(pgrep -x cosmos-lock | head -1)
  say "PASS A2: locker died, session stayed locked, respawned locker pid=$NEW took over"
else
  say "FAIL A2: no respawn/takeover after locker death"
fi
say "==RESPAWNED=="       # host: type right pw, shot v4-16
if await "cosmos: session unlocked" 240; then say "PASS A2: unlocked through the respawned locker"
else say "FAIL A2: respawned locker never unlocked"; fi

# --- A5 ---------------------------------------------------------------
mark
WL=$(ls $XRD | grep -m1 '^wayland-[0-9]*$')
EXPECT=$(ls -1 /home/cosmos | wc -l)
su -l cosmos -c "WAYLAND_DISPLAY=$WL XDG_RUNTIME_DIR=$XRD cosmos-files 2>&1 | logger -t cosmos-files &"
if await "files: listing" 20; then
  line=$(jlog | grep "files: listing" | tail -1)
  echo "  $line"
  rows=$(echo "$line" | sed -n 's/.*rows=\([0-9]*\).*/\1/p')
  footer=$(echo "$line" | sed -n 's/.*footer=\([0-9]*\).*/\1/p')
  if [ "$rows" = "$footer" ] && [ "$footer" = "$EXPECT" ]; then
    say "PASS A5: Files rows=$rows footer=$footer ls=$EXPECT"
  else
    say "FAIL A5: Files rows=$rows footer=$footer ls=$EXPECT"
  fi
else
  say "FAIL A5: no 'files: listing' line"
fi

# --- A3 ---------------------------------------------------------------
n=$(journalctl -b --no-pager | grep -ci panic)
if [ "$n" = 0 ]; then say "PASS A3: zero panics in journal"
else say "FAIL A3: $n panic lines"; journalctl -b --no-pager | grep -i panic | sed 's/^/  /'; fi
say "==V4-DONE=="
