#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# console-send-file.sh LOCAL REMOTE: types LOCAL into the hosted macOS console shell as short base64 lines (one every
# 0.3 s, so the tty input queue never overflows: one long typed line gets mangled by the line editor), decoded to REMOTE.
P=$(cd "$(dirname "$0")/.." && pwd)	# no cd there: LOCAL is a path of the caller's
f=$1; r=$2
[ -f "$f" ] || { echo "$f: no such file" >&2; exit 1; }	# before anything is typed: the lines below remove REMOTE
{ echo "rm -f $r.b64"
  base64 < "$f" | tr -d '\n' | fold -w 100 | while read -r l || [ -n "$l" ]; do echo "echo $l >> $r.b64"; done
  echo "base64 -D -i $r.b64 -o $r"
} | bash "$P/scripts/vm.sh" 'while IFS= read -r l; do printf "%s\n" "$l" >> /var/tmp/console-in; sleep 0.3; done'
