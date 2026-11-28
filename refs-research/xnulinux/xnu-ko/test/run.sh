#!/bin/bash
# SPDX-License-Identifier: APSL-2.0
# Build xnu.ko's loader (xnu_main.c, macho.c, adt.c) for user space with AddressSanitizer and
# UndefinedBehaviorSanitizer and run test.c, which loads images it makes itself and reads back what XNU is handed
# with reader.c (XNU's side of it, in a file of its own). Any host, gcc or clang (CC).
set -euo pipefail
D=$(cd "$(dirname "$0")" && pwd)
W=${W:-$D/build}; mkdir -p "$W"
# -w: the module's files are not written for this compiler's user space warnings; -ftrivial-auto-var-init=zero: what
# their compiled-out AArch64 statements would have set reads as 0 (test.c)
${CC:-cc} -O1 -g -fsanitize=address,undefined -fno-sanitize-recover=undefined -fno-sanitize=alignment -w \
	-ftrivial-auto-var-init=zero -I"$D" "$D/test.c" "$D/reader.c" -o "$W/kotest" > "$W/build.log" 2>&1 || { head -60 "$W/build.log"; exit 1; }
"$W/kotest"
