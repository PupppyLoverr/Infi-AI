#!/usr/bin/env bash
# guest-type.sh — type a string into the guest via HMP sendkey (keyboard works
# fine headless; only pointer needs QMP). RUN=1 appends Return at the end.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MON="${MON_SOCK:-$ROOT/work/image/monitor.sock}"

declare -A K=(
  [" "]="spc" [":"]="shift-semicolon" [";"]="semicolon" [","]="comma"
  ["'"]="apostrophe" ['"']="shift-apostrophe" ["{"]="shift-bracket_left"
  ["}"]="shift-bracket_right" ["["]="bracket_left" ["]"]="bracket_right"
  ["|"]="shift-backslash" ["\\"]="backslash" ["-"]="minus" ["_"]="shift-minus"
  ["="]="equal" ["+"]="shift-equal" ["."]="dot" ["/"]="slash"
  ['`']="grave_accent" ["$"]="shift-4" ["%"]="shift-5" ["^"]="shift-6"
  ["&"]="shift-7" ["*"]="shift-8" ["("]="shift-9" [")"]="shift-0"
  ["!"]="shift-1" ["@"]="shift-2" ["#"]="shift-3" ["?"]="shift-slash"
  [">"]="shift-dot" ["<"]="shift-comma" ["~"]="shift-grave_accent"
)
text="$1"
for (( i=0; i<${#text}; i++ )); do
  c="${text:i:1}"
  if [[ "$c" =~ [A-Z] ]]; then k="shift-${c,,}"; else k="${K[$c]:-$c}"; fi
  printf "sendkey %s\n" "$k" | socat -t 2 - UNIX-CONNECT:"$MON" >/dev/null 2>&1
  sleep 0.06
done
[ -n "${RUN:-}" ] && printf "sendkey ret\n" | socat -t 2 - UNIX-CONNECT:"$MON" >/dev/null 2>&1
