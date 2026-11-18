# Budgets — measured numbers, every milestone

Hard targets live in PLAN.md §7. This file holds what we have actually
measured. No estimates here — only runs.

## 2026-11-18 — base image (P1 groundwork, TTY-only, no desktop yet)

- `docker build -f base/Containerfile -t solyra-base:dev base/` → **build OK**
- Image size: **3.11 GB** (3,108,425,554 bytes, uncompressed rootfs)
- Base: `quay.io/fedora/fedora-bootc:46` + NetworkManager, polkit, sudo,
  openssh-server; locales/man/doc trees and package caches removed.

Reading: the ≤ 4 GB installed budget has room for the compositor + shell +
apps, but not much slack — P1's first job is firmware/kernel-modules trimming
and a per-directory du breakdown of the rootfs, before any desktop lands.

## 2026-11-18 — rootfs breakdown of the same base image

Inside the built image (`du -x -d1 /usr`):

| Path | Size |
| --- | --- |
| /usr total | 1.7 GB |
| /usr/lib | 826 MB |
| /usr/bin | 391 MB |
| /usr/lib64 | 286 MB |
| /usr/share | 134 MB |
| /usr/lib/firmware | 310 MB |
| /usr/lib/modules | 302 MB |

Reading: the 3.11 GB image size is the sum of uncompressed layers (the bootc
base ships as several layers, and the dnf install layer duplicates changed
files); the live rootfs is ~1.7 GB. Firmware + kernel modules = ~600 MB of
the most trimmable space. Next P1 step: bootc-image-builder → raw/qcow2,
measure real installed size and boot-to-TTY time on the booted image.
