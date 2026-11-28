#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# On an arm64 Mac: how ../start.s (the test root's /sbin/launchd) judges the child it execs. Its raw BSD system calls
# (svc #0x80: open, write, fork, execve, wait4, exit) are the same in a macOS process as under the hosted XNU, so the
# source is assembled as it is and run here, with two of its strings replaced: "/dev/console" by a file, and the script
# it gives /bin/sh -c by what the child is to do. It is linked as a dynamic executable with _start as its entry (arm64
# macOS runs no static ones) and started with fds 0-2 closed, so that its three opens are 0, 1 and 2, as for pid 1.
set -u
[ "$(uname -sm)" = "Darwin arm64" ] || { echo "skip: tools/init/start.s is Darwin arm64 code, this is $(uname -sm)"; exit 0; }
D=$(cd "$(dirname "$0")/.." && pwd); W=$(mktemp -d) && W=$(cd "$W" && pwd -P) || exit 1; trap 'rm -rf "$W"' EXIT; failed=0
check() {	# check 'what the child does' STATUS: the status start.s must exit with (42: its "child exited with 7 or 0")
  sed -e "s|\"/dev/console\"|\"$W/console\"|" -e '/^Lsharg2:/{' -e n -e "s|\".*\"|\"$1\"|" -e '}' "$D/start.s" > "$W/start.s"
  grep -q "\"$W/console\"" "$W/start.s" && grep -qF "\"$1\"" "$W/start.s" || { echo "FAIL  start.s has no \"/dev/console\" or no Lsharg2 string to replace"; exit 1; }
  clang -arch arm64 -nostdlib -lSystem -e _start "$W/start.s" -o "$W/init" || exit 1
  : > "$W/console"; "$W/init" <&- >&- 2>&-; local s=$? said=no
  grep -q '^parent: child exited with 7 or 0$' "$W/console" && said=yes
  if [ "$s $said" = "$2 $([ "$2" = 42 ] && echo yes || echo no)" ]; then echo "ok    child: $1: exit status $s"
  else echo "FAIL  child: $1: exit status $s (expected $2), \"parent: child exited with 7 or 0\": $said"; failed=1; fi
}
check 'exit 0' 42
check 'exit 7' 42
check 'exit 3' 19		# 16 + its status
check 'kill -9 $$' 6	# SIGKILL: what AMFI or the code-signing check gives a binary it refuses
check 'kill -11 $$' 6	# a fault
exit $failed
