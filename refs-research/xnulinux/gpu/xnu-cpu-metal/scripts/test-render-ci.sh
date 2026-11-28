#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-or-later
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SDK="$(xcrun --show-sdk-path)"
ARCH="${1:-arm64}"

xcrun -sdk macosx metal -c "$ROOT/test/tri.metal" -o "$ROOT/build/tri.air"
xcrun -sdk macosx metallib "$ROOT/build/tri.air" -o "$ROOT/build/tri.metallib"

/usr/bin/clang++ -arch "$ARCH" -O2 -fobjc-arc -isysroot "$SDK" \
  "$ROOT/test/test_render_ci.mm" \
  -framework Foundation -framework Metal -framework CoreImage -framework CoreGraphics \
  -o "$ROOT/build/test_render_ci_${ARCH}"

"$ROOT/build/test_render_ci_${ARCH}" "$ROOT/dist/XnuCpuMetal.bundle" "$ROOT/build/tri.metallib"
