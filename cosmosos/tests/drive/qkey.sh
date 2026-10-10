#!/usr/bin/env bash
# qkey.sh — send key events to the guest via QMP input-send-event on the
# virtio keyboard. Requires -device virtio-keyboard-pci in the QEMU launch.
#
# Why not HMP sendkey: sendkey only feeds the PS/2 i8042 path, and QEMU drops
# those events entirely under -display none (no display console). The virtio
# input device accepts input-send-event qcode keys headless — verified with
# od on /dev/input/eventN: AT kbd gets 0 bytes, virtio kbd gets EV_KEY.
#
#   qkey.sh ret              tap a key (qcode name — same names as sendkey)
#   qkey.sh meta_l-ret       hold modifier(s), tap last key, release
#   qkey.sh type STRING      type each char (US-layout map for punctuation)
# RUN=1 appends Return after `type`.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
QMP="${QMP_SOCK:-$ROOT/work/image/qmp.sock}"
CAPS='{"execute":"qmp_capabilities"}'

qev() { # $1=qcode $2=true|false
  printf '%s\n{"execute":"input-send-event","arguments":{"events":[{"type":"key","data":{"down":%s,"key":{"type":"qcode","data":"%s"}}}]}}\n' \
    "$CAPS" "$2" "$1" | socat -t 3 - UNIX-CONNECT:"$QMP" >/dev/null
}
tap() { qev "$1" true; sleep 0.04; qev "$1" false; sleep 0.04; }

key() { # "mod-mod-key": hold all but last, tap last, release in reverse
  local -a parts; local i last
  IFS='-' read -ra parts <<< "$1"
  last=$(( ${#parts[@]} - 1 ))
  for (( i=0; i<last; i++ )); do qev "${parts[$i]}" true; sleep 0.03; done
  tap "${parts[$last]}"
  for (( i=last-1; i>=0; i-- )); do qev "${parts[$i]}" false; sleep 0.03; done
}

type_text() { # US layout: punct needing shift goes through a shift hold
  declare -A S=(
    [":"]="semicolon" ['"']="apostrophe" ["{"]="bracket_left" ["}"]="bracket_right"
    ["|"]="backslash" ["_"]="minus" ["+"]="equal" ["?"]="slash" [">"]="dot"
    ["<"]="comma" ["~"]="grave_accent" ["$"]="4" ["%"]="5" ["^"]="6" ["&"]="7"
    ["*"]="8" ["("]="9" [")"]="0" ["!"]="1" ["@"]="2" ["#"]="3"
  )
  declare -A K=(
    [" "]="spc" [";"]="semicolon" [","]="comma" ["'"]="apostrophe"
    ["["]="bracket_left" ["]"]="bracket_right" ["\\"]="backslash"
    ["-"]="minus" ["="]="equal" ["."]="dot" ["/"]="slash" ['`']="grave_accent"
  )
  local text="$1" c i
  for (( i=0; i<${#text}; i++ )); do
    c="${text:i:1}"
    if [[ "$c" =~ [A-Z] ]]; then
      key "shift-${c,,}"
    elif [[ -n "${S[$c]:-}" ]]; then
      key "shift-${S[$c]}"
    else
      tap "${K[$c]:-$c}"
    fi
  done
  [ -n "${RUN:-}" ] && tap ret
}

case "${1:-}" in
  type) type_text "$2" ;;
  *)    key "$1" ;;
esac
# explicit success — `type` without RUN ends on a false && test otherwise
exit 0
