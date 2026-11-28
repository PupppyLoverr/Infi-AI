#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-or-later
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BUILD_DIR="$ROOT/build/llvm-arm64e"
PREFIX="$ROOT/build/llvm-arm64e-install"

export PATH="$ROOT/build/pyvenv/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin"

cmake -G Ninja -S "$ROOT/src/llvm-project/llvm" -B "$BUILD_DIR" \
  -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_C_COMPILER=/usr/bin/clang \
  -DCMAKE_CXX_COMPILER=/usr/bin/clang++ \
  -DCMAKE_OSX_ARCHITECTURES=arm64e \
  -DCMAKE_INSTALL_PREFIX="$PREFIX" \
  -DLLVM_TARGETS_TO_BUILD=AArch64 \
  -DLLVM_INCLUDE_TESTS=OFF \
  -DLLVM_INCLUDE_EXAMPLES=OFF \
  -DLLVM_INCLUDE_BENCHMARKS=OFF \
  -DLLVM_INCLUDE_DOCS=OFF \
  -DLLVM_BUILD_UTILS=OFF \
  -DLLVM_BUILD_TOOLS=OFF \
  -DLLVM_TOOL_LLVM_CONFIG_BUILD=ON \
  -DLLVM_ENABLE_ZSTD=OFF \
  -DLLVM_ENABLE_ZLIB=ON \
  -DLLVM_ENABLE_LIBXML2=OFF \
  -DLLVM_ENABLE_TERMINFO=OFF \
  -DLLVM_ENABLE_RTTI=ON \
  -DLLVM_BUILD_LLVM_DYLIB=OFF

ninja -C "$BUILD_DIR" -j"$(sysctl -n hw.ncpu)"
ninja -C "$BUILD_DIR" llvm-config
ninja -C "$BUILD_DIR" install
mkdir -p "$PREFIX/bin"
cp -f "$BUILD_DIR/bin/llvm-config" "$PREFIX/bin/llvm-config"
file "$PREFIX/bin/llvm-config"
"$PREFIX/bin/llvm-config" --version --targets-built --system-libs
