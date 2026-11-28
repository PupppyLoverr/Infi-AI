# SPDX-License-Identifier: GPL-2.0-only
# Sourced by tests/scripts/*.test.sh: a sandbox to run the repository's shell scripts in as they are, off the machines
# they are written for (no root, no VM, no Mac, nothing written outside a temporary directory).
#   $T         the temporary directory; at exit every process still running out of it is ended and it is removed
#   $HOME      $T/home
#   $T/repo    sandbox_repo: a copy of scripts/ (and what else is named), so a script's own "../" is $T
#   $T/bin     first in PATH. `stub NAME <<'EOF' ... EOF` puts a stand-in for a command there (bash; "$@" are the
#              caller's arguments, $T and $VM are set); each call of one is a line "NAME ARGUMENTS" in $T/calls
#   $VM        the stand-in VM or host: stub_ssh makes `ssh [OPTIONS] HOST COMMAND` run COMMAND there, its /var/tmp
#              being $VM/var/tmp and $T/vmbin first in its PATH (`vmstub NAME <<'EOF' ... EOF`)
#   check 'what must hold' COMMAND...    one check; `finish` prints the count and ends the test with its status
# Written for /bin/bash as macOS has it (3.2) as well as Linux's.
set -u
REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
T=$(mktemp -d) && T=$(cd "$T" && pwd -P) && [ -n "$T" ] || { echo "tests/scripts/lib.sh: no temporary directory" >&2; exit 1; }
VM=$T/vm; HOME=$T/home; PATH=$T/bin:$PATH; export HOME PATH
mkdir -p "$T/bin" "$T/vmbin" "$VM/var/tmp" "$HOME"; : > "$T/calls"
trap 'pkill -f "$T/" 2>/dev/null; rm -rf "$T"' EXIT
CHECKS=0; FAILED=0

stub() {	# stub NAME [DIRECTORY] < its body
  { printf '#!/bin/bash\nexport T=%q VM=%q\necho "%s $*" >> "$T/calls"\n' "$T" "$VM" "$1"; cat; } > "${2:-$T/bin}/$1"
  chmod +x "${2:-$T/bin}/$1"
}
vmstub() { stub "$1" "$T/vmbin"; }
stub_ssh() {
  stub ssh <<'EOF'
while [ $# -gt 0 ]; do case $1 in -[oipF]) shift 2;; -*) shift;; *) break;; esac; done
shift	# the host
cd "$VM" && PATH=$T/vmbin:$PATH exec /bin/bash -c "${*//\/var\/tmp\//$VM/var/tmp/}"
EOF
}
sandbox_repo() {	# sandbox_repo [PATH...]: $T/repo with scripts/ and the paths named, as the repository has them
  local p; mkdir -p "$T/repo"
  for p in scripts "$@"; do mkdir -p "$T/repo/$(dirname "$p")"; cp -Rp "$REPO/$p" "$T/repo/$p"; done
}
called() { grep -E -- "$1" "$T/calls" >/dev/null; }	# called 'PATTERN': a stand-in was called so
not() { ! "$@"; }
wait_for() {	# wait_for SECONDS COMMAND...: until COMMAND succeeds; fails when it has not after SECONDS
  local n=$(($1 * 10)); shift
  while ! "$@" >/dev/null 2>&1; do n=$((n - 1)); [ $n -gt 0 ] || return 1; sleep 0.1; done
}
within() {	# within SECONDS COMMAND...: COMMAND's status, or 124 when it had to be stopped after SECONDS
  local n=$(($1 * 10)) p; shift
  "$@" & p=$!
  while kill -0 $p 2>/dev/null; do n=$((n - 1)); [ $n -gt 0 ] || { kill $p 2>/dev/null; wait $p 2>/dev/null; return 124; }; sleep 0.1; done
  wait $p
}
check() {
  local what=$1; shift; CHECKS=$((CHECKS + 1))
  if "$@"; then echo "ok    $what"; else echo "FAIL  $what"; FAILED=$((FAILED + 1)); fi
}
finish() { echo "$((CHECKS - FAILED)) of $CHECKS checks passed"; [ $FAILED = 0 ]; exit; }
