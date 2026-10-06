# CosmosOS

A real, bootable Linux-based operating system where the entire desktop
experience — compositor, shell, toolkit, apps — is Cosmos. Linux provides
the kernel, drivers, systemd, and userspace; everything the user sees and
touches is ours.

This is the "Linux foundation" half of a two-part experiment: what
CosmosOS looks like when engineering effort goes into the Cosmos
experience instead of re-writing a kernel and userland from scratch.
(A scratch Rust kernel+userspace sibling lives in `CosmosOS/` at the repo
root, if present.)

## Architecture

```
Kernel/drivers/systemd (Debian trixie)
        │
cosmos-compositor   Smithay Wayland compositor. DRM/KMS + GBM on real
        │           hardware/QEMU (--tty-udev) or nested (--winit).
        │           Workspaces, tiling snaps, SSD chrome (titlebars drawn
        │           in-process via tiny-skia), cosmos-ipc broadcast socket.
        │
cosmos-shell        Layer-shell panel + searchable launcher + notification
        │           daemon (org.freedesktop.Notifications over D-Bus) +
        │           status area (clock, real D-Bus network/audio/battery
        │           state). Pure shm client, draws with pixman.
        │
cosmos-uitk         Shared app substrate: sctk Wayland plumbing + egui
        │           event bridge + software rasterizer → wl_shm buffers.
        │           One call: cosmos_uitk::run(title, app_id, min, |ui|).
        │
apps/               cosmos-terminal  real PTY (portable-pty + vt100)
                    cosmos-files     real read_dir browsing + xdg-open
                    cosmos-editor    real load/save text editor
                    cosmos-settings  ~/.config/cosmos/config.toml + live IPC
                    cosmos-monitor   real /proc stats, sparklines
```

Everything is real: no mocked data, no fake states, no dead buttons.

## Boot flow

```
bootloader → kernel → systemd → agetty --autologin cosmos (tty1)
    → .profile → cosmos-session
        → cosmos-compositor --tty-udev   (logind/libseat seat0)
        → waits for $XDG_RUNTIME_DIR/wayland-1
        → cosmos-shell &
```

## Build & run

Host requirements and image build live in `image/build.sh`
(Debian trixie rootfs via mmdebstrap) and `image/run.sh` (QEMU +
virtio-vga + KVM). See `docs/image.md`.

On any Linux Wayland session you can run the desktop nested:

```sh
cd cosmos && cargo build --workspace
cargo run -p cosmos-compositor -- --winit   # inside a Wayland session
WAYLAND_DISPLAY=wayland-1 cosmos-shell &
WAYLAND_DISPLAY=wayland-1 cosmos-files &
```

## Layout

- `cosmos/` — Rust workspace (compositor, shell, uitk, ipc, apps).
- `image/` — disk-image build + QEMU run scripts.
- `provision/` — build-host provisioning.
- `tests/` — QEMU smoke tests.
- `docs/` — build-host, image, GPUI-investigation notes.
- `PLAN.md` — the original 4-day plan.
