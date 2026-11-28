#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-or-later
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MESA_SRC="$ROOT/src/mesa"
BUILD_DIR="$ROOT/build/mesa-lvp"
PREFIX="$ROOT/dist/lavapipe"

export CC=/usr/bin/clang
export CXX=/usr/bin/clang++
export OBJC=/usr/bin/clang
export OBJCXX=/usr/bin/clang++
export PATH="$ROOT/build/pyvenv/bin:$ROOT/build/vk-deps/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/opt/llvm@22/bin"
export PKG_CONFIG_PATH="/opt/homebrew/lib/pkgconfig:/opt/homebrew/share/pkgconfig"

mkdir -p "$ROOT/build/static-libs"
ln -sf /opt/homebrew/opt/zstd/lib/libzstd.a "$ROOT/build/static-libs/libzstd.a"

if [ ! -f "$BUILD_DIR/build.ninja" ]; then
  meson setup "$BUILD_DIR" "$MESA_SRC" \
    --prefix="$PREFIX" \
    --buildtype=release \
    -Dc_link_args="-L$ROOT/build/static-libs" \
    -Dcpp_link_args="-L$ROOT/build/static-libs" \
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

echo "=== lavapipe installed to $PREFIX ==="
ls -lh "$PREFIX/lib/" "$PREFIX/share/vulkan/icd.d/"
otool -L "$PREFIX/lib/libvulkan_lvp.dylib"
