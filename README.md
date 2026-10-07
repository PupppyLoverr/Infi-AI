# CosmosOS

A Linux distribution that boots straight into **Cosmos** — a custom desktop
environment blending the best of Windows 11 and macOS into one coherent,
restrained design.

Everything lives in [`cosmosos/`](cosmosos/):

- `cosmosos/cosmos/` — the Rust desktop stack: a Smithay-based Wayland
  compositor (workspaces, snap tiling, macOS-style chrome), a layer-shell
  shell (menubar, dock, Spotlight/Start launcher, Quick Settings,
  notifications, Snap Assist, window switcher), the `cosmos-uitk` egui
  toolkit, and the native apps (terminal, files, editor, settings, system
  monitor).
- `cosmosos/image/` — the distro pipeline: `build.sh` produces a bootable
  raw x86_64 disk image (mmdebstrap trixie + GRUB) and `run.sh` boots it in
  QEMU/KVM.
- `cosmosos/docs/` — design docs (UI/UX blend decisions), the image build
  guide, and security posture.
- `cosmosos/tests/` — the QMP drive harness that human-drives the guest OS
  in QEMU.

## Quick start

On an x86_64 Linux host:

```sh
cd cosmosos/image
./build.sh   # → dist/cosmosos-x86_64.img
./run.sh     # boots it under KVM
```

See [`cosmosos/PLAN.md`](cosmosos/PLAN.md) for the roadmap and
[`cosmosos/docs/`](cosmosos/docs/) for design and build details.
