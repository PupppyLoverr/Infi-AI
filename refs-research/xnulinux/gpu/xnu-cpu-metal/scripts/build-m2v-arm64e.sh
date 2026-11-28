#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-or-later
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BRIDGE_DIR="$ROOT/src/xnu-m2v-bridge"
DIST_DIR="$ROOT/dist/m2v-arm64e"
WORK="$ROOT/build/m2v-arm64e-link"
VK_HDR="$ROOT/build/vk-deps/vulkan-headers/1.4.363.0/include"
OBJCOPY="/opt/homebrew/opt/llvm@22/bin/llvm-objcopy"

mkdir -p "$DIST_DIR/lib" "$DIST_DIR/include"
rm -rf "$WORK" && mkdir -p "$WORK/objs"

# 1. Build Rust staticlib for arm64e-apple-darwin with -Zbuild-std: the nightly the pinned metal2vulkan names in its own
# rust-toolchain.toml (RUST_NIGHTLY to try another), not whichever nightly rustup has today
export RUSTUP_TOOLCHAIN="${RUST_NIGHTLY:-nightly-2026-11-03}-aarch64-apple-darwin"
cargo build --manifest-path "$BRIDGE_DIR/Cargo.toml" \
  -Zbuild-std=std,panic_unwind \
  --target arm64e-apple-darwin \
  --release

# 2. Compile AdrenoMetal's RustPacBoundary.c as arm64e
/usr/bin/clang -arch arm64e -O2 -I"$VK_HDR" \
  -c "$ROOT/src/AdrenoMetal/bridge/RustPacBoundary.c" \
  -o "$WORK/RustPacBoundary.o"

# 3. Extract Rust staticlib objects and rename C-callback-taking imports to nmb_pac_*
(cd "$WORK/objs" && ar x "$BRIDGE_DIR/target/arm64e-apple-darwin/release/libxnu_m2v.a")
for obj in "$WORK/objs"/*.o; do
  "$OBJCOPY" \
    --redefine-sym _pthread_create=_nmb_pac_pthread_create \
    --redefine-sym __tlv_atexit=_nmb_pac_tlv_atexit \
    --redefine-sym _atexit=_nmb_pac_atexit \
    --redefine-sym ___cxa_atexit=_nmb_pac_cxa_atexit \
    --redefine-sym __dyld_register_func_for_add_image=_nmb_pac_dyld_register_func_for_add_image \
    --redefine-sym __Unwind_Backtrace=_nmb_pac_Unwind_Backtrace \
    --redefine-sym _dlsym=_nmb_pac_dlsym \
    "$obj" "$obj.tmp"
  mv "$obj.tmp" "$obj"
done
ar rcs "$WORK/libxnu_m2v_pac.a" "$WORK/objs"/*.o

# 4. Link arm64e dylib with RustPacBoundary.o + renamed Rust archive
/usr/bin/clang -arch arm64e -dynamiclib \
  -Wl,-force_load,"$WORK/libxnu_m2v_pac.a" \
  "$WORK/RustPacBoundary.o" \
  -liconv -lSystem \
  -install_name "@rpath/libxnu_m2v.dylib" \
  -o "$DIST_DIR/lib/libxnu_m2v.dylib"

# 5. Rewrite plain Rust `blr`/`br` into `bl`/`b _nmb_pac_call_xN` trampolines
python3 "$ROOT/scripts/rust-pac-rewrite.py" "$DIST_DIR/lib/libxnu_m2v.dylib"

# 6. Ad-hoc sign after binary rewrite
codesign -s - --force "$DIST_DIR/lib/libxnu_m2v.dylib"
cp -f "$BRIDGE_DIR/include/xnu_m2v.h" "$DIST_DIR/include/xnu_m2v.h"

echo "=== arm64e PAC-safe libxnu_m2v.dylib installed to $DIST_DIR ==="
file "$DIST_DIR/lib/libxnu_m2v.dylib"
otool -L "$DIST_DIR/lib/libxnu_m2v.dylib"
