#!/bin/sh
# island-check.sh — guest-side verifier for the dynamic island
# (menubar pill -> cosmos-island Top-layer card: CLIPBOARD history +
# STAGED FILES).
#
# Installed by inject-island-check.sh, run once at boot. The host driver
# (tests/drive/island.sh) sends QMP clicks/keys + takes screendumps;
# this script drives the clipboard via wl-copy/wl-paste (native
# wlr-data-control clients — same protocol path any third-party app
# uses) and asserts clipboard CONTENT in-guest, which is stronger than
# pixels. Island visual state (open/closed, history rows, staged-file
# section, geometry) is asserted host-side from the screendumps.
#
# Marker flow: guest `say`s a marker to ttyS0 -> host acts -> guest
# polls the real state (wl-paste) where it can, else just advances.

DIR=/var/lib/cosmos/island
OUT=$DIR/evidence.txt
mkdir -p "$DIR"
exec >"$OUT" 2>&1

say() {
  echo "$@"
  echo "$@" > /dev/ttyS0 2>/dev/null || true
}

wait_file() {
  i=0
  while [ $i -lt "$2" ]; do
    [ -e "$1" ] && return 0
    sleep 1; i=$((i+1))
  done
  return 1
}

# The compositor lets smithay pick the first free wayland-N socket, and a
# stale wayland-0 file can linger (wl-copy got ECONNREFUSED on it). Probe
# every candidate with a real unix connect; first live one wins.
WLSOCK=""
for _ in $(seq 1 120); do
  for s in /run/user/1000/wayland-*; do
    [ -S "$s" ] || continue
    if socat -t 2 - UNIX-CONNECT:"$s" </dev/null >/dev/null 2>&1; then
      WLSOCK="${s##*/}"; break
    fi
  done
  [ -n "$WLSOCK" ] && break
  sleep 1
done
say "wayland socket: $WLSOCK"

# Poll wl-paste until it returns WANT (or TIMEOUT seconds elapse).
wait_clip() { # wait_clip WANT TIMEOUT -> 0/1
  i=0
  while [ $i -lt "$2" ]; do
    timeout 10 su -l cosmos -c "WAYLAND_DISPLAY=$WLSOCK wl-paste" 2>/dev/null | grep -q "$1" && return 0
    sleep 1; i=$((i+1))
  done
  return 1
}

say "==ISLAND-CHECK=="
say "utc=$(date -u '+%Y-%m-%dT%H:%M:%SZ')"

if ! wait_file /run/user/1000/cosmos-ipc.sock 150; then
  say "FAIL: ipc sock never appeared"
  say "==IS-END=="; exec 1>&2 2>/dev/null; cat "$OUT" > /dev/ttyS0 2>/dev/null || true; exit 1
fi
for _ in $(seq 1 60); do pgrep -x cosmos-shell >/dev/null && break; sleep 1; done
sleep 8

for b in wl-copy wl-paste pcmanfm; do
  command -v "$b" >/dev/null && echo "  tool present: $b" || echo "  tool MISSING: $b"
done
command -v wl-copy >/dev/null || { say "FAIL: wl-clipboard missing"; }
command -v pcmanfm >/dev/null || say "WARN: pcmanfm missing — drag stage no-ops"

# Stage 1 — clipboard into history via wl-copy (external client path;
# foot in the task sheet is equivalent — same data-control wire path).
timeout 10 su -l cosmos -c "WAYLAND_DISPLAY=$WLSOCK wl-copy 'alpha-clip'" 2>&1
sleep 1
say "==IS-CLIP1=="

# Stage 2 — host clicks the pill to open the card.
say "==IS-CLICK-PILL=="

# Stage 3 — second clip while open: newest-first history.
timeout 10 su -l cosmos -c "WAYLAND_DISPLAY=$WLSOCK wl-copy 'beta-clip'" 2>&1
sleep 1
say "==IS-CLIP2-SENT=="

# Stage 4 — host clicks clip row 0 (newest = beta-clip); then wl-paste
# must return it — proves the click row -> live selection round-trip.
say "==IS-CLICK-ROW1=="
if wait_clip "alpha-clip" 45; then
  say "PASS: clip-row click restored live selection (wl-paste=alpha-clip)"
else
  say "FAIL: clip-row click did not set clipboard to alpha-clip"
fi

# Stage 5 — re-copy alpha-clip: history dedups (verified on the dump).
timeout 10 su -l cosmos -c "WAYLAND_DISPLAY=$WLSOCK wl-copy 'alpha-clip'" 2>&1
sleep 1
say "==IS-DEDUP=="

# Stage 6 — pcmanfm on XWayland :0 as the uri-list drag source. Seed a
# file in ~ so the file list has a row the host can grab (an empty home
# gives nothing to drag).
timeout 10 su -l cosmos -c 'echo island-drag-content > /home/cosmos/island-drag-test.txt'
if command -v pcmanfm >/dev/null; then
  timeout 10 su -l cosmos -c 'DISPLAY=:0 XDG_RUNTIME_DIR=/run/user/1000 pcmanfm --new-win /home/cosmos >/tmp/pcmanfm.log 2>&1 &'
fi
say "==IS-PCMANFM=="

# Stage 7 — host opens the card (pill click), drags a pcmanfm row onto
# it, then clicks the staged-file row; guest polls the clipboard for a
# real file path.
say "==IS-DRAG=="
if wait_clip "^/" 60; then
  say "PASS: staged-file click put a path on the clipboard"
  timeout 10 su -l cosmos -c "WAYLAND_DISPLAY=$WLSOCK wl-paste" 2>/dev/null | sed 's/^/  clipboard: /'
else
  say "FAIL: staged file never landed on the clipboard"
fi

# Stage 8 — Esc dismiss (host sends esc; pixel-verified host-side).
say "==IS-ESC=="
sleep 3

# Stage 9 — reopen + click-away dismiss (focus-leave close).
say "==IS-AWAY=="
sleep 3

# Stage 10 — 300ms re-click guard: host does away-close + immediate pill
# click; the card must NOT reopen (pixel-verified host-side).
say "==IS-GUARD=="
sleep 3

say "==IS-END=="
# QMP quit kills the guest without an orderly umount — sync first or the
# evidence file + any copied PNGs stay in btrfs dirty pages.
sync
sleep 2
exec 1>&2 2>/dev/null
cat "$OUT" > /dev/ttyS0 2>/dev/null || true
