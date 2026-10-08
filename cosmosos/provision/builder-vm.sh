#!/usr/bin/env bash
# builder-vm.sh — provision a Debian VM used to BUILD the cosmosos/cosmos
# workspace when you only need a compiler + the nested test rig (e.g. the
# aarch64 builder QEMU guest on Apple Silicon dev machines).
#
# For the full x86_64 build+image host use provision/host-deps.sh instead —
# it additionally installs QEMU, mmdebstrap and the compositor's udev deps.
#
# Installs:
#   * stable Rust via rustup
#   * libpixman-1-dev (cosmos-shell links pixman directly)
#   * weston + xauth + x11-utils (headless nested-host for tests/smoke-nested.sh)
#   * pkg-config/git/curl + C toolchain bits cargo needs for build scripts
#
# Idempotent; needs sudo for apt steps.
set -euo pipefail

export DEBIAN_FRONTEND=noninteractive
sudo apt-get update -qq
sudo apt-get install -y --no-install-recommends \
  build-essential pkg-config git curl rsync ca-certificates \
  libpixman-1-dev libpam0g-dev \
  weston xauth x11-utils

if ! command -v rustup >/dev/null 2>&1; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --default-toolchain stable
fi
# shellcheck disable=SC1091
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
rustup default stable
rustup component add rustfmt

echo "builder-vm.sh: done. Build with: cd cosmosos/cosmos && cargo build --workspace"
