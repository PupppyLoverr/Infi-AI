#!/bin/sh
# SPDX-License-Identifier: GPL-2.0-only
# In the hosted macOS, at boot (launchd job xnulinux.network): en0 is XNU's link to the host's xnu0 (bsd/net/if_linux64.c).
# Without an IOKit interface behind it configd's IPConfiguration does not manage it, so configure it here: address, default
# route, and the DNS/IPv4 state mDNSResponder resolves through (it does not read /etc/resolv.conf).
# With XnuEthernet (kexts/XnuEthernet, the default) en0 is an IOKit interface: configd's IPConfiguration runs DHCP on it
# (the host's dnsmasq answers) and sets the route and DNS itself, so there is nothing to do here. The static set-up below is
# for the kernel's own ifnet (boot-arg linux64-ifnet=1).
for i in $(seq 1 30); do ifconfig en0 >/dev/null 2>&1 && break; sleep 1; done
if ioreg -r -c XnuEthernet -d 1 2>/dev/null | grep -q XnuEthernet; then
  echo "xnulinux: en0 is XnuEthernet; configd configures it (DHCP)" > /dev/console
  exit 0
fi
ADDR=192.168.77.2 GW=192.168.77.1 DNS=192.168.77.1
for i in $(seq 1 60); do ifconfig en0 >/dev/null 2>&1 && scutil -r $GW >/dev/null 2>&1 && break; sleep 1; done
ifconfig en0 inet $ADDR netmask 255.255.255.0 up
route -n delete default >/dev/null 2>&1; route -n add default $GW
scutil <<EOS
d.init
d.add ServerAddresses * $DNS
set State:/Network/Service/xnulinux/DNS
d.init
d.add Router $GW
d.add Addresses * $ADDR
d.add SubnetMasks * 255.255.255.0
d.add InterfaceName en0
set State:/Network/Service/xnulinux/IPv4
d.init
d.add PrimaryService xnulinux
d.add PrimaryInterface en0
d.add Router $GW
set State:/Network/Global/IPv4
EOS
echo "xnulinux: en0 $ADDR via $GW, DNS $DNS" > /dev/console
