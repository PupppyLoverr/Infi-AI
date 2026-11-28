#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# In the VM: build tools/init with the DroidVM k27 toolchain and put it at /sbin/launchd in /lib/firmware/xnu/root.img.
set -euo pipefail
cd "$(dirname "$0")"; TC=~/w/dvm/toolchains/linux/bin26
$TC/target-clang -target arm64-apple-macos14.0 -c start.s -o start.o
$TC/target-clang -target arm64-apple-macos14.0 -nostdlib -static -e _start -Wl,-adhoc_codesign start.o -o init
$TC/target-clang -target arm64-apple-macos14.0 -c hello.s -o hello.o
$TC/target-clang -target arm64-apple-macos14.0 -nostdlib -static -e _start -Wl,-adhoc_codesign hello.o -o hello
cp /lib/firmware/xnu/root.img /tmp/root.img; sudo mkdir -p /mnt/rd
sudo mount -o loop -t hfsplus /tmp/root.img /mnt/rd
sudo cp init /mnt/rd/sbin/launchd; sudo chmod 755 /mnt/rd/sbin/launchd; sudo cp hello /mnt/rd/sbin/hello; sudo chmod 755 /mnt/rd/sbin/hello; ls -la /mnt/rd/sbin /mnt/rd/dev
sudo umount /mnt/rd; sudo cp /tmp/root.img /lib/firmware/xnu/root.img
