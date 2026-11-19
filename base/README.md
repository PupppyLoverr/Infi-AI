# Solyra base image

The development OS image is built from Fedora bootc. The local VM disk is
for desktop bring-up; it does not validate installation, updates, or rollback.

## Build locally

From the repository root, build the vendored compositor first:

```sh
sudo docker build -f comp/dev.Containerfile -t solyra-dev:dev comp/
mkdir -p comp/out
sudo docker run --rm -v "$PWD/comp:/src" solyra-dev:dev sh -ec '
  CARGO_PROFILE_RELEASE_STRIP=symbols CARGO_PROFILE_RELEASE_LTO=off \
    cargo build --locked --release -p niri -j 2
  install -m 755 target/release/niri out/niri
'
sudo docker build -f base/Containerfile -t solyra-base:dev .
```

## Produce a bootable test disk

`sudo ./scripts/build-disk.sh dist/solyra.raw` builds a local x86_64 UEFI
test disk from `solyra-base:dev`. It requires Docker, Skopeo, XFS tools,
dosfstools, mtools, `sfdisk`/`losetup`, and x86_64 EFI GRUB modules. It
requires a new output path and writes a development-only GRUB config; use
OVMF without Secure Boot for the VM. Installer media remains a later phase.

Provision a user with a home directory, then start `solyra-session --session`
from that user's active local TTY, or select **Solyra OS** in a Wayland-capable
display manager. The development image does not set a default password or
ship an autologin service. To override shortcuts and appearance, copy
`/etc/solyra/comp.kdl` to `~/.config/solyra/comp.kdl`. Super+Enter opens Foot,
Super+E opens Thunar, Super+D opens Fuzzel, and Super+Tab opens the workspace
overview.

## Get a ready-made disk (CI)

The `disk` workflow builds the image, builds the disk, boots it headless once as
a smoke test, and uploads `solyra.raw.zst` as the `solyra-disk` artifact
(Actions -> latest `disk` run -> Artifacts, kept 14 days). Then:

```sh
sha256sum -c SHA256SUMS
./run-vm.sh solyra.raw.zst        # needs QEMU, OVMF, zstd; KVM recommended
```

The development disk logs in automatically as `solyra` (password `solyra`). It
is a test image, not a release: no Secure Boot, no installer, no updates tested.
The smoke test proves boot to a serial login prompt only; the graphical desktop
needs a GL-capable virtual GPU, which `run-vm.sh` configures (VirGL).

Everyday programs in the image: Foot, Thunar, Firefox, Mousepad, NetworkManager
(`nmtui` from the control centre), PipeWire/WirePlumber. Keys: Super+Enter
terminal, Super+E files, Super+B browser, Super+D launcher, Super+Shift+A agent
tasks.

### VMware Workstation / Player (Windows)

The same run also uploads `solyra-vmware`: `solyra.vmdk` (the raw disk
converted by `qemu-img`; CI checks every byte reads back identical with
`qemu-img compare`), a ready `solyra.vmx` and `README.txt`. Unzip, then File >
Open `solyra.vmx`. The VM must use **UEFI with Secure Boot off** and **3D
acceleration on** (both set in the `.vmx`): niri refuses software rendering,
so without VMware's 3D (vmwgfx) the desktop never draws. `open-vm-tools` is in
the image. Full steps and troubleshooting: [vmware/README.txt](vmware/README.txt).

## Known limits of this image

- niri refuses software EGL renderers, so a VM needs a virtual GPU with 3D
  support (`-device virtio-vga-gl`, VirGL). With a plain `virtio-vga` the
  session starts and opens its Wayland socket but never draws a frame.
- Login is greetd with a text greeter (`agreety`); there is no graphical
  greeter or first-boot wizard yet.
- `systemd-oomd` and `systemd-resolved` log "Failed at step USER" during early
  boot on this image and need their system users checked.

## Measure (every milestone)

```sh
sudo docker image inspect solyra-base:dev --format '{{.Size}}'
```

Idle RAM and boot time are measured on the booted image in QEMU — the
`tests/drive` harness takes over once it is rewired for this layout.
