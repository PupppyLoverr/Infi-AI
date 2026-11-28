#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# the xnulinux guest: arm64 Ubuntu under KVM, QEMU from a container (no QEMU on the host). ssh: 127.0.0.1:2222, key ~/xnuvm/vmkey
# CPUSET= pins it (on the GB10: 5-9,15-19, the X925 cores)
V=~/xnuvm; MEM=${MEM:-24576}; CPUS=${CPUS:-10}
docker rm -f xnuvm >/dev/null 2>&1
docker run -d --name xnuvm --device /dev/kvm --group-add $(stat -c %g /dev/kvm) --network host ${CPUSET:+--cpuset-cpus "$CPUSET"} -v $V:/vm xnuvm-qemu \
  qemu-system-aarch64 -machine virt,gic-version=host -accel kvm -cpu host -smp $CPUS -m $MEM \
  -drive if=pflash,format=raw,readonly=on,file=/usr/share/AAVMF/AAVMF_CODE.fd -drive if=pflash,format=raw,file=/vm/vars.fd \
  -drive if=none,id=d0,file=/vm/disk.img,format=raw,cache=none,aio=native,discard=unmap -device virtio-blk-pci,drive=d0 \
  -drive if=none,id=cd,file=/vm/seed.iso,format=raw,readonly=on -device virtio-blk-pci,drive=cd \
  -drive if=none,id=mac,file=/vm/mac.img,format=raw,cache=none,aio=native,discard=unmap -device virtio-blk-pci,drive=mac,serial=macimg \
  -netdev user,id=n0,hostfwd=tcp:127.0.0.1:2222-:22 -device virtio-net-pci,netdev=n0 \
  -device virtio-rng-pci -display none -serial file:/vm/console.log -monitor unix:/vm/mon.sock,server,nowait
