#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# fki's interface on a real kernel: a small module (main.c, asm.S) calls what fki exports the way xnu.ko calls it, from
# inside fki_enter() where xnu.ko does, and holds every answer against fki.h and against the kernel's own; netpeer.c is
# Linux's end of the link xnu0 and fpload.c the process whose FP/SIMD registers a foreign entry must not touch.
# On an arm64 Linux host of the project: root (sudo), fki.ko built in ../.. and not in use (no hosted system: the test
# loads and unloads fki.ko itself, with the parameters each group of checks needs, and at the end loads it again with
# those it found), the kernel's headers, cc, ip, losetup.
#   run.sh            every group but the last
#   run.sh NAME...    only these: enter clock idle cpus mem fault fp file blob net netq netwake unload reload cpulist
#                     cpulist-bad reserve carve carve-hole
# carve-hole loads fki.ko with a carve-out where the machine has no memory at all. An fki.ko that does not refuse that
# one maps it, and the machine stops at the first access: ask for it by name, where that can be afforded.
# Exit status 0: all as fki.h says; 1: not; 2: could not run.
set -u
D=$(cd "$(dirname "$0")" && pwd); P=$(cd "$D/../../.." && pwd); K=${KDIR:-/lib/modules/$(uname -r)/build}
S=sudo; [ "$(id -u)" = 0 ] && S=
all="enter clock idle cpus mem fault fp file blob net netq netwake unload reload cpulist cpulist-bad reserve carve"
[ $# = 0 ] || all="$*"
[ ! -d /sys/module/xnu ] || { echo "a hosted system is running: not beside it" >&2; exit 2; }
[ -f "$P/fki-ko/fki.ko" ] && [ -f "$P/fki-ko/Module.symvers" ] || { echo "no fki-ko/fki.ko: build fki-ko first (make -C $K M=$P/fki-ko modules)" >&2; exit 2; }
was=no
if [ -d /sys/module/fki ]; then		# as it was loaded: put back at the end
  was=
  for f in /sys/module/fki/parameters/*; do v=$(tr -d '\n' < "$f"); [ -z "$v" ] || [ "$v" = 0 ] || was="$was ${f##*/}=$v"; done
fi

W=$(mktemp -d) || exit 2
loop=; peer=
cleanup() {
  [ -z "$peer" ] || $S kill "$peer" 2>/dev/null
  [ -z "$loop" ] || $S losetup -d "$loop" 2>/dev/null
  $S rm -f /lib/firmware/fki-ifacetest.bin
  $S rmmod fki 2>/dev/null
  [ "$was" = no ] || $S insmod "$P/fki-ko/fki.ko" $was
  rm -rf "$W"
}
trap cleanup EXIT
cp "$D/main.c" "$D/asm.S" "$D/Makefile" "$W/"; mkdir "$W/fki-ko"; cp "$P/fki-ko/fki.h" "$W/fki-ko/"
log=$(make -s -C "$K" M="$W" KBUILD_EXTRA_SYMBOLS="$P/fki-ko/Module.symvers" modules 2>&1) && [ -e "$W/ifacetest.ko" ] ||
  { echo "$log" | grep -iE "error|\*\*\*" | head -10 >&2; echo "the test module did not build" >&2; exit 2; }
cc -O2 -Wall -o "$W/netpeer" "$D/netpeer.c" && cc -O2 -Wall -o "$W/fpload" "$D/fpload.c" || { echo "the helpers did not build" >&2; exit 2; }

status=0; cur=none
fail() { [ $status = 2 ] || status=1; }
since() { $S dmesg | awk -v m="$mark" 'f { sub(/^\[[^]]*\] */, ""); print } index($0, m) { f = 1 }'; }	# the kernel log after the mark
mark() { mark="ifacetest: run $$ $RANDOM $*"; echo "$mark" | $S tee /dev/kmsg > /dev/null; }
load() {	# fki.ko with these parameters (as it is, when it is loaded with them already)
  [ "$cur" = "$*" ] && return
  $S rmmod fki 2>/dev/null
  [ ! -d /sys/module/fki ] || { echo "fki.ko is in use: cannot load it again" >&2; exit 2; }
  $S insmod "$P/fki-ko/fki.ko" "$@" || { echo "fki.ko did not load ($*)" >&2; exit 2; }
  cur="$*"
}
mod() {		# the test module once: mod TEST [parameter=value...]; prints its lines
  mark "$1"
  $S insmod "$W/ifacetest.ko" t="$@" 2>/dev/null		# (its init reports and returns an error: it never stays loaded)
  out=$(since | grep '^ifacetest: ')
  [ -n "$out" ] || { echo "the test module did not run (the kernel log has no line of it)" >&2; exit 2; }
  echo "$out" | sed 's/^ifacetest: /  /'
  echo "$out" | grep -q "^ifacetest: $1: ok\$" || fail
}
line() { if [ "$1" = 0 ]; then echo "  ok: $2"; else echo "  FAIL: $2"; fail; fi; }
up() { $S sysctl -qw net.ipv6.conf.xnu0.disable_ipv6=1; $S ip link set xnu0 up || { echo "xnu0 did not come up" >&2; exit 2; }; }	# (no frames of Linux's own on it)
cma() { awk '/^CmaFree:/ { print $2 }' /proc/meminfo; }
# (CmaFree is printed beside the checks and not judged: right after an unload it reads 6 to 10 MiB low for a while and
# is back later, with upstream's fki.ko as with any other. What tells a reservation that was not given back is the next
# load: with half of the CMA area reserved each time, the third could not get its own and fki.ko says "NOT reserved".)
online=$(for c in /sys/devices/system/cpu/cpu[0-9]*; do [ ! -e "$c/online" ] || [ "$(cat "$c/online")" = 1 ] && echo "${c##*cpu}"; done | sort -n | tr '\n' ' ')
last=$(echo $online | awk '{ print $NF }'); prev=$(echo $online | awk '{ if (NF > 1) print $(NF - 1) }')

for t in $all; do
  echo "== $t"
  case $t in
  enter|clock|idle|cpus|mem)
    load; mod $t;;
  fault)
    load; mod fault
    if since | grep -q 'no die() probe'; then echo "  skipped: an exception that is not an abort (this kernel gave fki no probe of die())"; else mod undef; fi;;
  fp)
    load; mark fp
    $S "$W/fpload" "$W/ifacetest.ko" | sed 's/^/  /'; r=${PIPESTATUS[0]}
    since | grep '^ifacetest: ' | sed 's/^ifacetest: /  (the module: /; s/$/)/'
    [ "$r" = 0 ] || { fail; [ "$r" = 1 ] || status=2; };;
  file)
    load
    head -c 1048581 /dev/urandom > "$W/file"; truncate -s 8M "$W/disk"
    loop=$($S losetup --find --show "$W/disk" 2>/dev/null) || loop=
    [ -n "$loop" ] || echo "  skipped: a block device's size (no loop device to be had)"
    mod file path="$W/file" n=1048581 ${loop:+dev=$loop devn=8388608}
    ! since | grep -q 'WARNING:'; line $? "the kernel log of those calls: $(since | grep -c 'WARNING:') warning(s)$(since | grep -m1 'WARNING:' | sed 's/^/ (/; s/$/)/')"
    [ -z "$loop" ] || $S losetup -d "$loop"; loop=;;
  blob)
    load
    head -c 300001 /dev/urandom | $S tee /lib/firmware/fki-ifacetest.bin > /dev/null
    mod blob blob=fki-ifacetest.bin n=300001
    $S rm -f /lib/firmware/fki-ifacetest.bin;;
  net)		# 8 frames from Linux's raw socket to the foreign side, 12 back
    cur=none; load; up
    $S "$W/netpeer" xnu0 send 8 | sed 's/^/  Linux: /'
    $S "$W/netpeer" xnu0 recv 12 10 > "$W/peer.out" 2>&1 &
    peer=$!
    for _ in $(seq 1 50); do grep -q '^listening$' "$W/peer.out" 2>/dev/null && break; sleep 0.1; done
    mod net pending=8 send=12
    wait "$peer" || fail; peer=
    grep -v '^listening$' "$W/peer.out" | sed 's/^/  Linux: /';;
  netq)		# more frames than the link's queue holds (1024)
    cur=none; load; up
    $S "$W/netpeer" xnu0 send 1100 | sed 's/^/  Linux: /'
    mod net pending=1024 send=0
    d=$(cat /sys/class/net/xnu0/statistics/tx_dropped); p=$(cat /sys/class/net/xnu0/statistics/tx_packets)
    [ "$d" = 76 ] && [ "$p" = 1024 ]; line $? "xnu0's counters after 1100 frames nobody took: $p sent, $d dropped (the queue holds 1024)";;
  netwake)
    load; up; mod netwake;;
  unload)	# frames still in the link's queue when fki.ko is unloaded: freed (no longer charged to the socket that sent them)
    cur=none; load; up
    $S "$W/netpeer" xnu0 hold 500 15 > "$W/peer.out" 2>&1 &
    peer=$!
    for _ in $(seq 1 50); do grep -q '^holding' "$W/peer.out" 2>/dev/null && break; sleep 0.1; done
    $S rmmod fki; r=$?; cur=none
    wait "$peer"; pr=$?; peer=
    sed 's/^/  Linux: /' "$W/peer.out"
    [ $r = 0 ] && [ $pr = 0 ]; line $? "fki.ko unloaded with 500 frames in the link's queue (rmmod: exit $r)";;
  reload)	# loaded and unloaded ten times, with a reservation where there is CMA: nothing is left behind
    $S rmmod fki 2>/dev/null; cur=none; mark reload
    t=$(awk '/^CmaTotal:/ { print $2 }' /proc/meminfo); r=; [ "${t:-0}" -ge 32768 ] 2>/dev/null && r=reserve_mb=$((t / 2048))	# half of it
    c0=$(cma); bad=0
    for _ in 1 2 3 4 5 6 7 8 9 10; do $S insmod "$P/fki-ko/fki.ko" $r && $S rmmod fki || bad=1; done
    c2=$(cma); got=$(since | grep -c "^fki: .* DRAM reserved\$"); not=$(since | grep -c '^fki: .* NOT reserved')
    [ $bad = 0 ] && { [ -z "$r" ] || [ "$got" = 10 ]; } && [ "$not" = 0 ] && ! since | grep -qiE 'WARNING|BUG|Oops|Call trace'
    line $? "fki.ko ${r:+with $r }loaded and unloaded 10 times${r:+: $got of the 10 loads got their reservation} (CmaFree $c0 kB before, $c2 kB after), the kernel log $(since | grep -ciE 'WARNING|BUG|Oops|Call trace') warnings";;
  cpulist)	# the foreign kernel's CPUs are not Linux's first ones
    [ -n "$prev" ] && [ "$prev" != 0 ] || { echo "  skipped: needs two CPUs online besides CPU 0"; continue; }
    load cpu_list=$prev,$last
    mod cpus cpus=$prev,$last; mod idle cpus=$prev,$last; up; mod netwake cpus=$prev,$last;;
  cpulist-bad)	# a CPU that is not there, and one named twice
    none=$(( $(cat /sys/devices/system/cpu/kernel_max) + 1 ))
    load cpu_list=$last,$none,$last
    mod cpus cpus=$last,$none,$last;;
  reserve)
    [ "$(awk '/^CmaTotal:/ { print $2 }' /proc/meminfo)" -ge 32768 ] 2>/dev/null || { echo "  skipped: this kernel has no CMA area of 32 MiB or more (cma=)"; continue; }
    $S rmmod fki 2>/dev/null; cur=none; c0=$(cma); mark reserve
    load reserve_mb=16; c1=$(cma)
    since | grep '^fki: .*reserved' | sed 's/^/  /'
    since | grep -q "^fki: 16 MiB for the foreign kernel's DRAM reserved\$"; said=$?
    mod mem resv=16		# (it asks for the 16 MiB and must get them at the reservation's address)
    $S rmmod fki; cur=none; c2=$(cma)
    line $said "reserve_mb=16: fki.ko says it has them (CmaFree $c0 kB before the load, $c1 kB with fki.ko loaded, $c2 kB after its unload)"
    mark reserve; load reserve_mb=1048576
    since | grep '^fki: .*reserved' | sed 's/^/  /'
    since | grep -q 'NOT reserved'; line $? "reserve_mb=1048576 (more than there is): fki.ko loads and says so";;
  carve)	# a carve-out that is Linux's own memory: refused, not mapped
    ram=$($S awk -F'[- ]' '/System RAM/ { print $1; exit }' /proc/iomem)
    base=$(printf '%#x' $(( (0x$ram + 0x4000000) & ~0x1fffff )))
    mark carve; load carve_base=$base carve_mb=64
    mod carve want=-16
    since | grep '^fki: carve-out' | cut -c1-150 | sed 's/^/  /';;
  carve-hole)	# a carve-out that is no memory at all: refused by the firmware's map, not mapped
    [ -d /sys/firmware/efi ] || { echo "  skipped: this kernel has no firmware memory map to ask (not an EFI boot)"; continue; }
    ! grep -qw 'mem=[^ ]*' /proc/cmdline || { echo "  skipped: this kernel was booted with mem=: memory may lie above what Linux uses"; continue; }
    top=$($S awk -F'[- ]' '/System RAM/ { e = $2 } END { print e }' /proc/iomem)
    base=$(printf '%#x' $(( ((0x$top >> 30) + 5) << 30 )))		# 4 to 5 GiB past the end of the machine's memory
    mark carve-hole; load carve_base=$base carve_mb=64
    mod carve want=-22
    since | grep '^fki: carve-out' | cut -c1-170 | sed 's/^/  /';;
  *) echo "no such group: $t" >&2; status=2;;
  esac
done
[ $status = 0 ] && echo "all as fki.h says" || echo "NOT all as fki.h says"
exit $status
