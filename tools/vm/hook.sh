#!/bin/bash
# Root-side watchdog for one CI boot: if the session has not produced its
# first screenshot after 150 s, send back why (Hyprland/Quickshell logs and
# the journal), then power off instead of idling to the timeout.
H=http://10.0.2.2:8088
up() { [ -f "$1" ] && curl -fsS -T "$1" "$H/$2" >/dev/null 2>&1; }
# Hyprland 0.57 reads ~/.config/hypr/hyprland.lua. Install ours and the
# test scenario before autologin starts it.
U=/home/solyra
# The owner is in Australian Eastern Standard Time (UTC+10, no DST):
# screenshots show their local time.
ln -sf /usr/share/zoneinfo/Australia/Brisbane /etc/localtime
install -Dm644 -o solyra -g solyra /opt/solyra/hypr/hyprland.lua $U/.config/hypr/hyprland.lua
printf 'hl.on("hyprland.start", function() hl.exec_cmd("bash /opt/solyra/scenario.sh > /tmp/scenario.log 2>&1") end)\n' > $U/.config/solyra/hypr-local.lua
chown solyra: $U/.config/solyra/hypr-local.lua
echo '[ "$(tty)" = /dev/tty1 ] && exec $(command -v start-hyprland || echo Hyprland) > /tmp/hypr.log 2>&1' > $U/.bash_profile
chown solyra: $U/.bash_profile
Hyprland --verify-config -c $U/.config/hypr/hyprland.lua > /tmp/hypr-verify.txt 2>&1
up /tmp/hypr-verify.txt hypr-verify.txt
{
  echo "=== Hyprland --version"; Hyprland --version 2>&1 | head -5
  echo "=== Hyprland --help"; Hyprland --help 2>&1 | head -40
  echo "=== /usr/share/hypr"; find /usr/share/hypr /usr/share/hyprland* -maxdepth 3 2>/dev/null | head -50
  echo "=== config strings"; strings "$(command -v Hyprland)" 2>/dev/null | grep -iE "hyprland\.(conf|lua)|legacy|hyprlang" | sort -u | head -40
} > /tmp/hypr-info.txt 2>&1
up /tmp/hypr-info.txt hypr-info.txt
for f in $(find /usr/share/hypr /usr/share/hyprland* -name '*.lua' 2>/dev/null | head -5); do up "$f" "example-$(basename $f)"; done
sleep 150
if [ ! -f /tmp/01-desktop-idle-notch.png ]; then
  echo "=== solyra hook: no session after 150 s ==="
  ls -la /tmp
  tail -60 /tmp/hypr.log 2>/dev/null
  journalctl -b --no-pager -p warning | tail -60 > /tmp/journal.txt; cat /tmp/journal.txt
  ls -la /dev/dri 2>&1
  up /tmp/hypr.log log-hypr.log; up /tmp/qs.log log-qs.log; up /tmp/journal.txt journal.txt
  for f in /home/solyra/.local/share/hyprland/hyprland.log /run/user/*/hypr/*/hyprland.log; do up "$f" hyprland-full.log; done
  up $U/.config/hypr/hyprland.lua generated-hyprland.lua
  sync; poweroff
fi
# Hard stop: never run past 12 minutes.
sleep 570; [ -f /tmp/solyra-done ] || { up /tmp/qs.log log-qs.log; up /tmp/hypr.log log-hypr.log; up /tmp/scenario.log log-scenario.log; sync; poweroff; }
