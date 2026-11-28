#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-or-later
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BRIDGE_DIR="$ROOT/src/xnu-m2v-bridge"
DIST_DIR="$ROOT/dist/m2v"
mkdir -p "$DIST_DIR/lib" "$DIST_DIR/include"

export RUSTUP_TOOLCHAIN=1.100.0
cargo build --manifest-path "$BRIDGE_DIR/Cargo.toml" --release --target aarch64-apple-darwin

cp -f "$BRIDGE_DIR/target/aarch64-apple-darwin/release/libxnu_m2v.dylib" "$DIST_DIR/lib/libxnu_m2v.dylib"
cp -f "$BRIDGE_DIR/include/xnu_m2v.h" "$DIST_DIR/include/xnu_m2v.h"
install_name_tool -id "@rpath/libxnu_m2v.dylib" "$DIST_DIR/lib/libxnu_m2v.dylib"
codesign -s - --force "$DIST_DIR/lib/libxnu_m2v.dylib" 2>/dev/null || true

# Also stage libvulkan.1.dylib beside libvulkan_lvp.dylib for self-contained distribution
cp -f "$ROOT/build/vk-deps/vulkan-loader/1.4.363.0/lib/libvulkan.1.4.363.dylib" "$ROOT/dist/lavapipe/lib/libvulkan.1.dylib"
chmod u+w "$ROOT/dist/lavapipe/lib/libvulkan.1.dylib"
install_name_tool -id "@rpath/libvulkan.1.dylib" "$ROOT/dist/lavapipe/lib/libvulkan.1.dylib"
codesign -s - --force "$ROOT/dist/lavapipe/lib/libvulkan.1.dylib" 2>/dev/null || true

echo "=== libxnu_m2v.dylib installed to $DIST_DIR ==="
ls -lh "$DIST_DIR/lib/libxnu_m2v.dylib"
otool -L "$DIST_DIR/lib/libxnu_m2v.dylib"
