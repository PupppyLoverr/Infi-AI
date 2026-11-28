#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# What the Makefile promises about the virglrenderer backend, on a copy of it with empty sources and a compiler that only
# writes down how it was called (so this runs with or without virglrenderer installed): gpu/xnu-drm/test/make.sh
set -euo pipefail
cd "$(dirname "$0")/.."; T=$(mktemp -d); trap 'rm -rf "$T"' EXIT
cp Makefile "$T/"; touch "$T/xnudrmd.c" "$T/xnudrmd_virgl.c" "$T/xnudrm_proto.h" "$T/xnudrmd.h"
printf '#!/bin/sh\nargs="$*"; while [ $# -gt 1 ]; do [ "$1" = -o ] && out=$2; shift; done; echo "$args" > "$out"; echo "$args" >> calls\n' > "$T/cc"; chmod +x "$T/cc"
mk() { make -s -C "$T" CC="$T/cc" "$@" 2>/dev/null; }	# (2>: pkg-config's complaint where VIRGL=1 is said and there is no virglrenderer)
calls() { wc -l < "$T/calls" | tr -d ' '; }
later() { sleep 1; }	# a make that compares whole seconds must see the next step as later
fail() { echo "FAIL $1: xnudrmd was built with: $(cat "$T/xnudrmd" 2>/dev/null)"; exit 1; }
mk VIRGL=0; ! grep -q xnudrmd_virgl.c "$T/xnudrmd" || fail "without virglrenderer: no backend"
later; mk VIRGL=1; grep -q -e '-DXNUDRM_VIRGL.* xnudrmd_virgl.c' "$T/xnudrmd" || fail "virglrenderer installed after a first build: xnudrmd is built again, with the backend"
n=$(calls); mk VIRGL=1; [ "$(calls)" = "$n" ] || fail "nothing changed: nothing is built"
later; mk VIRGL=0; ! grep -q xnudrmd_virgl.c "$T/xnudrmd" || fail "and gone again: built again, without it"
mk clean; mk VIRGL=1 CFLAGS=-O0; grep -q -e '-O0 .*-DXNUDRM_VIRGL.* xnudrmd_virgl.c' "$T/xnudrmd" || fail "CFLAGS on the command line keeps the backend's flags"
echo "xnu-drm Makefile: pass"
