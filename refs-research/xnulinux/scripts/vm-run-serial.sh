#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# On the Mac host: run the xnupoc VM with its console (hvc0) captured to ../serial-cap.log.
cd "$(dirname "$0")/.."
rm -f ../serial-name
python3 scripts/serial-pty.py ../serial-name ../serial-cap.log &
for i in $(seq 1 50); do [ -s ../serial-name ] && break; sleep 0.1; done
exec tart run xnupoc --no-graphics --serial-path "$(cat ../serial-name)"
