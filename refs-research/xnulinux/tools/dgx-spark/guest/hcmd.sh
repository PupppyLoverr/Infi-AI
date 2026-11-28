#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# hcmd.sh [TIMEOUT] < commands : run commands in the hosted macOS's console shell, output returned over the xnu0 link (not dmesg)
T=${1:-60}; mkdir -p /var/tmp/www; cat > /var/tmp/www/cmd.sh
# the hosted shell fetches the commands from 192.168.77.1:8099: served here for this run (a server already there keeps the port)
(timeout $T python3 -m http.server 8099 --bind 192.168.77.1 --directory /var/tmp/www >/dev/null 2>&1 &)
rm -f /var/tmp/hout; (timeout $T nc -l 192.168.77.1 8100 > /var/tmp/hout 2>/dev/null &)
sleep 0.5
# (-f and &&: commands that could not be fetched are not replaced by the ones an earlier run left in /tmp/cmd.sh)
printf '\n{ curl -sf -o /tmp/cmd.sh http://192.168.77.1:8099/cmd.sh && bash /tmp/cmd.sh || echo "hcmd: the commands could not be fetched or failed"; } 2>&1 | nc 192.168.77.1 8100\n' >> /var/tmp/console-in
while pgrep -f "nc -l 192.168.77.1 8100" >/dev/null; do sleep 1; done
cat /var/tmp/hout
