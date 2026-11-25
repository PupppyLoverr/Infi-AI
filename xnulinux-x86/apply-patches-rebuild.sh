#!/bin/bash
# x86_64 host: apply xnulinux's XNU patches to the k27 tree and rebuild.
# Mirrors scripts/vm-xnu-rebuild.sh; without the macOS kernelcache the
# collection-relink block skips itself, leaving the patched kernel image
# (kernel-k27.macho) — everything a mode=1/mode=2 boot test needs.
set -e -o pipefail
cd ~/w/dvm
export PATH=$PATH:/usr/lib/llvm-21/bin
[ -d ~/w/poc ] || git clone -q https://github.com/AFK-surf/xnulinux ~/w/poc
T=out/k27/xnu
P=~/w/poc/xnu-patches
FILES=$(cat $T/.linux64-patched 2>/dev/null; grep -h '^+++ b/' $P/*.patch | sed 's|^+++ b/||')
SNAP=$(mktemp -d)
for f in $FILES; do [ -e $T/$f ] && mkdir -p $SNAP/$(dirname $f) && cp -p $T/$f $SNAP/$f; done
for f in $FILES; do git -C $T checkout -q -- $f 2>/dev/null || rm -f $T/$f; done
for p in $P/*.patch; do git -C $T apply $p; echo "applied $(basename $p)"; done
for f in $FILES; do [ -e $SNAP/$f ] && cmp -s $SNAP/$f $T/$f && touch -r $SNAP/$f $T/$f; done
rm -rf $SNAP
grep -h '^+++ b/' $P/*.patch | sed 's|^+++ b/||' > $T/.linux64-patched
mkdir -p $T/EXTERNAL_HEADERS/linux64
for h in $P/include/*.h; do cmp -s $h $T/EXTERNAL_HEADERS/linux64/$(basename $h) || cp $h $T/EXTERNAL_HEADERS/linux64/; done
t=$SECONDS; JOBS=6 bash k27/build-kernel27.sh 2>&1 | tail -15; echo "kernel: $((SECONDS - t)) s"
ls -la out/k27/kernel-k27.macho
