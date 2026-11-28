#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-or-later
# What the build scripts expect under build/ that none of them makes. Once, before the first build:
#   build/pyvenv        the Python the Mesa and LLVM builds run (mako, pyyaml, packaging: Mesa's generators)
#   build/vk-deps/bin   Homebrew's bison and flex, ahead of macOS's own (Mesa needs bison 3)
#   build/vk-deps/vulkan-headers/$VK   the Vulkan headers of the loader's version (scripts/fetch-sources.sh fetches the loader)
# The loader itself goes to build/vk-deps/vulkan-loader by scripts/build-vkloader-arm64e.sh.
# Needs Homebrew's python3, bison, flex, cmake and ninja (the README's brew line); it installs nothing outside build/.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
VK=1.4.363.0

for tool in python3 bison flex; do
  [ -x "$(brew --prefix $tool 2>/dev/null)/bin/$tool" ] || { echo "setup-deps.sh: Homebrew's $tool is not installed (brew install $tool)" >&2; exit 1; }
done
mkdir -p build/vk-deps/bin
[ -x build/pyvenv/bin/python3 ] || "$(brew --prefix python3)/bin/python3" -m venv build/pyvenv
build/pyvenv/bin/pip -q install mako pyyaml packaging
ln -sf "$(brew --prefix bison)/bin/bison" "$(brew --prefix flex)/bin/flex" build/vk-deps/bin/

if [ ! -f "build/vk-deps/vulkan-headers/$VK/include/vulkan/vulkan.h" ]; then
  export PATH="$ROOT/build/pyvenv/bin:/opt/homebrew/bin:$PATH"
  [ -d build/Vulkan-Headers/.git ] || git clone -q --depth 1 --branch "vulkan-sdk-$VK" https://github.com/KhronosGroup/Vulkan-Headers.git build/Vulkan-Headers
  cmake -G Ninja -S build/Vulkan-Headers -B build/Vulkan-Headers/build -DCMAKE_INSTALL_PREFIX="$ROOT/build/vk-deps/vulkan-headers/$VK" >/dev/null
  ninja -C build/Vulkan-Headers/build install >/dev/null
fi
echo "build/pyvenv, build/vk-deps/bin and build/vk-deps/vulkan-headers/$VK are ready"
