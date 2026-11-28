#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-or-later
# Fetch the upstream sources the plugin builds against into src/ (never committed: see ../README.md),
# then apply our local patches from patches/<project>/*.patch idempotently.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT/src"

git_at() {   # git_at DIR URL REV
  if [ ! -d "$1/.git" ]; then
    mkdir -p "$1" && git -C "$1" init -q && git -C "$1" remote add origin "$2"
  fi
  git -C "$1" checkout -q -- . 2>/dev/null || true
  git -C "$1" fetch -q --depth 1 origin "$3" 2>/dev/null || git -C "$1" fetch -q origin
  git -C "$1" checkout -q "$3"
}

apply_patches() {   # apply_patches DIR PATCH_SUBDIR
  local dir="$1" pdir="$ROOT/patches/$2"
  [ -d "$pdir" ] || return 0
  shopt -s nullglob
  local patches=("$pdir"/*.patch)
  shopt -u nullglob
  [ ${#patches[@]} -gt 0 ] || return 0
  for p in "${patches[@]}"; do
    if patch -d "$dir" -p1 --dry-run -R -s -f < "$p" >/dev/null 2>&1; then
      echo "patch already applied: $(basename "$p") -> $2"
    else
      echo "applying patch: $(basename "$p") -> $2"
      patch -d "$dir" -p1 -s -f < "$p"
    fi
  done
}

git_at metal2vulkan  https://github.com/steelbrain/metal2vulkan.git   d9236a8a11d3b90a1aa27b54e2d74d3891c4d2e5   # v0.2.0, GPL-2.0-or-later / LGPL-3.0-or-later
git_at Vulkan-Loader https://github.com/KhronosGroup/Vulkan-Loader.git fa218055ff4948b159f31d43f800c808a13d51df   # vulkan-sdk-1.4.363.0, Apache-2.0
git_at llvm-project  https://github.com/llvm/llvm-project.git          ca7933e47d3a3451d81e72ac174dcb5aa28b59d1   # llvmorg-22.1.8 (arm64e llvmpipe JIT)
git_at AdrenoMetal   https://github.com/UniXNU/AdrenoMetal.git      cc0a4e8e57d20f4a4698753c2530875f2b10c0ca   # MIT; only bridge/RustPacBoundary.c (arm64e PAC trampolines) is built
mkdir -p "$ROOT/build"
# the tarball and the tree under another name until they are whole: an error page, a transfer cut short or a failed
# unpacking is not what the next run finds there
M="$ROOT/build/mesa-26.3.1.tar.xz"
[ -f "$M" ] || { curl -fsSLo "$M.part" https://archive.mesa3d.org/mesa-26.3.1.tar.xz; mv "$M.part" "$M"; }
[ -d mesa ] || { rm -rf "$ROOT/build/mesa.part"; mkdir "$ROOT/build/mesa.part"; tar -C "$ROOT/build/mesa.part" --strip-components=1 -xf "$M"; mv "$ROOT/build/mesa.part" mesa; }   # MIT (lavapipe)

apply_patches metal2vulkan  metal2vulkan
apply_patches Vulkan-Loader Vulkan-Loader
apply_patches mesa          mesa

echo "sources ready in $ROOT/src"
