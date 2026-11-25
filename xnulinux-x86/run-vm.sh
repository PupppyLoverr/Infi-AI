#!/bin/bash
# xnulinux on any x86_64 device: the arm64 Linux host is a fully emulated
# qemu-system-aarch64 machine. xnulinux inside it is unmodified — the hosted
# XNU drives the emulated CPU exactly as it would drive a physical arm64 one.
#   -cpu max,pauth=on : FEAT_PAuth (PAC) is emulated by QEMU TCG
#   16G RAM           : xnulinux reserves 8G for the hosted kernel
exec qemu-system-aarch64 \
  -M virt -cpu max,pauth=on -smp 6 -m 16G \
  -drive if=pflash,format=raw,file=AAVMF_CODE.fd,readonly=on \
  -drive if=pflash,format=raw,file=AAVMF_VARS.fd \
  -drive file=noble.img,format=qcow2,if=virtio \
  -drive file=seed.img,format=raw,if=virtio \
  -netdev user,id=n0,hostfwd=tcp::10022-:22 -device virtio-net-pci,netdev=n0 \
  -nographic -serial mon:stdio
