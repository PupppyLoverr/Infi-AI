#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# scripts/vm-net-up.sh run again and again on one host, as scripts/xnulinux-boot.sh runs it at every start of the hosted
# system: after each run a dnsmasq has to be serving xnu0. The stand-in dnsmasq holds 192.168.77.1's ports from its start
# until a moment after it is told to end, and one started while they are held fails as dnsmasq does. While it is ending
# it is no longer to be found by its command line, as a real process is not once it has given up its memory and not yet
# its sockets. ip, iptables, sysctl and ethtool are stand-ins that only write down how they were called.
. "$(dirname "$0")/lib.sh"
sandbox_repo
stub sudo <<'EOF'
while [ $# -gt 0 ]; do case $1 in *=*) export "$1"; shift;; *) break;; esac; done
exec "$@"
EOF
stub ip <<'EOF'
case "$*" in "route show default") cat "$T/default-route";; esac
exit 0
EOF
stub iptables <<'EOF'
case " $* " in *" -C "*) exit 1;; esac	# no rule is there yet: the script adds it
EOF
stub sysctl < /dev/null
stub ethtool < /dev/null
stub dnsmasq <<'EOF'
mkdir "$T/port" 2>/dev/null ||
  { echo "dnsmasq: failed to create listening socket for 192.168.77.1: Address already in use" >&2; exit 2; }
( trap 'exec /bin/bash -c "sleep 0.3; rmdir \"\$0\"" "$T/port"' TERM; while :; do sleep 0.1; done ) > /dev/null 2>&1 &
echo $! > "$T/dnsmasq.pid"
EOF
echo "default via 10.0.2.2 dev eth0 proto dhcp src 10.0.2.15 metric 100" > "$T/default-route"
run() { /bin/bash "$T/repo/scripts/vm-net-up.sh" > "$T/out" 2>&1; sleep 0.8; }	# (and what was ended has had time to go)
serving() { [ -d "$T/port" ] && kill -0 "$(cat "$T/dnsmasq.pid")" 2>/dev/null; }

run; first=$(cat "$T/dnsmasq.pid")
check 'the first run starts dnsmasq on xnu0' serving
check '... and says so, and where the NAT goes' grep -q 'NAT via eth0, dnsmasq: yes$' "$T/out"
check '... with the NAT rule on the default route'"'"'s interface' called '^iptables -t nat -A POSTROUTING .* -o eth0 -j MASQUERADE'

run
check 'after a second run (the next start of the hosted system) a dnsmasq is serving' serving
check '... a new one, the first having been ended' [ "$(cat "$T/dnsmasq.pid")" != "$first" ]
check '... and none was started while the old one still held the address' not grep -q 'Address already in use' "$T/out"
run
check 'after a third run too' serving

# a dnsmasq that cannot start (something else has the address): the script must not report it as there
pkill -f "$T/bin/dnsmasq"; wait_for 3 not test -d "$T/port"; mkdir "$T/port"
run
check 'a dnsmasq that did not start is reported, not "dnsmasq: yes"' grep -q 'dnsmasq: DID NOT START' "$T/out"
rmdir "$T/port"

# a host without a default route has nothing to NAT to
: > "$T/default-route"; : > "$T/calls"
run
check 'without a default route no NAT rule is asked for (iptables takes no empty interface)' not called '^iptables -t nat'
check '... and the script says there is none' grep -q 'NAT none (this host has no default route)' "$T/out"
check '... while dnsmasq is still started' serving
finish
