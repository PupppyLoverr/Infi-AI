#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-or-later
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SDK="$(xcrun --show-sdk-path)"
BUNDLE="$ROOT/dist/XnuCpuMetal.bundle"

rm -rf "$BUNDLE"
mkdir -p "$BUNDLE/Contents/MacOS" "$BUNDLE/Contents/Frameworks" "$BUNDLE/Contents/Resources"

cp -f "$ROOT/src/xnu-cpu-metal-bundle/Info.plist" "$BUNDLE/Contents/Info.plist"

# Stage self-contained dylibs inside Contents/Frameworks
cp -f "$ROOT/dist/m2v/lib/libxnu_m2v.dylib" "$BUNDLE/Contents/Frameworks/libxnu_m2v.dylib"
cp -f "$ROOT/dist/lavapipe/lib/libvulkan.1.dylib" "$BUNDLE/Contents/Frameworks/libvulkan.1.dylib"
cp -f "$ROOT/dist/lavapipe/lib/libvulkan_lvp.dylib" "$BUNDLE/Contents/Frameworks/libvulkan_lvp.dylib"

# Relocatable ICD JSON (relative to Contents/Resources/ -> ../Frameworks/libvulkan_lvp.dylib)
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

# Compile XnuCpuMetal MH_BUNDLE (arm64)
/usr/bin/clang++ -arch arm64 -bundle -O2 -std=c++20 -fobjc-arc \
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
  -o "$BUNDLE/Contents/MacOS/XnuCpuMetal"

codesign -s - --force "$BUNDLE/Contents/Frameworks/libxnu_m2v.dylib" 2>/dev/null || true
codesign -s - --force "$BUNDLE/Contents/Frameworks/libvulkan.1.dylib" 2>/dev/null || true
codesign -s - --force "$BUNDLE/Contents/Frameworks/libvulkan_lvp.dylib" 2>/dev/null || true
codesign -s - --force "$BUNDLE/Contents/MacOS/XnuCpuMetal" 2>/dev/null || true

echo "=== Built $BUNDLE ==="
ls -lh "$BUNDLE/Contents/MacOS/" "$BUNDLE/Contents/Frameworks/" "$BUNDLE/Contents/Resources/"
file "$BUNDLE/Contents/MacOS/XnuCpuMetal"
otool -L "$BUNDLE/Contents/MacOS/XnuCpuMetal"
