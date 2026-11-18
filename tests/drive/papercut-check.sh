#!/bin/sh
# papercut-check.sh — guest-side evidence dump for the image papercuts.
# Installed into the test image by inject-papercut-check.sh and run once by
# systemd at boot (papercut-check.service). Everything goes to /dev/ttyS0 so
# the host reads it verbatim from the QEMU serial log — no console typing.
#
# Covers: hostname, locale, zram swap, DRM modes (resolution), DMI product,
# motd state, sudo apt availability.
# Evidence file inside @ (survives: read it host-side via loop-mount even
# when the ttyS0 fd dies — serial-getty's vhangup steals the line ~12s in).
# Still stream to ttyS0 live; the final cat re-opens the line and dumps the
# whole file atomically.
OUT=/var/lib/cosmos/papercut-evidence.txt
mkdir -p "$(dirname "$OUT")"
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

echo "==DISK=="
lsblk -f 2>&1
echo "-- findmnt:"
findmnt -t btrfs,vfat 2>&1
findmnt -no SOURCE,FSTYPE,OPTIONS / 2>&1
echo "-- subvolumes:"
btrfs subvolume list / 2>&1
echo "-- fstab:"
cat /etc/fstab 2>/dev/null
df -h / /home /.snapshots /var/log /boot/efi 2>&1

echo "==SNAPPER=="
export TERM=vt100 COLUMNS=120
snapper list 2>&1 || true
echo "-- create probe:"
snapper -c root create --description "papercut-evidence" 2>&1 || true
snapper list 2>&1 || true
systemctl is-enabled snapper-timeline.timer snapper-cleanup.timer 2>&1
ls /etc/apt/apt.conf.d/ 2>/dev/null | grep -i snapper || echo "no apt snapper hook"

echo "==EFI=="
efibootmgr -v 2>&1 | head -15
echo "-- ESP tree:"
find /boot/efi/EFI -type f 2>/dev/null
ls -la /boot/efi/EFI/BOOT 2>/dev/null

echo "==MEM=="
grep -E 'MemTotal|MemAvailable|MemFree|SwapTotal|SwapFree' /proc/meminfo || true
free -m || true

echo "==SUDO/APT=="
sudo -n true 2>&1 && echo "sudo: NOPASSWD ok" || echo "sudo failed"
command -v apt-get 2>/dev/null || true
command -v firefox-esr 2>/dev/null || true

echo "==X11=="
# Lazy-XWayland proof: satellite runs from session start, real Xwayland must
# NOT exist until the first X11 client connects. Wait for satellite first
# (session starts it ~7-9s in; this check runs ~5s), then ps, then xeyes.
for _ in $(seq 1 30); do
  pgrep -f xwayland-satellite >/dev/null 2>&1 && break
  sleep 1
done
echo "-- ps BEFORE first X11 client:"
ps -ef | grep -iE '[x]wayland|[X]wayland' || echo "(none)"
if pgrep -x Xwayland >/dev/null 2>&1; then
  echo "FAIL: Xwayland already running before first client"
else
  echo "OK: no Xwayland process before first X11 client"
fi
command -v xeyes >/dev/null 2>&1 && {
  # DISPLAY must be explicit: it lives in cosmos-session's env, not profile.d,
  # so a su -l login shell doesn't inherit it.
  su -l cosmos -c 'DISPLAY=:0 nohup xeyes >/dev/null 2>&1 &' 2>/dev/null || true
  echo "-- xeyes spawned as cosmos (DISPLAY=:0)"
  sleep 4
  pgrep -a -x xeyes || echo "xeyes not running (spawn failed)"
  echo "-- ps AFTER first X11 client:"
  ps -ef | grep -iE '[x]wayland|[X]wayland' || echo "(none)"
  pgrep -x Xwayland >/dev/null 2>&1 && \
    echo "OK: Xwayland now running (lazy-spawned by satellite)" || \
    echo "FAIL: Xwayland still absent after xeyes"
} || echo "xeyes missing (x11-apps not installed)"

echo "==PAPERCUT-CHECK-END=="
exec 1>&2 2>/dev/null
cat "$OUT" > /dev/ttyS0 2>/dev/null || true
