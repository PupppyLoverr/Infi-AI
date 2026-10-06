# CosmosOS Linux Edition — Build Plan

Goal: a real, bootable Linux-based OS with the Cosmos desktop environment — compositor, shell, launcher, apps, settings — running on real filesystems, real networking, real process state. No mockups, no stubs.

## 0. Environment reality (important)

The spec assumed a Linux x86_64 dev box. This Devin session is a **macOS arm64 (Apple M4 Pro) VM** — no KVM, no Docker. Plan adapts:

- All Linux work happens inside **QEMU VMs on this Mac** (`brew install qemu`).
- **Builder VM**: Debian aarch64 cloud image, HVF-accelerated (near-native) — compiles Rust, assembles the rootfs, produces the CosmosOS disk image.
- **Test runs**: the produced image boots in a *second* QEMU instance, HVF-accelerated since it's also aarch64.
- **Why aarch64**: x86_64 guests on an arm64 host fall back to TCG emulation (~10-30x slower) — unusable for iteration. All code is arch-agnostic Rust + Debian packages; `build.sh` detects host arch, so the same repo produces an x86_64 image unchanged on a real x86_64 Linux machine.

## 1. Architecture

```
Linux kernel (Debian trixie, minimal)
 └ drivers: virtio-gpu/blk/net, libinput evdev, DRM/KMS
 └ system services: systemd, dbus, NetworkManager, PipeWire+WirePlumber, seatd
 └ cosmos-compositor  (Rust + Smithay — anvil-derived: udev/DRM backend + winit dev backend)
      ├─ server-side decorations (Cosmos window chrome on every client)
      ├─ workspaces 1..9, L/R-half + maximize snapping, z-order, focus, alt-tab
      └─ IPC socket → exposes windows/workspaces/config to shell + apps
 └ cosmos-shell (layer-shell clients, smithay-client-toolkit + egui→shm)
      ├─ panel: workspace indicator, task list, clock, tray (net/volume/battery — real D-Bus)
      ├─ launcher overlay: searches real .desktop apps + system actions
      └─ notification daemon (org.freedesktop.Notifications server)
 └ cosmos-uitk: shared egui theme/widgets + softbuffer presenter (the Cosmos design language)
 └ Cosmos apps (real xdg-toplevel windows):
      cosmos-terminal (alacritty_terminal PTY), cosmos-files, cosmos-edit,
      cosmos-settings, cosmos-monitor, cosmos-gpui-demo (if spike succeeds)
```

## 2. Design language

Monochrome (black/white following light/dark), Inter-family type, small radii, subtle fast motion, server-side chrome, no accent colors. One shared theme crate so every app looks identical — that's what makes it "Cosmos" rather than "Linux desktop with a wallpaper".

## 3. Repo layout (inside `cosmosos/`)

```
cosmosos/
  build.sh          # host → builder VM → image (arch-aware)
  run.sh            # boot image in QEMU (HVF arm64 / KVM|TCG x86_64)
  provision-builder.sh
  cosmos/           # Rust workspace: compositor, uitk, shell, ipc, apps
  image/            # mmdebstrap recipe, package lists, systemd units, .desktop files
  tests/            # QEMU serial smoke tests
  docs/             # ARCHITECTURE.md, APP-DEV.md, GPUI.md, WEB-RUNTIME.md
```

## 4. Day-by-day

- **Day 1**: QEMU + builder VM up; Rust workspace; compositor rendering real windows in winit → then DRM/libinput in the VM; autologin→compositor→panel→launcher→terminal booting end-to-end.
- **Day 2**: Files, Settings (+persisted config), workspaces, snapping, tray status (NetworkManager/PipeWire/UPower), notifications daemon.
- **Day 3**: text editor, system monitor, app-dev docs, perf measurements, smoke tests, first raw disk image end-to-end.
- **Day 4**: stabilization — repeated boot/launch/snap/workspace/fs-op cycles, fix crashes/races/visual bugs, GPUI timeboxed spike (real demo or boundary doc), web-runtime boundary doc, final image + README.

Failure-priority order per spec: bootable > compositor > input > windows > shell > terminal > files > settings > workspaces > net > monitor > editor > polish > GPUI > web.

## 5. Immediate risks

- macOS-side image creation needs a Linux VM intermediary — mitigated by doing *all* image assembly inside the builder VM.
- Smithay/egui layer-shell integration details — mitigated by building shell as separate SCTK layer-shell clients (no in-compositor UI toolkit needed).
- Audio in QEMU — best-effort (PipeWire + virtio-snd); documented honestly if unavailable.

## Status (as of 2026-10-06)

- [x] Day 1 — Rust workspace; Smithay compositor (`--winit` nested + `--tty-udev` real); workspaces, snapping, SSD chrome, cosmos-ipc. **[PR #2]**
- [x] Day 2 — cosmos-shell over layer-shell (panel, searchable launcher, notification daemon, D-Bus status area); workspaces+snapping shipped in PR #2. **[PR #3]**
- [x] Day 3 (partial) — cosmos-uitk + terminal/files/editor/settings/monitor apps; compositor window-id + IPC outbox hardening; `tests/smoke-nested.sh` (PASS); docs. **[PR #4, #5]**
- [~] Day 3 — disk image build on the x86_64 host: in progress.
- [ ] Day 4 — stabilization cycles, perf numbers, final image + report.

Verified nested: `weston --backend=headless → compositor --winit → shell + files + terminal` — real windows/pong over IPC, 5/5 procs alive, no panics.
