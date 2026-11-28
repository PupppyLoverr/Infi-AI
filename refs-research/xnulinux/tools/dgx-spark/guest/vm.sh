#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# run a command in the guest (or -s FILE: a script; -put/-get: copy)
O="-o Port=2222 -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o ConnectTimeout=10 -o ServerAliveInterval=30 -o LogLevel=ERROR -i $HOME/xnuvm/vmkey"
if [ "$1" = -s ]; then exec ssh $O admin@127.0.0.1 bash -s < "$2"; fi
if [ "$1" = -put ]; then exec scp -O -P 2222 ${O} -r "$2" admin@127.0.0.1:"$3"; fi
if [ "$1" = -get ]; then exec scp -O -P 2222 ${O} -r admin@127.0.0.1:"$2" "$3"; fi
exec ssh $O admin@127.0.0.1 "$@"
