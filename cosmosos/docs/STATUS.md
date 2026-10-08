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
menubar (logo + focused app + workspaces + clock + tray icons), LEFT-edge
vertical dock (pinned+running apps, magnification, hover labels, separators —
exclusive zone reserved so windows never overlap it), Spotlight-style
launcher (search + pinned grid + recommended), Quick Settings flyout
(volume slider via wpctl, network toggle via NM D-Bus, accent dial, dark/light
pill), notification daemon, Snap Assist card, zoom flyout (Win11 snap-layouts
card on green-button dwell → snap into thirds/quarters/wides/max), super+?
cheatsheet, window switcher. Inter + JetBrains Mono throughout.
Dynamic island (PR #101): centred menubar pill (clipboard snippet +
staged-file badge) expanding into a `cosmos-island` card — CLIPBOARD
history ring of 10 fed by a wlr-data-control watcher + STAGED FILES
via text/uri-list drops; uitk apps publish/consume a real clipboard
(copy-paste source + egui Paste events).

**cosmos-portal** (`cosmos/portal`) — xdg-desktop-portal backend
`org.freedesktop.impl.portal.desktop.cosmos`: Screenshot (compositor IPC →
GL offscreen PNG), PickColor (centre pixel), FileChooser (cosmos-files
`--chooser` picker, real file paths), ScreenCast (real PipeWire producer
node streaming BGRx frames captured via IPC screenshots). `.portal` +
`.service` data files ship in `portal/data/`; image wiring in flight.

**cosmos-uitk** (`cosmos/uitk`) — egui toolkit: sctk plumbing + software
rasterizer (per-pixel barycentric), release-gated shm buffers, per-cell ANSI
terminal rendering lives in apps/terminal.

**Apps** (`cosmos/apps`) — terminal (vt100 + pty + 256/truecolor + mouse +
bracketed paste + `-e`), files, editor, settings, monitor.

**Verified on guest (drive harness, QEMU screendumps)**: colours, shadows,
launcher/flyout/dock, snap+assist, focus chrome, notifications, network toggle,
opencode/htop/nvim rendering, zero-panic soaks. Budgets @17244cf: image 1.6G
real / 3.0G sparse, idle RAM ~330–410MiB, boot→panel 5.4s.

**Compositor gaps closed since the audit**: lazy XWayland via
`xwayland_shell` + satellite-owned XWM (PR #91), fractional output scale
125/150% honored (PR #92), IPC screenshot capture (PR #93), portal backend
Screenshot/PickColor/FileChooser/ScreenCast (PRs #94–#96), GPT+btrfs
subvolumes+snapper disk layout (PR #97, budgets 1.8G real / ~242MiB idle /
6.68s boot). Snap zones extended to 13 states with one shared geometry
table — the zoom flyout's layout thumbnails and the compositor's snap
rects can't drift (shared `cosmos_ipc::SNAP_LAYOUTS`). Remaining:
multi-monitor hotplug guest proof, virtio-gpu real modes, portal image
wiring — all in flight on the image host.

## Papercuts — fixed and boot-verified (PR #88)
Hostname `cosmosos` (DMI product name `CosmosOS` + PS1 fix), en_US.UTF-8
generated + defaulted via update-locale, zram lz4 @ 50% RAM, QEMU-follow
resolution via `run.sh GUEST_RES=WxH` (virtio-vga xres/yres, verified
1024x768 + 1280x720). Evidence: `docs/evidence/01-papercuts.png`.

## Direction (post Phase-1 audit)
Fork of pop-os/cosmic-comp CANCELLED — our compositor stays; cosmic-comp is
a read-only lift catalog (see `docs/LIFT.md`). Shell stays egui on uitk;
GPU render path (egui-wgpu) added by tier with the software rasterizer as
the Lite fallback. Disk layout is GPT + btrfs subvolumes + snapper (done,
PR #97). Dock is left-edge vertical (done, PR #90). Feature ladder: zoom
flyout (done, PR #99) → dynamic island (done, PR #101 — guest
verification in flight) → Search-or-Ask launcher → Start
panel+widgets → Control Centre → greetd+lock → cosmos-agentd →
ISO+Calamares.

## What's broken / open
- Snap Assist tray-click is fixed in code (bbox pass-through #50, clipped
  input region + dismiss-on-outside-press #51, free-half respects the left
  rail #90); regression-covered permanently by
  `tests/drive/assist-tray.sh` (IPC-asserted every round).
- llvmpipe-only known limits: no live blur until the GPU shell path lands.

## Build
`cosmosos/image/build.sh` on an x86_64 Linux host (provision via
`provision/host-deps.sh`) → `dist/cosmos.raw`; `run.sh` for KVM/TCG boot
(`GUEST_RES` for resolution, OVMF for the GPT/UEFI image).
Rust workspace compiles via `cargo check --workspace` in `cosmos/`.
