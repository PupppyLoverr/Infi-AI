#!/bin/bash
set -x
K=/root/xc/hdrs/usr/src/linux-headers-6.8.0-147-generic
cd /root/w/poc
FLAGS='-include /root/w/neon-shim.h -Wno-error=strict-prototypes'
make -C $K M=$PWD/fki-ko ARCH=arm64 CROSS_COMPILE=aarch64-linux-gnu- EXTRA_CFLAGS="$FLAGS" modules
make -C $K M=$PWD/xnu-ko KBUILD_EXTRA_SYMBOLS=$PWD/fki-ko/Module.symvers ARCH=arm64 CROSS_COMPILE=aarch64-linux-gnu- EXTRA_CFLAGS="$FLAGS" modules
ls -la fki-ko/fki.ko xnu-ko/xnu.ko
