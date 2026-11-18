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

## 2026-11-18 — P1 GATE: first bootable Solyra disk (QEMU x86_64, UEFI)

- `scripts/build-disk.sh` assembles an 8 GB raw disk: GPT with 512 MiB ESP
  (vfat, written via mtools) + xfs root; `bootc install to-filesystem`
  deploys the ostree commit; a hand-built GRUB core image
  (`grub-mkimage -O x86_64-efi`, prefix on the ESP) boots it.
- **Boot verified in QEMU (TCG, OVMF): "Welcome to Solyra OS 0.1
  (Perihelion)" → `Reached target multi-user.target`, NetworkManager up.**
- Deployed rootfs on disk: **2.0 GB** (`du` of the deployment + repo, xfs
  reflink dedupe included) — comfortably inside the ≤ 4 GB budget.
- Boot-to-multi-user under TCG: ~290 kernel-seconds ≈ 4 min wall clock
  (TCG runs ~5–8x slower than real hardware; real-hardware number pending
  until we can run with KVM — orb kernels have no KVM).
- Known gaps, tracked for P2/P6:
  - serial getty does not spawn a login prompt with `console=ttyS0`
    (VGA getty works; the setup wizard replaces console login anyway);
  - bootc's own bootloader (bootupd) step cannot run in the orb (no EFI
    firmware, no partitioned loop devices) — our GRUB core covers booting;
    revisit when building real installer media (P7).
- The niri vendored tree (`comp/`) now **typechecks cleanly** (`cargo check
  -p niri` in `comp/dev.Containerfile`, which needed cairo-gobject-devel +
  pango-devel) — the P2 fork baseline.
