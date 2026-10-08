#!/bin/sh
# papercut-check.sh — guest-side evidence dump for the image papercuts.
# Installed into the test image by inject-papercut-check.sh and run once by
# systemd at boot (papercut-check.service). Everything goes to /dev/ttyS0 so
# the host reads it verbatim from the QEMU serial log — no console typing.
#
# Covers: hostname, locale, zram swap, DRM modes (resolution), DMI product,
# motd state, sudo apt availability.
OUT=/dev/ttyS0
exec >"$OUT" 2>&1

echo "==PAPERCUT-CHECK=="
date -u '+utc=%Y-%m-%dT%H:%M:%SZ'

echo "==HOST=="
uname -n
hostnamectl --static 2>/dev/null
hostnamectl 2>/dev/null | sed -n '1,4p'

echo "==DMI=="
cat /sys/class/dmi/id/product_name 2>/dev/null
cat /sys/class/dmi/id/sys_vendor 2>/dev/null

echo "==LOCALE=="
echo "-- systemd context (expect C.UTF-8):"
locale 2>&1 | head -3
echo "-- cosmos login context (expect en_US.UTF-8):"
su -l cosmos -c 'locale' 2>&1 | head -14
echo "-- /etc/default/locale:"; cat /etc/default/locale 2>/dev/null
echo "-- /etc/locale.gen active:"; grep -v '^#' /etc/locale.gen 2>/dev/null | grep -v '^$'
echo "-- locale -a:"; locale -a 2>/dev/null | head -6

echo "==ZRAM=="
zramctl 2>&1
swapon --show 2>&1
systemctl is-active systemd-zram-setup@zram0.service 2>&1
cat /etc/systemd/zram-generator.conf 2>/dev/null

echo "==DRM=="
for f in /sys/class/drm/card*-*/modes; do
  echo "-- $f"
  head -5 "$f" 2>/dev/null
done
for f in /sys/class/drm/card*-*/status; do
  echo "$f: $(cat "$f" 2>/dev/null)"
done

echo "==MOTD=="
echo "motd bytes: $(wc -c < /etc/motd 2>/dev/null)"
echo "hushlogin: $(ls -la /home/cosmos/.hushlogin 2>/dev/null)"
echo "issue: $(cat /etc/issue 2>/dev/null | head -1)"

echo "==SUDO/APT=="
sudo -n true 2>&1 && echo "sudo: NOPASSWD ok"
command -v apt-get firefox-esr 2>/dev/null

echo "==PAPERCUT-CHECK-END=="
