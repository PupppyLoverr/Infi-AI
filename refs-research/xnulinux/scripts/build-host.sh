#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# Build the arm64 host Linux (4K pages like the UniXNU 4K XNU machine layer, 48-bit VA, user PAC) with the FKI patch, plus a busybox initramfs.
set -euo pipefail
P=$(cd "$(dirname "$0")/.." && pwd); W=${W:?W: a work directory holding src/linux (a Linux tree); the build goes to $W/out/linux}; W=$(cd "$W" && pwd); L=$W/src/linux; O=$W/out/linux; mkdir -p $O
cd $L
git checkout -q -- drivers/Kconfig drivers/Makefile
rsync -a $P/linux/ ./
grep -q drivers/fki/Kconfig drivers/Kconfig || sed -i 's|^endmenu|source "drivers/fki/Kconfig"\n\nendmenu|' drivers/Kconfig
grep -q 'CONFIG_FKI' drivers/Makefile || echo 'obj-$(CONFIG_FKI) += fki/' >> drivers/Makefile
M="make -s ARCH=arm64 LLVM=-19 O=$O -j$(nproc)"
[ -f $O/.config ] || $M defconfig
S=scripts/config; C="--file $O/.config"
$S $C -e ARM64_4K_PAGES -e ARM64_VA_BITS_48 \
  -e ARM64_PTR_AUTH -d ARM64_PTR_AUTH_KERNEL -d ARM64_BTI_KERNEL -e CONTIG_ALLOC -d RANDOMIZE_BASE -e MODULES -e MODULE_UNLOAD \
  -e BLK_DEV_INITRD -e DEVTMPFS -e DEVTMPFS_MOUNT -e VIRTIO_BLK -e VIRTIO_NET -e VIRTIO_CONSOLE \
  -e CMA -e DMA_CMA -e FW_LOADER -e KALLSYMS_ALL -e DEBUG_INFO_NONE -d DRM -d SOUND -d WLAN -d MEDIA_SUPPORT -d USB_SUPPORT \
  -e FKI
$M olddefconfig
$M Image modules
ls -la $O/arch/arm64/boot/Image
