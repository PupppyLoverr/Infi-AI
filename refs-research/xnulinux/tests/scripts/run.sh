#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# The tests of the repository's shell scripts: every tests/scripts/*.test.sh (each can be run by itself). A test runs
# the script as it is in a sandbox of stand-in commands (lib.sh), on Linux or macOS, as any user, in a few seconds.
cd "$(dirname "$0")" || exit 1
s=0
for t in *.test.sh; do echo "== $t"; /bin/bash "$t" || s=1; done
exit $s
