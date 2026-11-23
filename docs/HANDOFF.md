# Solyra OS — handoff

Everything a new contributor or agent needs to pick this up without reading the
thread. Repo: <https://github.com/PupppyLoverr/Infi-AI>, branch `main`.

## What this is

A Linux desktop distribution built as a container image:

- **Base:** Fedora bootc (atomic, image-based OS). `base/Containerfile`.
- **Compositor:** `solyra-comp`, a fork of niri 26.04 (`comp/`, vendored source
  at `f73c145`). Scrollable tiling, gradient borders, dual-kawase backdrop
  blur, per-surface layer rules.
- **Shell:** `solyra-panel` (`shell/panel`, pure Rust, ~1.5 MB). Five Wayland
  layer surfaces: bar, dock (three variants), launcher, control centre,
  expanded island.
- **Look:** flat warm-neutral surfaces with hairlines; overlays faintly
  translucent over a blurred wallpaper; one accent (`#B48AAD`) that only means
  *active*.

## What works today

Verified by the harnesses below, not by inspection:

| Area | State |
| --- | --- |
| Boot | x86_64 UEFI disk built from the image boots to a TTY (QEMU + OVMF) |
| Desktop | Cold boot reaches a live niri session: DRM output, Wayland + X11 sockets, wallpaper, bar, dock |
| Tiling | Windows tile below the bar (exclusive zone), focus moves, columns swap, overview works |
| Shell input | Real pointer + keyboard: workspace pills switch workspace, dock cells focus/launch, launcher filters as you type, control centre sliders/toggles persist |
| Glass | Bar, dock, launcher, control centre, island are translucent and blurred by the compositor |
| Size | 3.0 GB deployed root before first boot (budget ≤ 4 GB) |
| Idle RAM | ~341 MiB whole guest in a 1.8 GiB VM (budget ≤ 400 MiB) |

Added since, covered by `cargo test` (not yet run on a live niri session): token
module with light theme, grouped launcher, real battery/network readouts, agent
task card and approvals over a file protocol (`agent-protocol.md`). The old
control-centre Wi-Fi/Bluetooth toggles and the "Simulate Agent" button were
removed because they did not act on the system.

Not built yet: display manager, first-boot wizard, status-cluster popovers
beyond the control centre, notifications, lock screen theming, snap zones,
first-party apps (terminal/files/settings in zgx), agent sandbox, installer,
ARM64, Secure Boot, update/rollback.

## Repo map

```
base/            Containerfile, session config (solyra-config.kdl), foot theme,
                 sysusers/sudoers, session launcher, wayland-session entry
comp/            vendored niri fork (solyra-comp) + dev container
shell/panel/     the shell: canvas.rs (2D renderer), state.rs (IPC/apps/prefs),
                 surfaces.rs (all painters), main.rs (Wayland)
scripts/         build-disk.sh (image → bootable raw disk)
docs/            design-language.md (the spec), apps.md, setup.md,
                 agent-platform.md, decisions.md (ADRs), budgets.md
tests/drive/     QEMU QMP helpers (legacy; see its README)
assets/wallpapers/  five 1080p + five 4K originals (only 1080p ships)
```

## Build

```sh
# 1. Compositor + shell binaries (repo root, needs Docker)
sudo docker build -f comp/dev.Containerfile -t solyra-dev:dev comp/
mkdir -p comp/out shell/out
sudo docker run --rm -v "$PWD/comp:/comp" -w /comp solyra-dev:dev sh -ec '
  CARGO_PROFILE_RELEASE_STRIP=symbols CARGO_PROFILE_RELEASE_LTO=off \
    cargo build --locked --release -p niri -j 2
  install -m 755 target/release/niri out/niri'
sudo docker run --rm -v "$PWD:/repo" -w /repo/shell/panel solyra-dev:dev sh -ec '
  cargo build --locked --release
  install -m 755 target/release/solyra-panel ../out/solyra-panel'

# 2. OS image
sudo docker build -f base/Containerfile -t solyra-p2:desktop .

# 3. Bootable disk (8 GiB raw, UEFI). SOLYRA_AUTOLOGIN=1 boots to the desktop.
sudo env TMPDIR="$PWD/.amp/in/orb-backup" SOLYRA_IMAGE=solyra-p2:desktop \
  SOLYRA_AUTOLOGIN=1 ./scripts/build-disk.sh dist/solyra.raw
```

`build-disk.sh` needs: docker, skopeo, losetup/sfdisk, mkfs.vfat, mkfs.xfs,
xfs_admin, mtools, grub-mkimage (x86_64-efi). It refuses to overwrite an
existing path.

## Run the disk in a VM

The compositor needs a GPU-style device: niri rejects software EGL, so use
**virtio-vga-gl (VirGL)**, not plain `-vga`. With `SOLYRA_AUTOLOGIN=1` the
desktop comes up on its own.

```sh
qemu-system-x86_64 \
  -accel kvm -cpu host -m 4096 -smp 4 \
  -vga none -device virtio-vga-gl \
  -display gtk,gl=on \
  -device qemu-xhci -device usb-tablet -device virtio-keyboard-pci \
  -device virtio-rng-pci \
  -drive if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE_4M.fd \
  -drive if=pflash,format=raw,file=OVMF_VARS_4M.fd \
  -drive file=dist/solyra.raw,format=raw,if=virtio
```

- No KVM (nested VM): drop `-accel kvm -cpu host`, add `-accel tcg,thread=multi
  -cpu max`; expect a slow boot.
- Copy `OVMF_VARS_4M.fd` from `/usr/share/OVMF/` first; QEMU needs a writable
  copy.
- Without `-device virtio-vga-gl` the session starts but never draws a frame.
- No password is set on the dev image. Without autologin, log in on the
  console only after setting one (`sudo passwd solyra` from a root shell).

## Verify a change

```sh
# compositor config validity
sudo docker run --rm -v "$PWD/base/solyra-config.kdl:/tmp/c.kdl:ro" \
  --entrypoint /usr/bin/solyra-comp solyra-p2:desktop validate -c /tmp/c.kdl

# shell, with a real pointer and keyboard: Xvfb -> weston -> nested niri
bash .amp/in/orb-backup/shell-test.sh     # asserts workspace switch + dock launch
bash .amp/in/orb-backup/glass-test.sh     # captures bar/dock/launcher/control
```

Those harnesses live in `.amp/in` (gitignored) because they depend on host
tooling; port them to `tests/` if they should be permanent.

## Added after the first handoff (see git log for detail)

- `crates/solyra-tokens`, `crates/solyra-conf`: shared by the panel and apps.
- `apps/`: zgx apps (`solyra-settings`, `solyra-term`, `solyra-browse`,
  `solyra-setup`), self-tested in CI under headless Wayland.
- `agent/`: `solyra-agent` (Claude Code permission hook, Codex status hook, PTY
  wrapper, bubblewrap sandbox with snapshot undo). Protocol in
  `docs/agent-protocol.md`.
- Panel: HiDPI (integer scales), notifications (org.freedesktop.Notifications),
  custom wallpapers, agent questions/replies in the notch.
- CI: `.github/workflows/disk.yml` builds everything, boots the disk headless
  to a login prompt, and uploads `solyra.raw.zst`.

## Known gaps and traps

- **Display manager landed.** greetd runs `solyra-greet` (zgx) under cage on
  tty1; it speaks the greetd IPC protocol and starts
  `solyra-session --session`. The dev disk still adds the autologin
  `initial_session` (`SOLYRA_AUTOLOGIN=1`); the release image does not.
- **niri needs a real renderer.** Software EGL is rejected by design; VMs need
  VirGL, bare metal needs any GPU with EGL.
- **`systemd-oomd` / `systemd-resolved`** log `Failed at step USER` early in
  boot on first boot; unresolved, harmless so far.
- **Blur needs GL.** On llvmpipe it works but costs CPU; measure before
  enabling more passes.
- **The shell is a client.** Killing `solyra-panel` loses chrome, never
  windows. Restart it with `solyra-panel &`.
- **Preferences** live in `~/.config/solyra/shell.conf` (dock style, wallpaper,
  volume, brightness). Roles live in `/etc/solyra/defaults.toml`.
- **Vercel** fails on every push; it is an unrelated external project. Ignore
  it. GitHub Actions `checks` and `image-build` are the gates.

## Shell control surface

```sh
solyra-panel notify "Agent · tidying Downloads"   # island message (10 s)
solyra-panel toggle-launcher                       # Super+D / Super+Space
solyra-panel toggle-control                        # Super+Shift+C
solyra-panel toggle-island                         # Super+Shift+I
solyra-panel toggle-agents                         # Super+Shift+A
solyra-panel dock macos|windows|solyra             # switch dock variant
solyra-panel wallpaper ocean|sunset|forest|graphite|violet
solyra-panel close                                 # dismiss overlays
```

## Next steps, in order

1. **Snap zones + Snap Assist** in `comp/` (compositor work; the plan's P2
   gate). Edge-drop and `snap-left`/`snap-right` actions plus the
   `SnapAssistOpened`/pick/dismiss IPC are in; the panel-side assist card and
   floating-tile snapping remain.
2. ~~**Display manager**~~ — done: greetd + cage + `solyra-greet`.
3. ~~**First-boot wiring**~~ — done: greetd `initial_session` runs
   `solyra-setup` under cage as `solyra-setup` until `setup-done` exists.
   Remaining: the Flatpak gaming bundle inside the wizard (`docs/setup.md`).
4. **Terminal in zgx** (`docs/apps.md`) — exists as `solyra-term` (tabs,
   panes); remaining P4 work is the daily-driver checklist on a real boot.
5. ~~**Agent sandbox level 1**~~ — done and verified on Linux: dedicated
   `solyra-agent` user + bubblewrap, `space`/`wayland` policy keys, Agent
   Space on workspaces 7–9, snapshot/undo for agent-owned workspaces,
   opencode bridge verified end-to-end. Remaining: rollback demo gate and
   the panel-side snap-assist card above.
