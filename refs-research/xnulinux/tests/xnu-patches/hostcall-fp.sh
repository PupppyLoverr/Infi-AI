#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# What xnu-patches/include/linux64.h tells the compiler about a call into the host is what the host does: it keeps none
# of the FP/SIMD registers (Linux schedules during the call). hostcall_fp_test.c is built against the header alone, at
# three optimisation levels, and run as an ordinary program with a hook that returns with v0-v31 changed. arm64 machines
# only (it runs the header's own inline assembly); a few seconds.
#   tests/xnu-patches/hostcall-fp.sh [REPO]     CC names the compiler (cc)
set -eu
here=$(cd "$(dirname "$0")" && pwd)
repo=${1:-$(cd "$here/../.." && pwd)}
case "$(uname -m)" in
arm64|aarch64) ;;
*) echo "SKIP: this test runs arm64 code (machine is $(uname -m))"; exit 0 ;;
esac
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir "$tmp/inc"
ln -s "$repo/xnu-patches/include" "$tmp/inc/linux64"
rc=0
for opt in -O1 -O2 -Os; do
	"${CC:-cc}" $opt -Wall -Wextra -Werror -I"$tmp/inc" -o "$tmp/t" "$here/hostcall_fp_test.c"
	echo "== ${CC:-cc} $opt"
	"$tmp/t" || rc=1
done
exit $rc
