# CosmosOS disk image

`image/build.sh` produces a bootable raw x86_64 image at
`dist/cosmosos-x86_64.raw` (sparse, nominal 3G). `image/run.sh` boots it
in QEMU with KVM. Verified on the Ubuntu 22.04 x86_64 build host
(kernel 6.8, /dev/kvm, 32 GB).

## build.sh pipeline

1. **`cargo build --release --workspace`** in `cosmosos/cosmos`
   (`SKIP_CARGO_BUILD=1` to skip). All seven binaries are required:
   cosmos-compositor, cosmos-shell, cosmos-files, cosmos-terminal,
   cosmos-editor, cosmos-settings, cosmos-monitor.
2. **Overlay staging** into `work/image/overlay`:
   - binaries → `/usr/local/bin/`
   - `cosmosos/cosmos/apps/*/cosmos-*.desktop` → `/usr/share/applications/`
   - `/usr/local/bin/cosmos-session` — session wrapper (below)
   - `/etc/profile.d/99-cosmos-session.sh` — `exec`s the wrapper on tty1
   - `getty@tty1.service.d/autologin.conf` — `agetty --autologin cosmos`
   - `/etc/hostname` = `cosmosos`
3. **`mmdebstrap --variant=required` Debian trixie** rootfs with the
   package list below, then a `setup.sh` customize hook (user creation,
   service enables, resolv.conf symlink). NB: mmdebstrap hooks run on the
   HOST with `$1` = rootfs — guest commands go through `chroot "$1"`.
4. **Image assembly**: `truncate` sparse raw → `parted` msdos + one bootable
   ext4 → `losetup -P` → `mkfs.ext4` → rsync rootfs → host `grub-install
   --target=i386-pc` → hand-written `/boot/grub/grub.cfg` (kernel +
   initrd + `root=/dev/vda1 rw console=tty0 console=ttyS0,115200
   systemd.journald.forward_to_console=1`).

## Package list rationale

| Group | Packages | Why |
|---|---|---|
| Boot/system | `systemd-sysv udev dbus libpam-systemd kmod linux-image-amd64 initramfs-tools login` | PID1, udev device mgmt+seat tags, logind session (`XDG_RUNTIME_DIR`), kernel |
| Networking | `network-manager systemd-resolved` | NM owns `en*`/`eth*` — the shell tray reads NM over D-Bus. `systemd-networkd` also stays enabled but has **no** `.network` files → it manages nothing; documented fallback if NM is dropped. NM auto-DHCPs wired links ("Wired connection 1", verified `ens5` → 10.0.2.15) and pushes DNS via its systemd-resolved plugin |
| Audio | `pipewire pipewire-alsa wireplumber` | Real backends only — PipeWire runs as a user service under `user@1000`, WirePlumber manages session policy |
| Power | `upower` | Tray battery backend over D-Bus |
| Compositor libs | `libudev1 libxkbcommon0 libwayland-server0 libwayland-client0 libwayland-egl1 libwayland-cursor0 libdrm2 libgbm1 libegl1 libgles2 libinput10 libseat1 libdisplay-info2 libpixman-1-0 libgudev-1.0-0 libdbus-1-3` | From `ldd cosmos-compositor` + smithay dlopen deps |
| GL runtime | `libgl1-mesa-dri` | llvmpipe (swrast) for virtio-gpu — no real GPU, EGL still fully works |
| Misc | `fonts-dejavu-core xdg-utils kbd procps mesa-utils` | Fonts for fontdb/cosmic-text, desktop helpers, console keys, debug tools |

No X11, no other DE, no display manager.

## Session chain (what runs at boot)

```
kernel -> systemd -> getty@tty1 (agetty --autologin cosmos)
  -> login -f cosmos -> PAM (pam_systemd: logind session on seat0,
     XDG_RUNTIME_DIR=/run/user/1000) -> bash login shell
  -> /etc/profile -> /etc/profile.d/99-cosmos-session.sh
  -> exec /usr/local/bin/cosmos-session
       exec > >(logger -t cosmos-session) 2>&1   # journald -> console
       cosmos-compositor --tty-udev &
       wait for $XDG_RUNTIME_DIR/wayland-* socket (<=20s)
       WAYLAND_DISPLAY=wayland-1 cosmos-shell &
       wait compositor; on exit -> logout -> getty respawns -> loop
```

Seat access: libseat -> logind TakeDevice (the tty login session is the
active session on seat0). Groups `video input render tty` are belt-and-
suspenders on top.

**Logging:** `journald.forward_to_console=1` puts the whole journal on
`/dev/console` = ttyS0 → `run.sh`'s `work/image/serial.log`. Session
stdout goes `logger -t cosmos-session` → journal → serial. (Writing to
`/dev/console` directly as the unprivileged user fails — 0600 root —
which was an actual bug found in smoke: the shell died on `exec`,
agetty respawned, ~invisible loop.)

**Compositor exit behaviour:** log out (chosen over poweroff — logind
denies unprivileged `PowerOff` without polkit). The getty respawn then
restarts the whole session: built-in crash-retry, rate-limited 2s.

## run.sh flags

```
-enable-kvm  -m 1G  -smp 2
-drive file=...,format=raw,if=virtio
-device virtio-vga           # guest /dev/dri/card0+renderD128, DRM/KMS+GBM+EGL
-device virtio-tablet-pci    # absolute pointer
-audiodev none,id=snd0 -device intel-hda -device hda-duplex,audiodev=snd0
-netdev user + virtio-net-pci
-serial file:work/image/serial.log
-monitor unix:work/image/monitor.sock,server,nowait
```

- Display: `DISPLAY_MODE=sdl|gtk|none` (default `sdl` on host :0).
  Headless `none` still works — GPU exists, compositor renders, and you
  can pull frames via the monitor socket:
  `echo "screendump /tmp/x.ppm" | socat - UNIX-CONNECT:work/image/monitor.sock`
- Audio: `virtio-snd-pci` requested upstream but needs QEMU ≥ 7 —
  Ubuntu 22.04's QEMU 6.2 doesn't know the model, so `intel-hda` +
  `hda-duplex` gives the guest a real `snd_hda_intel` card instead.
  `audiodev none` discards samples on this headless host; swap in
  `pa`/`alsa` on a desktop host. Best-effort, as agreed.
- KVM: `/dev/kvm` needs the `kvm` group (host-deps.sh adds the user).
  Without it the script falls back to TCG and warns.

## Smoke result (2026-10-06)

Boot → `graphical.target` in ~5 s → **Cosmos desktop paints** and real
windows compose. Verified via QEMU monitor `screendump` while
`DISPLAY_MODE=none`: panel (cosmos logo, workspaces 1–9,
`Wired connection 1 · Vol 40%` tray — real NM/PipeWire data), then a
Terminal window (titlebar + live `cosmos@cosmosos:~$` prompt) and a
Files window (real `/home/cosmos` listing) rendered over KMS.
NM: `dhcp4 (ens5): address=10.0.2.15`, `dns=systemd-resolved`. upower,
pipewire, wireplumber (user services) all running. Zero panics.

Session chain: wayland-1 socket → IPC socket → libinput + xkb → EGL
on PLATFORM_GBM (llvmpipe) → Output `Virtual-1`/`wl_output` →
modeset `1024x768` → `drm master` → `gpu has no hardware render node —
software rendering on DrmNode{57984,Render}` → EGL HW-accel fallback
(`EGL_WL_bind_wayland_display` unsupported — dmabuf-only clients) →
cosmos-shell connects, `notifications service registered`, renders.

### Opt-in app-window smoke rig

`/usr/local/bin/cosmos-smoke-apps` is installed in every image but
runs only when `/etc/cosmos-smoke-apps` exists. Inject post-build:

```
sudo losetup -fP dist/cosmosos-x86_64.raw && sudo mount /dev/loopNp1 /mnt
sudo touch /mnt/etc/cosmos-smoke-apps && sudo umount /mnt && sudo losetup -d /dev/loopN
```

It launches all five shipped apps (`cosmos-terminal`, `cosmos-files`,
`cosmos-editor`, `cosmos-settings`, `cosmos-monitor`) staggered on the
live compositor, logs `free -m`, probes the IPC socket
(newline-delimited JSON: `{"op":"ping"}`, `{"op":"list_windows"}`,
`{"op":"list_workspaces"}` on `/run/user/1000/cosmos-ipc.sock` via
socat), kills the apps, and re-lists windows to prove cleanup.
Observed verbatim (post-`8efdac9` placement clamp):

```
cosmos: new window id=1..5              # Terminal Files Editor Settings System Monitor
{"type":"pong","pong":{"version":"0.1.0","name":"cosmos-compositor"}}
{"type":"windows","windows":[
 {"id":1,"title":"Terminal","app_id":"cosmos.terminal","x":340,"y":38,"w":680,"h":476,...},
 {"id":2,"title":"Files","app_id":"cosmos.files","x":295,"y":50,"w":560,"h":436,...},
 {"id":3,"title":"Editor","app_id":"cosmos.editor","x":188,"y":268,"w":560,"h":456,...},
 {"id":4,"title":"Settings","app_id":"cosmos.settings","x":371,"y":261,"w":520,"h":516,...},
 {"id":5,"title":"System Monitor","app_id":"cosmos.monitor","x":486,"y":106,"w":600,"h":496,...}]}
{"type":"workspaces","workspaces":[{"id":0,"focused":true,"window_count":5},...x9]}
... post-kill: {"type":"windows","windows":[]}
```

Geometry vs 1024x768: Terminal/Files/Editor fully inside; Settings
bottom edge 777 (9px over) and System Monitor right edge 1086 (62px
over) — clamp improved overflow vs the pre-fix run but two windows
still extend slightly past the output (reported upstream).

### Perf (QEMU -m 1G -smp 2, KVM, llvmpipe)

| Metric | Value |
|---|---|
| kernel → graphical.target | ~5.0 s |
| kernel → DRM modeset 1024x768 | ~5.7 s |
| kernel → first desktop frame | ≤10 s |
| guest RAM used at desktop (+5 apps) | 339 / 967 MiB |
| image file | 772 MiB real (3 GiB sparse) |
| image build time | ~2.5 min (~35 s mmdebstrap w/ SKIP_CARGO_BUILD=1, +~1 min release build) |

Pitfall found in smoke: the shell needs `fontconfig`, not just
`fonts-dejavu-core` — without it `cosmic-text` panics
`no default font found` (shape.rs:275) even though the ttf files
exist. Non-fatal noise remaining: RTKit absent, BlueZ/libcamera SPA
plugins missing (no BT/camera), `/etc/default/locale` missing,
`AB30/AR30/AB24` plane formats unavailable on virtio-gpu.
