#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-or-later
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SDK="$(xcrun --show-sdk-path)"
ARCH="${1:-arm64}"

mkdir -p "$ROOT/build"
if [ ! -f "$ROOT/build/vecadd.metallib" ]; then
  xcrun -sdk macosx metal -c "$ROOT/test/vecadd.metal" -o "$ROOT/build/vecadd.air"
  xcrun -sdk macosx metallib "$ROOT/build/vecadd.air" -o "$ROOT/build/vecadd.metallib"
fi

/usr/bin/clang++ -arch "$ARCH" -O2 -fobjc-arc -isysroot "$SDK" \
  "$ROOT/test/test_bundle.mm" \
  -framework Foundation -framework Metal \
  -o "$ROOT/build/test_bundle_${ARCH}"

file "$ROOT/build/test_bundle_${ARCH}"
"$ROOT/build/test_bundle_${ARCH}" "$ROOT/dist/XnuCpuMetal.bundle" "$ROOT/build/vecadd.metallib"
