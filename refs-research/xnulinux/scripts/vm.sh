#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# On the macOS host: run a command (or a script with -s FILE) in the xnupoc VM.
W="$(cd "$(dirname "$0")/../.." && pwd)"   # the session work dir (vmkey, vm.ip)
# nothing in the repository makes these two: say so here, not as "cat: .../vm.ip: No such file or directory"
[ -s "$W/vm.ip" ] && [ -e "$W/vmkey" ] || { echo "scripts/vm.sh: needs $W/vm.ip (the Linux VM's address: \`tart ip xnupoc\`; under scripts/krun-vm.sh 127.0.0.1, with its SSH_PORT, 2223, in $W/vm.port) and $W/vmkey (an ssh key the VM's admin user accepts)" >&2; exit 1; }
ip=$(cat "$W/vm.ip"); port=$(cat "$W/vm.port" 2>/dev/null || echo 22)	# vm.port: 2223 when the VM runs under krunkit (gvproxy)
O=(-o Port=$port -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o ConnectTimeout=10 -o ServerAliveInterval=30 -o LogLevel=ERROR -i "$W/vmkey")
if [ "$1" = -s ]; then exec ssh "${O[@]}" admin@$ip bash -s < "$2"; fi
if [ "$1" = -put ]; then exec scp -O "${O[@]}" -r "$2" admin@$ip:"$3"; fi
if [ "$1" = -get ]; then exec scp -O "${O[@]}" -r admin@$ip:"$2" "$3"; fi
exec ssh "${O[@]}" admin@$ip "$@"
