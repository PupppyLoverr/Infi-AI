#!/usr/bin/env bash
# run.sh — boot a CosmosOS raw image in QEMU.
#
#   -enable-kvm           hardware accel (falls back to TCG if no /dev/kvm)
#   -m 1G -smp 2          per spec
#   -device virtio-vga    virtio-gpu: guest gets /dev/dri/card0, compositor
#                         uses DRM/KMS + GBM + EGL (llvmpipe software GL)
#   -device virtio-tablet-pci   absolute-position pointer (better than PS/2)
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

# default qemu64 lacks AVX — some binaries (e.g. Bun-compiled opencode)
# misbehave on it. CPU_MODEL=host exposes the real CPU feature set.
CPU=()
[ -n "${CPU_MODEL:-}" ] && CPU=(-cpu "$CPU_MODEL")

case "$DISPLAY_MODE" in
  none)   DISP=(-display none) ;;
  sdl)    DISP=(-display sdl) ;;
  gtk)    DISP=(-display gtk) ;;
  *)      DISP=(-display "$DISPLAY_MODE") ;;
esac

exec qemu-system-x86_64 \
  "${KVM[@]}" \
  "${CPU[@]}" \
  -m 1G -smp 2 \
  -drive file="$IMG",format=raw,if=virtio \
  -device virtio-vga \
  -device virtio-tablet-pci \
  -audiodev none,id=snd0 -device intel-hda -device hda-duplex,audiodev=snd0 \
  -netdev user,id=n0 -device virtio-net-pci,netdev=n0 \
  -serial file:"$SERIAL_LOG" \
  -monitor unix:"$MON_SOCK",server,nowait \
  -qmp unix:"$QMP_SOCK",server,nowait \
  "${DISP[@]}" \
  "$@"
