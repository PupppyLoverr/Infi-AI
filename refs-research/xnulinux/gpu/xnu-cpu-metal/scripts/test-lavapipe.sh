#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-or-later
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VK_HDR="$ROOT/build/vk-deps/vulkan-headers/1.4.363.0/include"
VK_LIB="$ROOT/build/vk-deps/vulkan-loader/1.4.363.0/lib"

# Fix @@HOMEBREW_PREFIX@@ install_name on our local copy of libvulkan.1.dylib if needed
if otool -D "$VK_LIB/libvulkan.1.dylib" | grep -q '@@HOMEBREW_PREFIX@@'; then
  chmod u+w "$VK_LIB/libvulkan.1.4.363.dylib" 2>/dev/null || true
  install_name_tool -id "@rpath/libvulkan.1.dylib" "$VK_LIB/libvulkan.1.4.363.dylib"
  codesign -s - --force "$VK_LIB/libvulkan.1.4.363.dylib" 2>/dev/null || true
fi

"$ROOT/build/vk-deps/bin/glslangValidator" -V "$ROOT/test/vecadd.comp" -o "$ROOT/build/vecadd.spv"

/usr/bin/clang -O2 -I"$VK_HDR" "$ROOT/test/test_lavapipe_compute.c" \
  -L"$VK_LIB" -lvulkan -Wl,-rpath,"$VK_LIB" \
  -o "$ROOT/build/test_lavapipe_compute"

ICD_JSON="$(ls "$ROOT/dist/lavapipe/share/vulkan/icd.d/"lvp_icd*.json | head -n 1)"
echo "Using ICD JSON: $ICD_JSON"
cat "$ICD_JSON"
echo ""

VK_ICD_FILENAMES="$ICD_JSON" "$ROOT/build/test_lavapipe_compute" "$ROOT/build/vecadd.spv"
