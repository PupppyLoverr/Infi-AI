#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# fki's host-call device on a real kernel: a small module (hctest.c) makes host calls the way xnu.ko makes them for the
# hosted kernel, a small service (hcserve.c) answers them on /dev/xnu-hostcall the way xnudrmd does, and goes wrong in one
# way per test; hcxlate.c checks the device's ioctls. What every call returned, and after how long, is printed.
# On an arm64 Linux host of the project: root (sudo), fki.ko loaded and built in ../.. (its Module.symvers), the
# kernel's headers, cc. No hosted system is needed, and one must not be running, nor xnudrmd (one service at a time).
#   run.sh              the tests that take seconds
#   run.sh --slow       also the two that wait for the 120 s a call may take (about 4.5 minutes)
#   run.sh NAME...      only these (the names are in the list below)
# Exit status 0: every call ended as it should; 1: not; 2: could not run.
set -u
D=$(cd "$(dirname "$0")" && pwd); P=$(cd "$D/../../.." && pwd); K=${KDIR:-/lib/modules/$(uname -r)/build}
S=sudo; [ "$(id -u)" = 0 ] && S=
[ -d /sys/module/fki ] || { echo "fki.ko is not loaded (sudo insmod fki-ko/fki.ko)" >&2; exit 2; }
[ ! -d /sys/module/xnu ] || { echo "a hosted system is running: not beside it" >&2; exit 2; }
[ -f "$P/fki-ko/Module.symvers" ] || { echo "no fki-ko/Module.symvers: build fki-ko first (make -C $K M=$P/fki-ko modules)" >&2; exit 2; }
if $S fuser -s /dev/xnu-hostcall 2>/dev/null; then echo "/dev/xnu-hostcall is open (xnudrmd?): stop its service first" >&2; exit 2; fi

# test: the service's mode and argument (- for no service), the module's test
quick="nosvc:-::nosvc echo:echo::echo smallbuf:smallbuf::one readfault:readfault::one writefault:writefault::one close:close::close
bigreq:echo::bigreq bigreply:bigreply:$(( (8 << 20) + 40 )):bigreply xlate"
slow="timeout:noreply:125:timeout slowwrite:slowwrite:125:slowwrite"
want=; all="$quick"
for a; do case "$a" in --slow) all="$quick $slow";; *) want="$want $a";; esac; done
[ -z "$want" ] || all=$(for w in $want; do for e in $quick $slow; do [ "${e%%:*}" = "$w" ] && echo "$e"; done; done)
[ -n "$all" ] || { echo "no such test (the names: $(for e in $quick $slow; do printf '%s ' "${e%%:*}"; done))" >&2; exit 2; }

W=$(mktemp -d) || exit 2
svc=
cleanup() { [ -z "$svc" ] || $S kill "$svc" 2>/dev/null; rm -rf "$W"; }
trap cleanup EXIT
cp "$D/hctest.c" "$D/Makefile" "$W/"; mkdir "$W/fki-ko"; cp "$P/fki-ko/fki.h" "$W/fki-ko/"
log=$(make -s -C "$K" M="$W" KBUILD_EXTRA_SYMBOLS="$P/fki-ko/Module.symvers" modules 2>&1) && [ -e "$W/hctest.ko" ] ||
  { echo "$log" | grep -iE "error|\*\*\*" | head -10 >&2; echo "the test module did not build" >&2; exit 2; }
cc -O2 -Wall -o "$W/hcserve" "$D/hcserve.c" -lpthread && cc -O2 -Wall -o "$W/hcxlate" "$D/hcxlate.c" -lpthread ||
  { echo "the service did not build" >&2; exit 2; }

status=0
for e in $all; do
  IFS=: read -r name mode arg t <<EOF
$e
EOF
  echo "== $name"
  if [ "$name" = xlate ]; then
    $S "$W/hcxlate"; r=$?
    [ $r = 0 ] || { [ $r = 1 ] || { echo "hcxlate could not run" >&2; exit 2; }; status=1; }
    continue
  fi
  svc=
  if [ "$mode" != - ]; then
    $S "$W/hcserve" "$mode" $arg > "$W/svc.out" 2>&1 &
    svc=$!
    for _ in $(seq 1 50); do grep -q '^ready$' "$W/svc.out" 2>/dev/null && break; sleep 0.1; done
    grep -q '^ready$' "$W/svc.out" || { cat "$W/svc.out" >&2; echo "the service did not start" >&2; exit 2; }
  fi
  mark="hctest: run $$ $name"
  echo "$mark" | $S tee /dev/kmsg > /dev/null
  $S insmod "$W/hctest.ko" t="$t" 2>/dev/null		# (its init reports and returns an error: it never stays loaded)
  [ -z "$svc" ] || { sleep 0.3; $S pkill -P "$svc" 2>/dev/null; $S kill "$svc" 2>/dev/null; wait "$svc" 2>/dev/null; svc=; }
  [ "$mode" = - ] || grep -v '^ready$' "$W/svc.out" | sed 's/^/  service: /'
  out=$($S dmesg | awk -v m="$mark" 'f && /hctest: / { sub(/^\[[^]]*\] */, ""); print } index($0, m) { f = 1 }')
  [ -n "$out" ] || { echo "the test module did not run (the kernel log has no line of it)" >&2; exit 2; }
  echo "$out" | sed 's/^hctest: /  /'
  echo "$out" | grep -q "^hctest: $t: ok\$" || status=1
done
[ $status = 0 ] && echo "all as it should be" || echo "NOT all as it should be"
exit $status
