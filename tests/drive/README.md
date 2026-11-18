# Drive harness (kept from CosmosOS v1–v6)

QEMU QMP tooling that human-drives a guest OS: pointer moves/clicks/drags
(`qmp.sh`), screenshots (`shot.sh`), and per-feature check scripts. This is the
engine-agnostic part of the old test stack — it verified real boots with
screendumps, and Solyra reuses it from day one.

## Point the input and capture helpers at Solyra

Boot the [development disk](../../base/README.md) in QEMU and start a Solyra
session. For headless keyboard input, add `-device virtio-keyboard-pci` to
the VM and give QEMU separate Unix sockets for `-qmp` and `-monitor`.
Use a virtual GPU with 3D support for niri's native DRM backend; an
unaccelerated virtio GPU can reach the Wayland socket without drawing a frame.

Set `QMP_SOCK` for `qkey.sh` and `qmp.sh`. Set `MON_SOCK` and `DRIVE_DIR`
for `shot.sh`. From an empty desktop:

```sh
tests/drive/qkey.sh meta_l-ret  # Foot
tests/drive/qkey.sh meta_l-e    # Thunar, beside Foot
tests/drive/shot.sh solyra-tiling
tests/drive/qkey.sh meta_l-tab  # Workspace overview
tests/drive/shot.sh solyra-overview
```

The per-feature scripts still target CosmosOS's old layout and tooling.
Reusing the input and screenshot helpers does not port the old feature tests.

CI gates these scripts at `shellcheck -S warning`; info-level findings
(quote-splitting, `&& ||` notes, unreachable-command hints) are deliberately
not gated until the harness is rewired and actually driven again.
