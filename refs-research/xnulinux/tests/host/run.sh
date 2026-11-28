#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# In the VM, with a hosted system running: checks that the hosted kernel leaves Linux alone. 1 min.
#   fpcheck: a task on the hosted kernel's first CPU (0, or the first of fki's cpu_list) keeps its FP/SIMD registers across
#            being switched out
#   memcheck: 4 GB of Linux memory keep their contents (nothing writes over Linux's pages)
set -e
cd "$(dirname "$0")"
O=${TMPDIR:-/tmp}
cc -O2 -o $O/fpcheck fpcheck.c; cc -O2 -o $O/memcheck memcheck.c	# (not &&: set -e would pass over a failed first compile)
XCPU0=$( (tr -d '\n' 2>/dev/null < /sys/module/fki/parameters/cpu_list || true) | cut -d, -f1)	# as xnulinux-boot.sh has it
$O/fpcheck ${XCPU0:-0} 25 & fp_pid=$!
m=0; sudo $O/memcheck 4 25 || m=$?
f=0; wait "$fp_pid" || f=$?
[ $f = 0 ] && [ $m = 0 ] && echo "host tests: pass" || { echo "host tests: FAIL (fpcheck $f, memcheck $m)"; exit 1; }
