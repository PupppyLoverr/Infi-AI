#!/bin/bash
# Every boot, before autologin: fetch this CI run's build from the host and
# install it, then power off once the scenario signals it is done.
H=http://10.0.2.2:8088
for i in $(seq 30); do curl -fsS $H/payload.tar -o /tmp/payload.tar && break; sleep 2; done
mkdir -p /opt/solyra && tar -xf /tmp/payload.tar -C /opt/solyra
install -m755 /opt/solyra/bin/* /usr/local/bin/
install -Dm644 /opt/solyra/share/applications/*.desktop -t /usr/share/applications/
U=/home/solyra
mkdir -p $U/.config/quickshell $U/.config/hypr $U/.config/solyra $U/Downloads $U/Documents $U/Pictures $U/Music
cp -r /opt/solyra/qs/. $U/.config/quickshell/
cp /opt/solyra/hypr/hyprland.conf $U/.config/hypr/hyprland.conf
cp /opt/solyra/wallpaper.png $U/.config/solyra/wallpaper.png
echo "exec-once = bash /opt/solyra/scenario.sh > /tmp/scenario.log 2>&1" > $U/.config/solyra/hypr-local.conf
sed -i 's|^exec-once = quickshell$|exec-once = quickshell > /tmp/qs.log 2>\&1|' $U/.config/hypr/hyprland.conf
chown -R solyra: $U
echo "solyra-sync: payload installed" > /dev/ttyS0
# Per-run hook from the payload (diagnostics, watchdog): changes without
# rebuilding the cached image.
# Background work must outlive this oneshot unit (systemd kills its cgroup
# when it exits), so it runs as its own transient units.
[ -f /opt/solyra/hook.sh ] && systemd-run --unit=solyra-hook --no-block sh -c 'bash /opt/solyra/hook.sh > /dev/ttyS0 2>&1'
systemd-run --unit=solyra-done-watch --no-block sh -c 'while [ ! -f /tmp/solyra-done ]; do sleep 5; done; sync; poweroff'
