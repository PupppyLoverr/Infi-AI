#!/usr/bin/env bash
# host-deps.sh — provision an Ubuntu 22.04 x86_64 host for CosmosOS work.
#
# Installs everything needed to:
#   * compile the cosmosos/cosmos Rust workspace (Smithay 0.7 compositor et al.)
#   * assemble the CosmosOS disk image (mmdebstrap, partitioning, mkfs, squashfs, ISO)
#   * boot/test the image in QEMU with KVM + OVMF (+ swtpm for TPM experiments)
#
# Idempotent: safe to re-run. Requires sudo for apt/meson-install steps.
# Verified on Ubuntu 22.04 (jammy) x86_64, kernel 6.8, /dev/kvm present.
#
# Non-apt item: Ubuntu 22.04 does NOT package libdisplay-info, which the
# compositor needs via smithay-drm-extras' `display-info` feature (EDID
# probing on the udev/DRM backend). The Rust crate requires >=0.1.0,<0.3.0,
# so we build tag 0.2.0 from freedesktop GitLab into /usr/local.
set -euo pipefail

LIBDISPLAY_INFO_VERSION="0.2.0"

# --- apt packages ------------------------------------------------------------

export DEBIAN_FRONTEND=noninteractive
sudo apt-get update -qq
sudo apt-get install -y --no-install-recommends \
  `# QEMU + firmware` \
  qemu-system-x86 qemu-utils ovmf swtpm \
  `# image assembly` \
  mmdebstrap parted dosfstools e2fsprogs squashfs-tools mtools xorriso \
  `# base tooling` \
  pkg-config git curl rsync ca-certificates \
  `# C toolchain (bindgen + C deps of Rust crates)` \
  build-essential clang libclang-dev meson ninja-build \
  `# smithay/anvil native deps: udev+drm+gbm backends, libinput, libseat, xkb` \
  libudev-dev libxkbcommon-dev libwayland-dev libdrm-dev libgbm-dev \
  libegl1-mesa-dev libgles2-mesa-dev libinput-dev libseat-dev libsystemd-dev \
  libdbus-1-dev \
  `# winit/X11 client deps` \
  libxcb1-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
  libxcb-keysyms1-dev libxkbcommon-x11-dev libfontconfig-dev libssl-dev \
  `# smithay pixman renderer links -lpixman-1 at build time` \
  libpixman-1-dev \
  `# socat: QEMU monitor socket for screendump smoke tests` \
  socat

# --- rustup (stable toolchain) ----------------------------------------------

if ! command -v rustup >/dev/null 2>&1; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --default-toolchain stable
fi
# shellcheck disable=SC1091
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
rustup default stable

# --- Debian archive keyring (trixie signatures) --------------------------------
# Ubuntu 22.04 ships debian-archive-keyring 2021.x — too old to verify
# trixie InRelease (mmdebstrap fails with NO_PUBKEY). Pull the current
# keyring .deb straight from the Debian pool when a trixie key is missing.
if ! gpg --no-default-keyring \
    --keyring /usr/share/keyrings/debian-archive-keyring.gpg \
    --list-keys 2>/dev/null | grep -q 6ED0E7B82643E131; then
  keyring_deb="$(curl -fsSL http://ftp.debian.org/debian/pool/main/d/debian-archive-keyring/ \
    | grep -oE 'debian-archive-keyring_[0-9.]+_all\.deb' | sort -uV | tail -1)"
  tmpdir="$(mktemp -d)"
  curl -fsSL "http://ftp.debian.org/debian/pool/main/d/debian-archive-keyring/$keyring_deb" \
    -o "$tmpdir/keyring.deb"
  sudo dpkg -i "$tmpdir/keyring.deb"
  rm -rf "$tmpdir"
fi

# --- libdisplay-info 0.2.0 from source (not in Ubuntu 22.04) -----------------

if ! pkg-config --exists 'libdisplay-info >= 0.1.0' 'libdisplay-info < 0.3.0'; then
  tmpdir="$(mktemp -d)"
  trap 'rm -rf "$tmpdir"' EXIT
  git clone --depth 1 --branch "$LIBDISPLAY_INFO_VERSION" \
    https://gitlab.freedesktop.org/emersion/libdisplay-info.git "$tmpdir/libdisplay-info"
  meson setup "$tmpdir/libdisplay-info/build" "$tmpdir/libdisplay-info"
  ninja -C "$tmpdir/libdisplay-info/build"
  sudo ninja -C "$tmpdir/libdisplay-info/build" install
  sudo ldconfig
  rm -rf "$tmpdir"
  trap - EXIT
fi

# --- KVM access ---------------------------------------------------------------
# /dev/kvm is root:kvm 0660 — add the invoking user to the kvm group so
# qemu -enable-kvm works without sudo (takes effect in new sessions/shells).
if [ -e /dev/kvm ] && [ -n "${SUDO_USER:-$USER}" ]; then
  sudo usermod -aG kvm "${SUDO_USER:-$USER}"
fi

# --- smoke report ------------------------------------------------------------

echo "== host-deps: done =="
command -v qemu-system-x86_64 mmdebstrap mkfs.vfat mksquashfs xorriso swtpm >/dev/null
ls /dev/kvm >/dev/null 2>&1 && echo "kvm: /dev/kvm present" || echo "kvm: MISSING (TCG fallback only)"
for p in libdisplay-info libsystemd libseat libudev xkbcommon gbm libdrm libinput; do
  printf "  %-16s %s\n" "$p" "$(pkg-config --modversion "$p")"
done
rustc --version
