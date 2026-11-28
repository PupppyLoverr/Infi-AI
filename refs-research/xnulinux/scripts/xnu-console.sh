#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# On the Mac host, with the VM running and XNU loaded with CONSOLE_IN=/var/tmp/console-in: an interactive terminal on
# XNU's /dev/console. Output: the "darwin console:" lines of the VM's kernel log, followed through journald as
# scripts/vm-dmesg-shell.sh reads it (scripts/xnulinux-boot.sh keeps XNU's lines off the VM console, so a captured serial
# log has none of them); input: each line typed here is appended to the console input file in the VM.
P=$(cd "$(dirname "$0")/.." && pwd)
# (the reader's ssh gets no input of its own: the first command of a background pipeline has the script's, the lines typed)
bash "$P/scripts/vm.sh" 'sudo journalctl -k -f -n 0 -q -o cat' < /dev/null | sed -un 's/^fki: darwin console: //p' &
trap 'pkill -P $$' EXIT	# the reader: its ssh as well as its sed ($! is only the sed)
while IFS= read -r line; do
  printf '%s\n' "$line" | bash "$P/scripts/vm.sh" 'cat >> /var/tmp/console-in'
done
