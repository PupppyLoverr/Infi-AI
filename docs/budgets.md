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

## 2026-11-18 — first bootable Solyra disk (QEMU x86_64, UEFI)

- `scripts/build-disk.sh` assembles an 8 GiB raw disk: GPT with a 512 MiB
  ESP (vfat, written via mtools) and an XFS root. `bootc install to-filesystem
  --bootloader none` deploys the OSTree commit; a separate GRUB core image
  (`grub-mkimage -O x86_64-efi`, prefix on the ESP) boots it.
- **Boot verified in QEMU (TCG, OVMF): "Welcome to Solyra OS 0.1
  (Perihelion)" → `Reached target multi-user.target`, NetworkManager up.**
- Deployed root filesystem: **2.0 GiB** (`du -shx` before first boot).
  That excludes the EFI partition and filesystem metadata, so it is not
  the total installed footprint.
- Serial login, idle RAM, aarch64, and hardware boot time are unverified.
  QEMU's software emulation is a boot check, not a hardware timing result.
- The niri vendored tree (`comp/`) now **typechecks cleanly** (`cargo check
  -p niri` in `comp/dev.Containerfile`, which needed cairo-gobject-devel +
  pango-devel) — the P2 fork baseline.

## 2026-11-18 — desktop development disk (P2)

- Deployed root filesystem before first boot: **3.02 GB** (3,023,699,968
  bytes, `du -sx -B1` on the mounted XFS root; `df` reports 2.81 GiB used).
  This excludes EFI, filesystem metadata, and first-boot or update growth, so
  it is not yet the final installed-size measurement.
- The raw disk has **8 GiB** of virtual capacity, not 8 GiB of OS files.
  Docker reports about **4.4 GB** of image layers, a third, different metric.
- The stripped `solyra-comp` binary is **28.5 MB** on disk. That is not its
  memory use.

## 2026-11-18 — idle memory and session cost (QEMU x86_64, 1.8 GiB guest)

Measured inside a cold-booted disk by reading `/proc/meminfo` from the running
session (`MemTotal - MemAvailable`), with the fixture recording frames:

| State | Used |
| --- | --- |
| Empty niri desktop, session services up | **341 MiB** |
| Foot + Thunar tiled, both drawn | **391 MiB** |

Both figures are the whole guest: kernel, systemd, NetworkManager, udisks2,
pipewire, the compositor, and the apps. The 400 MiB idle budget in PLAN.md §7
therefore holds with room to spare, but this is a VM with software rendering,
not a hardware measurement. Compositor RSS and boot-to-first-frame are still
unmeasured.
