#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# On the macOS host: talk to the hosted macOS shell through the VM. XNU's output is read from the kernel ring buffer (dmesg,
# log_buf_len=128M) instead of the serial console: a flood of console lines through a 115200-baud console stalled
# printk with interrupts off on XNU's CPU (RCU stall). The ring buffer is read through journald (`journalctl -k`): after a
# few minutes of XNU's output `dmesg` (util-linux, /dev/kmsg) stopped showing new records while journald kept them.
# Usage: vm-dmesg-shell.sh mark | send 'cmd' [secs] | since MARK
cd "$(dirname "$0")/.."
case "$1" in
  mark) bash scripts/vm.sh 'sudo journalctl -k -n1 -q -o short-monotonic | sed "s/^\[ *\([0-9.]*\)\].*/\1/"';;
  send) printf '%s\n' "$2" | timeout 30 bash scripts/vm.sh 'cat >> /var/tmp/console-in'; sleep "${3:-10}";;
  since) bash scripts/vm.sh 'sudo journalctl -k -b -q -o short-monotonic --no-pager' | awk -v m="$2" 'substr($0,2,12)+0 > m+0';;
esac
