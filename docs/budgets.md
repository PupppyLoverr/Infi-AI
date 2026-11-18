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
