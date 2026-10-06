#!/usr/bin/env bash
# build.sh — produce a bootable raw x86_64 CosmosOS disk image.
#
# Pipeline:
#   1. cargo build --release --workspace (skip with SKIP_CARGO_BUILD=1)
#   2. mmdebstrap a Debian trixie minimal rootfs + kernel + runtime deps
#   3. lay down the Cosmos overlay (binaries, .desktop files, session plumbing)
#   4. assemble a partitioned raw image (msdos + ext4, GRUB i386-pc BIOS boot)
#
# Output: cosmosos/dist/cosmosos-x86_64.raw   (sparse, ~3G)
# Boot it with cosmosos/image/run.sh.
#
# Requires the host deps from provision/host-deps.sh plus libpixman-1-dev,
# socat (test tooling) and grub-install (grub-pc-bin on host is optional —
# the host's grub-install writes the image boot sector).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COSMOS="$ROOT/cosmos"
WORK="$ROOT/work/image"
DIST="$ROOT/dist"
OVERLAY="$WORK/overlay"
ROOTFS="$WORK/rootfs"
IMG="${IMG:-$DIST/cosmosos-x86_64.raw}"
IMG_SIZE="${IMG_SIZE:-3G}"
SUITE="${SUITE:-trixie}"
MIRROR="${MIRROR:-http://deb.debian.org/debian}"

# --- 1. build the workspace ---------------------------------------------------

if [ "${SKIP_CARGO_BUILD:-0}" != "1" ]; then
  echo "== cargo build --release --workspace =="
  cargo build --release --workspace --manifest-path "$COSMOS/Cargo.toml"
fi
BINDIR="$COSMOS/target/release"
for b in cosmos-compositor cosmos-shell cosmos-files cosmos-terminal \
         cosmos-editor cosmos-settings cosmos-monitor; do
  [ -x "$BINDIR/$b" ] || { echo "missing binary: $BINDIR/$b" >&2; exit 1; }
done

# --- 2. stage the overlay tree ------------------------------------------------

echo "== staging overlay =="
rm -rf "$OVERLAY"
mkdir -p "$OVERLAY"/{usr/local/bin,usr/share/applications,etc/profile.d,etc/systemd/network,etc/systemd/system/getty@tty1.service.d}

install -m755 "$BINDIR"/cosmos-{compositor,shell,files,terminal,editor,settings,monitor} \
  "$OVERLAY/usr/local/bin/"
install -m644 "$COSMOS"/apps/*/cosmos-*.desktop "$OVERLAY/usr/share/applications/"

# session wrapper: agetty -> login shell on tty1 -> profile.d -> this script.
# compositor exits -> we power off (appliance-style boot-to-desktop).
cat > "$OVERLAY/usr/local/bin/cosmos-session" <<'EOF'
#!/bin/bash
# CosmosOS session wrapper. Runs as user `cosmos` on tty1 via
# /etc/profile.d/99-cosmos-session.sh after agetty --autologin.
# Output goes through logger -> journald; the kernel cmdline sets
# systemd.journald.forward_to_console=1 so it also lands on /dev/console
# (ttyS0 -> run.sh's serial log). Writing to /dev/console directly as an
# unprivileged user fails (0600 root) and would kill this shell.
exec > >(logger -t cosmos-session) 2>&1

export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
export WAYLAND_DISPLAY=""

echo "cosmos-session: starting cosmos-compositor on tty $(tty)"
cosmos-compositor --tty-udev &
COMP_PID=$!

# wait for the wayland socket (smithay picks first free wayland-N)
SOCKET=""
for _ in $(seq 1 100); do
  for s in "$XDG_RUNTIME_DIR"/wayland-*; do
    [ -S "$s" ] && { SOCKET="$s"; break; }
  done
  [ -n "$SOCKET" ] && break
  kill -0 "$COMP_PID" 2>/dev/null || break   # compositor died early
  sleep 0.2
done

if [ -n "$SOCKET" ]; then
  export WAYLAND_DISPLAY="${SOCKET##*/}"
  echo "cosmos-session: wayland socket $WAYLAND_DISPLAY up, starting cosmos-shell"
  cosmos-shell &
  # opt-in smoke rig: present only when /etc/cosmos-smoke-apps flag exists
  # (created post-build by loop-mounting the image; not shipped enabled)
  if [ -f /etc/cosmos-smoke-apps ] && [ -x /usr/local/bin/cosmos-smoke-apps ]; then
    echo "cosmos-session: smoke flag present, starting cosmos-smoke-apps"
    /usr/local/bin/cosmos-smoke-apps &
  fi
else
  echo "cosmos-session: no wayland socket appeared (compositor died?)"
fi

wait "$COMP_PID"
rc=$?
# compositor exit -> log out. agetty's respawn then re-runs autologin and
# restarts the session: a built-in crash-retry loop. (poweroff was tried
# first; logind denies it for the unprivileged user without polkit.)
echo "cosmos-session: compositor exited ($rc) — logging out (session restarts on respawn)"
sleep 2
exit "$rc"
EOF
chmod 755 "$OVERLAY/usr/local/bin/cosmos-session"

# opt-in smoke rig: launches two app clients on the live compositor, logs
# guest `free -m`, probes the IPC socket (ping/list_windows/list_workspaces
# — newline-delimited JSON, see cosmos/ipc), then kills the apps and lists
# windows again to prove cleanup. Installed always; RUNS only when the
# /etc/cosmos-smoke-apps flag file exists — inject it by loop-mounting the
# finished image (`touch mnt/etc/cosmos-smoke-apps`).
cat > "$OVERLAY/usr/local/bin/cosmos-smoke-apps" <<'EOF'
#!/bin/bash
exec > >(logger -t cosmos-smoke) 2>&1
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
export WAYLAND_DISPLAY="${WAYLAND_DISPLAY:-wayland-1}"
IPC="$XDG_RUNTIME_DIR/cosmos-ipc.sock"
probe() { echo "$1" | socat -t 3 - UNIX-CONNECT:"$IPC" || echo "ipc probe failed: $1"; }

echo "cosmos-smoke: warming up for shell"
sleep 6

echo "cosmos-smoke: launching cosmos-terminal pid & cosmos-files"
cosmos-terminal & TPID=$!
sleep 3
cosmos-files & FPID=$!
sleep 8

echo "cosmos-smoke: guest memory (free -m):"
free -m

echo "cosmos-smoke: ipc ping:"
probe '{"op":"ping"}'
echo "cosmos-smoke: ipc list_windows (apps up):"
probe '{"op":"list_windows"}'
echo "cosmos-smoke: ipc list_workspaces:"
probe '{"op":"list_workspaces"}'

sleep 4
echo "cosmos-smoke: killing apps ($TPID $FPID)"
kill "$TPID" "$FPID" 2>/dev/null
sleep 3
echo "cosmos-smoke: ipc list_windows (post-kill):"
probe '{"op":"list_windows"}'
echo "cosmos-smoke: done"
EOF
chmod 755 "$OVERLAY/usr/local/bin/cosmos-smoke-apps"

cat > "$OVERLAY/etc/profile.d/99-cosmos-session.sh" <<'EOF'
# Start the Cosmos session when logging in on tty1.
if [ "$(tty 2>/dev/null)" = "/dev/tty1" ]; then
    exec /usr/local/bin/cosmos-session
fi
EOF

cat > "$OVERLAY/etc/systemd/system/getty@tty1.service.d/autologin.conf" <<'EOF'
[Service]
ExecStart=
ExecStart=-/sbin/agetty --autologin cosmos --noclear %I $TERM
EOF

# NetworkManager owns every en*/eth* link (auto-DHCP "Wired connection") —
# the shell tray reads NM over D-Bus, so NM must hold the real interface.
# systemd-networkd stays enabled but has NO .network files: it manages
# nothing and serves as the documented fallback if NM is ever removed.
# No 20-uplink.network on purpose.

echo "cosmosos" > "$OVERLAY/etc/hostname"

# --- 3. mmdebstrap the rootfs -------------------------------------------------
# split-package reality check (trixie): networkd lives in `systemd`,
# resolved is `systemd-resolved`. No X11, no desktop environment.
# Mesa DRI gives llvmpipe (swrast) so virtio-gpu EGL works without vulkan.

PACKAGES="systemd-sysv udev dbus libpam-systemd kmod \
linux-image-amd64 initramfs-tools systemd-resolved \
network-manager pipewire pipewire-alsa wireplumber upower \
libudev1 libxkbcommon0 libwayland-server0 libwayland-client0 \
libwayland-egl1 libwayland-cursor0 libdrm2 libgbm1 libegl1 libgles2 \
libgl1-mesa-dri libinput10 libseat1 libdisplay-info2 libpixman-1-0 \
libgudev-1.0-0 libdbus-1-3 \
fonts-dejavu-core fontconfig xdg-utils kbd procps mesa-utils socat login"

# in-chroot setup. NOTE: mmdebstrap hooks run on the HOST with $1=rootfs —
# guest commands must go through `chroot "$1"` (a bare useradd here creates
# the user on the build host!). Kept as a file for debuggability.
cat > "$WORK/setup.sh" <<'EOF'
#!/bin/sh
set -e
for g in video input render tty; do
  getent group "$g" >/dev/null 2>&1 || groupadd -r "$g"
done
useradd -m -s /bin/bash -G video,input,render,tty cosmos
systemctl enable systemd-networkd.service systemd-resolved.service || true
ln -sf /run/systemd/resolve/stub-resolv.conf /etc/resolv.conf
rm -f /tmp/setup.sh
EOF

echo "== mmdebstrap $SUITE =="
sudo rm -rf "$ROOTFS"
sudo mmdebstrap --variant=required --architectures=amd64 \
  --include="$PACKAGES" \
  --customize-hook="sync-in '$OVERLAY' /" \
  --customize-hook="copy-in '$WORK/setup.sh' /tmp" \
  --customize-hook='chroot "$1" sh /tmp/setup.sh' \
  "$SUITE" "$ROOTFS" "$MIRROR"

# --- 4. assemble the raw image ------------------------------------------------

echo "== assembling $IMG =="
mkdir -p "$DIST"
MNT="$WORK/mnt"
mkdir -p "$MNT"

truncate -s "$IMG_SIZE" "$IMG"
parted -s "$IMG" mklabel msdos
parted -s "$IMG" mkpart primary ext4 1MiB 100%
parted -s "$IMG" set 1 boot on

LOOP=""
cleanup() {
  [ -n "$LOOP" ] && sudo umount -R "$MNT" 2>/dev/null || true
  [ -n "$LOOP" ] && sudo losetup -d "$LOOP" 2>/dev/null || true
}
trap cleanup EXIT

LOOP="$(sudo losetup -fP --show "$IMG")"
echo "loop: $LOOP"
sudo mkfs.ext4 -q -L cosmosos "${LOOP}p1"
sudo mount "${LOOP}p1" "$MNT"

sudo rsync -aHAX "$ROOTFS/" "$MNT/"

# fstab + kernel cmdline
sudo tee "$MNT/etc/fstab" <<EOF
/dev/vda1 / ext4 defaults,noatime,errors=remount-ro 0 1
EOF

KERNEL="$(basename "$(ls "$MNT"/boot/vmlinuz-* | sort -V | tail -1)")"
INITRD="$(basename "$(ls "$MNT"/boot/initrd.img-* | sort -V | tail -1)")"
echo "kernel: $KERNEL  initrd: $INITRD"

sudo grub-install --target=i386-pc \
  --boot-directory="$MNT/boot" \
  --modules="part_msdos ext2" \
  "$LOOP"

sudo tee "$MNT/boot/grub/grub.cfg" <<EOF
set default="0"
set timeout=2
insmod part_msdos
insmod ext2

menuentry "CosmosOS" {
    linux /boot/$KERNEL root=/dev/vda1 rw console=tty0 console=ttyS0,115200 systemd.journald.forward_to_console=1
    initrd /boot/$INITRD
}
EOF

sudo umount -R "$MNT"
sudo losetup -d "$LOOP"
LOOP=""
trap - EXIT

echo "== done: $IMG ($(du -h "$IMG" | cut -f1) real, ${IMG_SIZE} sparse) =="
