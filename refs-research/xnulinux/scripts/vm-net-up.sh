#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# In the VM, after fki.ko is loaded: the host side of the hosted macOS's en0. fki.ko's netdev xnu0 carries en0's
# frames; Linux routes and NATs 192.168.77.0/24 out of its own uplink, and dnsmasq (when installed) answers DHCP and
# DNS on xnu0. The hosted system: en0 192.168.77.2/24, router and DNS 192.168.77.1.
set -u
ip link show xnu0 >/dev/null 2>&1 || { echo "vm-net-up: no xnu0 (fki.ko without the link)"; exit 0; }
sudo ip addr replace 192.168.77.1/24 dev xnu0
sudo ip link set xnu0 up
sudo sysctl -q -w net.ipv4.ip_forward=1
# the default route's interface: the word after "dev", wherever it is ("default dev wg0 scope link" has no gateway before
# it, and a multipath route names its interfaces on the next-hop lines)
up=$(ip route show default | awk '{ for (i = 1; i < NF; i++) if ($i == "dev") { print $(i + 1); exit } }')
if [ -n "$up" ]; then
  sudo iptables -t nat -C POSTROUTING -s 192.168.77.0/24 -o "$up" -j MASQUERADE 2>/dev/null ||
    sudo iptables -t nat -A POSTROUTING -s 192.168.77.0/24 -o "$up" -j MASQUERADE
fi
sudo iptables -C FORWARD -i xnu0 -j ACCEPT 2>/dev/null || sudo iptables -I FORWARD -i xnu0 -j ACCEPT
sudo iptables -C FORWARD -o xnu0 -m state --state RELATED,ESTABLISHED -j ACCEPT 2>/dev/null ||
  sudo iptables -I FORWARD -o xnu0 -m state --state RELATED,ESTABLISHED -j ACCEPT
# The way out can have a smaller MTU than en0's 1500 (Google Cloud: 1460). Without this the hosted system announces an MSS
# of 1460, the far end's full-size segments do not fit, and a TLS handshake with a large reply (github.com, npm) never ends.
sudo iptables -t mangle -C FORWARD -p tcp --tcp-flags SYN,RST SYN -j TCPMSS --clamp-mss-to-pmtu 2>/dev/null ||
  sudo iptables -t mangle -A FORWARD -p tcp --tcp-flags SYN,RST SYN -j TCPMSS --clamp-mss-to-pmtu
# An uplink with hardware GRO (gVNIC on Google Cloud) hands Linux packets it has marked for segmentation. xnu0 takes one
# frame at a time, so Linux segments them again on the way to it, and for some of them __skb_gso_segment returns -EINVAL:
# validate_xmit_skb drops the packet (xnu0's tx_dropped) and every retransmission of it, and a download into the hosted
# system stalls after its first packets. With hardware GRO off on the uplink they arrive as they were sent.
if command -v ethtool >/dev/null && ethtool -k "$up" 2>/dev/null | grep -q '^rx-gro-hw: on'; then
  sudo ethtool -K "$up" rx-gro-hw off && echo "vm-net-up: hardware GRO off on $up (its packets cannot be forwarded to xnu0)"
fi
command -v dnsmasq >/dev/null || sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -q dnsmasq-base >/dev/null 2>&1
D=no
if command -v dnsmasq >/dev/null; then
  # pkill does not wait for the one it ended, which holds 192.168.77.1's ports a moment longer. A dnsmasq started in that
  # moment fails ("Address already in use") and then there is none: every second start of the hosted system had no DHCP
  # and no DNS. Waited for by process id, taken before the kill: a process that is ending has no command line to be
  # found by any more while it still has its sockets.
  OLD=$(pgrep -f 'dnsmasq.*xnu0' || true)
  sudo pkill -f 'dnsmasq.*xnu0' 2>/dev/null
  for _ in $(seq 1 50); do
    left=; for p in $OLD; do ! sudo kill -0 "$p" 2>/dev/null || left=1; done
    [ -n "$left" ] || break; sleep 0.1
  done
  if sudo dnsmasq --interface=xnu0 --bind-interfaces --except-interface=lo --dhcp-range=192.168.77.2,192.168.77.50,12h \
    --dhcp-option=3,192.168.77.1 --dhcp-option=6,192.168.77.1 --pid-file=/run/dnsmasq-xnu0.pid
  then D=yes; else D="DID NOT START (the hosted system gets no address and no DNS)"; fi
fi
[ -n "$up" ] && N="via $up" || N="none (this host has no default route)"
echo "vm-net-up: xnu0 192.168.77.1/24 up, NAT $N, dnsmasq: $D"
