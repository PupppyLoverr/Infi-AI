#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-or-later
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VK_HDR_DIR="$ROOT/build/vk-deps/vulkan-headers/1.4.363.0"
BUNDLE_SYSCONFDIR="/Library/GPUBundles/XnuCpuMetal.bundle/Contents/Resources"

export PATH="$ROOT/build/pyvenv/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin"

build_arch() {
  local ARCH="$1"
  local BUILD_DIR="$ROOT/build/vkloader-${ARCH}"
  local PREFIX="$ROOT/build/vkloader-${ARCH}-install"
  rm -rf "$BUILD_DIR"
  cmake -G Ninja -S "$ROOT/src/Vulkan-Loader" -B "$BUILD_DIR" \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_C_COMPILER=/usr/bin/clang \
    -DCMAKE_CXX_COMPILER=/usr/bin/clang++ \
    -DCMAKE_OSX_ARCHITECTURES="$ARCH" \
    -DCMAKE_INSTALL_PREFIX="$PREFIX" \
    -DSYSCONFDIR="$BUNDLE_SYSCONFDIR" \
    -DVULKAN_HEADERS_INSTALL_DIR="$VK_HDR_DIR" \
    -DBUILD_TESTS=OFF
  ninja -C "$BUILD_DIR" -j"$(sysctl -n hw.ncpu)"
  ninja -C "$BUILD_DIR" install
  install_name_tool -id "@rpath/libvulkan.1.dylib" "$PREFIX/lib/libvulkan.1.dylib"
  codesign -s - --force "$PREFIX/lib/libvulkan.1.dylib" 2>/dev/null || true
  file "$PREFIX/lib/libvulkan.1.dylib"
  otool -L "$PREFIX/lib/libvulkan.1.dylib"
}

build_arch arm64
build_arch arm64e

# the arm64 loader where scripts/build-m2v.sh takes it from (it goes beside lavapipe's driver)
mkdir -p "$ROOT/build/vk-deps/vulkan-loader/1.4.363.0/lib"
cp -f "$ROOT/build/vkloader-arm64-install/lib/libvulkan.1.4.363.dylib" "$ROOT/build/vk-deps/vulkan-loader/1.4.363.0/lib/"
