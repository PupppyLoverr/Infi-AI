#!/usr/bin/env bash
# guest-type.sh — type a string into the guest via QMP input-send-event on the
# virtio keyboard (HMP sendkey is dropped by QEMU under -display none).
# RUN=1 appends Return at the end.
set -euo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec "$DIR/qkey.sh" type "$1"
