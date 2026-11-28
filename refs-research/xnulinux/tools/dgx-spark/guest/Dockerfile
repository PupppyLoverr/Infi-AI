# SPDX-License-Identifier: GPL-2.0-only
FROM ubuntu:24.04
RUN apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends qemu-system-arm qemu-efi-aarch64 qemu-utils cloud-image-utils genisoimage ipxe-qemu >/dev/null && rm -rf /var/lib/apt/lists/*
