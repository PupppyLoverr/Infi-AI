#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# In the Linux guest: build Mesa's Venus Vulkan driver with our patches (patches/), then check it on the GPU behind
# virtio-gpu: list the device and run test/compute.c (a compute dispatch over 16 MB of host-visible memory, read back and
# compared). Output: $OUT/lib/libvulkan_virtio.so and $OUT/share/vulkan/icd.d/virtio_icd.*.json.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
MESA=26.3.1
W=${W:-$HOME/w/venus}; mkdir -p "$W"; cd "$W"; W=$PWD; OUT=${OUT:-$W/out}	# W absolute: PATH and meson's --prefix below
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq ninja-build libdrm-dev glslang-tools bison flex pkg-config \
  libexpat1-dev zlib1g-dev libudev-dev libzstd-dev libvulkan-dev vulkan-tools python3-venv >/dev/null
[ -x venv/bin/meson ] || { python3 -m venv venv && venv/bin/pip -q install meson mako pyyaml packaging; }
export PATH=$W/venv/bin:$PATH
# (under another name until it is whole: an error page or a transfer cut short is not the tarball of the next run)
[ -f mesa-$MESA.tar.xz ] || { curl -fsSLo mesa-$MESA.tar.xz.part https://archive.mesa3d.org/mesa-$MESA.tar.xz; mv mesa-$MESA.tar.xz.part mesa-$MESA.tar.xz; }
rm -rf mesa && mkdir mesa && tar -C mesa --strip-components=1 -xf mesa-$MESA.tar.xz
for p in "$HERE"/patches/*.patch; do patch -d mesa -p1 --no-backup-if-mismatch -s < "$p"; done
meson setup mesa/build mesa --prefix="$OUT" --buildtype=release -Dvulkan-drivers=virtio -Dgallium-drivers= \
  -Dplatforms= -Dglx=disabled -Degl=disabled -Dgbm=disabled -Dllvm=disabled -Dopengl=false -Dgles1=disabled \
  -Dgles2=disabled -Dvideo-codecs= >/dev/null
ninja -C mesa/build install > ninja.log 2>&1 || { grep -E "error|FAILED" ninja.log | head -30; exit 1; }
ICD=$(ls "$OUT"/share/vulkan/icd.d/virtio_icd.*.json)
glslangValidator -V "$HERE"/test/compute.comp -o "$W"/compute.spv >/dev/null
cc -O2 "$HERE"/test/compute.c -o "$W"/compute -ldl
# /dev/dri/renderD128 is root:render; run as root unless the user is in render
SUDO=; [ -r /dev/dri/renderD128 ] && [ -w /dev/dri/renderD128 ] || SUDO=sudo
$SUDO env VK_ICD_FILENAMES="$ICD" vulkaninfo --summary 2>/dev/null | grep -E "deviceName|driverInfo"
$SUDO env VK_ICD_FILENAMES="$ICD" "$W"/compute "$W"/compute.spv
