#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-or-later
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# 1. Compile vecadd.metal -> vecadd.air -> vecadd.metallib via Xcode Metal toolchain
xcrun -sdk macosx metal -c "$ROOT/test/vecadd.metal" -o "$ROOT/build/vecadd.air"
xcrun -sdk macosx metallib "$ROOT/build/vecadd.air" -o "$ROOT/build/vecadd.metallib"

# 2. Build C test binary linked against libxnu_m2v.dylib
/usr/bin/clang -O2 -I"$ROOT/dist/m2v/include" "$ROOT/test/test_m2v_compute.c" \
  -L"$ROOT/dist/m2v/lib" -lxnu_m2v -Wl,-rpath,"$ROOT/dist/m2v/lib" \
  -o "$ROOT/build/test_m2v_compute"

# 3. Run Metal -> metal2vulkan -> lavapipe!
mkdir -p "$ROOT/build/m2v-cache"
"$ROOT/build/test_m2v_compute" \
  "$ROOT/dist/lavapipe/lib/libvulkan.1.dylib" \
  "$ROOT/dist/lavapipe/share/vulkan/icd.d/lvp_icd.aarch64.json" \
  "$ROOT/build/vecadd.metallib" \
  "$ROOT/build/m2v-cache"
