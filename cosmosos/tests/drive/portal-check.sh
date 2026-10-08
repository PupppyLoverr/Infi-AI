#!/bin/sh
# portal-check.sh — guest-side verifier for the xdg-desktop-portal wiring.
#
# Installed into a test image by inject-portal-check.sh, run once at boot
# by portal-check.service. Asserts from inside the real session:
#
#   1. /run/user/1000/bus exists (dbus-user-session is alive)
#   2. cosmos.portal + the dbus activation .service are installed
#   3. busctl --user sees xdg-desktop-portal AND our impl object; a
#      Screenshot call on org.freedesktop.impl.portal.desktop.cosmos
#      returns response 0 and writes a real PNG under
#      $XDG_RUNTIME_DIR/cosmos-screenshots/ (RGBA, non-empty, real dims)
#
# Markers go to /dev/ttyS0 (serial-getty@ttyS0 masked by the injector);
# the evidence file lands on ttyS0 at the end and stays on disk under
# /var/lib/cosmos/portal/ for loop-mount harvest.

DIR=/var/lib/cosmos/portal
OUT=$DIR/evidence.txt
SOCK=/run/user/1000/cosmos-ipc.sock
mkdir -p "$DIR"
exec >"$OUT" 2>&1

say() {
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

say "==PORTAL-CHECK=="
say "utc=$(date -u '+%Y-%m-%dT%H:%M:%SZ')"

# 1 — session bus + portal stack -----------------------------------------
if ! wait_file /run/user/1000/bus 120; then
  say "FAIL: /run/user/1000/bus never appeared (dbus-user-session missing?)"
  say "==PT-END=="
  exec 1>&2 2>/dev/null; cat "$OUT" > /dev/ttyS0 2>/dev/null || true
  exit 1
fi
for _ in $(seq 1 60); do
  pgrep -x cosmos-shell >/dev/null 2>&1 && break
  sleep 1
done
sleep 5

say "env (systemd --user manager — what activated services inherit):"
su -l cosmos -c 'systemctl --user show-environment 2>/dev/null | grep -E "XDG_CURRENT_DESKTOP|DBUS|WAYLAND" | sed "s/^/  /"'
su -l cosmos -c 'systemctl --user show-environment 2>/dev/null | grep -q "XDG_CURRENT_DESKTOP=cosmos"' \
  && say "PASS: XDG_CURRENT_DESKTOP=cosmos in user manager env" \
  || say "FAIL: XDG_CURRENT_DESKTOP missing from user manager env"

say "files:"
for f in /usr/share/xdg-desktop-portal/portals/cosmos.portal \
         /usr/share/dbus-1/services/org.freedesktop.impl.portal.desktop.cosmos.service \
         /usr/lib/systemd/user/cosmos-portal.service \
         /usr/local/bin/cosmos-portal; do
  [ -e "$f" ] && echo "  PASS file: $f" || echo "  FAIL file missing: $f"
done
# Debian ships the frontend under libexec, not /usr/bin.
if [ -e /usr/libexec/xdg-desktop-portal ] || [ -e /usr/bin/xdg-desktop-portal ]; then
  echo "  PASS file: xdg-desktop-portal frontend present"
else
  echo "  FAIL file missing: xdg-desktop-portal frontend"
fi

say "==PT-INTROSPECT=="
su -l cosmos -c 'busctl --user introspect org.freedesktop.impl.portal.desktop.cosmos /org/freedesktop/portal/desktop 2>&1' | grep -E 'Screenshot|PickColor|NAME' | head -12
su -l cosmos -c 'busctl --user introspect org.freedesktop.impl.portal.desktop.cosmos /org/freedesktop/portal/desktop 2>&1' | grep -q 'org.freedesktop.impl.portal.Screenshot' \
  && say "PASS: impl portal exposes Screenshot" \
  || say "FAIL: impl portal has no Screenshot iface"

say "==PT-SCREENSHOT=="
rm -f /run/user/1000/cosmos-screenshots/screenshot-*.png 2>/dev/null || true
su -l cosmos -c 'gdbus call --session --timeout 20 \
  -d org.freedesktop.impl.portal.desktop.cosmos \
  -o /org/freedesktop/portal/desktop \
  -m org.freedesktop.impl.portal.Screenshot.Screenshot \
  /org/freedesktop/portal/desktop/request/cosmos/t0 app.cosmos "" {}' 2>&1

# cosmos-portal falls back to /tmp/cosmos-screenshots when its own env
# lacks XDG_RUNTIME_DIR — check both. Also snapshot the portal journal.
su -l cosmos -c 'systemctl --user show-environment | grep -c XDG_RUNTIME_DIR' | sed 's/^/  XDG_RUNTIME_DIR in portal env count: /'
su -l cosmos -c 'journalctl --user -u cosmos-portal.service --no-pager -n 20 2>/dev/null' | sed 's/^/  journal: /'

SHOT=$(ls -t /run/user/1000/cosmos-screenshots/screenshot-*.png /tmp/cosmos-screenshots/screenshot-*.png 2>/dev/null | head -1)
if [ -n "$SHOT" ]; then
  # copy into the harvest dir before the evidence block (PNG is binary;
  # report only size/magic to serial, copy for the host)
  cp "$SHOT" "$DIR/portal-shot.png"
  sz=$(stat -c %s "$DIR/portal-shot.png")
  head -c8 "$DIR/portal-shot.png" | od -An -tx1 | tr -d ' \n'
  echo "  size=$sz path=$SHOT"
  [ "$sz" -gt 4000 ] && say "PASS: portal Screenshot wrote ${sz}B PNG" \
                     || say "FAIL: portal shot suspiciously small (${sz}B)"
else
  say "FAIL: no PNG under /run/user/1000/cosmos-screenshots/"
fi

# flush everything to the disk image — the host drives QEMU quit, which
# kills the guest without an orderly umount; unsynced btrfs pages would
# leave evidence.txt truncated and portal-shot.png missing.
sync
sleep 2
say "==PT-END=="
exec 1>&2 2>/dev/null
cat "$OUT" > /dev/ttyS0 2>/dev/null || true
