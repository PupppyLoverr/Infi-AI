#!/bin/bash
# Every later boot: wait for the Plasma session, 5 minutes idle, then report
# on the serial console. Stock first, then trim and reboot, then trimmed.
stage=$(cat /var/lib/measure/stage)
proc=$(cat /var/lib/measure/proc 2>/dev/null || echo plasmashell)
for i in $(seq 240); do pgrep -x "$proc" >/dev/null && break; sleep 5; done
up=$(cut -d' ' -f1 /proc/uptime)
sleep 300
{
echo "=== MEASURE $stage ==="
echo "${proc}_seen_at_uptime_s=$up"
pgrep -a -x "$proc" || echo "NO $proc"
pgrep -a -x kwin_wayland; pgrep -a -x Hyprland; tail -3 /tmp/hypr.log 2>/dev/null
free -m
systemd-analyze 2>&1 | head -2
echo "packages=$(pacman -Qq | wc -l)"
echo "root_used_mb=$(df -m --output=used / | tail -1)"
echo "usr_mb=$(du -sxm /usr | cut -f1)"
echo "--- top RSS (KiB) ---"
ps -eo rss,comm --sort=-rss | head -25
echo "--- total RSS of all processes (KiB): $(ps -eo rss= | awk '{s+=$1} END{print s}')"
echo "=== END $stage ==="
} > /dev/ttyS0 2>&1
if [ "$stage" = stock ]; then
  echo trimmed > /var/lib/measure/stage
  bash /root/trim.sh > /dev/ttyS0 2>&1
  reboot
else
  poweroff
fi
