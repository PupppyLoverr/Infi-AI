#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-or-later
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SDK="$(xcrun --show-sdk-path)"
BUNDLE="$ROOT/dist/XnuCpuMetal.bundle"
TMP="$ROOT/build/bundle-universal"

rm -rf "$BUNDLE" "$TMP"
mkdir -p "$BUNDLE/Contents/MacOS" "$BUNDLE/Contents/Frameworks" "$BUNDLE/Contents/Resources" "$TMP"

cp -f "$ROOT/src/xnu-cpu-metal-bundle/Info.plist" "$BUNDLE/Contents/Info.plist"

# 1. Create universal (arm64 + arm64e) Frameworks dylibs
lipo -create \
  "$ROOT/dist/m2v/lib/libxnu_m2v.dylib" \
  "$ROOT/dist/m2v-arm64e/lib/libxnu_m2v.dylib" \
  -output "$BUNDLE/Contents/Frameworks/libxnu_m2v.dylib"

lipo -create \
  "$ROOT/build/vkloader-arm64-install/lib/libvulkan.1.dylib" \
  "$ROOT/build/vkloader-arm64e-install/lib/libvulkan.1.dylib" \
  -output "$BUNDLE/Contents/Frameworks/libvulkan.1.dylib"

lipo -create \
  "$ROOT/dist/lavapipe/lib/libvulkan_lvp.dylib" \
  "$ROOT/dist/lavapipe-arm64e/lib/libvulkan_lvp.dylib" \
  -output "$BUNDLE/Contents/Frameworks/libvulkan_lvp.dylib"

VENUS_DYLIB="${VENUS_DYLIB:-$HOME/w/venus-darwin/out/lib/libvulkan_virtio.dylib}"	# gpu/venus/build-darwin.sh output
HAS_VENUS=0
if [ -f "$VENUS_DYLIB" ]; then
  cp -f "$VENUS_DYLIB" "$BUNDLE/Contents/Frameworks/libvulkan_virtio.dylib"
  chmod u+w "$BUNDLE/Contents/Frameworks/libvulkan_virtio.dylib"
  install_name_tool -id "@rpath/libvulkan_virtio.dylib" "$BUNDLE/Contents/Frameworks/libvulkan_virtio.dylib"
  HAS_VENUS=1
fi

# 2. Relocatable ICD JSONs (both top-level for env-var/backend selection and vulkan/icd.d/ for secure-mode SYSCONFDIR)
cat > "$BUNDLE/Contents/Resources/lvp_icd.aarch64.json" <<'EOF'
{
    "ICD": {
        "api_version": "1.4.363",
        "library_arch": "64",
        "library_path": "../Frameworks/libvulkan_lvp.dylib"
    },
    "file_format_version": "1.0.1"
}
EOF
mkdir -p "$BUNDLE/Contents/Resources/vulkan/icd.d"
cat > "$BUNDLE/Contents/Resources/vulkan/icd.d/lvp_icd.aarch64.json" <<'EOF'
{
    "ICD": {
        "api_version": "1.4.363",
        "library_arch": "64",
        "library_path": "../../../Frameworks/libvulkan_lvp.dylib"
    },
    "file_format_version": "1.0.1"
}
EOF
if [ "$HAS_VENUS" = "1" ]; then
  cat > "$BUNDLE/Contents/Resources/virtio_icd.aarch64.json" <<'EOF'
{
    "ICD": {
        "api_version": "1.4.363",
        "library_arch": "64",
        "library_path": "../Frameworks/libvulkan_virtio.dylib"
    },
    "file_format_version": "1.0.1"
}
EOF
  cat > "$BUNDLE/Contents/Resources/vulkan/icd.d/virtio_icd.aarch64.json" <<'EOF'
{
    "ICD": {
        "api_version": "1.4.363",
        "library_arch": "64",
        "library_path": "../../../Frameworks/libvulkan_virtio.dylib"
    },
    "file_format_version": "1.0.1"
}
EOF
fi

# 3. Compile XnuCpuMetal MH_BUNDLE for both arm64 and arm64e, then lipo
for ARCH in arm64 arm64e; do
  /usr/bin/clang++ -arch "$ARCH" -bundle -O2 -std=c++20 -fobjc-arc \
    -isysroot "$SDK" \
    -F"$SDK/System/Library/PrivateFrameworks" \
    -I"$ROOT/dist/m2v/include" \
    -I"$ROOT/src/xnu-cpu-metal-bundle" \
    "$ROOT/src/xnu-cpu-metal-bundle/XnuCpuMetalPlugin.mm" \
    -L"$BUNDLE/Contents/Frameworks" -lxnu_m2v \
    -Wl,-rpath,@loader_path/../Frameworks \
    -framework Foundation \
    -framework Metal \
    -framework IOKit \
    -framework IOSurface \
    -framework IOGPU \
    -o "$TMP/XnuCpuMetal.${ARCH}"
done

lipo -create "$TMP/XnuCpuMetal.arm64" "$TMP/XnuCpuMetal.arm64e" \
  -output "$BUNDLE/Contents/MacOS/XnuCpuMetal"

# 4. Ad-hoc sign all universal binaries
codesign -s - --force "$BUNDLE/Contents/Frameworks/libxnu_m2v.dylib"
codesign -s - --force "$BUNDLE/Contents/Frameworks/libvulkan.1.dylib"
codesign -s - --force "$BUNDLE/Contents/Frameworks/libvulkan_lvp.dylib"
if [ "$HAS_VENUS" = "1" ]; then
  codesign -s - --force "$BUNDLE/Contents/Frameworks/libvulkan_virtio.dylib"
fi
codesign -s - --force "$BUNDLE/Contents/MacOS/XnuCpuMetal"

echo "=== Built universal (arm64 + arm64e) $BUNDLE ==="
for f in \
  "$BUNDLE/Contents/MacOS/XnuCpuMetal" \
  "$BUNDLE/Contents/Frameworks/libxnu_m2v.dylib" \
  "$BUNDLE/Contents/Frameworks/libvulkan.1.dylib" \
  "$BUNDLE/Contents/Frameworks/libvulkan_lvp.dylib"; do
  lipo -info "$f"
done
if [ "$HAS_VENUS" = "1" ]; then
  lipo -info "$BUNDLE/Contents/Frameworks/libvulkan_virtio.dylib"
fi
ls -lh "$BUNDLE/Contents/MacOS/" "$BUNDLE/Contents/Frameworks/"
