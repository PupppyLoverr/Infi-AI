#!/usr/bin/env bash
# run.sh — boot a CosmosOS raw image in QEMU.
#
#   -enable-kvm           hardware accel (falls back to TCG if no /dev/kvm)
#   -m 1G -smp 2          per spec
#   -device virtio-vga    virtio-gpu: guest gets /dev/dri/card0, compositor
#                         uses DRM/KMS + GBM + EGL (llvmpipe software GL)
#   -device virtio-tablet-pci   absolute-position pointer (better than PS/2)
#   -device virtio-keyboard-pci  real keyboard for input-send-event qcode keys
#                         (HMP sendkey is dropped by QEMU under -display none)
#   -device intel-hda + hda-duplex  HDA sound card (snd_hda_intel in guest).
#                               virtio-snd-pci would be nicer but needs
#                               QEMU >= 7 — Ubuntu 22.04 ships 6.2, which
#                               doesn't know the model. audiodev `none`
#                               discards samples on the host; swap for
#                               pa/alsa on a desktop host.
#   -netdev user + virtio-net   QEMU user-mode net, DHCP via NetworkManager
#   -serial file:...      kernel+init+session output lands in the serial log
#                         (kernel cmdline has console=ttyS0 last +
#                         journald.forward_to_console; the session wrapper
#                         logs via logger -> journald -> console)
#   -monitor unix:...     QEMU monitor socket — grab a framebuffer dump any
#                         time:  echo screendump /tmp/shot.ppm | socat - UNIX-CONNECT:work/image/monitor.sock
#   -qmp unix:...         QMP socket — input-send-event works headless (HMP
#                         mouse_move/mouse_button are dead with -display none):
#                           printf '{"execute":"qmp_capabilities"}\n{"execute":"input-send-event","arguments":{"events":[{"type":"abs","data":{"axis":"x","value":16383}},{"type":"abs","data":{"axis":"y","value":16383}}]}}\n' | socat -t 2 - UNIX-CONNECT:work/image/qmp.sock
#
# Display: default SDL window on the host desktop (:0). For headless CI-ish
# runs use  DISPLAY_MODE=none ./run.sh  — the GPU device still exists in the
# guest, the compositor still renders, and screendump still works.
#
# Resolution: GUEST_RES=WxH (default 1024x768) is requested via
# virtio-vga xres/yres — the guest's EDID then advertises that mode as
# preferred and virtio_gpu modesetting picks it up. e.g.
#   GUEST_RES=1280x720 ./run.sh
# Verify in-guest: cat /sys/class/drm/card0-Virtual-1/modes | head -1
#
# -smbios type=1,product=CosmosOS   DMI product name — makes tools that
#   report the machine (fastfetch 'Host:', hostnamectl) say CosmosOS
#   instead of the QEMU board name (pc-i440fx-jammy).
#
# Firmware: the image is UEFI-only (GPT + FAT32 ESP + GRUB2-EFI). OVMF
# runs as a pflash pair — readonly code.fd plus a writable vars.fd kept
# per-image in work/image/OVMF_VARS.fd so NVRAM (efibootmgr entries)
# persists across boots of the same image. Secure boot is OFF with the
# plain OVMF_CODE build. FIRMWARE=ovmf is the only supported value.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORK="$ROOT/work/image"
IMG="${IMG:-$ROOT/dist/cosmosos-x86_64.raw}"
SERIAL_LOG="$WORK/serial.log"
MON_SOCK="$WORK/monitor.sock"
QMP_SOCK="$WORK/qmp.sock"
DISPLAY_MODE="${DISPLAY_MODE:-sdl}"

[ -f "$IMG" ] || { echo "no image at $IMG — run build.sh first" >&2; exit 1; }
mkdir -p "$WORK"
: > "$SERIAL_LOG"

KVM=(-enable-kvm)
[ -e /dev/kvm ] || { echo "note: /dev/kvm missing — TCG emulation"; KVM=(); }

# Under KVM pass the host CPU through, so guests see the same feature set
# (AVX2 etc.) as real hardware. qemu64 lacks it, and Bun-compiled opencode
# crashes there. CPU_MODEL overrides, e.g. CPU_MODEL=qemu64 to test old CPUs.
CPU_MODEL="${CPU_MODEL:-$([ ${#KVM[@]} -gt 0 ] && echo host)}"
CPU=()
[ -n "$CPU_MODEL" ] && CPU=(-cpu "$CPU_MODEL")

# GUEST_MEM overrides the 1G spec memory — e.g. opencode's Bun runtime wants
# ~1G by itself, so drive tests that exercise it should use GUEST_MEM=2G.
MEM="${GUEST_MEM:-1G}"

case "$DISPLAY_MODE" in
  none)   DISP=(-display none) ;;
  sdl)    DISP=(-display sdl) ;;
  gtk)    DISP=(-display gtk) ;;
  *)      DISP=(-display "$DISPLAY_MODE") ;;
esac

# GUEST_RES=WxH — advertised as the preferred EDID mode (see header note).
# Default keeps 1024x768 so drive-harness click coordinates stay valid.
GUEST_RES="${GUEST_RES:-1024x768}"
RES_W="${GUEST_RES%%x*}" RES_H="${GUEST_RES##*x}"

# OVMF pflash: code.fd readonly + per-image writable vars.fd (NVRAM).
# Prefer the 4M builds when present (Ubuntu ships both under OVMF/).
OVMF_CODE="" OVMF_VARS=""
for suf in _4M ""; do
  [ -f "/usr/share/OVMF/OVMF_CODE${suf}.fd" ] && [ -f "/usr/share/OVMF/OVMF_VARS${suf}.fd" ] || continue
  OVMF_CODE="/usr/share/OVMF/OVMF_CODE${suf}.fd"
  OVMF_VARS="/usr/share/OVMF/OVMF_VARS${suf}.fd"
  break
done
[ -n "$OVMF_CODE" ] || { echo "no OVMF firmware found — apt install ovmf" >&2; exit 1; }
VARS="$WORK/OVMF_VARS.fd"
[ -f "$VARS" ] || cp "$OVMF_VARS" "$VARS"

exec qemu-system-x86_64 \
  -drive if=pflash,format=raw,readonly=on,file="$OVMF_CODE" \
  -drive if=pflash,format=raw,file="$VARS" \
  "${KVM[@]}" \
  "${CPU[@]}" \
  -m "$MEM" -smp 2 \
  -drive file="$IMG",format=raw,if=virtio \
  -device virtio-vga,xres="$RES_W",yres="$RES_H" \
  -device virtio-tablet-pci \
  -device virtio-keyboard-pci \
  -audiodev none,id=snd0 -device intel-hda -device hda-duplex,audiodev=snd0 \
  -netdev user,id=n0 -device virtio-net-pci,netdev=n0 \
  -smbios type=1,product=CosmosOS \
  -serial file:"$SERIAL_LOG" \
  -monitor unix:"$MON_SOCK",server,nowait \
  -qmp unix:"$QMP_SOCK",server,nowait \
  "${DISP[@]}" \
  "$@"
