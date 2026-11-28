#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# Build and install virglrenderer with Venus (/usr/local) for xnudrmd's virglrenderer backend.
# Ubuntu 24.04's 1.0.0 fails vkCreateDevice on NVIDIA's driver; 1.2.0 works.
set -euo pipefail
V=1.2.0
W=${W:-$HOME/w/virglrenderer}; mkdir -p "$W"; cd "$W"
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq build-essential meson ninja-build pkg-config libdrm-dev \
  libepoxy-dev libvulkan-dev libgbm-dev libegl-dev python3-yaml >/dev/null
# (under another name until it is whole: an error page or a transfer cut short is not the tarball of the next run)
[ -f virglrenderer-$V.tar.gz ] || { curl -fsSLo virglrenderer-$V.tar.gz.part \
  https://gitlab.freedesktop.org/virgl/virglrenderer/-/archive/$V/virglrenderer-$V.tar.gz; mv virglrenderer-$V.tar.gz.part virglrenderer-$V.tar.gz; }
rm -rf src && mkdir src && tar -C src --strip-components=1 -xf virglrenderer-$V.tar.gz
# A process for each context (the default), not a thread: NVIDIA's driver gives one process about 512 open device files,
# a context with a device holds 10 to 75 of them, and past that vkCreateDevice fails (VK_ERROR_INITIALIZATION_FAILED):
# with every context in one process the twentieth-odd client of a macOS desktop got the software renderer instead.
# One context's crash also no longer takes the others' with it.
meson setup src/build src --prefix=/usr/local --buildtype=release -Dvenus=true -Drender-server-worker=process >/dev/null
ninja -C src/build > ninja.log 2>&1 || { grep -E "error|FAILED" ninja.log | head -30; exit 1; }
sudo ninja -C src/build install >/dev/null
sudo ldconfig
pkg-config --modversion virglrenderer
