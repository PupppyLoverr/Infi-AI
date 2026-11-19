#!/usr/bin/env bash
# Boot a Solyra disk image in QEMU.
#
#   ./scripts/run-vm.sh solyra.raw          # or solyra.raw.zst (decompressed next to it)
#   SOLYRA_RAM=6144 SOLYRA_CPUS=4 ./scripts/run-vm.sh solyra.raw
#   SOLYRA_DISPLAY=sdl ./scripts/run-vm.sh solyra.raw
#
# The compositor (niri) refuses software rendering, so the guest needs a 3D
# virtual GPU: this uses virtio-vga-gl (VirGL), which needs a QEMU built with
# OpenGL and a host GPU/driver that supports it. Without KVM it still boots,
# but slowly.
#
# The development disk logs in automatically as "solyra" (password "solyra"
# unless it was built with another SOLYRA_DEV_PASSWORD).
set -euo pipefail

DISK="${1:-}"
if [[ -z "$DISK" || ! -f "$DISK" ]]; then
  echo "usage: $0 <solyra.raw | solyra.raw.zst>" >&2
  exit 2
fi
if [[ "$DISK" == *.zst ]]; then
  command -v zstd >/dev/null || { echo "zstd is needed to unpack $DISK" >&2; exit 1; }
  RAW="${DISK%.zst}"
  if [[ ! -f "$RAW" ]]; then
    echo "unpacking $DISK -> $RAW (about 8 GiB, sparse)"
    zstd -d --sparse -o "$RAW" "$DISK"
  fi
  DISK="$RAW"
fi

QEMU="${QEMU:-qemu-system-x86_64}"
command -v "$QEMU" >/dev/null || { echo "$QEMU not found (install QEMU)" >&2; exit 1; }

find_first() { for f in "$@"; do [[ -f "$f" ]] && { echo "$f"; return 0; }; done; return 1; }
OVMF_CODE="${OVMF_CODE:-$(find_first \
  /usr/share/OVMF/OVMF_CODE_4M.fd /usr/share/OVMF/OVMF_CODE.fd \
  /usr/share/edk2/ovmf/OVMF_CODE.fd /usr/share/edk2-ovmf/x64/OVMF_CODE.fd \
  /usr/share/qemu/edk2-x86_64-code.fd /opt/homebrew/share/qemu/edk2-x86_64-code.fd || true)}"
OVMF_VARS_SRC="${OVMF_VARS_SRC:-$(find_first \
  /usr/share/OVMF/OVMF_VARS_4M.fd /usr/share/OVMF/OVMF_VARS.fd \
  /usr/share/edk2/ovmf/OVMF_VARS.fd /usr/share/edk2-ovmf/x64/OVMF_VARS.fd || true)}"
if [[ -z "$OVMF_CODE" ]]; then
  echo "UEFI firmware (OVMF) not found. Install it (apt: ovmf, dnf: edk2-ovmf) or set OVMF_CODE." >&2
  exit 1
fi

ARGS=(-m "${SOLYRA_RAM:-4096}" -smp "${SOLYRA_CPUS:-4}")
if [[ -w /dev/kvm ]]; then
  ARGS+=(-accel kvm -cpu host)
else
  echo "note: no KVM, using software emulation (slow)" >&2
  ARGS+=(-accel "tcg,thread=multi" -cpu max)
fi

if [[ -n "$OVMF_VARS_SRC" ]]; then
  VARS="${DISK}.OVMF_VARS.fd"
  [[ -f "$VARS" ]] || cp "$OVMF_VARS_SRC" "$VARS"
  ARGS+=(-drive "if=pflash,format=raw,readonly=on,file=$OVMF_CODE"
         -drive "if=pflash,format=raw,file=$VARS")
else
  ARGS+=(-bios "$OVMF_CODE")
fi

case "${SOLYRA_DISPLAY:-gtk}" in
  gtk) DISPLAY_ARGS=(-display "gtk,gl=on") ;;
  sdl) DISPLAY_ARGS=(-display "sdl,gl=on") ;;
  *)   DISPLAY_ARGS=(-display "${SOLYRA_DISPLAY}") ;;
esac

exec "$QEMU" "${ARGS[@]}" \
  -vga none -device virtio-vga-gl "${DISPLAY_ARGS[@]}" \
  -device qemu-xhci -device usb-tablet -device usb-kbd \
  -device virtio-rng-pci \
  -netdev user,id=n0 -device virtio-net-pci,netdev=n0 \
  -drive "file=$DISK,format=raw,if=virtio" \
  -serial "${SOLYRA_SERIAL:-none}"
