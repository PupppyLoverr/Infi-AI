# CosmosOS — Status

Current state of the tree. Updated at each phase boundary.

## What exists

**Base OS** — Debian 13 (trixie) rootfs assembled by `image/build.sh` (mmdebstrap →
msdos/ext4/GRUB raw image), `image/run.sh` boots it in QEMU. Guest boots to an
autologin session running our own Wayland stack. opencode, foot, firefox-esr and
the curated CLI set (ripgrep/fd/fzf/eza/bat/btop/fastfetch/neovim/tmux/lazygit)
are packaged into the image; NetworkManager + polkit + netdev group are wired
for the tray toggle.

**cosmos-compositor** (`cosmos/compositor`, ~13.5k LOC Rust on Smithay 0.7) —
udev (DRM/GBM, llvmpipe-capable) + winit backends, xdg-shell + layer-shell,
SSD chrome (macOS traffic lights, centered title), workspaces 1-9, snap L/R +
maximize + drag-edge preview + Snap Assist picker, master+stack tiling
(Super+T), minimize/show-desktop, motion system (fade/scale/slide), accent
theme presets, drop shadows, vignette wallpaper, IPC socket (cosmos-ipc),
QMP drive harness instrumentation.

**cosmos-shell** (`cosmos/shell`) — layer-shell surfaces in one process:
menubar (logo + focused app + workspaces + clock + tray icons), bottom dock
(pinned+running apps, magnification, hover labels, separators), Spotlight-style
launcher (search + pinned grid + recommended), Quick Settings flyout
(volume slider via wpctl, network toggle via NM D-Bus, accent dial, dark/light
pill), notification daemon, Snap Assist card, super+? cheatsheet, window
switcher. Inter + JetBrains Mono throughout.

**cosmos-uitk** (`cosmos/uitk`) — egui toolkit: sctk plumbing + software
rasterizer (per-pixel barycentric), release-gated shm buffers, per-cell ANSI
terminal rendering lives in apps/terminal.

**Apps** (`cosmos/apps`) — terminal (vt100 + pty + 256/truecolor + mouse +
bracketed paste + `-e`), files, editor, settings, monitor.

**Verified on guest (drive harness, QEMU screendumps)**: colours, shadows,
launcher/flyout/dock, snap+assist, focus chrome, notifications, network toggle,
opencode/htop/nvim rendering, zero-panic soaks. Budgets @17244cf: image 1.6G
real / 3.0G sparse, idle RAM ~330–410MiB, boot→panel 5.4s.

## What's broken / papercuts
- Hostname shows the QEMU machine name; locale is C; no swap (needs zram);
  resolution fixed at 1024x768 (needs virtio-gpu resize/modes).
- Snap Assist card can eat a tray click while open (assist dismiss works).
- llvmpipe-only known limits: no live blur (tier system planned).

## Build
`cosmosos/image/build.sh` on an x86_64 Linux host (provision via
`provision/host-deps.sh`) → `dist/cosmos.raw`; `run.sh` for KVM/TCG boot.
Rust workspace compiles via `cargo check --workspace` in `cosmos/`.
