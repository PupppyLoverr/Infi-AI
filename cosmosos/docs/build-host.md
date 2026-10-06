# CosmosOS build host

Reference environment for building `cosmosos/cosmos` and assembling the
CosmosOS disk image. Provisioned by `provision/host-deps.sh` (idempotent —
run it on any fresh Ubuntu 22.04 x86_64 box).

## Verified environment

| Item | Value |
|---|---|
| OS | Ubuntu 22.04.5 LTS (jammy) |
| Kernel | 6.8.0-1061-aws |
| Arch | x86_64 |
| Virtualization | `/dev/kvm` present (HVM, near-native guests) |
| RAM | 32 GB |
| Rust | rustup, stable 1.97.1 (`rustc 1.97.1 8bab26f4f 2026-07-14`) |
| QEMU | 6.2.0 (`qemu-system-x86`, OVMF, swtpm 0.6.3) |
| mmdebstrap | 0.8.4 |

## What the script installs

- **QEMU + firmware**: `qemu-system-x86`, `qemu-utils`, `ovmf` (UEFI boot),
  `swtpm` (TPM 2.0 emulation, kept for later secure-boot experiments).
- **Image assembly**: `mmdebstrap`, `parted`, `dosfstools`, `e2fsprogs`,
  `squashfs-tools`, `mtools`, `xorriso`.
- **Base tooling**: `pkg-config`, `git`, `curl`, `rsync`,
  `build-essential`, `clang`, `libclang-dev` (bindgen), `meson`,
  `ninja-build`.
- **Smithay/anvil native deps**: `libudev-dev`, `libxkbcommon-dev`,
  `libwayland-dev`, `libdrm-dev`, `libgbm-dev`, `libegl1-mesa-dev`,
  `libgles2-mesa-dev`, `libinput-dev`, `libseat-dev`, `libsystemd-dev`
  (required by `libseat.pc`), `libdbus-1-dev`.
- **winit/X11**: `libxcb1-dev`, `libxcb-render0-dev`, `libxcb-shape0-dev`,
  `libxcb-xfixes0-dev`, `libxcb-keysyms1-dev`, `libxkbcommon-x11-dev`,
  `libfontconfig-dev`, `libssl-dev`.

## Non-apt dependency: libdisplay-info

Ubuntu 22.04 does **not** package libdisplay-info (first appeared in
22.10). The compositor needs it through `smithay-drm-extras`'s
`display-info` feature — `udev.rs` calls
`display_info::for_connector(...)` for EDID probing.

`libdisplay-info-sys 0.2.2` requires `>= 0.1.0, < 0.3.0`, so
`host-deps.sh` builds **0.2.0** from
`gitlab.freedesktop.org/emersion/libdisplay-info` and installs it into
`/usr/local` (meson + ninja). Idempotent: skipped when `pkg-config`
already finds a compatible version.

## Known workspace state (as of first provision)

`cargo check --workspace` on the committed tree fails at **manifest
parse**, before any compilation:

```
error: failed to load manifest for workspace member `cosmos/compositor`
Caused by:
  feature `udev` includes `smithay-drm-extras`, but `smithay-drm-extras`
  is not an optional dependency
```

With a two-line manifest change (applied in a scratch copy only — the
`cosmos/` tree is owned by the lead dev):

```toml
smithay-drm-extras = { version = "0.1", features = ["display-info"], optional = true }
```

(`features = ["edid"]` is also wrong — `smithay-drm-extras 0.1.0` has no
`edid` feature; available: `default`, `display-info`, `libdisplay-info`.)

…the entire workspace then `cargo check`s cleanly in ~19 s. Remaining
output is warnings only: ~113 `unexpected cfg condition value` warnings
for undeclared `debug` and `xwayland` features in `compositor/src/` —
pre-existing code noise, not blockers.
