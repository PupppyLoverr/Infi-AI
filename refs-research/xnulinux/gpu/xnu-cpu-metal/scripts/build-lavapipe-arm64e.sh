#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-or-later
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MESA_SRC="$ROOT/src/mesa"
BUILD_DIR="$ROOT/build/mesa-lvp-arm64e"
PREFIX="$ROOT/dist/lavapipe-arm64e"
LLVM_BIN="$ROOT/build/llvm-arm64e-install/bin"

export CC=/usr/bin/clang
export CXX=/usr/bin/clang++
export OBJC=/usr/bin/clang
export OBJCXX=/usr/bin/clang++
export PATH="$ROOT/build/pyvenv/bin:$ROOT/build/vk-deps/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin:$LLVM_BIN"
export PKG_CONFIG_PATH="/opt/homebrew/lib/pkgconfig:/opt/homebrew/share/pkgconfig"

if [ ! -f "$BUILD_DIR/build.ninja" ]; then
  meson setup "$BUILD_DIR" "$MESA_SRC" \
    --prefix="$PREFIX" \
    --buildtype=release \
    -Dc_args="-arch arm64e" \
    -Dcpp_args="-arch arm64e" \
    -Dobjc_args="-arch arm64e" \
    -Dobjcpp_args="-arch arm64e" \
    -Dc_link_args="-arch arm64e" \
    -Dcpp_link_args="-arch arm64e" \
    -Dobjc_link_args="-arch arm64e" \
    -Dobjcpp_link_args="-arch arm64e" \
    -Dplatforms= \
    -Dgallium-drivers=llvmpipe \
    -Dvulkan-drivers=swrast \
    -Dglx=disabled \
    -Degl=disabled \
    -Dopengl=false \
    -Dgles1=disabled \
    -Dgles2=disabled \
    -Dllvm=enabled \
    -Dshared-llvm=disabled \
    -Dzstd=disabled \
    -Dxmlconfig=disabled \
    -Dshader-cache=enabled \
    -Dshader-cache-max-size=256M \
    -Dvideo-codecs= \
    -Dvalgrind=disabled \
    -Dlibunwind=disabled \
    -Dbuild-tests=false
fi

ninja -C "$BUILD_DIR" -j"$(sysctl -n hw.ncpu)"
ninja -C "$BUILD_DIR" install

echo "=== lavapipe arm64e installed to $PREFIX ==="
file "$PREFIX/lib/libvulkan_lvp.dylib"
otool -L "$PREFIX/lib/libvulkan_lvp.dylib"
